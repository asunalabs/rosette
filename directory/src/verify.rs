//! Phone verification (T2). `phone_hash` is Argon2id keyed with a
//! server-side pepper over E.164-normalized input (OQ4) — the pepper lives
//! outside the DB (secrets manager/KMS), passed in by the caller. The salt
//! is fixed/public; only the pepper needs to stay secret, since Argon2id's
//! cost is what makes offline phone-number-list guessing expensive.

use argon2::{Algorithm, Argon2, Params, Version};

/// Domain-separation salt. Public — deduplication requires a deterministic
/// hash per phone number, so the salt can't be random per call.
const HASH_SALT: &[u8] = b"chat-directory-phone-hash-v1";

#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("phone number is not valid E.164")]
    InvalidPhoneFormat,
    #[error("otp code rejected")]
    CodeRejected,
    #[error("argon2 error: {0}")]
    Hash(String),
    #[error("otp vendor error: {0}")]
    Vendor(String),
}

/// Server-side pepper, sourced from a secrets manager/KMS — never stored in
/// the DB alongside `phone_hash`.
pub struct Pepper<'a>(pub &'a [u8]);

/// Minimal E.164 check: `+` followed by 8-15 digits. Not full libphonenumber
/// validation — good enough to reject garbage before it reaches the vendor.
pub fn normalize_e164(raw: &str) -> Result<String, VerifyError> {
    let digits = raw
        .strip_prefix('+')
        .ok_or(VerifyError::InvalidPhoneFormat)?;
    if !(8..=15).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(VerifyError::InvalidPhoneFormat);
    }
    Ok(format!("+{digits}"))
}

pub fn phone_hash(e164: &str, pepper: Pepper) -> Result<String, VerifyError> {
    let argon2 = Argon2::new_with_secret(
        pepper.0,
        Algorithm::Argon2id,
        Version::V0x13,
        Params::default(),
    )
    .map_err(|e| VerifyError::Hash(e.to_string()))?;
    let mut out = [0u8; 32];
    argon2
        .hash_password_into(e164.as_bytes(), HASH_SALT, &mut out)
        .map_err(|e| VerifyError::Hash(e.to_string()))?;
    Ok(out.iter().fold(String::with_capacity(64), |mut s, b| {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
        s
    }))
}

#[derive(Debug)]
pub enum VendorError {
    /// Vendor call didn't complete in time — soft-gate, don't hard-fail signup.
    Timeout,
    Other(String),
}

pub trait OtpVendor: Send + Sync {
    /// Triggers delivery of a one-time code to `e164` (SMS/call, vendor's
    /// concern). Doesn't return the code — only the vendor and the user's
    /// phone ever see it.
    fn send_code(&self, e164: &str) -> Result<(), VendorError>;
    fn verify(&self, e164: &str, code: &str) -> Result<bool, VendorError>;
}

#[derive(Debug, PartialEq, Eq)]
pub enum VerificationOutcome {
    Verified,
    /// Vendor outage: account created, but unverified and excluded from
    /// search until it can be re-verified.
    Degraded,
}

/// No real SMS vendor is wired up yet — nothing in this project has a
/// Twilio/etc. account. This stub sends nothing and accepts a fixed dev
/// code, so the rest of the stack (signup -> verify -> session -> search)
/// is actually exercisable end to end. Swap for a real vendor before any
/// real phone number touches this.
pub struct DevOtpVendor;

pub const DEV_OTP_CODE: &str = "000000";

impl OtpVendor for DevOtpVendor {
    fn send_code(&self, e164: &str) -> Result<(), VendorError> {
        tracing::info!(%e164, code = DEV_OTP_CODE, "dev vendor: pretending to send OTP");
        Ok(())
    }

    fn verify(&self, _e164: &str, code: &str) -> Result<bool, VendorError> {
        Ok(code == DEV_OTP_CODE)
    }
}

pub fn verify_phone(
    vendor: &dyn OtpVendor,
    raw_phone: &str,
    code: &str,
    pepper: Pepper,
) -> Result<(String, VerificationOutcome), VerifyError> {
    let e164 = normalize_e164(raw_phone)?;
    let hash = phone_hash(&e164, pepper)?;
    let outcome = match vendor.verify(&e164, code) {
        Ok(true) => VerificationOutcome::Verified,
        Ok(false) => return Err(VerifyError::CodeRejected),
        Err(VendorError::Timeout) => VerificationOutcome::Degraded,
        Err(VendorError::Other(msg)) => return Err(VerifyError::Vendor(msg)),
    };
    Ok((hash, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TimeoutVendor;
    impl OtpVendor for TimeoutVendor {
        fn send_code(&self, _e164: &str) -> Result<(), VendorError> {
            Ok(())
        }
        fn verify(&self, _e164: &str, _code: &str) -> Result<bool, VendorError> {
            Err(VendorError::Timeout)
        }
    }

    struct OkVendor;
    impl OtpVendor for OkVendor {
        fn send_code(&self, _e164: &str) -> Result<(), VendorError> {
            Ok(())
        }
        fn verify(&self, _e164: &str, _code: &str) -> Result<bool, VendorError> {
            Ok(true)
        }
    }

    #[test]
    fn vendor_timeout_degrades_instead_of_failing_signup() {
        let (hash, outcome) = verify_phone(
            &TimeoutVendor,
            "+15551234567",
            "000000",
            Pepper(b"test-pepper"),
        )
        .expect("degraded signup should still succeed");
        assert_eq!(outcome, VerificationOutcome::Degraded);
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn happy_path_verifies() {
        let (_hash, outcome) =
            verify_phone(&OkVendor, "+15551234567", "000000", Pepper(b"test-pepper")).unwrap();
        assert_eq!(outcome, VerificationOutcome::Verified);
    }

    #[test]
    fn same_phone_same_pepper_hashes_deterministically() {
        let a = phone_hash("+15551234567", Pepper(b"pepper")).unwrap();
        let b = phone_hash("+15551234567", Pepper(b"pepper")).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_non_e164() {
        assert!(normalize_e164("5551234567").is_err());
        assert!(normalize_e164("+abc").is_err());
    }
}
