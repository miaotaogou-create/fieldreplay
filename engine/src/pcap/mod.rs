//! 经典 libpcap：导出（E10）与导入（E14b）。
//!
//! ponytail: 合成 L2/L3/L4，checksum 可为 0；上限是 Wireshark 校验告警，升级路径是补真实校验和或改 pcapng。

mod import;

pub use import::{
    pcap_bytes_to_session, read_pcap_to_session, self_check_pcap_import, PcapImportSession,
    PcapImportStats, PCAP_MAGIC_NSEC,
};

use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::Path;

use crate::fcap::{ChannelEntry, ChannelType, PacketKind, PacketRecord};

/// 经典 pcap 微秒、主机字节序魔数（小端机写盘为 `d4 c3 b2 a1`）。
pub const PCAP_MAGIC_USEC: u32 = 0xa1b2_c3d4;
pub const LINKTYPE_ETHERNET: u32 = 1;
const SNAPLEN: u32 = 65535;
const ETH_HEADER_LEN: usize = 14;
const IPV4_HEADER_LEN: usize = 20;
const UDP_HEADER_LEN: usize = 8;
const TCP_HEADER_LEN: usize = 20;

#[derive(Debug, Clone, Copy, Default)]
pub struct PcapExportStats {
    pub written: u64,
    pub skipped: u64,
}

/// 将会话帧写成经典 `.pcap` 字节；返回统计。
pub fn session_to_pcap_bytes(
    start_wall_unix_us: u64,
    channels: &[ChannelEntry],
    packets: &[PacketRecord],
) -> io::Result<(Vec<u8>, PcapExportStats)> {
    let mut out = Vec::with_capacity(24 + packets.len() * 64);
    write_global_header(&mut out)?;
    let mut stats = PcapExportStats::default();
    let mut tcp_seq: u32 = 1;
    for pkt in packets {
        match encode_packet(start_wall_unix_us, channels, pkt, &mut tcp_seq) {
            Some(frame) => {
                write_packet_record(&mut out, start_wall_unix_us, pkt.relative_us, &frame)?;
                stats.written += 1;
            }
            None => stats.skipped += 1,
        }
    }
    Ok((out, stats))
}

/// 写盘版；便于 CLI。
pub fn write_session_pcap(
    path: &Path,
    start_wall_unix_us: u64,
    channels: &[ChannelEntry],
    packets: &[PacketRecord],
) -> io::Result<PcapExportStats> {
    let (bytes, stats) = session_to_pcap_bytes(start_wall_unix_us, channels, packets)?;
    std::fs::write(path, bytes)?;
    Ok(stats)
}

fn write_global_header(w: &mut Vec<u8>) -> io::Result<()> {
    w.write_all(&PCAP_MAGIC_USEC.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // version_major
    w.write_all(&4u16.to_le_bytes())?; // version_minor
    w.write_all(&0i32.to_le_bytes())?; // thiszone
    w.write_all(&0u32.to_le_bytes())?; // sigfigs
    w.write_all(&SNAPLEN.to_le_bytes())?;
    w.write_all(&LINKTYPE_ETHERNET.to_le_bytes())?;
    Ok(())
}

fn write_packet_record(
    w: &mut Vec<u8>,
    start_wall_unix_us: u64,
    relative_us: u64,
    frame: &[u8],
) -> io::Result<()> {
    let abs = start_wall_unix_us.saturating_add(relative_us);
    let ts_sec = (abs / 1_000_000) as u32;
    let ts_usec = (abs % 1_000_000) as u32;
    let incl = (frame.len() as u32).min(SNAPLEN);
    let orig = frame.len() as u32;
    w.write_all(&ts_sec.to_le_bytes())?;
    w.write_all(&ts_usec.to_le_bytes())?;
    w.write_all(&incl.to_le_bytes())?;
    w.write_all(&orig.to_le_bytes())?;
    w.write_all(&frame[..incl as usize])?;
    Ok(())
}

fn encode_packet(
    _start_wall_unix_us: u64,
    channels: &[ChannelEntry],
    pkt: &PacketRecord,
    tcp_seq: &mut u32,
) -> Option<Vec<u8>> {
    if pkt.kind != PacketKind::Data {
        return None;
    }
    let ch = channels
        .iter()
        .find(|c| c.channel_index == pkt.channel_index)?;
    match ch.channel_type {
        ChannelType::Udp | ChannelType::Video => {
            let (src, dst) = resolve_udp_endpoints(ch, pkt);
            Some(build_eth_ipv4_udp(ch.channel_index, src, dst, &pkt.payload))
        }
        ChannelType::Tcp => {
            let (src, dst) = resolve_tcp_endpoints(ch, pkt);
            let seq = *tcp_seq;
            *tcp_seq = tcp_seq.saturating_add(pkt.payload.len() as u32).max(seq + 1);
            Some(build_eth_ipv4_tcp(
                ch.channel_index,
                src,
                dst,
                seq,
                &pkt.payload,
            ))
        }
        ChannelType::Serial | ChannelType::Mqtt => None,
    }
}

fn resolve_udp_endpoints(ch: &ChannelEntry, pkt: &PacketRecord) -> (SocketAddrV4, SocketAddrV4) {
    let bind = parse_bind_v4(&ch.config_json).unwrap_or_else(|| {
        SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9)
    });
    let peer = parse_aux_v4(&pkt.aux)
        .or_else(|| parse_peer_v4(&ch.config_json))
        .unwrap_or_else(|| SocketAddrV4::new(Ipv4Addr::new(10, 255, 0, 1), 9));
    if pkt.direction == 0 {
        // RX：对端 → 本机
        (peer, bind)
    } else {
        (bind, peer)
    }
}

fn resolve_tcp_endpoints(ch: &ChannelEntry, pkt: &PacketRecord) -> (SocketAddrV4, SocketAddrV4) {
    let local = parse_tcp_local_v4(&ch.config_json).unwrap_or_else(|| {
        SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9)
    });
    let peer = parse_aux_v4(&pkt.aux)
        .unwrap_or_else(|| SocketAddrV4::new(Ipv4Addr::new(10, 255, 0, 2), 9));
    if pkt.direction == 0 {
        (peer, local)
    } else {
        (local, peer)
    }
}

fn parse_aux_v4(aux: &[u8]) -> Option<SocketAddrV4> {
    let s = std::str::from_utf8(aux).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    match s.parse::<SocketAddr>() {
        Ok(SocketAddr::V4(v4)) => Some(v4),
        _ => None,
    }
}

fn parse_bind_v4(config_json: &str) -> Option<SocketAddrV4> {
    let ip = extract_str(config_json, "bind_ip")
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .unwrap_or(Ipv4Addr::new(127, 0, 0, 1));
    let port = extract_u64(config_json, "bind_port").unwrap_or(9) as u16;
    Some(SocketAddrV4::new(ip, port))
}

fn parse_peer_v4(config_json: &str) -> Option<SocketAddrV4> {
    let ip = extract_str(config_json, "peer_ip")
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<Ipv4Addr>().ok())?;
    let port = extract_u64(config_json, "peer_port").unwrap_or(0) as u16;
    if port == 0 {
        return None;
    }
    Some(SocketAddrV4::new(ip, port))
}

fn parse_tcp_local_v4(config_json: &str) -> Option<SocketAddrV4> {
    let ip = extract_str(config_json, "host")
        .and_then(|s| s.parse::<Ipv4Addr>().ok())
        .unwrap_or(Ipv4Addr::new(127, 0, 0, 1));
    let port = extract_u64(config_json, "port").unwrap_or(9) as u16;
    Some(SocketAddrV4::new(ip, port))
}

fn mac_for_channel(channel_index: u16, role: u8) -> [u8; 6] {
    [
        0x02,
        0x00,
        0x00,
        role,
        (channel_index >> 8) as u8,
        (channel_index & 0xff) as u8,
    ]
}

fn build_eth_ipv4_udp(
    channel_index: u16,
    src: SocketAddrV4,
    dst: SocketAddrV4,
    payload: &[u8],
) -> Vec<u8> {
    let udp_len = UDP_HEADER_LEN + payload.len();
    let ip_len = IPV4_HEADER_LEN + udp_len;
    let mut frame = Vec::with_capacity(ETH_HEADER_LEN + ip_len);
    // Ethernet
    frame.extend_from_slice(&mac_for_channel(channel_index, 1)); // dst
    frame.extend_from_slice(&mac_for_channel(channel_index, 2)); // src
    frame.extend_from_slice(&0x0800u16.to_be_bytes());
    // IPv4
    push_ipv4_header(&mut frame, src.ip(), dst.ip(), 17, ip_len as u16);
    // UDP
    frame.extend_from_slice(&src.port().to_be_bytes());
    frame.extend_from_slice(&dst.port().to_be_bytes());
    frame.extend_from_slice(&(udp_len as u16).to_be_bytes());
    frame.extend_from_slice(&0u16.to_be_bytes()); // checksum 0
    frame.extend_from_slice(payload);
    frame
}

fn build_eth_ipv4_tcp(
    channel_index: u16,
    src: SocketAddrV4,
    dst: SocketAddrV4,
    seq: u32,
    payload: &[u8],
) -> Vec<u8> {
    let tcp_len = TCP_HEADER_LEN + payload.len();
    let ip_len = IPV4_HEADER_LEN + tcp_len;
    let mut frame = Vec::with_capacity(ETH_HEADER_LEN + ip_len);
    frame.extend_from_slice(&mac_for_channel(channel_index, 1));
    frame.extend_from_slice(&mac_for_channel(channel_index, 2));
    frame.extend_from_slice(&0x0800u16.to_be_bytes());
    push_ipv4_header(&mut frame, src.ip(), dst.ip(), 6, ip_len as u16);
    // TCP 最小头：data offset=5，ACK+PSH
    frame.extend_from_slice(&src.port().to_be_bytes());
    frame.extend_from_slice(&dst.port().to_be_bytes());
    frame.extend_from_slice(&seq.to_be_bytes());
    frame.extend_from_slice(&0u32.to_be_bytes()); // ack
    frame.push(0x50); // data offset 5
    frame.push(0x18); // PSH+ACK
    frame.extend_from_slice(&0x4000u16.to_be_bytes()); // window
    frame.extend_from_slice(&0u16.to_be_bytes()); // checksum
    frame.extend_from_slice(&0u16.to_be_bytes()); // urgent
    frame.extend_from_slice(payload);
    frame
}

fn push_ipv4_header(
    frame: &mut Vec<u8>,
    src: &Ipv4Addr,
    dst: &Ipv4Addr,
    proto: u8,
    total_len: u16,
) {
    let mut hdr = [0u8; IPV4_HEADER_LEN];
    hdr[0] = 0x45; // v4, IHL=5
    hdr[1] = 0;
    hdr[2..4].copy_from_slice(&total_len.to_be_bytes());
    hdr[4..6].copy_from_slice(&0u16.to_be_bytes()); // id
    hdr[6..8].copy_from_slice(&0x4000u16.to_be_bytes()); // DF
    hdr[8] = 64; // ttl
    hdr[9] = proto;
    hdr[12..16].copy_from_slice(&src.octets());
    hdr[16..20].copy_from_slice(&dst.octets());
    let sum = ipv4_checksum(&hdr);
    hdr[10..12].copy_from_slice(&sum.to_be_bytes());
    frame.extend_from_slice(&hdr);
}

fn ipv4_checksum(hdr: &[u8; IPV4_HEADER_LEN]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i < IPV4_HEADER_LEN {
        if i == 10 {
            i += 2;
            continue;
        }
        let word = u16::from_be_bytes([hdr[i], hdr[i + 1]]);
        sum += u32::from(word);
        i += 2;
    }
    while sum > 0xffff {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

fn after_key<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let i = json.find(&needle)?;
    let after = &json[i + needle.len()..];
    Some(after.trim_start_matches(|c: char| c == ':' || c.is_whitespace()))
}

fn extract_str(json: &str, key: &str) -> Option<String> {
    let after = after_key(json, key)?;
    if !after.starts_with('"') {
        return None;
    }
    let mut out = String::new();
    let mut chars = after[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            },
            '"' => return Some(out),
            c => out.push(c),
        }
    }
    None
}

fn extract_u64(json: &str, key: &str) -> Option<u64> {
    let after = after_key(json, key)?;
    let end = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    if end == 0 {
        return None;
    }
    after[..end].parse().ok()
}

/// 最小自检：两帧 UDP → 魔数 + written=2；一帧串口计入 skipped。
pub fn self_check_pcap_export() -> io::Result<()> {
    let channels = vec![
        ChannelEntry {
            channel_index: 0,
            channel_type: ChannelType::Udp,
            name: "udp0".into(),
            config_json: r#"{"bind_ip":"127.0.0.1","bind_port":9000,"peer_ip":"","peer_port":0}"#
                .into(),
        },
        ChannelEntry {
            channel_index: 1,
            channel_type: ChannelType::Serial,
            name: "com".into(),
            config_json: r#"{"port_name":"COM3","baud":115200}"#.into(),
        },
    ];
    let packets = vec![
        PacketRecord {
            relative_us: 0,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"10.0.0.2:5000".to_vec(),
            payload: b"hello".to_vec(),
        },
        PacketRecord {
            relative_us: 1000,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 1,
            flags: 0,
            aux: b"10.0.0.2:5000".to_vec(),
            payload: b"world".to_vec(),
        },
        PacketRecord {
            relative_us: 2000,
            channel_index: 1,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: vec![],
            payload: b"serial".to_vec(),
        },
    ];
    let start = 1_700_000_000_000_000u64;
    let (bytes, stats) = session_to_pcap_bytes(start, &channels, &packets)?;
    if bytes.len() < 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "pcap 过短"));
    }
    let magic = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if magic != PCAP_MAGIC_USEC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pcap 魔数不符: {magic:#x}"),
        ));
    }
    if stats.written != 2 || stats.skipped != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "统计不符: written={} skipped={}（期望 2/1）",
                stats.written, stats.skipped
            ),
        ));
    }
    // 全局头 24 + 两包头各 16 + 两帧（14+20+8+5 与 14+20+8+5）
    let expect_min = 24 + 2 * (16 + ETH_HEADER_LEN + IPV4_HEADER_LEN + UDP_HEADER_LEN + 5);
    if bytes.len() < expect_min {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("pcap 长度过短: {} < {expect_min}", bytes.len()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_two_frames_and_skip_serial() {
        self_check_pcap_export().expect("pcap self-check");
    }
}
