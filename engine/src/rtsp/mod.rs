//! RTSP 通道：E7c 客户端拉流；E7d 服务侧重放（交织 TCP）；不解 NAL。
//! 仅标准库。迷你服务端仅供 self-check。

mod client;
mod mini;
mod proto;
mod serve;
mod url;

pub use client::{encode_interleaved, RtspClient};
pub use mini::MiniRtspServer;
pub use serve::{accept_until_play, poll_client_signal};
pub use url::RtspUrl;
