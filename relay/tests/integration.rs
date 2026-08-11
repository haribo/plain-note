//! Relay integration tests: HTTP enrollment (via oneshot) and the WebSocket
//! sync path (over a real spawned server) — auth, push/pull, and live broadcast.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use note_protocol::{
    ClientMsg, CreateGroupResponse, CreateInviteRequest, DeviceListResponse, EnrollRequest,
    EnrollResponse, PROTOCOL_VERSION, ServerMsg,
};
use note_relay::build_app;
use note_relay::state::AppState;
use note_relay::storage::{InMemoryStorage, SqliteStorage, Storage};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tower::ServiceExt;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn signing_key() -> SigningKey {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).expect("OS RNG unavailable");
    SigningKey::from_bytes(&seed)
}

async fn spawn_server(storage: Arc<InMemoryStorage>, admin: Option<String>) -> SocketAddr {
    let state = AppState::new(storage, admin);
    let app = build_app(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    addr
}

async fn send(ws: &mut Ws, m: ClientMsg) {
    ws.send(Message::Text(m.to_json().into())).await.unwrap();
}

async fn recv(ws: &mut Ws) -> ServerMsg {
    loop {
        let msg = ws.next().await.expect("stream ended").expect("ws error");
        if let Ok(t) = msg.to_text()
            && !t.is_empty()
        {
            return serde_json::from_str(t).unwrap();
        }
    }
}

/// Perform the Hello/Challenge/Auth handshake and return the open socket.
async fn connect_auth(url: &str, device_id: &str, sk: &SigningKey) -> Ws {
    let (mut ws, _) = connect_async(url).await.unwrap();
    send(
        &mut ws,
        ClientMsg::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await;
    let challenge = match recv(&mut ws).await {
        ServerMsg::Challenge { challenge } => B64.decode(challenge).unwrap(),
        other => panic!("expected challenge, got {other:?}"),
    };
    let signature = B64.encode(sk.sign(&challenge).to_bytes());
    send(
        &mut ws,
        ClientMsg::Auth {
            device_id: device_id.to_string(),
            signature,
        },
    )
    .await;
    match recv(&mut ws).await {
        ServerMsg::AuthOk { .. } => {}
        other => panic!("expected auth_ok, got {other:?}"),
    }
    ws
}

#[tokio::test]
async fn sync_between_two_devices() {
    let storage = Arc::new(InMemoryStorage::new());
    let (group, code1) = storage.create_group();
    let sk1 = signing_key();
    let (dev1, _) = storage
        .enroll(&code1, sk1.verifying_key().to_bytes().to_vec())
        .unwrap();
    let code2 = storage.create_invite(&group).unwrap();
    let sk2 = signing_key();
    let (dev2, _) = storage
        .enroll(&code2, sk2.verifying_key().to_bytes().to_vec())
        .unwrap();

    let addr = spawn_server(storage, None).await;
    let url = format!("ws://{addr}/v1/sync");

    // Device 1 pushes an (opaque) envelope.
    let mut ws1 = connect_auth(&url, &dev1, &sk1).await;
    send(
        &mut ws1,
        ClientMsg::Push {
            envelope: "ENVELOPE-1".to_string(),
            client_change_id: "c1".to_string(),
        },
    )
    .await;
    match recv(&mut ws1).await {
        ServerMsg::Ack {
            client_change_id,
            seq,
        } => {
            assert_eq!(client_change_id, "c1");
            assert_eq!(seq, 1);
        }
        other => panic!("expected ack, got {other:?}"),
    }

    // Device 2 pulls from scratch and sees it.
    let mut ws2 = connect_auth(&url, &dev2, &sk2).await;
    send(&mut ws2, ClientMsg::Pull { since_seq: 0 }).await;
    match recv(&mut ws2).await {
        ServerMsg::Change {
            seq,
            device_id,
            envelope,
        } => {
            assert_eq!(seq, 1);
            assert_eq!(device_id, dev1);
            assert_eq!(envelope, "ENVELOPE-1");
        }
        other => panic!("expected change, got {other:?}"),
    }
    match recv(&mut ws2).await {
        ServerMsg::PullDone { seq } => assert_eq!(seq, 1),
        other => panic!("expected pull_done, got {other:?}"),
    }
}

#[tokio::test]
async fn live_broadcast_to_connected_peer() {
    let storage = Arc::new(InMemoryStorage::new());
    let (group, code1) = storage.create_group();
    let sk1 = signing_key();
    let (dev1, _) = storage
        .enroll(&code1, sk1.verifying_key().to_bytes().to_vec())
        .unwrap();
    let code2 = storage.create_invite(&group).unwrap();
    let sk2 = signing_key();
    let (dev2, _) = storage
        .enroll(&code2, sk2.verifying_key().to_bytes().to_vec())
        .unwrap();

    let addr = spawn_server(storage, None).await;
    let url = format!("ws://{addr}/v1/sync");

    let mut ws2 = connect_auth(&url, &dev2, &sk2).await;
    let mut ws1 = connect_auth(&url, &dev1, &sk1).await;

    // ws1 pushes; ws2, already connected, receives it live via broadcast.
    send(
        &mut ws1,
        ClientMsg::Push {
            envelope: "LIVE".to_string(),
            client_change_id: "cx".to_string(),
        },
    )
    .await;

    match recv(&mut ws2).await {
        ServerMsg::Change {
            device_id,
            envelope,
            ..
        } => {
            assert_eq!(device_id, dev1);
            assert_eq!(envelope, "LIVE");
        }
        other => panic!("expected broadcast change, got {other:?}"),
    }
}

#[tokio::test]
async fn auth_rejects_bad_signature() {
    let storage = Arc::new(InMemoryStorage::new());
    let (_group, code) = storage.create_group();
    let sk = signing_key();
    let (dev, _) = storage
        .enroll(&code, sk.verifying_key().to_bytes().to_vec())
        .unwrap();

    let addr = spawn_server(storage, None).await;
    let url = format!("ws://{addr}/v1/sync");

    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(
        &mut ws,
        ClientMsg::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await;
    let _ = recv(&mut ws).await; // challenge
    // Sign the wrong bytes.
    let bogus = B64.encode(sk.sign(b"not the challenge").to_bytes());
    send(
        &mut ws,
        ClientMsg::Auth {
            device_id: dev,
            signature: bogus,
        },
    )
    .await;
    match recv(&mut ws).await {
        ServerMsg::Error { code, .. } => assert_eq!(code, "unauthorized"),
        other => panic!("expected unauthorized error, got {other:?}"),
    }
}

#[tokio::test]
async fn http_enroll_flow() {
    let storage = Arc::new(InMemoryStorage::new());
    let state = AppState::new(storage, Some("secret-admin".to_string()));
    let app = build_app(state);

    // Admin creates a group + invite.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/groups")
                .header(header::AUTHORIZATION, "Bearer secret-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let group: CreateGroupResponse = serde_json::from_slice(&body).unwrap();

    // A device enrolls with the invite.
    let sk = signing_key();
    let enroll = EnrollRequest {
        invite_code: group.invite_code,
        device_pubkey: B64.encode(sk.verifying_key().to_bytes()),
    };
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/enroll")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&enroll).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let enrolled: EnrollResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(enrolled.group_id, group.group_id);
    assert!(!enrolled.device_id.is_empty());
}

#[tokio::test]
async fn admin_requires_token() {
    let storage = Arc::new(InMemoryStorage::new());
    let state = AppState::new(storage, Some("secret-admin".to_string()));
    let app = build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/groups")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn revocation_blocks_a_device() {
    let storage = Arc::new(InMemoryStorage::new());
    let (group, code) = storage.create_group();
    let sk = signing_key();
    let (dev, _) = storage
        .enroll(&code, sk.verifying_key().to_bytes().to_vec())
        .unwrap();

    let state = AppState::new(storage, Some("adm".to_string()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let serve_state = state.clone();
    tokio::spawn(async move { note_relay::serve(listener, serve_state).await.unwrap() });
    let app = build_app(state);
    let url = format!("ws://{addr}/v1/sync");

    // The device is listed and can authenticate.
    assert_eq!(device_count(&app, &group).await, 1);
    let ws = connect_auth(&url, &dev, &sk).await;
    drop(ws);

    // Revoke it.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/devices/{dev}"))
                .header(header::AUTHORIZATION, "Bearer adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    assert_eq!(device_count(&app, &group).await, 0);

    // It can no longer authenticate.
    let (mut ws, _) = connect_async(&url).await.unwrap();
    send(
        &mut ws,
        ClientMsg::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await;
    let challenge = match recv(&mut ws).await {
        ServerMsg::Challenge { challenge } => B64.decode(challenge).unwrap(),
        other => panic!("expected challenge, got {other:?}"),
    };
    send(
        &mut ws,
        ClientMsg::Auth {
            device_id: dev,
            signature: B64.encode(sk.sign(&challenge).to_bytes()),
        },
    )
    .await;
    match recv(&mut ws).await {
        ServerMsg::Error { code, .. } => assert_eq!(code, "unauthorized"),
        other => panic!("expected unauthorized, got {other:?}"),
    }
}

async fn device_count(app: &axum::Router, group: &str) -> usize {
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/devices?group_id={group}"))
                .header(header::AUTHORIZATION, "Bearer adm")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let list: DeviceListResponse = serde_json::from_slice(&body).unwrap();
    list.devices.len()
}

#[tokio::test]
async fn attachment_upload_download_and_isolation() {
    use sha2::{Digest, Sha256};

    let storage = Arc::new(InMemoryStorage::new());
    let (_g1, code1) = storage.create_group();
    let (dev1, _) = storage.enroll(&code1, vec![1]).unwrap();
    let token1 = storage.issue_token(&dev1);
    // A second, separate group + device.
    let (_g2, code2) = storage.create_group();
    let (dev2, _) = storage.enroll(&code2, vec![2]).unwrap();
    let token2 = storage.issue_token(&dev2);

    let app = build_app(AppState::new(storage, None));

    let blob = b"encrypted-attachment-bytes".to_vec();
    let id = hex::encode(Sha256::digest(&blob));

    // Upload with a valid token.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/attachments/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token1}"))
                .body(Body::from(blob.clone()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Download round-trips the exact bytes.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/attachments/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token1}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let got = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(got.to_vec(), blob);

    // Missing/invalid token is rejected.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/attachments/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // A device in another group cannot see it.
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/attachments/{id}"))
                .header(header::AUTHORIZATION, format!("Bearer {token2}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Id that does not match the body hash is rejected.
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/attachments/deadbeef")
                .header(header::AUTHORIZATION, format!("Bearer {token1}"))
                .body(Body::from(blob))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

// --- negative / attack paths ---

#[tokio::test]
async fn ws_rejects_protocol_version_mismatch() {
    let storage = Arc::new(InMemoryStorage::new());
    let addr = spawn_server(storage, None).await;
    let url = format!("ws://{addr}/v1/sync");
    let (mut ws, _) = connect_async(url.as_str()).await.unwrap();
    send(
        &mut ws,
        ClientMsg::Hello {
            protocol_version: PROTOCOL_VERSION + 99,
        },
    )
    .await;
    match recv(&mut ws).await {
        ServerMsg::Error { code, .. } => assert_eq!(code, "protocol"),
        other => panic!("expected error, got {other:?}"),
    }
}

#[tokio::test]
async fn ws_rejects_unknown_device() {
    let storage = Arc::new(InMemoryStorage::new());
    let addr = spawn_server(storage, None).await;
    let url = format!("ws://{addr}/v1/sync");
    let (mut ws, _) = connect_async(url.as_str()).await.unwrap();
    send(
        &mut ws,
        ClientMsg::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await;
    let _ = recv(&mut ws).await; // Challenge
    send(
        &mut ws,
        ClientMsg::Auth {
            device_id: "00".repeat(16),
            signature: B64.encode([0u8; 64]),
        },
    )
    .await;
    match recv(&mut ws).await {
        ServerMsg::Error { code, .. } => assert_eq!(code, "unauthorized"),
        other => panic!("expected error, got {other:?}"),
    }
}

fn admin_app(admin: &str) -> axum::Router {
    let storage = Arc::new(InMemoryStorage::new());
    build_app(AppState::new(storage, Some(admin.to_string())))
}

#[tokio::test]
async fn enroll_rejects_bad_invite() {
    let app = admin_app("secret-admin");
    let sk = signing_key();
    let enroll = EnrollRequest {
        invite_code: "not-a-real-invite".into(),
        device_pubkey: B64.encode(sk.verifying_key().to_bytes()),
    };
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/enroll")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&enroll).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn create_invite_rejects_missing_group() {
    let app = admin_app("secret-admin");
    let body = serde_json::to_vec(&CreateInviteRequest {
        group_id: "deadbeefdeadbeefdeadbeefdeadbeef".into(),
    })
    .unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/invites")
                .header(header::AUTHORIZATION, "Bearer secret-admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn revoke_rejects_unknown_device() {
    let app = admin_app("secret-admin");
    let resp = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/devices/deadbeefdeadbeefdeadbeefdeadbeef")
                .header(header::AUTHORIZATION, "Bearer secret-admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn admin_rejects_wrong_token() {
    let app = admin_app("secret-admin");
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/groups")
                .header(header::AUTHORIZATION, "Bearer WRONG-TOKEN")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sync_survives_relay_restart_with_sqlite() {
    let mut path = std::env::temp_dir();
    path.push(format!("pn-relay-e2e-{}.db", std::process::id()));
    let p = path.to_str().unwrap().to_string();
    let _ = std::fs::remove_file(&p);

    // Provision two devices on a SQLite-backed store.
    let storage = Arc::new(SqliteStorage::open(&p).unwrap());
    let (group, code1) = storage.create_group();
    let sk1 = signing_key();
    let (dev1, _) = storage
        .enroll(&code1, sk1.verifying_key().to_bytes().to_vec())
        .unwrap();
    let code2 = storage.create_invite(&group).unwrap();
    let sk2 = signing_key();
    let (dev2, _) = storage
        .enroll(&code2, sk2.verifying_key().to_bytes().to_vec())
        .unwrap();

    // First relay incarnation: device 1 pushes a change (durably stored).
    let app = build_app(AppState::new(storage.clone(), None));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let url = format!("ws://{addr}/v1/sync");
    let mut ws1 = connect_auth(&url, &dev1, &sk1).await;
    send(
        &mut ws1,
        ClientMsg::Push {
            envelope: "DURABLE".to_string(),
            client_change_id: "c1".to_string(),
        },
    )
    .await;
    match recv(&mut ws1).await {
        ServerMsg::Ack { seq, .. } => assert_eq!(seq, 1),
        other => panic!("expected ack, got {other:?}"),
    }

    // Restart: drop the socket, stop the server, drop the storage handle.
    drop(ws1);
    handle.abort();
    drop(storage);

    // Reopen the same DB file in a fresh relay incarnation.
    let storage2 = Arc::new(SqliteStorage::open(&p).unwrap());
    let app2 = build_app(AppState::new(storage2, None));
    let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr2 = listener2.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener2, app2).await.unwrap() });
    let url2 = format!("ws://{addr2}/v1/sync");

    // Device 2 pulls from the restarted relay and sees the persisted change.
    let mut ws2 = connect_auth(&url2, &dev2, &sk2).await;
    send(&mut ws2, ClientMsg::Pull { since_seq: 0 }).await;
    match recv(&mut ws2).await {
        ServerMsg::Change {
            seq,
            device_id,
            envelope,
        } => {
            assert_eq!(seq, 1);
            assert_eq!(device_id, dev1);
            assert_eq!(envelope, "DURABLE");
        }
        other => panic!("expected change, got {other:?}"),
    }

    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{p}{suffix}"));
    }
}

#[tokio::test]
async fn revoking_a_device_ends_its_live_session() {
    let storage = Arc::new(InMemoryStorage::new());
    let (_group, code) = storage.create_group();
    let sk = signing_key();
    let (dev, _) = storage
        .enroll(&code, sk.verifying_key().to_bytes().to_vec())
        .unwrap();
    let addr = spawn_server(storage.clone(), None).await;
    let url = format!("ws://{addr}/v1/sync");

    let mut ws = connect_auth(&url, &dev, &sk).await;
    // Revoke the device while its session is still open.
    assert!(storage.revoke_device(&dev));
    // The next operation on the live socket must be rejected.
    send(&mut ws, ClientMsg::Pull { since_seq: 0 }).await;
    match recv(&mut ws).await {
        ServerMsg::Error { code, .. } => assert_eq!(code, "unauthorized"),
        other => panic!("expected unauthorized after revoke, got {other:?}"),
    }
}
