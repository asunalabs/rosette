//! T27 transport half: directory-issued attestation tokens.
//!
//! After phone verification the directory hands the client a batch of these;
//! the client spends one per queue-creation request (`CreateMailbox` /
//! `CreateGroupInbox`). The relay verifies them **offline** against a cached
//! directory public key baked into its config at deploy time — no live call to
//! the directory, so T21's crash-isolation guarantee is untouched.
//!
//! A token embeds **no `user_id`**, only a random nonce and an expiry, so the
//! relay never learns who is creating a queue — it learns only "some
//! phone-verified account did." The nonce lets the relay reject replays; the
//! expiry bounds how long the spent-set must remember a nonce (entries prune
//! once expired), which is why eviction is by expiry and never by FIFO count —
//! a count-evicted-but-unexpired nonce would be replayable.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

/// A single-use, phone-verification attestation. The signature covers the
/// nonce and the expiry together, so neither can be altered independently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttestationToken {
    pub nonce: [u8; 16],
    /// Unix seconds after which the token is invalid (and its spent-set entry
    /// may be pruned).
    pub expires_at: i64,
    /// The 64-byte Ed25519 signature. `Vec` only because serde's derive tops
    /// out at `[u8; 32]`; `signature_valid` enforces the length via
    /// `Signature::from_slice`, so a wrong-length blob is rejected, not trusted.
    pub signature: Vec<u8>,
}

/// The exact bytes the signature covers: `nonce || expires_at` (big-endian).
/// A stable, length-fixed encoding so signer and verifier never disagree.
fn signed_payload(nonce: &[u8; 16], expires_at: i64) -> [u8; 24] {
    let mut buf = [0u8; 24];
    buf[..16].copy_from_slice(nonce);
    buf[16..].copy_from_slice(&expires_at.to_be_bytes());
    buf
}

impl AttestationToken {
    /// Directory-side: mint a signed token.
    pub fn sign(signing_key: &SigningKey, nonce: [u8; 16], expires_at: i64) -> Self {
        let sig = signing_key.sign(&signed_payload(&nonce, expires_at));
        AttestationToken {
            nonce,
            expires_at,
            signature: sig.to_bytes().to_vec(),
        }
    }

    /// Relay-side: is the Ed25519 signature valid for this directory key?
    ///
    /// Expiry and replay are the caller's job (it owns the clock and the
    /// spent-set) — kept separate so this stays a pure, allocation-free crypto
    /// check with no policy baked in.
    pub fn signature_valid(&self, verifying_key: &VerifyingKey) -> bool {
        let Ok(sig) = Signature::from_slice(&self.signature) else {
            return false;
        };
        verifying_key
            .verify(&signed_payload(&self.nonce, self.expires_at), &sig)
            .is_ok()
    }

    /// True once `now_unix` has reached the expiry. Expired tokens are rejected
    /// AND become prunable from the spent-set.
    pub fn is_expired(&self, now_unix: i64) -> bool {
        now_unix >= self.expires_at
    }
}

/// Parse a 32-byte Ed25519 verifying (public) key — what the relay loads from
/// config. Returns `None` on a malformed key rather than panicking, so a bad
/// deploy env is a startup error the caller reports, not a crash.
pub fn verifying_key_from_bytes(bytes: &[u8; 32]) -> Option<VerifyingKey> {
    VerifyingKey::from_bytes(bytes).ok()
}

/// Build a signing key from a 32-byte seed — what the directory loads from
/// config. The corresponding public key is `signing_key.verifying_key()`.
pub fn signing_key_from_seed(seed: &[u8; 32]) -> SigningKey {
    SigningKey::from_bytes(seed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn keypair() -> (SigningKey, VerifyingKey) {
        // Deterministic seed — tests need no RNG and stay reproducible.
        let sk = signing_key_from_seed(&[7u8; 32]);
        let vk = sk.verifying_key();
        (sk, vk)
    }

    #[test]
    fn a_freshly_signed_token_verifies() {
        let (sk, vk) = keypair();
        let t = AttestationToken::sign(&sk, [1u8; 16], 10_000);
        assert!(t.signature_valid(&vk));
        assert!(!t.is_expired(9_999));
        assert!(t.is_expired(10_000));
    }

    #[test]
    fn a_wrong_key_rejects() {
        let (sk, _vk) = keypair();
        let other = signing_key_from_seed(&[9u8; 32]).verifying_key();
        let t = AttestationToken::sign(&sk, [1u8; 16], 10_000);
        assert!(!t.signature_valid(&other));
    }

    #[test]
    fn tampering_with_expiry_or_nonce_breaks_the_signature() {
        let (sk, vk) = keypair();
        let mut t = AttestationToken::sign(&sk, [1u8; 16], 10_000);
        let good = t.clone();
        t.expires_at = 99_999; // extend the token
        assert!(!t.signature_valid(&vk), "extended expiry must not verify");
        let mut t2 = good.clone();
        t2.nonce = [2u8; 16];
        assert!(!t2.signature_valid(&vk), "swapped nonce must not verify");
        assert!(
            good.signature_valid(&vk),
            "the untampered token still verifies"
        );
    }

    #[test]
    fn a_garbage_signature_is_rejected_not_a_panic() {
        let (_sk, vk) = keypair();
        let t = AttestationToken {
            nonce: [0u8; 16],
            expires_at: 10_000,
            signature: vec![0u8; 64],
        };
        assert!(!t.signature_valid(&vk));
    }
}
