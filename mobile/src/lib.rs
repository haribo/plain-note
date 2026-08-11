//! Plain Note — mobile bindings (UniFFI facade).
//!
//! A thin façade over `plain-note-client` exposed to Kotlin/Android via UniFFI.
//! The Android app reuses the Rust core through this crate instead of
//! reimplementing any security-critical logic. See
//! `docs/design/mobile-bindings.md`.
//!
//! Increment 1 covers local operations only (no sync/network).

use std::path::PathBuf;
use std::sync::Arc;

use note_core::{Note, NoteMeta};
use plain_note_client::commands::{self, FolderRow};
use plain_note_client::store::{self, LocalStore};
use plain_note_client::{config, remote};

uniffi::setup_scaffolding!();

pub mod doc;

/// A note as shown in a list: metadata only, no body.
#[derive(Debug, uniffi::Record)]
pub struct NoteSummary {
    pub id: String,
    pub title: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub pinned: bool,
    pub updated: i64,
}

/// A note with its Markdown body, for the editor.
#[derive(Debug, uniffi::Record)]
pub struct NoteContent {
    pub id: String,
    pub title: String,
    pub text: String,
    pub folder: String,
    pub tags: Vec<String>,
    pub pinned: bool,
}

/// One entry in a note's history timeline (opaque id + when it was reached).
#[derive(Debug, uniffi::Record)]
pub struct NoteVersionInfo {
    pub version_id: String,
    pub timestamp: i64,
}

/// A folder with its resolved display path.
#[derive(Debug, uniffi::Record)]
pub struct FolderInfo {
    pub id: String,
    pub name: String,
    pub parent: String,
    pub path: String,
}

/// A device registered in the sync group.
#[derive(Debug, uniffi::Record)]
pub struct DeviceInfo {
    pub id: String,
    pub is_self: bool,
}

/// Errors crossing the FFI boundary.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum AppError {
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Ambiguous(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Io(String),
    #[error("{0}")]
    Network(String),
}

/// Classify an `anyhow` error into an [`AppError`] variant. The core surfaces
/// resolution failures as plain messages, so we match on those.
fn map(e: anyhow::Error) -> AppError {
    if e.downcast_ref::<std::io::Error>().is_some() {
        return AppError::Io(e.to_string());
    }
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("no note matches")
        || lower.contains("no folder matches")
        || lower.contains("not found")
    {
        AppError::NotFound(msg)
    } else if lower.contains("ambiguous") {
        AppError::Ambiguous(msg)
    } else {
        AppError::Invalid(msg)
    }
}

/// Like [`map`], but unclassified failures default to `Network` — used for
/// remote operations, where the common failure is the relay being unreachable.
fn map_net(e: anyhow::Error) -> AppError {
    match map(e) {
        AppError::Invalid(m) => AppError::Network(m),
        other => other,
    }
}

/// A short-lived current-thread runtime to drive one async remote call.
fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| AppError::Io(e.to_string()))
}

type Result<T> = std::result::Result<T, AppError>;

fn summary(m: NoteMeta) -> NoteSummary {
    NoteSummary {
        id: m.id.as_str().to_string(),
        title: m.title,
        folder: m.folder,
        tags: m.tags,
        pinned: m.pinned,
        updated: m.updated,
    }
}

fn content(n: Note) -> NoteContent {
    NoteContent {
        id: n.id.as_str().to_string(),
        title: n.title,
        text: n.text,
        folder: n.folder,
        tags: n.tags,
        pinned: n.pinned,
    }
}

/// The mobile app handle: a local store, the enrollment config path, and the
/// operations over them.
#[derive(uniffi::Object)]
pub struct NoteApp {
    store: LocalStore,
    config_path: PathBuf,
}

#[uniffi::export]
impl NoteApp {
    /// Open (or lazily create) the store at `store_path`; enrollment settings
    /// live at `config_path` (both under the app's private storage).
    #[uniffi::constructor]
    pub fn new(store_path: String, config_path: String) -> Arc<Self> {
        Arc::new(Self {
            // Single-process store: Android has no working `flock`; the store's
            // in-process mutex serializes the ViewModel's concurrent IO calls.
            store: LocalStore::new_single_process(store_path),
            config_path: PathBuf::from(config_path),
        })
    }

    // --- notes ---

    /// Create an empty note and return its id.
    pub fn create_note(&self) -> Result<String> {
        let id = commands::new_note(&self.store, now(), None, None, None).map_err(map)?;
        Ok(id.as_str().to_string())
    }

    /// List active notes, optionally filtered by folder id and/or tag.
    pub fn list_notes(
        &self,
        folder: Option<String>,
        tag: Option<String>,
    ) -> Result<Vec<NoteSummary>> {
        let notes = commands::list(&self.store, folder.as_deref(), tag.as_deref()).map_err(map)?;
        Ok(notes.into_iter().map(summary).collect())
    }

    /// Search titles and bodies (case-insensitive substring).
    pub fn search(&self, query: String) -> Result<Vec<NoteSummary>> {
        let notes = commands::search(&self.store, &query).map_err(map)?;
        Ok(notes.into_iter().map(summary).collect())
    }

    /// Fetch a note's full content by id.
    pub fn get_note(&self, id: String) -> Result<NoteContent> {
        let n = commands::get(&self.store, &id).map_err(map)?;
        Ok(content(n))
    }

    /// The note's version timeline, newest first.
    pub fn history(&self, id: String) -> Result<Vec<NoteVersionInfo>> {
        Ok(commands::history(&self.store, &id)
            .map_err(map)?
            .into_iter()
            .map(|v| NoteVersionInfo {
                version_id: v.version_id,
                timestamp: v.timestamp,
            })
            .collect())
    }

    /// The note's content at a given version.
    pub fn note_at(&self, id: String, version_id: String) -> Result<NoteContent> {
        let n = commands::note_at(&self.store, &id, &version_id).map_err(map)?;
        Ok(content(n))
    }

    /// Restore a note to a past version (a new forward edit).
    pub fn restore_version(&self, id: String, version_id: String) -> Result<()> {
        commands::restore_version(&self.store, now(), &id, &version_id).map_err(map)?;
        Ok(())
    }

    pub fn set_title(&self, id: String, title: String) -> Result<()> {
        commands::set_title(&self.store, now(), &id, &title).map_err(map)?;
        Ok(())
    }

    pub fn set_body(&self, id: String, text: String) -> Result<()> {
        commands::set_body(&self.store, now(), &id, &text).map_err(map)?;
        Ok(())
    }

    /// Move a note into a folder id, or to the top level when `folder` is null.
    pub fn move_note(&self, id: String, folder: Option<String>) -> Result<()> {
        commands::move_note(&self.store, now(), &id, folder.as_deref()).map_err(map)?;
        Ok(())
    }

    pub fn add_tag(&self, id: String, tag: String) -> Result<()> {
        commands::add_tag(&self.store, now(), &id, &tag).map_err(map)?;
        Ok(())
    }

    pub fn remove_tag(&self, id: String, tag: String) -> Result<()> {
        commands::remove_tag(&self.store, now(), &id, &tag).map_err(map)?;
        Ok(())
    }

    /// Move a note to the trash.
    pub fn trash(&self, id: String) -> Result<()> {
        commands::trash(&self.store, now(), &id).map_err(map)?;
        Ok(())
    }

    /// Restore a trashed note.
    pub fn restore(&self, id: String) -> Result<()> {
        commands::restore(&self.store, now(), &id).map_err(map)?;
        Ok(())
    }

    pub fn list_trashed(&self) -> Result<Vec<NoteSummary>> {
        let notes = commands::list_trashed(&self.store).map_err(map)?;
        Ok(notes.into_iter().map(summary).collect())
    }

    /// Permanently delete every trashed note; returns how many were purged.
    pub fn empty_trash(&self) -> Result<u32> {
        let n = commands::empty_trash(&self.store).map_err(map)?;
        Ok(n as u32)
    }

    pub fn set_pinned(&self, id: String, pinned: bool) -> Result<()> {
        commands::set_pinned(&self.store, now(), &id, pinned).map_err(map)?;
        Ok(())
    }

    /// Permanently delete a note (active or trashed).
    pub fn delete(&self, id: String) -> Result<()> {
        commands::delete(&self.store, &id).map_err(map)?;
        Ok(())
    }

    // --- folders ---

    /// Create a folder, optionally under a parent folder id.
    pub fn create_folder(&self, name: String, parent: Option<String>) -> Result<String> {
        let id =
            commands::create_folder(&self.store, now(), &name, parent.as_deref()).map_err(map)?;
        Ok(id.as_str().to_string())
    }

    pub fn list_folders(&self) -> Result<Vec<FolderInfo>> {
        let rows = commands::list_folders(&self.store).map_err(map)?;
        Ok(rows.into_iter().map(folder_info).collect())
    }

    pub fn rename_folder(&self, id: String, name: String) -> Result<()> {
        commands::rename_folder(&self.store, &id, &name).map_err(map)?;
        Ok(())
    }

    /// Move a folder under another, or to the top level when `parent` is null.
    pub fn move_folder(&self, id: String, parent: Option<String>) -> Result<()> {
        commands::move_folder(&self.store, &id, parent.as_deref()).map_err(map)?;
        Ok(())
    }

    /// Delete a folder; its notes and subfolders move up to its parent.
    pub fn delete_folder(&self, id: String) -> Result<()> {
        commands::delete_folder(&self.store, now(), &id).map_err(map)?;
        Ok(())
    }

    // --- sync & pairing (remote) ---

    /// Whether this device is enrolled in a sync group.
    pub fn is_enrolled(&self) -> bool {
        config::Settings::load_from(&self.config_path).is_ok()
    }

    /// Create a new group on the relay and enroll this device (admin). Returns
    /// the pairing blob to share with another device (e.g. as a QR code).
    pub fn init_remote(&self, relay_url: String, admin_secret: String) -> Result<String> {
        runtime()?
            .block_on(remote::init(&self.config_path, &relay_url, &admin_secret))
            .map_err(map_net)
    }

    /// Join an existing group from a pairing blob (e.g. scanned from a QR code).
    pub fn pair(&self, blob: String) -> Result<()> {
        runtime()?
            .block_on(remote::pair(&self.config_path, &blob))
            .map_err(map_net)
    }

    /// Push local changes and pull remote ones; returns the new sequence number.
    pub fn sync(&self) -> Result<u64> {
        runtime()?
            .block_on(remote::sync(&self.config_path, &self.store))
            .map_err(map_net)
    }

    /// List the devices registered in this group (admin).
    pub fn list_devices(&self, admin_secret: String) -> Result<Vec<DeviceInfo>> {
        let devices = runtime()?
            .block_on(remote::devices(&self.config_path, &admin_secret))
            .map_err(map_net)?;
        Ok(devices
            .into_iter()
            .map(|(id, is_self)| DeviceInfo { id, is_self })
            .collect())
    }

    /// Revoke a device so it can no longer sync (admin).
    pub fn revoke(&self, device_id: String, admin_secret: String) -> Result<()> {
        runtime()?
            .block_on(remote::revoke(&self.config_path, &admin_secret, &device_id))
            .map_err(map_net)
    }
}

fn folder_info(row: FolderRow) -> FolderInfo {
    FolderInfo {
        id: row.meta.id.as_str().to_string(),
        name: row.meta.name,
        parent: row.meta.parent,
        path: row.path,
    }
}

/// Current wall-clock time in unix milliseconds (the clock lives client-side).
fn now() -> i64 {
    store::now_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_app() -> Arc<NoteApp> {
        static CTR: AtomicU64 = AtomicU64::new(0);
        let n = CTR.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir();
        let store = dir.join(format!("pn-mobile-{}-{n}.automerge", std::process::id()));
        let config = dir.join(format!("pn-mobile-{}-{n}.config", std::process::id()));
        NoteApp::new(
            store.to_string_lossy().to_string(),
            config.to_string_lossy().to_string(),
        )
    }

    #[test]
    fn fresh_app_is_not_enrolled() {
        let app = temp_app();
        assert!(!app.is_enrolled());
    }

    #[test]
    fn sync_without_enrollment_errors() {
        let app = temp_app();
        // No config file -> settings load fails; classified as a network error.
        assert!(app.sync().is_err());
    }

    #[test]
    fn create_list_get_and_edit() {
        let app = temp_app();
        let id = app.create_note().unwrap();
        app.set_title(id.clone(), "Hello".into()).unwrap();
        app.set_body(id.clone(), "body".into()).unwrap();

        let notes = app.list_notes(None, None).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "Hello");

        let content = app.get_note(id).unwrap();
        assert_eq!(content.text, "body");
    }

    #[test]
    fn folders_filter_and_move() {
        let app = temp_app();
        let folder = app.create_folder("Work".into(), None).unwrap();
        let id = app.create_note().unwrap();
        app.move_note(id, Some(folder.clone())).unwrap();

        assert_eq!(app.list_notes(Some(folder), None).unwrap().len(), 1);
        let folders = app.list_folders().unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Work");
    }

    #[test]
    fn trash_restore_and_delete() {
        let app = temp_app();
        let id = app.create_note().unwrap();
        app.set_pinned(id.clone(), true).unwrap();

        app.trash(id.clone()).unwrap();
        assert!(app.list_notes(None, None).unwrap().is_empty());
        assert_eq!(app.list_trashed().unwrap().len(), 1);

        app.restore(id.clone()).unwrap();
        assert_eq!(app.list_notes(None, None).unwrap().len(), 1);

        app.delete(id).unwrap();
        assert!(app.list_notes(None, None).unwrap().is_empty());
    }

    #[test]
    fn missing_note_maps_to_not_found() {
        let app = temp_app();
        let err = app.get_note("deadbeef".into()).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn error_mapping_classifies_each_variant() {
        use anyhow::anyhow;
        assert!(matches!(
            map(anyhow!("ambiguous prefix xyz")),
            AppError::Ambiguous(_)
        ));
        assert!(matches!(
            map(anyhow!("totally unexpected")),
            AppError::Invalid(_)
        ));
        assert!(matches!(
            map(anyhow::Error::new(std::io::Error::other("disk"))),
            AppError::Io(_)
        ));
        // map_net reclassifies the Invalid catch-all as Network, keeps the rest.
        assert!(matches!(
            map_net(anyhow!("totally unexpected")),
            AppError::Network(_)
        ));
        assert!(matches!(
            map_net(anyhow!("ambiguous prefix xyz")),
            AppError::Ambiguous(_)
        ));
    }

    #[test]
    fn tag_add_remove_and_search() {
        let app = temp_app();
        let id = app.create_note().unwrap();
        app.set_title(id.clone(), "Groceries".into()).unwrap();
        app.add_tag(id.clone(), "urgent".into()).unwrap();
        assert!(
            app.get_note(id.clone())
                .unwrap()
                .tags
                .contains(&"urgent".to_string())
        );
        app.remove_tag(id.clone(), "urgent".into()).unwrap();
        assert!(app.get_note(id.clone()).unwrap().tags.is_empty());
        let hits = app.search("grocer".into()).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, id);
    }

    #[test]
    fn empty_trash_purges_trashed_notes() {
        let app = temp_app();
        let id = app.create_note().unwrap();
        app.trash(id).unwrap();
        assert_eq!(app.empty_trash().unwrap(), 1);
        assert!(app.list_trashed().unwrap().is_empty());
    }

    #[test]
    fn history_note_at_and_restore_via_facade() {
        let app = temp_app();
        let id = app.create_note().unwrap();
        app.set_title(id.clone(), "V1".into()).unwrap();
        let h = app.history(id.clone()).unwrap();
        assert!(!h.is_empty());
        let oldest = h.last().unwrap().version_id.clone();
        let snap = app.note_at(id.clone(), oldest.clone()).unwrap();
        assert_eq!(snap.id, id);
        app.restore_version(id.clone(), oldest).unwrap();
    }

    #[test]
    fn folder_rename_move_and_delete() {
        let app = temp_app();
        let parent = app.create_folder("Parent".into(), None).unwrap();
        let child = app.create_folder("Child".into(), None).unwrap();
        app.rename_folder(child.clone(), "Renamed".into()).unwrap();
        app.move_folder(child.clone(), Some(parent.clone()))
            .unwrap();
        let folders = app.list_folders().unwrap();
        let moved = folders.iter().find(|f| f.id == child).unwrap();
        assert_eq!(moved.name, "Renamed");
        assert_eq!(moved.parent, parent);
        app.delete_folder(parent.clone()).unwrap();
        assert!(app.list_folders().unwrap().iter().all(|f| f.id != parent));
    }
}
