//! 经典 libpcap 导入（E14b）：以太网 IPv4 UDP/TCP 有载荷帧 → 会话通道表 + 帧。
//!
//! ponytail: 本机侧取流首包目的地址；抓包机非设备时 RX/TX 可能对调，升级靠 UI 提示或手改 config。

use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::net::{Ipv4Addr, SocketAddrV4};

use crate::fcap::{ChannelEntry, ChannelType, PacketKind, PacketRecord};
use crate::tcp::{TcpConfig, TcpRole};

use super::{LINKTYPE_ETHERNET, PCAP_MAGIC_USEC};

/// 纳秒时间戳经典 pcap 魔数（小端）。
pub const PCAP_MAGIC_NSEC: u32 = 0xa1b2_3c4d;
const PCAP_MAGIC_USEC_SWAPPED: u32 = 0xd4c3_b2a1;
const PCAP_MAGIC_NSEC_SWAPPED: u32 = 0x4d3c_b2a1;

const ETH_HEADER_LEN: usize = 14;
const VLAN_TAG_LEN: usize = 4;
const IPV4_MIN_HEADER: usize = 20;
const UDP_HEADER_LEN: usize = 8;
const TCP_MIN_HEADER: usize = 20;
const ETHERTYPE_IPV4: u16 = 0x0800;
const ETHERTYPE_VLAN: u16 = 0x8100;
const IPPROTO_TCP: u8 = 6;
const IPPROTO_UDP: u8 = 17;

#[derive(Debug, Clone, Copy, Default)]
pub struct PcapImportStats {
    pub written: u64,
    pub skipped: u64,
    pub channel_count: u16,
}

/// 导入成功后的内存会话切片（控制面可再包成 `LoadedSession`）。
#[derive(Debug, Clone)]
pub struct PcapImportSession {
    pub start_wall_unix_us: u64,
    pub channels: Vec<ChannelEntry>,
    pub packets: Vec<PacketRecord>,
    pub meta_json: String,
    pub stats: PcapImportStats,
}

/// 将经典以太网 `.pcap` 字节转为会话；无可纳入帧则失败。
pub fn pcap_bytes_to_session(
    bytes: &[u8],
    source_hint: &str,
) -> io::Result<PcapImportSession> {
    let header = parse_global_header(bytes)?;
    let mut off = 24usize;
    let mut skipped = 0u64;
    let mut parsed: Vec<ParsedFrame> = Vec::new();

    while off + 16 <= bytes.len() {
        let ts_sec = header.read_u32(&bytes[off..off + 4]);
        let ts_frac = header.read_u32(&bytes[off + 4..off + 8]);
        let incl_len = header.read_u32(&bytes[off + 8..off + 12]) as usize;
        off += 16;
        if off + incl_len > bytes.len() {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "pcap 包体截断：incl_len 超出文件末尾",
            ));
        }
        let frame = &bytes[off..off + incl_len];
        off += incl_len;

        let abs_us = header.packet_time_us(ts_sec, ts_frac);
        match decode_frame(frame) {
            Some(decoded) => parsed.push(ParsedFrame { abs_us, decoded }),
            None => skipped += 1,
        }
    }

    if parsed.is_empty() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "无可导入帧（仅支持以太网 IPv4 UDP/TCP 有载荷数据）",
        ));
    }

    let t0 = parsed[0].abs_us;
    let start_wall_unix_us = t0;

    let mut flow_map: HashMap<FlowKey, u16> = HashMap::new();
    let mut channels: Vec<ChannelEntry> = Vec::new();
    let mut packets: Vec<PacketRecord> = Vec::new();
    let mut written = 0u64;

    for frame in &parsed {
        let key = FlowKey::from_endpoints(
            frame.decoded.proto,
            frame.decoded.src,
            frame.decoded.dst,
        );
        let ch_idx = if let Some(&idx) = flow_map.get(&key) {
            idx
        } else {
            let idx = channels.len() as u16;
            let local = frame.decoded.dst;
            let peer = frame.decoded.src;
            let entry = match frame.decoded.proto {
                IPPROTO_UDP => ChannelEntry {
                    channel_index: idx,
                    channel_type: ChannelType::Udp,
                    name: format!("udp_{}", local.port()),
                    config_json: format!(
                        r#"{{"bind_ip":"{}","bind_port":{},"peer_ip":"{}","peer_port":{}}}"#,
                        local.ip(),
                        local.port(),
                        peer.ip(),
                        peer.port()
                    ),
                },
                IPPROTO_TCP => ChannelEntry {
                    channel_index: idx,
                    channel_type: ChannelType::Tcp,
                    name: format!("tcp_{}", local.port()),
                    config_json: TcpConfig::new(
                        TcpRole::Server,
                        local.ip().to_string(),
                        local.port(),
                    )
                    .to_config_json(),
                },
                _ => unreachable!(),
            };
            channels.push(entry);
            flow_map.insert(key, idx);
            idx
        };

        let ch = &channels[ch_idx as usize];
        let local = local_from_channel(ch);
        let (direction, peer) = if frame.decoded.dst == local {
            (0u8, frame.decoded.src)
        } else if frame.decoded.src == local {
            (1u8, frame.decoded.dst)
        } else {
            // 异常：五元组同流但端点未对齐本机侧；按「目的=本机」启发式仍标 RX
            (0u8, frame.decoded.src)
        };

        packets.push(PacketRecord {
            relative_us: frame.abs_us.saturating_sub(t0),
            channel_index: ch_idx,
            kind: PacketKind::Data,
            direction,
            flags: 0,
            aux: format!("{}:{}", peer.ip(), peer.port()).into_bytes(),
            payload: frame.decoded.payload.clone(),
        });
        written += 1;
    }

    let channel_count = channels.len() as u16;
    let title = if source_hint.trim().is_empty() {
        "imported from pcap".to_string()
    } else {
        format!("imported from {source_hint}")
    };
    let notes = format!("imported {written} written, {skipped} skipped, {channel_count} channels");
    let meta_json = format!(
        r#"{{"session_title":"{}","notes":"{}"}}"#,
        escape_json(&title),
        escape_json(&notes)
    );

    Ok(PcapImportSession {
        start_wall_unix_us,
        channels,
        packets,
        meta_json,
        stats: PcapImportStats {
            written,
            skipped,
            channel_count,
        },
    })
}

/// 读盘版。
pub fn read_pcap_to_session(
    path: &std::path::Path,
) -> io::Result<PcapImportSession> {
    let bytes = std::fs::read(path)?;
    let hint = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("pcap");
    pcap_bytes_to_session(&bytes, hint)
}

struct GlobalHeader {
    le: bool,
    nsec: bool,
}

impl GlobalHeader {
    fn read_u16(&self, b: &[u8]) -> u16 {
        if self.le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        }
    }

    fn read_u32(&self, b: &[u8]) -> u32 {
        if self.le {
            u32::from_le_bytes([b[0], b[1], b[2], b[3]])
        } else {
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        }
    }

    fn packet_time_us(&self, ts_sec: u32, ts_frac: u32) -> u64 {
        let sec = ts_sec as u64;
        let us = if self.nsec {
            (ts_frac as u64) / 1000
        } else {
            ts_frac as u64
        };
        sec.saturating_mul(1_000_000).saturating_add(us)
    }
}

fn parse_global_header(bytes: &[u8]) -> io::Result<GlobalHeader> {
    if bytes.len() < 24 {
        return Err(io::Error::new(ErrorKind::InvalidData, "pcap 过短：缺全局头"));
    }
    // 魔数按磁盘原样认；再决定字段字节序
    let magic_le = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let (le, nsec) = match magic_le {
        PCAP_MAGIC_USEC => (true, false),
        PCAP_MAGIC_NSEC => (true, true),
        PCAP_MAGIC_USEC_SWAPPED => (false, false),
        PCAP_MAGIC_NSEC_SWAPPED => (false, true),
        other => {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("不支持的 pcap 魔数 {other:#x}（需要经典 libpcap，不支持 pcapng）"),
            ));
        }
    };
    let header = GlobalHeader { le, nsec };
    let linktype = header.read_u32(&bytes[20..24]);
    if linktype != LINKTYPE_ETHERNET {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("仅支持以太网经典 pcap（LINKTYPE_ETHERNET=1），当前 linktype={linktype}"),
        ));
    }
    let _ = header.read_u16(&bytes[4..6]); // version
    Ok(header)
}

#[derive(Debug, Clone)]
struct DecodedL4 {
    proto: u8,
    src: SocketAddrV4,
    dst: SocketAddrV4,
    payload: Vec<u8>,
}

struct ParsedFrame {
    abs_us: u64,
    decoded: DecodedL4,
}

fn decode_frame(frame: &[u8]) -> Option<DecodedL4> {
    if frame.len() < ETH_HEADER_LEN {
        return None;
    }
    let mut ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    let mut l3_off = ETH_HEADER_LEN;
    if ethertype == ETHERTYPE_VLAN {
        if frame.len() < ETH_HEADER_LEN + VLAN_TAG_LEN {
            return None;
        }
        ethertype = u16::from_be_bytes([frame[16], frame[17]]);
        l3_off = ETH_HEADER_LEN + VLAN_TAG_LEN;
    }
    if ethertype != ETHERTYPE_IPV4 {
        return None;
    }
    let ip = frame.get(l3_off..)?;
    if ip.len() < IPV4_MIN_HEADER {
        return None;
    }
    let ver_ihl = ip[0];
    if ver_ihl >> 4 != 4 {
        return None;
    }
    let ihl = ((ver_ihl & 0x0f) as usize) * 4;
    if ihl < IPV4_MIN_HEADER || ip.len() < ihl {
        return None;
    }
    let frag = u16::from_be_bytes([ip[6], ip[7]]);
    let mf = (frag & 0x2000) != 0;
    let offset = frag & 0x1fff;
    if mf || offset != 0 {
        return None;
    }
    let total_len = u16::from_be_bytes([ip[2], ip[3]]) as usize;
    let ip_payload_end = total_len.min(ip.len());
    if ip_payload_end < ihl {
        return None;
    }
    let proto = ip[9];
    let src_ip = Ipv4Addr::new(ip[12], ip[13], ip[14], ip[15]);
    let dst_ip = Ipv4Addr::new(ip[16], ip[17], ip[18], ip[19]);
    let l4 = &ip[ihl..ip_payload_end];

    match proto {
        IPPROTO_UDP => {
            if l4.len() < UDP_HEADER_LEN {
                return None;
            }
            let sport = u16::from_be_bytes([l4[0], l4[1]]);
            let dport = u16::from_be_bytes([l4[2], l4[3]]);
            let udp_len = u16::from_be_bytes([l4[4], l4[5]]) as usize;
            if udp_len < UDP_HEADER_LEN {
                return None;
            }
            let payload_end = udp_len.min(l4.len());
            let payload = l4[UDP_HEADER_LEN..payload_end].to_vec();
            if payload.is_empty() {
                return None;
            }
            Some(DecodedL4 {
                proto,
                src: SocketAddrV4::new(src_ip, sport),
                dst: SocketAddrV4::new(dst_ip, dport),
                payload,
            })
        }
        IPPROTO_TCP => {
            if l4.len() < TCP_MIN_HEADER {
                return None;
            }
            let sport = u16::from_be_bytes([l4[0], l4[1]]);
            let dport = u16::from_be_bytes([l4[2], l4[3]]);
            let data_off = ((l4[12] >> 4) as usize) * 4;
            if data_off < TCP_MIN_HEADER || l4.len() < data_off {
                return None;
            }
            let payload = l4[data_off..].to_vec();
            if payload.is_empty() {
                return None;
            }
            Some(DecodedL4 {
                proto,
                src: SocketAddrV4::new(src_ip, sport),
                dst: SocketAddrV4::new(dst_ip, dport),
                payload,
            })
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FlowKey {
    proto: u8,
    a_ip: u32,
    a_port: u16,
    b_ip: u32,
    b_port: u16,
}

impl FlowKey {
    fn from_endpoints(proto: u8, src: SocketAddrV4, dst: SocketAddrV4) -> Self {
        let s = (u32::from(*src.ip()), src.port());
        let d = (u32::from(*dst.ip()), dst.port());
        let (a, b) = if s <= d { (s, d) } else { (d, s) };
        Self {
            proto,
            a_ip: a.0,
            a_port: a.1,
            b_ip: b.0,
            b_port: b.1,
        }
    }
}

fn local_from_channel(ch: &ChannelEntry) -> SocketAddrV4 {
    match ch.channel_type {
        ChannelType::Udp | ChannelType::Video => {
            let ip = extract_json_str(&ch.config_json, "bind_ip")
                .and_then(|s| s.parse().ok())
                .unwrap_or(Ipv4Addr::new(127, 0, 0, 1));
            let port = extract_json_u64(&ch.config_json, "bind_port").unwrap_or(9) as u16;
            SocketAddrV4::new(ip, port)
        }
        ChannelType::Tcp => {
            let ip = extract_json_str(&ch.config_json, "host")
                .and_then(|s| s.parse().ok())
                .unwrap_or(Ipv4Addr::new(127, 0, 0, 1));
            let port = extract_json_u64(&ch.config_json, "port").unwrap_or(9) as u16;
            SocketAddrV4::new(ip, port)
        }
        _ => SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9),
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn after_key<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let i = json.find(&needle)?;
    let after = &json[i + needle.len()..];
    Some(after.trim_start_matches(|c: char| c == ':' || c.is_whitespace()))
}

fn extract_json_str(json: &str, key: &str) -> Option<String> {
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

fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let after = after_key(json, key)?;
    let end = after
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(after.len());
    if end == 0 {
        return None;
    }
    after[..end].parse().ok()
}

/// 自检：E10 导出 → 导入；UDP/TCP 载荷与相对轴对齐；ARP 计入 skipped；全跳过失败。
pub fn self_check_pcap_import() -> io::Result<()> {
    use super::session_to_pcap_bytes;

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
            channel_type: ChannelType::Tcp,
            name: "tcp0".into(),
            config_json: TcpConfig::new(TcpRole::Server, "127.0.0.1", 9100).to_config_json(),
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
            relative_us: 1500,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 1,
            flags: 0,
            aux: b"10.0.0.2:5000".to_vec(),
            payload: b"world".to_vec(),
        },
        PacketRecord {
            relative_us: 3000,
            channel_index: 1,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"10.0.0.3:6000".to_vec(),
            payload: b"tcp-data".to_vec(),
        },
    ];
    let start = 1_700_000_000_000_000u64;
    let (pcap, export_stats) = session_to_pcap_bytes(start, &channels, &packets)?;
    if export_stats.written != 3 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("导出 written 期望 3，得 {}", export_stats.written),
        ));
    }

    let imported = pcap_bytes_to_session(&pcap, "roundtrip.pcap")?;
    if imported.stats.written != 3 || imported.stats.skipped != 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "导入统计不符: written={} skipped={}（期望 3/0）",
                imported.stats.written, imported.stats.skipped
            ),
        ));
    }
    if imported.stats.channel_count != 2 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "通道数不符: {}（期望 2）",
                imported.stats.channel_count
            ),
        ));
    }
    if imported.start_wall_unix_us != start {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "墙钟不符: {} != {start}",
                imported.start_wall_unix_us
            ),
        ));
    }
    if imported.packets.len() != 3 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("帧数不符: {}", imported.packets.len()),
        ));
    }
    for (i, (want, got)) in packets.iter().zip(imported.packets.iter()).enumerate() {
        if want.payload != got.payload {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!("帧 {i} payload 不一致"),
            ));
        }
        if want.relative_us != got.relative_us {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "帧 {i} relative_us 不一致: {} != {}",
                    got.relative_us, want.relative_us
                ),
            ));
        }
        if want.direction != got.direction {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                format!(
                    "帧 {i} direction 不一致: {} != {}",
                    got.direction, want.direction
                ),
            ));
        }
    }
    if !imported.meta_json.contains("imported 3 written") {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("meta notes 缺导入摘要: {}", imported.meta_json),
        ));
    }

    // ARP 等非 IPv4：计入 skipped；与 UDP 同文件仍成功
    let mut mixed = pcap.clone();
    append_arp_frame(&mut mixed, start + 5000)?;
    let mixed_import = pcap_bytes_to_session(&mixed, "with-arp.pcap")?;
    if mixed_import.stats.written != 3 || mixed_import.stats.skipped != 1 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            format!(
                "含 ARP 统计不符: written={} skipped={}（期望 3/1）",
                mixed_import.stats.written, mixed_import.stats.skipped
            ),
        ));
    }

    // 仅 ARP → 失败
    let mut arp_only = Vec::new();
    write_empty_pcap_header(&mut arp_only)?;
    append_arp_frame(&mut arp_only, start)?;
    match pcap_bytes_to_session(&arp_only, "arp-only.pcap") {
        Ok(_) => {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "全跳过 pcap 应失败，却成功了",
            ));
        }
        Err(e) => {
            let msg = e.to_string();
            if !msg.contains("无可导入帧") {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!("全跳过错误文案不符: {msg}"),
                ));
            }
        }
    }

    // 非以太网 linktype → 整文件拒绝
    let mut raw_ip = Vec::new();
    raw_ip.extend_from_slice(&PCAP_MAGIC_USEC.to_le_bytes());
    raw_ip.extend_from_slice(&2u16.to_le_bytes());
    raw_ip.extend_from_slice(&4u16.to_le_bytes());
    raw_ip.extend_from_slice(&0i32.to_le_bytes());
    raw_ip.extend_from_slice(&0u32.to_le_bytes());
    raw_ip.extend_from_slice(&65535u32.to_le_bytes());
    raw_ip.extend_from_slice(&101u32.to_le_bytes()); // LINKTYPE_RAW
    match pcap_bytes_to_session(&raw_ip, "raw.pcap") {
        Ok(_) => {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "非以太网 pcap 应拒绝",
            ));
        }
        Err(e) => {
            if !e.to_string().contains("仅支持以太网") {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    format!("非以太网错误文案不符: {e}"),
                ));
            }
        }
    }

    Ok(())
}

fn write_empty_pcap_header(w: &mut Vec<u8>) -> io::Result<()> {
    use std::io::Write;
    w.write_all(&PCAP_MAGIC_USEC.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&4u16.to_le_bytes())?;
    w.write_all(&0i32.to_le_bytes())?;
    w.write_all(&0u32.to_le_bytes())?;
    w.write_all(&65535u32.to_le_bytes())?;
    w.write_all(&LINKTYPE_ETHERNET.to_le_bytes())?;
    Ok(())
}

fn append_arp_frame(w: &mut Vec<u8>, abs_us: u64) -> io::Result<()> {
    use std::io::Write;
    // 最小 ARP 请求外形（以太网 + ARP）；导入应跳过
    let mut frame = Vec::new();
    frame.extend_from_slice(&[0xff; 6]); // dst broadcast
    frame.extend_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x00, 0x01]); // src
    frame.extend_from_slice(&0x0806u16.to_be_bytes()); // ARP
    frame.extend_from_slice(&[0u8; 28]); // ARP payload 占位
    let ts_sec = (abs_us / 1_000_000) as u32;
    let ts_usec = (abs_us % 1_000_000) as u32;
    let incl = frame.len() as u32;
    w.write_all(&ts_sec.to_le_bytes())?;
    w.write_all(&ts_usec.to_le_bytes())?;
    w.write_all(&incl.to_le_bytes())?;
    w.write_all(&incl.to_le_bytes())?;
    w.write_all(&frame)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_skips() {
        self_check_pcap_import().expect("pcap import self-check");
    }
}
