//! Local persistence and shell integration for the CLI.
//!
//! The store is a single Automerge file on disk. Each command loads it, mutates,
//! and saves it back. Encryption and sync are not involved here — that is the
//! relay's boundary, handled later by `core::sync`.

use std::env;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use fs2::FileExt;
use note_core::{FolderId, NoteId, NoteStore, Timestamp};

/// Resolve the default store file path: `$PN_STORE`, else
/// `$XDG_DATA_HOME/plain-note`, else `$HOME/.local/share/plain-note`.
pub fn default_store_path() -> PathBuf {
    if let Ok(p) = env::var("PN_STORE") {
        return PathBuf::from(p);
    }
    let base = env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("plain-note").join("store.automerge")
}

/// A note store bound to a file on disk. The path is injected, so tests point at
/// a temp file instead of the user's real store — the seam that makes the
/// command logic testable.
pub struct LocalStore {
    path: PathBuf,
    /// In single-process mode, an in-process mutex serializes access instead of
    /// a file lock — for platforms without `flock` (Android). `None` = use the
    /// advisory file lock (multi-process coexistence: CLI, daemon, GUI).
    mem_lock: Option<Arc<Mutex<()>>>,
}

/// Held for the duration of a store operation to serialize access, either via an
/// advisory file lock (released on drop) or an in-process mutex.
enum StoreGuard<'a> {
    File(#[allow(dead_code)] File),
    Mem(#[allow(dead_code)] MutexGuard<'a, ()>),
}

impl LocalStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            mem_lock: None,
        }
    }

    /// A store that serializes access with an in-process mutex instead of a file
    /// lock. Use for single-process clients on platforms without `flock`
    /// (Android). All access must go through one instance.
    pub fn new_single_process(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            mem_lock: Some(Arc::new(Mutex::new(()))),
        }
    }

    /// The store at the default (environment-resolved) path.
    pub fn at_default() -> Self {
        Self::new(default_store_path())
    }

    /// The store file path (for watchers).
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Load the store under a shared lock, or start empty if it does not exist.
    pub fn load(&self) -> Result<NoteStore> {
        let _lock = self.lock(false)?;
        self.load_raw()
    }

    /// Persist the store under an exclusive lock.
    pub fn save(&self, store: &mut NoteStore) -> Result<()> {
        let _lock = self.lock(true)?;
        self.save_raw(store)
    }

    /// Read the store under a shared lock. Use for read-only operations.
    pub fn read<T>(&self, f: impl FnOnce(&NoteStore) -> Result<T>) -> Result<T> {
        let _lock = self.lock(false)?;
        let doc = self.load_raw()?;
        f(&doc)
    }

    /// Like [`Self::read`], but hands the closure a `&mut NoteStore` for
    /// read-only operations that need it (e.g. reading Automerge change
    /// metadata). Takes a shared lock and does not persist.
    pub fn read_mut<T>(&self, f: impl FnOnce(&mut NoteStore) -> Result<T>) -> Result<T> {
        let _lock = self.lock(false)?;
        let mut doc = self.load_raw()?;
        f(&mut doc)
    }

    /// Atomically load, mutate, and save under a single exclusive lock. This is
    /// the safe primitive for concurrent writers (CLI, daemon): each mutation
    /// sees the latest on-disk state and no update is lost.
    pub fn update<T>(&self, f: impl FnOnce(&mut NoteStore) -> Result<T>) -> Result<T> {
        let _lock = self.lock(true)?;
        let mut doc = self.load_raw()?;
        let out = f(&mut doc)?;
        self.save_raw(&mut doc)?;
        Ok(out)
    }

    /// Acquire an advisory lock on a sibling lock file. The returned handle
    /// releases the lock when dropped. `load`/`save`/`read`/`update` use their
    /// own scopes, so they never deadlock each other within one process.
    fn lock(&self, exclusive: bool) -> Result<StoreGuard<'_>> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        if let Some(m) = &self.mem_lock {
            // Single-process: serialize in-process (mutex is exclusive either way).
            let guard = m.lock().unwrap_or_else(|e| e.into_inner());
            return Ok(StoreGuard::Mem(guard));
        }
        let lock_path = self.path.with_extension("lock");
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)
            .with_context(|| format!("opening lock file {}", lock_path.display()))?;
        if exclusive {
            file.lock_exclusive()
        } else {
            file.lock_shared()
        }
        .with_context(|| format!("locking {}", lock_path.display()))?;
        Ok(StoreGuard::File(file))
    }

    fn load_raw(&self) -> Result<NoteStore> {
        match fs::read(&self.path) {
            Ok(bytes) => NoteStore::load(&bytes)
                .with_context(|| format!("loading store at {}", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NoteStore::new()),
            Err(e) => Err(e).with_context(|| format!("reading store at {}", self.path.display())),
        }
    }

    fn save_raw(&self, store: &mut NoteStore) -> Result<()> {
        fs::write(&self.path, store.save())
            .with_context(|| format!("writing store at {}", self.path.display()))
    }
}

/// Current wall-clock time as unix milliseconds. The clock lives in the client,
/// never in `core`.
pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as Timestamp)
        .unwrap_or(0)
}

/// Resolve a possibly-abbreviated folder id to a full one. Accepts any unique
/// prefix of an existing folder's hex id.
pub fn resolve_folder_id(store: &NoteStore, prefix: &str) -> Result<FolderId> {
    let matches: Vec<FolderId> = store
        .list_folders()?
        .into_iter()
        .map(|f| f.id)
        .filter(|id| id.as_str().starts_with(prefix))
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => Err(anyhow!("no folder matches id '{prefix}'")),
        n => Err(anyhow!(
            "folder id '{prefix}' is ambiguous ({n} folders match)"
        )),
    }
}

/// Resolve a possibly-abbreviated note id to a full one. Accepts any unique
/// prefix of an existing (non-trashed) note's hex id.
pub fn resolve_id(store: &NoteStore, prefix: &str) -> Result<NoteId> {
    resolve_from(store.list()?, prefix)
}

/// Like [`resolve_id`] but over trashed notes (for restore/purge).
pub fn resolve_trashed_id(store: &NoteStore, prefix: &str) -> Result<NoteId> {
    resolve_from(store.list_trashed()?, prefix)
}

fn resolve_from(notes: Vec<note_core::NoteMeta>, prefix: &str) -> Result<NoteId> {
    let matches: Vec<NoteId> = notes
        .into_iter()
        .map(|m| m.id)
        .filter(|id| id.as_str().starts_with(prefix))
        .collect();
    match matches.len() {
        1 => Ok(matches.into_iter().next().unwrap()),
        0 => Err(anyhow!("no note matches id '{prefix}'")),
        n => Err(anyhow!("id '{prefix}' is ambiguous ({n} notes match)")),
    }
}

/// Open `initial` in the user's editor and return the edited contents.
pub fn edit_in_editor(initial: &str) -> Result<String> {
    let editor = env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let mut file = env::temp_dir();
    file.push(format!(
        "note-edit-{}-{}.md",
        std::process::id(),
        now_millis()
    ));
    fs::write(&file, initial).with_context(|| format!("writing temp file {}", file.display()))?;

    let status = Command::new(&editor)
        .arg(&file)
        .status()
        .with_context(|| format!("launching editor '{editor}'"))?;
    if !status.success() {
        let _ = fs::remove_file(&file);
        bail!("editor '{editor}' exited with a non-zero status");
    }

    let content = fs::read_to_string(&file).with_context(|| "reading edited file")?;
    let _ = fs::remove_file(&file);
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_rejects_a_corrupt_store_file() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "pn-corrupt-{}-{}.automerge",
            std::process::id(),
            CTR.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, b"not an automerge document").unwrap();
        let store = LocalStore::new_single_process(&path);
        assert!(store.load().is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resolve_unique_prefix() {
        let mut s = NoteStore::new();
        let id = s.create_note(1).unwrap();
        let full = id.as_str().to_string();
        let got = resolve_id(&s, &full).unwrap();
        assert_eq!(got.as_str(), full);
    }

    #[test]
    fn resolve_no_match_errors() {
        let mut s = NoteStore::new();
        s.create_note(1).unwrap();
        assert!(resolve_id(&s, "zzzzzzzz").is_err());
    }

    #[test]
    fn resolve_ambiguous_errors() {
        let mut s = NoteStore::new();
        s.create_note(1).unwrap();
        s.create_note(2).unwrap();
        // The empty prefix matches every note.
        assert!(resolve_id(&s, "").is_err());
    }

    #[test]
    fn single_process_store_skips_file_lock() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "pn-sp-test-{}-{}.automerge",
            std::process::id(),
            CTR.fetch_add(1, Ordering::Relaxed)
        ));

        let store = LocalStore::new_single_process(&path);
        store
            .update(|doc| {
                doc.create_note(1)?;
                Ok(())
            })
            .unwrap();
        let count = store.read(|doc| Ok(doc.list()?.len())).unwrap();
        assert_eq!(count, 1);
        // Single-process mode uses an in-process mutex, not a `.lock` file.
        assert!(!path.with_extension("lock").exists());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn concurrent_updates_do_not_clobber() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        let mut path = std::env::temp_dir();
        path.push(format!(
            "pn-lock-test-{}-{}.automerge",
            std::process::id(),
            CTR.fetch_add(1, Ordering::Relaxed)
        ));

        // Many threads each create a note through `update` on the same path. The
        // file lock serializes load->mutate->save, so every note survives.
        const N: i64 = 12;
        let mut handles = Vec::new();
        for i in 0..N {
            let p = path.clone();
            handles.push(std::thread::spawn(move || {
                let store = LocalStore::new(p);
                store
                    .update(|doc| {
                        doc.create_note(i)?;
                        Ok(())
                    })
                    .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }

        let count = LocalStore::new(path.clone())
            .read(|doc| Ok(doc.list()?.len()))
            .unwrap();
        assert_eq!(count, N as usize);

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(path.with_extension("lock"));
    }
}
