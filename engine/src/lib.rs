//! FieldReplay 原生录放引擎库：`.fcap` 读写、UDP/串口/TCP 通道、录制与时间轴重放。

pub mod clock;
pub mod fcap;
pub mod record;
pub mod replay;
pub mod serial;
pub mod tcp;
pub mod udp;

pub use clock::SessionClock;
pub use fcap::{
    ChannelEntry, ChannelType, FileHeader, PacketKind, PacketRecord, SessionReader, SessionWriter,
    FLAG_FINALIZED, FLAG_HAS_META, FCAP_MAGIC, FCAP_VERSION_V1, HEADER_BYTES,
};
pub use record::{
    record_serial_to_file, record_serial_with_port, record_tcp_to_file, record_udp_to_file,
    RecordOptions, SerialRecordOptions, TcpRecordOptions,
};
pub use replay::{
    replay_serial_from_file, replay_serial_with_port, replay_tcp_from_file, replay_udp_from_file,
    ReplayOptions, SerialReplayOptions, TcpReplayOptions,
};
pub use serial::{Parity, SerialConfig, SerialPort};
pub use tcp::{TcpConfig, TcpRole};
