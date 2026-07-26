//! HTTP endpoints: device enrollment and token-gated admin operations.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use note_protocol::{
    CreateGroupResponse, CreateInviteRequest, CreateInviteResponse, EnrollRequest, EnrollResponse,
};

use crate::state::AppState;

type ApiError = (StatusCode, String);

/// `POST /v1/enroll` — consume an invite and register a device's public key.
pub async fn enroll(
    State(state): State<Arc<AppState>>,
    Json(req): Json<EnrollRequest>,
) -> Result<Json<EnrollResponse>, ApiError> {
    let pubkey = B64
        .decode(req.device_pubkey.as_bytes())
        .map_err(|_| (StatusCode::BAD_REQUEST, "invalid device_pubkey".to_string()))?;
    let (device_id, group_id) = state
        .storage
        .enroll(&req.invite_code, pubkey)
        .map_err(|e| (StatusCode::FORBIDDEN, e.to_string()))?;
    Ok(Json(EnrollResponse {
        device_id,
        group_id,
    }))
}

/// `POST /v1/groups` — create a group and its first invite (admin).
pub async fn create_group(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<CreateGroupResponse>, ApiError> {
    check_admin(&state, &headers)?;
    let (group_id, invite_code) = state.storage.create_group();
    Ok(Json(CreateGroupResponse {
        group_id,
        invite_code,
    }))
}

/// `POST /v1/invites` — mint another invite for an existing group (admin).
pub async fn create_invite(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateInviteRequest>,
) -> Result<Json<CreateInviteResponse>, ApiError> {
    check_admin(&state, &headers)?;
    let invite_code = state
        .storage
        .create_invite(&req.group_id)
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(Json(CreateInviteResponse { invite_code }))
}

fn check_admin(state: &AppState, headers: &HeaderMap) -> Result<(), ApiError> {
    let token = state.admin_token.as_ref().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "admin operations are disabled".to_string(),
    ))?;
    let provided = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided == format!("Bearer {token}") {
        Ok(())
    } else {
        Err((StatusCode::UNAUTHORIZED, "invalid admin token".to_string()))
    }
}
