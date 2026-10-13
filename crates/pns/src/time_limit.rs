//! Port of `pcbnew/router/time_limit.{h,cpp}`: `PNS::TIME_LIMIT`, the clock `SHOVE::shoveMainLoop` stops a shove by
//! (`ROUTING_SETTINGS::ShoveTimeLimit()`, the `shove_time_limit` key of the router settings, 1000 ms).
//!
//! `wxGetLocalTimeMillis()` is the wall clock in milliseconds; here it is [`std::time::Instant`], which cannot run backwards.
//! `Expired()` is `elapsed >= limit`, so a limit of 0 (or less) is expired from the first look: the shove then gets exactly
//! one iteration, as in KiCad, where the loop checks after the iteration, not before it.
use std::time::{Duration, Instant};

/// `PNS::TIME_LIMIT`.
#[derive(Debug, Clone, Copy)]
pub struct TimeLimit {
    limit_ms: i64,
    start: Instant,
}

impl TimeLimit {
    /// `TIME_LIMIT( aMilliseconds )`: the clock starts with the limit (`Restart()` in the constructor).
    pub fn new(limit_ms: i32) -> Self {
        TimeLimit { limit_ms: i64::from(limit_ms), start: Instant::now() }
    }

    /// `TIME_LIMIT::Expired`: `now - start >= limit`.
    pub fn expired(&self) -> bool {
        self.expired_after(self.start.elapsed())
    }

    /// `Expired` for a given time since the start (what the clock reads, whole milliseconds as `wxGetLocalTimeMillis` counts them).
    fn expired_after(&self, elapsed: Duration) -> bool {
        i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX) >= self.limit_ms
    }

    /// `TIME_LIMIT::Restart`.
    pub fn restart(&mut self) {
        self.start = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_when_the_elapsed_milliseconds_reach_the_limit() {
        let t = TimeLimit::new(1000);
        assert!(!t.expired_after(Duration::from_millis(0)));
        assert!(!t.expired_after(Duration::from_millis(999)));
        assert!(!t.expired_after(Duration::from_micros(999_999)), "the clock counts whole milliseconds");
        assert!(t.expired_after(Duration::from_millis(1000)), "Expired() is >=, not >");
        assert!(t.expired_after(Duration::from_secs(5)));
    }

    #[test]
    fn a_limit_of_nothing_is_expired_at_once() {
        for ms in [0, -1, i32::MIN] {
            assert!(TimeLimit::new(ms).expired(), "limit {ms}");
        }
    }

    #[test]
    fn a_long_limit_does_not_expire_and_restart_keeps_it_running() {
        let mut t = TimeLimit::new(i32::MAX);
        assert!(!t.expired());
        t.restart();
        assert!(!t.expired());
    }
}
