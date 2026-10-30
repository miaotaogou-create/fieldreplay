//! 多通道并发录制（E11a）：共享墙钟与 `.fcap` 写盘。
//!
//! ponytail: IO 循环与 `record.rs` 单路路径有意并行一份，避免本阶段大改单路；
//! 天花板是两边行为漂移；升级路径是抽出共享 worker 后让单路改走本模块。

use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::clock::SessionClock;
use crate::fcap::{
    ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionWriter,
};
use crate::mqtt::{pack_mqtt_flags, MqttClient, MqttConfig};
use crate::rtsp::{RtspClient, RtspUrl};
use crate::serial::{SerialConfig, SerialPort};
use crate::tcp::{
    accept_until, connect, listen, peer_aux as tcp_peer_aux, read_some, ReadOutcome, TcpConfig,
    TcpRole,
};
use crate::udp::{bind_receiver, peer_aux, recv_one};

/// 预览钩子（多线程安全）。
pub type PacketHook = Arc<dyn Fn(&PacketRecord) + Send + Sync>;

/// 一路待录通道（打开参数已解析）。
#[derive(Clone)]
pub enum ChannelJob {
    Udp {
        name: String,
        bind: SocketAddr,
    },
    Video {
        name: String,
        bind: SocketAddr,
        rtsp_url: Option<String>,
    },
    Serial {
        name: String,
        config: SerialConfig,
    },
    Tcp {
        name: String,
        config: TcpConfig,
    },
    Mqtt {
        name: String,
        config: MqttConfig,
    },
}

impl ChannelJob {
    pub fn name(&self) -> &str {
        match self {
            Self::Udp { name, .. }
            | Self::Video { name, .. }
            | Self::Serial { name, .. }
            | Self::Tcp { name, .. }
            | Self::Mqtt { name, .. } => name,
        }
    }

    pub fn channel_type(&self) -> ChannelType {
        match self {
            Self::Udp { .. } => ChannelType::Udp,
            Self::Video { .. } => ChannelType::Video,
            Self::Serial { .. } => ChannelType::Serial,
            Self::Tcp { .. } => ChannelType::Tcp,
            Self::Mqtt { .. } => ChannelType::Mqtt,
        }
    }

    pub fn config_json(&self) -> String {
        match self {
            Self::Udp { bind, .. } => format!(
                r#"{{"bind_ip":"{}","bind_port":{},"peer_ip":"","peer_port":0}}"#,
                bind.ip(),
                bind.port()
            ),
            Self::Video { bind, rtsp_url, .. } => {
                let url = rtsp_url.as_deref().unwrap_or("");
                let media = if url.trim().is_empty() {
                    "rtp_udp"
                } else {
                    "rtsp_tcp"
                };
                format!(
                    r#"{{"bind_ip":"{}","bind_port":{},"peer_ip":"","peer_port":0,"media":"{}","rtsp_url":{}}}"#,
                    bind.ip(),
                    bind.port(),
                    media,
                    json_string(url)
                )
            }
            Self::Serial { config, .. } => config.to_config_json(),
            Self::Tcp { config, .. } => config.to_config_json(),
            Self::Mqtt { config, .. } => config.to_config_json(),
        }
    }
}

pub struct MultiRecordOptions {
    pub out_path: PathBuf,
    pub session_title: Option<String>,
    pub max_duration: Option<Duration>,
    pub poll_timeout: Duration,
}

/// 多路录制共享写盘句柄；控制面可经此热插 `kind=marker`。
pub struct LiveRecordSink {
    writer: Arc<Mutex<SessionWriter>>,
    clock: SessionClock,
    channel_count: u16,
    hook: Option<PacketHook>,
}

impl LiveRecordSink {
    fn append(&self, pkt: &PacketRecord) -> io::Result<()> {
        {
            let mut w = self
                .writer
                .lock()
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "写盘锁中毒"))?;
            w.append_packet(pkt)?;
        }
        if let Some(cb) = &self.hook {
            cb(pkt);
        }
        Ok(())
    }

    /// 按当前相对戳写入 marker（payload 空，aux=UTF-8 说明）。
    pub fn append_marker(&self, text: &str, channel_index: u16) -> io::Result<PacketRecord> {
        if self.channel_count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "无通道表，无法插入 marker",
            ));
        }
        if channel_index >= self.channel_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "通道索引 {channel_index} 超出范围（0..{}）",
                    self.channel_count.saturating_sub(1)
                ),
            ));
        }
        let pkt = PacketRecord {
            relative_us: self.clock.relative_us(),
            channel_index,
            kind: PacketKind::Marker,
            direction: 0,
            flags: 0,
            aux: text.as_bytes().to_vec(),
            payload: Vec::new(),
        };
        self.append(&pkt)?;
        Ok(pkt)
    }
}

/// 多路并发录制到同一 `.fcap`。任一路启动期失败则停其余并返回 Err。
/// `live_out` 非空时，写盘句柄就绪后写入供控制面热插 marker；结束时清空。
pub fn record_multi_to_file(
    opts: &MultiRecordOptions,
    jobs: &[ChannelJob],
    stop: &AtomicBool,
    hook: Option<PacketHook>,
    live_out: Option<Arc<Mutex<Option<Arc<LiveRecordSink>>>>>,
) -> io::Result<u64> {
    if jobs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "多路录制至少需要一路通道",
        ));
    }

    let clock = SessionClock::start();
    let channels: Vec<ChannelEntry> = jobs
        .iter()
        .enumerate()
        .map(|(i, job)| ChannelEntry {
            channel_index: i as u16,
            channel_type: job.channel_type(),
            name: job.name().to_string(),
            config_json: job.config_json(),
        })
        .collect();
    let meta = opts
        .session_title
        .as_ref()
        .map(|t| format!(r#"{{"session_title":{}}}"#, json_string(t)));

    let writer = Arc::new(Mutex::new(SessionWriter::create(
        &opts.out_path,
        clock.start_wall_unix_us(),
        &channels,
        meta.as_deref(),
    )?));
    let sink = Arc::new(LiveRecordSink {
        writer: Arc::clone(&writer),
        clock,
        channel_count: jobs.len() as u16,
        hook: hook.clone(),
    });
    if let Some(slot) = &live_out {
        if let Ok(mut g) = slot.lock() {
            *g = Some(Arc::clone(&sink));
        }
    }

    let ready_ok = Arc::new(AtomicU64::new(0));
    let ready_fail = Arc::new(AtomicBool::new(false));
    let fail_msg = Arc::new(Mutex::new(String::new()));
    let workers_stop = Arc::new(AtomicBool::new(false));

    let mut joins: Vec<JoinHandle<io::Result<u64>>> = Vec::with_capacity(jobs.len());
    for (i, job) in jobs.iter().enumerate() {
        let sink_c = Arc::clone(&sink);
        let stop_c = Arc::clone(&workers_stop);
        let job_c = job.clone();
        let ready_ok_c = Arc::clone(&ready_ok);
        let ready_fail_c = Arc::clone(&ready_fail);
        let fail_msg_c = Arc::clone(&fail_msg);
        let poll = opts.poll_timeout;
        let max_dur = opts.max_duration;
        let ch = i as u16;

        joins.push(thread::spawn(move || {
            let run_stop = Arc::clone(&stop_c);
            let result = run_job(
                &job_c,
                ch,
                &sink_c,
                &run_stop,
                max_dur,
                poll,
                &ready_ok_c,
                &ready_fail_c,
                &fail_msg_c,
            );
            if result.is_err() {
                ready_fail_c.store(true, Ordering::SeqCst);
                run_stop.store(true, Ordering::SeqCst);
            }
            result
        }));
    }

    let n = jobs.len() as u64;
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        if ready_fail.load(Ordering::SeqCst) {
            break;
        }
        if ready_ok.load(Ordering::SeqCst) >= n {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    if ready_fail.load(Ordering::SeqCst) || ready_ok.load(Ordering::SeqCst) < n {
        workers_stop.store(true, Ordering::SeqCst);
        for j in joins {
            let _ = j.join();
        }
        let msg = fail_msg.lock().map(|m| m.clone()).unwrap_or_default();
        let msg = if msg.is_empty() {
            "多路录制启动失败".into()
        } else {
            msg
        };
        if let Some(slot) = &live_out {
            if let Ok(mut g) = slot.lock() {
                *g = None;
            }
        }
        let _ = std::fs::remove_file(&opts.out_path);
        return Err(io::Error::new(io::ErrorKind::Other, msg));
    }

    while !stop.load(Ordering::SeqCst) && !workers_stop.load(Ordering::SeqCst) {
        if let Some(max) = opts.max_duration {
            if sink.clock.relative_us() >= max.as_micros() as u64 {
                break;
            }
        }
        thread::sleep(Duration::from_millis(20));
    }
    workers_stop.store(true, Ordering::SeqCst);

    let mut first_err: Option<io::Error> = None;
    for j in joins {
        match j.join() {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
            Err(_) => {
                if first_err.is_none() {
                    first_err = Some(io::Error::new(
                        io::ErrorKind::Other,
                        "录制线程异常退出",
                    ));
                }
            }
        }
    }

    if let Some(slot) = &live_out {
        if let Ok(mut g) = slot.lock() {
            *g = None;
        }
    }

    let count = {
        let mut w = writer
            .lock()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "写盘锁中毒"))?;
        w.finalize()?;
        w.packet_count()
    };

    if let Some(e) = first_err {
        return Err(e);
    }

    eprintln!(
        "多路录制结束：{count} 帧 / {} 通道 → {}",
        jobs.len(),
        opts.out_path.display()
    );
    Ok(count)
}

fn run_job(
    job: &ChannelJob,
    channel_index: u16,
    sink: &LiveRecordSink,
    stop: &AtomicBool,
    max_duration: Option<Duration>,
    poll_timeout: Duration,
    ready_ok: &AtomicU64,
    ready_fail: &AtomicBool,
    fail_msg: &Mutex<String>,
) -> io::Result<u64> {
    let mark_fail = |e: &io::Error| {
        ready_fail.store(true, Ordering::SeqCst);
        if let Ok(mut m) = fail_msg.lock() {
            if m.is_empty() {
                *m = format!("通道[{channel_index}] {}: {e}", job.name());
            }
        }
    };

    match job {
        ChannelJob::Udp { bind, name } => {
            let sock = match bind_receiver(*bind, Some(poll_timeout)) {
                Ok(s) => s,
                Err(e) => {
                    mark_fail(&e);
                    return Err(e);
                }
            };
            ready_ok.fetch_add(1, Ordering::SeqCst);
            eprintln!("录制中（多路 UDP）：{name} bind {bind}");
            run_datagram_loop(sink, channel_index, &sock, stop, max_duration)
        }
        ChannelJob::Video {
            bind,
            rtsp_url,
            name,
        } => {
            if let Some(url) = rtsp_url.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                let parsed = match RtspUrl::parse(url) {
                    Ok(p) => p,
                    Err(e) => {
                        mark_fail(&e);
                        return Err(e);
                    }
                };
                let mut client = match RtspClient::open(&parsed, poll_timeout) {
                    Ok(c) => c,
                    Err(e) => {
                        mark_fail(&e);
                        return Err(e);
                    }
                };
                ready_ok.fetch_add(1, Ordering::SeqCst);
                eprintln!("录制中（多路视频·RTSP）：{name}");
                let mut count = 0u64;
                while !stop.load(Ordering::SeqCst) {
                    if let Some(max) = max_duration {
                        if sink.clock.relative_us() >= max.as_micros() as u64 {
                            break;
                        }
                    }
                    match client.poll_rtp(stop)? {
                        Some(payload) => {
                            let pkt = PacketRecord {
                                relative_us: sink.clock.relative_us(),
                                channel_index,
                                kind: PacketKind::Data,
                                direction: 0,
                                flags: 0,
                                aux: b"rtsp".to_vec(),
                                payload,
                            };
                            sink.append(&pkt)?;
                            count += 1;
                        }
                        None => continue,
                    }
                }
                client.teardown();
                Ok(count)
            } else {
                let sock = match bind_receiver(*bind, Some(poll_timeout)) {
                    Ok(s) => s,
                    Err(e) => {
                        mark_fail(&e);
                        return Err(e);
                    }
                };
                ready_ok.fetch_add(1, Ordering::SeqCst);
                eprintln!("录制中（多路视频·RTP/UDP）：{name} bind {bind}");
                run_datagram_loop(sink, channel_index, &sock, stop, max_duration)
            }
        }
        ChannelJob::Serial { config, name } => {
            let port = match SerialPort::open(config, poll_timeout) {
                Ok(p) => p,
                Err(e) => {
                    mark_fail(&e);
                    return Err(e);
                }
            };
            ready_ok.fetch_add(1, Ordering::SeqCst);
            eprintln!("录制中（多路串口）：{name} {}", config.port_name);
            let mut buf = vec![0u8; 4096];
            let mut count = 0u64;
            while !stop.load(Ordering::SeqCst) {
                if let Some(max) = max_duration {
                    if sink.clock.relative_us() >= max.as_micros() as u64 {
                        break;
                    }
                }
                match port.read(&mut buf)? {
                    Some(n) if n > 0 => {
                        let pkt = PacketRecord {
                            relative_us: sink.clock.relative_us(),
                            channel_index,
                            kind: PacketKind::Data,
                            direction: 0,
                            flags: 0,
                            aux: Vec::new(),
                            payload: buf[..n].to_vec(),
                        };
                        sink.append(&pkt)?;
                        count += 1;
                    }
                    _ => continue,
                }
            }
            Ok(count)
        }
        ChannelJob::Tcp { config, name } => {
            // 启动期：server=listen 成功；client=connect 成功。accept 算运行期。
            match config.role {
                TcpRole::Server => {
                    let bind = match config.socket_addr() {
                        Ok(a) => a,
                        Err(e) => {
                            mark_fail(&e);
                            return Err(e);
                        }
                    };
                    let listener = match listen(bind) {
                        Ok(l) => l,
                        Err(e) => {
                            mark_fail(&e);
                            return Err(e);
                        }
                    };
                    ready_ok.fetch_add(1, Ordering::SeqCst);
                    eprintln!(
                        "录制中（多路 TCP server）：{name} {}:{}",
                        config.host, config.port
                    );
                    let stream = match accept_until(&listener, stop, poll_timeout)? {
                        Some((s, peer)) => {
                            sink.append(
                                &PacketRecord {
                                    relative_us: sink.clock.relative_us(),
                                    channel_index,
                                    kind: PacketKind::TcpConnect,
                                    direction: 0,
                                    flags: 0,
                                    aux: tcp_peer_aux(peer),
                                    payload: Vec::new(),
                                }
                            )?;
                            s
                        }
                        None => return Ok(0),
                    };
                    run_tcp_data_loop(sink, channel_index, stream, stop, max_duration)
                }
                TcpRole::Client => {
                    let addr = match config.socket_addr() {
                        Ok(a) => a,
                        Err(e) => {
                            mark_fail(&e);
                            return Err(e);
                        }
                    };
                    let stream = match connect(addr, poll_timeout) {
                        Ok(s) => s,
                        Err(e) => {
                            mark_fail(&e);
                            return Err(e);
                        }
                    };
                    let peer = stream.peer_addr().unwrap_or(addr);
                    ready_ok.fetch_add(1, Ordering::SeqCst);
                    eprintln!(
                        "录制中（多路 TCP client）：{name} {}:{}",
                        config.host, config.port
                    );
                    sink.append(
                        &PacketRecord {
                            relative_us: sink.clock.relative_us(),
                            channel_index,
                            kind: PacketKind::TcpConnect,
                            direction: 1,
                            flags: 0,
                            aux: tcp_peer_aux(peer),
                            payload: Vec::new(),
                        }
                    )?;
                    run_tcp_data_loop(sink, channel_index, stream, stop, max_duration)
                }
            }
        }
        ChannelJob::Mqtt { config, name } => {
            let mut client = match MqttClient::connect(config, poll_timeout) {
                Ok(c) => c,
                Err(e) => {
                    mark_fail(&e);
                    return Err(e);
                }
            };
            if let Err(e) = client.subscribe(&config.topic, config.qos) {
                mark_fail(&e);
                return Err(e);
            }
            ready_ok.fetch_add(1, Ordering::SeqCst);
            eprintln!(
                "录制中（多路 MQTT）：{name} {}:{} topic={}",
                config.broker, config.port, config.topic
            );
            let mut count = 0u64;
            while !stop.load(Ordering::SeqCst) {
                if let Some(max) = max_duration {
                    if sink.clock.relative_us() >= max.as_micros() as u64 {
                        break;
                    }
                }
                match client.poll_publish(stop)? {
                    Some(msg) => {
                        let pkt = PacketRecord {
                            relative_us: sink.clock.relative_us(),
                            channel_index,
                            kind: PacketKind::Data,
                            direction: 0,
                            flags: pack_mqtt_flags(msg.qos, msg.retain),
                            aux: msg.topic.into_bytes(),
                            payload: msg.payload,
                        };
                        sink.append(&pkt)?;
                        count += 1;
                    }
                    None => continue,
                }
            }
            let _ = client.disconnect();
            Ok(count)
        }
    }
}

fn run_datagram_loop(
    sink: &LiveRecordSink,
    channel_index: u16,
    sock: &std::net::UdpSocket,
    stop: &AtomicBool,
    max_duration: Option<Duration>,
) -> io::Result<u64> {
    let mut buf = vec![0u8; 65535];
    let mut count = 0u64;
    while !stop.load(Ordering::SeqCst) {
        if let Some(max) = max_duration {
            if sink.clock.relative_us() >= max.as_micros() as u64 {
                break;
            }
        }
        match recv_one(sock, &mut buf)? {
            Some((n, peer)) => {
                let pkt = PacketRecord {
                    relative_us: sink.clock.relative_us(),
                    channel_index,
                    kind: PacketKind::Data,
                    direction: 0,
                    flags: 0,
                    aux: peer_aux(peer),
                    payload: buf[..n].to_vec(),
                };
                sink.append(&pkt)?;
                count += 1;
            }
            None => continue,
        }
    }
    Ok(count)
}

fn run_tcp_data_loop(
    sink: &LiveRecordSink,
    channel_index: u16,
    stream: std::net::TcpStream,
    stop: &AtomicBool,
    max_duration: Option<Duration>,
) -> io::Result<u64> {
    let mut buf = vec![0u8; 65535];
    let mut count = 1u64; // 已写 TcpConnect
    let mut eof = false;
    while !stop.load(Ordering::SeqCst) && !eof {
        if let Some(max) = max_duration {
            if sink.clock.relative_us() >= max.as_micros() as u64 {
                break;
            }
        }
        match read_some(&stream, &mut buf)? {
            ReadOutcome::Data(n) => {
                sink.append(
                    &PacketRecord {
                        relative_us: sink.clock.relative_us(),
                        channel_index,
                        kind: PacketKind::Data,
                        direction: 0,
                        flags: 0,
                        aux: Vec::new(),
                        payload: buf[..n].to_vec(),
                    }
                )?;
                count += 1;
            }
            ReadOutcome::Timeout => continue,
            ReadOutcome::Eof => {
                eof = true;
                sink.append(
                    &PacketRecord {
                        relative_us: sink.clock.relative_us(),
                        channel_index,
                        kind: PacketKind::TcpDisconnect,
                        direction: 0,
                        flags: 0,
                        aux: b"eof".to_vec(),
                        payload: Vec::new(),
                    }
                )?;
                count += 1;
            }
        }
    }
    if !eof {
        sink.append(
            &PacketRecord {
                relative_us: sink.clock.relative_us(),
                channel_index,
                kind: PacketKind::TcpDisconnect,
                direction: 1,
                flags: 0,
                aux: b"stop".to_vec(),
                payload: Vec::new(),
            }
        )?;
        count += 1;
        drop(stream);
    }
    Ok(count)
}

fn json_string(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 最小自检：两路 UDP 并发录制。
pub fn self_check_multi_udp() -> io::Result<()> {
    use std::net::UdpSocket;

    let a = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let b = {
        let s = UdpSocket::bind("127.0.0.1:0")?;
        s.local_addr()?
    };
    let out = std::env::temp_dir().join(format!(
        "fieldreplay_multi_{}.fcap",
        std::process::id()
    ));
    let stop = Arc::new(AtomicBool::new(false));
    let stop_c = Arc::clone(&stop);
    let out_c = out.clone();

    let rec = thread::spawn(move || {
        record_multi_to_file(
            &MultiRecordOptions {
                out_path: out_c,
                session_title: Some("multi-udp".into()),
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

    let mut reader = crate::fcap::SessionReader::open(&out)?;
    let header = reader.header().clone();
    if header.channel_count != 2 {
        let _ = std::fs::remove_file(&out);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("通道数期望 2，实际 {}", header.channel_count),
        ));
    }
    let packets = reader.read_all_packets()?;
    let mut saw0 = false;
    let mut saw1 = false;
    for p in &packets {
        if p.channel_index == 0 && p.kind == PacketKind::Data {
            saw0 = true;
        }
        if p.channel_index == 1 && p.kind == PacketKind::Data {
            saw1 = true;
        }
    }
    let _ = std::fs::remove_file(&out);
    if !saw0 || !saw1 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("两路均应有数据帧 saw0={saw0} saw1={saw1}"),
        ));
    }
    eprintln!("  多路 UDP 并发录制自检：通过（{n} 帧）");
    Ok(())
}
