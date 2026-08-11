//! note-relay — zero-knowledge relay server.
//!
//! Handles device authentication (revocable Ed25519 credentials), routing of
//! encrypted changes within a sync group, and storage of opaque encrypted
//! envelopes. It never performs crypto on note contents and can never read
//! them. Multi-tenant: several sync groups share one relay, isolated by
//! `group_id`.
//!
//! The library exposes [`build_app`] and [`state::AppState`] so tests and
//! embedders can run the server in-process; `main.rs` is a thin wrapper.

pub mod routes;
pub mod state;
pub mod storage;
pub mod ws;

use std::sync::Arc;

use axum::Router;
use axum::routing::{delete, get, post, put};
use tokio::net::TcpListener;

use crate::state::AppState;

/// Build the HTTP + WebSocket router bound to the given state.
pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/v1/enroll", post(routes::enroll))
        .route("/v1/groups", post(routes::create_group))
        .route("/v1/invites", post(routes::create_invite))
        .route("/v1/devices", get(routes::list_devices))
        .route("/v1/devices/:id", delete(routes::revoke_device))
        .route(
            "/v1/attachments/:id",
            put(routes::put_attachment).get(routes::get_attachment),
        )
        .route("/v1/sync", get(ws::ws_handler))
        .with_state(state)
}

/// Serve the relay on an already-bound listener. Convenience for embedders and
/// tests that need the server running in-process.
pub async fn serve(listener: TcpListener, state: Arc<AppState>) -> std::io::Result<()> {
    axum::serve(listener, build_app(state)).await
}
