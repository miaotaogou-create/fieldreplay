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
        let mut base = base;
        let never_paused = AtomicBool::new(false);
        let _ = Self::sleep_until_relative_interruptible(
            &mut base,
            relative_us,
            1.0,
            &AtomicBool::new(false),
            &never_paused,
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

    /// 可中断等待；`speed`>1 加快、`<1` 放慢。
    /// `paused` 为真时冻结时间轴：把暂停时长累加进 `base`，恢复后继续按相对时戳调度。
    /// 返回 `false` 表示被 `stop`。
    pub fn sleep_until_relative_interruptible(
        base: &mut Instant,
        relative_us: u64,
        speed: f64,
        stop: &AtomicBool,
        paused: &AtomicBool,
    ) -> bool {
        let speed = Self::clamp_speed(speed);
        let wall_from_start = Duration::from_secs_f64(
            ((relative_us as f64) / 1_000_000.0 / speed).max(0.0),
        );
        loop {
            if stop.load(Ordering::SeqCst) {
                return false;
            }
            if paused.load(Ordering::SeqCst) {
                let pause_started = Instant::now();
                while paused.load(Ordering::SeqCst) {
                    if stop.load(Ordering::SeqCst) {
                        return false;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                // 暂停期间墙钟流逝不计入相对时间轴
                *base += pause_started.elapsed();
                continue;
            }
            let target = *base + wall_from_start;
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
        let paused = AtomicBool::new(false);
        let mut base = Instant::now();
        let wall0 = Instant::now();
        assert!(SessionClock::sleep_until_relative_interruptible(
            &mut base, 40_000, 2.0, &stop, &paused
        ));
        let elapsed_2x = wall0.elapsed();
        // 40ms@2x ≈ 20ms；允许调度抖动
        assert!(
            elapsed_2x.as_millis() < 35,
            "2x 应明显快于实时，实际 {:?}",
            elapsed_2x
        );
    }

    #[test]
    fn pause_extends_wall_clock_without_advancing_timeline() {
        use std::sync::Arc;
        use std::thread;

        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new(AtomicBool::new(true));
        let stop_c = Arc::clone(&stop);
        let paused_c = Arc::clone(&paused);

        let handle = thread::spawn(move || {
            let mut base = Instant::now();
            let wall0 = Instant::now();
            // 目标 30ms@1x；若暂停不生效会很快返回
            let ok = SessionClock::sleep_until_relative_interruptible(
                &mut base, 30_000, 1.0, &stop_c, &paused_c,
            );
            (ok, wall0.elapsed())
        });

        thread::sleep(Duration::from_millis(80));
        paused.store(false, Ordering::SeqCst);
        let (ok, elapsed) = handle.join().expect("join");
        assert!(ok, "不应被 stop");
        assert!(
            elapsed.as_millis() >= 90,
            "暂停约 80ms + 等待 30ms，墙钟应 ≥90ms，实际 {:?}",
            elapsed
        );
    }
}
