# Mobile sync & pairing (UniFFI facade — increment 2)

## Purpose

Let the Android app enroll a device and synchronize with a relay, reusing
`client::remote` through the facade. QR is only a transport for the pairing
blob; all crypto/sync stays in Rust. The relay remains zero-knowledge.

## Runtime & threading

`client::remote` is async (tokio + reqwest + WebSocket). The facade runs each
remote call on an **internal current-thread tokio runtime** (`block_on`) —
consistent with the synchronous style of increment 1. Kotlin calls these methods
**off the main thread** (the ViewModel's `Dispatchers.IO`). The Android app
needs the `INTERNET` permission.

*(Alternative considered: UniFFI async → Kotlin `suspend`. More idiomatic but
adds the async-runtime feature and complexity; deferred.)*

## Facade changes

- Constructor becomes `NoteApp(store_path, config_path)`. Enrollment `Settings`
  live in a device-local config file under the app's `filesDir` (there is no XDG
  on Android).
- New methods:
  - `is_enrolled() -> bool`
  - `init_remote(relay_url, admin_secret) -> String` — create a group, return the
    **pairing blob** (the app renders it as a QR for another device to scan).
  - `pair(blob)` — join an existing group from a scanned blob.
  - `sync() -> u64` — one-shot push local / pull remote; returns the new seq.
  - `list_devices(admin_secret) -> [DeviceInfo]`, `revoke(device_id, admin_secret)`.
- `AppError` gains a `Network` variant.

## QR handling (Android side, not the facade)

Scanning/encoding is pure UI: CameraX + ML Kit Barcode to decode → `pair(blob)`;
`init_remote` returns a blob the app renders as a QR. The facade only exchanges
the **string**.

## Sync strategy (this increment)

**One-shot** `sync()` called on app open/resume and after edits.
**Continuous / background sync** (Android foreground service vs WorkManager) is
deliberately deferred — it needs its own lifecycle design.

## Security

The pairing blob carries sensitive enrollment material: shown briefly, never
persisted or logged; scanning is on-device. The relay never sees plaintext or
the E2E key. No note plaintext or key material crosses the FFI boundary.

## Out of scope

Continuous/background sync (WorkManager), attachments (increment 3), and a
mobile equivalent of the CLI `sync --watch`.
