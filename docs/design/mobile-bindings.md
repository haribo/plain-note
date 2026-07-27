# Mobile bindings (UniFFI facade)

## Purpose

Expose the Rust core to the Android app (Kotlin/Compose) through a thin
**facade**, so mobile clients reuse the exact security-critical logic (crypto,
CRDT, sync) instead of reimplementing it. Kotlin holds UI only.

## Crate

New workspace member `mobile/` (package `plain-note-mobile`), a library with
`crate-type = ["cdylib", "lib"]`. It depends on `plain-note-client` (and
transitively `note-core`). Bindings are generated with **UniFFI 0.28
proc-macros** (`#[uniffi::export]`, no UDL). Kotlin namespace `dev.plainnote.core`.

## Exposed surface (increment 1 — local only)

A single stateful object built from a store path:

- `NoteApp(store_path)` — opens/creates the local store.
- Notes: `create_note`, `list_notes(folder?, tag?)`, `search(query)`,
  `get_note(id)`, `set_title`, `set_body`, `move_note(id, folder?)`, `add_tag`,
  `remove_tag`, `trash`, `restore`, `list_trashed`, `empty_trash`, `set_pinned`,
  `delete`.
- Folders: `create_folder`, `list_folders`, `rename_folder`,
  `move_folder(id, parent?)`, `delete_folder`.

Data crosses the boundary as **plain UniFFI records**, never core types:

- `NoteSummary { id, title, folder, tags, pinned, updated }`
- `NoteContent { id, title, text, folder, tags, pinned }`
- `FolderInfo { id, name, parent, path }`

Errors surface as one enum `AppError { NotFound, Ambiguous, Invalid, Io }`.

## Threading & lifecycle

Methods are **synchronous** and each takes the store file lock internally
(single-process on Android, but kept for CLI/daemon coexistence on shared
storage). Kotlin must call them **off the main thread**. `NoteApp` is a
long-lived handle held by the Android app.

## Deliberately not exposed

Crypto keys, raw Automerge documents, the wire format. The clock stays
client-side (the facade uses system time, like the CLI).

## Out of scope (later increments)

- **Sync + QR pairing** (async/network) — increment 2, needs an explicit
  runtime/threading design.
- **Attachments** (HTTP) — increment 3.
- **The Android Compose project** and its cargo-ndk cross-build.
