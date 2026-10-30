//! 录制相对时间基（微秒）；重放可中断等待。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// 录制暂停共享状态：多线程读 `relative_us`，控制面调 pause/resume。
#[derive(Debug)]
struct RecordPause {
    paused: AtomicBool,
    /// 已累计暂停墙钟（微秒），从墙钟流逝中扣除。
    accum_us: AtomicU64,
    /// 进入暂停时冻结的相对戳；未暂停为 [`SessionClock::NO_FREEZE`]。
    frozen_us: AtomicU64,
}

impl Default for RecordPause {
    fn default() -> Self {
        Self {
            paused: AtomicBool::new(false),
            accum_us: AtomicU64::new(0),
            frozen_us: AtomicU64::new(SessionClock::NO_FREEZE),
        }
    }
}

/// 会话时钟：录制起点墙钟 + 相对微秒（可冻结）。
#[derive(Debug, Clone)]
pub struct SessionClock {
    start: Instant,
    start_wall_unix_us: u64,
    pause: Arc<RecordPause>,
}

impl SessionClock {
    /// 未冻结时的哨兵。
    pub const NO_FREEZE: u64 = u64::MAX;

    pub fn start() -> Self {
        let start_wall_unix_us = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
        Self {
            start: Instant::now(),
            start_wall_unix_us,
            pause: Arc::new(RecordPause::default()),
        }
    }

    pub fn start_wall_unix_us(&self) -> u64 {
        self.start_wall_unix_us
    }

    /// 会话相对微秒；暂停中返回冻结值，恢复后从该值继续（暂停墙钟不计入）。
    pub fn relative_us(&self) -> u64 {
        if self.pause.paused.load(Ordering::SeqCst) {
            let f = self.pause.frozen_us.load(Ordering::SeqCst);
            if f != Self::NO_FREEZE {
                return f;
            }
        }
        let raw = self.start.elapsed().as_micros() as u64;
        raw.saturating_sub(self.pause.accum_us.load(Ordering::SeqCst))
    }

    pub fn is_paused(&self) -> bool {
        self.pause.paused.load(Ordering::SeqCst)
    }

    /// 冻结相对轴。已暂停返回 `false`（幂等）。
    pub fn pause(&self) -> bool {
        if self.pause.paused.load(Ordering::SeqCst) {
            return false;
        }
        let raw = self.start.elapsed().as_micros() as u64;
        let accum = self.pause.accum_us.load(Ordering::SeqCst);
        let frozen = raw.saturating_sub(accum);
        self.pause.frozen_us.store(frozen, Ordering::SeqCst);
        self.pause.paused.store(true, Ordering::SeqCst);
        true
    }

    /// 解除冻结，相对轴从冻结值继续。未暂停返回 `false`（幂等）。
    pub fn resume(&self) -> bool {
        if !self.pause.paused.load(Ordering::SeqCst) {
            return false;
        }
        let frozen = self.pause.frozen_us.load(Ordering::SeqCst);
        let frozen = if frozen == Self::NO_FREEZE {
            let raw = self.start.elapsed().as_micros() as u64;
            raw.saturating_sub(self.pause.accum_us.load(Ordering::SeqCst))
        } else {
            frozen
        };
        let raw = self.start.elapsed().as_micros() as u64;
        self.pause
            .accum_us
            .store(raw.saturating_sub(frozen), Ordering::SeqCst);
        self.pause.frozen_us.store(Self::NO_FREEZE, Ordering::SeqCst);
        self.pause.paused.store(false, Ordering::SeqCst);
        true
    }

    /// 无挂起 seek 时的哨兵值（存入 `AtomicU64`）。
    pub const NO_SEEK: u64 = u64::MAX;

    /// 重放调度：自 `base` 起等到目标相对微秒（1x）。
    pub fn sleep_until_relative(base: Instant, relative_us: u64) {
        let mut base = base;
        let never_paused = AtomicBool::new(false);
        let no_seek = AtomicU64::new(Self::NO_SEEK);
        let _ = Self::sleep_until_relative_interruptible(
            &mut base,
            relative_us,
            1.0,
            &AtomicBool::new(false),
            &never_paused,
            &no_seek,
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

    /// 按相对微秒与倍速，把 `base` 校准为「此刻对应时间轴上的 `relative_us`」。
    pub fn rebase_timeline(base: &mut Instant, relative_us: u64, speed: f64) {
        let speed = Self::clamp_speed(speed);
        let wall = Duration::from_secs_f64(
            ((relative_us as f64) / 1_000_000.0 / speed).max(0.0),
        );
        *base = Instant::now().checked_sub(wall).unwrap_or_else(Instant::now);
    }

    /// 可中断等待；`speed`>1 加快、`<1` 放慢。
    /// `paused` 为真时冻结时间轴：把暂停时长累加进 `base`，恢复后继续按相对时戳调度。
    /// `seek_us != NO_SEEK` 时立即返回 [`WaitResult::Seek`]。
    pub fn sleep_until_relative_interruptible(
        base: &mut Instant,
        relative_us: u64,
        speed: f64,
        stop: &AtomicBool,
        paused: &AtomicBool,
        seek_us: &AtomicU64,
    ) -> WaitResult {
        let speed = Self::clamp_speed(speed);
        let wall_from_start = Duration::from_secs_f64(
            ((relative_us as f64) / 1_000_000.0 / speed).max(0.0),
        );
        loop {
            if stop.load(Ordering::SeqCst) {
                return WaitResult::Stopped;
            }
            if seek_us.load(Ordering::SeqCst) != Self::NO_SEEK {
                return WaitResult::Seek;
            }
            if paused.load(Ordering::SeqCst) {
                let pause_started = Instant::now();
                while paused.load(Ordering::SeqCst) {
                    if stop.load(Ordering::SeqCst) {
                        return WaitResult::Stopped;
                    }
                    if seek_us.load(Ordering::SeqCst) != Self::NO_SEEK {
                        // 暂停中 seek：仍扣掉已暂停墙钟，避免恢复后时间轴跳
                        *base += pause_started.elapsed();
                        return WaitResult::Seek;
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
                return WaitResult::Ready;
            }
            let remain = target - now;
            let slice = remain.min(Duration::from_millis(50));
            std::thread::sleep(slice);
        }
    }
}

/// 重放等待结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitResult {
    Ready,
    Stopped,
    Seek,
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
        let seek = AtomicU64::new(SessionClock::NO_SEEK);
        let mut base = Instant::now();
        let wall0 = Instant::now();
        assert_eq!(
            SessionClock::sleep_until_relative_interruptible(
                &mut base, 40_000, 2.0, &stop, &paused, &seek
            ),
            WaitResult::Ready
        );
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
        let seek = Arc::new(AtomicU64::new(SessionClock::NO_SEEK));
        let stop_c = Arc::clone(&stop);
        let paused_c = Arc::clone(&paused);
        let seek_c = Arc::clone(&seek);

        let handle = thread::spawn(move || {
            let mut base = Instant::now();
            let wall0 = Instant::now();
            // 目标 30ms@1x；若暂停不生效会很快返回
            let r = SessionClock::sleep_until_relative_interruptible(
                &mut base, 30_000, 1.0, &stop_c, &paused_c, &seek_c,
            );
            (r, wall0.elapsed())
        });

        thread::sleep(Duration::from_millis(80));
        paused.store(false, Ordering::SeqCst);
        let (r, elapsed) = handle.join().expect("join");
        assert_eq!(r, WaitResult::Ready, "不应被 stop/seek");
        assert!(
            elapsed.as_millis() >= 90,
            "暂停约 80ms + 等待 30ms，墙钟应 ≥90ms，实际 {:?}",
            elapsed
        );
    }

    #[test]
    fn seek_flag_aborts_wait() {
        let stop = AtomicBool::new(false);
        let paused = AtomicBool::new(false);
        let seek = AtomicU64::new(12_345);
        let mut base = Instant::now();
        assert_eq!(
            SessionClock::sleep_until_relative_interruptible(
                &mut base, 1_000_000, 1.0, &stop, &paused, &seek
            ),
            WaitResult::Seek
        );
    }

    #[test]
    fn record_pause_freezes_relative_us() {
        let clock = SessionClock::start();
        std::thread::sleep(Duration::from_millis(30));
        assert!(clock.pause());
        assert!(!clock.pause(), "已暂停应幂等");
        assert!(clock.is_paused());
        let frozen = clock.relative_us();
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(
            clock.relative_us(),
            frozen,
            "暂停中相对轴应冻结"
        );
        assert!(clock.resume());
        assert!(!clock.resume(), "未暂停应幂等");
        std::thread::sleep(Duration::from_millis(20));
        let after = clock.relative_us();
        assert!(
            after >= frozen,
            "恢复后相对轴应 ≥ 冻结值: frozen={frozen} after={after}"
        );
        // 暂停 80ms 墙钟不应计入；恢复后仅再走约 20ms
        assert!(
            after.saturating_sub(frozen) < 60_000,
            "暂停墙钟不应计入相对轴: delta={}",
            after - frozen
        );
    }
}
