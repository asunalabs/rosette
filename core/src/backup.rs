//! Account-recovery crypto (issue #2). A random 32-byte backup key (BK)
//! seals a small identity blob; BK is wrapped once under the user's PIN and
//! once under a 5-word recovery phrase, and two salted auth hashes gate
//! server-side retrieval. Every parameter here is fixed by the spec —
//! Argon2id m=64MiB t=3 p=4, XChaCha20-Poly1305, SHA-256 — the code makes
//! zero crypto choices.
//!
//! Accepted ceiling (spec'd): an attacker holding the directory DB can
//! brute-force the PIN wrap offline (10^6 space). The phrase wrap
//! (~64.6 bits) and the blob itself stay strong even then.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};

pub const BK_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const WORDLIST: &str = include_str!("eff_large_wordlist.txt");
pub const PHRASE_WORDS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("PIN must be 4-6 digits")]
    InvalidPin,
    /// AEAD failure: wrong secret or tampered ciphertext. Deliberately one
    /// variant — the tag check cannot and must not say which.
    #[error("wrong secret or corrupt ciphertext")]
    WrongSecret,
    #[error("encode: {0}")]
    Encode(String),
}

/// Everything the directory stores for one account, and everything Change
/// PIN (2c) needs to re-wrap locally. All fields are ciphertext, salts, or
/// hashes — nothing here is secret on its own.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct BackupBundle {
    pub blob: Vec<u8>,
    pub w_pin: Vec<u8>,
    pub salt_p: Vec<u8>,
    pub w_phrase: Vec<u8>,
    pub salt_f: Vec<u8>,
    pub auth_pin: Vec<u8>,
    pub salt_a: Vec<u8>,
    pub auth_phrase: Vec<u8>,
    pub salt_pa: Vec<u8>,
}

/// What the blob decrypts to. Deliberately EXCLUDES MLS group state and
/// message history — ratchet state must never time-travel through a backup.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BackupPayload {
    /// Identity-only session snapshot (`session::strip_snapshot_to_identity`).
    /// None when enrollment runs before the first connect creates one; the
    /// debounced re-upload refreshes it later.
    pub identity: Option<Vec<u8>>,
    pub username: Option<String>,
    // ponytail: v0.1 contact model is peer display names — restore re-pairs,
    // and nothing richer (user ids) exists yet. Widen when contacts do.
    pub contacts: Vec<String>,
}

pub fn validate_pin(pin: &str) -> bool {
    (4..=6).contains(&pin.len()) && pin.bytes().all(|b| b.is_ascii_digit())
}

/// Restore-side phrase normalization: what the user typed → what
/// `generate_phrase` produced. Lowercase, trimmed, single spaces.
pub fn normalize_phrase(input: &str) -> String {
    input
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 5 words drawn uniformly from the embedded EFF large wordlist
/// (7776 words, ~64.6 bits total), lowercase, space-joined.
pub fn generate_phrase() -> String {
    let words: Vec<&str> = WORDLIST.lines().collect();
    let mut rng = rand::rngs::OsRng;
    (0..PHRASE_WORDS)
        .map(|_| words[rng.gen_range(0..words.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut out);
    out
}

/// Argon2id, spec params: m=64MiB t=3 p=4, 32-byte output.
fn derive(secret: &[u8], salt: &[u8]) -> [u8; BK_LEN] {
    let params = Params::new(64 * 1024, 3, 4, Some(BK_LEN)).expect("fixed Argon2 params are valid");
    let mut out = [0u8; BK_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(secret, salt, &mut out)
        .expect("fixed-size Argon2 derivation never fails");
    out
}

/// nonce(24) || XChaCha20-Poly1305 ciphertext.
fn aead_seal(key: &[u8; BK_LEN], plaintext: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let mut out = nonce.to_vec();
    out.extend(
        cipher
            .encrypt(&nonce, plaintext)
            .expect("in-memory AEAD encryption never fails"),
    );
    out
}

fn aead_open(key: &[u8; BK_LEN], data: &[u8]) -> Result<Vec<u8>, BackupError> {
    if data.len() < NONCE_LEN {
        return Err(BackupError::WrongSecret);
    }
    let (nonce, ct) = data.split_at(NONCE_LEN);
    XChaCha20Poly1305::new(Key::from_slice(key))
        .decrypt(XNonce::from_slice(nonce), ct)
        .map_err(|_| BackupError::WrongSecret)
}

/// Server-side retrieval gate: SHA256(Argon2id(secret, salt)). The server
/// stores this and rate-limits comparisons; it can verify but never derive
/// the wrap key (different salt).
pub fn auth_hash(secret: &str, salt: &[u8]) -> Vec<u8> {
    Sha256::digest(derive(secret.as_bytes(), salt)).to_vec()
}

pub fn seal_blob(bk: &[u8; BK_LEN], payload: &[u8]) -> Vec<u8> {
    aead_seal(bk, payload)
}

pub fn open_blob(bk: &[u8; BK_LEN], blob: &[u8]) -> Result<Vec<u8>, BackupError> {
    aead_open(bk, blob)
}

/// Recover BK from one of the two wraps. `secret` is the PIN for
/// (w_pin, salt_p) or the phrase for (w_phrase, salt_f).
pub fn unwrap_bk(secret: &str, salt: &[u8], wrapped: &[u8]) -> Result<[u8; BK_LEN], BackupError> {
    aead_open(&derive(secret.as_bytes(), salt), wrapped)?
        .try_into()
        .map_err(|_| BackupError::WrongSecret)
}

/// Wrap BK under a secret — `unwrap_bk`'s inverse. Used at enroll and by
/// Change PIN (2c)/phrase-path restore (#3) to re-wrap with a new PIN.
pub fn wrap_bk(secret: &str, salt: &[u8], bk: &[u8; BK_LEN]) -> Vec<u8> {
    aead_seal(&derive(secret.as_bytes(), salt), bk)
}

/// Build the full upload bundle from fresh salts. Four Argon2 derivations —
/// takes a couple of seconds by design; callers run it off the UI thread.
pub fn build_bundle(
    pin: &str,
    phrase: &str,
    bk: &[u8; BK_LEN],
    payload: &[u8],
) -> Result<BackupBundle, BackupError> {
    if !validate_pin(pin) {
        return Err(BackupError::InvalidPin);
    }
    let salt_p = random_bytes::<SALT_LEN>();
    let salt_f = random_bytes::<SALT_LEN>();
    let salt_a = random_bytes::<SALT_LEN>();
    let salt_pa = random_bytes::<SALT_LEN>();
    Ok(BackupBundle {
        blob: seal_blob(bk, payload),
        w_pin: wrap_bk(pin, &salt_p, bk),
        salt_p: salt_p.to_vec(),
        w_phrase: wrap_bk(phrase, &salt_f, bk),
        salt_f: salt_f.to_vec(),
        auth_pin: auth_hash(pin, &salt_a),
        salt_a: salt_a.to_vec(),
        auth_phrase: auth_hash(phrase, &salt_pa),
        salt_pa: salt_pa.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle() -> ([u8; BK_LEN], BackupBundle) {
        let bk = random_bytes::<BK_LEN>();
        let b = build_bundle(
            "1234",
            "correct horse battery staple extra",
            &bk,
            b"payload",
        )
        .unwrap();
        (bk, b)
    }

    #[test]
    fn pin_validation() {
        assert!(validate_pin("1234"));
        assert!(validate_pin("123456"));
        assert!(!validate_pin("123"));
        assert!(!validate_pin("1234567"));
        assert!(!validate_pin("12a4"));
        assert!(!validate_pin(""));
        assert!(!validate_pin("12 4"));
    }

    #[test]
    fn phrase_is_five_words_from_the_wordlist() {
        let words: std::collections::HashSet<&str> = WORDLIST.lines().collect();
        assert_eq!(words.len(), 7776, "EFF large wordlist must be complete");
        let phrase = generate_phrase();
        let drawn: Vec<&str> = phrase.split(' ').collect();
        assert_eq!(drawn.len(), PHRASE_WORDS);
        for w in drawn {
            assert!(words.contains(w), "{w} is not on the wordlist");
        }
        assert_ne!(
            generate_phrase(),
            generate_phrase(),
            "phrases must not repeat"
        );
    }

    #[test]
    fn pin_wrap_roundtrips_and_wrong_pin_fails_loudly() {
        let (bk, b) = bundle();
        assert_eq!(unwrap_bk("1234", &b.salt_p, &b.w_pin).unwrap(), bk);
        assert!(matches!(
            unwrap_bk("1235", &b.salt_p, &b.w_pin),
            Err(BackupError::WrongSecret)
        ));
    }

    #[test]
    fn phrase_wrap_roundtrips_and_wrong_phrase_fails_loudly() {
        let (bk, b) = bundle();
        let phrase = "correct horse battery staple extra";
        assert_eq!(unwrap_bk(phrase, &b.salt_f, &b.w_phrase).unwrap(), bk);
        assert!(matches!(
            unwrap_bk("wrong horse battery staple extra", &b.salt_f, &b.w_phrase),
            Err(BackupError::WrongSecret)
        ));
    }

    #[test]
    fn blob_roundtrips_and_ciphertext_leaks_nothing() {
        let (bk, b) = bundle();
        assert_eq!(open_blob(&bk, &b.blob).unwrap(), b"payload");
        assert!(
            !b.blob.windows(7).any(|w| w == b"payload"),
            "blob must be ciphertext, not plaintext"
        );
        let other = random_bytes::<BK_LEN>();
        assert!(matches!(
            open_blob(&other, &b.blob),
            Err(BackupError::WrongSecret)
        ));
    }

    #[test]
    fn auth_hashes_are_deterministic_and_salt_separated() {
        let salt1 = random_bytes::<SALT_LEN>();
        let salt2 = random_bytes::<SALT_LEN>();
        assert_eq!(auth_hash("1234", &salt1), auth_hash("1234", &salt1));
        assert_ne!(auth_hash("1234", &salt1), auth_hash("1234", &salt2));
        assert_ne!(auth_hash("1234", &salt1), auth_hash("1235", &salt1));
    }

    #[test]
    fn phrase_normalization_matches_generation() {
        assert_eq!(
            normalize_phrase("  Correct  HORSE\tbattery staple  extra "),
            "correct horse battery staple extra"
        );
        let generated = generate_phrase();
        assert_eq!(normalize_phrase(&generated), generated);
    }

    #[test]
    fn wrap_bk_roundtrips_with_unwrap_bk() {
        let bk = random_bytes::<BK_LEN>();
        let salt = random_bytes::<SALT_LEN>();
        let wrapped = wrap_bk("9999", &salt, &bk);
        assert_eq!(unwrap_bk("9999", &salt, &wrapped).unwrap(), bk);
        assert!(unwrap_bk("9998", &salt, &wrapped).is_err());
    }

    #[test]
    fn build_bundle_rejects_a_bad_pin() {
        let bk = random_bytes::<BK_LEN>();
        assert!(matches!(
            build_bundle("12", "a b c d e", &bk, b"x"),
            Err(BackupError::InvalidPin)
        ));
    }
}
