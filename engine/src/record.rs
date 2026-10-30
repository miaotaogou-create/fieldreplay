//! 录制会话：收包 → 打戳 → 追加 `.fcap`（UDP / 视频·RTP·UDP / 串口 / TCP / MQTT）。

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
use crate::mqtt::{pack_mqtt_flags, MqttClient, MqttConfig};
use crate::serial::{SerialConfig, SerialPort};
use crate::tcp::{
    accept_until, connect, listen, peer_aux as tcp_peer_aux, read_some, ReadOutcome, TcpConfig,
    TcpRole,
};
use crate::udp::{bind_receiver, peer_aux, recv_one};

/// 每写入一帧时可选回调（控制面预览推送）。
pub type OnPacket<'a> = Option<&'a mut dyn FnMut(&PacketRecord)>;

fn write_packet(
    writer: &mut SessionWriter,
    pkt: &PacketRecord,
    on_packet: &mut OnPacket<'_>,
) -> io::Result<()> {
    writer.append_packet(pkt)?;
    if let Some(cb) = on_packet.as_mut() {
        cb(pkt);
    }
    Ok(())
}

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
    let mut none: OnPacket<'_> = None;
    record_udp_to_file_with(opts, stop, &mut none)
}

pub fn record_udp_to_file_with(
    opts: &RecordOptions,
    stop: &AtomicBool,
    on_packet: &mut OnPacket<'_>,
) -> io::Result<u64> {
    record_datagram_to_file_with(opts, ChannelType::Udp, None, stop, on_packet)
}

/// 视频通道（E7a）：UDP 数据报热路径，通道类型写 `video`；`rtsp_url` 仅进 config 备注。
pub fn record_video_to_file(opts: &RecordOptions, stop: &AtomicBool) -> io::Result<u64> {
    let mut none: OnPacket<'_> = None;
    record_video_to_file_with(opts, None, stop, &mut none)
}

pub fn record_video_to_file_with(
    opts: &RecordOptions,
    rtsp_url: Option<&str>,
    stop: &AtomicBool,
    on_packet: &mut OnPacket<'_>,
) -> io::Result<u64> {
    record_datagram_to_file_with(opts, ChannelType::Video, rtsp_url, stop, on_packet)
}

fn record_datagram_to_file_with(
    opts: &RecordOptions,
    channel_type: ChannelType,
    rtsp_url: Option<&str>,
    stop: &AtomicBool,
    on_packet: &mut OnPacket<'_>,
) -> io::Result<u64> {
    let clock = SessionClock::start();
    let config_json = match channel_type {
        ChannelType::Video => format!(
            r#"{{"bind_ip":"{}","bind_port":{},"peer_ip":"","peer_port":0,"media":"rtp_udp","rtsp_url":{}}}"#,
            opts.bind.ip(),
            opts.bind.port(),
            json_string(rtsp_url.unwrap_or(""))
        ),
        _ => format!(
            r#"{{"bind_ip":"{}","bind_port":{},"peer_ip":"","peer_port":0}}"#,
            opts.bind.ip(),
            opts.bind.port()
        ),
    };
    let channels = [ChannelEntry {
        channel_index: 0,
        channel_type,
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
    let label = match channel_type {
        ChannelType::Video => "视频(RTP/UDP)",
        _ => "UDP",
    };

    eprintln!(
        "录制中（{label}）：bind {} → {}",
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
                write_packet(&mut writer, &pkt, on_packet)?;
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
    let mut none: OnPacket<'_> = None;
    record_serial_to_file_with(opts, stop, &mut none)
}

pub fn record_serial_to_file_with(
    opts: &SerialRecordOptions,
    stop: &AtomicBool,
    on_packet: &mut OnPacket<'_>,
) -> io::Result<u64> {
    let port = SerialPort::open(&opts.config, opts.poll_timeout)?;
    record_serial_with_port_with(opts, stop, &port, on_packet)
}

/// 在已打开端口上录制（实口或自检环回）。
pub fn record_serial_with_port(
    opts: &SerialRecordOptions,
    stop: &AtomicBool,
    port: &SerialPort,
) -> io::Result<u64> {
    let mut none: OnPacket<'_> = None;
    record_serial_with_port_with(opts, stop, port, &mut none)
}

pub fn record_serial_with_port_with(
    opts: &SerialRecordOptions,
    stop: &AtomicBool,
    port: &SerialPort,
    on_packet: &mut OnPacket<'_>,
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
                write_packet(&mut writer, &pkt, on_packet)?;
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
    let mut none: OnPacket<'_> = None;
    record_tcp_to_file_with(opts, stop, &mut none)
}

pub fn record_tcp_to_file_with(
    opts: &TcpRecordOptions,
    stop: &AtomicBool,
    on_packet: &mut OnPacket<'_>,
) -> io::Result<u64> {
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
                    write_packet(
                        &mut writer,
                        &PacketRecord {
                            relative_us: clock.relative_us(),
                            channel_index: 0,
                            kind: PacketKind::TcpConnect,
                            direction: 0,
                            flags: 0,
                            aux: tcp_peer_aux(peer),
                            payload: Vec::new(),
                        },
                        on_packet,
                    )?;
                    stream
                }
                None => {
                    let count = writer.packet_count();
                    writer.finalize()?;
                    eprintln!(
                        "录制结束（未建立连接）：{count} 帧 → {}",
                        opts.out_path.display()
                    );
                    return Ok(count);
                }
            }
        }
        TcpRole::Client => {
            let addr = opts.config.socket_addr()?;
            let stream = connect(addr, opts.poll_timeout)?;
            let peer = stream.peer_addr().unwrap_or(addr);
            write_packet(
                &mut writer,
                &PacketRecord {
                    relative_us: clock.relative_us(),
                    channel_index: 0,
                    kind: PacketKind::TcpConnect,
                    direction: 1, // 工具主动连出
                    flags: 0,
                    aux: tcp_peer_aux(peer),
                    payload: Vec::new(),
                },
                on_packet,
            )?;
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
                write_packet(
                    &mut writer,
                    &PacketRecord {
                        relative_us: clock.relative_us(),
                        channel_index: 0,
                        kind: PacketKind::Data,
                        direction: 0, // RX
                        flags: 0,
                        aux: Vec::new(),
                        payload: buf[..n].to_vec(),
                    },
                    on_packet,
                )?;
            }
            ReadOutcome::Timeout => continue,
            ReadOutcome::Eof => {
                eof = true;
                write_packet(
                    &mut writer,
                    &PacketRecord {
                        relative_us: clock.relative_us(),
                        channel_index: 0,
                        kind: PacketKind::TcpDisconnect,
                        direction: 0,
                        flags: 0,
                        aux: b"eof".to_vec(),
                        payload: Vec::new(),
                    },
                    on_packet,
                )?;
            }
        }
    }

    if !eof {
        write_packet(
            &mut writer,
            &PacketRecord {
                relative_us: clock.relative_us(),
                channel_index: 0,
                kind: PacketKind::TcpDisconnect,
                direction: 1,
                flags: 0,
                aux: b"stop".to_vec(),
                payload: Vec::new(),
            },
            on_packet,
        )?;
        // 主动收尾：丢弃 stream 以关闭连接
        drop(stream);
    }

    let count = writer.packet_count();
    writer.finalize()?;
    eprintln!("录制结束：{count} 帧 → {}", opts.out_path.display());
    Ok(count)
}

pub struct MqttRecordOptions {
    pub config: MqttConfig,
    pub out_path: std::path::PathBuf,
    pub channel_name: String,
    pub session_title: Option<String>,
    pub max_duration: Option<Duration>,
    pub poll_timeout: Duration,
}

/// 连接 Broker 订阅，把入站 PUBLISH 落盘（topic→aux，payload→payload）。
pub fn record_mqtt_to_file(opts: &MqttRecordOptions, stop: &AtomicBool) -> io::Result<u64> {
    let mut none: OnPacket<'_> = None;
    record_mqtt_to_file_with(opts, stop, &mut none)
}

pub fn record_mqtt_to_file_with(
    opts: &MqttRecordOptions,
    stop: &AtomicBool,
    on_packet: &mut OnPacket<'_>,
) -> io::Result<u64> {
    let clock = SessionClock::start();
    let channels = [ChannelEntry {
        channel_index: 0,
        channel_type: ChannelType::Mqtt,
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
        "录制中：MQTT {}:{} topic={} qos={} → {}",
        opts.config.broker,
        opts.config.port,
        opts.config.topic,
        opts.config.qos,
        opts.out_path.display()
    );

    let mut client = MqttClient::connect(&opts.config, opts.poll_timeout)?;
    client.subscribe(&opts.config.topic, opts.config.qos)?;

    while !stop.load(Ordering::SeqCst) {
        if let Some(max) = opts.max_duration {
            if clock.relative_us() >= max.as_micros() as u64 {
                break;
            }
        }
        match client.poll_publish(stop)? {
            Some(msg) => {
                write_packet(
                    &mut writer,
                    &PacketRecord {
                        relative_us: clock.relative_us(),
                        channel_index: 0,
                        kind: PacketKind::Data,
                        direction: 0, // RX
                        flags: pack_mqtt_flags(msg.qos, msg.retain),
                        aux: msg.topic.into_bytes(),
                        payload: msg.payload,
                    },
                    on_packet,
                )?;
            }
            None => continue,
        }
    }

    let _ = client.disconnect();
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
