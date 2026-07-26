//! Note command logic, decoupled from I/O and presentation.
//!
//! Each function takes an injected [`LocalStore`] and an explicit timestamp, and
//! returns data (ids, notes, metadata) rather than printing — so it can be unit
//! tested against a temp store with a deterministic clock. `main` owns the
//! printing and the editor/clock wiring.

use anyhow::{Result, anyhow};
use note_core::{Note, NoteId, NoteMeta, Timestamp};

use crate::store::{LocalStore, resolve_id};

pub fn new_note(
    store: &LocalStore,
    now: Timestamp,
    title: Option<&str>,
    folder: Option<&str>,
    body: Option<&str>,
) -> Result<NoteId> {
    let mut doc = store.load()?;
    let id = doc.create_note(now)?;
    if let Some(t) = title {
        doc.set_title(&id, t, now)?;
    }
    if let Some(f) = folder {
        doc.set_folder(&id, f, now)?;
    }
    if let Some(b) = body {
        doc.replace_text(&id, b, now)?;
    }
    store.save(&mut doc)?;
    Ok(id)
}

pub fn list(store: &LocalStore, folder: Option<&str>, tag: Option<&str>) -> Result<Vec<NoteMeta>> {
    let doc = store.load()?;
    let mut notes = doc.list()?;
    notes.retain(|n| {
        folder.is_none_or(|f| n.folder.as_str() == f)
            && tag.is_none_or(|t| n.tags.iter().any(|x| x == t))
    });
    notes.sort_by_key(|n| std::cmp::Reverse(n.updated));
    Ok(notes)
}

pub fn search(store: &LocalStore, query: &str) -> Result<Vec<NoteMeta>> {
    let doc = store.load()?;
    let mut hits = doc.search(query)?;
    hits.sort_by_key(|n| std::cmp::Reverse(n.updated));
    Ok(hits)
}

pub fn get(store: &LocalStore, id_prefix: &str) -> Result<Note> {
    let doc = store.load()?;
    let id = resolve_id(&doc, id_prefix)?;
    doc.get_note(&id)?.ok_or_else(|| anyhow!("note vanished"))
}

pub fn set_title(
    store: &LocalStore,
    now: Timestamp,
    id_prefix: &str,
    title: &str,
) -> Result<NoteId> {
    let mut doc = store.load()?;
    let id = resolve_id(&doc, id_prefix)?;
    doc.set_title(&id, title, now)?;
    store.save(&mut doc)?;
    Ok(id)
}

pub fn set_folder(
    store: &LocalStore,
    now: Timestamp,
    id_prefix: &str,
    folder: &str,
) -> Result<NoteId> {
    let mut doc = store.load()?;
    let id = resolve_id(&doc, id_prefix)?;
    doc.set_folder(&id, folder, now)?;
    store.save(&mut doc)?;
    Ok(id)
}

/// Replace a note's Markdown body (used by `edit` once the editor returns).
pub fn set_body(store: &LocalStore, now: Timestamp, id_prefix: &str, body: &str) -> Result<NoteId> {
    let mut doc = store.load()?;
    let id = resolve_id(&doc, id_prefix)?;
    doc.replace_text(&id, body, now)?;
    store.save(&mut doc)?;
    Ok(id)
}

pub fn add_tag(store: &LocalStore, now: Timestamp, id_prefix: &str, tag: &str) -> Result<NoteId> {
    let mut doc = store.load()?;
    let id = resolve_id(&doc, id_prefix)?;
    doc.add_tag(&id, tag, now)?;
    store.save(&mut doc)?;
    Ok(id)
}

pub fn remove_tag(
    store: &LocalStore,
    now: Timestamp,
    id_prefix: &str,
    tag: &str,
) -> Result<NoteId> {
    let mut doc = store.load()?;
    let id = resolve_id(&doc, id_prefix)?;
    doc.remove_tag(&id, tag, now)?;
    store.save(&mut doc)?;
    Ok(id)
}

pub fn delete(store: &LocalStore, id_prefix: &str) -> Result<NoteId> {
    let mut doc = store.load()?;
    let id = resolve_id(&doc, id_prefix)?;
    doc.delete_note(&id)?;
    store.save(&mut doc)?;
    Ok(id)
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
    fn create_list_and_filter() {
        let (s, path) = temp_store();
        let a = new_note(&s, 10, Some("Rust"), Some("dev"), Some("ownership")).unwrap();
        let _b = new_note(&s, 20, Some("Milk"), Some("home"), None).unwrap();

        let all = list(&s, None, None).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].updated, 20); // newest first

        let dev = list(&s, Some("dev"), None).unwrap();
        assert_eq!(dev.len(), 1);
        assert_eq!(dev[0].id, a);

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
        let id = new_note(&s, 1, None, None, None).unwrap();
        set_title(&s, 2, id.as_str(), "Renamed").unwrap();
        set_folder(&s, 3, id.as_str(), "work").unwrap();
        set_body(&s, 4, id.as_str(), "hello").unwrap();

        // A fresh LocalStore at the same path re-reads from disk.
        let reopened = LocalStore::new(path.clone());
        let note = get(&reopened, id.as_str()).unwrap();
        assert_eq!(note.title, "Renamed");
        assert_eq!(note.folder, "work");
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
