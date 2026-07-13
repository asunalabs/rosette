//! HTTP API (T4, T5, T16). JSON over axum — directory is a centrally-run
//! service behind a normal reverse proxy, unlike relay's raw TCP protocol
//! for self-hosted operators, so the ops vocabulary here (health endpoint,
//! Cache-Control) is the standard HTTP-microservice one.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::config::DirectoryConfig;
use crate::ratelimit::{RateLimiter, UNVERIFIED_SEARCH_PER_MINUTE, VERIFIED_SEARCH_PER_MINUTE};
use crate::search::{search_by_prefix, PREFIX_LEN_HEX};
use crate::store::{ClaimError, DirectoryStore};
use crate::username::format_handle;
use crate::verify::{self, OtpVendor, Pepper, VerificationOutcome, VerifyError};

pub struct AppState {
    pub store: Arc<DirectoryStore>,
    pub vendor: Arc<dyn OtpVendor>,
    pub pepper: Vec<u8>,
    pub config: DirectoryConfig,
    pub rate_limiter: RateLimiter,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/signup", post(signup))
        .route("/verify", post(verify_handler))
        .route("/username", post(claim_username))
        .route("/searchable", post(set_searchable))
        .route("/account", delete(delete_account))
        .route("/search", get(search))
        .route("/pairing-bootstrap", post(set_pairing_bootstrap))
        .route("/pairing-bootstrap/request", post(request_pairing_bootstrap))
        .with_state(state)
        .layer(middleware::from_fn(no_store_middleware))
}

pub async fn bind_and_serve(addr: &str, state: Arc<AppState>) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("directory listening on {}", listener.local_addr()?);
    axum::serve(listener, router(state)).await?;
    Ok(())
}

/// Binds an OS-assigned port and serves in the background — used by tests
/// (and available to any future dev tooling) rather than a fixed port.
pub async fn spawn_for_tests(state: Arc<AppState>) -> anyhow::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(listener, router(state)).await;
    });
    Ok(addr)
}

// T16: every response, success or error, carries Cache-Control: no-store.
async fn no_store_middleware(req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    res.headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    res
}

enum ApiError {
    BadRequest(&'static str),
    Unauthorized,
    FeatureDisabled,
    RateLimited,
    NotFound,
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::FeatureDisabled => (StatusCode::SERVICE_UNAVAILABLE, "feature disabled"),
            ApiError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate limited"),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            ApiError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// T4: every search caller must be authenticated. Used by /username and
/// /searchable too, since those are also account-scoped actions.
fn authenticate(headers: &HeaderMap, store: &DirectoryStore) -> Result<u64, ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthorized)?;
    store.session_user_id(token).ok_or(ApiError::Unauthorized)
}

async fn health() -> &'static str {
    "ok"
}

#[derive(Deserialize)]
struct SignupRequest {
    phone: String,
}

#[derive(Serialize)]
struct SignupResponse {
    status: &'static str,
}

async fn signup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SignupRequest>,
) -> Result<Json<SignupResponse>, ApiError> {
    if !state.config.accounts_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    let e164 =
        verify::normalize_e164(&req.phone).map_err(|_| ApiError::BadRequest("invalid phone"))?;
    let hash = verify::phone_hash(&e164, Pepper(&state.pepper)).map_err(|_| ApiError::Internal)?;
    if state
        .store
        .is_phone_in_cooldown(&hash)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        return Err(ApiError::BadRequest(
            "phone number is in cooldown after a recent deletion",
        ));
    }
    state
        .vendor
        .send_code(&e164)
        .map_err(|_| ApiError::Internal)?;
    if state
        .store
        .find_user_by_phone_hash(&hash)
        .await
        .map_err(|_| ApiError::Internal)?
        .is_none()
    {
        state
            .store
            .create_pending_user(&hash)
            .await
            .map_err(|_| ApiError::Internal)?;
    }
    Ok(Json(SignupResponse {
        status: "code_sent",
    }))
}

#[derive(Deserialize)]
struct VerifyRequest {
    phone: String,
    code: String,
}

#[derive(Serialize)]
struct VerifyResponse {
    user_id: u64,
    session_token: String,
    verified: bool,
}

async fn verify_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    if !state.config.accounts_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    let (hash, outcome) = verify::verify_phone(
        state.vendor.as_ref(),
        &req.phone,
        &req.code,
        Pepper(&state.pepper),
    )
    .map_err(|e| match e {
        VerifyError::CodeRejected => ApiError::BadRequest("code rejected"),
        VerifyError::InvalidPhoneFormat => ApiError::BadRequest("invalid phone"),
        VerifyError::Hash(_) | VerifyError::Vendor(_) => ApiError::Internal,
    })?;

    let user_id = match state
        .store
        .find_user_by_phone_hash(&hash)
        .await
        .map_err(|_| ApiError::Internal)?
    {
        Some(id) => id,
        None => state
            .store
            .create_pending_user(&hash)
            .await
            .map_err(|_| ApiError::Internal)?,
    };
    let verified = matches!(outcome, VerificationOutcome::Verified);
    state
        .store
        .mark_verified(user_id, verified)
        .await
        .map_err(|_| ApiError::Internal)?;
    let session_token = state.store.create_session(user_id);
    Ok(Json(VerifyResponse {
        user_id,
        session_token,
        verified,
    }))
}

#[derive(Deserialize)]
struct UsernameRequest {
    nickname: String,
}

#[derive(Serialize)]
struct UsernameResponse {
    handle: String,
}

async fn claim_username(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UsernameRequest>,
) -> Result<Json<UsernameResponse>, ApiError> {
    let user_id = authenticate(&headers, &state.store)?;
    let (slot, width) = state
        .store
        .claim_username(user_id, &req.nickname)
        .await
        .map_err(|e| match e {
            ClaimError::Username(_) => ApiError::BadRequest("invalid or taken nickname"),
            ClaimError::Db(_) => ApiError::Internal,
        })?;
    Ok(Json(UsernameResponse {
        handle: format_handle(&req.nickname, slot, width),
    }))
}

#[derive(Deserialize)]
struct SearchableRequest {
    searchable: bool,
}

async fn set_searchable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SearchableRequest>,
) -> Result<StatusCode, ApiError> {
    let user_id = authenticate(&headers, &state.store)?;
    state
        .store
        .set_searchable(user_id, req.searchable)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PairingBootstrapRequest {
    /// The exact base64 string `ChatEngine::contact_link()` produces for a
    /// QR code: a fresh one-time KeyPackage plus this user's bootstrap
    /// mailbox endpoint. Directory stores and serves it opaquely — it never
    /// decodes it, so it never needs to depend on `core`/`engine` (T1).
    contact_link_b64: String,
}

/// T25: publish (or replenish) this account's one-time pairing bootstrap,
/// so a directory-search hit can request pairing without a QR/link
/// exchange. Replenishment after a peer consumes it is a client concern —
/// this just stores whatever the caller most recently uploaded.
async fn set_pairing_bootstrap(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<PairingBootstrapRequest>,
) -> Result<StatusCode, ApiError> {
    let user_id = authenticate(&headers, &state.store)?;
    if req.contact_link_b64.trim().is_empty() {
        return Err(ApiError::BadRequest("contact_link_b64 must not be empty"));
    }
    state
        .store
        .set_pairing_bootstrap(user_id, &req.contact_link_b64)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PairingBootstrapQuery {
    user_id: u64,
}

#[derive(Serialize)]
struct PairingBootstrapResponse {
    contact_link_b64: String,
}

/// T25: the search-to-pairing handoff. Consumes (deletes) the target's
/// one-time bootstrap and hands it to the caller — same rate limits and
/// verified/unverified tiers as `/search`, since this is the same "look up
/// another user" abuse surface (T20/T22).
async fn request_pairing_bootstrap(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<PairingBootstrapQuery>,
) -> Result<Json<PairingBootstrapResponse>, ApiError> {
    if !state.config.search_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    let caller_id = authenticate(&headers, &state.store)?;

    let caller_verified = state
        .store
        .is_verified(caller_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .unwrap_or(false);
    let limit = if caller_verified {
        VERIFIED_SEARCH_PER_MINUTE
    } else {
        UNVERIFIED_SEARCH_PER_MINUTE
    };
    if !state.rate_limiter.check_and_bump(caller_id, limit) {
        return Err(ApiError::RateLimited);
    }

    let contact_link_b64 = state
        .store
        .consume_pairing_bootstrap(q.user_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(PairingBootstrapResponse { contact_link_b64 }))
}

/// T15/T19: real erasure, not a flag flip — see `DirectoryStore::erase_user`.
/// Also starts the OQ5 cooldown on the now-freed phone number.
async fn delete_account(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let user_id = authenticate(&headers, &state.store)?;
    state
        .store
        .erase_user(user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct SearchQuery {
    prefix: String,
}

#[derive(Serialize)]
struct SearchResultEntry {
    user_id: u64,
    handle: String,
}

#[derive(Serialize)]
struct SearchResponse {
    results: Vec<SearchResultEntry>,
}

async fn search(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<SearchQuery>,
) -> Result<Json<SearchResponse>, ApiError> {
    if !state.config.search_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    // T4: authenticated caller required.
    let caller_id = authenticate(&headers, &state.store)?;

    // T20: unverified callers get a measurably tighter limit.
    let caller_verified = state
        .store
        .is_verified(caller_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .unwrap_or(false);
    let limit = if caller_verified {
        VERIFIED_SEARCH_PER_MINUTE
    } else {
        UNVERIFIED_SEARCH_PER_MINUTE
    };
    // T22: bulk sequential search from one account gets rate-limited.
    if !state.rate_limiter.check_and_bump(caller_id, limit) {
        return Err(ApiError::RateLimited);
    }

    if q.prefix.len() != PREFIX_LEN_HEX {
        return Err(ApiError::BadRequest(
            "prefix must be exactly PREFIX_LEN_HEX hex chars",
        ));
    }

    let bucket = search_by_prefix(state.store.as_ref(), &q.prefix).await;
    let mut results = Vec::with_capacity(bucket.len());
    for entry in bucket {
        // T8: the response type has no field for phone_hash at all — it
        // physically cannot leak here, not just "isn't rendered."
        if let Ok(Some(handle)) = state.store.handle_for(entry.user_id).await {
            results.push(SearchResultEntry {
                user_id: entry.user_id,
                handle,
            });
        }
    }
    Ok(Json(SearchResponse { results }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::DevOtpVendor;
    use sqlx::PgPool;

    fn state_for(pool: PgPool) -> Arc<AppState> {
        Arc::new(AppState {
            store: Arc::new(DirectoryStore::from_pool(pool)),
            vendor: Arc::new(DevOtpVendor),
            pepper: b"test-pepper".to_vec(),
            config: DirectoryConfig {
                accounts_enabled: true,
                search_enabled: true,
            },
            rate_limiter: RateLimiter::new(),
        })
    }

    #[sqlx::test]
    async fn pairing_bootstrap_is_consumed_exactly_once(pool: PgPool) {
        let state = state_for(pool);
        let target = state.store.create_pending_user("target-hash-0").await.unwrap();
        state.store.claim_username(target, "pairtarget").await.unwrap();
        state.store.set_searchable(target, true).await.unwrap();
        let target_token = state.store.create_session(target);

        let requester = state.store.create_pending_user("requester-hash-0").await.unwrap();
        let requester_token = state.store.create_session(requester);

        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        // No bootstrap uploaded yet: a request finds nothing.
        let miss = client
            .post(format!("http://{addr}/pairing-bootstrap/request?user_id={target}"))
            .header("Authorization", format!("Bearer {requester_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(miss.status(), reqwest::StatusCode::NOT_FOUND);

        // Target publishes a bootstrap.
        let upload = client
            .post(format!("http://{addr}/pairing-bootstrap"))
            .header("Authorization", format!("Bearer {target_token}"))
            .json(&serde_json::json!({ "contact_link_b64": "opaque-link-bytes" }))
            .send()
            .await
            .unwrap();
        assert_eq!(upload.status(), reqwest::StatusCode::NO_CONTENT);

        // First request succeeds and returns exactly what was uploaded.
        let first = client
            .post(format!("http://{addr}/pairing-bootstrap/request?user_id={target}"))
            .header("Authorization", format!("Bearer {requester_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = first.json().await.unwrap();
        assert_eq!(body["contact_link_b64"], "opaque-link-bytes");

        // A second request for the same target finds nothing: one-time use.
        let second = client
            .post(format!("http://{addr}/pairing-bootstrap/request?user_id={target}"))
            .header("Authorization", format!("Bearer {requester_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn pairing_bootstrap_unreachable_for_a_non_searchable_target(pool: PgPool) {
        let state = state_for(pool);
        let target = state.store.create_pending_user("target-hash-1").await.unwrap();
        // Never calls set_searchable(true) — stays private.
        state
            .store
            .set_pairing_bootstrap(target, "opaque-link-bytes")
            .await
            .unwrap();

        let requester = state.store.create_pending_user("requester-hash-1").await.unwrap();
        let requester_token = state.store.create_session(requester);

        let addr = spawn_for_tests(state).await.unwrap();
        let resp = reqwest::Client::new()
            .post(format!("http://{addr}/pairing-bootstrap/request?user_id={target}"))
            .header("Authorization", format!("Bearer {requester_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn erasing_a_user_deletes_their_pairing_bootstrap(pool: PgPool) {
        let state = state_for(pool);
        let target = state.store.create_pending_user("target-hash-2").await.unwrap();
        state.store.set_searchable(target, true).await.unwrap();
        state
            .store
            .set_pairing_bootstrap(target, "opaque-link-bytes")
            .await
            .unwrap();

        state.store.erase_user(target).await.unwrap();

        assert_eq!(
            state.store.consume_pairing_bootstrap(target).await.unwrap(),
            None,
            "an erased user's bootstrap must not be servable"
        );
    }

    #[sqlx::test]
    async fn unauthenticated_search_is_rejected(pool: PgPool) {
        let state = state_for(pool);
        let addr = spawn_for_tests(state).await.unwrap();
        let resp = reqwest::Client::new()
            .get(format!("http://{addr}/search?prefix=abcde"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test]
    async fn every_response_carries_cache_control_no_store(pool: PgPool) {
        let state = state_for(pool);
        let addr = spawn_for_tests(state).await.unwrap();
        // Check both a 200 (health) and a 401 (unauthenticated search) —
        // T16 says *all* responses, not just successful ones.
        let health = reqwest::Client::new()
            .get(format!("http://{addr}/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(health.headers().get("cache-control").unwrap(), "no-store");

        let unauth = reqwest::Client::new()
            .get(format!("http://{addr}/search?prefix=abcde"))
            .send()
            .await
            .unwrap();
        assert_eq!(unauth.headers().get("cache-control").unwrap(), "no-store");
    }

    #[sqlx::test]
    async fn search_feature_flag_disables_search_but_not_signup(pool: PgPool) {
        let mut state = state_for(pool);
        Arc::get_mut(&mut state).unwrap().config.search_enabled = false;
        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        let signup = client
            .post(format!("http://{addr}/signup"))
            .json(&serde_json::json!({ "phone": "+15551234567" }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            signup.status(),
            reqwest::StatusCode::OK,
            "signup must still work"
        );

        let search = client
            .get(format!("http://{addr}/search?prefix=abcde"))
            .header("Authorization", "Bearer whatever")
            .send()
            .await
            .unwrap();
        assert_eq!(search.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);
    }

    #[sqlx::test]
    async fn delete_account_erases_and_removes_from_search(pool: PgPool) {
        let state = state_for(pool);
        let user_id = state
            .store
            .create_pending_user("erase-me-hash-000000000000000000000000000000000000000000000000")
            .await
            .unwrap();
        state.store.claim_username(user_id, "temp").await.unwrap();
        state.store.set_searchable(user_id, true).await.unwrap();
        let token = state.store.create_session(user_id);

        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();
        let resp = client
            .delete(format!("http://{addr}/account"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

        // Now findable-by-search must return nothing for that prefix.
        let search = client
            .get(format!("http://{addr}/search?prefix=erase"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = search.json().await.unwrap();
        assert_eq!(body["results"].as_array().unwrap().len(), 0);
    }

    #[sqlx::test]
    async fn search_response_never_contains_a_phone_hash_field(pool: PgPool) {
        // T8: even inspecting the raw JSON, there's no key that could hold
        // one — the response type doesn't have the field.
        let state = state_for(pool);
        let user_id = state
            .store
            .create_pending_user("aaaaaverysecretphonehash")
            .await
            .unwrap();
        state
            .store
            .claim_username(user_id, "findable")
            .await
            .unwrap();
        state.store.set_searchable(user_id, true).await.unwrap();
        let token = state.store.create_session(user_id);

        let addr = spawn_for_tests(state).await.unwrap();
        let resp = reqwest::Client::new()
            .get(format!("http://{addr}/search?prefix=aaaaa"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        let body: serde_json::Value = resp.json().await.unwrap();
        let text = body.to_string();
        assert!(
            !text.contains("phone"),
            "response leaked a phone-shaped field: {text}"
        );
        assert!(text.contains("findable"));
    }
}
