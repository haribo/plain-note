//! Note command logic, decoupled from I/O and presentation.
//!
//! Each function takes an injected [`LocalStore`] and an explicit timestamp, and
//! returns data (ids, notes, metadata) rather than printing — so it can be unit
//! tested against a temp store with a deterministic clock. `main` owns the
//! printing and the editor/clock wiring.

use anyhow::{Result, anyhow};
use note_core::{FolderId, FolderMeta, Note, NoteId, NoteMeta, NoteVersion, Timestamp};

use crate::store::{LocalStore, resolve_folder_id, resolve_id, resolve_trashed_id};

pub fn new_note(
    store: &LocalStore,
    now: Timestamp,
    title: Option<&str>,
    folder: Option<&str>,
    body: Option<&str>,
) -> Result<NoteId> {
    store.update(|doc| {
        // Resolve the folder prefix first, so a bad folder id fails before any write.
        let folder_id = match folder {
            Some(p) => Some(resolve_folder_id(doc, p)?.as_str().to_string()),
            None => None,
        };
        let id = doc.create_note(now)?;
        if let Some(t) = title {
            doc.set_title(&id, t, now)?;
        }
        if let Some(f) = folder_id {
            doc.move_note(&id, &f, now)?;
        }
        if let Some(b) = body {
            doc.replace_text(&id, b, now)?;
        }
        Ok(id)
    })
}

pub fn list(store: &LocalStore, folder: Option<&str>, tag: Option<&str>) -> Result<Vec<NoteMeta>> {
    store.read(|doc| {
        // Accept a folder id prefix for the filter, like every other folder input.
        let folder_id = match folder {
            Some(p) => Some(resolve_folder_id(doc, p)?.as_str().to_string()),
            None => None,
        };
        let mut notes = doc.list()?;
        notes.retain(|n| {
            folder_id.as_deref().is_none_or(|f| n.folder.as_str() == f)
                && tag.is_none_or(|t| n.tags.iter().any(|x| x == t))
        });
        // Pinned first, then most recently updated.
        notes.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated.cmp(&a.updated)));
        Ok(notes)
    })
}

pub fn search(store: &LocalStore, query: &str) -> Result<Vec<NoteMeta>> {
    store.read(|doc| {
        let mut hits = doc.search(query)?;
        hits.sort_by_key(|n| std::cmp::Reverse(n.updated));
        Ok(hits)
    })
}

pub fn get(store: &LocalStore, id_prefix: &str) -> Result<Note> {
    store.read(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.get_note(&id)?.ok_or_else(|| anyhow!("note vanished"))
    })
}

/// A note's version timeline, newest first.
pub fn history(store: &LocalStore, id_prefix: &str) -> Result<Vec<NoteVersion>> {
    store.read_mut(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        Ok(doc.note_history(&id)?)
    })
}

/// A note's content at a given version.
pub fn note_at(store: &LocalStore, id_prefix: &str, version_id: &str) -> Result<Note> {
    store.read(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.note_at(&id, version_id)?
            .ok_or_else(|| anyhow!("no such version"))
    })
}

/// Restore a note to a past version (a new forward edit).
pub fn restore_version(
    store: &LocalStore,
    now: Timestamp,
    id_prefix: &str,
    version_id: &str,
) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.restore(&id, version_id, now)?;
        Ok(id)
    })
}

pub fn set_title(
    store: &LocalStore,
    now: Timestamp,
    id_prefix: &str,
    title: &str,
) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.set_title(&id, title, now)?;
        Ok(id)
    })
}

/// Move a note into a folder given by id-prefix, or to the root (`None`).
pub fn move_note(
    store: &LocalStore,
    now: Timestamp,
    note_prefix: &str,
    folder_prefix: Option<&str>,
) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, note_prefix)?;
        let folder = match folder_prefix {
            Some(p) => resolve_folder_id(doc, p)?.as_str().to_string(),
            None => note_core::ROOT_FOLDER.to_string(),
        };
        doc.move_note(&id, &folder, now)?;
        Ok(id)
    })
}

/// A folder with its resolved display path, for listing.
pub struct FolderRow {
    pub meta: FolderMeta,
    pub path: String,
}

pub fn create_folder(
    store: &LocalStore,
    now: Timestamp,
    name: &str,
    parent_prefix: Option<&str>,
) -> Result<FolderId> {
    store.update(|doc| {
        let parent = match parent_prefix {
            Some(p) => resolve_folder_id(doc, p)?.as_str().to_string(),
            None => note_core::ROOT_FOLDER.to_string(),
        };
        Ok(doc.create_folder(name, &parent, now)?)
    })
}

pub fn list_folders(store: &LocalStore) -> Result<Vec<FolderRow>> {
    store.read(|doc| {
        let mut rows = Vec::new();
        for meta in doc.list_folders()? {
            let path = doc.folder_path(meta.id.as_str())?;
            rows.push(FolderRow { meta, path });
        }
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(rows)
    })
}

pub fn rename_folder(store: &LocalStore, id_prefix: &str, name: &str) -> Result<FolderId> {
    store.update(|doc| {
        let id = resolve_folder_id(doc, id_prefix)?;
        doc.rename_folder(&id, name)?;
        Ok(id)
    })
}

pub fn move_folder(
    store: &LocalStore,
    id_prefix: &str,
    new_parent_prefix: Option<&str>,
) -> Result<FolderId> {
    store.update(|doc| {
        let id = resolve_folder_id(doc, id_prefix)?;
        let parent = match new_parent_prefix {
            Some(p) => resolve_folder_id(doc, p)?.as_str().to_string(),
            None => note_core::ROOT_FOLDER.to_string(),
        };
        doc.move_folder(&id, &parent)?;
        Ok(id)
    })
}

pub fn delete_folder(store: &LocalStore, now: Timestamp, id_prefix: &str) -> Result<FolderId> {
    store.update(|doc| {
        let id = resolve_folder_id(doc, id_prefix)?;
        doc.delete_folder(&id, now)?;
        Ok(id)
    })
}

/// Resolve a note's folder id to a display path (for listings). Empty = root.
pub fn folder_path(store: &LocalStore, folder_id: &str) -> Result<String> {
    store.read(|doc| Ok(doc.folder_path(folder_id)?))
}

/// Replace a note's Markdown body (used by `edit` once the editor returns).
pub fn set_body(store: &LocalStore, now: Timestamp, id_prefix: &str, body: &str) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.replace_text(&id, body, now)?;
        Ok(id)
    })
}

pub fn add_tag(store: &LocalStore, now: Timestamp, id_prefix: &str, tag: &str) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.add_tag(&id, tag, now)?;
        Ok(id)
    })
}

pub fn remove_tag(
    store: &LocalStore,
    now: Timestamp,
    id_prefix: &str,
    tag: &str,
) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.remove_tag(&id, tag, now)?;
        Ok(id)
    })
}

/// Move an active note to the trash (soft delete).
pub fn trash(store: &LocalStore, now: Timestamp, id_prefix: &str) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.trash_note(&id, now)?;
        Ok(id)
    })
}

/// Restore a trashed note.
pub fn restore(store: &LocalStore, now: Timestamp, id_prefix: &str) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_trashed_id(doc, id_prefix)?;
        doc.restore_note(&id, now)?;
        Ok(id)
    })
}

/// Every trashed note, newest first.
pub fn list_trashed(store: &LocalStore) -> Result<Vec<NoteMeta>> {
    store.read(|doc| {
        let mut notes = doc.list_trashed()?;
        notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
        Ok(notes)
    })
}

/// Permanently delete every trashed note. Returns how many were purged.
pub fn empty_trash(store: &LocalStore) -> Result<usize> {
    store.update(|doc| {
        let ids: Vec<NoteId> = doc.list_trashed()?.into_iter().map(|n| n.id).collect();
        for id in &ids {
            doc.delete_note(id)?;
        }
        Ok(ids.len())
    })
}

/// Pin or unpin a note.
pub fn set_pinned(
    store: &LocalStore,
    now: Timestamp,
    id_prefix: &str,
    pinned: bool,
) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix)?;
        doc.set_pinned(&id, pinned, now)?;
        Ok(id)
    })
}

/// Permanently delete a note by id (active or trashed).
pub fn delete(store: &LocalStore, id_prefix: &str) -> Result<NoteId> {
    store.update(|doc| {
        let id = resolve_id(doc, id_prefix).or_else(|_| resolve_trashed_id(doc, id_prefix))?;
        doc.delete_note(&id)?;
        Ok(id)
    })
}

/// A note's attachment references as `(attachment_id, filename)`.
pub fn attachments(store: &LocalStore, note_prefix: &str) -> Result<Vec<(String, String)>> {
    Ok(get(store, note_prefix)?.attachments)
}

/// Drop an attachment reference from a note (the blob stays on the relay).
/// Returns the removed attachment id.
pub fn detach(
    store: &LocalStore,
    now: Timestamp,
    note_prefix: &str,
    att_prefix: &str,
) -> Result<String> {
    store.update(|doc| {
        let id = resolve_id(doc, note_prefix)?;
        let note = doc.get_note(&id)?.ok_or_else(|| anyhow!("note vanished"))?;
        let mut matches = note
            .attachments
            .into_iter()
            .filter(|(a, _)| a.starts_with(att_prefix));
        let att = match (matches.next(), matches.next()) {
            (Some((a, _)), None) => a,
            (None, _) => return Err(anyhow!("no attachment matches '{att_prefix}'")),
            (Some(_), Some(_)) => {
                return Err(anyhow!("attachment id '{att_prefix}' is ambiguous"));
            }
        };
        doc.remove_attachment(&id, &att, now)?;
        Ok(att)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> (LocalStore, std::path::PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        // Counter guarantees uniqueness even when tests run in the same
        // millisecond on parallel threads.
        path.push(format!(
            "pn-cmd-test-{}-{}.automerge",
            std::process::id(),
            CTR.fetch_add(1, Ordering::Relaxed)
        ));
        (LocalStore::new(path.clone()), path)
    }

    #[test]
    fn create_list_and_filter_by_folder() {
        let (s, path) = temp_store();
        let dev = create_folder(&s, 1, "dev", None).unwrap();
        let a = new_note(&s, 10, Some("Rust"), Some(dev.as_str()), Some("ownership")).unwrap();
        let _b = new_note(&s, 20, Some("Milk"), None, None).unwrap();

        let all = list(&s, None, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].updated, 20); // newest first

        let in_dev = list(&s, Some(dev.as_str()), None).unwrap();
        assert_eq!(in_dev.len(), 1);
        assert_eq!(in_dev[0].id, a);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn folder_commands() {
        let (s, path) = temp_store();
        let work = create_folder(&s, 1, "work", None).unwrap();
        let proj = create_folder(&s, 1, "projects", Some(work.as_str())).unwrap();

        let rows = list_folders(&s).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().any(|r| r.path == "work/projects"));

        rename_folder(&s, proj.as_str(), "proj").unwrap();
        move_folder(&s, proj.as_str(), None).unwrap();
        let rows = list_folders(&s).unwrap();
        assert!(rows.iter().any(|r| r.path == "proj"));

        // Move a note in, then delete the folder — the note reparents to root.
        let n = new_note(&s, 1, Some("N"), None, None).unwrap();
        move_note(&s, 2, n.as_str(), Some(work.as_str())).unwrap();
        delete_folder(&s, 3, work.as_str()).unwrap();
        assert_eq!(get(&s, n.as_str()).unwrap().folder, note_core::ROOT_FOLDER);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn tag_search_and_delete() {
        let (s, path) = temp_store();
        let id = new_note(&s, 1, Some("Note"), None, Some("body text")).unwrap();
        add_tag(&s, 2, id.as_str(), "urgent").unwrap();

        let by_tag = list(&s, None, Some("urgent")).unwrap();
        assert_eq!(by_tag.len(), 1);
        let hits = search(&s, "BODY").unwrap();
        assert_eq!(hits.len(), 1);

        remove_tag(&s, 3, id.as_str(), "urgent").unwrap();
        assert!(list(&s, None, Some("urgent")).unwrap().is_empty());

        delete(&s, id.as_str()).unwrap();
        assert!(list(&s, None, None).unwrap().is_empty());

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn edits_persist_across_reload() {
        let (s, path) = temp_store();
        let work = create_folder(&s, 1, "work", None).unwrap();
        let id = new_note(&s, 1, None, None, None).unwrap();
        set_title(&s, 2, id.as_str(), "Renamed").unwrap();
        move_note(&s, 3, id.as_str(), Some(work.as_str())).unwrap();
        set_body(&s, 4, id.as_str(), "hello").unwrap();

        // A fresh LocalStore at the same path re-reads from disk.
        let reopened = LocalStore::new(path.clone());
        let note = get(&reopened, id.as_str()).unwrap();
        assert_eq!(note.title, "Renamed");
        assert_eq!(note.folder, work.as_str());
        assert_eq!(note.text, "hello");

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unknown_id_errors() {
        let (s, path) = temp_store();
        new_note(&s, 1, Some("x"), None, None).unwrap();
        assert!(set_title(&s, 2, "zzzz", "y").is_err());
        let _ = std::fs::remove_file(path);
    }
}
