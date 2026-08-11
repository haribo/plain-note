//! note-core — shared logic for all clients.
//!
//! This crate is the single home of the security-critical code: the note data
//! model (Automerge CRDT), end-to-end encryption of sync deltas and
//! attachments, and the client side of the sync protocol. Every client (CLI,
//! GUI, Android via UniFFI) links against this crate so the sensitive logic is
//! written and audited once.

pub mod crypto;
pub mod doc;
pub mod model;
pub mod sync;

pub use crypto::{
    Aad, CryptoError, GroupKey, Kind, attachment_id, open, open_attachment, seal, seal_attachment,
};
pub use doc::{Block, Doc, Inline, Marks, TaskItem, doc_to_markdown, markdown_to_doc};
pub use model::{
    FolderId, FolderMeta, ModelError, Note, NoteId, NoteMeta, NoteStore, NoteVersion, ROOT_FOLDER,
    Timestamp,
};
pub use sync::{SyncConfig, SyncError, sync_once};

/// Crate version, surfaced to clients for diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
