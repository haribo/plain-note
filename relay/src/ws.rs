//! WebSocket sync endpoint (`/v1/sync`).
//!
//! Flow: `Hello` → `Challenge` → `Auth` (Ed25519 signature over the challenge)
//! → `AuthOk`, then a loop handling `Push` / `Pull` / `Ping` while forwarding
//! other members' changes from the group broadcast channel. The relay handles
//! only ciphertext envelopes; it verifies signatures for auth but never reads
//! note content.

use std::sync::Arc;

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use note_protocol::{ClientMsg, PROTOCOL_VERSION, ServerMsg};
use tokio::sync::broadcast::error::RecvError;

use crate::state::{AppState, Broadcast};

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = run_session(socket, state).await {
            tracing::debug!("sync session ended: {e:#}");
        }
    })
}

async fn run_session(mut socket: WebSocket, state: Arc<AppState>) -> anyhow::Result<()> {
    // 1. Hello
    match recv_client(&mut socket).await? {
        Some(ClientMsg::Hello { protocol_version }) => {
            if protocol_version != PROTOCOL_VERSION {
                send(&mut socket, err("protocol", "unsupported protocol version")).await?;
                return Ok(());
            }
        }
        _ => {
            send(&mut socket, err("protocol", "expected hello")).await?;
            return Ok(());
        }
    }

    // 2. Challenge
    let mut challenge = [0u8; 32];
    getrandom::fill(&mut challenge).expect("OS RNG unavailable");
    send(
        &mut socket,
        ServerMsg::Challenge {
            challenge: B64.encode(challenge),
        },
    )
    .await?;

    // 3. Auth
    let (device_id, signature) = match recv_client(&mut socket).await? {
        Some(ClientMsg::Auth {
            device_id,
            signature,
        }) => (device_id, signature),
        _ => {
            send(&mut socket, err("protocol", "expected auth")).await?;
            return Ok(());
        }
    };
    let Some(device) = state.storage.device(&device_id) else {
        send(&mut socket, err("unauthorized", "unknown device")).await?;
        return Ok(());
    };
    if !verify_signature(&device.pubkey, &challenge, &signature) {
        send(&mut socket, err("unauthorized", "bad signature")).await?;
        return Ok(());
    }
    let group_id = device.group_id.clone();

    // 4. AuthOk
    send(
        &mut socket,
        ServerMsg::AuthOk {
            group_id: group_id.clone(),
            current_seq: state.storage.current_seq(&group_id),
        },
    )
    .await?;

    // 5. Subscribe to the group's live fan-out.
    let session_id = state.next_session_id();
    let tx = state.channel(&group_id);
    let mut rx = tx.subscribe();

    // 6. Main loop.
    let (mut sink, mut stream) = socket.split();
    loop {
        tokio::select! {
            incoming = stream.next() => {
                let Some(Ok(msg)) = incoming else { break };
                let text = match msg {
                    Message::Text(t) => t,
                    Message::Close(_) => break,
                    _ => continue,
                };
                let Ok(cmsg) = serde_json::from_str::<ClientMsg>(&text) else {
                    sink.send(json(err("protocol", "malformed message"))).await?;
                    continue;
                };
                // Enforce revocation mid-session: a device revoked after auth
                // must stop syncing on its live socket, not just on reconnect.
                if state.storage.device(&device_id).is_none() {
                    sink.send(json(err("unauthorized", "device revoked"))).await?;
                    break;
                }
                match cmsg {
                    ClientMsg::Push { envelope, client_change_id } => {
                        let (seq, is_new) = state.storage.append_change(
                            &group_id, &device_id, &client_change_id, envelope.clone());
                        sink.send(json(ServerMsg::Ack { client_change_id, seq })).await?;
                        if is_new {
                            let _ = tx.send(Broadcast {
                                seq,
                                device_id: device_id.clone(),
                                envelope,
                                origin: session_id,
                            });
                        }
                    }
                    ClientMsg::Pull { since_seq } => {
                        for c in state.storage.changes_since(&group_id, since_seq) {
                            sink.send(json(ServerMsg::Change {
                                seq: c.seq,
                                device_id: c.device_id,
                                envelope: c.envelope,
                            })).await?;
                        }
                        sink.send(json(ServerMsg::PullDone {
                            seq: state.storage.current_seq(&group_id),
                        })).await?;
                    }
                    ClientMsg::Ping => sink.send(json(ServerMsg::Pong)).await?,
                    ClientMsg::Hello { .. } | ClientMsg::Auth { .. } => {}
                }
            }
            bc = rx.recv() => {
                match bc {
                    Ok(b) if b.origin != session_id => {
                        sink.send(json(ServerMsg::Change {
                            seq: b.seq,
                            device_id: b.device_id,
                            envelope: b.envelope,
                        })).await?;
                    }
                    Ok(_) => {}
                    // Lagged: the client can recover with a Pull; keep the session.
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }
    Ok(())
}

fn verify_signature(pubkey: &[u8], challenge: &[u8], signature_b64: &str) -> bool {
    let Ok(pk_bytes): Result<[u8; 32], _> = pubkey.try_into() else {
        return false;
    };
    let Ok(vk) = VerifyingKey::from_bytes(&pk_bytes) else {
        return false;
    };
    let Ok(sig_bytes) = B64.decode(signature_b64.as_bytes()) else {
        return false;
    };
    let Ok(sig_arr): Result<[u8; 64], _> = sig_bytes.try_into() else {
        return false;
    };
    let sig = Signature::from_bytes(&sig_arr);
    vk.verify(challenge, &sig).is_ok()
}

fn err(code: &str, message: &str) -> ServerMsg {
    ServerMsg::Error {
        code: code.to_string(),
        message: message.to_string(),
    }
}

fn json(msg: ServerMsg) -> Message {
    Message::Text(msg.to_json())
}

async fn send(socket: &mut WebSocket, msg: ServerMsg) -> anyhow::Result<()> {
    socket.send(json(msg)).await?;
    Ok(())
}

async fn recv_client(socket: &mut WebSocket) -> anyhow::Result<Option<ClientMsg>> {
    while let Some(msg) = socket.recv().await {
        match msg? {
            Message::Text(t) => return Ok(Some(serde_json::from_str(&t)?)),
            Message::Close(_) => return Ok(None),
            _ => continue,
        }
    }
    Ok(None)
}
