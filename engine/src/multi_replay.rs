//! 多通道对齐重放（E11c）：单一相对时间轴，按 `channel_index` 扇出到各目标。
//!
//! ponytail: 本阶段 sink 仅 UDP/视频(UDP 推送)；串口/TCP/MQTT/RTSP 多路目标留待后续。
//! 天花板是混合类型会话只能部分重放；升级路径是为各类型实现 `ReplaySink` 后并入本调度器。

use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::clock::{SessionClock, WaitResult};
use crate::fcap::{ChannelType, PacketKind, SessionReader};
use crate::replay::{
    emit_with_fault, take_pending_seek, try_restart_loop, FaultInjection, FaultRng,
    OnReplayProgress,
};
use crate::udp::{bind_sender, send_to};

/// 一路已打开的数据报目标（UDP 或视频 UDP 推送）。
pub struct DatagramTarget {
    pub channel_index: u16,
    pub expect: ChannelType,
    pub sock: UdpSocket,
    pub addr: SocketAddr,
}

pub struct MultiReplayOptions {
    pub in_path: PathBuf,
    pub speed: f64,
    pub fault: FaultInjection,
    pub looping: bool,
}

/// 按相对时间轴统一调度：有目标的通道发往对应 sink，无目标通道跳过。
pub fn replay_multi_datagram_from_file_with(
    opts: &MultiReplayOptions,
    targets: &[DatagramTarget],
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
        .map(|t| t.channel_index)
        .max()
        .unwrap_or(0) as usize;
    let session_max = reader
        .channels()
        .iter()
        .map(|c| c.channel_index)
        .max()
        .unwrap_or(0) as usize;
    let slot_n = max_ch.max(session_max) + 1;
    let mut by_ch: Vec<Option<&DatagramTarget>> = vec![None; slot_n];
    for t in targets {
        let i = t.channel_index as usize;
        if by_ch[i].is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("targets 中通道 {i} 重复"),
            ));
        }
        let Some(entry) = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == t.channel_index)
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("会话无通道 {}", t.channel_index),
            ));
        };
        if entry.channel_type != t.expect {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "通道 {} 类型不匹配：会话 {:?}，目标 {:?}",
                    t.channel_index, entry.channel_type, t.expect
                ),
            ));
        }
        by_ch[i] = Some(t);
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
        .map(|t| format!("{}→{}", t.channel_index, t.addr))
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
                continue;
            }
            break;
        };
        if pkt.kind != PacketKind::Data {
            continue;
        }
        let Some(tgt) = by_ch.get(pkt.channel_index as usize).and_then(|x| *x) else {
            continue;
        };
        pass_matched += 1;
        if pkt.relative_us < skip_before_us {
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

    eprintln!("重放结束（多路）：发送 {sent} 帧");
    Ok(sent)
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
    // 复用：再录一份过短；改为直接用内存不够，简单再录一轮太慢——改为对已删文件不测。
    // 跳过语义由控制面响应 `skipped` 覆盖；此处确认「只绑一路」时另一路不串到本 socket。
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
