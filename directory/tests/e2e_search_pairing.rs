//! T14, partial and honestly so: "search -> match -> MLS pairing handoff."
//!
//! What's real and tested here: a search hit carries exactly the identity
//! info (`user_id`, `handle`) a pairing-initiation step would need as
//! input.
//!
//! What's NOT built, anywhere in this codebase, and is NOT tested here:
//! the actual handoff from a directory search result to an MLS pairing
//! session. Pairing today (`core::pairing`, `engine::pairing`) is entirely
//! link/QR-exchange based — a `ContactLink` carries a `KeyPackage` plus a
//! bootstrap-queue endpoint, obtained out-of-band. Nothing in `directory`
//! stores, serves, or references a `KeyPackage`, and there's no mechanism
//! for "request pairing with the user I just found," especially if that
//! user is offline. Building that bridge is a new, undesigned, security-
//! relevant feature (does directory need to hold KeyPackages? are they
//! one-time or reusable — MLS best practice wants one-time; a reusable
//! public KeyPackage server would be a meaningful deviation? how does an
//! offline user's device receive the pairing request?) — not something to
//! improvise as a side effect of an E2E test. See
//! `docs/plans/tasks-identity-directory-pivot.md` T14 note for the
//! decision this actually needs before a real version of this test can
//! exist. The "2-minute onboarding, zero shared contacts" bar in T14's
//! verify criterion is unmeasurable until that bridge exists.

use std::sync::Arc;

use directory::{AppState, DirectoryConfig, DirectoryStore, RateLimiter};
use sqlx::PgPool;

#[sqlx::test]
async fn search_hit_carries_the_identity_info_a_future_pairing_handoff_would_need(pool: PgPool) {
    let state = Arc::new(AppState {
        store: Arc::new(DirectoryStore::from_pool(pool)),
        vendor: Arc::new(directory::DevOtpVendor),
        pepper: b"e2e-test-pepper".to_vec(),
        config: DirectoryConfig {
            accounts_enabled: true,
            search_enabled: true,
        },
        rate_limiter: RateLimiter::new(),
    });
    let phone = "+15559990099";
    let auth_hash = directory::phone_hash(
        &directory::normalize_e164(phone).unwrap(),
        directory::Pepper(b"e2e-test-pepper"),
    )
    .unwrap();
    let user_id = state
        .store
        .find_or_create_pending_user(&auth_hash)
        .await
        .unwrap();
    state.store.claim_username(user_id, "findme").await.unwrap();
    // The search hash is a SEPARATE, unkeyed value a real client computes
    // locally (SHA-256 of the normalized number, no server secret) — not
    // the keyed auth hash above, which no client can reproduce (OQ4).
    let search_hash = format!("findme0{}", "0".repeat(57));
    state
        .store
        .set_searchable(user_id, true, Some(&search_hash))
        .await
        .unwrap();
    let token = state.store.create_session(user_id);

    let addr = directory::spawn_for_tests(state).await.unwrap();
    let prefix = directory::hash_prefix(&search_hash);
    let resp = reqwest::Client::new()
        .get(format!("http://{addr}/search?prefix={prefix}"))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .await
        .unwrap();
    let body: serde_json::Value = resp.json().await.unwrap();
    let results = body["results"].as_array().unwrap();
    assert_eq!(results.len(), 1);

    // This is the entire surface a pairing-initiation step could build on
    // today: a user_id and a display handle. No KeyPackage, no queue
    // endpoint, no way to reach this user if they're offline. That gap is
    // the real content of T14, not this assertion.
    assert!(results[0]["user_id"].is_u64());
    assert!(results[0]["handle"].is_string());
}
