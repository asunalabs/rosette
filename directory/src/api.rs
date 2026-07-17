//! HTTP API (T4, T5, T16). JSON over axum — directory is a centrally-run
//! service behind a normal reverse proxy, unlike relay's raw TCP protocol
//! for self-hosted operators, so the ops vocabulary here (health endpoint,
//! Cache-Control) is the standard HTTP-microservice one.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::config::DirectoryConfig;
use crate::ratelimit::{
    self, RateLimiter, UNAUTH_REQUESTS_PER_MINUTE_PER_IP, UNVERIFIED_SEARCH_PER_MINUTE,
    VERIFIED_SEARCH_PER_MINUTE, VERIFY_ATTEMPTS_PER_MINUTE,
};
use crate::search::{search_by_prefix, PREFIX_LEN_HEX};
use crate::store::{BackupUpload, ClaimError, DirectoryStore, RestoreVerdict};
use crate::username::format_handle;
use crate::verify::{self, OtpVendor, Pepper, VendorError, VerifyError};

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
        .route("/username-lookup", get(username_lookup))
        .route("/searchable", post(set_searchable))
        .route("/account", delete(delete_account))
        .route("/search", get(search))
        .route("/pairing-bootstrap", post(set_pairing_bootstrap))
        .route(
            "/pairing-bootstrap/request",
            post(request_pairing_bootstrap),
        )
        .route("/v1/backup", put(put_backup))
        .route("/v1/backup/restore/begin", post(restore_begin))
        .route("/v1/backup/restore/complete", post(restore_complete))
        .with_state(state)
        .layer(middleware::from_fn(no_store_middleware))
}

/// `into_make_service_with_connect_info` (ET1): without it `ConnectInfo` is not
/// in the request extensions and the per-IP limit's extractor rejects every
/// request. Both entry points need it, and the tests go through the same one
/// the server does — an unthrottled test harness would be a harness for
/// different code.
pub async fn bind_and_serve(addr: &str, state: Arc<AppState>) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("directory listening on {}", listener.local_addr()?);
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Binds an OS-assigned port and serves in the background — used by tests
/// (and available to any future dev tooling) rather than a fixed port.
pub async fn spawn_for_tests(state: Arc<AppState>) -> anyhow::Result<SocketAddr> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
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
    /// The OTP vendor didn't answer, so the code went unchecked. Distinct
    /// from `Internal`: it's transient and the client should offer a retry.
    VendorUnavailable,
    RateLimited,
    NotFound,
    Internal,
    /// Issue #3: wrong restore secret. The message carries the remaining
    /// attempts (PIN path) so the client can show it verbatim.
    WrongSecret {
        remaining: Option<i32>,
    },
    /// Issue #3: PIN path locked out; the message names the wait verbatim.
    Locked {
        seconds: i64,
    },
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::FeatureDisabled => (StatusCode::SERVICE_UNAVAILABLE, "feature disabled"),
            ApiError::VendorUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                "verification temporarily unavailable",
            ),
            ApiError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate limited"),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            ApiError::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal error"),
            ApiError::WrongSecret { remaining } => {
                let msg = match remaining {
                    Some(n) => format!("wrong PIN — {n} attempts left before a lockout"),
                    None => "wrong recovery phrase".to_string(),
                };
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({
                        "error": msg,
                        "remaining_attempts": remaining,
                    })),
                )
                    .into_response();
            }
            ApiError::Locked { seconds } => {
                let human = if seconds >= 3600 {
                    format!("{} h", (seconds + 3599) / 3600)
                } else {
                    format!("{} min", ((seconds + 59) / 60).max(1))
                };
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({
                        "error": format!("too many wrong PINs — try again in {human}, or use your recovery phrase"),
                        "retry_after_secs": seconds,
                    })),
                )
                    .into_response();
            }
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

/// ET1: the address Caddy actually saw, for the endpoints with no caller to key
/// a limit on.
///
/// **Why `X-Forwarded-For` is trustworthy here, when in general it is not.**
/// `directory` is not reachable directly: `deploy/docker-compose.yml` publishes
/// no port for it (only Caddy's 80/443), so the sole path in is Caddy's
/// `reverse_proxy directory:7444` — one hop, same box, same compose network.
/// There is no second proxy and no direct route to forge around.
///
/// **The rightmost entry, not the leftmost.** Caddy *appends* the peer it saw,
/// so a client who sends `X-Forwarded-For: 1.2.3.4` produces `1.2.3.4, <real>`.
/// The last entry is the one Caddy wrote; everything left of it is
/// attacker-controlled. The usual "client IP = first entry" idiom would hand an
/// attacker a forged key, letting them throttle *someone else* — worse than not
/// limiting at all, which is exactly why this was deferred until the deployment
/// was known rather than guessed.
///
/// Falls back to the socket peer when the header is absent: that is the local
/// path (`cargo run`, the e2e tests), where there is no proxy and the peer is
/// the client. **If a second hop is ever put in front of Caddy, revisit this** —
/// the rightmost entry would then be Caddy's own address and every caller would
/// share one bucket.
fn client_ip(headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .map(str::trim)
        .and_then(|s| s.parse::<IpAddr>().ok())
        .unwrap_or_else(|| peer.ip())
}

/// T4: every search caller must be authenticated. Used by /username and
/// /searchable too, since those are also account-scoped actions.
async fn authenticate(headers: &HeaderMap, store: &DirectoryStore) -> Result<u64, ApiError> {
    let value = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(ApiError::Unauthorized)?;
    let token = value
        .strip_prefix("Bearer ")
        .ok_or(ApiError::Unauthorized)?;
    store
        .session_user_id(token)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::Unauthorized)
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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<SignupRequest>,
) -> Result<Json<SignupResponse>, ApiError> {
    if !state.config.accounts_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    // ET1: before the Argon2id below, which is the whole cost of this endpoint.
    // ET19 moved that hash off the executor so a flood cannot stall other
    // endpoints; it still burns 19 MiB and two passes per call, and nothing
    // authenticates here. The per-number limit does not help — a flood uses a
    // different number every time.
    if !state
        .rate_limiter
        .check_and_bump_ip(client_ip(&headers, peer), UNAUTH_REQUESTS_PER_MINUTE_PER_IP)
    {
        return Err(ApiError::RateLimited);
    }
    let e164 =
        verify::normalize_e164(&req.phone).map_err(|_| ApiError::BadRequest("invalid phone"))?;
    // ET19: Argon2id is deliberately expensive — 19 MiB and 2 passes — and this
    // call is unauthenticated and unthrottled. Run directly, each one pins a
    // tokio worker for the duration, so a /signup flood starves *every* endpoint
    // rather than just this one. ET6 moved /verify's hash behind an approved code
    // for the same reason; that mitigation stops at this endpoint, which is one
    // over and wide open.
    //
    // `spawn_blocking`, not the `block_in_place` used for the vendor HTTP call in
    // verify.rs: that one panics on a current-thread runtime, which is exactly
    // what `#[sqlx::test]` gives the e2e tests. The clones are a pepper and a
    // phone number — noise next to 19 MiB of hashing.
    let hash = {
        let pepper = state.pepper.clone();
        let e164 = e164.clone();
        tokio::task::spawn_blocking(move || verify::phone_hash(&e164, Pepper(&pepper)))
            .await
            .map_err(|_| ApiError::Internal)?
            .map_err(|_| ApiError::Internal)?
    };
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
    // Classified exactly like `/verify`'s vendor failure (ET6), and for the
    // same reason: a vendor outage stops the user *here*, one screen before
    // the held screen ET8 built for it. Flattening this to 500 made that
    // screen reachable only if the vendor was up for `send_code` and down for
    // `verify` seconds later.
    state.vendor.send_code(&e164).map_err(|e| match e {
        VendorError::Unavailable => ApiError::VendorUnavailable,
        VendorError::Other(_) => ApiError::Internal,
    })?;
    // ET15: one statement, not a check-then-act. This used to be a find followed
    // by a create, which two concurrent signups for the same number could both
    // pass — leaving a duplicate row that `erase_user` (single `user_id`) would
    // not scrub.
    state
        .store
        .find_or_create_pending_user(&hash)
        .await
        .map_err(|_| ApiError::Internal)?;
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
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<VerifyRequest>,
) -> Result<Json<VerifyResponse>, ApiError> {
    if !state.config.accounts_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    // ET1, the resource half: the per-number limit below stops a *victim's* code
    // being guessed; it never trips for a flood that uses a different number
    // each time, and every one of those still buys a vendor round trip.
    if !state
        .rate_limiter
        .check_and_bump_ip(client_ip(&headers, peer), UNAUTH_REQUESTS_PER_MINUTE_PER_IP)
    {
        return Err(ApiError::RateLimited);
    }
    // ET1: throttle before the vendor call, not after. This endpoint is
    // unauthenticated by construction — it is how a caller *gets* a session — so
    // the key is the number being claimed, not a `user_id`. Guarding here is
    // what makes it a guard at all: `verify_phone` is where the 6-digit guess is
    // spent and where a worker gets pinned for the vendor round trip, so a check
    // after it would count the damage rather than prevent it.
    //
    // Normalizing twice (here and inside `verify_phone`) is deliberate: it costs
    // a string scan and keeps the guard readable at the top of the handler,
    // rather than threading a pre-normalized number through `verify_phone`'s
    // signature to save it.
    let e164 =
        verify::normalize_e164(&req.phone).map_err(|_| ApiError::BadRequest("invalid phone"))?;
    if !state
        .rate_limiter
        .check_and_bump_phone(ratelimit::phone_rate_key(&e164), VERIFY_ATTEMPTS_PER_MINUTE)
    {
        return Err(ApiError::RateLimited);
    }
    // `verify_phone` returning Ok IS the verification — a vendor timeout is a
    // 503 here, never a session (ARCH-5). Everything below this line is
    // reachable only for a code the vendor affirmatively approved.
    let hash = verify::verify_phone(
        state.vendor.as_ref(),
        &req.phone,
        &req.code,
        Pepper(&state.pepper),
    )
    .map_err(|e| match e {
        VerifyError::CodeRejected => ApiError::BadRequest("code rejected"),
        VerifyError::InvalidPhoneFormat => ApiError::BadRequest("invalid phone"),
        VerifyError::VendorUnavailable => ApiError::VendorUnavailable,
        VerifyError::Hash(_) | VerifyError::Vendor(_) => ApiError::Internal,
    })?;

    // ET14: `/verify` is the *second* endpoint that creates users, so it owes
    // the same OQ5 cooldown check `/signup` does. Without it: sign up ->
    // `DELETE /account` (cooldown starts, `users.phone_hash` is scrubbed but
    // `phone_cooldown` keeps it) -> `POST /verify` with the still-valid code ->
    // no live row matches the hash -> a fresh *verified* account is minted for a
    // number that is supposed to be locked for 24h.
    //
    // Checked after `verify_phone` because `phone_cooldown` is keyed on the
    // hash, and only an approved code yields one. Still a call-site check, which
    // is the shape of the original bug — ET15 collapsed the two find-or-creates
    // into one statement, so this is now the only rule the two endpoints must
    // remember in parallel. It belongs in the DB.
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
    // ET15: the other half of the same check-then-act — see `signup`.
    let user_id = state
        .store
        .find_or_create_pending_user(&hash)
        .await
        .map_err(|_| ApiError::Internal)?;
    state
        .store
        .mark_verified(user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    let session_token = state
        .store
        .create_session(user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    // Always true now that this is the only path that reaches `create_session`.
    // Kept on the wire so the client's `nextAfterVerify` gate stays live as
    // defense-in-depth against a future server regression (founder decision).
    Ok(Json(VerifyResponse {
        user_id,
        session_token,
        verified: true,
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
    let user_id = authenticate(&headers, &state.store).await?;
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
    /// Client-computed, unkeyed SHA-256 hex digest of the account's
    /// normalized phone number (see `store::DirectoryStore::set_searchable`)
    /// — required when `searchable` is true.
    phone_search_hash: Option<String>,
}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_hexdigit())
}

async fn set_searchable(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<SearchableRequest>,
) -> Result<StatusCode, ApiError> {
    let user_id = authenticate(&headers, &state.store).await?;
    let hash = if req.searchable {
        let Some(hash) = &req.phone_search_hash else {
            return Err(ApiError::BadRequest(
                "phone_search_hash is required when searchable is true",
            ));
        };
        if !is_hex(hash, 64) {
            return Err(ApiError::BadRequest(
                "phone_search_hash must be a 64-hex-char SHA-256 digest",
            ));
        }
        Some(hash.as_str())
    } else {
        None
    };
    state
        .store
        .set_searchable(user_id, req.searchable, hash)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct UsernameLookupQuery {
    nickname: String,
    discriminator: u32,
}

#[derive(Serialize)]
struct UsernameLookupResponse {
    user_id: u64,
}

/// Public username lookup (OQ10) — the default discovery path, no
/// `searchable` gate (claiming a handle is itself the opt-in). Still
/// authenticated + logged-nothing, same posture as every other endpoint.
async fn username_lookup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<UsernameLookupQuery>,
) -> Result<Json<UsernameLookupResponse>, ApiError> {
    authenticate(&headers, &state.store).await?;
    let user_id = state
        .store
        .find_user_by_handle(&q.nickname, q.discriminator)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(UsernameLookupResponse { user_id }))
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
    let user_id = authenticate(&headers, &state.store).await?;
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
    let caller_id = authenticate(&headers, &state.store).await?;

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
    let user_id = authenticate(&headers, &state.store).await?;
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
    /// The unkeyed search hash (see migration 0003) for this bucket member
    /// — HIBP-style k-anonymity requires the *client* to do the final exact
    /// match against hashes it computed for its own contacts; the server
    /// only ever narrows to a ~20-bit bucket (T3/T17), never picks the
    /// match itself. Deliberately present, unlike `phone_hash` (T8): this
    /// value has no server secret behind it and is exactly what search
    /// exists to hand back — see `search_hash_is_present_but_the_keyed_auth_hash_never_is`.
    search_hash: String,
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
    let caller_id = authenticate(&headers, &state.store).await?;

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
        // T8: the response type has no field for the KEYED auth phone_hash
        // — it physically cannot leak here. `search_hash` below is the
        // separate, unkeyed value this endpoint exists to return.
        if let Ok(Some(handle)) = state.store.handle_for(entry.user_id).await {
            results.push(SearchResultEntry {
                user_id: entry.user_id,
                handle,
                search_hash: entry.phone_hash,
            });
        }
    }
    Ok(Json(SearchResponse { results }))
}

/// Issue #2: base64 fields straight from the client's `BackupBundle`.
#[derive(Deserialize)]
struct BackupPutRequest {
    blob: String,
    w_pin: String,
    salt_p: String,
    w_phrase: String,
    salt_f: String,
    auth_pin: String,
    salt_a: String,
    auth_phrase: String,
    salt_pa: String,
}

/// Issue #2: store this account's E2E-encrypted recovery bundle (upsert on
/// the caller's own row — the target is never chosen by the client). The
/// server checks shapes only; contents are opaque ciphertext by design.
/// Retrieval, with PIN/phrase proof and the 10-attempt lockout, is issue #3.
async fn put_backup(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<BackupPutRequest>,
) -> Result<StatusCode, ApiError> {
    let user_id = authenticate(&headers, &state.store).await?;
    let b64 = |s: &str| {
        base64::engine::general_purpose::STANDARD
            .decode(s)
            .map_err(|_| ApiError::BadRequest("all bundle fields must be base64"))
    };
    let upload = BackupUpload {
        blob: b64(&req.blob)?,
        w_pin: b64(&req.w_pin)?,
        salt_p: b64(&req.salt_p)?,
        w_phrase: b64(&req.w_phrase)?,
        salt_f: b64(&req.salt_f)?,
        auth_pin_hash: b64(&req.auth_pin)?,
        salt_a: b64(&req.salt_a)?,
        auth_phrase_hash: b64(&req.auth_phrase)?,
        salt_pa: b64(&req.salt_pa)?,
    };
    if [
        &upload.salt_p,
        &upload.salt_f,
        &upload.salt_a,
        &upload.salt_pa,
    ]
    .iter()
    .any(|s| s.len() != 16)
    {
        return Err(ApiError::BadRequest("salts must be 16 bytes"));
    }
    if upload.auth_pin_hash.len() != 32 || upload.auth_phrase_hash.len() != 32 {
        return Err(ApiError::BadRequest("auth hashes must be 32 bytes"));
    }
    if upload.blob.is_empty() || upload.w_pin.is_empty() || upload.w_phrase.is_empty() {
        return Err(ApiError::BadRequest("ciphertexts must not be empty"));
    }
    state
        .store
        .upsert_backup(user_id, &upload)
        .await
        .map_err(|_| ApiError::Internal)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Issue #3: restore tokens live this long — enough for a couple of PIN
/// attempts, short enough that a stolen token is nearly useless.
const RESTORE_TOKEN_TTL_SECS: i64 = 600;

fn b64e(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[derive(Deserialize)]
struct RestoreBeginRequest {
    phone: String,
    code: String,
}

#[derive(Serialize)]
struct RestoreBeginResponse {
    restore_token: String,
    session_token: String,
    salt_a: String,
    salt_pa: String,
}

/// Issue #3, step 1: phone re-verification for restore. Returns the two auth
/// salts (public by design) plus a short-lived restore token; the bundle
/// itself is unreachable until `restore_complete` proves the PIN or phrase —
/// phone OTP alone must never hand a SIM-swapper material to brute-force
/// offline.
///
/// An `Ok` from `verify_phone` *is* the hard-verified outcome (ET6): the
/// `Degraded` variant this used to test for no longer exists, because a vendor
/// that cannot answer now fails closed for every caller rather than only for
/// this one. An unreachable vendor is `VendorUnavailable` -> 503, so the
/// SIM-swap ceiling this endpoint was guarding still holds — it is enforced one
/// layer down instead of re-checked here.
async fn restore_begin(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RestoreBeginRequest>,
) -> Result<Json<RestoreBeginResponse>, ApiError> {
    if !state.config.accounts_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    let hash = verify::verify_phone(
        state.vendor.as_ref(),
        &req.phone,
        &req.code,
        Pepper(&state.pepper),
    )
    .map_err(|e| match e {
        VerifyError::CodeRejected => ApiError::BadRequest("code rejected"),
        VerifyError::InvalidPhoneFormat => ApiError::BadRequest("invalid phone"),
        VerifyError::VendorUnavailable => ApiError::VendorUnavailable,
        VerifyError::Hash(_) | VerifyError::Vendor(_) => ApiError::Internal,
    })?;
    let user_id = state
        .store
        .find_user_by_phone_hash(&hash)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    let (salt_a, salt_pa) = state
        .store
        .backup_salts(user_id)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    // The phone is re-verified, so a normal session is fair game too — the
    // restored client needs one for /username-lookup, pairing, re-upload.
    let session_token = state
        .store
        .create_session(user_id)
        .await
        .map_err(|_| ApiError::Internal)?;
    let restore_token = state
        .store
        .create_restore_token(user_id, RESTORE_TOKEN_TTL_SECS);
    Ok(Json(RestoreBeginResponse {
        restore_token,
        session_token,
        salt_a: b64e(&salt_a),
        salt_pa: b64e(&salt_pa),
    }))
}

#[derive(Deserialize)]
struct RestoreCompleteRequest {
    restore_token: String,
    /// "pin" or "phrase" — which auth hash the proof targets.
    method: String,
    /// base64 SHA256(Argon2id(secret, salt)) — see ffi `backup_auth_proof`.
    auth: String,
}

#[derive(Serialize)]
struct RestoreCompleteResponse {
    blob: String,
    w_pin: String,
    salt_p: String,
    w_phrase: String,
    salt_f: String,
    auth_pin: String,
    salt_a: String,
    auth_phrase: String,
    salt_pa: String,
}

/// Issue #3, step 2: prove the PIN or phrase, get the bundle. Wrong PIN
/// counts toward the 10-attempt lockout (schedule in store.rs); the phrase
/// path is never locked. The restore token survives failed attempts and is
/// consumed exactly once — on success.
async fn restore_complete(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RestoreCompleteRequest>,
) -> Result<Json<RestoreCompleteResponse>, ApiError> {
    if !state.config.accounts_enabled {
        return Err(ApiError::FeatureDisabled);
    }
    let user_id = state
        .store
        .restore_token_user(&req.restore_token)
        .ok_or(ApiError::Unauthorized)?;
    let is_pin = match req.method.as_str() {
        "pin" => true,
        "phrase" => false,
        _ => return Err(ApiError::BadRequest("method must be \"pin\" or \"phrase\"")),
    };
    let auth = base64::engine::general_purpose::STANDARD
        .decode(&req.auth)
        .map_err(|_| ApiError::BadRequest("auth must be base64"))?;
    if auth.len() != 32 {
        return Err(ApiError::BadRequest("auth must be 32 bytes"));
    }
    let verdict = state
        .store
        .verify_backup_auth(user_id, &auth, is_pin)
        .await
        .map_err(|_| ApiError::Internal)?
        .ok_or(ApiError::NotFound)?;
    match verdict {
        RestoreVerdict::Match(row) => {
            state.store.consume_restore_token(&req.restore_token);
            Ok(Json(RestoreCompleteResponse {
                blob: b64e(&row.blob),
                w_pin: b64e(&row.w_pin),
                salt_p: b64e(&row.salt_p),
                w_phrase: b64e(&row.w_phrase),
                salt_f: b64e(&row.salt_f),
                auth_pin: b64e(&row.auth_pin_hash),
                salt_a: b64e(&row.salt_a),
                auth_phrase: b64e(&row.auth_phrase_hash),
                salt_pa: b64e(&row.salt_pa),
            }))
        }
        RestoreVerdict::WrongPin { remaining } => Err(ApiError::WrongSecret {
            remaining: Some(remaining),
        }),
        RestoreVerdict::WrongPhrase => Err(ApiError::WrongSecret { remaining: None }),
        RestoreVerdict::Locked { until } => Err(ApiError::Locked {
            seconds: (until - crate::store::now_unix()).max(1),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify::DevOtpVendor;
    use sqlx::PgPool;

    /// ET1: `client_ip` decides *whose* bucket a request counts against, so
    /// reading `X-Forwarded-For` from the wrong end is not a style question — it
    /// hands an attacker a forged key and lets them throttle a stranger, which
    /// is worse than having no limit at all.
    mod client_ip {
        use super::*;

        fn headers(xff: Option<&str>) -> HeaderMap {
            let mut h = HeaderMap::new();
            if let Some(v) = xff {
                h.insert("x-forwarded-for", v.parse().unwrap());
            }
            h
        }

        fn peer() -> SocketAddr {
            "10.0.0.9:5555".parse().unwrap()
        }

        /// Caddy appends the peer it saw, so a client-supplied prefix lands to
        /// the LEFT of the truth. Taking the leftmost — the usual "client IP"
        /// idiom — would read the forgery.
        #[test]
        fn a_forged_prefix_cannot_win_because_we_read_the_end_caddy_wrote() {
            let ip = client_ip(&headers(Some("1.2.3.4, 203.0.113.7")), peer());

            assert_eq!(
                ip,
                "203.0.113.7".parse::<IpAddr>().unwrap(),
                "the rightmost entry is the only one the proxy wrote"
            );
            assert_ne!(ip, "1.2.3.4".parse::<IpAddr>().unwrap());
        }

        #[test]
        fn a_single_entry_is_the_real_client() {
            assert_eq!(
                client_ip(&headers(Some("203.0.113.7")), peer()),
                "203.0.113.7".parse::<IpAddr>().unwrap()
            );
        }

        #[test]
        fn whitespace_after_the_comma_is_not_a_parse_failure() {
            assert_eq!(
                client_ip(&headers(Some("1.2.3.4,   203.0.113.7  ")), peer()),
                "203.0.113.7".parse::<IpAddr>().unwrap()
            );
        }

        /// No proxy: `cargo run`, the e2e tests. The peer *is* the client.
        #[test]
        fn without_the_header_the_socket_peer_is_the_client() {
            assert_eq!(
                client_ip(&headers(None), peer()),
                "10.0.0.9".parse::<IpAddr>().unwrap()
            );
        }

        /// Garbage must fall back to the peer, never panic and never key
        /// everyone onto one bucket by accident.
        #[test]
        fn an_unparseable_header_falls_back_to_the_peer() {
            assert_eq!(
                client_ip(&headers(Some("not-an-ip")), peer()),
                "10.0.0.9".parse::<IpAddr>().unwrap()
            );
            assert_eq!(
                client_ip(&headers(Some("")), peer()),
                "10.0.0.9".parse::<IpAddr>().unwrap()
            );
        }

        #[test]
        fn v6_survives_the_round_trip() {
            assert_eq!(
                client_ip(&headers(Some("2001:db8::1")), peer()),
                "2001:db8::1".parse::<IpAddr>().unwrap()
            );
        }
    }

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

    fn backup_body() -> serde_json::Value {
        let b64 = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        serde_json::json!({
            "blob": b64(&[1, 2, 3]),
            "w_pin": b64(&[4; 56]),
            "salt_p": b64(&[5; 16]),
            "w_phrase": b64(&[6; 56]),
            "salt_f": b64(&[7; 16]),
            "auth_pin": b64(&[8; 32]),
            "salt_a": b64(&[9; 16]),
            "auth_phrase": b64(&[10; 32]),
            "salt_pa": b64(&[11; 16]),
        })
    }

    #[sqlx::test]
    async fn backup_put_requires_auth(pool: PgPool) {
        let state = state_for(pool);
        let addr = spawn_for_tests(state).await.unwrap();
        let resp = reqwest::Client::new()
            .put(format!("http://{addr}/v1/backup"))
            .json(&backup_body())
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    }

    #[sqlx::test]
    async fn backup_put_upserts_own_row_and_rejects_malformed_fields(pool: PgPool) {
        let state = state_for(pool);
        let u = state
            .store
            .find_or_create_pending_user("backup-hash")
            .await
            .unwrap();
        let token = state.store.create_session(u).await.unwrap();
        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        let ok = client
            .put(format!("http://{addr}/v1/backup"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&backup_body())
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), reqwest::StatusCode::NO_CONTENT);

        // Second PUT replaces, not duplicates (204 again, no conflict).
        let again = client
            .put(format!("http://{addr}/v1/backup"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&backup_body())
            .send()
            .await
            .unwrap();
        assert_eq!(again.status(), reqwest::StatusCode::NO_CONTENT);

        let mut short_salt = backup_body();
        short_salt["salt_p"] = serde_json::json!("YWJj"); // 3 bytes
        let bad = client
            .put(format!("http://{addr}/v1/backup"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&short_salt)
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), reqwest::StatusCode::BAD_REQUEST);

        let mut not_b64 = backup_body();
        not_b64["blob"] = serde_json::json!("!!! not base64 !!!");
        let bad = client
            .put(format!("http://{addr}/v1/backup"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&not_b64)
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), reqwest::StatusCode::BAD_REQUEST);
    }

    /// Seeds a user with a backup row whose auth hashes are known bytes.
    /// Returns (phone, pin_auth_b64, phrase_auth_b64).
    async fn restore_fixture(state: &Arc<AppState>) -> (String, String, String) {
        let phone = "+15559990001";
        let hash = crate::verify::phone_hash(phone, Pepper(&state.pepper)).unwrap();
        let u = state
            .store
            .find_or_create_pending_user(&hash)
            .await
            .unwrap();
        let store_upload = crate::store::BackupUpload {
            blob: vec![1, 2, 3],
            w_pin: vec![4; 56],
            salt_p: vec![5; 16],
            w_phrase: vec![6; 56],
            salt_f: vec![7; 16],
            auth_pin_hash: vec![8; 32],
            salt_a: vec![9; 16],
            auth_phrase_hash: vec![10; 32],
            salt_pa: vec![11; 16],
        };
        state.store.upsert_backup(u, &store_upload).await.unwrap();
        let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
        (phone.to_string(), b64(&[8u8; 32]), b64(&[10u8; 32]))
    }

    #[sqlx::test]
    async fn restore_flow_hands_out_the_bundle_only_after_pin_proof(pool: PgPool) {
        let state = state_for(pool);
        let (phone, pin_auth, _) = restore_fixture(&state).await;
        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        // Begin: OTP verify (dev vendor code 000000) → token + salts, and
        // crucially NO bundle field in the response.
        let begin = client
            .post(format!("http://{addr}/v1/backup/restore/begin"))
            .json(&serde_json::json!({ "phone": phone, "code": "000000" }))
            .send()
            .await
            .unwrap();
        assert_eq!(begin.status(), reqwest::StatusCode::OK);
        let begin: serde_json::Value = begin.json().await.unwrap();
        let text = begin.to_string();
        assert!(
            !text.contains("blob") && !text.contains("w_pin"),
            "phone OTP alone must never expose bundle material: {text}"
        );
        let token = begin["restore_token"].as_str().unwrap().to_string();

        // Wrong method string is a 400.
        let bad = client
            .post(format!("http://{addr}/v1/backup/restore/complete"))
            .json(
                &serde_json::json!({ "restore_token": token, "method": "hunch", "auth": pin_auth }),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), reqwest::StatusCode::BAD_REQUEST);

        // Wrong proof → 401 naming remaining attempts; token survives.
        let wrong_auth = base64::engine::general_purpose::STANDARD.encode([0u8; 32]);
        let wrong = client
            .post(format!("http://{addr}/v1/backup/restore/complete"))
            .json(
                &serde_json::json!({ "restore_token": token, "method": "pin", "auth": wrong_auth }),
            )
            .send()
            .await
            .unwrap();
        assert_eq!(wrong.status(), reqwest::StatusCode::UNAUTHORIZED);
        let wrong: serde_json::Value = wrong.json().await.unwrap();
        assert_eq!(wrong["remaining_attempts"], 9);

        // Right proof → full bundle; token is consumed by success.
        let ok = client
            .post(format!("http://{addr}/v1/backup/restore/complete"))
            .json(&serde_json::json!({ "restore_token": token, "method": "pin", "auth": pin_auth }))
            .send()
            .await
            .unwrap();
        assert_eq!(ok.status(), reqwest::StatusCode::OK);
        let bundle: serde_json::Value = ok.json().await.unwrap();
        assert_eq!(
            bundle["blob"],
            base64::engine::general_purpose::STANDARD.encode([1u8, 2, 3])
        );
        let replay = client
            .post(format!("http://{addr}/v1/backup/restore/complete"))
            .json(&serde_json::json!({ "restore_token": token, "method": "pin", "auth": pin_auth }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            replay.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "token is single-use"
        );
    }

    #[sqlx::test]
    async fn restore_locks_the_pin_path_but_never_the_phrase_path(pool: PgPool) {
        let state = state_for(pool);
        let (phone, pin_auth, phrase_auth) = restore_fixture(&state).await;
        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();
        let begin: serde_json::Value = client
            .post(format!("http://{addr}/v1/backup/restore/begin"))
            .json(&serde_json::json!({ "phone": phone, "code": "000000" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let token = begin["restore_token"].as_str().unwrap().to_string();

        let wrong_auth = base64::engine::general_purpose::STANDARD.encode([0u8; 32]);
        let mut last_status = reqwest::StatusCode::OK;
        let mut last_body = serde_json::Value::Null;
        for _ in 0..10 {
            let resp = client
                .post(format!("http://{addr}/v1/backup/restore/complete"))
                .json(&serde_json::json!({ "restore_token": token, "method": "pin", "auth": wrong_auth }))
                .send()
                .await
                .unwrap();
            last_status = resp.status();
            last_body = resp.json().await.unwrap();
        }
        assert_eq!(last_status, reqwest::StatusCode::TOO_MANY_REQUESTS);
        assert!(
            last_body["error"]
                .as_str()
                .unwrap()
                .contains("try again in"),
            "locked response must name the wait: {last_body}"
        );

        // Right PIN while locked: still refused.
        let locked = client
            .post(format!("http://{addr}/v1/backup/restore/complete"))
            .json(&serde_json::json!({ "restore_token": token, "method": "pin", "auth": pin_auth }))
            .send()
            .await
            .unwrap();
        assert_eq!(locked.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);

        // Phrase path sails through the lockout.
        let phrase = client
            .post(format!("http://{addr}/v1/backup/restore/complete"))
            .json(&serde_json::json!({ "restore_token": token, "method": "phrase", "auth": phrase_auth }))
            .send()
            .await
            .unwrap();
        assert_eq!(phrase.status(), reqwest::StatusCode::OK);
    }

    #[sqlx::test]
    async fn restore_begin_404s_without_an_account_or_backup(pool: PgPool) {
        let state = state_for(pool);
        // A user with no backup row.
        let phone = "+15559990002";
        let hash = crate::verify::phone_hash(phone, Pepper(&state.pepper)).unwrap();
        state
            .store
            .find_or_create_pending_user(&hash)
            .await
            .unwrap();
        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        let no_backup = client
            .post(format!("http://{addr}/v1/backup/restore/begin"))
            .json(&serde_json::json!({ "phone": phone, "code": "000000" }))
            .send()
            .await
            .unwrap();
        assert_eq!(no_backup.status(), reqwest::StatusCode::NOT_FOUND);

        let no_account = client
            .post(format!("http://{addr}/v1/backup/restore/begin"))
            .json(&serde_json::json!({ "phone": "+15550000000", "code": "000000" }))
            .send()
            .await
            .unwrap();
        assert_eq!(no_account.status(), reqwest::StatusCode::NOT_FOUND);

        let bad_code = client
            .post(format!("http://{addr}/v1/backup/restore/begin"))
            .json(&serde_json::json!({ "phone": phone, "code": "111111" }))
            .send()
            .await
            .unwrap();
        assert_eq!(bad_code.status(), reqwest::StatusCode::BAD_REQUEST);
    }

    #[sqlx::test]
    async fn pairing_bootstrap_is_consumed_exactly_once(pool: PgPool) {
        let state = state_for(pool);
        let target = state
            .store
            .find_or_create_pending_user("target-hash-0")
            .await
            .unwrap();
        state
            .store
            .claim_username(target, "pairtarget")
            .await
            .unwrap();
        state
            .store
            .set_searchable(target, true, Some(&"a".repeat(64)))
            .await
            .unwrap();
        let target_token = state.store.create_session(target).await.unwrap();

        let requester = state
            .store
            .find_or_create_pending_user("requester-hash-0")
            .await
            .unwrap();
        let requester_token = state.store.create_session(requester).await.unwrap();

        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        // No bootstrap uploaded yet: a request finds nothing.
        let miss = client
            .post(format!(
                "http://{addr}/pairing-bootstrap/request?user_id={target}"
            ))
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
            .post(format!(
                "http://{addr}/pairing-bootstrap/request?user_id={target}"
            ))
            .header("Authorization", format!("Bearer {requester_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(first.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = first.json().await.unwrap();
        assert_eq!(body["contact_link_b64"], "opaque-link-bytes");

        // A second request for the same target finds nothing: one-time use.
        let second = client
            .post(format!(
                "http://{addr}/pairing-bootstrap/request?user_id={target}"
            ))
            .header("Authorization", format!("Bearer {requester_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(second.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn pairing_bootstrap_unreachable_for_a_non_searchable_target(pool: PgPool) {
        let state = state_for(pool);
        let target = state
            .store
            .find_or_create_pending_user("target-hash-1")
            .await
            .unwrap();
        // Never calls set_searchable(true) — stays private.
        state
            .store
            .set_pairing_bootstrap(target, "opaque-link-bytes")
            .await
            .unwrap();

        let requester = state
            .store
            .find_or_create_pending_user("requester-hash-1")
            .await
            .unwrap();
        let requester_token = state.store.create_session(requester).await.unwrap();

        let addr = spawn_for_tests(state).await.unwrap();
        let resp = reqwest::Client::new()
            .post(format!(
                "http://{addr}/pairing-bootstrap/request?user_id={target}"
            ))
            .header("Authorization", format!("Bearer {requester_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn erasing_a_user_deletes_their_pairing_bootstrap(pool: PgPool) {
        let state = state_for(pool);
        let target = state
            .store
            .find_or_create_pending_user("target-hash-2")
            .await
            .unwrap();
        state
            .store
            .set_searchable(target, true, Some(&"a".repeat(64)))
            .await
            .unwrap();
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
            .find_or_create_pending_user(
                "erase-me-hash-000000000000000000000000000000000000000000000000",
            )
            .await
            .unwrap();
        state.store.claim_username(user_id, "temp").await.unwrap();
        let search_hash = format!("erase0{}", "0".repeat(58));
        state
            .store
            .set_searchable(user_id, true, Some(&search_hash))
            .await
            .unwrap();
        let token = state.store.create_session(user_id).await.unwrap();

        let state_ref = state.clone(); // the store outlives the move into the server
        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();
        let resp = client
            .delete(format!("http://{addr}/account"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);

        // ET2: the token that just erased the account must stop working. Until
        // ET2 it kept authenticating against a dangling `user_id` — the rows were
        // gone and the session was not, so an erased account's bearer token was
        // still a valid caller.
        let after_erase = client
            .get(format!("http://{addr}/search?prefix=erase"))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(
            after_erase.status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "erasure must revoke the session, not just the rows"
        );

        // ...and the account is gone from search. Checked with a *different*,
        // live caller: asserting this with the erased user's own token (as this
        // test used to) now only proves the 401 above, not the removal.
        let onlooker = state_ref
            .store
            .find_or_create_pending_user("onlooker-hash-000000000000000000000000000000000000000000")
            .await
            .unwrap();
        let onlooker_token = state_ref.store.create_session(onlooker).await.unwrap();
        let search = client
            .get(format!("http://{addr}/search?prefix=erase"))
            .header("Authorization", format!("Bearer {onlooker_token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(search.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = search.json().await.unwrap();
        assert_eq!(body["results"].as_array().unwrap().len(), 0);
    }

    #[sqlx::test]
    async fn search_response_carries_the_search_hash_but_never_the_keyed_auth_one(pool: PgPool) {
        // T8, refined: the response type still has no field that could hold
        // the KEYED auth phone_hash (`find_or_create_pending_user`'s argument below
        // never appears). It DOES now carry `search_hash` — the separate,
        // unkeyed value the client needs to do the final exact match
        // locally (HIBP-style); that's the endpoint's whole point, not a
        // leak of the thing T8 originally protected.
        let state = state_for(pool);
        let user_id = state
            .store
            .find_or_create_pending_user("aaaaaverysecretphonehash")
            .await
            .unwrap();
        state
            .store
            .claim_username(user_id, "findable")
            .await
            .unwrap();
        let search_hash = format!("aaaaa{}", "0".repeat(59));
        state
            .store
            .set_searchable(user_id, true, Some(&search_hash))
            .await
            .unwrap();
        let token = state.store.create_session(user_id).await.unwrap();

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
        assert_eq!(
            body["results"][0]["search_hash"], search_hash,
            "client needs the full search hash back to do the exact match itself"
        );
    }

    #[sqlx::test]
    async fn username_lookup_finds_a_claimed_handle_and_404s_for_an_unknown_one(pool: PgPool) {
        let state = state_for(pool);
        let target = state
            .store
            .find_or_create_pending_user("lookup-hash")
            .await
            .unwrap();
        let (slot, _width) = state
            .store
            .claim_username(target, "findbyname")
            .await
            .unwrap();
        let requester = state
            .store
            .find_or_create_pending_user("requester-hash")
            .await
            .unwrap();
        let token = state.store.create_session(requester).await.unwrap();

        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        let hit = client
            .get(format!(
                "http://{addr}/username-lookup?nickname=findbyname&discriminator={slot}"
            ))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(hit.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = hit.json().await.unwrap();
        assert_eq!(body["user_id"], target);

        let miss = client
            .get(format!(
                "http://{addr}/username-lookup?nickname=findbyname&discriminator={}",
                slot + 1
            ))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .unwrap();
        assert_eq!(miss.status(), reqwest::StatusCode::NOT_FOUND);
    }

    #[sqlx::test]
    async fn set_searchable_rejects_missing_or_malformed_hash(pool: PgPool) {
        let state = state_for(pool);
        let user_id = state.store.find_or_create_pending_user("h").await.unwrap();
        let token = state.store.create_session(user_id).await.unwrap();
        let addr = spawn_for_tests(state).await.unwrap();
        let client = reqwest::Client::new();

        let missing = client
            .post(format!("http://{addr}/searchable"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({ "searchable": true }))
            .send()
            .await
            .unwrap();
        assert_eq!(missing.status(), reqwest::StatusCode::BAD_REQUEST);

        let too_short = client
            .post(format!("http://{addr}/searchable"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({ "searchable": true, "phone_search_hash": "abc" }))
            .send()
            .await
            .unwrap();
        assert_eq!(too_short.status(), reqwest::StatusCode::BAD_REQUEST);

        let not_hex = client
            .post(format!("http://{addr}/searchable"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({ "searchable": true, "phone_search_hash": "z".repeat(64) }))
            .send()
            .await
            .unwrap();
        assert_eq!(not_hex.status(), reqwest::StatusCode::BAD_REQUEST);

        // Turning it off never needs a hash.
        let off = client
            .post(format!("http://{addr}/searchable"))
            .header("Authorization", format!("Bearer {token}"))
            .json(&serde_json::json!({ "searchable": false }))
            .send()
            .await
            .unwrap();
        assert_eq!(off.status(), reqwest::StatusCode::NO_CONTENT);
    }
}
