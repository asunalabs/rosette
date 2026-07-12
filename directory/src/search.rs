//! Anti-enumeration search (T3). k-anonymity hash-prefix bucketing,
//! HIBP-style — see `docs/plans/spike-t17-anti-enumeration.md` for why this
//! was chosen over bloom filters (rejected: unrateable offline enumeration)
//! and real PSI/OPRF (rejected for v1: disproportionate engineering).
//!
//! The server API only ever accepts a *prefix*, never a full hash — so
//! there is no code path in which the server sees a specific target to
//! branch on. That's what makes "found vs. not found" timing-indistinguishable
//! by construction rather than by careful constant-time coding: the
//! information needed to leak (which exact hash the caller wants) never
//! reaches this function. Bucket *cardinality* is the accepted leakage
//! channel (same as HIBP: "did anyone" is revealed, "did you specifically"
//! is not) — see the spike doc's cost/complexity table.

// ponytail: prefix length is HIBP's default (20 bits / 5 hex chars), not a
// number chosen for this project's actual scale. Real tuning needs launch
// user counts — see the spike doc's "open tuning question" — revisit at T10.
pub const PREFIX_LEN_HEX: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneEntry {
    pub phone_hash: String,
    pub user_id: u64,
}

/// Pluggable so the real Postgres-backed index (`DirectoryStore`) and a
/// plain in-memory one (tests) share the anti-enumeration logic. Async
/// because the real implementation is a DB query.
#[async_trait::async_trait]
pub trait PrefixIndex: Send + Sync {
    async fn bucket(&self, prefix: &str) -> Vec<PhoneEntry>;
}

pub fn hash_prefix(full_hash: &str) -> &str {
    let end = PREFIX_LEN_HEX.min(full_hash.len());
    &full_hash[..end]
}

/// Returns the full bucket for `prefix` — always the same response shape
/// for a given bucket, whether or not the caller's real target is in it.
/// Takes only a prefix; there is no overload that accepts a full hash.
pub async fn search_by_prefix(index: &dyn PrefixIndex, prefix: &str) -> Vec<PhoneEntry> {
    index.bucket(prefix).await
}

#[derive(Default)]
pub struct InMemoryIndex {
    by_prefix: std::collections::HashMap<String, Vec<PhoneEntry>>,
}

impl InMemoryIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, phone_hash: String, user_id: u64) {
        let prefix = hash_prefix(&phone_hash).to_string();
        self.by_prefix.entry(prefix).or_default().push(PhoneEntry {
            phone_hash,
            user_id,
        });
    }
}

#[async_trait::async_trait]
impl PrefixIndex for InMemoryIndex {
    async fn bucket(&self, prefix: &str) -> Vec<PhoneEntry> {
        self.by_prefix.get(prefix).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::Instant;

    /// Deterministic 64-hex-char stand-in for a real Argon2id phone_hash —
    /// stdlib only, no rand dependency for a test fixture.
    fn synthetic_hash(seed: u64) -> String {
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        let a = h.finish();
        (seed ^ 0x9E37_79B9_7F4A_7C15).hash(&mut h);
        let b = h.finish();
        format!(
            "{a:016x}{b:016x}{:016x}{:016x}",
            a ^ b,
            a.wrapping_mul(b | 1)
        )
    }

    #[tokio::test]
    async fn empty_bucket_returns_empty_not_panic() {
        let index = InMemoryIndex::new();
        assert!(search_by_prefix(&index, "abcde").await.is_empty());
    }

    #[tokio::test]
    async fn same_prefix_groups_together_regardless_of_exact_hash() {
        let mut index = InMemoryIndex::new();
        let mut same_prefix_hashes = Vec::new();
        // Force a shared prefix by construction rather than hoping for a
        // hash collision.
        for i in 0u64..12 {
            let full = format!("00000{}", synthetic_hash(i));
            same_prefix_hashes.push(full.clone());
            index.insert(full, i);
        }
        let bucket = search_by_prefix(&index, "00000").await;
        assert_eq!(bucket.len(), 12);
        let returned: std::collections::HashSet<_> =
            bucket.iter().map(|e| e.phone_hash.clone()).collect();
        for h in &same_prefix_hashes {
            assert!(returned.contains(h));
        }
    }

    #[tokio::test]
    async fn search_signature_only_accepts_a_prefix_not_a_full_hash() {
        // Structural guarantee, not a runtime one: `search_by_prefix` takes
        // a `&str` used only for bucket lookup — passing a full 64-char
        // hash just becomes (at most) the whole string treated as the
        // bucket key, which will simply miss every real bucket (since real
        // buckets are keyed by a 5-char prefix). There's no "exact match"
        // code path to accidentally hit.
        let mut index = InMemoryIndex::new();
        let full = synthetic_hash(1);
        index.insert(full.clone(), 1);
        assert!(search_by_prefix(&index, &full).await.is_empty());
        assert_eq!(search_by_prefix(&index, hash_prefix(&full)).await.len(), 1);
    }

    /// CI timing-variance assertion (T3's verify criterion): lookup time
    /// for equal-sized buckets must not depend on bucket *contents*, only
    /// on bucket *cardinality* (the accepted leakage channel). This is a
    /// regression guard against someone later adding a content-dependent
    /// short-circuit (e.g. an early exit on a "known" hash) — the current
    /// implementation has no such branch, so this should stay comfortably
    /// under the tolerance.
    #[tokio::test]
    async fn equal_sized_buckets_have_indistinguishable_lookup_time() {
        const BUCKET_SIZE: u64 = 40;
        const TRIALS: u32 = 300;

        let mut index = InMemoryIndex::new();
        for i in 0..BUCKET_SIZE {
            index.insert(format!("aaaaa{}", synthetic_hash(i)), i);
        }
        for i in 0..BUCKET_SIZE {
            index.insert(format!("bbbbb{}", synthetic_hash(i + 1_000_000)), i);
        }

        async fn time_bucket(
            index: &InMemoryIndex,
            prefix: &str,
            bucket_size: u64,
            trials: u32,
        ) -> u128 {
            // Warm up, then measure — avoids first-call allocator noise
            // dominating a handful of trials.
            for _ in 0..20 {
                let _ = search_by_prefix(index, prefix).await;
            }
            let start = Instant::now();
            for _ in 0..trials {
                let bucket = search_by_prefix(index, prefix).await;
                assert_eq!(bucket.len() as u64, bucket_size);
            }
            start.elapsed().as_nanos()
        }

        let a_nanos = time_bucket(&index, "aaaaa", BUCKET_SIZE, TRIALS).await;
        let b_nanos = time_bucket(&index, "bbbbb", BUCKET_SIZE, TRIALS).await;

        let (lo, hi) = if a_nanos < b_nanos {
            (a_nanos, b_nanos)
        } else {
            (b_nanos, a_nanos)
        };
        // Generous tolerance (3x) to stay non-flaky under CI noise while
        // still catching a real content-dependent branch, which would
        // show up as an order-of-magnitude-scale gap, not a 2x one.
        assert!(
            hi <= lo.saturating_mul(3) + 1,
            "bucket lookup time diverged by content, not just size: {a_nanos}ns vs {b_nanos}ns"
        );
    }
}
