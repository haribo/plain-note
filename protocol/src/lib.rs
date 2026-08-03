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
    /// Bearer token for authenticated HTTP calls (attachment upload/download).
    pub device_token: String,
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

/// A device as exposed to an admin — id only, never the public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
}

/// `GET /v1/devices?group_id=` response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceInfo>,
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

    // The wire format is a contract between client and relay; these pin the
    // exact JSON so a field/variant rename fails CI instead of drifting silently.

    #[test]
    fn client_msg_tags_are_stable() {
        assert_eq!(
            ClientMsg::Hello {
                protocol_version: 1
            }
            .to_json(),
            r#"{"type":"hello","protocol_version":1}"#
        );
        assert_eq!(
            ClientMsg::Auth {
                device_id: "d".into(),
                signature: "s".into()
            }
            .to_json(),
            r#"{"type":"auth","device_id":"d","signature":"s"}"#
        );
        assert_eq!(
            ClientMsg::Push {
                envelope: "e".into(),
                client_change_id: "c".into()
            }
            .to_json(),
            r#"{"type":"push","envelope":"e","client_change_id":"c"}"#
        );
        assert_eq!(ClientMsg::Ping.to_json(), r#"{"type":"ping"}"#);
    }

    #[test]
    fn server_msg_tags_are_stable() {
        assert_eq!(
            ServerMsg::Challenge {
                challenge: "x".into()
            }
            .to_json(),
            r#"{"type":"challenge","challenge":"x"}"#
        );
        assert_eq!(
            ServerMsg::AuthOk {
                group_id: "g".into(),
                current_seq: 5
            }
            .to_json(),
            r#"{"type":"auth_ok","group_id":"g","current_seq":5}"#
        );
        assert_eq!(
            ServerMsg::Ack {
                client_change_id: "c".into(),
                seq: 2
            }
            .to_json(),
            r#"{"type":"ack","client_change_id":"c","seq":2}"#
        );
        assert_eq!(
            ServerMsg::PullDone { seq: 9 }.to_json(),
            r#"{"type":"pull_done","seq":9}"#
        );
        assert_eq!(ServerMsg::Pong.to_json(), r#"{"type":"pong"}"#);
        assert_eq!(
            ServerMsg::Error {
                code: "bad".into(),
                message: "nope".into()
            }
            .to_json(),
            r#"{"type":"error","code":"bad","message":"nope"}"#
        );
    }

    #[test]
    fn http_structs_field_names_are_stable() {
        let cases = [
            (
                serde_json::to_string(&EnrollRequest {
                    invite_code: "i".into(),
                    device_pubkey: "k".into(),
                })
                .unwrap(),
                r#"{"invite_code":"i","device_pubkey":"k"}"#,
            ),
            (
                serde_json::to_string(&EnrollResponse {
                    device_id: "d".into(),
                    group_id: "g".into(),
                    device_token: "t".into(),
                })
                .unwrap(),
                r#"{"device_id":"d","group_id":"g","device_token":"t"}"#,
            ),
            (
                serde_json::to_string(&CreateGroupResponse {
                    group_id: "g".into(),
                    invite_code: "i".into(),
                })
                .unwrap(),
                r#"{"group_id":"g","invite_code":"i"}"#,
            ),
            (
                serde_json::to_string(&CreateInviteRequest {
                    group_id: "g".into(),
                })
                .unwrap(),
                r#"{"group_id":"g"}"#,
            ),
            (
                serde_json::to_string(&CreateInviteResponse {
                    invite_code: "i".into(),
                })
                .unwrap(),
                r#"{"invite_code":"i"}"#,
            ),
            (
                serde_json::to_string(&DeviceListResponse {
                    devices: vec![DeviceInfo { id: "x".into() }],
                })
                .unwrap(),
                r#"{"devices":[{"id":"x"}]}"#,
            ),
        ];
        for (got, want) in cases {
            assert_eq!(got, want);
        }
    }
}
