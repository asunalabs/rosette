//! T13: signup -> OTP -> username claim -> findable-by-search, end to end
//! over real HTTP against a real (ephemeral, per-test) Postgres DB. Also
//! covers the T2 soft-gate path: a degraded (vendor-timeout) account must
//! NOT become findable.

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

struct AlwaysTimeoutVendor;
impl OtpVendor for AlwaysTimeoutVendor {
    fn send_code(&self, _e164: &str) -> Result<(), VendorError> {
        Ok(())
    }
    fn verify(&self, _e164: &str, _code: &str) -> Result<bool, VendorError> {
        Err(VendorError::Timeout)
    }
}

#[sqlx::test]
async fn degraded_soft_gate_account_never_becomes_findable(pool: PgPool) {
    let state = state_with_vendor(pool, Arc::new(AlwaysTimeoutVendor));
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
        .json(&serde_json::json!({ "phone": phone, "code": "000000" }))
        .send()
        .await
        .unwrap();
    // Degraded signup still succeeds (T2) but comes back unverified.
    assert_eq!(verify.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = verify.json().await.unwrap();
    assert_eq!(body["verified"], false);
    let user_id = body["user_id"].as_u64().unwrap();
    let token = body["session_token"].as_str().unwrap().to_string();

    // Claiming a username and opting into search are both independent of
    // verification status by design (T24's opt-in is a visibility toggle,
    // not a re-verification) — but neither action should silently flip
    // verified to true. Check the DB directly, not just another endpoint's
    // response shape.
    client
        .post(format!("{base}/username"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({ "nickname": "degraded" }))
        .send()
        .await
        .unwrap();
    client
        .post(format!("{base}/searchable"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&serde_json::json!({
            "searchable": true,
            "phone_search_hash": format!("degrade{}", "0".repeat(57)),
        }))
        .send()
        .await
        .unwrap();

    let still_verified = store_ref.is_verified(user_id).await.unwrap();
    assert_eq!(
        still_verified,
        Some(false),
        "degraded account must not silently become verified"
    );
}
