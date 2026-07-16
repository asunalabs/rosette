//! Ephemeral rate limiting (T9, T20, T22). Tracks *counts* per caller,
//! never query content — there is no field anywhere in this module capable
//! of holding a search prefix, so "no query-content logging" is structural,
//! not a policy someone has to remember to follow. State resets on
//! restart, same as relay's per-connection abuse bookkeeping.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// T20: accounts that signed up but never completed `/verify` get a
/// measurably tighter limit than verified ones. (Not "degraded verification
/// per T2" — ET6 deleted that path; `find_or_create_pending_user` is now the only
/// thing that writes `verified = false`.)
pub const VERIFIED_SEARCH_PER_MINUTE: u32 = 30;
pub const UNVERIFIED_SEARCH_PER_MINUTE: u32 = 5;

// Compile-time guard (stronger than a runtime test): the build fails
// outright if someone flips these so unverified is no longer tighter.
const _: () = assert!(UNVERIFIED_SEARCH_PER_MINUTE < VERIFIED_SEARCH_PER_MINUTE);

struct Window {
    count: u32,
    started: Instant,
}

#[derive(Default)]
pub struct RateLimiter {
    windows: Mutex<HashMap<u64, Window>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if this call is within `limit_per_minute` for
    /// `caller_id` (and counts it); `false` if the caller is over the
    /// limit. `caller_id` is whatever the API layer authenticated — never
    /// the thing being searched for.
    pub fn check_and_bump(&self, caller_id: u64, limit_per_minute: u32) -> bool {
        let mut windows = self.windows.lock().unwrap();
        let window = windows.entry(caller_id).or_insert_with(|| Window {
            count: 0,
            started: Instant::now(),
        });
        if window.started.elapsed() >= Duration::from_secs(60) {
            window.started = Instant::now();
            window.count = 0;
        }
        if window.count >= limit_per_minute {
            return false;
        }
        window.count += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn high_volume_sequential_search_gets_rate_limited() {
        let limiter = RateLimiter::new();
        let mut allowed = 0;
        let mut denied = 0;
        for _ in 0..(VERIFIED_SEARCH_PER_MINUTE * 3) {
            if limiter.check_and_bump(42, VERIFIED_SEARCH_PER_MINUTE) {
                allowed += 1;
            } else {
                denied += 1;
            }
        }
        assert_eq!(allowed, VERIFIED_SEARCH_PER_MINUTE);
        assert!(
            denied > 0,
            "bulk sequential search from one account must hit the limit"
        );
    }

    #[test]
    fn unverified_caller_hits_the_wall_sooner_than_a_verified_one() {
        let limiter = RateLimiter::new();
        let mut unverified_allowed = 0;
        let mut verified_allowed = 0;
        for _ in 0..VERIFIED_SEARCH_PER_MINUTE {
            if limiter.check_and_bump(1, UNVERIFIED_SEARCH_PER_MINUTE) {
                unverified_allowed += 1;
            }
            if limiter.check_and_bump(2, VERIFIED_SEARCH_PER_MINUTE) {
                verified_allowed += 1;
            }
        }
        assert!(unverified_allowed < verified_allowed);
    }

    #[test]
    fn different_callers_have_independent_windows() {
        let limiter = RateLimiter::new();
        for _ in 0..UNVERIFIED_SEARCH_PER_MINUTE {
            assert!(limiter.check_and_bump(1, UNVERIFIED_SEARCH_PER_MINUTE));
        }
        assert!(!limiter.check_and_bump(1, UNVERIFIED_SEARCH_PER_MINUTE));
        // A different caller isn't affected by caller 1's exhausted window.
        assert!(limiter.check_and_bump(2, UNVERIFIED_SEARCH_PER_MINUTE));
    }
}
