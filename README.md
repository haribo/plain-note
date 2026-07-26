# Plain Note

Open-source, cross-platform note manager with **end-to-end encrypted**
synchronization through a **zero-knowledge relay**.

## What it is

- Notes in **Markdown**, with images/attachments stored as separate files.
- Three clients: a **Linux CLI** (`pn`), a **Linux GUI** (GTK4), and an
  **Android** app.
- Notes sync across devices in real time. The relay **can never read your notes** —
  everything is encrypted client-side before it leaves the device.
- Anyone can self-host a relay; the operator still can't exploit stored data.

See [`docs/design/overview.md`](docs/design/overview.md) for the full design and
[`docs/sync-protocol.md`](docs/sync-protocol.md) for the wire protocol.

## Architecture

| Component     | Technology |
|---------------|------------|
| `core/`       | Rust — crypto, Automerge CRDT, sync protocol (shared by all clients) |
| `protocol/`   | Rust — wire types shared by client and relay |
| `cli/`        | Rust — command-line client (binary: `pn`) |
| `relay/`      | Rust — zero-knowledge relay server (binary: `pn-relay`, Axum + WebSocket) |
| `gui/`        | Rust — GTK4 + libadwaita desktop client (binary: `plain-note-gui`) |
| Android       | Kotlin/Jetpack Compose over the Rust core via UniFFI (planned) |

## Build

```sh
cargo build
cargo test --workspace
```

## CLI usage

`pn` manages a local Automerge store (default `$XDG_DATA_HOME/plain-note`,
override with `$PN_STORE`). Note and folder ids accept any unique prefix; common
commands have short aliases.

```sh
pn new --title "Meeting" [--folder <id>] --edit  # alias: pn n — opens $EDITOR
pn list [--folder <id>] [--tag urgent]           # alias: pn ls — newest first
pn show <id>                                      # print Markdown body
pn edit <id>                                      # alias: pn e — edit in $EDITOR
pn set-title <id> <title>
pn mv <id> [--to <folder-id>]                     # move note (omit --to for root)
pn tag <id> <tag> | pn untag <id> <tag>
pn search <query>                                 # aliases: pn s / pn find
pn rm <id>
```

Attachments are encrypted client-side and stored on the relay (needs sync set up):

```sh
pn attach <note-id> <file>                        # encrypt + upload, link to the note
pn attachments <note-id>                          # list "id  filename"
pn fetch <note-id> <att-id> [--out <path>]        # download + decrypt
pn detach <note-id> <att-id>                       # drop the reference
```

Folders form a real tree (create / rename / move / delete):

```sh
pn folder new work                               # -> prints folder id
pn folder new projects --parent <work-id>
pn folder ls                                      # tree, one "id  path" per line
pn folder rename <id> <name>
pn folder mv <id> [--to <parent-id>]             # omit --to to move to the top level
pn folder rm <id>                                 # its notes/subfolders move to its parent
```

### Sync

Run a relay (`pn-relay`), configured via environment:

```sh
PN_RELAY_BIND=127.0.0.1:8787 \
PN_RELAY_ADMIN_TOKEN=<token> \
PN_RELAY_DB=/var/lib/plain-note/relay.db \
pn-relay
```

`PN_RELAY_DB` selects durable SQLite storage; unset falls back to in-memory
(data lost on restart).

Then, on the first device, create a group and enroll; this prints a pairing blob
(the QR payload stand-in) carrying the shared E2E key:

```sh
pn remote init --relay http://<host>:8787 --admin <token>
pn sync
```

On another device, join with the blob and sync:

```sh
pn remote pair <blob>
pn sync
```

For continuous background sync, run the daemon (re-syncs on local edits and
polls for remote changes):

```sh
pn sync --watch
```

Enable it as a per-user service with the unit in
[`packaging/systemd/`](packaging/systemd/plain-note-sync.service).

Sync config lives at `$XDG_CONFIG_HOME/plain-note/config.json` (override with
`$PN_CONFIG`); it holds this device's credentials and the E2E key.

## GUI

A GTK4 + libadwaita desktop client shares the same store as `pn`:

```sh
cargo run -p plain-note-gui
```

Covers local editing: folder tree + note list with search in the sidebar, and a
Markdown editor with tags and auto-save. Live auto-sync in the GUI is in
progress. Needs GTK 4 and libadwaita installed.

## Roadmap

1. **Core + CLI + relay** — encrypted sync validated end to end. ✔
2. **Linux GUI** (GTK4 + libadwaita) — in progress.
3. **Android** (Kotlin/Compose via UniFFI).

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
