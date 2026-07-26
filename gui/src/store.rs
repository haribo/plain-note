//! Local persistence for the GUI — the same on-disk Automerge store as `pn`.
//!
//! Minimal duplicate of the CLI's store logic; a shared client library is a
//! planned follow-up so both binaries reuse one implementation.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use note_core::{NoteStore, Timestamp};

/// Default store path: `$PN_STORE`, else `$XDG_DATA_HOME/plain-note`, else
/// `$HOME/.local/share/plain-note`. Matches the CLI so both share one store.
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

pub struct LocalStore {
    path: PathBuf,
}

impl LocalStore {
    pub fn at_default() -> Self {
        Self {
            path: default_store_path(),
        }
    }

    pub fn load(&self) -> Result<NoteStore> {
        match fs::read(&self.path) {
            Ok(bytes) => NoteStore::load(&bytes)
                .with_context(|| format!("loading store at {}", self.path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NoteStore::new()),
            Err(e) => Err(e).with_context(|| format!("reading store at {}", self.path.display())),
        }
    }

    pub fn save(&self, doc: &mut NoteStore) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        fs::write(&self.path, doc.save())
            .with_context(|| format!("writing store at {}", self.path.display()))
    }
}

pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as Timestamp)
        .unwrap_or(0)
}
