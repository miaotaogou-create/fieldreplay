//! 按 `relative_us` 将数据帧发往目标（UDP / 串口 / TCP / MQTT）。

use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::clock::SessionClock;
use crate::fcap::{ChannelType, PacketKind, SessionReader};
use crate::mqtt::{
    apply_topic_prefix, mqtt_qos_from_flags, mqtt_retain_from_flags, MqttClient, MqttConfig,
};
use crate::serial::{SerialConfig, SerialPort};
use crate::tcp::{accept_until, connect, listen, write_all, TcpConfig, TcpRole};
use crate::udp::{bind_sender, send_to};

/// 每发出一帧数据后的进度回调（`sent` 为已发数据帧数，`relative_us` 为该帧时戳）。
pub type OnReplayProgress<'a> = Option<&'a mut dyn FnMut(u64, u64)>;

pub struct ReplayOptions {
    pub in_path: std::path::PathBuf,
    pub target: SocketAddr,
    /// 发送端本地 bind（默认 `0.0.0.0:0`）
    pub local_bind: SocketAddr,
    /// 墙钟倍速；1.0=实时。非法值按 1.0。
    pub speed: f64,
}

pub fn replay_udp_from_file(opts: &ReplayOptions) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let mut on: OnReplayProgress<'_> = None;
    replay_udp_from_file_with(opts, &stop, &mut on)
}

pub fn replay_udp_from_file_with(
    opts: &ReplayOptions,
    stop: &AtomicBool,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    let mut reader = SessionReader::open(&opts.in_path)?;
    if !reader.header().is_finalized() {
        eprintln!("警告：文件未标记 FINALIZED，将扫到 EOF");
    }

    let has_udp = reader
        .channels()
        .iter()
        .any(|c| c.channel_type == ChannelType::Udp);
    if !has_udp {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "会话中无 UDP 通道",
        ));
    }

    let sock = bind_sender(opts.local_bind)?;
    let base = Instant::now();
    let mut sent = 0u64;

    let speed = SessionClock::clamp_speed(opts.speed);
    eprintln!(
        "重放中：{} → {}（按 relative_us，{}x）",
        opts.in_path.display(),
        opts.target,
        speed
    );

    reader.rewind_packets()?;
    while let Some(pkt) = reader.next_packet()? {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if pkt.kind != PacketKind::Data {
            continue;
        }
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Udp) {
            continue;
        }
        if !SessionClock::sleep_until_relative_interruptible(base, pkt.relative_us, speed, stop) {
            break;
        }
        send_to(&sock, &pkt.payload, opts.target)?;
        sent += 1;
        if let Some(cb) = on_progress.as_mut() {
            cb(sent, pkt.relative_us);
        }
    }

    eprintln!("重放结束：发送 {sent} 帧");
    Ok(sent)
}

pub fn replay_udp_path(
    path: impl AsRef<Path>,
    target: SocketAddr,
) -> io::Result<u64> {
    let opts = ReplayOptions {
        in_path: path.as_ref().to_path_buf(),
        target,
        local_bind: "0.0.0.0:0".parse().unwrap(),
        speed: 1.0,
    };
    replay_udp_from_file(&opts)
}

pub struct SerialReplayOptions {
    pub in_path: std::path::PathBuf,
    pub config: SerialConfig,
    /// 墙钟倍速；1.0=实时。
    pub speed: f64,
}

pub fn replay_serial_from_file(opts: &SerialReplayOptions) -> io::Result<u64> {
    let port = SerialPort::open(
        &opts.config,
        std::time::Duration::from_millis(200),
    )?;
    let stop = AtomicBool::new(false);
    let mut on: OnReplayProgress<'_> = None;
    replay_serial_with_port_with(opts, &port, &stop, &mut on)
}

pub fn replay_serial_with_port(
    opts: &SerialReplayOptions,
    port: &SerialPort,
) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let mut on: OnReplayProgress<'_> = None;
    replay_serial_with_port_with(opts, port, &stop, &mut on)
}

pub fn replay_serial_with_port_with(
    opts: &SerialReplayOptions,
    port: &SerialPort,
    stop: &AtomicBool,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    let mut reader = SessionReader::open(&opts.in_path)?;
    if !reader.header().is_finalized() {
        eprintln!("警告：文件未标记 FINALIZED，将扫到 EOF");
    }

    let has_serial = reader
        .channels()
        .iter()
        .any(|c| c.channel_type == ChannelType::Serial);
    if !has_serial {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "会话中无串口通道",
        ));
    }

    let base = Instant::now();
    let mut sent = 0u64;
    let speed = SessionClock::clamp_speed(opts.speed);

    eprintln!(
        "重放中：{} → 串口 {}（按 relative_us，{}x）",
        opts.in_path.display(),
        opts.config.port_name,
        speed
    );

    reader.rewind_packets()?;
    while let Some(pkt) = reader.next_packet()? {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if pkt.kind != PacketKind::Data {
            continue;
        }
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Serial) {
            continue;
        }
        if !SessionClock::sleep_until_relative_interruptible(base, pkt.relative_us, speed, stop) {
            break;
        }
        port.write_all(&pkt.payload)?;
        sent += 1;
        if let Some(cb) = on_progress.as_mut() {
            cb(sent, pkt.relative_us);
        }
    }

    eprintln!("重放结束：发送 {sent} 帧");
    Ok(sent)
}

pub struct TcpReplayOptions {
    pub in_path: std::path::PathBuf,
    /// 重放侧地址：角色取反后，server→bind，client→connect。
    pub endpoint: SocketAddr,
    pub poll_timeout: Duration,
    /// 墙钟倍速；1.0=实时。
    pub speed: f64,
}

/// 按会话内录制角色取反后建立连接，并把录到的 RX 数据按时间轴写回对端。
pub fn replay_tcp_from_file(
    opts: &TcpReplayOptions,
    stop: &AtomicBool,
) -> io::Result<u64> {
    let mut on: OnReplayProgress<'_> = None;
    replay_tcp_from_file_with(opts, stop, &mut on)
}

pub fn replay_tcp_from_file_with(
    opts: &TcpReplayOptions,
    stop: &AtomicBool,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    let mut reader = SessionReader::open(&opts.in_path)?;
    if !reader.header().is_finalized() {
        eprintln!("警告：文件未标记 FINALIZED，将扫到 EOF");
    }

    let recorded = reader
        .channels()
        .iter()
        .find(|c| c.channel_type == ChannelType::Tcp)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "会话中无 TCP 通道")
        })?;
    let recorded_cfg = TcpConfig::from_config_json(&recorded.config_json)?;
    let play_role = recorded_cfg.role.invert();

    let speed = SessionClock::clamp_speed(opts.speed);
    eprintln!(
        "重放中：{} → TCP {} {}（录制角色 {}，按 relative_us，{}x）",
        opts.in_path.display(),
        play_role.as_str(),
        opts.endpoint,
        recorded_cfg.role.as_str(),
        speed
    );

    let base = Instant::now();
    let mut sent = 0u64;
    let mut stream = None;

    reader.rewind_packets()?;
    while let Some(pkt) = reader.next_packet()? {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Tcp) {
            continue;
        }

        if !SessionClock::sleep_until_relative_interruptible(base, pkt.relative_us, speed, stop) {
            break;
        }

        match pkt.kind {
            PacketKind::TcpConnect => {
                let s = match play_role {
                    TcpRole::Server => {
                        let listener = listen(opts.endpoint)?;
                        let local = listener.local_addr().unwrap_or(opts.endpoint);
                        eprintln!("TCP 重放等待上位机接入：{local}");
                        match accept_until(&listener, stop, opts.poll_timeout)? {
                            Some((s, peer)) => {
                                eprintln!("TCP 已接入：{peer}");
                                s
                            }
                            None => {
                                eprintln!("重放中止：等待连接时被停止");
                                break;
                            }
                        }
                    }
                    TcpRole::Client => {
                        eprintln!("TCP 重放连接：{}", opts.endpoint);
                        connect(opts.endpoint, opts.poll_timeout)?
                    }
                };
                stream = Some(s);
            }
            PacketKind::Data => {
                // 录制 RX = 设备→工具；重放演设备时写回对端（上位机）
                if pkt.direction != 0 {
                    continue;
                }
                let Some(ref s) = stream else {
                    eprintln!("警告：未连接就遇到数据帧，已跳过");
                    continue;
                };
                write_all(s, &pkt.payload)?;
                sent += 1;
                if let Some(cb) = on_progress.as_mut() {
                    cb(sent, pkt.relative_us);
                }
            }
            PacketKind::TcpDisconnect => {
                stream = None;
            }
            PacketKind::Marker => {}
        }
    }

    eprintln!("重放结束：发送 {sent} 帧");
    Ok(sent)
}

pub struct MqttReplayOptions {
    pub in_path: std::path::PathBuf,
    /// 重放侧 Broker；若文件内有 mqtt 配置，host/port 可被本项覆盖。
    pub broker: SocketAddr,
    /// 拼到录制 topic 前（可空）。
    pub topic_prefix: String,
    /// 发布 QoS：`None` = 用帧 flags 里录到的值。
    pub qos_override: Option<u8>,
    pub poll_timeout: Duration,
    /// 墙钟倍速；1.0=实时。
    pub speed: f64,
}

/// 连接 Broker，按时间轴把 MQTT 数据帧 PUBLISH 出去（演设备/传感器）。
pub fn replay_mqtt_from_file(opts: &MqttReplayOptions) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let mut on: OnReplayProgress<'_> = None;
    replay_mqtt_from_file_with(opts, &stop, &mut on)
}

pub fn replay_mqtt_from_file_with(
    opts: &MqttReplayOptions,
    stop: &AtomicBool,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    let mut reader = SessionReader::open(&opts.in_path)?;
    if !reader.header().is_finalized() {
        eprintln!("警告：文件未标记 FINALIZED，将扫到 EOF");
    }

    let recorded = reader
        .channels()
        .iter()
        .find(|c| c.channel_type == ChannelType::Mqtt)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "会话中无 MQTT 通道")
        })?;
    let recorded_cfg = MqttConfig::from_config_json(&recorded.config_json)?;

    let config = MqttConfig::new(
        opts.broker.ip().to_string(),
        opts.broker.port(),
        recorded_cfg.topic.clone(),
        recorded_cfg.qos,
    );

    let speed = SessionClock::clamp_speed(opts.speed);
    eprintln!(
        "重放中：{} → MQTT {}:{} prefix={:?}（按 relative_us，{}x）",
        opts.in_path.display(),
        config.broker,
        config.port,
        opts.topic_prefix,
        speed
    );

    let mut client = MqttClient::connect(&config, opts.poll_timeout)?;
    let base = Instant::now();
    let mut sent = 0u64;

    reader.rewind_packets()?;
    while let Some(pkt) = reader.next_packet()? {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if pkt.kind != PacketKind::Data {
            continue;
        }
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Mqtt) {
            continue;
        }
        let topic = String::from_utf8_lossy(&pkt.aux);
        let topic = apply_topic_prefix(&opts.topic_prefix, &topic);
        let qos = opts
            .qos_override
            .unwrap_or_else(|| mqtt_qos_from_flags(pkt.flags));
        let retain = mqtt_retain_from_flags(pkt.flags);

        if !SessionClock::sleep_until_relative_interruptible(base, pkt.relative_us, speed, stop) {
            break;
        }
        client.publish(&topic, &pkt.payload, qos, retain)?;
        sent += 1;
        if let Some(cb) = on_progress.as_mut() {
            cb(sent, pkt.relative_us);
        }
    }

    let _ = client.disconnect();
    eprintln!("重放结束：发布 {sent} 帧");
    Ok(sent)
}
