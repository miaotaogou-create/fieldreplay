//! FieldReplay 原生录放引擎库：`.fcap` 读写、UDP 通道、录制与时间轴重放。

pub mod clock;
pub mod fcap;
pub mod record;
pub mod replay;
pub mod udp;

pub use clock::SessionClock;
pub use fcap::{
    ChannelEntry, ChannelType, FileHeader, PacketKind, PacketRecord, SessionReader, SessionWriter,
    FLAG_FINALIZED, FLAG_HAS_META, FCAP_MAGIC, FCAP_VERSION_V1, HEADER_BYTES,
};
pub use record::{record_udp_to_file, RecordOptions};
pub use replay::{replay_udp_from_file, ReplayOptions};
