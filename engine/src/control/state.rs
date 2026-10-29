//! 控制面共享状态（会话载入 + 实时录制）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::fcap::{ChannelEntry, PacketRecord};

pub struct ActiveRecording {
    pub stop: Arc<AtomicBool>,
    pub out_path: PathBuf,
    pub channel_type: String,
    pub packet_count: Arc<AtomicU64>,
    pub join: Option<JoinHandle<std::io::Result<u64>>>,
}

#[derive(Clone)]
pub struct LoadedSession {
    pub source: String,
    pub start_wall_unix_us: u64,
    pub duration_us: u64,
    pub total_payload_bytes: u64,
    pub finalized: bool,
    pub meta_json: Option<String>,
    pub channels: Vec<ChannelEntry>,
    pub packets: Vec<PacketRecord>,
}

pub struct ControlState {
    pub session: Option<LoadedSession>,
    pub recording: Option<ActiveRecording>,
}

impl ControlState {
    pub fn new() -> Self {
        Self {
            session: None,
            recording: None,
        }
    }
}
