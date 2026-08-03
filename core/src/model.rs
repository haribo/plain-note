//! Note data model, backed by an Automerge CRDT document.
//!
//! The CRDT document is the source of truth. A note carries Markdown text, a
//! title, folder placement, tags, and attachment references. Concurrent edits
//! from different devices merge deterministically without a conflict step —
//! that is the reason the model is an Automerge document rather than plain files.
//!
//! Document shape (map keys are stable identifiers, never shown to the user).
//! Notes and folders both live directly at ROOT — there is no shared container
//! object, so two devices that each start from an empty store never create
//! conflicting containers; an empty store is simply an empty document. Folder
//! keys are prefixed `f:`; note ids are 32-hex and never contain `:`.
//!
//! ```text
//! ROOT
//!   <note_id>: Map
//!     title:       str
//!     folder:      str        (folder id, or "" for the top level)
//!     text:        Text       (Markdown, char-level CRDT merge)
//!     tags:        Map<tag, true>           (set semantics)
//!     attachments: Map<attachment_id, true> (set semantics)
//!     created:     int (unix millis)
//!     updated:     int (unix millis)
//!   f:<folder_id>: Map
//!     name:        str
//!     parent:      str        (parent folder id, or "" for the top level)
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
use std::collections::{HashMap, HashSet};

/// Unix milliseconds. Supplied by the caller; the model never reads a clock.
pub type Timestamp = i64;

const TITLE: &str = "title";
const FOLDER: &str = "folder";
const TEXT: &str = "text";
const TAGS: &str = "tags";
const ATTACHMENTS: &str = "attachments";
const CREATED: &str = "created";
const UPDATED: &str = "updated";
const NAME: &str = "name";
const PARENT: &str = "parent";
const TRASHED: &str = "trashed";
const PINNED: &str = "pinned";

/// ROOT-key prefix marking a folder entry. Notes are 32-hex ids and never
/// contain `:`, so the two never collide.
const FOLDER_PREFIX: &str = "f:";

/// The sentinel parent/folder id meaning "top level" (no folder).
pub const ROOT_FOLDER: &str = "";

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error(transparent)]
    Automerge(#[from] AutomergeError),
    #[error("note not found: {0}")]
    NotFound(String),
    #[error("folder not found: {0}")]
    FolderNotFound(String),
    #[error("invalid folder move: {0}")]
    InvalidMove(&'static str),
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

/// Opaque folder identifier: 16 random bytes, hex-encoded. `""` is the reserved
/// root (no folder), not a real id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FolderId(String);

impl FolderId {
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

impl From<String> for FolderId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// A folder node: its id, display name, and parent folder id (`""` = root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderMeta {
    pub id: FolderId,
    pub name: String,
    pub parent: String,
}

/// Listing view of a note: everything except the full text body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteMeta {
    pub id: NoteId,
    pub title: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub trashed: bool,
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
    /// Attachment references as `(attachment_id, filename)`.
    pub attachments: Vec<(String, String)>,
    pub pinned: bool,
    pub trashed: bool,
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
        self.doc.put(&note, PINNED, false)?;
        self.doc.put(&note, TRASHED, false)?;
        self.doc.put(&note, CREATED, now)?;
        self.doc.put(&note, UPDATED, now)?;
        Ok(id)
    }

    /// Move a note to the trash (soft delete). It disappears from [`Self::list`]
    /// and [`Self::search`] but is kept for restore; use [`Self::delete_note`]
    /// to purge it.
    pub fn trash_note(&mut self, id: &NoteId, now: Timestamp) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        self.doc.put(&note, TRASHED, true)?;
        self.touch(&note, now)
    }

    /// Restore a trashed note.
    pub fn restore_note(&mut self, id: &NoteId, now: Timestamp) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        self.doc.put(&note, TRASHED, false)?;
        self.touch(&note, now)
    }

    /// Pin or unpin a note (clients surface pinned notes first).
    pub fn set_pinned(
        &mut self,
        id: &NoteId,
        pinned: bool,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        self.doc.put(&note, PINNED, pinned)?;
        self.touch(&note, now)
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

    /// Move a note into a folder (`folder_id`), or to the top level with
    /// [`ROOT_FOLDER`]. The folder must exist.
    pub fn move_note(
        &mut self,
        id: &NoteId,
        folder_id: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        self.require_folder(folder_id)?;
        let note = self.note_obj(id)?;
        self.doc.put(&note, FOLDER, folder_id)?;
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

    /// Reference an attachment by id, remembering its `filename` for downloads.
    pub fn add_attachment(
        &mut self,
        id: &NoteId,
        attachment_id: &str,
        filename: &str,
        now: Timestamp,
    ) -> Result<(), ModelError> {
        let note = self.note_obj(id)?;
        let att = self.child_map(&note, ATTACHMENTS)?;
        self.doc.put(&att, attachment_id, filename)?;
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

    // --- folders ---

    /// Create a folder under `parent_id` ([`ROOT_FOLDER`] for top level).
    pub fn create_folder(
        &mut self,
        name: &str,
        parent_id: &str,
        _now: Timestamp,
    ) -> Result<FolderId, ModelError> {
        self.require_folder(parent_id)?;
        let id = FolderId::generate();
        let folder = self
            .doc
            .put_object(ROOT, folder_key(id.as_str()), ObjType::Map)?;
        self.doc.put(&folder, NAME, name)?;
        self.doc.put(&folder, PARENT, parent_id)?;
        Ok(id)
    }

    pub fn rename_folder(&mut self, id: &FolderId, name: &str) -> Result<(), ModelError> {
        let folder = self.folder_obj(id)?;
        self.doc.put(&folder, NAME, name)?;
        Ok(())
    }

    /// Reparent a folder. Rejects moving a folder into itself or a descendant.
    pub fn move_folder(&mut self, id: &FolderId, new_parent: &str) -> Result<(), ModelError> {
        if new_parent == id.as_str() {
            return Err(ModelError::InvalidMove("a folder cannot be its own parent"));
        }
        self.require_folder(new_parent)?;
        if self.is_descendant(new_parent, id.as_str()) {
            return Err(ModelError::InvalidMove(
                "cannot move a folder into its own descendant",
            ));
        }
        let folder = self.folder_obj(id)?;
        self.doc.put(&folder, PARENT, new_parent)?;
        Ok(())
    }

    /// Delete a folder, reparenting its child folders and notes to its parent.
    pub fn delete_folder(&mut self, id: &FolderId, now: Timestamp) -> Result<(), ModelError> {
        let folder = self.folder_obj(id)?;
        let parent = self.str_field(&folder, PARENT)?;

        for child in self.list_folders()? {
            if child.parent == id.as_str() {
                let obj = self.folder_obj(&child.id)?;
                self.doc.put(&obj, PARENT, parent.as_str())?;
            }
        }
        let orphans: Vec<NoteId> = self
            .list()?
            .into_iter()
            .filter(|n| n.folder == id.as_str())
            .map(|n| n.id)
            .collect();
        for note_id in orphans {
            let note = self.note_obj(&note_id)?;
            self.doc.put(&note, FOLDER, parent.as_str())?;
            self.touch(&note, now)?;
        }

        self.doc.delete(ROOT, folder_key(id.as_str()))?;
        Ok(())
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
            attachments: self.entries_of(&note, ATTACHMENTS)?,
            pinned: self.bool_field(&note, PINNED)?,
            trashed: self.bool_field(&note, TRASHED)?,
            created: self.int_field(&note, CREATED)?,
            updated: self.int_field(&note, UPDATED)?,
        }))
    }

    /// Metadata for every active (non-trashed) note, unordered.
    pub fn list(&self) -> Result<Vec<NoteMeta>, ModelError> {
        let mut out = Vec::new();
        for id_str in self.doc.keys(ROOT) {
            if is_folder_key(&id_str) {
                continue;
            }
            let meta = self.meta_of(&id_str)?;
            if !meta.trashed {
                out.push(meta);
            }
        }
        Ok(out)
    }

    /// Metadata for every trashed note (the Trash view).
    pub fn list_trashed(&self) -> Result<Vec<NoteMeta>, ModelError> {
        let mut out = Vec::new();
        for id_str in self.doc.keys(ROOT) {
            if is_folder_key(&id_str) {
                continue;
            }
            let meta = self.meta_of(&id_str)?;
            if meta.trashed {
                out.push(meta);
            }
        }
        Ok(out)
    }

    /// Every folder node, unordered.
    pub fn list_folders(&self) -> Result<Vec<FolderMeta>, ModelError> {
        let mut out = Vec::new();
        for key in self.doc.keys(ROOT) {
            let Some(id) = key.strip_prefix(FOLDER_PREFIX) else {
                continue;
            };
            let obj = self
                .child_object(&ROOT, &key)?
                .ok_or(ModelError::Malformed("folder vanished mid-iteration"))?;
            out.push(FolderMeta {
                id: FolderId(id.to_string()),
                name: self.str_field(&obj, NAME)?,
                parent: self.str_field(&obj, PARENT)?,
            });
        }
        Ok(out)
    }

    /// Slash-joined display path for a folder id (`""` for root). Cycle-safe:
    /// a parent chain that loops (possible only under a pathological merge) is
    /// broken rather than looping forever.
    pub fn folder_path(&self, id: &str) -> Result<String, ModelError> {
        if id == ROOT_FOLDER {
            return Ok(String::new());
        }
        let byid: HashMap<String, FolderMeta> = self
            .list_folders()?
            .into_iter()
            .map(|f| (f.id.0.clone(), f))
            .collect();
        let mut parts = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = id.to_string();
        while cur != ROOT_FOLDER {
            if !seen.insert(cur.clone()) {
                break;
            }
            match byid.get(&cur) {
                Some(f) => {
                    parts.push(f.name.clone());
                    cur = f.parent.clone();
                }
                None => break,
            }
        }
        parts.reverse();
        Ok(parts.join("/"))
    }

    /// Notes whose title or Markdown body contains `query`, case-insensitively.
    pub fn search(&self, query: &str) -> Result<Vec<NoteMeta>, ModelError> {
        let needle = query.to_lowercase();
        let mut out = Vec::new();
        for id_str in self.doc.keys(ROOT) {
            if is_folder_key(&id_str) {
                continue;
            }
            let Some(note) = self.child_object(&ROOT, &id_str)? else {
                continue;
            };
            let title = self.str_field(&note, TITLE)?;
            let text_obj = self.text_obj(&note)?;
            let body = self.doc.text(&text_obj)?;
            if title.to_lowercase().contains(&needle) || body.to_lowercase().contains(&needle) {
                let meta = self.meta_of(&id_str)?;
                if !meta.trashed {
                    out.push(meta);
                }
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

    fn folder_obj(&self, id: &FolderId) -> Result<ObjId, ModelError> {
        self.child_object(&ROOT, &folder_key(id.as_str()))?
            .ok_or_else(|| ModelError::FolderNotFound(id.0.clone()))
    }

    /// `Ok` if the id is the root sentinel or an existing folder.
    fn require_folder(&self, id: &str) -> Result<(), ModelError> {
        if id == ROOT_FOLDER || self.child_object(&ROOT, &folder_key(id))?.is_some() {
            Ok(())
        } else {
            Err(ModelError::FolderNotFound(id.to_string()))
        }
    }

    /// Is `candidate` inside the subtree rooted at `ancestor`? Cycle-safe.
    fn is_descendant(&self, candidate: &str, ancestor: &str) -> bool {
        let byid: HashMap<String, String> = self
            .list_folders()
            .unwrap_or_default()
            .into_iter()
            .map(|f| (f.id.0, f.parent))
            .collect();
        let mut cur = candidate.to_string();
        let mut seen = HashSet::new();
        while cur != ROOT_FOLDER {
            if cur == ancestor {
                return true;
            }
            if !seen.insert(cur.clone()) {
                return false;
            }
            match byid.get(&cur) {
                Some(p) => cur = p.clone(),
                None => return false,
            }
        }
        false
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

    fn bool_field(&self, obj: &ObjId, key: &str) -> Result<bool, ModelError> {
        Ok(self
            .doc
            .get(obj, key)?
            .and_then(|(v, _)| v.to_bool())
            .unwrap_or(false))
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

    /// Key/string-value pairs of a child map, sorted by key.
    fn entries_of(&self, note: &ObjId, key: &str) -> Result<Vec<(String, String)>, ModelError> {
        match self.child_object(note, key)? {
            Some(map) => {
                let mut out = Vec::new();
                for k in self.doc.keys(&map) {
                    let v = self.str_field(&map, &k)?;
                    out.push((k, v));
                }
                out.sort();
                Ok(out)
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
            pinned: self.bool_field(&note, PINNED)?,
            trashed: self.bool_field(&note, TRASHED)?,
            created: self.int_field(&note, CREATED)?,
            updated: self.int_field(&note, UPDATED)?,
        })
    }
}

fn folder_key(id: &str) -> String {
    format!("{FOLDER_PREFIX}{id}")
}

fn is_folder_key(key: &str) -> bool {
    key.starts_with(FOLDER_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_read() {
        let mut s = NoteStore::new();
        let home = s.create_folder("home", ROOT_FOLDER, 100).unwrap();
        let id = s.create_note(100).unwrap();
        s.set_title(&id, "Groceries", 110).unwrap();
        s.move_note(&id, home.as_str(), 110).unwrap();
        s.replace_text(&id, "- milk\n- eggs", 120).unwrap();

        let note = s.get_note(&id).unwrap().unwrap();
        assert_eq!(note.title, "Groceries");
        assert_eq!(note.folder, home.as_str());
        assert_eq!(s.folder_path(&note.folder).unwrap(), "home");
        assert_eq!(note.text, "- milk\n- eggs");
        assert_eq!(note.created, 100);
        assert_eq!(note.updated, 120);
    }

    #[test]
    fn folder_hierarchy_and_paths() {
        let mut s = NoteStore::new();
        let work = s.create_folder("work", ROOT_FOLDER, 1).unwrap();
        let proj = s.create_folder("projects", work.as_str(), 1).unwrap();
        assert_eq!(s.folder_path(proj.as_str()).unwrap(), "work/projects");
        assert_eq!(s.list_folders().unwrap().len(), 2);

        // move_note validates the target folder exists.
        let n = s.create_note(1).unwrap();
        assert!(s.move_note(&n, "nonexistent", 2).is_err());
        s.move_note(&n, proj.as_str(), 2).unwrap();
        assert_eq!(s.get_note(&n).unwrap().unwrap().folder, proj.as_str());

        // Notes are excluded from folder listings and vice versa.
        assert_eq!(s.list().unwrap().len(), 1);

        // rename + move
        s.rename_folder(&proj, "proj").unwrap();
        assert_eq!(s.folder_path(proj.as_str()).unwrap(), "work/proj");
        s.move_folder(&proj, ROOT_FOLDER).unwrap();
        assert_eq!(s.folder_path(proj.as_str()).unwrap(), "proj");

        // cannot move a folder into its own descendant
        let child = s.create_folder("child", proj.as_str(), 1).unwrap();
        assert!(s.move_folder(&proj, child.as_str()).is_err());
        assert!(s.move_folder(&proj, proj.as_str()).is_err());
    }

    #[test]
    fn delete_folder_reparents_children() {
        let mut s = NoteStore::new();
        let a = s.create_folder("a", ROOT_FOLDER, 1).unwrap();
        let b = s.create_folder("b", a.as_str(), 1).unwrap();
        let n = s.create_note(1).unwrap();
        s.move_note(&n, a.as_str(), 1).unwrap();

        s.delete_folder(&a, 2).unwrap();
        // b and the note reparent to a's parent (root).
        assert_eq!(s.get_note(&n).unwrap().unwrap().folder, ROOT_FOLDER);
        assert_eq!(
            s.list_folders()
                .unwrap()
                .iter()
                .find(|f| f.id == b)
                .unwrap()
                .parent,
            ROOT_FOLDER
        );
    }

    #[test]
    fn folders_merge_across_devices() {
        let mut a = NoteStore::new();
        let mut b = NoteStore::load(&a.save()).unwrap();
        // Concurrent folder creation on two devices — distinct ids, both survive.
        a.create_folder("from-a", ROOT_FOLDER, 1).unwrap();
        b.create_folder("from-b", ROOT_FOLDER, 1).unwrap();
        a.merge(&mut b).unwrap();
        let names: Vec<String> = a
            .list_folders()
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"from-a".to_string()));
        assert!(names.contains(&"from-b".to_string()));
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
    fn attachments_tracked_with_filenames() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        s.add_attachment(&id, "aa", "diagram.png", 2).unwrap();
        s.add_attachment(&id, "bb", "notes.pdf", 3).unwrap();
        assert_eq!(
            s.get_note(&id).unwrap().unwrap().attachments,
            vec![
                ("aa".to_string(), "diagram.png".to_string()),
                ("bb".to_string(), "notes.pdf".to_string()),
            ]
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
    fn trash_hides_restores_and_purges() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        assert_eq!(s.list().unwrap().len(), 1);

        s.trash_note(&id, 2).unwrap();
        assert_eq!(s.list().unwrap().len(), 0);
        assert_eq!(s.list_trashed().unwrap().len(), 1);
        assert!(s.get_note(&id).unwrap().unwrap().trashed);

        s.restore_note(&id, 3).unwrap();
        assert_eq!(s.list().unwrap().len(), 1);
        assert!(s.list_trashed().unwrap().is_empty());

        s.delete_note(&id).unwrap(); // purge
        assert!(s.get_note(&id).unwrap().is_none());
    }

    #[test]
    fn pin_flag_round_trips() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        assert!(!s.get_note(&id).unwrap().unwrap().pinned);
        s.set_pinned(&id, true, 2).unwrap();
        assert!(s.get_note(&id).unwrap().unwrap().pinned);
        assert!(s.list().unwrap()[0].pinned);
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
        let work = a.create_folder("work", ROOT_FOLDER, 1).unwrap();
        let id = a.create_note(1).unwrap();
        let mut b = NoteStore::load(&a.save()).unwrap();

        a.move_note(&id, work.as_str(), 2).unwrap();
        b.add_tag(&id, "x", 2).unwrap();

        let mut a2 = NoteStore::load(&a.save()).unwrap();
        let mut b2 = NoteStore::load(&b.save()).unwrap();
        a.merge(&mut b).unwrap();
        b2.merge(&mut a2).unwrap();

        assert_eq!(a.get_note(&id).unwrap(), b2.get_note(&id).unwrap());
    }

    #[test]
    fn concurrent_text_edits_merge() {
        // The headline CRDT claim: concurrent char-level body edits both survive.
        let mut a = NoteStore::new();
        let id = a.create_note(1).unwrap();
        a.replace_text(&id, "hello world", 1).unwrap();
        let mut b = NoteStore::load(&a.save()).unwrap();
        a.splice_text(&id, 11, 0, "!", 2).unwrap(); // append at the end
        b.splice_text(&id, 0, 0, ">> ", 2).unwrap(); // prepend at the start
        a.merge(&mut b).unwrap();
        let text = a.get_note(&id).unwrap().unwrap().text;
        assert!(text.starts_with(">> "), "b's edit survived: {text:?}");
        assert!(text.ends_with('!'), "a's edit survived: {text:?}");
        assert!(text.contains("hello world"));
    }

    #[test]
    fn delete_note_wins_over_concurrent_edit() {
        // A hard delete on one device removes the note despite a concurrent edit
        // on another — the delete is not silently undone by the edit.
        let mut a = NoteStore::new();
        let id = a.create_note(1).unwrap();
        a.replace_text(&id, "keep", 1).unwrap();
        let mut b = NoteStore::load(&a.save()).unwrap();
        a.delete_note(&id).unwrap();
        b.set_title(&id, "edited", 2).unwrap();
        a.merge(&mut b).unwrap();
        assert!(a.get_note(&id).unwrap().is_none());
    }

    #[test]
    fn load_rejects_corrupt_bytes() {
        assert!(NoteStore::load(&[0xde, 0xad, 0xbe, 0xef]).is_err());
    }

    #[test]
    fn apply_change_bytes_rejects_malformed() {
        let mut s = NoteStore::new();
        let err = s.apply_change_bytes(vec![1, 2, 3, 4]).unwrap_err();
        assert!(matches!(err, ModelError::InvalidChange));
    }
}
