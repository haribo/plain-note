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
    ClientMsg, CreateGroupResponse, EnrollRequest, EnrollResponse, PROTOCOL_VERSION, ServerMsg,
};
use note_relay::build_app;
use note_relay::state::AppState;
use note_relay::storage::{InMemoryStorage, Storage};
use rand::RngCore;
use rand::rngs::OsRng;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tower::ServiceExt;

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

fn signing_key() -> SigningKey {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
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
    ws.send(Message::Text(m.to_json())).await.unwrap();
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
