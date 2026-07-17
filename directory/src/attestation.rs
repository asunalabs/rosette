//! T27 (directory half): load the Ed25519 signing key and mint attestation
//! token batches for phone-verified clients.
//!
//! OFF by default — no key configured means no tokens, and the relay (which
//! reads the matching *public* key from its own config) likewise doesn't
//! enforce. So an un-configured deploy behaves exactly as it did before T27;
//! the gate only closes once ops sets a key here AND the public key on the
//! relay.

use ed25519_dalek::SigningKey;
use proto::attestation::{signing_key_from_seed, AttestationToken};
use rand::RngCore;

use crate::store::now_unix;

/// How long an issued token stays valid — long enough that a client rarely
/// refetches, short enough to bound the relay's spent-set memory.
pub const TOKEN_TTL_SECS: i64 = 30 * 24 * 60 * 60; // 30 days
/// Tokens minted per issuance, so the client isn't refetching per queue.
pub const BATCH_SIZE: usize = 20;

/// Load the signing key from `DIRECTORY_ATTESTATION_SIGNING_KEY` (64 hex chars
/// = a 32-byte seed). Mirrors the pepper's posture (`main.rs`): a fixed dev key
/// only behind an explicit `DIRECTORY_ALLOW_DEV_ATTESTATION` opt-in, and `None`
/// when neither is set — the feature stays inert rather than failing startup.
pub fn signing_key_from_env() -> anyhow::Result<Option<SigningKey>> {
    match std::env::var("DIRECTORY_ATTESTATION_SIGNING_KEY") {
        Ok(hex) => {
            let seed = decode_hex_32(&hex).ok_or_else(|| {
                anyhow::anyhow!(
                    "DIRECTORY_ATTESTATION_SIGNING_KEY must be 64 hex chars (a 32-byte seed)"
                )
            })?;
            Ok(Some(signing_key_from_seed(&seed)))
        }
        Err(_) if std::env::var("DIRECTORY_ALLOW_DEV_ATTESTATION").is_ok() => {
            tracing::warn!(
                "DIRECTORY_ATTESTATION_SIGNING_KEY unset — using a fixed dev-only \
                 attestation key. Never do this in production."
            );
            Ok(Some(signing_key_from_seed(&[42u8; 32])))
        }
        Err(_) => Ok(None),
    }
}

fn decode_hex_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Mint a fresh batch of single-use tokens for a just-verified client. Each
/// carries a random nonce (so the relay's spent-set can reject replays) and a
/// shared expiry.
pub fn mint_batch(signing_key: &SigningKey) -> Vec<AttestationToken> {
    let expires_at = now_unix() + TOKEN_TTL_SECS;
    let mut rng = rand::thread_rng();
    (0..BATCH_SIZE)
        .map(|_| {
            let mut nonce = [0u8; 16];
            rng.fill_bytes(&mut nonce);
            AttestationToken::sign(signing_key, nonce, expires_at)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_batch_verifies_against_the_public_key() {
        let sk = signing_key_from_seed(&[3u8; 32]);
        let vk = sk.verifying_key();
        let batch = mint_batch(&sk);
        assert_eq!(batch.len(), BATCH_SIZE);
        for t in &batch {
            assert!(t.signature_valid(&vk), "every minted token verifies");
            assert!(!t.is_expired(now_unix()), "freshly minted, not expired");
        }
        // Nonces are unique, so the relay's spent-set never collides two.
        let mut nonces: Vec<_> = batch.iter().map(|t| t.nonce).collect();
        nonces.sort();
        nonces.dedup();
        assert_eq!(nonces.len(), BATCH_SIZE, "nonces are unique");
    }

    #[test]
    fn a_bad_hex_key_is_rejected() {
        assert!(decode_hex_32("nothex").is_none());
        assert!(decode_hex_32(&"ab".repeat(31)).is_none()); // 62 chars
        assert!(decode_hex_32(&"ab".repeat(32)).is_some()); // 64 chars
    }
}
