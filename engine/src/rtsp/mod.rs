//! RTSP 通道（E7c）：最小客户端拉流（交织 TCP），按块落盘；不解 NAL。
//! 仅标准库。迷你服务端仅供 self-check。

mod client;
mod mini;
mod url;

pub use client::{encode_interleaved, RtspClient};
pub use mini::MiniRtspServer;
pub use url::RtspUrl;
