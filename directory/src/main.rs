use std::sync::Arc;

use directory::{AppState, DevOtpVendor, DirectoryConfig, DirectoryStore, RateLimiter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let addr = std::env::var("DIRECTORY_ADDR").unwrap_or_else(|_| "127.0.0.1:7444".to_string());
    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| anyhow::anyhow!("DATABASE_URL must be set (postgres://...)"))?;

    // OQ4: the pepper belongs in a secrets manager/KMS, never the DB next
    // to what it protects. No KMS integration exists yet (T2's own note) —
    // this reads an env var and loudly refuses a default in anything that
    // looks like production, rather than silently using a known value.
    let pepper = match std::env::var("DIRECTORY_PEPPER") {
        Ok(p) => p.into_bytes(),
        Err(_) if std::env::var("DIRECTORY_ALLOW_DEV_PEPPER").is_ok() => {
            tracing::warn!("DIRECTORY_PEPPER unset — using a fixed dev-only value. Never do this in production.");
            b"dev-only-pepper-do-not-use-in-production".to_vec()
        }
        Err(_) => {
            anyhow::bail!(
                "DIRECTORY_PEPPER must be set (from a secrets manager/KMS in production). \
                 Set DIRECTORY_ALLOW_DEV_PEPPER=1 to run with an insecure dev default instead."
            )
        }
    };

    let store = Arc::new(DirectoryStore::connect(&database_url).await?);
    let state = Arc::new(AppState {
        store,
        vendor: Arc::new(DevOtpVendor),
        pepper,
        config: DirectoryConfig::from_env(),
        rate_limiter: RateLimiter::new(),
    });

    directory::bind_and_serve(&addr, state).await
}
