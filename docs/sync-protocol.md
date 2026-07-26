# Plain Note — Sync Protocol

Wire protocol between a client (`note-core`) and the relay. Companion to
[`design/overview.md`](design/overview.md). This is a living spec for roadmap stage 1; version it
as it stabilizes.

**Protocol version:** `1` (sent in every session handshake).

## 0. Design constraints

- The relay is **zero-knowledge**: it only ever sees ciphertext and routing
  metadata (group id, device id, sequence numbers, sizes, timestamps). It never
  holds an E2E key and cannot read note content.
- **Authentication** (may this device talk to the relay?) is fully separate from
  **encryption** (may this device read the content?). The relay enforces the
  former; only sync-group membership grants the latter.
- **Multi-tenant**: many sync groups share one relay, mutually isolated.
- CRDT (**Automerge**) makes merges automatic; the relay is a durable,
  ordered **broadcast log of encrypted changes** — it never merges anything.

## 1. Identifiers

| Id              | Shape                    | Meaning |
|-----------------|--------------------------|---------|
| `group_id`      | 16-byte random (UUIDv4)  | A sync group = the set of devices sharing one E2E key. Tenant boundary. |
| `device_id`     | 16-byte random (UUIDv4)  | One physical device within a group. |
| `note_id`       | 16-byte random           | A note inside the CRDT document. Assigned client-side. |
| `attachment_id` | 32-byte = SHA-256 of **ciphertext** | Content-addressed encrypted blob. Deduplicates automatically. |
| `seq`           | u64, per group           | Monotonic sequence number the relay assigns to each accepted change. |

`group_id` is **not** secret (the relay knows it) but is unguessable. It is
never derived from the E2E key.

## 2. Cryptography

- **AEAD:** XChaCha20-Poly1305 (24-byte random nonce → safe without a nonce
  counter, which matters across independent devices).
- **E2E key `K`:** 32 random bytes, generated once on the first device of a
  group. Shared to new devices only via QR pairing (§4). Never sent to the relay.
- **Device identity:** each device holds an **Ed25519** keypair. The public key
  is registered with the relay at enrollment; the private key signs auth
  challenges. This is auth-only and unrelated to `K`.

### 2.1 Encrypted envelope

Every content payload (a CRDT change, an attachment) is wrapped:

```
Envelope = version(1B) || nonce(24B) || ciphertext
ciphertext = XChaCha20Poly1305_Encrypt(key=K, nonce, plaintext, aad)
```

`aad` (authenticated, not encrypted) binds the ciphertext to its group, origin
device, and kind, so the relay cannot replay a blob into another group or
reinterpret it as a different kind:

```
aad = group_id(16B) || kind(1B) || device_id(16B)
kind: 0x01 = CRDT change, 0x02 = attachment
```

Values that exist only after sealing are deliberately **not** in the AAD: the
relay-assigned `seq` (§6) is untrusted ordering metadata that CRDT application
tolerates, and `attachment_id = SHA-256(ciphertext)` (§7) is verified
independently by both relay and client. Binding either would be circular or
meaningless.

The relay stores the whole `Envelope` opaquely; it can read none of it.

## 3. Access control (enrollment)

Auth is a credential the relay issues; it gates connection, not decryption.

1. **Admin issues an invitation.** The relay admin creates a **single-use,
   revocable invitation code** bound to a `group_id` (existing or new). Codes
   expire.
2. **Device enrolls.** The device generates its Ed25519 keypair and calls
   `POST /v1/enroll { invite_code, device_pubkey }`. The relay verifies the code,
   records `(group_id, device_id, device_pubkey)`, marks the code used, and
   returns `{ device_id, group_id, device_token }`. The `device_token` is a
   bearer secret for authenticated HTTP calls (attachment upload/download);
   the relay stores only its hash. WebSocket sync uses the Ed25519 key, not the
   token.
3. **Session auth (challenge-response)** on every WebSocket connect:
   - Client opens the socket and sends `Hello`.
   - Relay replies with a random `challenge` (32 bytes).
   - Client sends `Auth { device_id, signature = Ed25519_Sign(sk, challenge) }`.
   - Relay verifies against the stored pubkey. On success the session is bound to
     `(group_id, device_id)`.
4. **Revocation.** The admin removes a device's pubkey; its next auth fails and
   any live session is dropped. Revocation is independent of `K` — a revoked
   device keeps whatever plaintext it already synced (unavoidable), but can no
   longer push/pull.

> The relay never learns `K` at any step. Enrollment authorizes *transport*, QR
> pairing (next) authorizes *decryption*.

## 4. E2E key pairing (device-to-device, off-relay)

Adding a device to an existing group's **encryption** is separate from enrolling
it on the relay.

- An existing device shows a **QR code** containing everything the new device
  needs to join, transferred directly (screen → camera), never via the relay:

  ```
  QR payload (CBOR/JSON), one-time:
  {
    "v": 1,
    "relay_url": "wss://relay.example/v1/sync",
    "group_id":  "<uuid>",
    "invite_code": "<single-use enroll code>",
    "e2e_key":   "<32 bytes, base64>"     // the secret; leaves only via this QR
  }
  ```

- The new device: stores `K`, generates its Ed25519 keypair, enrolls with
  `invite_code` (§3), then connects and does an initial pull (§6).
- The QR is single-use and short-lived; `invite_code` is consumed on enrollment.

## 5. Transport & message framing

- Primary channel: **WebSocket** at `/v1/sync` for real-time change flow.
- Bulk blobs (attachments) go over **HTTP** (`/v1/attachments`) to keep the
  socket responsive; both require the same session/credential.
- Messages are JSON for stage 1 (readable, easy to debug); a binary framing may
  replace it later. Binary fields (nonces, ciphertext) are base64 in JSON.

### 5.1 Client → relay

| Type      | Fields | Purpose |
|-----------|--------|---------|
| `Hello`   | `protocol_version` | Begin session. |
| `Auth`    | `device_id, signature` | Answer the auth challenge. |
| `Push`    | `envelope` (base64), `client_change_id` | Submit one encrypted CRDT change. `client_change_id` is the Automerge change hash (hex) and doubles as the relay dedup key. |
| `Pull`    | `since_seq` | Request all changes with `seq > since_seq`. |
| `Ping`    | — | Keepalive. |

### 5.2 Relay → client

| Type       | Fields | Purpose |
|------------|--------|---------|
| `Challenge`| `challenge` | Random bytes to sign. |
| `AuthOk`   | `group_id, current_seq` | Session established; latest known seq. |
| `Ack`      | `client_change_id, seq` | A `Push` was durably stored and got `seq`. |
| `Change`   | `seq, device_id, envelope` | A change (from any group member) to apply. |
| `PullDone` | `seq` | End of a `Pull` response: the client is caught up to `seq`. |
| `Pong`     | — | Keepalive reply. |
| `Error`    | `code, message` | Auth failure, unknown group, rate limit, etc. |

## 6. Synchronization model

The relay keeps, **per group**, an append-only log of `Change` records:
`(seq, device_id, envelope, received_at)`. It assigns `seq` and never inspects
`envelope`.

**Push (local edit):**
1. Client makes an Automerge change → gets the raw change bytes.
2. Encrypt into an `Envelope` (kind `0x01`, `aad` bound to `group_id`,
   `device_id`; `seq` is `0` until assigned).
3. Send `Push`. Relay appends, assigns `seq`, replies `Ack`, and **fans out** a
   `Change` to every other connected device in the group.
4. Offline? Queue locally; replay `Push`es on reconnect.

**Pull (catch-up / new device):**
1. Client tracks the highest `seq` it has applied (`last_seq`, persisted).
2. It sends `Pull { since_seq: last_seq }`.
3. Relay streams every `Change` with `seq > since_seq` in order, then a
   `PullDone { seq }` marking the client caught up to `seq`.
4. Client decrypts each envelope and applies the Automerge change. Application is
   idempotent and order-tolerant (CRDT), so duplicates and races are harmless;
   `last_seq` only advances. `PullDone` terminates a one-shot sync round.

**Dedup and idempotency:** `client_change_id` is the Automerge change hash, which
is globally unique per change (it binds the actor and causal deps). The relay
dedups per group by this id: a re-pushed change returns its original `seq` and is
not re-appended or re-broadcast. This makes a client safe to re-push its whole
change set (e.g. after a restart) without bloating the log. The hash reveals no
content — it is an opaque identifier over ciphertext-adjacent metadata.

**Why this is conflict-free:** Automerge changes commute and merge
deterministically. Two devices editing the same note offline both push; each
applies the other's change on reconnect and converges to the same document — no
server-side merge, no user-visible conflict.

### 6.1 Compaction (later)

The log grows unbounded. A future optimization: a device periodically uploads an
encrypted **snapshot** (a compacted Automerge save) with the `seq` it covers;
the relay may then drop changes at or below that `seq`. New devices pull the
latest snapshot plus subsequent changes. Deferred past stage 1.

## 7. Attachments

Images/files are separate encrypted blobs, referenced from Markdown by
`attachment_id`.

- **Upload:** `PUT /v1/attachments/{attachment_id}` with the `Envelope`
  (kind `0x02`) as body. `attachment_id = SHA-256(ciphertext)`, so the relay can
  verify the id matches the body without reading it, and identical blobs
  deduplicate. Idempotent.
- **Download:** `GET /v1/attachments/{attachment_id}` returns the `Envelope`;
  the client decrypts with `K`.
- The Markdown note (inside the CRDT) stores the `attachment_id`; the blob syncs
  lazily/independently of the change log.
- Garbage collection of unreferenced blobs is a later concern (the relay can't
  see references, so GC is client-driven via an encrypted manifest — deferred).

## 8. What the relay can and cannot see

| Sees (metadata) | Cannot see |
|-----------------|------------|
| `group_id`, `device_id`, `seq`, timestamps | Note text, titles, folders, tags |
| Change/attachment **sizes** and counts | Any plaintext, ever |
| Which devices are online, connection times | The E2E key `K` |
| `attachment_id` (hash of ciphertext) | Attachment contents or filenames |

Traffic-analysis metadata (sizes, timing) is inherent to any relay and out of
scope for stage 1; padding/batching can be considered later.

## 9. Error & edge handling (stage 1 minimum)

- **Auth failure / revoked device:** `Error { code: "unauthorized" }`, socket
  closed.
- **Unknown/expired invite:** `enroll` returns HTTP 403.
- **Replayed `Push`:** deduplicated by `client_change_id` per group; relay
  returns the original `Ack` and does not re-broadcast.
- **`attachment_id` mismatch:** upload rejected (HTTP 422) — body hash ≠ id.
- **Gap in `seq` on the client:** issue a `Pull { since_seq: last_seq }` to
  refill; never apply out of order beyond what CRDT tolerates.
