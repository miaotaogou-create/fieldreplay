//! `.fcap` 顺序读。

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use super::types::{
    ChannelEntry, ChannelType, FileHeader, PacketKind, PacketRecord, FLAG_FINALIZED, FCAP_MAGIC,
    FCAP_VERSION_V1, HEADER_BYTES,
};

pub struct SessionReader {
    file: File,
    header: FileHeader,
    channels: Vec<ChannelEntry>,
    meta_json: Option<String>,
    packets_offset: u64,
}

impl SessionReader {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let header = read_header(&mut file)?;
        if header.magic != FCAP_MAGIC {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "不是 .fcap 文件（magic 不符）",
            ));
        }
        if header.version != FCAP_VERSION_V1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("不支持的 .fcap 版本: {}", header.version),
            ));
        }
        if header.header_bytes < HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "header_bytes 非法",
            ));
        }
        // v1 写端固定 64；若未来头更长则跳过多余字节
        if header.header_bytes as u64 > HEADER_BYTES as u64 {
            file.seek(SeekFrom::Start(header.header_bytes as u64))?;
        }

        let mut table_buf = vec![0u8; header.channel_table_bytes as usize];
        file.read_exact(&mut table_buf)?;
        let channels = decode_channel_table(&table_buf, header.channel_count)?;

        let meta_json = if header.meta_bytes > 0 {
            let mut meta = vec![0u8; header.meta_bytes as usize];
            file.read_exact(&mut meta)?;
            Some(
                String::from_utf8(meta)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
            )
        } else {
            None
        };

        let packets_offset = file.stream_position()?;

        if header.flags & FLAG_FINALIZED == 0 && header.packet_count == 0 {
            // 未收尾：仍允许扫到 EOF，由调用方决定是否接受
        }

        Ok(Self {
            file,
            header,
            channels,
            meta_json,
            packets_offset,
        })
    }

    pub fn header(&self) -> &FileHeader {
        &self.header
    }

    pub fn channels(&self) -> &[ChannelEntry] {
        &self.channels
    }

    pub fn meta_json(&self) -> Option<&str> {
        self.meta_json.as_deref()
    }

    pub fn rewind_packets(&mut self) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(self.packets_offset))?;
        Ok(())
    }

    /// 读下一条；EOF 返回 `None`。未知 kind 仍返回记录（kind 映射失败则跳过正文并继续——见下）。
    pub fn next_packet(&mut self) -> io::Result<Option<PacketRecord>> {
        match read_packet(&mut self.file) {
            Ok(p) => Ok(Some(p)),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn read_all_packets(&mut self) -> io::Result<Vec<PacketRecord>> {
        self.rewind_packets()?;
        let mut out = Vec::new();
        while let Some(p) = self.next_packet()? {
            out.push(p);
        }
        if self.header.is_finalized() && out.len() as u64 != self.header.packet_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "帧数与文件头不符: 读到 {}，头声明 {}",
                    out.len(),
                    self.header.packet_count
                ),
            ));
        }
        Ok(out)
    }
}

fn read_header(r: &mut impl Read) -> io::Result<FileHeader> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    Ok(FileHeader {
        magic,
        version: read_u16(r)?,
        header_bytes: read_u16(r)?,
        flags: read_u32(r)?,
        start_wall_unix_us: read_u64(r)?,
        channel_count: read_u16(r)?,
        reserved0: read_u16(r)?,
        channel_table_bytes: read_u32(r)?,
        meta_bytes: read_u32(r)?,
        packet_count: read_u64(r)?,
        total_payload_bytes: read_u64(r)?,
        duration_us: read_u64(r)?,
        reserved1: read_u32(r)?,
        crc32_header: read_u32(r)?,
    })
}

fn decode_channel_table(buf: &[u8], expected_count: u16) -> io::Result<Vec<ChannelEntry>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < buf.len() {
        if buf.len() - i < 8 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "通道表截断",
            ));
        }
        let channel_index = u16::from_le_bytes([buf[i], buf[i + 1]]);
        let type_u8 = buf[i + 2];
        // buf[i+3] reserved
        let name_len = u16::from_le_bytes([buf[i + 4], buf[i + 5]]) as usize;
        let config_len = u16::from_le_bytes([buf[i + 6], buf[i + 7]]) as usize;
        i += 8;
        if i + name_len + config_len > buf.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "通道表 name/config 截断",
            ));
        }
        let name = String::from_utf8(buf[i..i + name_len].to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        i += name_len;
        let config_json = String::from_utf8(buf[i..i + config_len].to_vec())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        i += config_len;

        let channel_type = ChannelType::from_u8(type_u8).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("未知通道类型: {type_u8}"),
            )
        })?;
        out.push(ChannelEntry {
            channel_index,
            channel_type,
            name,
            config_json,
        });
    }
    if out.len() as u16 != expected_count {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "通道表条数不符: 解析 {}，头声明 {}",
                out.len(),
                expected_count
            ),
        ));
    }
    Ok(out)
}

fn read_packet(r: &mut impl Read) -> io::Result<PacketRecord> {
    let relative_us = read_u64(r)?;
    let channel_index = read_u16(r)?;
    let mut kd = [0u8; 2];
    r.read_exact(&mut kd)?;
    let direction = kd[1];
    let flags = read_u32(r)?;
    let aux_len = read_u32(r)? as usize;
    let payload_len = read_u32(r)? as usize;
    let mut aux = vec![0u8; aux_len];
    r.read_exact(&mut aux)?;
    let mut payload = vec![0u8; payload_len];
    r.read_exact(&mut payload)?;

    // ponytail: 未知 kind 仍读满 aux+payload 以便继续扫；权宜映射为 Data（载荷保留）。
    let kind = PacketKind::from_u8(kd[0]).unwrap_or(PacketKind::Data);

    Ok(PacketRecord {
        relative_us,
        channel_index,
        kind,
        direction,
        flags,
        aux,
        payload,
    })
}

fn read_u16(r: &mut impl Read) -> io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
