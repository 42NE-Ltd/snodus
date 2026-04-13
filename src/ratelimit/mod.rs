pub mod middleware;

use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub enum RateLimitResult {
    Allowed {
        remaining: u32,
        limit: u32,
        reset_secs: u64,
    },
    Denied {
        retry_after_secs: u64,
        limit: u32,
    },
}

#[derive(Debug, Clone, Copy)]
struct WindowState {
    count: u32,
    window_start: Instant,
}

#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<DashMap<Uuid, WindowState>>,
    window: Duration,
}

impl RateLimiter {
    pub fn new(window_secs: u64) -> Self {
        Self {
            inner: Arc::new(DashMap::new()),
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn check(&self, key_id: Uuid, limit: u32) -> RateLimitResult {
        let now = Instant::now();
        let window = self.window;

        let mut entry = self.inner.entry(key_id).or_insert(WindowState {
            count: 0,
            window_start: now,
        });

        if now.duration_since(entry.window_start) >= window {
            entry.window_start = now;
            entry.count = 0;
        }

        let elapsed = now.duration_since(entry.window_start);
        let reset_secs = window.saturating_sub(elapsed).as_secs();

        if entry.count >= limit {
            return RateLimitResult::Denied {
                retry_after_secs: reset_secs.max(1),
                limit,
            };
        }

        entry.count += 1;
        let remaining = limit.saturating_sub(entry.count);
        RateLimitResult::Allowed {
            remaining,
            limit,
            reset_secs,
        }
    }

    /// Remove stale entries whose window has fully expired.
    pub fn cleanup(&self) {
        let now = Instant::now();
        let window = self.window;
        self.inner
            .retain(|_, state| now.duration_since(state.window_start) < window * 2);
    }
}

pub fn start_cleanup_task(limiter: RateLimiter) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            limiter.cleanup();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn allows_up_to_limit() {
        let rl = RateLimiter::new(60);
        let key = Uuid::new_v4();
        for _ in 0..60 {
            assert!(matches!(rl.check(key, 60), RateLimitResult::Allowed { .. }));
        }
    }

    #[test]
    fn denies_over_limit() {
        let rl = RateLimiter::new(60);
        let key = Uuid::new_v4();
        for _ in 0..5 {
            rl.check(key, 5);
        }
        match rl.check(key, 5) {
            RateLimitResult::Denied {
                retry_after_secs,
                limit,
            } => {
                assert_eq!(limit, 5);
                assert!(retry_after_secs > 0);
            }
            _ => panic!("expected denied"),
        }
    }

    #[test]
    fn resets_after_window() {
        let rl = RateLimiter::new(1);
        let key = Uuid::new_v4();
        for _ in 0..3 {
            rl.check(key, 3);
        }
        assert!(matches!(rl.check(key, 3), RateLimitResult::Denied { .. }));
        sleep(Duration::from_millis(1100));
        assert!(matches!(rl.check(key, 3), RateLimitResult::Allowed { .. }));
    }

    #[test]
    fn different_keys_are_isolated() {
        let rl = RateLimiter::new(60);
        let k1 = Uuid::new_v4();
        let k2 = Uuid::new_v4();
        for _ in 0..3 {
            rl.check(k1, 3);
        }
        assert!(matches!(rl.check(k1, 3), RateLimitResult::Denied { .. }));
        assert!(matches!(rl.check(k2, 3), RateLimitResult::Allowed { .. }));
    }

    #[test]
    fn cleanup_removes_stale_entries() {
        let rl = RateLimiter::new(1);
        let key = Uuid::new_v4();
        rl.check(key, 10);
        assert_eq!(rl.inner.len(), 1);
        sleep(Duration::from_millis(2100));
        rl.cleanup();
        assert_eq!(rl.inner.len(), 0);
    }
}
