//! `.fcap` 顺序写：开文件 → 追加帧 → 收尾回写头。

use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

use super::types::{
    ChannelEntry, FileHeader, PacketRecord, FLAG_FINALIZED, FLAG_HAS_META, HEADER_BYTES,
};

pub struct SessionWriter {
    file: File,
    header: FileHeader,
    packet_count: u64,
    total_payload_bytes: u64,
    duration_us: u64,
    finalized: bool,
}

impl SessionWriter {
    /// 打开新会话：写头 + 通道表 + 可选 meta；计数暂 0。
    pub fn create(
        path: impl AsRef<Path>,
        start_wall_unix_us: u64,
        channels: &[ChannelEntry],
        meta_json: Option<&str>,
    ) -> io::Result<Self> {
        let meta = meta_json.unwrap_or("").as_bytes();
        if meta.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "SessionMeta 过长",
            ));
        }
        let table = encode_channel_table(channels)?;
        if channels.len() > u16::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "通道数过多",
            ));
        }

        let header = FileHeader::new_open(
            start_wall_unix_us,
            channels.len() as u16,
            table.len() as u32,
            meta.len() as u32,
        );

        let mut file = File::create(path)?;
        write_header(&mut file, &header)?;
        file.write_all(&table)?;
        if !meta.is_empty() {
            file.write_all(meta)?;
        }
        file.flush()?;

        Ok(Self {
            file,
            header,
            packet_count: 0,
            total_payload_bytes: 0,
            duration_us: 0,
            finalized: false,
        })
    }

    pub fn append_packet(&mut self, pkt: &PacketRecord) -> io::Result<()> {
        if self.finalized {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "会话已收尾，不能再追加",
            ));
        }
        write_packet(&mut self.file, pkt)?;
        self.packet_count = self.packet_count.saturating_add(1);
        self.total_payload_bytes = self
            .total_payload_bytes
            .saturating_add(pkt.payload.len() as u64);
        if pkt.relative_us >= self.duration_us {
            self.duration_us = pkt.relative_us;
        }
        Ok(())
    }

    /// 回写计数并置 FINALIZED。
    pub fn finalize(mut self) -> io::Result<()> {
        self.file.flush()?;
        self.header.packet_count = self.packet_count;
        self.header.total_payload_bytes = self.total_payload_bytes;
        self.header.duration_us = self.duration_us;
        self.header.flags |= FLAG_FINALIZED;
        if self.header.meta_bytes > 0 {
            self.header.flags |= FLAG_HAS_META;
        }
        self.file.seek(SeekFrom::Start(0))?;
        write_header(&mut self.file, &self.header)?;
        self.file.flush()?;
        self.finalized = true;
        Ok(())
    }

    pub fn packet_count(&self) -> u64 {
        self.packet_count
    }
}

fn encode_channel_table(channels: &[ChannelEntry]) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    for ch in channels {
        let name = ch.name.as_bytes();
        let cfg = ch.config_json.as_bytes();
        if name.len() > u16::MAX as usize || cfg.len() > u16::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "通道名或配置过长",
            ));
        }
        buf.extend_from_slice(&ch.channel_index.to_le_bytes());
        buf.push(ch.channel_type as u8);
        buf.push(0); // reserved
        buf.extend_from_slice(&(name.len() as u16).to_le_bytes());
        buf.extend_from_slice(&(cfg.len() as u16).to_le_bytes());
        buf.extend_from_slice(name);
        buf.extend_from_slice(cfg);
    }
    Ok(buf)
}

fn write_header(w: &mut impl Write, h: &FileHeader) -> io::Result<()> {
    w.write_all(&h.magic)?;
    w.write_all(&h.version.to_le_bytes())?;
    w.write_all(&h.header_bytes.to_le_bytes())?;
    w.write_all(&h.flags.to_le_bytes())?;
    w.write_all(&h.start_wall_unix_us.to_le_bytes())?;
    w.write_all(&h.channel_count.to_le_bytes())?;
    w.write_all(&h.reserved0.to_le_bytes())?;
    w.write_all(&h.channel_table_bytes.to_le_bytes())?;
    w.write_all(&h.meta_bytes.to_le_bytes())?;
    w.write_all(&h.packet_count.to_le_bytes())?;
    w.write_all(&h.total_payload_bytes.to_le_bytes())?;
    w.write_all(&h.duration_us.to_le_bytes())?;
    w.write_all(&h.reserved1.to_le_bytes())?;
    w.write_all(&h.crc32_header.to_le_bytes())?;
    debug_assert_eq!(HEADER_BYTES as usize, 64);
    Ok(())
}

fn write_packet(w: &mut impl Write, pkt: &PacketRecord) -> io::Result<()> {
    if pkt.aux.len() > u32::MAX as usize || pkt.payload.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "aux/payload 过长",
        ));
    }
    w.write_all(&pkt.relative_us.to_le_bytes())?;
    w.write_all(&pkt.channel_index.to_le_bytes())?;
    w.write_all(&[pkt.kind as u8, pkt.direction])?;
    w.write_all(&pkt.flags.to_le_bytes())?;
    w.write_all(&(pkt.aux.len() as u32).to_le_bytes())?;
    w.write_all(&(pkt.payload.len() as u32).to_le_bytes())?;
    w.write_all(&pkt.aux)?;
    w.write_all(&pkt.payload)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fcap::{ChannelType, PacketKind, SessionReader};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn roundtrip_two_udp_frames() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "fieldreplay_roundtrip_{}.fcap",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let channels = [ChannelEntry {
            channel_index: 0,
            channel_type: ChannelType::Udp,
            name: "UDP-9000".into(),
            config_json: r#"{"bind_ip":"127.0.0.1","bind_port":9000,"peer_ip":"","peer_port":0}"#
                .into(),
        }];
        let wall = 1_700_000_000_000_000u64;
        let mut w = SessionWriter::create(&path, wall, &channels, Some(r#"{"session_title":"t"}"#))
            .expect("create");
        let p1 = PacketRecord {
            relative_us: 1000,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: b"127.0.0.1:50000".to_vec(),
            payload: b"hello".to_vec(),
        };
        let p2 = PacketRecord {
            relative_us: 2500,
            channel_index: 0,
            kind: PacketKind::Data,
            direction: 0,
            flags: 0,
            aux: Vec::new(),
            payload: vec![0x01, 0x02, 0xFF],
        };
        w.append_packet(&p1).unwrap();
        w.append_packet(&p2).unwrap();
        w.finalize().unwrap();

        let mut r = SessionReader::open(&path).expect("open");
        assert!(r.header().is_finalized());
        assert_eq!(r.header().packet_count, 2);
        assert_eq!(r.header().duration_us, 2500);
        assert_eq!(r.header().total_payload_bytes, 8);
        assert_eq!(r.channels().len(), 1);
        assert_eq!(r.channels()[0].name, "UDP-9000");
        assert_eq!(r.meta_json().as_deref(), Some(r#"{"session_title":"t"}"#));

        let pkts = r.read_all_packets().unwrap();
        assert_eq!(pkts.len(), 2);
        assert_eq!(pkts[0].relative_us, 1000);
        assert_eq!(pkts[0].payload, b"hello");
        assert_eq!(pkts[1].relative_us, 2500);
        assert_eq!(pkts[1].payload, vec![0x01, 0x02, 0xFF]);

        let _ = std::fs::remove_file(&path);
    }
}
