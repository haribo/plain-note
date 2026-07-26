//! note-core — shared logic for all clients.
//!
//! This crate is the single home of the security-critical code: the note data
//! model (Automerge CRDT), end-to-end encryption of sync deltas and
//! attachments, and the client side of the sync protocol. Every client (CLI,
//! GUI, Android via UniFFI) links against this crate so the sensitive logic is
//! written and audited once.

pub mod crypto;
pub mod model;
pub mod sync;

pub use crypto::{Aad, CryptoError, GroupKey, Kind, open, seal};
pub use model::{ModelError, Note, NoteId, NoteMeta, NoteStore, Timestamp};
pub use sync::{SyncConfig, SyncError, sync_once};

/// Crate version, surfaced to clients for diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
