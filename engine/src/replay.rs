//! 按 `relative_us` 将 UDP 数据帧发往目标。

use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Instant;

use crate::clock::SessionClock;
use crate::fcap::{ChannelType, PacketKind, SessionReader};
use crate::udp::{bind_sender, send_to};

pub struct ReplayOptions {
    pub in_path: std::path::PathBuf,
    pub target: SocketAddr,
    /// 发送端本地 bind（默认 `0.0.0.0:0`）
    pub local_bind: SocketAddr,
}

pub fn replay_udp_from_file(opts: &ReplayOptions) -> io::Result<u64> {
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

    eprintln!(
        "重放中：{} → {}（按 relative_us）",
        opts.in_path.display(),
        opts.target
    );

    reader.rewind_packets()?;
    while let Some(pkt) = reader.next_packet()? {
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
        SessionClock::sleep_until_relative(base, pkt.relative_us);
        send_to(&sock, &pkt.payload, opts.target)?;
        sent += 1;
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
    };
    replay_udp_from_file(&opts)
}
