//! 模组级 panic 熔断计数，见 `docs/01-specs/error-handling-spec.md` §4。

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(crate) const DEFAULT_WINDOW: Duration = Duration::from_secs(10);
pub(crate) const DEFAULT_THRESHOLD: usize = 16;

/// 滑动窗口 panic 计数器。
pub struct CircuitBreaker {
    window: Duration,
    threshold: usize,
    hits: VecDeque<Instant>,
    tripped: bool,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(DEFAULT_WINDOW, DEFAULT_THRESHOLD)
    }
}

impl CircuitBreaker {
    pub fn new(window: Duration, threshold: usize) -> Self {
        Self {
            window,
            threshold,
            hits: VecDeque::new(),
            tripped: false,
        }
    }

    /// 记录一次 panic；首次越过阈值时返回 `true`（每次熔断只报告一次）。
    pub fn record(&mut self, now: Instant) -> bool {
        while let Some(&front) = self.hits.front() {
            if now.duration_since(front) > self.window {
                self.hits.pop_front();
            } else {
                break;
            }
        }
        self.hits.push_back(now);
        if !self.tripped && self.hits.len() > self.threshold {
            self.tripped = true;
            return true;
        }
        false
    }

    /// 模组重新启用时复位。
    pub fn reset(&mut self) {
        self.hits.clear();
        self.tripped = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trips_once_over_threshold() {
        let mut b = CircuitBreaker::new(Duration::from_secs(10), 3);
        let t = Instant::now();
        assert!(!b.record(t));
        assert!(!b.record(t));
        assert!(!b.record(t));
        assert!(b.record(t));
        assert!(!b.record(t));
        b.reset();
        assert!(!b.record(t));
    }

    #[test]
    fn old_hits_expire() {
        let mut b = CircuitBreaker::new(Duration::from_secs(10), 1);
        let t = Instant::now();
        assert!(!b.record(t));
        assert!(!b.record(t + Duration::from_secs(11)));
    }
}
