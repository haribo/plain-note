# Plain Note — Design

> An open-source, cross-platform note manager with end-to-end encrypted
> synchronization through a zero-knowledge relay. CLI binary: `pn`.

## 1. Goal

A note-taking tool available on three clients — a Linux command-line client, a
Linux GUI, and an Android app — whose notes synchronize seamlessly across
devices. Synchronization goes through a relay server that **must never be able
to read note contents**. Anyone can host a relay, but a relay operator (or an
authorized client from another sync group) can never exploit the stored data.

Non-goals for the first versions: web client, real-time collaborative editing
between different users, rich-text WYSIWYG.

## 2. Core Principles

1. **Zero-knowledge relay** — everything that transits or is stored on the relay
   is encrypted client-side beforehand. The relay only ever sees opaque blobs.
2. **Local plaintext** — on-device notes are stored in clear (readable,
   editable `.md` files). Encryption happens only at the sync boundary.
3. **Separation of concerns** — *authentication* (who may use the relay) is
   fully independent from *encryption* (who may read the content). A device can
   be authorized to use the relay yet still be unable to read another group's
   notes.
4. **Self-hostable, access-controlled** — the relay is privately hosted; its
   backend controls which clients are allowed to connect. It is multi-tenant:
   several users (with different E2E keys) can share one relay, each isolated.
5. **Shared core** — the sensitive logic (crypto, CRDT, sync protocol) is
   written once and reused across all clients.

## 3. Notes Model

- **Format**: Markdown (plain text, easy to edit in any editor).
- **Images / attachments**: stored as **separate files**, referenced from the
  Markdown (`![caption](attachments/xxx.png)`). Each attachment is encrypted as
  an independent blob and synced separately, so only what changed is uploaded.
- **Organization**: folders **and** tags (a note lives in one folder, can carry
  many tags).
- **Search**: full-text search over note content.
- **Source of truth**: the CRDT document (see §5), not the raw `.md` file. The
  local `.md` is a readable export/import surface; external edits are re-imported
  into the CRDT.

## 4. Encryption & Key Management

- **End-to-end encryption (E2E)**: encryption/decryption happen only on the
  user's devices. The relay never holds a key.
- **On the relay**: encrypted **CRDT deltas** (not full-document blobs replaced
  each time) plus encrypted attachment blobs.
- **Key sharing between devices**: **QR-code pairing**. One device generates the
  random E2E key; a new device joins the sync group by scanning a QR code. The
  key never transits through the relay and never leaves the devices over the
  network.
- **Sync group**: the set of devices sharing the same E2E key. The relay routes
  encrypted updates within a group without knowing their contents.
- Losing all devices in a group without a backup of the key means the notes are
  unrecoverable — the intended price of zero-knowledge.

## 5. Synchronization

- **Conflict resolution**: **CRDT** (automatic merge, no user-visible
  conflicts), using **Automerge** (`automerge-rs`).
- **Transport**: **real-time over WebSocket**. Changes propagate immediately when
  a device is online; offline edits catch up on reconnection.
- **What is synced**: encrypted CRDT updates for note content/metadata, plus
  encrypted attachment blobs.

## 6. Relay (Backend)

Zero-knowledge server: authentication, routing, and storage of encrypted blobs
only. No crypto of note contents.

- **Language/stack**: **Rust** — Axum + WebSocket, SQLite or Postgres for
  storage. Single lightweight binary, easy to self-host.
- **Multi-tenant**: multiple sync groups isolated from one another.
- **Access control**:
  1. The relay admin generates **revocable, single-use invitation codes**.
  2. A device registers with a code and receives a **signed device credential**
     (auth token). The relay keeps a list of authorized devices and can revoke
     any of them.
  3. This credential only proves "allowed to use this relay". It is unrelated to
     the E2E key (which travels only via QR between the user's own devices).

## 7. Tech Stack

| Component     | Technology |
|---------------|------------|
| Shared core   | **Rust** — crypto, Automerge CRDT, sync protocol |
| CLI           | Rust (calls the core directly) |
| Linux GUI     | **GTK4** (`gtk4-rs`), calls the core directly |
| Android       | **Kotlin / Jetpack Compose** UI over the Rust core via **UniFFI** |
| Relay         | **Rust** — Axum + WebSocket |

The Rust core is the single home of all security-critical logic and is reused by
every client (natively for CLI/GUI, via UniFFI bindings for Android).

## 8. License

**Apache-2.0** — permissive (free commercial and closed-source use), with an
explicit warranty/liability disclaimer and an explicit patent grant, and it is
the de-facto standard of the Rust ecosystem.

## 9. Roadmap (build order)

Order follows technical dependencies.

1. **Core + CLI + relay** — build the Rust core (crypto / CRDT / sync), validate
   it through the CLI (fast to iterate) and the relay. Goal: solid encrypted
   sync between two CLI instances through the relay.
2. **Linux GUI (GTK4)** — on top of the proven core.
3. **Android (Kotlin/Compose via UniFFI)** — on the same proven core.

Each stage builds on a validated foundation rather than reworking the core
repeatedly.
