//! T13: signup -> OTP -> username claim -> findable-by-search, end to end
//! over real HTTP against a real (ephemeral, per-test) Postgres DB. Also
//! covers ET6/ARCH-5: a vendor timeout must mint no session at all (it used
//! to soft-gate into a "degraded" one, which was an auth bypass).

use std::sync::Arc;

use directory::verify::{DevOtpVendor, VendorError, DEV_OTP_CODE};
use directory::{AppState, DirectoryConfig, DirectoryStore, OtpVendor, RateLimiter};
use sqlx::PgPool;

fn state_with_vendor(pool: PgPool, vendor: Arc<dyn OtpVendor>) -> Arc<AppState> {
    Arc::new(AppState {
        store: Arc::new(DirectoryStore::from_pool(pool)),
        vendor,
        pepper: b"e2e-test-pepper".to_vec(),
        config: DirectoryConfig {
            accounts_enabled: true,
            search_enabled: true,
        },
        rate_limiter: RateLimiter::new(),
    })
}

#[sqlx::test]
async fn fresh_account_reaches_search_findable_state_end_to_end(pool: PgPool) {
    let state = state_with_vendor(pool, Arc::new(DevOtpVendor));
    let store_ref = state.store.clone(); // kept for a direct DB check below
    let addr = directory::spawn_for_tests(state).await.unwrap();
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    // 1. signup
    let phone = "+15559990001";
    let signup = client
        .post(format!("{base}/signup"))
        .json(&serde_json::json!({ "phone": phone }))
        .send()
        .await
        .unwrap();
    assert_eq!(signup.status(), reqwest::StatusCode::OK);

    // 2. OTP verify (dev vendor's fixed code)
    let verify = client
        .post(format!("{base}/verify"))
        .json(&serde_json::json!({ "phone": phone, "code": DEV_OTP_CODE }))
        .send()
        .await
        .unwrap();
    assert_eq!(verify.status(), reqwest::StatusCode::OK);
    let verify_body: serde_json::Value = verify.json().await.unwrap();
    assert_eq!(verify_body["verified"], true);
    let token = verify_body["session_token"].as_str().unwrap().to_string();

    // The response body's `verified` is a server-side constant (api.rs hardcodes
    // it), so asserting on it proves nothing about the DB. The column is what
    // `is_verified` reads to pick the search rate-limit tier — assert on that, or
    // `mark_verified` could write `false` and the whole suite would still pass.
    let hash = directory::verify::phone_hash(phone, directory::verify::Pepper(b"e2e-test-pepper"))
        .unwrap();
    let user_id = store_ref
        .find_user_by_phone_hash(&hash)
        .await
        .unwrap()
        .expect("verify created the user");
    assert_eq!(
        store_ref.is_verified(user_id).await.unwrap(),
        Some(true),
        "an approved code must flip the DB flag, not just the response body"
    );

    // 3. username claim
    let username = client
        .post(format!("{base}/username"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "nickname": "e2ealice" }))
        .send()
        .await
        .unwrap();
    assert_eq!(username.status(), reqwest::StatusCode::OK);
    let handle_body: serde_json::Value = username.json().await.unwrap();
    assert_eq!(handle_body["handle"], "e2ealice#01");

    // A real client computes this locally (unkeyed SHA-256 of the
    // normalized number) — a fixed stand-in is fine for the test, it just
    // needs to be a stable 64-hex-char value.
    let search_hash = format!("e2ea1ce{}", "0".repeat(57));
    let prefix = directory::hash_prefix(&search_hash);

    // Not found yet — hasn't opted in to search (T24: off by default).
    let search_before = client
        .get(format!("{base}/search?prefix={prefix}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    let before_body: serde_json::Value = search_before.json().await.unwrap();
    assert_eq!(before_body["results"].as_array().unwrap().len(), 0);

    // 4. opt in to search (T24 toggle)
    let opt_in = client
        .post(format!("{base}/searchable"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "searchable": true, "phone_search_hash": search_hash }))
        .send()
        .await
        .unwrap();
    assert_eq!(opt_in.status(), reqwest::StatusCode::NO_CONTENT);

    // 5. now findable by search.
    let search_after = client
        .get(format!("{base}/search?prefix={prefix}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    let after_body: serde_json::Value = search_after.json().await.unwrap();
    let results = after_body["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["handle"], "e2ealice#01");
}

/// Up for `send_code`, down for `verify` — deliberately, so the test below
/// isolates `/verify`: the victim needs a real pending row from `/signup`
/// before the attacker's `/verify` can be judged. Not a claim that outages
/// look like this. `AlwaysUnavailableVendor` covers the realistic shape,
/// where the vendor is down for both calls.
struct VerifyUnavailableVendor;
impl OtpVendor for VerifyUnavailableVendor {
    fn send_code(&self, _e164: &str) -> Result<(), VendorError> {
        Ok(())
    }
    fn verify(&self, _e164: &str, _code: &str) -> Result<bool, VendorError> {
        Err(VendorError::Unavailable)
    }
}

/// A real outage: the vendor answers nothing, so `/signup` fails too.
struct AlwaysUnavailableVendor;
impl OtpVendor for AlwaysUnavailableVendor {
    fn send_code(&self, _e164: &str) -> Result<(), VendorError> {
        Err(VendorError::Unavailable)
    }
    fn verify(&self, _e164: &str, _code: &str) -> Result<bool, VendorError> {
        Err(VendorError::Unavailable)
    }
}

/// The held screen ET8 built lives one screen *past* `/signup`, so `/signup`
/// must classify a vendor outage the same way `/verify` does. It used to
/// flatten every vendor error to 500, which the client reads as a generic
/// error — so during the outage the held screen was for, the user never
/// reached it.
#[sqlx::test]
async fn vendor_outage_makes_signup_fail_closed_with_503(pool: PgPool) {
    let state = state_with_vendor(pool, Arc::new(AlwaysUnavailableVendor));
    let addr = directory::spawn_for_tests(state).await.unwrap();
    let base = format!("http://{addr}");

    let res = reqwest::Client::new()
        .post(format!("{base}/signup"))
        .json(&serde_json::json!({ "phone": "+15559990003" }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        res.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "a vendor outage is transient and retryable — 503, not a 500 that reads as our bug"
    );
}

/// ARCH-5, the attacker's view: during a vendor outage, an unauthenticated
/// POST /verify carrying a *victim's* number and an arbitrary code used to
/// return a session token bound to that victim (account erasure and an MLS
/// pairing MITM follow). The vendor times out for the whole test, so the code
/// below is never checked by anyone — and must therefore buy nothing.
#[sqlx::test]
async fn vendor_timeout_mints_no_session_and_leaves_the_victim_unverified(pool: PgPool) {
    let state = state_with_vendor(pool, Arc::new(VerifyUnavailableVendor));
    let store_ref = state.store.clone(); // kept for a direct DB check below
    let addr = directory::spawn_for_tests(state).await.unwrap();
    let base = format!("http://{addr}");
    let client = reqwest::Client::new();

    let phone = "+15559990002";
    client
        .post(format!("{base}/signup"))
        .json(&serde_json::json!({ "phone": phone }))
        .send()
        .await
        .unwrap();

    let verify = client
        .post(format!("{base}/verify"))
        .json(&serde_json::json!({ "phone": phone, "code": "not-the-code" }))
        .send()
        .await
        .unwrap();

    assert_eq!(
        verify.status(),
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "an unchecked code must fail closed, not mint a degraded session"
    );
    let body: serde_json::Value = verify.json().await.unwrap();
    assert!(
        body.get("session_token").is_none() && body.get("user_id").is_none(),
        "503 body must carry no session material, got: {body}"
    );

    // The pending row from /signup still exists — assert on the DB, not on
    // another endpoint's response shape: the flag is what search reads.
    let hash = directory::verify::phone_hash(phone, directory::verify::Pepper(b"e2e-test-pepper"))
        .unwrap();
    let user_id = store_ref
        .find_user_by_phone_hash(&hash)
        .await
        .unwrap()
        .expect("signup created a pending user");
    assert_eq!(
        store_ref.is_verified(user_id).await.unwrap(),
        Some(false),
        "a timed-out verify must not flip the victim's verified flag"
    );
}
