//! Relay persistence: groups, invites, devices, and the per-group append-only
//! change log. Everything stored here is opaque to the relay — the change
//! `envelope` is ciphertext, the device public key is only used to verify auth
//! signatures, never to read content.
//!
//! The [`Storage`] trait isolates the backing store so an on-disk implementation
//! (SQLite) can replace [`InMemoryStorage`] without touching the HTTP/WS layer.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use rand::RngCore;
use rand::rngs::OsRng;

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
}
