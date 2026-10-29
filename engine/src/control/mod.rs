//! UI 本地控制面（WebSocket + 手写 JSON，仅标准库）。

mod b64;
mod dto;
mod hub;
mod json_util;
mod live;
mod replay_live;
mod server;
mod session_file;
mod sha1;
mod state;
mod ws;

pub use server::{self_check_control, serve, API_VERSION, DEFAULT_BIND, ENGINE_VERSION};
