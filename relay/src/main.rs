//! note-relay binary — a thin wrapper around the library.

use std::env;
use std::sync::Arc;

use anyhow::Result;
use note_relay::build_app;
use note_relay::state::AppState;
use note_relay::storage::InMemoryStorage;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let bind = env::var("PN_RELAY_BIND").unwrap_or_else(|_| "127.0.0.1:8787".to_string());
    let admin_token = env::var("PN_RELAY_ADMIN_TOKEN").ok();
    if admin_token.is_none() {
        tracing::warn!("PN_RELAY_ADMIN_TOKEN unset — admin endpoints are disabled");
    }

    let storage = Arc::new(InMemoryStorage::new());
    let state = AppState::new(storage, admin_token);

    let listener = TcpListener::bind(&bind).await?;
    tracing::info!(
        "note-relay {} listening on {bind}",
        env!("CARGO_PKG_VERSION")
    );
    axum::serve(listener, build_app(state)).await?;
    Ok(())
}
