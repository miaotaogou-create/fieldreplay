//! `.fcap` v1 读写（契约见 `docs/design/fcap-format.md`）。

mod reader;
mod types;
mod writer;

pub use reader::SessionReader;
pub use types::{
    ChannelEntry, ChannelType, FileHeader, PacketKind, PacketRecord, FLAG_FINALIZED, FLAG_HAS_META,
    FCAP_MAGIC, FCAP_VERSION_V1, HEADER_BYTES, PACKET_FIXED_BYTES,
};
pub use writer::SessionWriter;
