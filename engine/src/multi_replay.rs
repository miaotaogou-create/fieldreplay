//! 多通道对齐重放（E11c / E12）：单一相对时间轴，按 `channel_index` 扇出到各目标。
//!
//! ponytail: E12a–c sink 含 UDP/视频(UDP)、TCP、串口、MQTT；多路 RTSP listen 仍不支持。
//! 天花板是混合类型需各 sink 进同一调度器；升级路径继续往 `MultiReplayTarget` 加变体。

use std::io;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::clock::{SessionClock, WaitResult};
use crate::fcap::{ChannelType, PacketKind, SessionReader};
use crate::mqtt::{
    apply_topic_prefix, mqtt_qos_from_flags, mqtt_retain_from_flags, MqttClient, MqttConfig,
};
use crate::replay::{
    emit_with_fault, take_pending_seek, try_restart_loop, FaultInjection, FaultRng,
    OnReplayProgress,
};
use crate::serial::{SerialConfig, SerialPort};
use crate::tcp::{accept_until, connect, listen, write_all, TcpRole};
use crate::udp::{bind_sender, send_to};

/// 一路已打开的数据报目标（UDP 或视频 UDP 推送）。
pub struct DatagramTarget {
    pub channel_index: u16,
    pub expect: ChannelType,
    pub sock: UdpSocket,
    pub addr: SocketAddr,
}

/// 一路 TCP 多路目标：连接在时间轴上的 `TcpConnect` 事件时建立（与单路语义一致）。
pub struct TcpMultiTarget {
    pub channel_index: u16,
    pub endpoint: SocketAddr,
    pub play_role: TcpRole,
    pub poll_timeout: Duration,
    pub stream: Option<TcpStream>,
}

/// 一路串口多路目标（启动时已打开；与单路一样只写 Data）。
pub struct SerialMultiTarget {
    pub channel_index: u16,
    pub port_name: String,
    pub port: SerialPort,
}

/// 一路 MQTT 多路目标（启动时已 CONNECT；按帧 aux 作 topic 发布）。
pub struct MqttMultiTarget {
    pub channel_index: u16,
    pub broker_label: String,
    pub client: MqttClient,
    pub topic_prefix: String,
    pub qos_override: Option<u8>,
}

/// 多路重放目标（E12a–c：UDP + TCP + 串口 + MQTT）。
pub enum MultiReplayTarget {
    Udp(DatagramTarget),
    Tcp(TcpMultiTarget),
    Serial(SerialMultiTarget),
    Mqtt(MqttMultiTarget),
}

impl MultiReplayTarget {
    pub fn channel_index(&self) -> u16 {
        match self {
            Self::Udp(t) => t.channel_index,
            Self::Tcp(t) => t.channel_index,
            Self::Serial(t) => t.channel_index,
            Self::Mqtt(t) => t.channel_index,
        }
    }

    pub fn expect(&self) -> ChannelType {
        match self {
            Self::Udp(t) => t.expect,
            Self::Tcp(_) => ChannelType::Tcp,
            Self::Serial(_) => ChannelType::Serial,
            Self::Mqtt(_) => ChannelType::Mqtt,
        }
    }

    fn label(&self) -> String {
        match self {
            Self::Udp(t) => format!("{}→{}", t.channel_index, t.addr),
            Self::Tcp(t) => format!(
                "{}→tcp:{}:{}",
                t.channel_index,
                t.play_role.as_str(),
                t.endpoint
            ),
            Self::Serial(t) => format!("{}→serial:{}", t.channel_index, t.port_name),
            Self::Mqtt(t) => format!("{}→mqtt:{}", t.channel_index, t.broker_label),
        }
    }

    fn clear_stream_on_seek(&mut self) {
        if let Self::Tcp(t) = self {
            t.stream = None;
        }
    }
}

pub struct MultiReplayOptions {
    pub in_path: PathBuf,
    pub speed: f64,
    pub fault: FaultInjection,
    pub looping: bool,
}

/// 按相对时间轴统一调度：有目标的通道发往对应 sink，无目标通道跳过。
pub fn replay_multi_from_file_with(
    opts: &MultiReplayOptions,
    targets: &mut [MultiReplayTarget],
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    if targets.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "多路重放需要至少一个目标",
        ));
    }

    let mut reader = SessionReader::open(&opts.in_path)?;
    if !reader.header().is_finalized() {
        eprintln!("警告：文件未标记 FINALIZED，将扫到 EOF");
    }

    let max_ch = targets
        .iter()
        .map(|t| t.channel_index())
        .max()
        .unwrap_or(0) as usize;
    let session_max = reader
        .channels()
        .iter()
        .map(|c| c.channel_index)
        .max()
        .unwrap_or(0) as usize;
    let slot_n = max_ch.max(session_max) + 1;
    let mut ch_to_tgt: Vec<Option<usize>> = vec![None; slot_n];
    for (i, t) in targets.iter().enumerate() {
        let ch = t.channel_index() as usize;
        if ch_to_tgt[ch].is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("targets 中通道 {ch} 重复"),
            ));
        }
        let Some(entry) = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == t.channel_index())
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("会话无通道 {}", t.channel_index()),
            ));
        };
        if entry.channel_type != t.expect() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "通道 {} 类型不匹配：会话 {:?}，目标 {:?}",
                    t.channel_index(),
                    entry.channel_type,
                    t.expect()
                ),
            ));
        }
        ch_to_tgt[ch] = Some(i);
    }

    let speed = SessionClock::clamp_speed(opts.speed);
    let mut base = Instant::now();
    let mut skip_before_us = from_us;
    if from_us > 0 {
        SessionClock::rebase_timeline(&mut base, from_us, speed);
    }
    let mut sent = 0u64;
    let mut rng = FaultRng::new();
    let fault = opts.fault.clamped();
    let mut pass_matched = 0u64;

    let map_desc: String = targets
        .iter()
        .map(|t| t.label())
        .collect::<Vec<_>>()
        .join(", ");
    eprintln!(
        "重放中（多路对齐）：{} → [{}]（{}x，fromUs={from_us}{}{}）",
        opts.in_path.display(),
        map_desc,
        speed,
        if fault.is_active() { "，故障注入" } else { "" },
        if opts.looping { "，循环" } else { "" }
    );

    reader.rewind_packets()?;
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if let Some(target) = take_pending_seek(seek_us, &mut reader, &mut base, speed)? {
            skip_before_us = target;
            for t in targets.iter_mut() {
                t.clear_stream_on_seek();
            }
            if let Some(cb) = on_progress.as_mut() {
                cb(sent, target);
            }
            continue;
        }
        let Some(pkt) = reader.next_packet()? else {
            if try_restart_loop(
                opts.looping,
                stop,
                pass_matched,
                &mut reader,
                &mut base,
                speed,
                &mut skip_before_us,
                &mut pass_matched,
                sent,
                on_progress,
            )? {
                for t in targets.iter_mut() {
                    t.clear_stream_on_seek();
                }
                continue;
            }
            break;
        };

        let Some(ti) = ch_to_tgt
            .get(pkt.channel_index as usize)
            .and_then(|x| *x)
        else {
            continue;
        };

        // UDP/视频/串口/MQTT：只关心 Data；TCP：连接事件也计入本趟匹配
        let data_only = matches!(
            targets[ti],
            MultiReplayTarget::Udp(_)
                | MultiReplayTarget::Serial(_)
                | MultiReplayTarget::Mqtt(_)
        );
        if data_only && pkt.kind != PacketKind::Data {
            continue;
        }
        if matches!(targets[ti], MultiReplayTarget::Tcp(_))
            && !matches!(
                pkt.kind,
                PacketKind::Data | PacketKind::TcpConnect | PacketKind::TcpDisconnect
            )
        {
            continue;
        }

        pass_matched += 1;

        if pkt.relative_us < skip_before_us {
            if let MultiReplayTarget::Tcp(t) = &mut targets[ti] {
                match pkt.kind {
                    PacketKind::TcpConnect => {
                        t.stream = Some(open_tcp_stream(
                            t.play_role,
                            t.endpoint,
                            stop,
                            t.poll_timeout,
                        )?);
                    }
                    PacketKind::TcpDisconnect => {
                        t.stream = None;
                    }
                    _ => {}
                }
            }
            continue;
        }

        match SessionClock::sleep_until_relative_interruptible(
            &mut base,
            pkt.relative_us,
            speed,
            stop,
            paused,
            seek_us,
        ) {
            WaitResult::Stopped => break,
            WaitResult::Seek => continue,
            WaitResult::Ready => {}
        }

        match &mut targets[ti] {
            MultiReplayTarget::Udp(tgt) => {
                emit_with_fault(
                    &pkt.payload,
                    &fault,
                    &mut rng,
                    pkt.relative_us,
                    &mut sent,
                    on_progress,
                    |bytes| send_to(&tgt.sock, bytes, tgt.addr).map(|_| ()),
                )?;
            }
            MultiReplayTarget::Serial(tgt) => {
                emit_with_fault(
                    &pkt.payload,
                    &fault,
                    &mut rng,
                    pkt.relative_us,
                    &mut sent,
                    on_progress,
                    |bytes| tgt.port.write_all(bytes),
                )?;
            }
            MultiReplayTarget::Mqtt(tgt) => {
                let topic = String::from_utf8_lossy(&pkt.aux);
                let topic = apply_topic_prefix(&tgt.topic_prefix, &topic);
                let qos = tgt
                    .qos_override
                    .unwrap_or_else(|| mqtt_qos_from_flags(pkt.flags));
                let retain = mqtt_retain_from_flags(pkt.flags);
                emit_with_fault(
                    &pkt.payload,
                    &fault,
                    &mut rng,
                    pkt.relative_us,
                    &mut sent,
                    on_progress,
                    |bytes| tgt.client.publish(&topic, bytes, qos, retain),
                )?;
            }
            MultiReplayTarget::Tcp(tgt) => match pkt.kind {
                PacketKind::TcpConnect => {
                    tgt.stream = Some(open_tcp_stream(
                        tgt.play_role,
                        tgt.endpoint,
                        stop,
                        tgt.poll_timeout,
                    )?);
                }
                PacketKind::Data => {
                    if pkt.direction != 0 {
                        continue;
                    }
                    let Some(ref s) = tgt.stream else {
                        eprintln!(
                            "警告：通道 {} 未连接就遇到数据帧，已跳过",
                            tgt.channel_index
                        );
                        continue;
                    };
                    emit_with_fault(
                        &pkt.payload,
                        &fault,
                        &mut rng,
                        pkt.relative_us,
                        &mut sent,
                        on_progress,
                        |bytes| write_all(s, bytes),
                    )?;
                }
                PacketKind::TcpDisconnect => {
                    tgt.stream = None;
                }
                _ => {}
            },
        }
    }

    eprintln!("重放结束（多路）：发送 {sent} 帧");
    Ok(sent)
}

/// 兼容旧调用：仅数据报目标时转发到统一调度器。
pub fn replay_multi_datagram_from_file_with(
    opts: &MultiReplayOptions,
    targets: &[DatagramTarget],
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    // DatagramTarget 不含 Copy；搬进枚举后由调度器持有
    let mut owned: Vec<MultiReplayTarget> = Vec::with_capacity(targets.len());
    for t in targets {
        let sock = t.sock.try_clone().map_err(|e| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("克隆 UDP 发送端失败: {e}"),
            )
        })?;
        owned.push(MultiReplayTarget::Udp(DatagramTarget {
            channel_index: t.channel_index,
            expect: t.expect,
            sock,
            addr: t.addr,
        }));
    }
    replay_multi_from_file_with(opts, &mut owned, stop, paused, seek_us, from_us, on_progress)
}

fn open_tcp_stream(
    play_role: TcpRole,
    endpoint: SocketAddr,
    stop: &AtomicBool,
    poll_timeout: Duration,
) -> io::Result<TcpStream> {
    match play_role {
        TcpRole::Server => {
            let listener = listen(endpoint)?;
            let local = listener.local_addr().unwrap_or(endpoint);
            eprintln!("TCP 多路重放等待上位机接入：{local}");
            match accept_until(&listener, stop, poll_timeout)? {
                Some((s, peer)) => {
                    eprintln!("TCP 多路已接入：{peer}");
                    Ok(s)
                }
                None => Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "等待 TCP 连接时被停止",
                )),
            }
        }
        TcpRole::Client => {
            eprintln!("TCP 多路重放连接：{endpoint}");
            connect(endpoint, poll_timeout)
        }
    }
}

/// 打开数据报发送端；失败则整次失败。
pub fn open_datagram_target(
    channel_index: u16,
    expect: ChannelType,
    addr: SocketAddr,
    local_bind: SocketAddr,
) -> io::Result<DatagramTarget> {
    let sock = bind_sender(local_bind)?;
    Ok(DatagramTarget {
        channel_index,
        expect,
        sock,
        addr,
    })
}

/// 构造尚未建连的 TCP 多路目标（连接在时间轴事件上建立）。
pub fn open_tcp_multi_target(
    channel_index: u16,
    endpoint: SocketAddr,
    play_role: TcpRole,
    poll_timeout: Duration,
) -> TcpMultiTarget {
    TcpMultiTarget {
        channel_index,
        endpoint,
        play_role,
        poll_timeout,
        stream: None,
    }
}

/// 打开串口多路目标；失败则整次失败。
pub fn open_serial_multi_target(
    channel_index: u16,
    config: &SerialConfig,
    poll_timeout: Duration,
) -> io::Result<SerialMultiTarget> {
    let port = SerialPort::open(config, poll_timeout)?;
    Ok(SerialMultiTarget {
        channel_index,
        port_name: config.port_name.clone(),
        port,
    })
}

/// 自检用：注入已打开的串口（内存环回或实口）。
pub fn serial_multi_target_with_port(
    channel_index: u16,
    port_name: impl Into<String>,
    port: SerialPort,
) -> SerialMultiTarget {
    SerialMultiTarget {
        channel_index,
        port_name: port_name.into(),
        port,
    }
}

/// 连接 Broker，构造 MQTT 多路目标；失败则整次失败。
pub fn open_mqtt_multi_target(
    channel_index: u16,
    config: &MqttConfig,
    topic_prefix: impl Into<String>,
    qos_override: Option<u8>,
    poll_timeout: Duration,
) -> io::Result<MqttMultiTarget> {
    let client = MqttClient::connect(config, poll_timeout)?;
    Ok(MqttMultiTarget {
        channel_index,
        broker_label: format!("{}:{}", config.broker, config.port),
        client,
        topic_prefix: topic_prefix.into(),
        qos_override,
    })
}

/// 最小自检：两路 UDP 同会话录制后对齐重放，载荷与通道绑定正确。
pub fn self_check_multi_udp_replay() -> io::Result<()> {
    use std::net::UdpSocket;
    use std::sync::Arc;
    use std::thread;

    use crate::multi_record::{record_multi_to_file, ChannelJob, MultiRecordOptions};

    let a = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let b = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let out = std::env::temp_dir().join(format!(
        "fieldreplay_multi_replay_{}.fcap",
        std::process::id()
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);
    let out_c = out.clone();

    let rec = thread::spawn(move || {
        record_multi_to_file(
            &MultiRecordOptions {
                out_path: out_c,
                session_title: Some("multi-udp-replay".into()),
                max_duration: Some(Duration::from_secs(3)),
                poll_timeout: Duration::from_millis(50),
            },
            &[
                ChannelJob::Udp {
                    name: "u0".into(),
                    bind: a,
                },
                ChannelJob::Udp {
                    name: "u1".into(),
                    bind: b,
                },
            ],
            &stop_c,
            None,
            None,
            None,
        )
    });

    thread::sleep(Duration::from_millis(200));
    let sender = UdpSocket::bind("127.0.0.1:0")?;
    sender.send_to(b"A1", a)?;
    sender.send_to(b"B1", b)?;
    thread::sleep(Duration::from_millis(30));
    sender.send_to(b"A2", a)?;
    sender.send_to(b"B2", b)?;
    thread::sleep(Duration::from_millis(150));
    stop.store(true, Ordering::SeqCst);

    let n = rec
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "多路录制线程 panic"))??;
    if n < 4 {
        let _ = std::fs::remove_file(&out);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("多路录制帧数不足: {n}"),
        ));
    }

    let recv0 = UdpSocket::bind("127.0.0.1:0")?;
    recv0.set_read_timeout(Some(Duration::from_millis(500)))?;
    let addr0 = recv0.local_addr()?;
    let recv1 = UdpSocket::bind("127.0.0.1:0")?;
    recv1.set_read_timeout(Some(Duration::from_millis(500)))?;
    let addr1 = recv1.local_addr()?;

    let t0 = open_datagram_target(0, ChannelType::Udp, addr0, "0.0.0.0:0".parse().unwrap())?;
    let t1 = open_datagram_target(1, ChannelType::Udp, addr1, "0.0.0.0:0".parse().unwrap())?;

    let stop_r = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    let sent = replay_multi_datagram_from_file_with(
        &MultiReplayOptions {
            in_path: out.clone(),
            speed: 50.0,
            fault: FaultInjection::OFF,
            looping: false,
        },
        &[t0, t1],
        &stop_r,
        &paused,
        &seek,
        0,
        &mut on,
    );
    let _ = std::fs::remove_file(&out);
    let sent = sent?;
    if sent < 4 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("多路重放发送帧数不足: {sent}"),
        ));
    }

    let mut got0 = Vec::new();
    let mut got1 = Vec::new();
    let mut buf = [0u8; 64];
    for _ in 0..8 {
        if let Ok((n, _)) = recv0.recv_from(&mut buf) {
            got0.push(buf[..n].to_vec());
        }
        if let Ok((n, _)) = recv1.recv_from(&mut buf) {
            got1.push(buf[..n].to_vec());
        }
    }

    if got0.len() < 2 || got1.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "对齐重放收包不足 got0={} got1={}",
                got0.len(),
                got1.len()
            ),
        ));
    }
    if got0[0] != b"A1" || got0[1] != b"A2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("通道0载荷不符: {:?}", got0),
        ));
    }
    if got1[0] != b"B1" || got1[1] != b"B2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("通道1载荷不符: {:?}", got1),
        ));
    }

    // 缺目标：仅绑通道0时应跳过通道1
    let out2 = std::env::temp_dir().join(format!(
        "fieldreplay_multi_replay_skip_{}.fcap",
        std::process::id()
    ));
    let a2 = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let b2 = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let stop2 = Arc::new(AtomicBool::new(false));
    let stop2c = Arc::clone(&stop2);
    let out2c = out2.clone();
    let rec2 = thread::spawn(move || {
        record_multi_to_file(
            &MultiRecordOptions {
                out_path: out2c,
                session_title: Some("multi-skip".into()),
                max_duration: Some(Duration::from_secs(2)),
                poll_timeout: Duration::from_millis(50),
            },
            &[
                ChannelJob::Udp {
                    name: "u0".into(),
                    bind: a2,
                },
                ChannelJob::Udp {
                    name: "u1".into(),
                    bind: b2,
                },
            ],
            &stop2c,
            None,
            None,
            None,
        )
    });
    thread::sleep(Duration::from_millis(150));
    let s2 = UdpSocket::bind("127.0.0.1:0")?;
    s2.send_to(b"X0", a2)?;
    s2.send_to(b"Y1", b2)?;
    thread::sleep(Duration::from_millis(100));
    stop2.store(true, Ordering::SeqCst);
    let _ = rec2
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "skip 录制线程 panic"))??;

    let only = UdpSocket::bind("127.0.0.1:0")?;
    only.set_read_timeout(Some(Duration::from_millis(300)))?;
    let only_addr = only.local_addr()?;
    let t_only = open_datagram_target(0, ChannelType::Udp, only_addr, "0.0.0.0:0".parse().unwrap())?;
    let stop_s = AtomicBool::new(false);
    let paused_s = AtomicBool::new(false);
    let seek_s = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on_s: OnReplayProgress<'_> = None;
    let sent_s = replay_multi_datagram_from_file_with(
        &MultiReplayOptions {
            in_path: out2.clone(),
            speed: 50.0,
            fault: FaultInjection::OFF,
            looping: false,
        },
        &[t_only],
        &stop_s,
        &paused_s,
        &seek_s,
        0,
        &mut on_s,
    );
    let _ = std::fs::remove_file(&out2);
    let sent_s = sent_s?;
    if sent_s < 1 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("仅绑通道0时期望至少发1帧，实际 {sent_s}"),
        ));
    }
    let mut only_got = Vec::new();
    let mut buf2 = [0u8; 64];
    while let Ok((n, _)) = only.recv_from(&mut buf2) {
        only_got.push(buf2[..n].to_vec());
    }
    if !only_got.iter().any(|p| p == b"X0") {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("缺目标跳过场景未收到 X0: {:?}", only_got),
        ));
    }
    if only_got.iter().any(|p| p == b"Y1") {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "通道1 无目标时载荷不应发到通道0 接收端",
        ));
    }

    eprintln!("  多路 UDP 对齐重放自检：通过（sent={sent}，skip 路径 ok）");
    Ok(())
}

/// E12a：同会话 UDP + TCP（录制 server）对齐重放；TCP 重放侧为 client。
pub fn self_check_multi_udp_tcp_replay() -> io::Result<()> {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream, UdpSocket};
    use std::sync::Arc;
    use std::thread;

    use crate::multi_record::{record_multi_to_file, ChannelJob, MultiRecordOptions};
    use crate::tcp::{TcpConfig, TcpRole};

    let udp_bind = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let tcp_listen = {
        let l = TcpListener::bind("127.0.0.1:0")?;
        let addr = l.local_addr()?;
        // 仅占端口拿地址；真正 listen 交给多路录制
        drop(l);
        addr
    };

    let out = std::env::temp_dir().join(format!(
        "fieldreplay_multi_udp_tcp_{}.fcap",
        std::process::id()
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);
    let out_c = out.clone();
    let tcp_cfg = TcpConfig::new(TcpRole::Server, "127.0.0.1", tcp_listen.port());

    let rec = thread::spawn(move || {
        record_multi_to_file(
            &MultiRecordOptions {
                out_path: out_c,
                session_title: Some("multi-udp-tcp".into()),
                max_duration: Some(Duration::from_secs(4)),
                poll_timeout: Duration::from_millis(50),
            },
            &[
                ChannelJob::Udp {
                    name: "u0".into(),
                    bind: udp_bind,
                },
                ChannelJob::Tcp {
                    name: "t1".into(),
                    config: tcp_cfg,
                },
            ],
            &stop_c,
            None,
            None,
            None,
        )
    });

    thread::sleep(Duration::from_millis(250));
    let udp_tx = UdpSocket::bind("127.0.0.1:0")?;
    udp_tx.send_to(b"U1", udp_bind)?;

    let mut tcp_peer = TcpStream::connect(tcp_listen)?;
    tcp_peer.set_write_timeout(Some(Duration::from_millis(500)))?;
    tcp_peer.write_all(b"T1")?;
    tcp_peer.flush()?;
    thread::sleep(Duration::from_millis(40));
    udp_tx.send_to(b"U2", udp_bind)?;
    tcp_peer.write_all(b"T2")?;
    tcp_peer.flush()?;
    thread::sleep(Duration::from_millis(120));
    drop(tcp_peer);
    thread::sleep(Duration::from_millis(80));
    stop.store(true, Ordering::SeqCst);

    let n = rec
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "UDP+TCP 录制线程 panic"))??;
    if n < 4 {
        let _ = std::fs::remove_file(&out);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("UDP+TCP 录制帧数不足: {n}"),
        ));
    }

    let udp_rx = UdpSocket::bind("127.0.0.1:0")?;
    udp_rx.set_read_timeout(Some(Duration::from_millis(800)))?;
    let udp_rx_addr = udp_rx.local_addr()?;

    let tcp_rx_listener = TcpListener::bind("127.0.0.1:0")?;
    let tcp_rx_addr = tcp_rx_listener.local_addr()?;
    let tcp_got = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let tcp_got_c = Arc::clone(&tcp_got);
    let acceptor = thread::spawn(move || -> io::Result<()> {
        tcp_rx_listener.set_nonblocking(false)?;
        let (mut stream, _) = tcp_rx_listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_millis(1500)))?;
        let mut buf = [0u8; 256];
        let mut acc = Vec::new();
        loop {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => acc.extend_from_slice(&buf[..n]),
                Err(e)
                    if e.kind() == io::ErrorKind::WouldBlock
                        || e.kind() == io::ErrorKind::TimedOut =>
                {
                    break;
                }
                Err(e) => return Err(e),
            }
            if acc.len() >= 4 {
                break;
            }
        }
        *tcp_got_c.lock().unwrap() = acc;
        Ok(())
    });

    // 录制为 server → 重放 client 连到我们的 listener
    let mut targets = vec![
        MultiReplayTarget::Udp(open_datagram_target(
            0,
            ChannelType::Udp,
            udp_rx_addr,
            "0.0.0.0:0".parse().unwrap(),
        )?),
        MultiReplayTarget::Tcp(open_tcp_multi_target(
            1,
            tcp_rx_addr,
            TcpRole::Client,
            Duration::from_millis(200),
        )),
    ];

    let stop_r = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    let sent = replay_multi_from_file_with(
        &MultiReplayOptions {
            in_path: out.clone(),
            speed: 50.0,
            fault: FaultInjection::OFF,
            looping: false,
        },
        &mut targets,
        &stop_r,
        &paused,
        &seek,
        0,
        &mut on,
    );
    let _ = std::fs::remove_file(&out);
    let sent = sent?;
    if sent < 2 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("UDP+TCP 重放发送帧数不足: {sent}"),
        ));
    }

    acceptor
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "TCP 接收线程 panic"))??;

    let mut got_u = Vec::new();
    let mut buf = [0u8; 64];
    for _ in 0..8 {
        if let Ok((n, _)) = udp_rx.recv_from(&mut buf) {
            got_u.push(buf[..n].to_vec());
        }
    }
    if got_u.len() < 2 || got_u[0] != b"U1" || got_u[1] != b"U2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("UDP 通道载荷不符: {:?}", got_u),
        ));
    }
    let got_t = tcp_got.lock().unwrap().clone();
    if got_t != b"T1T2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("TCP 通道载荷不符: {:?}", got_t),
        ));
    }

    eprintln!("  多路 UDP+TCP 对齐重放自检：通过（sent={sent}）");
    Ok(())
}

/// E12b：同会话 UDP + 串口对齐重放（串口侧内存环回）。
pub fn self_check_multi_udp_serial_replay() -> io::Result<()> {
    use std::net::UdpSocket;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    use crate::fcap::{
        ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionWriter,
    };

    // ponytail: 合成多通道会话，避免为自检改 ChannelJob::Clone 注入 Mem 口；
    // 天花板是未覆盖多路录制串口路径（该路径生产已开实口）；升级可加 SerialPort 注入。
    let out = std::env::temp_dir().join(format!(
        "fieldreplay_multi_udp_serial_{}.fcap",
        std::process::id()
    ));
    {
        let channels = [
            ChannelEntry {
                channel_index: 0,
                channel_type: ChannelType::Udp,
                name: "u0".into(),
                config_json: r#"{"bind_ip":"127.0.0.1","bind_port":0,"peer_ip":"","peer_port":0}"#
                    .into(),
            },
            ChannelEntry {
                channel_index: 1,
                channel_type: ChannelType::Serial,
                name: "s1".into(),
                config_json: SerialConfig::new("LOOP-REC", 115_200).to_config_json(),
            },
        ];
        let mut w = SessionWriter::create(&out, 1_700_000_000_000_000, &channels, None)?;
        for (us, ch, payload) in [
            (10_000u64, 0u16, &b"U1"[..]),
            (20_000, 1, &b"S1"[..]),
            (30_000, 0, &b"U2"[..]),
            (40_000, 1, &b"S2"[..]),
        ] {
            w.append_packet(&PacketRecord {
                relative_us: us,
                channel_index: ch,
                kind: PacketKind::Data,
                direction: 0,
                flags: 0,
                aux: Vec::new(),
                payload: payload.to_vec(),
            })?;
        }
        w.finalize()?;
    }

    let udp_rx = UdpSocket::bind("127.0.0.1:0")?;
    udp_rx.set_read_timeout(Some(Duration::from_millis(800)))?;
    let udp_rx_addr = udp_rx.local_addr()?;

    let (end_play, end_listen) = SerialPort::open_loopback_pair(Duration::from_millis(200));
    let serial_got = Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
    let serial_got_c = Arc::clone(&serial_got);
    let listener = thread::spawn(move || -> io::Result<()> {
        let mut buf = [0u8; 64];
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut out = Vec::new();
        while out.len() < 2 && Instant::now() < deadline {
            if let Some(n) = end_listen.read(&mut buf)? {
                if n > 0 {
                    out.push(buf[..n].to_vec());
                }
            }
        }
        *serial_got_c.lock().unwrap() = out;
        Ok(())
    });

    let mut targets = vec![
        MultiReplayTarget::Udp(open_datagram_target(
            0,
            ChannelType::Udp,
            udp_rx_addr,
            "0.0.0.0:0".parse().unwrap(),
        )?),
        MultiReplayTarget::Serial(serial_multi_target_with_port(1, "LOOP-PLAY", end_play)),
    ];

    let stop_r = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    let sent = replay_multi_from_file_with(
        &MultiReplayOptions {
            in_path: out.clone(),
            speed: 50.0,
            fault: FaultInjection::OFF,
            looping: false,
        },
        &mut targets,
        &stop_r,
        &paused,
        &seek,
        0,
        &mut on,
    );
    let _ = std::fs::remove_file(&out);
    let sent = sent?;
    if sent < 4 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("UDP+串口重放发送帧数不足: {sent}"),
        ));
    }

    listener
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "串口接收线程 panic"))??;

    let mut got_u = Vec::new();
    let mut buf = [0u8; 64];
    for _ in 0..8 {
        if let Ok((n, _)) = udp_rx.recv_from(&mut buf) {
            got_u.push(buf[..n].to_vec());
        }
    }
    if got_u.len() < 2 || got_u[0] != b"U1" || got_u[1] != b"U2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("UDP 通道载荷不符: {:?}", got_u),
        ));
    }
    let got_s = serial_got.lock().unwrap().clone();
    if got_s.len() < 2 || got_s[0] != b"S1" || got_s[1] != b"S2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("串口通道载荷不符: {:?}", got_s),
        ));
    }

    eprintln!("  多路 UDP+串口对齐重放自检：通过（sent={sent}）");
    Ok(())
}

/// E12c：同会话 UDP + MQTT 对齐重放（MiniBroker 旁路订阅核验）。
pub fn self_check_multi_udp_mqtt_replay() -> io::Result<()> {
    use std::net::UdpSocket;
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;

    use crate::fcap::{ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionWriter};
    use crate::mqtt::{pack_mqtt_flags, MiniBroker, MqttClient, MqttConfig};

    let broker = MiniBroker::start()?;
    let addr = broker.addr();
    thread::sleep(Duration::from_millis(40));

    // ponytail: 合成多通道会话，与 E12b 同套路；升级可接多路录制 MQTT job。
    let out = std::env::temp_dir().join(format!(
        "fieldreplay_multi_udp_mqtt_{}.fcap",
        std::process::id()
    ));
    {
        let channels = [
            ChannelEntry {
                channel_index: 0,
                channel_type: ChannelType::Udp,
                name: "u0".into(),
                config_json: r#"{"bind_ip":"127.0.0.1","bind_port":0,"peer_ip":"","peer_port":0}"#
                    .into(),
            },
            ChannelEntry {
                channel_index: 1,
                channel_type: ChannelType::Mqtt,
                name: "m1".into(),
                config_json: MqttConfig::new(addr.ip().to_string(), addr.port(), "plant/#", 0)
                    .to_config_json(),
            },
        ];
        let mut w = SessionWriter::create(&out, 1_700_000_000_000_000, &channels, None)?;
        for (us, ch, topic, payload) in [
            (10_000u64, 0u16, "", &b"U1"[..]),
            (20_000, 1, "plant/line/a", &b"M1"[..]),
            (30_000, 0, "", &b"U2"[..]),
            (40_000, 1, "plant/line/b", &b"M2"[..]),
        ] {
            w.append_packet(&PacketRecord {
                relative_us: us,
                channel_index: ch,
                kind: PacketKind::Data,
                direction: 0,
                flags: pack_mqtt_flags(0, false),
                aux: topic.as_bytes().to_vec(),
                payload: payload.to_vec(),
            })?;
        }
        w.finalize()?;
    }

    let udp_rx = UdpSocket::bind("127.0.0.1:0")?;
    udp_rx.set_read_timeout(Some(Duration::from_millis(800)))?;
    let udp_rx_addr = udp_rx.local_addr()?;

    let listen_cfg = MqttConfig::new(addr.ip().to_string(), addr.port(), "unused", 0);
    let mut listener = MqttClient::connect(&listen_cfg, Duration::from_millis(100))?;
    listener.subscribe("plant/#", 0)?;
    thread::sleep(Duration::from_millis(80));

    let mqtt_got = Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<u8>)>::new()));
    let mqtt_got_c = Arc::clone(&mqtt_got);
    let listen_stop = Arc::new(AtomicBool::new(false));
    let listen_stop_c = Arc::clone(&listen_stop);
    let collector = thread::spawn(move || -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(4);
        let mut out = Vec::new();
        while out.len() < 2 && Instant::now() < deadline {
            if let Some(msg) = listener.poll_publish(&listen_stop_c)? {
                out.push((msg.topic, msg.payload));
            }
        }
        let _ = listener.disconnect();
        *mqtt_got_c.lock().unwrap() = out;
        Ok(())
    });

    let play_cfg = MqttConfig::new(addr.ip().to_string(), addr.port(), "unused", 0);
    let mut targets = vec![
        MultiReplayTarget::Udp(open_datagram_target(
            0,
            ChannelType::Udp,
            udp_rx_addr,
            "0.0.0.0:0".parse().unwrap(),
        )?),
        MultiReplayTarget::Mqtt(open_mqtt_multi_target(
            1,
            &play_cfg,
            "",
            Some(0),
            Duration::from_millis(100),
        )?),
    ];

    let stop_r = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    let sent = replay_multi_from_file_with(
        &MultiReplayOptions {
            in_path: out.clone(),
            speed: 10.0,
            fault: FaultInjection::OFF,
            looping: false,
        },
        &mut targets,
        &stop_r,
        &paused,
        &seek,
        0,
        &mut on,
    );
    let sent = sent?;
    // 迷你 Broker 约 2ms 一轮；等扇出完成再停旁路订阅，避免漏收末帧
    thread::sleep(Duration::from_millis(200));
    listen_stop.store(true, Ordering::SeqCst);
    collector
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "MQTT 接收线程 panic"))??;
    broker.stop();
    let _ = std::fs::remove_file(&out);

    if sent < 4 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("UDP+MQTT 重放发送帧数不足: {sent}"),
        ));
    }

    let mut got_u = Vec::new();
    let mut buf = [0u8; 64];
    for _ in 0..8 {
        if let Ok((n, _)) = udp_rx.recv_from(&mut buf) {
            got_u.push(buf[..n].to_vec());
        }
    }
    if got_u.len() < 2 || got_u[0] != b"U1" || got_u[1] != b"U2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("UDP 通道载荷不符: {:?}", got_u),
        ));
    }
    let got_m = mqtt_got.lock().unwrap().clone();
    if got_m.len() < 2 || got_m[0].1 != b"M1" || got_m[1].1 != b"M2" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("MQTT 通道载荷不符: {:?}", got_m),
        ));
    }
    if got_m[0].0 != "plant/line/a" || got_m[1].0 != "plant/line/b" {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("MQTT topic 不符: {:?}", got_m),
        ));
    }

    eprintln!("  多路 UDP+MQTT 对齐重放自检：通过（sent={sent}）");
    Ok(())
}
