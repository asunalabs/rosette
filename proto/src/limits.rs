//! Relay resource limits (amendment A7). A public, no-account relay without
//! caps is an outage waiting to happen — these are protocol, not config
//! afterthoughts, so clients handle rejection from day one.

/// Matches the largest padding bucket in envelope.rs — a message that doesn't
/// fit any bucket is rejected before padding, not silently truncated.
pub const MAX_MESSAGE_SIZE: usize = 65536;

/// Max undelivered entries a single queue may hold. Amendment A3's fan-out
/// journal entries count against the recipient queue they target.
pub const MAX_QUEUE_DEPTH: usize = 1000;

/// Total on-disk bound for one relay instance's default configuration.
/// Deliberately conservative for a stranger's first VPS deploy; operators can
/// raise it.
pub const MAX_STORAGE_BYTES: u64 = 10 * 1024 * 1024 * 1024;

/// Sends accepted per queue per rolling minute before RateLimited.
pub const RATE_LIMIT_PER_QUEUE_PER_MINUTE: u32 = 60;

/// Proof-of-work difficulty (leading zero bits) required to create a new
/// queue (amendment A18). Crude by design — raise if abuse is observed.
pub const QUEUE_CREATION_POW_DIFFICULTY: u8 = 16;

/// Max unsolved PoW challenges the relay tracks at once. A stranger can request
/// challenges without ever solving them; without this cap the outstanding set
/// grows until OOM. At the cap the relay FIFO-evicts the oldest unsolved
/// challenge (a solver racing eviction just re-requests — cheap for them, and
/// the cap is generous relative to legitimate concurrent pairings).
pub const MAX_OUTSTANDING_POW_CHALLENGES: usize = 10_000;

/// Undelivered fan-out journal entries older than this are dropped (amendment
/// A3's retention rule — delete-on-ack, TTL on undelivered).
pub const FAN_OUT_JOURNAL_TTL_SECS: u64 = 14 * 24 * 60 * 60;
