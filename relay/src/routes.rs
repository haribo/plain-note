//! HTTP endpoints: device enrollment and token-gated admin operations.

use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header::AUTHORIZATION, header::CONTENT_TYPE};
use axum::response::IntoResponse;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use note_protocol::{
    CreateGroupResponse, CreateInviteRequest, CreateInviteResponse, DeviceInfo, DeviceListResponse,
    EnrollRequest, EnrollResponse,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::state::AppState;
use crate::storage::Device;

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
pub struct GroupQuery {
    group_id: String,
}

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
    let device_token = state.storage.issue_token(&device_id);
    Ok(Json(EnrollResponse {
        device_id,
        group_id,
        device_token,
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

/// `GET /v1/devices?group_id=` — list a group's devices (admin).
pub async fn list_devices(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<GroupQuery>,
) -> Result<Json<DeviceListResponse>, ApiError> {
    check_admin(&state, &headers)?;
    let devices = state
        .storage
        .list_devices(&q.group_id)
        .into_iter()
        .map(|d| DeviceInfo { id: d.id })
        .collect();
    Ok(Json(DeviceListResponse { devices }))
}

/// `DELETE /v1/devices/{id}` — revoke a device (admin).
pub async fn revoke_device(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(device_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    check_admin(&state, &headers)?;
    if state.storage.revoke_device(&device_id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err((StatusCode::NOT_FOUND, "unknown device".to_string()))
    }
}

/// `PUT /v1/attachments/{id}` — store an encrypted blob (device bearer auth).
pub async fn put_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let device = authed_device(&state, &headers)?;
    let computed = hex::encode(Sha256::digest(&body));
    if computed != id {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "attachment id does not match the body hash".to_string(),
        ));
    }
    let created = state
        .storage
        .put_attachment(&device.group_id, &id, body.to_vec());
    Ok(if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    })
}

/// `GET /v1/attachments/{id}` — fetch an encrypted blob (device bearer auth).
pub async fn get_attachment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let device = authed_device(&state, &headers)?;
    match state.storage.get_attachment(&device.group_id, &id) {
        Some(blob) => Ok(([(CONTENT_TYPE, "application/octet-stream")], blob)),
        None => Err((StatusCode::NOT_FOUND, "attachment not found".to_string())),
    }
}

fn authed_device(state: &AppState, headers: &HeaderMap) -> Result<Device, ApiError> {
    let token = headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or((StatusCode::UNAUTHORIZED, "missing bearer token".to_string()))?;
    state
        .storage
        .device_by_token(token)
        .ok_or((StatusCode::UNAUTHORIZED, "invalid device token".to_string()))
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
