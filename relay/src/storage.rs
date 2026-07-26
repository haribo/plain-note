//! Relay persistence: groups, invites, devices, and the per-group append-only
//! change log. Everything stored here is opaque to the relay — the change
//! `envelope` is ciphertext, the device public key is only used to verify auth
//! signatures, never to read content.
//!
//! The [`Storage`] trait isolates the backing store so [`SqliteStorage`]
//! (durable) and [`InMemoryStorage`] (tests, ephemeral) are interchangeable
//! without touching the HTTP/WS layer.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use rand::RngCore;
use rand::rngs::OsRng;
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("invite not found or already used")]
    InvalidInvite,
    #[error("group not found")]
    GroupNotFound,
}

/// A registered device: its transport credential (Ed25519 public key) and the
/// group it belongs to.
#[derive(Debug, Clone)]
pub struct Device {
    pub id: String,
    pub group_id: String,
    pub pubkey: Vec<u8>,
}

/// One entry in a group's append-only log. `envelope` is the base64 ciphertext
/// exactly as the client sent it. `change_id` is the client-supplied dedup key
/// (the Automerge change hash) — an opaque identifier the relay never interprets.
#[derive(Debug, Clone)]
pub struct StoredChange {
    pub seq: u64,
    pub device_id: String,
    pub envelope: String,
}

pub trait Storage: Send + Sync {
    /// Create a new group and return `(group_id, first_invite_code)`.
    fn create_group(&self) -> (String, String);
    /// Mint another single-use invite for an existing group.
    fn create_invite(&self, group_id: &str) -> Result<String, StorageError>;
    /// Consume an invite, registering a device. Returns `(device_id, group_id)`.
    fn enroll(&self, invite_code: &str, pubkey: Vec<u8>) -> Result<(String, String), StorageError>;
    /// Look up a device by id (for auth).
    fn device(&self, device_id: &str) -> Option<Device>;
    /// Append an encrypted change, assigning the next per-group `seq`. Idempotent
    /// per `change_id`: a repeat returns the original `seq` and `is_new = false`,
    /// so a client retrying (or restarting) never duplicates the log.
    fn append_change(
        &self,
        group_id: &str,
        device_id: &str,
        change_id: &str,
        envelope: String,
    ) -> (u64, bool);
    /// Every change with `seq > since_seq`, in order.
    fn changes_since(&self, group_id: &str, since_seq: u64) -> Vec<StoredChange>;
    /// The highest `seq` stored for a group (0 if none).
    fn current_seq(&self, group_id: &str) -> u64;
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    OsRng.fill_bytes(&mut buf);
    hex::encode(buf)
}

#[derive(Default)]
struct Inner {
    groups: HashSet<String>,
    invites: HashMap<String, String>, // code -> group_id (present == unused)
    devices: HashMap<String, Device>,
    logs: HashMap<String, Vec<StoredChange>>,
    seen: HashMap<String, HashMap<String, u64>>, // group_id -> (change_id -> seq)
}

#[derive(Default)]
pub struct InMemoryStorage {
    inner: Mutex<Inner>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Storage for InMemoryStorage {
    fn create_group(&self) -> (String, String) {
        let group_id = random_hex(16);
        let code = random_hex(24);
        let mut inner = self.inner.lock().unwrap();
        inner.groups.insert(group_id.clone());
        inner.invites.insert(code.clone(), group_id.clone());
        inner.logs.entry(group_id.clone()).or_default();
        (group_id, code)
    }

    fn create_invite(&self, group_id: &str) -> Result<String, StorageError> {
        let mut inner = self.inner.lock().unwrap();
        if !inner.groups.contains(group_id) {
            return Err(StorageError::GroupNotFound);
        }
        let code = random_hex(24);
        inner.invites.insert(code.clone(), group_id.to_string());
        Ok(code)
    }

    fn enroll(&self, invite_code: &str, pubkey: Vec<u8>) -> Result<(String, String), StorageError> {
        let mut inner = self.inner.lock().unwrap();
        let group_id = inner
            .invites
            .remove(invite_code)
            .ok_or(StorageError::InvalidInvite)?;
        let device_id = random_hex(16);
        inner.devices.insert(
            device_id.clone(),
            Device {
                id: device_id.clone(),
                group_id: group_id.clone(),
                pubkey,
            },
        );
        Ok((device_id, group_id))
    }

    fn device(&self, device_id: &str) -> Option<Device> {
        self.inner.lock().unwrap().devices.get(device_id).cloned()
    }

    fn append_change(
        &self,
        group_id: &str,
        device_id: &str,
        change_id: &str,
        envelope: String,
    ) -> (u64, bool) {
        let mut inner = self.inner.lock().unwrap();
        if let Some(&seq) = inner.seen.get(group_id).and_then(|m| m.get(change_id)) {
            return (seq, false);
        }
        let log = inner.logs.entry(group_id.to_string()).or_default();
        let seq = log.len() as u64 + 1;
        log.push(StoredChange {
            seq,
            device_id: device_id.to_string(),
            envelope,
        });
        inner
            .seen
            .entry(group_id.to_string())
            .or_default()
            .insert(change_id.to_string(), seq);
        (seq, true)
    }

    fn changes_since(&self, group_id: &str, since_seq: u64) -> Vec<StoredChange> {
        let inner = self.inner.lock().unwrap();
        inner
            .logs
            .get(group_id)
            .map(|log| log.iter().filter(|c| c.seq > since_seq).cloned().collect())
            .unwrap_or_default()
    }

    fn current_seq(&self, group_id: &str) -> u64 {
        self.inner
            .lock()
            .unwrap()
            .logs
            .get(group_id)
            .map(|l| l.len() as u64)
            .unwrap_or(0)
    }
}

/// Durable SQLite-backed storage. A single connection behind a `Mutex` — relay
/// concurrency is low and the trait methods are synchronous and fast.
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// Open (creating if needed) a store at `path`. Use `":memory:"` for an
    /// ephemeral store. Runs the idempotent schema at open.
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;
             CREATE TABLE IF NOT EXISTS groups (id TEXT PRIMARY KEY);
             CREATE TABLE IF NOT EXISTS invites (
                 code TEXT PRIMARY KEY,
                 group_id TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS devices (
                 id TEXT PRIMARY KEY,
                 group_id TEXT NOT NULL,
                 pubkey BLOB NOT NULL
             );
             CREATE TABLE IF NOT EXISTS changes (
                 group_id TEXT NOT NULL,
                 seq INTEGER NOT NULL,
                 device_id TEXT NOT NULL,
                 change_id TEXT NOT NULL,
                 envelope TEXT NOT NULL,
                 PRIMARY KEY (group_id, seq),
                 UNIQUE (group_id, change_id)
             );",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl Storage for SqliteStorage {
    fn create_group(&self) -> (String, String) {
        let group_id = random_hex(16);
        let code = random_hex(24);
        let conn = self.conn.lock().unwrap();
        conn.execute("INSERT INTO groups (id) VALUES (?1)", params![group_id])
            .expect("insert group");
        conn.execute(
            "INSERT INTO invites (code, group_id) VALUES (?1, ?2)",
            params![code, group_id],
        )
        .expect("insert invite");
        (group_id, code)
    }

    fn create_invite(&self, group_id: &str) -> Result<String, StorageError> {
        let conn = self.conn.lock().unwrap();
        let exists = conn
            .query_row(
                "SELECT 1 FROM groups WHERE id = ?1",
                params![group_id],
                |_| Ok(()),
            )
            .optional()
            .expect("query group")
            .is_some();
        if !exists {
            return Err(StorageError::GroupNotFound);
        }
        let code = random_hex(24);
        conn.execute(
            "INSERT INTO invites (code, group_id) VALUES (?1, ?2)",
            params![code, group_id],
        )
        .expect("insert invite");
        Ok(code)
    }

    fn enroll(&self, invite_code: &str, pubkey: Vec<u8>) -> Result<(String, String), StorageError> {
        let conn = self.conn.lock().unwrap();
        let group_id: Option<String> = conn
            .query_row(
                "SELECT group_id FROM invites WHERE code = ?1",
                params![invite_code],
                |r| r.get(0),
            )
            .optional()
            .expect("query invite");
        let group_id = group_id.ok_or(StorageError::InvalidInvite)?;
        conn.execute("DELETE FROM invites WHERE code = ?1", params![invite_code])
            .expect("delete invite");
        let device_id = random_hex(16);
        conn.execute(
            "INSERT INTO devices (id, group_id, pubkey) VALUES (?1, ?2, ?3)",
            params![device_id, group_id, pubkey],
        )
        .expect("insert device");
        Ok((device_id, group_id))
    }

    fn device(&self, device_id: &str) -> Option<Device> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, group_id, pubkey FROM devices WHERE id = ?1",
            params![device_id],
            |r| {
                Ok(Device {
                    id: r.get(0)?,
                    group_id: r.get(1)?,
                    pubkey: r.get(2)?,
                })
            },
        )
        .optional()
        .expect("query device")
    }

    fn append_change(
        &self,
        group_id: &str,
        device_id: &str,
        change_id: &str,
        envelope: String,
    ) -> (u64, bool) {
        let conn = self.conn.lock().unwrap();
        let existing: Option<i64> = conn
            .query_row(
                "SELECT seq FROM changes WHERE group_id = ?1 AND change_id = ?2",
                params![group_id, change_id],
                |r| r.get(0),
            )
            .optional()
            .expect("query dedup");
        if let Some(seq) = existing {
            return (seq as u64, false);
        }
        let next: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM changes WHERE group_id = ?1",
                params![group_id],
                |r| r.get(0),
            )
            .expect("next seq");
        conn.execute(
            "INSERT INTO changes (group_id, seq, device_id, change_id, envelope)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![group_id, next, device_id, change_id, envelope],
        )
        .expect("insert change");
        (next as u64, true)
    }

    fn changes_since(&self, group_id: &str, since_seq: u64) -> Vec<StoredChange> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT seq, device_id, envelope FROM changes
                 WHERE group_id = ?1 AND seq > ?2 ORDER BY seq",
            )
            .expect("prepare");
        let rows = stmt
            .query_map(params![group_id, since_seq as i64], |r| {
                Ok(StoredChange {
                    seq: r.get::<_, i64>(0)? as u64,
                    device_id: r.get(1)?,
                    envelope: r.get(2)?,
                })
            })
            .expect("query changes");
        rows.map(|r| r.expect("row")).collect()
    }

    fn current_seq(&self, group_id: &str) -> u64 {
        let conn = self.conn.lock().unwrap();
        let seq: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM changes WHERE group_id = ?1",
                params![group_id],
                |r| r.get(0),
            )
            .expect("current seq");
        seq as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enroll_consumes_invite() {
        let s = InMemoryStorage::new();
        let (group, code) = s.create_group();
        let (device, g2) = s.enroll(&code, vec![1, 2, 3]).unwrap();
        assert_eq!(group, g2);
        assert_eq!(s.device(&device).unwrap().group_id, group);
        // second use of the same invite fails
        assert!(s.enroll(&code, vec![4]).is_err());
    }

    #[test]
    fn log_is_ordered_and_filtered() {
        let s = InMemoryStorage::new();
        let (group, _) = s.create_group();
        assert_eq!(s.append_change(&group, "d1", "c1", "a".into()), (1, true));
        assert_eq!(s.append_change(&group, "d1", "c2", "b".into()), (2, true));
        assert_eq!(s.current_seq(&group), 2);
        let since1 = s.changes_since(&group, 1);
        assert_eq!(since1.len(), 1);
        assert_eq!(since1[0].seq, 2);
        assert_eq!(since1[0].envelope, "b");
    }

    #[test]
    fn append_is_idempotent_per_change_id() {
        let s = InMemoryStorage::new();
        let (group, _) = s.create_group();
        assert_eq!(s.append_change(&group, "d1", "c1", "a".into()), (1, true));
        // same change_id: no new entry, original seq returned
        assert_eq!(s.append_change(&group, "d1", "c1", "a".into()), (1, false));
        assert_eq!(s.current_seq(&group), 1);
    }

    #[test]
    fn invite_for_missing_group_errors() {
        let s = InMemoryStorage::new();
        assert!(s.create_invite("nope").is_err());
    }

    // The trait contract must hold identically for the SQLite backend.

    #[test]
    fn sqlite_enroll_and_log() {
        let s = SqliteStorage::open(":memory:").unwrap();
        let (group, code) = s.create_group();
        let (device, g2) = s.enroll(&code, vec![1, 2, 3]).unwrap();
        assert_eq!(group, g2);
        assert_eq!(s.device(&device).unwrap().pubkey, vec![1, 2, 3]);
        assert!(s.enroll(&code, vec![4]).is_err()); // invite consumed

        assert_eq!(
            s.append_change(&group, &device, "c1", "a".into()),
            (1, true)
        );
        assert_eq!(
            s.append_change(&group, &device, "c2", "b".into()),
            (2, true)
        );
        // idempotent per change_id
        assert_eq!(
            s.append_change(&group, &device, "c1", "a".into()),
            (1, false)
        );
        assert_eq!(s.current_seq(&group), 2);
        let since1 = s.changes_since(&group, 1);
        assert_eq!(since1.len(), 1);
        assert_eq!(since1[0].envelope, "b");
    }

    #[test]
    fn sqlite_invite_for_missing_group_errors() {
        let s = SqliteStorage::open(":memory:").unwrap();
        assert!(s.create_invite("nope").is_err());
    }

    #[test]
    fn sqlite_persists_across_reopen() {
        let mut path = std::env::temp_dir();
        path.push(format!("pn-relay-test-{}.db", std::process::id()));
        let p = path.to_str().unwrap();
        let _ = std::fs::remove_file(p);

        let (group, device);
        {
            let s = SqliteStorage::open(p).unwrap();
            let (g, code) = s.create_group();
            let (d, _) = s.enroll(&code, vec![9]).unwrap();
            s.append_change(&g, &d, "c1", "env".into());
            group = g;
            device = d;
        }
        {
            let s = SqliteStorage::open(p).unwrap();
            assert_eq!(s.current_seq(&group), 1);
            assert_eq!(s.device(&device).unwrap().group_id, group);
            assert_eq!(s.changes_since(&group, 0)[0].envelope, "env");
        }

        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{p}{suffix}"));
        }
    }
}
