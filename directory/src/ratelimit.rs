//! Ephemeral rate limiting (T9, T20, T22). Tracks *counts* per caller,
//! never query content — there is no field anywhere in this module capable
//! of holding a search prefix, so "no query-content logging" is structural,
//! not a policy someone has to remember to follow. State resets on
//! restart, same as relay's per-connection abuse bookkeeping.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// T20: accounts that signed up but never completed `/verify` get a
/// measurably tighter limit than verified ones. (Not "degraded verification
/// per T2" — ET6 deleted that path; `find_or_create_pending_user` is now the only
/// thing that writes `verified = false`.)
pub const VERIFIED_SEARCH_PER_MINUTE: u32 = 30;
pub const UNVERIFIED_SEARCH_PER_MINUTE: u32 = 5;

/// ET1: guesses per minute against **one number's** OTP. Sized to a human who
/// mistypes, not to a script: at 5/min a 6-digit code takes ~4 months of
/// uninterrupted guessing, and the vendor's own per-verification cap (Twilio
/// Verify: 5 checks) usually bites first. That cap is the point — it is a
/// property of whichever vendor happens to be wired up, and `OtpVendor` exists
/// to make swapping vendors a one-impl-block affair, so leaning on it alone
/// means a future vendor without a cap silently removes the only defense.
pub const VERIFY_ATTEMPTS_PER_MINUTE: u32 = 5;

// Compile-time guard (stronger than a runtime test): the build fails
// outright if someone flips these so unverified is no longer tighter.
const _: () = assert!(UNVERIFIED_SEARCH_PER_MINUTE < VERIFIED_SEARCH_PER_MINUTE);

/// Which population a key belongs to.
///
/// Without this, `/verify`'s phone-derived keys and `/search`'s authenticated
/// `user_id`s would share one namespace: a phone whose digest happened to equal
/// user 42's id would throttle user 42's searches, from an endpoint they never
/// called. Two number spaces, two buckets.
#[derive(PartialEq, Eq, Hash, Clone, Copy)]
enum Bucket {
    /// An authenticated `user_id`.
    Caller,
    /// An unauthenticated caller, keyed by the number they are claiming.
    Phone,
}

/// A rate-limit key for an unauthenticated `/verify` caller, from the number
/// they claim to own.
///
/// Deliberately **not** the Argon2id `phone_hash`: that one is expensive by
/// design, and after ET6 it is computed only for a code the vendor already
/// approved — a rejected code never produces one, which is exactly the case
/// this limit exists to stop. The key has to be cheap and available before the
/// vendor call, or it cannot guard either the guess or the worker the call pins.
///
/// Not a privacy primitive: a 64-bit unkeyed digest over a space as small as
/// phone numbers is enumerable, so this must never be logged, persisted, or
/// sent anywhere. It is a map key for a counter, in a process-local map that
/// dies on restart — the module still holds no phone number and no query
/// content, which is the property the header claims.
pub fn phone_rate_key(e164: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    e164.hash(&mut h);
    h.finish()
}

struct Window {
    count: u32,
    started: Instant,
}

#[derive(Default)]
pub struct RateLimiter {
    windows: Mutex<HashMap<(Bucket, u64), Window>>,
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
        self.bump(Bucket::Caller, caller_id, limit_per_minute)
    }

    /// ET1: the same window, for a caller who has not authenticated and cannot
    /// — `/verify` is how you *get* a session. Keyed by [`phone_rate_key`], so
    /// the limit is per number: one victim's number cannot be brute-forced
    /// faster by spreading guesses across connections.
    ///
    /// ponytail: per-number only. A flood across *many* numbers still pins one
    /// 30s vendor worker each, and stopping that needs a per-IP limit — which
    /// needs `ConnectInfo` plus a decision about trusting `X-Forwarded-For`
    /// behind the reverse proxy this service is designed to sit behind. That is
    /// a deployment call, not a code one. See the deferred note in ET1.
    pub fn check_and_bump_phone(&self, phone_key: u64, limit_per_minute: u32) -> bool {
        self.bump(Bucket::Phone, phone_key, limit_per_minute)
    }

    fn bump(&self, bucket: Bucket, key: u64, limit_per_minute: u32) -> bool {
        let mut windows = self.windows.lock().unwrap();
        let window = windows.entry((bucket, key)).or_insert_with(|| Window {
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

    /// ET1: the guess limit is what stops a 6-digit code being brute-forced.
    #[test]
    fn otp_guesses_against_one_number_hit_a_wall() {
        let limiter = RateLimiter::new();
        let key = phone_rate_key("+15551234567");
        let mut allowed = 0;
        for _ in 0..(VERIFY_ATTEMPTS_PER_MINUTE * 4) {
            if limiter.check_and_bump_phone(key, VERIFY_ATTEMPTS_PER_MINUTE) {
                allowed += 1;
            }
        }
        assert_eq!(
            allowed, VERIFY_ATTEMPTS_PER_MINUTE,
            "a flood against one number must not buy more than the window"
        );
    }

    #[test]
    fn one_throttled_number_does_not_lock_out_everyone_else() {
        let limiter = RateLimiter::new();
        let victim = phone_rate_key("+15551234567");
        for _ in 0..VERIFY_ATTEMPTS_PER_MINUTE {
            assert!(limiter.check_and_bump_phone(victim, VERIFY_ATTEMPTS_PER_MINUTE));
        }
        assert!(!limiter.check_and_bump_phone(victim, VERIFY_ATTEMPTS_PER_MINUTE));

        let bystander = phone_rate_key("+15559998888");
        assert!(
            limiter.check_and_bump_phone(bystander, VERIFY_ATTEMPTS_PER_MINUTE),
            "attacking one number must not deny service to every other signup"
        );
    }

    /// The reason `Bucket` exists. `/verify`'s keys are digests and `/search`'s
    /// are `user_id`s — two number spaces in one map. Without the namespace, a
    /// number whose digest collided with a real user id would throttle that
    /// user's searches from an endpoint they never touched. Forced here rather
    /// than waited for: a natural collision is a ~1-in-2^64 flake nobody could
    /// debug, so the test supplies the collision itself.
    #[test]
    fn a_phone_key_cannot_throttle_the_user_whose_id_it_collides_with() {
        let limiter = RateLimiter::new();
        let collision: u64 = 42;

        for _ in 0..VERIFY_ATTEMPTS_PER_MINUTE {
            assert!(limiter.check_and_bump_phone(collision, VERIFY_ATTEMPTS_PER_MINUTE));
        }
        assert!(!limiter.check_and_bump_phone(collision, VERIFY_ATTEMPTS_PER_MINUTE));

        assert!(
            limiter.check_and_bump(collision, VERIFIED_SEARCH_PER_MINUTE),
            "user 42's searches must be unaffected by a phone digest that happens to equal 42"
        );
    }

    #[test]
    fn the_phone_key_is_stable_and_number_specific() {
        assert_eq!(
            phone_rate_key("+15551234567"),
            phone_rate_key("+15551234567")
        );
        assert_ne!(
            phone_rate_key("+15551234567"),
            phone_rate_key("+15551234568")
        );
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
