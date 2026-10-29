//! 录制会话：收包 → 打戳 → 追加 `.fcap`（UDP / 串口 / TCP）。

use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::clock::SessionClock;
use crate::fcap::{
    ChannelEntry, ChannelType, PacketKind, PacketRecord, SessionWriter,
};
use crate::serial::{SerialConfig, SerialPort};
use crate::tcp::{
    accept_until, connect, listen, peer_aux as tcp_peer_aux, read_some, ReadOutcome, TcpConfig,
    TcpRole,
};
use crate::udp::{bind_receiver, peer_aux, recv_one};

pub struct RecordOptions {
    pub bind: SocketAddr,
    pub out_path: std::path::PathBuf,
    pub channel_name: String,
    pub session_title: Option<String>,
    /// `None` = 直到 `stop` 为 true
    pub max_duration: Option<Duration>,
    /// 读超时，便于轮询 stop / 时长
    pub poll_timeout: Duration,
}

/// 录制直到 `stop` 置位或超过 `max_duration`。
pub fn record_udp_to_file(opts: &RecordOptions, stop: &AtomicBool) -> io::Result<u64> {
    let clock = SessionClock::start();
    let config_json = format!(
        r#"{{"bind_ip":"{}","bind_port":{},"peer_ip":"","peer_port":0}}"#,
        opts.bind.ip(),
        opts.bind.port()
    );
    let channels = [ChannelEntry {
        channel_index: 0,
        channel_type: ChannelType::Udp,
        name: opts.channel_name.clone(),
        config_json,
    }];
    let meta = opts.session_title.as_ref().map(|t| {
        format!(r#"{{"session_title":{}}}"#, json_string(t))
    });

    let mut writer = SessionWriter::create(
        &opts.out_path,
        clock.start_wall_unix_us(),
        &channels,
        meta.as_deref(),
    )?;

    let sock = bind_receiver(opts.bind, Some(opts.poll_timeout))?;
    let mut buf = vec![0u8; 65535];

    eprintln!(
        "录制中：bind {} → {}",
        opts.bind,
        opts.out_path.display()
    );

    while !stop.load(Ordering::SeqCst) {
        if let Some(max) = opts.max_duration {
            if clock.relative_us() >= max.as_micros() as u64 {
                break;
            }
        }
        match recv_one(&sock, &mut buf)? {
            Some((n, peer)) => {
                let pkt = PacketRecord {
                    relative_us: clock.relative_us(),
                    channel_index: 0,
                    kind: PacketKind::Data,
                    direction: 0, // RX
                    flags: 0,
                    aux: peer_aux(peer),
                    payload: buf[..n].to_vec(),
                };
                writer.append_packet(&pkt)?;
            }
            None => continue,
        }
    }

    let count = writer.packet_count();
    writer.finalize()?;
    eprintln!("录制结束：{count} 帧 → {}", opts.out_path.display());
    Ok(count)
}

/// 便捷：无外部 stop，仅按时长录制。
pub fn record_udp_for_duration(
    bind: SocketAddr,
    out: impl AsRef<Path>,
    duration: Duration,
) -> io::Result<u64> {
    let stop = Arc::new(AtomicBool::new(false));
    let opts = RecordOptions {
        bind,
        out_path: out.as_ref().to_path_buf(),
        channel_name: format!("UDP-{}", bind.port()),
        session_title: Some(format!("udp-{}", bind.port())),
        max_duration: Some(duration),
        poll_timeout: Duration::from_millis(200),
    };
    record_udp_to_file(&opts, &stop)
}

pub struct SerialRecordOptions {
    pub config: SerialConfig,
    pub out_path: std::path::PathBuf,
    pub channel_name: String,
    pub session_title: Option<String>,
    pub max_duration: Option<Duration>,
    pub poll_timeout: Duration,
}

/// 打开实口并录制，直到 `stop` 或超过 `max_duration`。
pub fn record_serial_to_file(
    opts: &SerialRecordOptions,
    stop: &AtomicBool,
) -> io::Result<u64> {
    let port = SerialPort::open(&opts.config, opts.poll_timeout)?;
    record_serial_with_port(opts, stop, &port)
}

/// 在已打开端口上录制（实口或自检环回）。
pub fn record_serial_with_port(
    opts: &SerialRecordOptions,
    stop: &AtomicBool,
    port: &SerialPort,
) -> io::Result<u64> {
    let clock = SessionClock::start();
    let channels = [ChannelEntry {
        channel_index: 0,
        channel_type: ChannelType::Serial,
        name: opts.channel_name.clone(),
        config_json: opts.config.to_config_json(),
    }];
    let meta = opts.session_title.as_ref().map(|t| {
        format!(r#"{{"session_title":{}}}"#, json_string(t))
    });

    let mut writer = SessionWriter::create(
        &opts.out_path,
        clock.start_wall_unix_us(),
        &channels,
        meta.as_deref(),
    )?;

    eprintln!(
        "录制中：串口 {} @ {} → {}",
        opts.config.port_name,
        opts.config.baud,
        opts.out_path.display()
    );

    let mut buf = vec![0u8; 4096];
    while !stop.load(Ordering::SeqCst) {
        if let Some(max) = opts.max_duration {
            if clock.relative_us() >= max.as_micros() as u64 {
                break;
            }
        }
        match port.read(&mut buf)? {
            Some(n) if n > 0 => {
                let pkt = PacketRecord {
                    relative_us: clock.relative_us(),
                    channel_index: 0,
                    kind: PacketKind::Data,
                    direction: 0, // RX
                    flags: 0,
                    aux: Vec::new(),
                    payload: buf[..n].to_vec(),
                };
                writer.append_packet(&pkt)?;
            }
            _ => continue,
        }
    }

    let count = writer.packet_count();
    writer.finalize()?;
    eprintln!("录制结束：{count} 帧 → {}", opts.out_path.display());
    Ok(count)
}

pub struct TcpRecordOptions {
    pub config: TcpConfig,
    pub out_path: std::path::PathBuf,
    pub channel_name: String,
    pub session_title: Option<String>,
    pub max_duration: Option<Duration>,
    pub poll_timeout: Duration,
}

/// 按配置角色建立 TCP 连接并录制（连接事件 + RX 字节流），直到 `stop` / 时长 / 对端断开。
pub fn record_tcp_to_file(opts: &TcpRecordOptions, stop: &AtomicBool) -> io::Result<u64> {
    let clock = SessionClock::start();
    let channels = [ChannelEntry {
        channel_index: 0,
        channel_type: ChannelType::Tcp,
        name: opts.channel_name.clone(),
        config_json: opts.config.to_config_json(),
    }];
    let meta = opts.session_title.as_ref().map(|t| {
        format!(r#"{{"session_title":{}}}"#, json_string(t))
    });

    let mut writer = SessionWriter::create(
        &opts.out_path,
        clock.start_wall_unix_us(),
        &channels,
        meta.as_deref(),
    )?;

    eprintln!(
        "录制中：TCP {} {}:{} → {}",
        opts.config.role.as_str(),
        opts.config.host,
        opts.config.port,
        opts.out_path.display()
    );

    let stream = match opts.config.role {
        TcpRole::Server => {
            let bind = opts.config.socket_addr()?;
            let listener = listen(bind)?;
            let local = listener.local_addr().unwrap_or(bind);
            eprintln!("TCP 等待接入：{local}");
            match accept_until(&listener, stop, opts.poll_timeout)? {
                Some((stream, peer)) => {
                    writer.append_packet(&PacketRecord {
                        relative_us: clock.relative_us(),
                        channel_index: 0,
                        kind: PacketKind::TcpConnect,
                        direction: 0,
                        flags: 0,
                        aux: tcp_peer_aux(peer),
                        payload: Vec::new(),
                    })?;
                    stream
                }
                None => {
                    let count = writer.packet_count();
                    writer.finalize()?;
                    eprintln!("录制结束（未建立连接）：{count} 帧 → {}", opts.out_path.display());
                    return Ok(count);
                }
            }
        }
        TcpRole::Client => {
            let addr = opts.config.socket_addr()?;
            let stream = connect(addr, opts.poll_timeout)?;
            let peer = stream.peer_addr().unwrap_or(addr);
            writer.append_packet(&PacketRecord {
                relative_us: clock.relative_us(),
                channel_index: 0,
                kind: PacketKind::TcpConnect,
                direction: 1, // 工具主动连出
                flags: 0,
                aux: tcp_peer_aux(peer),
                payload: Vec::new(),
            })?;
            stream
        }
    };

    let mut buf = vec![0u8; 65535];
    let mut eof = false;
    while !stop.load(Ordering::SeqCst) && !eof {
        if let Some(max) = opts.max_duration {
            if clock.relative_us() >= max.as_micros() as u64 {
                break;
            }
        }
        match read_some(&stream, &mut buf)? {
            ReadOutcome::Data(n) => {
                writer.append_packet(&PacketRecord {
                    relative_us: clock.relative_us(),
                    channel_index: 0,
                    kind: PacketKind::Data,
                    direction: 0, // RX
                    flags: 0,
                    aux: Vec::new(),
                    payload: buf[..n].to_vec(),
                })?;
            }
            ReadOutcome::Timeout => continue,
            ReadOutcome::Eof => {
                eof = true;
                writer.append_packet(&PacketRecord {
                    relative_us: clock.relative_us(),
                    channel_index: 0,
                    kind: PacketKind::TcpDisconnect,
                    direction: 0,
                    flags: 0,
                    aux: b"eof".to_vec(),
                    payload: Vec::new(),
                })?;
            }
        }
    }

    if !eof {
        writer.append_packet(&PacketRecord {
            relative_us: clock.relative_us(),
            channel_index: 0,
            kind: PacketKind::TcpDisconnect,
            direction: 1,
            flags: 0,
            aux: b"stop".to_vec(),
            payload: Vec::new(),
        })?;
        // 主动收尾：丢弃 stream 以关闭连接
        drop(stream);
    }

    let count = writer.packet_count();
    writer.finalize()?;
    eprintln!("录制结束：{count} 帧 → {}", opts.out_path.display());
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
