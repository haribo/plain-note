# Note history

## Goal

Let a user see how a note evolved and recover an earlier state — a mis-edit, an
accidentally deleted paragraph, an unwanted overwrite. This is a **linear,
per-note timeline with restore**, not a git-style branching or named-version
model. Restoring never rewrites the past: it appends a new edit that reproduces a
chosen earlier state, so the timeline only ever grows forward.

## Why it costs almost nothing

Each note already lives in an Automerge CRDT document that retains **every
change** internally, and the relay already stores those changes encrypted — that
is how sync works (see `overview.md` §5 and `sync-protocol.md`). History is a
**read-only derivation over data the device already holds**; a synced device has
the full change set because pull starts at `seq 0`. Therefore:

- **No new stored data** and **no wire-format change**.
- **The zero-knowledge relay is unaffected** — no new plaintext or key material
  crosses it; history is reconstructed on-device.

Automerge exposes reading any object at a past point (`text_at`, `get_at`,
`values_at`, `length_at`, taking a set of `heads`), and every change carries a
timestamp and an author. That is the whole engine history needs.

## What a "version" is

Raw Automerge changes are far too granular to show (roughly one per edit batch).
User-visible versions are formed by **coalescing** changes into snapshots:

1. A new version boundary starts when **the author (device) changes**, **or**
   after an **idle gap of at least 5 minutes** between change timestamps. The two
   bounds are complementary: the device bound keeps cross-device edits legible;
   the idle bound makes same-device recovery ("what I had 20 minutes ago")
   possible and absorbs keystroke-level noise into one version.
2. A version is the note's state **at the heads** ending its batch, labelled with
   a wall-clock time and the originating device (e.g. `15:04 · this device`,
   `14:30 · phone`).
3. After coalescing, a candidate version is dropped **only if its content is
   identical to the previous kept version** (zero-diff dedup — e.g. a device
   switch with no content change). Versions are **never** dropped for being a
   *small* change: the most valuable state to restore is often a small
   destructive edit (a deleted line, a changed number), so filtering by diff size
   is explicitly rejected.

Ordering follows Automerge's **causal order** (heads); wall-clock time is shown
for readability only, which sidesteps cross-device clock skew.

### Retention cap

Keep at most the **100 most recent versions per note** (a `core` constant, no
user setting in v1). Beyond that the oldest versions are hidden from the
timeline. This is a **display/derivation cap, not a storage bound**: the
underlying Automerge operations are retained regardless, so the cap is cosmetic
and reversible — the number can be raised later, or real compaction added, with
**no data loss**. The restore point created by a "restore" counts as an ordinary
version and is subject to the same cap.

## Scope of a version

Title, body, tags, and folder — the note's user-visible content. Attachment
**references** are included (blobs are immutable and content-addressed, so a
historical version points at whatever ids it had). Trash state is **not** part of
history (trash is a lifecycle, not content). Restore replaces the whole content
set, not individual fields.

## Shared core API

All history logic lives in `note-core` and is reused by every client (clients
only render):

- `note_history(id) -> [Version { version_id, timestamp, device_label }]`, newest
  first, capped at 100.
- `note_at(id, version_id) -> NoteContent` — historical read via the `*_at` calls.
- `restore(id, version_id, now)` — sets the current content to that snapshot by
  splicing/replacing. This is an ordinary **forward** change, so it is merge-safe
  and itself becomes a new history entry (`restored from 14:30`).

`version_id` is an opaque handle over the snapshot's `heads`; clients never
interpret it.

## Client surfaces

Each is delivered separately, under its own workflow (mockups where required):

- **CLI**: `pn history <id>`, `pn show <id> --at <n|version>`,
  `pn restore <id> <n|version>`.
- **GUI**: a *History* control in the editor opening a timeline (time · device);
  select to preview read-only; *Restore*. Covered by a golden and an in-process
  integration test.
- **Android**: an editor-overflow *History* screen (list → preview → restore).
  Mockup-first, with a Roborazzi golden, per `ui-change-workflow.md`.

## Cross-device semantics

The timeline is the **merged** history of all synced changes; a freshly paired
device that has synced sees the full history. History is only as deep as the
changes present locally — which today is all of them, because neither the relay
nor the clients prune. If pruning is ever introduced, older history becomes
unreachable (see below).

## Out of scope

- **Compaction / pruning of old operations** to bound store growth. Automerge
  keeps full operation history, so the store grows with edit volume — already
  true today for sync. Trading history depth for size is destructive and
  sync-wide; it needs its **own ADR** and would cap how far back restore can
  reach. The 100-version retention cap above does **not** address this.
- Diff / blame visualisation, per-field or partial restore, branching or named
  versions, and history search.
