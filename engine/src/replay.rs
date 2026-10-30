//! 按 `relative_us` 将数据帧发往目标（UDP / 视频·RTP·UDP|RTSP / 串口 / TCP / MQTT）。

use std::borrow::Cow;
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::clock::{SessionClock, WaitResult};
use crate::fcap::{ChannelType, PacketKind, SessionReader};
use crate::mqtt::{
    apply_topic_prefix, mqtt_qos_from_flags, mqtt_retain_from_flags, MqttClient, MqttConfig,
};
use crate::rtsp::{accept_until_play, encode_interleaved, poll_client_signal};
use crate::serial::{SerialConfig, SerialPort};
use crate::tcp::{accept_until, connect, listen, write_all, TcpConfig, TcpRole};
use crate::udp::{bind_sender, send_to};

/// 每发出一帧数据后的进度回调（`sent` 为已发数据帧数，`relative_us` 为该帧时戳）。
pub type OnReplayProgress<'a> = Option<&'a mut dyn FnMut(u64, u64)>;

/// 重放故障注入：随机丢包 / 篡改载荷（不解析业务 CRC，只扰动原始字节）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaultInjection {
    pub enabled: bool,
    /// 0..=100；启用后按该概率丢弃本帧（不发线上）。
    pub loss_rate_percent: u8,
    /// 启用后约 20% 概率篡改载荷字节后再发出。
    pub corrupt_payload: bool,
}

impl FaultInjection {
    pub const OFF: Self = Self {
        enabled: false,
        loss_rate_percent: 0,
        corrupt_payload: false,
    };

    pub fn is_active(self) -> bool {
        self.enabled && (self.loss_rate_percent > 0 || self.corrupt_payload)
    }

    pub fn clamped(self) -> Self {
        Self {
            enabled: self.enabled,
            loss_rate_percent: self.loss_rate_percent.min(100),
            corrupt_payload: self.corrupt_payload,
        }
    }
}

impl Default for FaultInjection {
    fn default() -> Self {
        Self::OFF
    }
}

/// 简易 LCG（仅 std；自检可固定种子）。
pub struct FaultRng(u64);

impl FaultRng {
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15);
        Self::seeded(seed)
    }

    pub fn seeded(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        (self.0 >> 32) as u32
    }

    fn roll_percent(&mut self, percent: u32) -> bool {
        if percent == 0 {
            return false;
        }
        if percent >= 100 {
            return true;
        }
        self.next_u32() % 100 < percent
    }
}

/// `None` = 丢弃；`Some` = 发出（可能已篡改）。
pub fn apply_fault<'a>(
    payload: &'a [u8],
    fault: &FaultInjection,
    rng: &mut FaultRng,
) -> Option<Cow<'a, [u8]>> {
    let fault = fault.clamped();
    if !fault.enabled {
        return Some(Cow::Borrowed(payload));
    }
    if rng.roll_percent(u32::from(fault.loss_rate_percent)) {
        return None;
    }
    if fault.corrupt_payload && rng.roll_percent(20) {
        let mut buf = payload.to_vec();
        if !buf.is_empty() {
            let i = (rng.next_u32() as usize) % buf.len();
            buf[i] ^= 0xA5;
            let last = buf.len() - 1;
            buf[last] = buf[last].wrapping_add(1);
        }
        return Some(Cow::Owned(buf));
    }
    Some(Cow::Borrowed(payload))
}

fn emit_with_fault(
    payload: &[u8],
    fault: &FaultInjection,
    rng: &mut FaultRng,
    relative_us: u64,
    sent: &mut u64,
    on_progress: &mut OnReplayProgress<'_>,
    mut send: impl FnMut(&[u8]) -> io::Result<()>,
) -> io::Result<()> {
    match apply_fault(payload, fault, rng) {
        None => {
            // 丢包仍推进时间轴进度，便于 UI scrub；sent 不计
            if let Some(cb) = on_progress.as_mut() {
                cb(*sent, relative_us);
            }
            Ok(())
        }
        Some(bytes) => {
            send(bytes.as_ref())?;
            *sent += 1;
            if let Some(cb) = on_progress.as_mut() {
                cb(*sent, relative_us);
            }
            Ok(())
        }
    }
}

pub struct ReplayOptions {
    pub in_path: std::path::PathBuf,
    pub target: SocketAddr,
    /// 发送端本地 bind（默认 `0.0.0.0:0`）
    pub local_bind: SocketAddr,
    /// 墙钟倍速；1.0=实时。非法值按 1.0。
    pub speed: f64,
    pub fault: FaultInjection,
    /// 播完后回卷从头再放，直到 stop。
    pub looping: bool,
}

pub fn replay_udp_from_file(opts: &ReplayOptions) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    replay_udp_from_file_with(opts, &stop, &paused, &seek, 0, &mut on)
}

pub fn replay_udp_from_file_with(
    opts: &ReplayOptions,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    replay_datagram_from_file_with(
        opts,
        ChannelType::Udp,
        "UDP",
        stop,
        paused,
        seek_us,
        from_us,
        on_progress,
    )
}

pub fn replay_video_from_file(opts: &ReplayOptions) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    replay_video_from_file_with(opts, &stop, &paused, &seek, 0, &mut on)
}

pub fn replay_video_from_file_with(
    opts: &ReplayOptions,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    replay_datagram_from_file_with(
        opts,
        ChannelType::Video,
        "视频(RTP/UDP)",
        stop,
        paused,
        seek_us,
        from_us,
        on_progress,
    )
}

/// E7d：本机 RTSP 服务端，PLAY 后按时间轴交织吐块（不解 NAL）。
pub struct RtspReplayOptions {
    pub in_path: std::path::PathBuf,
    pub listen: SocketAddr,
    pub speed: f64,
    pub fault: FaultInjection,
    pub looping: bool,
}

pub fn replay_video_rtsp_from_file(opts: &RtspReplayOptions) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    replay_video_rtsp_from_file_with(opts, &stop, &paused, &seek, 0, &mut on)
}

pub fn replay_video_rtsp_from_file_with(
    opts: &RtspReplayOptions,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    let listener = TcpListener::bind(opts.listen)?;
    replay_video_rtsp_with_listener(
        listener,
        &opts.in_path,
        opts.speed,
        opts.fault,
        opts.looping,
        stop,
        paused,
        seek_us,
        from_us,
        on_progress,
    )
}

/// 使用已绑定的 listener（自检可先 `local_addr` 再启动客户端）。
pub fn replay_video_rtsp_with_listener(
    listener: TcpListener,
    in_path: &Path,
    speed: f64,
    fault: FaultInjection,
    looping: bool,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    let mut reader = SessionReader::open(in_path)?;
    if !reader.header().is_finalized() {
        eprintln!("警告：文件未标记 FINALIZED，将扫到 EOF");
    }
    if !reader
        .channels()
        .iter()
        .any(|c| c.channel_type == ChannelType::Video)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "会话中无视频通道",
        ));
    }

    let bound = listener.local_addr()?;
    let speed = SessionClock::clamp_speed(speed);
    eprintln!(
        "重放中（视频·RTSP 服务端）：{} → listen {}（等待 PLAY，{}x，fromUs={from_us}{}{}）",
        in_path.display(),
        bound,
        speed,
        if fault.clamped().is_active() {
            "，故障注入"
        } else {
            ""
        },
        if looping { "，循环" } else { "" }
    );

    let mut stream = accept_until_play(&listener, stop)?;
    eprintln!("RTSP 客户端已 PLAY，开始按时间轴吐块");

    let mut base = Instant::now();
    let mut skip_before_us = from_us;
    if from_us > 0 {
        SessionClock::rebase_timeline(&mut base, from_us, speed);
    }
    let mut sent = 0u64;
    let mut rng = FaultRng::new();
    let fault = fault.clamped();
    let mut pass_matched = 0u64;
    let mut sig_buf = Vec::new();

    reader.rewind_packets()?;
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match poll_client_signal(&mut stream, &mut sig_buf) {
            Ok(()) => {}
            Err(e)
                if e.kind() == io::ErrorKind::UnexpectedEof
                    || e.kind() == io::ErrorKind::ConnectionAborted
                    || e.kind() == io::ErrorKind::ConnectionReset
                    || e.kind() == io::ErrorKind::BrokenPipe =>
            {
                break;
            }
            Err(e) => return Err(e),
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
                looping,
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
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Video) {
            continue;
        }
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
        let write_ok = emit_with_fault(
            &pkt.payload,
            &fault,
            &mut rng,
            pkt.relative_us,
            &mut sent,
            on_progress,
            |bytes| {
                let frame = encode_interleaved(0, bytes);
                stream.write_all(&frame)
            },
        );
        if let Err(e) = write_ok {
            if e.kind() == io::ErrorKind::BrokenPipe
                || e.kind() == io::ErrorKind::ConnectionReset
                || e.kind() == io::ErrorKind::UnexpectedEof
            {
                break;
            }
            return Err(e);
        }
    }

    eprintln!("重放结束：发送 {sent} 帧");
    Ok(sent)
}

fn replay_datagram_from_file_with(
    opts: &ReplayOptions,
    expect: ChannelType,
    label: &str,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<u64> {
    let mut reader = SessionReader::open(&opts.in_path)?;
    if !reader.header().is_finalized() {
        eprintln!("警告：文件未标记 FINALIZED，将扫到 EOF");
    }

    let has_ch = reader
        .channels()
        .iter()
        .any(|c| c.channel_type == expect);
    if !has_ch {
        let tip = match expect {
            ChannelType::Video => "会话中无视频通道",
            _ => "会话中无 UDP 通道",
        };
        return Err(io::Error::new(io::ErrorKind::InvalidInput, tip));
    }

    let sock = bind_sender(opts.local_bind)?;
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

    eprintln!(
        "重放中（{label}）：{} → {}（按 relative_us，{}x，fromUs={from_us}{}{}）",
        opts.in_path.display(),
        opts.target,
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
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(expect) {
            continue;
        }
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
            |bytes| send_to(&sock, bytes, opts.target).map(|_| ()),
        )?;
    }

    eprintln!("重放结束：发送 {sent} 帧");
    Ok(sent)
}

/// 消费一次挂起的 seek：回卷读头并校准时钟；无挂起则 `None`。
fn take_pending_seek(
    seek_us: &AtomicU64,
    reader: &mut SessionReader,
    base: &mut Instant,
    speed: f64,
) -> io::Result<Option<u64>> {
    let raw = seek_us.swap(SessionClock::NO_SEEK, Ordering::SeqCst);
    if raw == SessionClock::NO_SEEK {
        return Ok(None);
    }
    reader.rewind_packets()?;
    SessionClock::rebase_timeline(base, raw, speed);
    Ok(Some(raw))
}

/// EOF 时若开启循环且本遍匹配过通道帧：回卷到 0 再放；返回 true 表示已重启。
fn try_restart_loop(
    looping: bool,
    stop: &AtomicBool,
    pass_matched: u64,
    reader: &mut SessionReader,
    base: &mut Instant,
    speed: f64,
    skip_before_us: &mut u64,
    pass_matched_out: &mut u64,
    sent: u64,
    on_progress: &mut OnReplayProgress<'_>,
) -> io::Result<bool> {
    if !looping || stop.load(Ordering::SeqCst) || pass_matched == 0 {
        return Ok(false);
    }
    reader.rewind_packets()?;
    *skip_before_us = 0;
    *pass_matched_out = 0;
    SessionClock::rebase_timeline(base, 0, speed);
    if let Some(cb) = on_progress.as_mut() {
        cb(sent, 0);
    }
    Ok(true)
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
        fault: FaultInjection::OFF,
        looping: false,
    };
    replay_udp_from_file(&opts)
}

pub struct SerialReplayOptions {
    pub in_path: std::path::PathBuf,
    pub config: SerialConfig,
    /// 墙钟倍速；1.0=实时。
    pub speed: f64,
    pub fault: FaultInjection,
    /// 播完后回卷从头再放，直到 stop。
    pub looping: bool,
}

pub fn replay_serial_from_file(opts: &SerialReplayOptions) -> io::Result<u64> {
    let port = SerialPort::open(
        &opts.config,
        std::time::Duration::from_millis(200),
    )?;
    let stop = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    replay_serial_with_port_with(opts, &port, &stop, &paused, &seek, 0, &mut on)
}

pub fn replay_serial_with_port(
    opts: &SerialReplayOptions,
    port: &SerialPort,
) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    replay_serial_with_port_with(opts, port, &stop, &paused, &seek, 0, &mut on)
}

pub fn replay_serial_with_port_with(
    opts: &SerialReplayOptions,
    port: &SerialPort,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
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

    eprintln!(
        "重放中：{} → 串口 {}（按 relative_us，{}x，fromUs={from_us}{}{}）",
        opts.in_path.display(),
        opts.config.port_name,
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
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Serial) {
            continue;
        }
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
            |bytes| port.write_all(bytes),
        )?;
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
    pub fault: FaultInjection,
    /// 播完后回卷从头再放，直到 stop（含连接事件）。
    pub looping: bool,
}

/// 按会话内录制角色取反后建立连接，并把录到的 RX 数据按时间轴写回对端。
pub fn replay_tcp_from_file(
    opts: &TcpReplayOptions,
    stop: &AtomicBool,
) -> io::Result<u64> {
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    replay_tcp_from_file_with(opts, stop, &paused, &seek, 0, &mut on)
}

pub fn replay_tcp_from_file_with(
    opts: &TcpReplayOptions,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
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
    let fault = opts.fault.clamped();
    eprintln!(
        "重放中：{} → TCP {} {}（录制角色 {}，按 relative_us，{}x，fromUs={from_us}{}{}）",
        opts.in_path.display(),
        play_role.as_str(),
        opts.endpoint,
        recorded_cfg.role.as_str(),
        speed,
        if fault.is_active() { "，故障注入" } else { "" },
        if opts.looping { "，循环" } else { "" }
    );

    let mut base = Instant::now();
    let mut skip_before_us = from_us;
    if from_us > 0 {
        SessionClock::rebase_timeline(&mut base, from_us, speed);
    }
    let mut sent = 0u64;
    let mut rng = FaultRng::new();
    let mut stream = None;
    let mut pass_matched = 0u64;

    reader.rewind_packets()?;
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if let Some(target) = take_pending_seek(seek_us, &mut reader, &mut base, speed)? {
            skip_before_us = target;
            stream = None; // 回卷后需重新跑连接事件
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
                stream = None;
                continue;
            }
            break;
        };
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Tcp) {
            continue;
        }
        pass_matched += 1;

        // seek 目标之前：连接事件仍立即处理；数据帧跳过
        if pkt.relative_us < skip_before_us {
            match pkt.kind {
                PacketKind::TcpConnect => {
                    let s = match play_role {
                        TcpRole::Server => {
                            let listener = listen(opts.endpoint)?;
                            match accept_until(&listener, stop, opts.poll_timeout)? {
                                Some((s, _)) => s,
                                None => break,
                            }
                        }
                        TcpRole::Client => connect(opts.endpoint, opts.poll_timeout)?,
                    };
                    stream = Some(s);
                }
                PacketKind::TcpDisconnect => {
                    stream = None;
                }
                _ => {}
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
    /// CONNECT 凭据；`None` 时回退到会话通道 config_json（E9a）。
    pub username: Option<String>,
    pub password: Option<String>,
    pub poll_timeout: Duration,
    /// 墙钟倍速；1.0=实时。
    pub speed: f64,
    pub fault: FaultInjection,
    /// 播完后回卷从头再放，直到 stop。
    pub looping: bool,
}

/// 连接 Broker，按时间轴把 MQTT 数据帧 PUBLISH 出去（演设备/传感器）。
pub fn replay_mqtt_from_file(opts: &MqttReplayOptions) -> io::Result<u64> {
    let stop = AtomicBool::new(false);
    let paused = AtomicBool::new(false);
    let seek = AtomicU64::new(SessionClock::NO_SEEK);
    let mut on: OnReplayProgress<'_> = None;
    replay_mqtt_from_file_with(opts, &stop, &paused, &seek, 0, &mut on)
}

pub fn replay_mqtt_from_file_with(
    opts: &MqttReplayOptions,
    stop: &AtomicBool,
    paused: &AtomicBool,
    seek_us: &AtomicU64,
    from_us: u64,
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

    let username = opts
        .username
        .clone()
        .or_else(|| recorded_cfg.username.clone());
    let password = opts
        .password
        .clone()
        .or_else(|| recorded_cfg.password.clone());
    let config = MqttConfig::new(
        opts.broker.ip().to_string(),
        opts.broker.port(),
        recorded_cfg.topic.clone(),
        recorded_cfg.qos,
    )
    .with_credentials(username, password);

    let speed = SessionClock::clamp_speed(opts.speed);
    let fault = opts.fault.clamped();
    eprintln!(
        "重放中：{} → MQTT {}:{} prefix={:?}（按 relative_us，{}x，fromUs={from_us}{}{}）",
        opts.in_path.display(),
        config.broker,
        config.port,
        opts.topic_prefix,
        speed,
        if fault.is_active() { "，故障注入" } else { "" },
        if opts.looping { "，循环" } else { "" }
    );

    let mut client = MqttClient::connect(&config, opts.poll_timeout)?;
    let mut base = Instant::now();
    let mut skip_before_us = from_us;
    if from_us > 0 {
        SessionClock::rebase_timeline(&mut base, from_us, speed);
    }
    let mut sent = 0u64;
    let mut rng = FaultRng::new();
    let mut pass_matched = 0u64;

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
        let ch = reader
            .channels()
            .iter()
            .find(|c| c.channel_index == pkt.channel_index);
        if ch.map(|c| c.channel_type) != Some(ChannelType::Mqtt) {
            continue;
        }
        pass_matched += 1;
        if pkt.relative_us < skip_before_us {
            continue;
        }
        let topic = String::from_utf8_lossy(&pkt.aux);
        let topic = apply_topic_prefix(&opts.topic_prefix, &topic);
        let qos = opts
            .qos_override
            .unwrap_or_else(|| mqtt_qos_from_flags(pkt.flags));
        let retain = mqtt_retain_from_flags(pkt.flags);

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
            |bytes| client.publish(&topic, bytes, qos, retain),
        )?;
    }

    let _ = client.disconnect();
    eprintln!("重放结束：发布 {sent} 帧");
    Ok(sent)
}

#[cfg(test)]
mod fault_tests {
    use super::*;

    #[test]
    fn loss_100_always_drops() {
        let fault = FaultInjection {
            enabled: true,
            loss_rate_percent: 100,
            corrupt_payload: false,
        };
        let mut rng = FaultRng::seeded(42);
        for _ in 0..32 {
            assert!(apply_fault(b"abc", &fault, &mut rng).is_none());
        }
    }

    #[test]
    fn off_keeps_payload() {
        let mut rng = FaultRng::seeded(1);
        let out = apply_fault(b"hi", &FaultInjection::OFF, &mut rng).unwrap();
        assert_eq!(out.as_ref(), b"hi");
    }

    #[test]
    fn corrupt_may_mutate() {
        let fault = FaultInjection {
            enabled: true,
            loss_rate_percent: 0,
            corrupt_payload: true,
        };
        let mut rng = FaultRng::seeded(7);
        let mut saw_corrupt = false;
        for _ in 0..200 {
            if let Some(Cow::Owned(buf)) = apply_fault(b"ABCDEFGH", &fault, &mut rng) {
                assert_ne!(buf, b"ABCDEFGH");
                saw_corrupt = true;
                break;
            }
        }
        assert!(saw_corrupt, "固定种子下应至少一次篡改");
    }
}
