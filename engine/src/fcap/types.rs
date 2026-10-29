//! `.fcap` v1 常量与内存结构。

pub const FCAP_MAGIC: [u8; 4] = *b"FCAP";
pub const FCAP_VERSION_V1: u16 = 0x0001;
pub const HEADER_BYTES: u16 = 64;
pub const PACKET_FIXED_BYTES: usize = 24;

pub const FLAG_FINALIZED: u32 = 1 << 0;
pub const FLAG_HAS_META: u32 = 1 << 1;

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelType {
    Udp = 1,
    Serial = 2,
    Tcp = 3,
    Mqtt = 4,
    Video = 5,
}

impl ChannelType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Udp),
            2 => Some(Self::Serial),
            3 => Some(Self::Tcp),
            4 => Some(Self::Mqtt),
            5 => Some(Self::Video),
            _ => None,
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketKind {
    Data = 0,
    TcpConnect = 1,
    TcpDisconnect = 2,
    Marker = 3,
}

impl PacketKind {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Data),
            1 => Some(Self::TcpConnect),
            2 => Some(Self::TcpDisconnect),
            3 => Some(Self::Marker),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileHeader {
    pub magic: [u8; 4],
    pub version: u16,
    pub header_bytes: u16,
    pub flags: u32,
    pub start_wall_unix_us: u64,
    pub channel_count: u16,
    pub reserved0: u16,
    pub channel_table_bytes: u32,
    pub meta_bytes: u32,
    pub packet_count: u64,
    pub total_payload_bytes: u64,
    pub duration_us: u64,
    pub reserved1: u32,
    pub crc32_header: u32,
}

impl FileHeader {
    pub fn new_open(
        start_wall_unix_us: u64,
        channel_count: u16,
        channel_table_bytes: u32,
        meta_bytes: u32,
    ) -> Self {
        let mut flags = 0u32;
        if meta_bytes > 0 {
            flags |= FLAG_HAS_META;
        }
        Self {
            magic: FCAP_MAGIC,
            version: FCAP_VERSION_V1,
            header_bytes: HEADER_BYTES,
            flags,
            start_wall_unix_us,
            channel_count,
            reserved0: 0,
            channel_table_bytes,
            meta_bytes,
            packet_count: 0,
            total_payload_bytes: 0,
            duration_us: 0,
            reserved1: 0,
            crc32_header: 0,
        }
    }

    pub fn is_finalized(&self) -> bool {
        self.flags & FLAG_FINALIZED != 0
    }
}

#[derive(Debug, Clone)]
pub struct ChannelEntry {
    pub channel_index: u16,
    pub channel_type: ChannelType,
    pub name: String,
    pub config_json: String,
}

#[derive(Debug, Clone)]
pub struct PacketRecord {
    pub relative_us: u64,
    pub channel_index: u16,
    pub kind: PacketKind,
    /// 0 = RX，1 = TX
    pub direction: u8,
    pub flags: u32,
    pub aux: Vec<u8>,
    pub payload: Vec<u8>,
}
