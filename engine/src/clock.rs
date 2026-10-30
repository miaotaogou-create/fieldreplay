//! 录制相对时间基（微秒）。

use std::sync::atomic::{AtomicBool, Ordering};
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

    /// 重放调度：自 `base` 起等到目标相对微秒（1x）。
    pub fn sleep_until_relative(base: Instant, relative_us: u64) {
        let _ = Self::sleep_until_relative_interruptible(
            base,
            relative_us,
            1.0,
            &AtomicBool::new(false),
        );
    }

    /// 将倍速夹到可用范围；非法值回退 1x。
    pub fn clamp_speed(speed: f64) -> f64 {
        if !speed.is_finite() || speed <= 0.0 {
            1.0
        } else {
            speed.clamp(0.05, 100.0)
        }
    }

    /// 可中断等待；`speed`>1 加快、`<1` 放慢。返回 `false` 表示被 `stop`。
    pub fn sleep_until_relative_interruptible(
        base: Instant,
        relative_us: u64,
        speed: f64,
        stop: &AtomicBool,
    ) -> bool {
        let speed = Self::clamp_speed(speed);
        let wall_secs = (relative_us as f64) / 1_000_000.0 / speed;
        let target = base + Duration::from_secs_f64(wall_secs.max(0.0));
        loop {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            let now = Instant::now();
            if target <= now {
                return true;
            }
            let remain = target - now;
            let slice = remain.min(Duration::from_millis(50));
            std::thread::sleep(slice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;

    #[test]
    fn clamp_speed_rejects_non_positive() {
        assert!((SessionClock::clamp_speed(0.0) - 1.0).abs() < f64::EPSILON);
        assert!((SessionClock::clamp_speed(-2.0) - 1.0).abs() < f64::EPSILON);
        assert!((SessionClock::clamp_speed(f64::NAN) - 1.0).abs() < f64::EPSILON);
        assert!((SessionClock::clamp_speed(2.0) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn speed_2x_reaches_target_sooner_than_1x() {
        let stop = AtomicBool::new(false);
        let base = Instant::now();
        assert!(SessionClock::sleep_until_relative_interruptible(
            base, 40_000, 2.0, &stop
        ));
        let elapsed_2x = base.elapsed();
        // 40ms@2x ≈ 20ms；允许调度抖动
        assert!(
            elapsed_2x.as_millis() < 35,
            "2x 应明显快于实时，实际 {:?}",
            elapsed_2x
        );
    }
}
