//! 控制面共享状态（会话载入 + 实时录制 / 重放）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::fcap::{ChannelEntry, PacketRecord};
use crate::multi_record::LiveRecordSink;

pub struct ActiveRecording {
    pub stop: Arc<AtomicBool>,
    pub out_path: PathBuf,
    pub channel_type: String,
    pub packet_count: Arc<AtomicU64>,
    pub join: Option<JoinHandle<std::io::Result<u64>>>,
    /// 录制线程就绪后填入，供 `session.addMarker` 热写。
    pub live_sink: Arc<Mutex<Option<Arc<LiveRecordSink>>>>,
}

pub struct ActiveReplay {
    pub stop: Arc<AtomicBool>,
    /// 为真时冻结重放时间轴（不发下一帧），墙钟暂停时长从相对调度中扣除。
    pub paused: Arc<AtomicBool>,
    /// 挂起的 seek 目标相对微秒；`SessionClock::NO_SEEK` 表示无。
    pub seek_us: Arc<AtomicU64>,
    /// 线程自然结束或 stop 后置 true，供 status/下一次 start 回收槽位。
    pub finished: Arc<AtomicBool>,
    pub channel_type: String,
    pub sent_count: Arc<AtomicU64>,
    pub duration_us: u64,
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
    pub replaying: Option<ActiveReplay>,
}

impl ControlState {
    pub fn new() -> Self {
        Self {
            session: None,
            recording: None,
            replaying: None,
        }
    }

    /// 回收已结束的重放线程，避免自然播完后仍占着 `replaying` 槽。
    pub fn reap_finished_replay(&mut self) {
        let done = self
            .replaying
            .as_ref()
            .map(|a| a.finished.load(Ordering::SeqCst))
            .unwrap_or(false);
        if !done {
            return;
        }
        if let Some(mut active) = self.replaying.take() {
            if let Some(j) = active.join.take() {
                let _ = j.join();
            }
        }
    }
}
