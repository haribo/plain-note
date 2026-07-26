//! Wire types shared by the client (`note-core`) and the relay (`note-relay`).
//!
//! This crate is deliberately tiny and dependency-light (serde only): it carries
//! no crypto, no CRDT, no async. It is the single definition of the JSON wire
//! format specified in `docs/sync-protocol.md`, so client and relay cannot drift.
//!
//! Byte fields (nonces, ciphertext, signatures, public keys) are base64 strings;
//! ids (`group_id`, `device_id`) are lowercase hex of 16 random bytes.

use serde::{Deserialize, Serialize};

/// Protocol version sent in the WebSocket handshake. Bumped on any wire-breaking
/// change.
pub const PROTOCOL_VERSION: u16 = 1;

// --- Enrollment (HTTP) ---

/// `POST /v1/enroll` — join a group's transport using a single-use invite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub invite_code: String,
    /// base64 Ed25519 public key of the enrolling device.
    pub device_pubkey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollResponse {
    pub device_id: String,
    pub group_id: String,
}

// --- Admin (HTTP, token-gated) ---

/// `POST /v1/groups` — create a new sync group and its first invite.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateGroupResponse {
    pub group_id: String,
    pub invite_code: String,
}

/// `POST /v1/invites` — mint another invite for an existing group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInviteRequest {
    pub group_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInviteResponse {
    pub invite_code: String,
}

// --- Sync (WebSocket) ---

/// Messages sent by a client to the relay over `/v1/sync`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// Open the session.
    Hello { protocol_version: u16 },
    /// Answer the auth challenge: base64 Ed25519 signature over the challenge.
    Auth {
        device_id: String,
        signature: String,
    },
    /// Submit one encrypted Automerge change (base64 envelope).
    Push {
        envelope: String,
        /// Client-chosen id echoed back in the `Ack`, for local correlation.
        client_change_id: String,
    },
    /// Request every change with `seq > since_seq`.
    Pull { since_seq: u64 },
    /// Keepalive.
    Ping,
}

/// Messages sent by the relay to a client over `/v1/sync`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// Random bytes (base64) the client must sign to authenticate.
    Challenge { challenge: String },
    /// Session established; the highest seq the relay currently holds.
    AuthOk { group_id: String, current_seq: u64 },
    /// A `Push` was durably stored and assigned `seq`.
    Ack { client_change_id: String, seq: u64 },
    /// A change (from any group member) to apply.
    Change {
        seq: u64,
        device_id: String,
        envelope: String,
    },
    /// End of a `Pull` response: the client is now caught up to `seq`.
    PullDone { seq: u64 },
    /// Keepalive reply.
    Pong,
    /// Terminal or recoverable error; see `code`.
    Error { code: String, message: String },
}

impl ClientMsg {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ClientMsg serializes")
    }
}

impl ServerMsg {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ServerMsg serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_msg_is_tagged() {
        let json = ClientMsg::Pull { since_seq: 7 }.to_json();
        assert_eq!(json, r#"{"type":"pull","since_seq":7}"#);
        let back: ClientMsg = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ClientMsg::Pull { since_seq: 7 }));
    }

    #[test]
    fn server_change_roundtrips() {
        let msg = ServerMsg::Change {
            seq: 3,
            device_id: "ab".into(),
            envelope: "Zm9v".into(),
        };
        let back: ServerMsg = serde_json::from_str(&msg.to_json()).unwrap();
        match back {
            ServerMsg::Change { seq, device_id, .. } => {
                assert_eq!(seq, 3);
                assert_eq!(device_id, "ab");
            }
            _ => panic!("wrong variant"),
        }
    }
}
