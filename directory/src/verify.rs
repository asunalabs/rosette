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

/// Twilio Verify (v2) as the initial `OtpVendor`. Picked per the project
/// decision to start with one vendor but keep swapping providers a
/// one-`impl`-block affair — everything code needs from a vendor already
/// lives behind the `OtpVendor` trait, so a second provider (Vonage, AWS
/// SNS, ...) is a new struct implementing the same two methods, no changes
/// anywhere else.
pub struct TwilioOtpVendor {
    account_sid: String,
    auth_token: String,
    verify_service_sid: String,
    client: reqwest::blocking::Client,
}

impl TwilioOtpVendor {
    pub fn new(account_sid: String, auth_token: String, verify_service_sid: String) -> Self {
        Self {
            account_sid,
            auth_token,
            verify_service_sid,
            client: reqwest::blocking::Client::new(),
        }
    }

    fn verifications_url(&self) -> String {
        format!(
            "https://verify.twilio.com/v2/Services/{}/Verifications",
            self.verify_service_sid
        )
    }

    fn verification_check_url(&self) -> String {
        format!(
            "https://verify.twilio.com/v2/Services/{}/VerificationCheck",
            self.verify_service_sid
        )
    }
}

/// `send_code`/`verify` are sync (the trait predates any vendor that needs
/// the network — DevOtpVendor never did). `block_in_place` moves the
/// blocking HTTP call off the async task so it doesn't stall the tokio
/// executor; it requires the multi-threaded runtime, which is what
/// `#[tokio::main]` with the `full` feature (main.rs) actually gives us. A
/// real ceiling, not a hypothetical one: this would panic under a
/// current-thread runtime. Revisit by making `OtpVendor` async if a future
/// vendor needs more than a single request/response round trip.
fn blocking_call<T>(f: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(f)
}

impl OtpVendor for TwilioOtpVendor {
    fn send_code(&self, e164: &str) -> Result<(), VendorError> {
        blocking_call(|| {
            let result = self
                .client
                .post(self.verifications_url())
                .basic_auth(&self.account_sid, Some(&self.auth_token))
                .form(&[("To", e164), ("Channel", "sms")])
                .send();
            match result {
                Ok(resp) if resp.status().is_success() => Ok(()),
                Ok(resp) => Err(VendorError::Other(format!(
                    "twilio start-verification failed: {}",
                    resp.status()
                ))),
                Err(e) if e.is_timeout() => Err(VendorError::Timeout),
                Err(e) => Err(VendorError::Other(e.to_string())),
            }
        })
    }

    fn verify(&self, e164: &str, code: &str) -> Result<bool, VendorError> {
        blocking_call(|| {
            let result = self
                .client
                .post(self.verification_check_url())
                .basic_auth(&self.account_sid, Some(&self.auth_token))
                .form(&[("To", e164), ("Code", code)])
                .send();
            let resp = match result {
                Ok(resp) if resp.status().is_success() => resp,
                Ok(resp) => {
                    return Err(VendorError::Other(format!(
                        "twilio verification-check failed: {}",
                        resp.status()
                    )))
                }
                Err(e) if e.is_timeout() => return Err(VendorError::Timeout),
                Err(e) => return Err(VendorError::Other(e.to_string())),
            };
            let body: serde_json::Value =
                resp.json().map_err(|e| VendorError::Other(e.to_string()))?;
            Ok(verification_approved(&body))
        })
    }
}

/// Split out from `verify` so the Twilio response-shape logic is
/// unit-testable without a network call.
fn verification_approved(body: &serde_json::Value) -> bool {
    body.get("status").and_then(|s| s.as_str()) == Some("approved")
}

/// Picks the OTP vendor from the environment, the same "loud refusal unless
/// explicitly opted into a dev default" shape main.rs already uses for the
/// pepper (OQ4) — a real phone number should never silently get the fixed
/// dev code.
pub fn vendor_from_env() -> anyhow::Result<std::sync::Arc<dyn OtpVendor>> {
    let sid = std::env::var("TWILIO_ACCOUNT_SID");
    let token = std::env::var("TWILIO_AUTH_TOKEN");
    let service = std::env::var("TWILIO_VERIFY_SERVICE_SID");
    if let (Ok(sid), Ok(token), Ok(service)) = (sid, token, service) {
        return Ok(std::sync::Arc::new(TwilioOtpVendor::new(
            sid, token, service,
        )));
    }
    if std::env::var("DIRECTORY_ALLOW_DEV_OTP_VENDOR").is_ok() {
        tracing::warn!(
            "TWILIO_* unset — using DevOtpVendor (fixed code, sends nothing). \
             Never do this in production."
        );
        return Ok(std::sync::Arc::new(DevOtpVendor));
    }
    anyhow::bail!(
        "no OTP vendor configured: set TWILIO_ACCOUNT_SID, TWILIO_AUTH_TOKEN, and \
         TWILIO_VERIFY_SERVICE_SID, or set DIRECTORY_ALLOW_DEV_OTP_VENDOR=1 to run with \
         an insecure dev default instead."
    )
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

    #[test]
    fn twilio_urls_are_scoped_to_the_configured_verify_service() {
        let vendor = TwilioOtpVendor::new(
            "AC_sid".to_string(),
            "token".to_string(),
            "VA_service".to_string(),
        );
        assert_eq!(
            vendor.verifications_url(),
            "https://verify.twilio.com/v2/Services/VA_service/Verifications"
        );
        assert_eq!(
            vendor.verification_check_url(),
            "https://verify.twilio.com/v2/Services/VA_service/VerificationCheck"
        );
    }

    #[test]
    fn verification_approved_reads_the_twilio_status_field() {
        assert!(verification_approved(
            &serde_json::json!({ "status": "approved", "sid": "VE123" })
        ));
        assert!(!verification_approved(
            &serde_json::json!({ "status": "pending" })
        ));
        assert!(!verification_approved(&serde_json::json!({})));
    }

    #[test]
    fn vendor_from_env_prefers_twilio_and_never_silently_falls_back() {
        // One test, not two: TWILIO_*/DIRECTORY_ALLOW_DEV_OTP_VENDOR are
        // process-global, so exercising both branches in the same test
        // avoids a race against a parallel test thread over the same vars.
        std::env::remove_var("TWILIO_ACCOUNT_SID");
        std::env::remove_var("TWILIO_AUTH_TOKEN");
        std::env::remove_var("TWILIO_VERIFY_SERVICE_SID");
        std::env::remove_var("DIRECTORY_ALLOW_DEV_OTP_VENDOR");
        assert!(
            vendor_from_env().is_err(),
            "must not fall back to DevOtpVendor without an explicit opt-in"
        );

        std::env::set_var("TWILIO_ACCOUNT_SID", "AC_test");
        std::env::set_var("TWILIO_AUTH_TOKEN", "token_test");
        std::env::set_var("TWILIO_VERIFY_SERVICE_SID", "VA_test");
        let vendor = vendor_from_env();
        std::env::remove_var("TWILIO_ACCOUNT_SID");
        std::env::remove_var("TWILIO_AUTH_TOKEN");
        std::env::remove_var("TWILIO_VERIFY_SERVICE_SID");
        assert!(
            vendor.is_ok(),
            "fully configured Twilio env must be picked up"
        );
    }

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
