//! Shared server state: the storage backend, the admin token, and the live
//! per-group broadcast channels used to fan a pushed change out to the other
//! connected devices of the same group.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::storage::Storage;

/// A change fanned out to a group's connected sessions. `origin` is the session
/// that pushed it, so that session can skip its own echo.
#[derive(Debug, Clone)]
pub struct Broadcast {
    pub seq: u64,
    pub device_id: String,
    pub envelope: String,
    pub origin: u64,
}

pub struct AppState {
    pub storage: Arc<dyn Storage>,
    pub admin_token: Option<String>,
    channels: Mutex<HashMap<String, broadcast::Sender<Broadcast>>>,
    session_counter: AtomicU64,
}

impl AppState {
    pub fn new(storage: Arc<dyn Storage>, admin_token: Option<String>) -> Arc<Self> {
        Arc::new(Self {
            storage,
            admin_token,
            channels: Mutex::new(HashMap::new()),
            session_counter: AtomicU64::new(0),
        })
    }

    /// A process-unique id for a WebSocket session.
    pub fn next_session_id(&self) -> u64 {
        self.session_counter.fetch_add(1, Ordering::Relaxed)
    }

    /// The broadcast sender for a group, created on first use.
    pub fn channel(&self, group_id: &str) -> broadcast::Sender<Broadcast> {
        let mut channels = self.channels.lock().unwrap();
        channels
            .entry(group_id.to_string())
            .or_insert_with(|| broadcast::channel(256).0)
            .clone()
    }
}
