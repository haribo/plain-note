//! Note data model, backed by an Automerge CRDT document.
//!
//! The CRDT document is the source of truth. A note carries Markdown text, a
//! title, folder placement, tags, and attachment references. Concurrent edits
//! from different devices merge deterministically without a conflict step —
//! that is the reason the model is an Automerge document rather than plain files.
//!
//! Document shape (map keys are stable identifiers, never shown to the user).
//! Notes live directly at ROOT — there is no shared container object, so two
//! devices that each start from an empty store never create conflicting
//! containers; an empty store is simply an empty document.
//!
//! ```text
//! ROOT
//!   <note_id>: Map
//!     title:       str
//!     folder:      str
//!     text:        Text      (Markdown, char-level CRDT merge)
//!     tags:        Map<tag, true>           (set semantics)
//!     attachments: Map<attachment_id, true> (set semantics)
//!     created:     int (unix millis)
//!     updated:     int (unix millis)
//! ```
//!
//! Timestamps are passed in by the caller ([`Timestamp`]) so the model stays
//! pure and deterministic; the core does no clock or I/O access.

use automerge::transaction::Transactable;
use automerge::{
    AutoCommit, AutomergeError, Change, ChangeHash, ObjId, ObjType, ROOT, ReadDoc, Value,
};
use rand::RngCore;
use rand::rngs::OsRng;

/// Unix milliseconds. Supplied by the caller; the model never reads a clock.
pub type Timestamp = i64;

const TITLE: &str = "title";
const FOLDER: &str = "folder";
const TEXT: &str = "text";
const TAGS: &str = "tags";
const ATTACHMENTS: &str = "attachments";
const CREATED: &str = "created";
const UPDATED: &str = "updated";

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error(transparent)]
    Automerge(#[from] AutomergeError),
    #[error("note not found: {0}")]
    NotFound(String),
    #[error("malformed document: {0}")]
    Malformed(&'static str),
    #[error("invalid change data")]
    InvalidChange,
}

/// Opaque note identifier: 16 random bytes, hex-encoded. Assigned client-side.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NoteId(String);

impl NoteId {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 16];
        OsRng.fill_bytes(&mut bytes);
        let mut s = String::with_capacity(32);
        for b in bytes {
            s.push_str(&format!("{b:02x}"));
        }
        Self(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for NoteId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Listing view of a note: everything except the full text body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteMeta {
    pub id: NoteId,
    pub title: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub created: Timestamp,
    pub updated: Timestamp,
}

/// Full note, including the Markdown body and attachment references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: NoteId,
    pub title: String,
    pub folder: String,
    pub text: String,
    pub tags: Vec<String>,
    pub attachments: Vec<String>,
    pub created: Timestamp,
    pub updated: Timestamp,
}

/// A collection of notes backed by a single Automerge document.
pub struct NoteStore {
    doc: AutoCommit,
}

impl Default for NoteStore {
    fn default() -> Self {
        Self::new()
    }
}

impl NoteStore {
    /// Create an empty store. This performs no writes, so an empty store is an
    /// empty Automerge document that merges cleanly with any other.
    pub fn new() -> Self {
        Self {
            doc: AutoCommit::new(),
        }
    }

    /// Load a store from its serialized Automerge form.
    pub fn load(bytes: &[u8]) -> Result<Self, ModelError> {
        Ok(Self {
            doc: AutoCommit::load(bytes)?,
        })
    }

    /// Serialize the full document (for local persistence or an initial sync).
    pub fn save(&mut self) -> Vec<u8> {
        self.doc.save()
    }

    /// Merge another store's changes into this one. CRDT semantics guarantee
    /// both documents converge regardless of merge direction or order.
    pub fn merge(&mut self, other: &mut NoteStore) -> Result<(), ModelError> {
        self.doc.merge(&mut other.doc)?;
        Ok(())
    }

    // --- sync integration (crate-internal, used by `sync`) ---

    /// Every change in the document as `(hash, raw bytes)`, in causal order. The
    /// hash doubles as the relay dedup key; the raw bytes are what gets encrypted
    /// and pushed.
    pub(crate) fn raw_changes(&mut self) -> Vec<(ChangeHash, Vec<u8>)> {
        self.doc
            .get_changes(&[])
            .into_iter()
            .map(|c| (c.hash(), c.raw_bytes().to_vec()))
            .collect()
    }

    /// Apply one raw Automerge change (decrypted from a relay envelope).
    /// Idempotent: applying an already-known change is a no-op.
    pub(crate) fn apply_change_bytes(&mut self, bytes: Vec<u8>) -> Result<(), ModelError> {
        let change = Change::from_bytes(bytes).map_err(|_| ModelError::InvalidChange)?;
        self.doc.apply_changes([change])?;
        Ok(())
    }

    // --- mutations ---

    /// Create a new empty note and return its id.
    pub fn create_note(&mut self, now: Timestamp) -> Result<NoteId, ModelError> {
        let id = NoteId::generate();
        let note = self.doc.put_object(ROOT, id.as_str(), ObjType::Map)?;
        self.doc.put_object(&note, TEXT, ObjType::Text)?;
        self.doc.put_object(&note, TAGS, ObjType::Map)?;
        self.doc.put_object(&note, ATTACHMENTS, ObjType::Map)?;
        self.doc.put(&note, TITLE, "")?;
        self.doc.put(&note, FOLDER, "")?;
        self.doc.put(&note, CREATED, now)?;
        self.doc.put(&note, UPDATED, now)?;
        Ok(id)
    }

    /// Remove a note entirely. Merges cleanly: a delete and a concurrent edit
    /// resolve deterministically under Automerge.
    pub fn delete_note(&mut self, id: &NoteId) -> Result<(), ModelError> {
        // Confirm existence so callers get NotFound rather than a silent no-op.
        self.note_obj(id)?;
        self.doc.delete(ROOT, id.as_str())?;
        Ok(())
    }

    pub fn set_title(
        &mut self,
        id: &NoteId,
        title: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        self.doc.put(&note, TITLE, title)?;
        self.touch(&note, now)
    }

    pub fn set_folder(
        &mut self,
        id: &NoteId,
        folder: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        self.doc.put(&note, FOLDER, folder)?;
        self.touch(&note, now)
    }

    /// Replace the whole Markdown body. Convenience over [`Self::splice_text`];
    /// for character-level collaborative edits prefer splicing.
    pub fn replace_text(
        &mut self,
        id: &NoteId,
        text: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        let text_obj = self.text_obj(&note)?;
        let len = self.doc.length(&text_obj) as isize;
        self.doc.splice_text(&text_obj, 0, len, text)?;
        self.touch(&note, now)
    }

    /// Splice the Markdown body: delete `del` characters at `pos`, then insert
    /// `ins`. Positions and lengths are in characters.
    pub fn splice_text(
        &mut self,
        id: &NoteId,
        pos: usize,
        del: isize,
        ins: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        let text_obj = self.text_obj(&note)?;
        self.doc.splice_text(&text_obj, pos, del, ins)?;
        self.touch(&note, now)
    }

    pub fn add_tag(&mut self, id: &NoteId, tag: &str, now: Timestamp) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        let tags = self.child_map(&note, TAGS)?;
        self.doc.put(&tags, tag, true)?;
        self.touch(&note, now)
    }

    pub fn remove_tag(&mut self, id: &NoteId, tag: &str, now: Timestamp) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        let tags = self.child_map(&note, TAGS)?;
        self.doc.delete(&tags, tag)?;
        self.touch(&note, now)
    }

    pub fn add_attachment(
        &mut self,
        id: &NoteId,
        attachment_id: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        let att = self.child_map(&note, ATTACHMENTS)?;
        self.doc.put(&att, attachment_id, true)?;
        self.touch(&note, now)
    }

    pub fn remove_attachment(
        &mut self,
        id: &NoteId,
        attachment_id: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        let att = self.child_map(&note, ATTACHMENTS)?;
        self.doc.delete(&att, attachment_id)?;
        self.touch(&note, now)
    }

    // --- queries ---

    /// Full note by id, or `None` if it does not exist.
    pub fn get_note(&self, id: &NoteId) -> Result<Option<Note>, ModelError> {
        let Some(note) = self.child_object(&ROOT, id.as_str())? else {
            return Ok(None);
        };
        let text_obj = self.text_obj(&note)?;
        Ok(Some(Note {
            id: id.clone(),
            title: self.str_field(&note, TITLE)?,
            folder: self.str_field(&note, FOLDER)?,
            text: self.doc.text(&text_obj)?,
            tags: self.keys_of(&note, TAGS)?,
            attachments: self.keys_of(&note, ATTACHMENTS)?,
            created: self.int_field(&note, CREATED)?,
            updated: self.int_field(&note, UPDATED)?,
        }))
    }

    /// Metadata for every note, unordered.
    pub fn list(&self) -> Result<Vec<NoteMeta>, ModelError> {
        let mut out = Vec::new();
        for id_str in self.doc.keys(ROOT) {
            out.push(self.meta_of(&id_str)?);
        }
        Ok(out)
    }

    /// Notes whose title or Markdown body contains `query`, case-insensitively.
    pub fn search(&self, query: &str) -> Result<Vec<NoteMeta>, ModelError> {
        let needle = query.to_lowercase();
        let mut out = Vec::new();
        for id_str in self.doc.keys(ROOT) {
            let Some(note) = self.child_object(&ROOT, &id_str)? else {
                continue;
            };
            let title = self.str_field(&note, TITLE)?;
            let text_obj = self.text_obj(&note)?;
            let body = self.doc.text(&text_obj)?;
            if title.to_lowercase().contains(&needle) || body.to_lowercase().contains(&needle) {
                out.push(self.meta_of(&id_str)?);
            }
        }
        Ok(out)
    }

    // --- internals ---

    fn touch(&mut self, note: &ObjId, now: Timestamp) -> Result<(), ModelError> {
        self.doc.put(note, UPDATED, now)?;
        Ok(())
    }

    fn note_obj(&self, id: &NoteId) -> Result<ObjId, ModelError> {
        self.child_object(&ROOT, id.as_str())?
            .ok_or_else(|| ModelError::NotFound(id.0.clone()))
    }

    fn text_obj(&self, note: &ObjId) -> Result<ObjId, ModelError> {
        self.child_object(note, TEXT)?
            .ok_or(ModelError::Malformed("missing text object"))
    }

    fn child_map(&self, note: &ObjId, key: &str) -> Result<ObjId, ModelError> {
        self.child_object(note, key)?
            .ok_or(ModelError::Malformed("missing child map"))
    }

    /// Resolve a child object id under `obj[key]`, if it is an object.
    fn child_object(&self, obj: &ObjId, key: &str) -> Result<Option<ObjId>, ModelError> {
        match self.doc.get(obj, key)? {
            Some((Value::Object(_), id)) => Ok(Some(id)),
            _ => Ok(None),
        }
    }

    fn str_field(&self, obj: &ObjId, key: &str) -> Result<String, ModelError> {
        Ok(self
            .doc
            .get(obj, key)?
            .and_then(|(v, _)| v.to_str().map(str::to_owned))
            .unwrap_or_default())
    }

    fn int_field(&self, obj: &ObjId, key: &str) -> Result<Timestamp, ModelError> {
        Ok(self
            .doc
            .get(obj, key)?
            .and_then(|(v, _)| v.to_i64())
            .unwrap_or_default())
    }

    fn keys_of(&self, note: &ObjId, key: &str) -> Result<Vec<String>, ModelError> {
        match self.child_object(note, key)? {
            Some(map) => {
                let mut ks: Vec<String> = self.doc.keys(&map).collect();
                ks.sort();
                Ok(ks)
            }
            None => Ok(Vec::new()),
        }
    }

    fn meta_of(&self, id_str: &str) -> Result<NoteMeta, ModelError> {
        let note = self
            .child_object(&ROOT, id_str)?
            .ok_or(ModelError::Malformed("note vanished mid-iteration"))?;
        Ok(NoteMeta {
            id: NoteId(id_str.to_owned()),
            title: self.str_field(&note, TITLE)?,
            folder: self.str_field(&note, FOLDER)?,
            tags: self.keys_of(&note, TAGS)?,
            created: self.int_field(&note, CREATED)?,
            updated: self.int_field(&note, UPDATED)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_read() {
        let mut s = NoteStore::new();
        let id = s.create_note(100).unwrap();
        s.set_title(&id, "Groceries", 110).unwrap();
        s.set_folder(&id, "home", 110).unwrap();
        s.replace_text(&id, "- milk\n- eggs", 120).unwrap();

        let note = s.get_note(&id).unwrap().unwrap();
        assert_eq!(note.title, "Groceries");
        assert_eq!(note.folder, "home");
        assert_eq!(note.text, "- milk\n- eggs");
        assert_eq!(note.created, 100);
        assert_eq!(note.updated, 120);
    }

    #[test]
    fn missing_note_is_none_and_errors_on_mutation() {
        let mut s = NoteStore::new();
        let ghost = NoteId::from("deadbeef".to_string());
        assert!(s.get_note(&ghost).unwrap().is_none());
        assert!(matches!(
            s.set_title(&ghost, "x", 1),
            Err(ModelError::NotFound(_))
        ));
    }

    #[test]
    fn tags_are_a_set() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        s.add_tag(&id, "urgent", 2).unwrap();
        s.add_tag(&id, "work", 3).unwrap();
        s.add_tag(&id, "urgent", 4).unwrap(); // idempotent
        assert_eq!(
            s.get_note(&id).unwrap().unwrap().tags,
            vec!["urgent", "work"]
        );
        s.remove_tag(&id, "urgent", 5).unwrap();
        assert_eq!(s.get_note(&id).unwrap().unwrap().tags, vec!["work"]);
    }

    #[test]
    fn attachments_tracked() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        s.add_attachment(&id, "aa", 2).unwrap();
        s.add_attachment(&id, "bb", 3).unwrap();
        assert_eq!(
            s.get_note(&id).unwrap().unwrap().attachments,
            vec!["aa", "bb"]
        );
    }

    #[test]
    fn splice_edits_body() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        s.replace_text(&id, "hello world", 2).unwrap();
        s.splice_text(&id, 5, 0, ",", 3).unwrap(); // "hello, world"
        assert_eq!(s.get_note(&id).unwrap().unwrap().text, "hello, world");
    }

    #[test]
    fn delete_removes_note() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        s.delete_note(&id).unwrap();
        assert!(s.get_note(&id).unwrap().is_none());
        assert!(matches!(s.delete_note(&id), Err(ModelError::NotFound(_))));
    }

    #[test]
    fn list_and_search() {
        let mut s = NoteStore::new();
        let a = s.create_note(1).unwrap();
        s.set_title(&a, "Rust notes", 1).unwrap();
        s.replace_text(&a, "ownership and borrowing", 1).unwrap();
        let b = s.create_note(2).unwrap();
        s.set_title(&b, "Shopping", 2).unwrap();
        s.replace_text(&b, "milk", 2).unwrap();

        assert_eq!(s.list().unwrap().len(), 2);

        let hits = s.search("BORROW").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a);

        let by_title = s.search("shop").unwrap();
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].id, b);
    }

    #[test]
    fn save_load_roundtrip() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        s.set_title(&id, "Persisted", 1).unwrap();
        s.replace_text(&id, "content", 1).unwrap();
        let bytes = s.save();

        let loaded = NoteStore::load(&bytes).unwrap();
        let note = loaded.get_note(&id).unwrap().unwrap();
        assert_eq!(note.title, "Persisted");
        assert_eq!(note.text, "content");
    }

    #[test]
    fn concurrent_edits_merge_without_conflict() {
        let mut a = NoteStore::new();
        let id = a.create_note(1).unwrap();
        a.replace_text(&id, "base", 1).unwrap();

        // b starts from a's state, then both edit concurrently.
        let mut b = NoteStore::load(&a.save()).unwrap();
        a.add_tag(&id, "red", 2).unwrap();
        b.add_tag(&id, "blue", 2).unwrap();

        a.merge(&mut b).unwrap();
        let note = a.get_note(&id).unwrap().unwrap();
        assert_eq!(note.tags, vec!["blue", "red"]); // both survive, sorted
    }

    #[test]
    fn merge_is_symmetric() {
        let mut a = NoteStore::new();
        let id = a.create_note(1).unwrap();
        let mut b = NoteStore::load(&a.save()).unwrap();

        a.set_folder(&id, "work", 2).unwrap();
        b.add_tag(&id, "x", 2).unwrap();

        let mut a2 = NoteStore::load(&a.save()).unwrap();
        let mut b2 = NoteStore::load(&b.save()).unwrap();
        a.merge(&mut b).unwrap();
        b2.merge(&mut a2).unwrap();

        assert_eq!(a.get_note(&id).unwrap(), b2.get_note(&id).unwrap());
    }
}
