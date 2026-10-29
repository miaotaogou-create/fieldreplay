//! FieldReplay 原生录放引擎库：`.fcap` 读写、UDP/串口通道、录制与时间轴重放。

pub mod clock;
pub mod fcap;
pub mod record;
pub mod replay;
pub mod serial;
pub mod udp;

pub use clock::SessionClock;
pub use fcap::{
    ChannelEntry, ChannelType, FileHeader, PacketKind, PacketRecord, SessionReader, SessionWriter,
    FLAG_FINALIZED, FLAG_HAS_META, FCAP_MAGIC, FCAP_VERSION_V1, HEADER_BYTES,
};
pub use record::{
    record_serial_to_file, record_serial_with_port, record_udp_to_file, RecordOptions,
    SerialRecordOptions,
};
pub use replay::{
    replay_serial_from_file, replay_serial_with_port, replay_udp_from_file, ReplayOptions,
    SerialReplayOptions,
};
pub use serial::{Parity, SerialConfig, SerialPort};
