//! Local persistence and shell integration for the CLI.
//!
//! The store is a single Automerge file on disk. Each command loads it, mutates,
//! and saves it back. Encryption and sync are not involved here — that is the
//! relay's boundary, handled later by `core::sync`.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow, bail};
use note_core::{NoteId, NoteStore, Timestamp};

/// Resolve the store file path: `$PN_STORE`, else `$XDG_DATA_HOME/plain-note`,
/// else `$HOME/.local/share/plain-note`.
pub fn store_path() -> PathBuf {
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

/// Load the store, or start an empty one if the file does not exist yet.
pub fn load() -> Result<NoteStore> {
    let path = store_path();
    match fs::read(&path) {
        Ok(bytes) => {
            NoteStore::load(&bytes).with_context(|| format!("loading store at {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(NoteStore::new()),
        Err(e) => Err(e).with_context(|| format!("reading store at {}", path.display())),
    }
}

/// Persist the store, creating the parent directory if needed.
pub fn save(store: &mut NoteStore) -> Result<()> {
    let path = store_path();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(&path, store.save()).with_context(|| format!("writing store at {}", path.display()))
}

/// Current wall-clock time as unix milliseconds. The clock lives in the client,
/// never in `core`.
pub fn now_millis() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as Timestamp)
        .unwrap_or(0)
}

/// Resolve a possibly-abbreviated note id to a full one. Accepts any unique
/// prefix of an existing note's hex id.
pub fn resolve_id(store: &NoteStore, prefix: &str) -> Result<NoteId> {
    let matches: Vec<NoteId> = store
        .list()?
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
}
