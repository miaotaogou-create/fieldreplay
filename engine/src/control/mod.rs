//! UI 本地控制面（WebSocket + 手写 JSON，仅标准库；可选同端口静态 UI）。

mod b64;
mod dto;
mod hub;
mod http_static;
mod json_util;
mod live;
mod replay_live;
mod server;
mod session_file;
mod sha1;
mod state;
mod ws;

pub use server::{self_check_control, serve, API_VERSION, DEFAULT_BIND, ENGINE_VERSION};
