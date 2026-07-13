//! Identity/directory service — phone verification and (later) username
//! search. Depends only on `proto`; must never depend on `core`/`engine` so
//! the directory process stays crash-isolated from the relay/client path.

pub mod api;
pub mod config;
pub mod ratelimit;
pub mod search;
pub mod store;
pub mod username;
pub mod verify;

pub use api::{bind_and_serve, spawn_for_tests, AppState};
pub use config::DirectoryConfig;
pub use ratelimit::RateLimiter;
pub use search::{hash_prefix, search_by_prefix, InMemoryIndex, PhoneEntry, PrefixIndex};
pub use store::{ClaimError, DirectoryStore};
pub use username::{format_handle, render_discriminator, validate_nickname, UsernameError};
pub use verify::{
    normalize_e164, phone_hash, vendor_from_env, verify_phone, DevOtpVendor, OtpVendor, Pepper,
    TwilioOtpVendor, VendorError, VerificationOutcome, VerifyError,
};
