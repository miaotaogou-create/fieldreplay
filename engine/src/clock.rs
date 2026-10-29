//! 录制相对时间基（微秒）。

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 会话时钟：录制起点墙钟 + 相对微秒。
#[derive(Debug, Clone)]
pub struct SessionClock {
    start: Instant,
    start_wall_unix_us: u64,
}

impl SessionClock {
    pub fn start() -> Self {
        let start_wall_unix_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        Self {
            start: Instant::now(),
            start_wall_unix_us,
        }
    }

    pub fn start_wall_unix_us(&self) -> u64 {
        self.start_wall_unix_us
    }

    pub fn relative_us(&self) -> u64 {
        self.start.elapsed().as_micros() as u64
    }

    /// 重放调度：自 `base` 起等到目标相对微秒。
    pub fn sleep_until_relative(base: Instant, relative_us: u64) {
        let target = base + Duration::from_micros(relative_us);
        let now = Instant::now();
        if target > now {
            std::thread::sleep(target - now);
        }
    }
}
