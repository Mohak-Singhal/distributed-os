//! Time utilities.
//!
//! Thin helpers over `chrono` to avoid repeated boilerplate across crates.

use chrono::{DateTime, Utc};

/// Return the current UTC timestamp.
#[inline]
pub fn now() -> DateTime<Utc> {
    Utc::now()
}

/// Return the number of seconds elapsed since `then`.
/// Returns `0` if `then` is in the future.
pub fn secs_since(then: DateTime<Utc>) -> u64 {
    let delta = now() - then;
    delta.num_seconds().max(0) as u64
}

/// Return `true` if more than `secs` seconds have passed since `then`.
pub fn older_than(then: DateTime<Utc>, secs: u64) -> bool {
    secs_since(then) > secs
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn secs_since_past() {
        let past = now() - Duration::seconds(10);
        assert!(secs_since(past) >= 10);
    }

    #[test]
    fn secs_since_future_returns_zero() {
        let future = now() + Duration::seconds(60);
        assert_eq!(secs_since(future), 0);
    }

    #[test]
    fn older_than_detects_stale() {
        let old = now() - Duration::seconds(20);
        assert!(older_than(old, 15));
        assert!(!older_than(old, 30));
    }
}
