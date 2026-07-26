//! Client side of the sync protocol.
//!
//! Extracts Automerge changes from a [`NoteStore`], encrypts each into an
//! envelope with [`crate::crypto`], and exchanges them with the relay over a
//! WebSocket. Incoming changes are decrypted and applied; CRDT semantics make
//! the merge automatic and order-tolerant.
//!
//! [`sync_once`] performs one round trip (push local, pull remote) and returns
//! the new high-water `seq` the caller should persist. Encryption/decryption
//! happen only here and in `crypto`; the relay sees only ciphertext.
//!
//! The wire protocol is specified in `docs/sync-protocol.md`.

use automerge::ChangeHash;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signer, SigningKey};
use futures_util::{SinkExt, StreamExt};
use note_protocol::{ClientMsg, PROTOCOL_VERSION, ServerMsg};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};

use crate::crypto::{self, Aad, GroupKey, Kind};
use crate::model::{ModelError, NoteStore};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Everything a client needs to sync one group through one relay.
pub struct SyncConfig {
    /// WebSocket URL of the relay's `/v1/sync` endpoint.
    pub relay_url: String,
    /// Hex group id, as returned by enrollment.
    pub group_id: String,
    /// Hex device id, as returned by enrollment.
    pub device_id: String,
    /// This device's Ed25519 signing key (transport auth; unrelated to E2E).
    pub signing_key: SigningKey,
    /// The group's shared E2E key (content encryption).
    pub e2e_key: GroupKey,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("protocol violation: {0}")]
    Protocol(String),
    #[error("authentication rejected: {0}")]
    Auth(String),
    #[error("relay error [{code}]: {message}")]
    Relay { code: String, message: String },
    #[error(transparent)]
    Crypto(#[from] crypto::CryptoError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("invalid id encoding")]
    BadId,
}

/// Run one sync round trip: push local changes, pull everything with
/// `seq > since_seq`, and return the new high-water `seq` to persist.
pub async fn sync_once(
    config: &SyncConfig,
    store: &mut NoteStore,
    since_seq: u64,
) -> Result<u64, SyncError> {
    let mut ws = connect_and_auth(config).await?;

    let group = id_bytes(&config.group_id)?;
    let own_device = id_bytes(&config.device_id)?;

    // Push every local change (relay dedups by change id, so re-pushes are safe).
    for (hash, bytes) in store.raw_changes() {
        let aad = Aad {
            group_id: group,
            device_id: own_device,
            kind: Kind::Change,
        };
        let envelope = B64.encode(crypto::seal(&config.e2e_key, aad, &bytes));
        send(
            &mut ws,
            ClientMsg::Push {
                envelope,
                client_change_id: hash_hex(&hash),
            },
        )
        .await?;
    }

    // Then pull and drain until caught up.
    send(&mut ws, ClientMsg::Pull { since_seq }).await?;

    let mut last_seq = since_seq;
    loop {
        match recv(&mut ws).await? {
            ServerMsg::Change {
                seq,
                device_id,
                envelope,
            } => {
                apply_change(config, store, &group, &device_id, &envelope)?;
                last_seq = last_seq.max(seq);
            }
            ServerMsg::PullDone { seq } => {
                last_seq = last_seq.max(seq);
                break;
            }
            ServerMsg::Ack { .. } | ServerMsg::Pong => {}
            ServerMsg::Error { code, message } => return Err(SyncError::Relay { code, message }),
            ServerMsg::Challenge { .. } | ServerMsg::AuthOk { .. } => {
                return Err(SyncError::Protocol("unexpected handshake message".into()));
            }
        }
    }
    Ok(last_seq)
}

async fn connect_and_auth(config: &SyncConfig) -> Result<Ws, SyncError> {
    let (mut ws, _) = connect_async(&config.relay_url)
        .await
        .map_err(|e| SyncError::Connection(e.to_string()))?;

    send(
        &mut ws,
        ClientMsg::Hello {
            protocol_version: PROTOCOL_VERSION,
        },
    )
    .await?;

    let challenge = match recv(&mut ws).await? {
        ServerMsg::Challenge { challenge } => B64
            .decode(challenge.as_bytes())
            .map_err(|_| SyncError::Protocol("challenge not base64".into()))?,
        ServerMsg::Error { code, message } => return Err(SyncError::Relay { code, message }),
        other => {
            return Err(SyncError::Protocol(format!(
                "expected challenge, got {other:?}"
            )));
        }
    };

    let signature = B64.encode(config.signing_key.sign(&challenge).to_bytes());
    send(
        &mut ws,
        ClientMsg::Auth {
            device_id: config.device_id.clone(),
            signature,
        },
    )
    .await?;

    match recv(&mut ws).await? {
        ServerMsg::AuthOk { .. } => Ok(ws),
        ServerMsg::Error { message, .. } => Err(SyncError::Auth(message)),
        other => Err(SyncError::Protocol(format!(
            "expected auth_ok, got {other:?}"
        ))),
    }
}

fn apply_change(
    config: &SyncConfig,
    store: &mut NoteStore,
    group: &[u8; 16],
    origin_device_hex: &str,
    envelope_b64: &str,
) -> Result<(), SyncError> {
    let origin = id_bytes(origin_device_hex)?;
    let aad = Aad {
        group_id: *group,
        device_id: origin,
        kind: Kind::Change,
    };
    let envelope = B64
        .decode(envelope_b64.as_bytes())
        .map_err(|_| SyncError::Protocol("envelope not base64".into()))?;
    let bytes = crypto::open(&config.e2e_key, aad, &envelope)?;
    store.apply_change_bytes(bytes)?;
    Ok(())
}

fn id_bytes(hex_str: &str) -> Result<[u8; 16], SyncError> {
    let v = hex::decode(hex_str).map_err(|_| SyncError::BadId)?;
    v.try_into().map_err(|_| SyncError::BadId)
}

fn hash_hex(hash: &ChangeHash) -> String {
    hex::encode(hash.0)
}

async fn send(ws: &mut Ws, msg: ClientMsg) -> Result<(), SyncError> {
    ws.send(Message::Text(msg.to_json()))
        .await
        .map_err(|e| SyncError::Connection(e.to_string()))
}

async fn recv(ws: &mut Ws) -> Result<ServerMsg, SyncError> {
    while let Some(msg) = ws.next().await {
        let msg = msg.map_err(|e| SyncError::Connection(e.to_string()))?;
        if let Ok(text) = msg.to_text()
            && !text.is_empty()
        {
            return serde_json::from_str(text).map_err(|e| SyncError::Protocol(e.to_string()));
        }
    }
    Err(SyncError::Connection("connection closed".into()))
}
