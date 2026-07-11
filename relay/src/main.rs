use std::sync::Arc;

use relay::RelayState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let addr = std::env::args().nth(1).unwrap_or_else(|| "127.0.0.1:7443".to_string());
    let state = Arc::new(RelayState::new());
    relay::net::serve(&addr, state).await
}
