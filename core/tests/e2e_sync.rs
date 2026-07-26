//! End-to-end encrypted sync: two independent note stores converge through a
//! real in-process relay, and the relay only ever holds ciphertext.

use std::sync::Arc;

use ed25519_dalek::SigningKey;
use note_core::{GroupKey, NoteStore, SyncConfig, sync_once};
use note_relay::state::AppState;
use note_relay::storage::{InMemoryStorage, Storage};
use rand::RngCore;
use rand::rngs::OsRng;

fn signing_key() -> SigningKey {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    SigningKey::from_bytes(&seed)
}

/// Bring up a relay, provision one group with two devices, and return the
/// WebSocket URL plus a ready `SyncConfig` per device (sharing one E2E key).
async fn setup() -> (String, SyncConfig, SyncConfig, Arc<InMemoryStorage>) {
    let storage = Arc::new(InMemoryStorage::new());

    let (group, code1) = storage.create_group();
    let sk1 = signing_key();
    let (dev1, _) = storage
        .enroll(&code1, sk1.verifying_key().to_bytes().to_vec())
        .unwrap();
    let code2 = storage.create_invite(&group).unwrap();
    let sk2 = signing_key();
    let (dev2, _) = storage
        .enroll(&code2, sk2.verifying_key().to_bytes().to_vec())
        .unwrap();

    let state = AppState::new(storage.clone(), None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { note_relay::serve(listener, state).await.unwrap() });
    let url = format!("ws://{addr}/v1/sync");

    // Both devices share the same E2E key — this is what QR pairing transfers.
    let e2e = GroupKey::generate();

    let cfg1 = SyncConfig {
        relay_url: url.clone(),
        group_id: group.clone(),
        device_id: dev1,
        signing_key: sk1,
        e2e_key: e2e.clone(),
    };
    let cfg2 = SyncConfig {
        relay_url: url.clone(),
        group_id: group,
        device_id: dev2,
        signing_key: sk2,
        e2e_key: e2e,
    };
    (url, cfg1, cfg2, storage)
}

#[tokio::test]
async fn two_stores_converge_through_relay() {
    let (_url, cfg_a, cfg_b, storage) = setup().await;

    // Device A creates and edits a note, then syncs (push).
    let mut a = NoteStore::new();
    let id = a.create_note(1000).unwrap();
    a.set_title(&id, "Shared", 1000).unwrap();
    a.replace_text(&id, "hello from A", 1010).unwrap();
    a.add_tag(&id, "sync", 1010).unwrap();
    let seq_a = sync_once(&cfg_a, &mut a, 0).await.unwrap();
    assert!(seq_a > 0);

    // Device B starts empty and syncs (pull) — it receives A's note.
    let mut b = NoteStore::new();
    let seq_b = sync_once(&cfg_b, &mut b, 0).await.unwrap();
    assert_eq!(seq_b, seq_a);
    assert_eq!(a.get_note(&id).unwrap(), b.get_note(&id).unwrap());
    let nb = b.get_note(&id).unwrap().unwrap();
    assert_eq!(nb.title, "Shared");
    assert_eq!(nb.text, "hello from A");
    assert_eq!(nb.tags, vec!["sync"]);

    // Device B edits and syncs; device A syncs and sees the change.
    b.add_tag(&id, "fromb", 1020).unwrap();
    let seq_b = sync_once(&cfg_b, &mut b, seq_b).await.unwrap();
    let _seq_a = sync_once(&cfg_a, &mut a, seq_a).await.unwrap();
    assert_eq!(
        a.get_note(&id).unwrap().unwrap().tags,
        vec!["fromb", "sync"]
    );
    assert_eq!(a.get_note(&id).unwrap(), b.get_note(&id).unwrap());
    let _ = seq_b;

    // The relay stores only ciphertext: no plaintext leaks into any envelope.
    for c in storage.changes_since(&cfg_a.group_id, 0) {
        let raw = base64_decode(&c.envelope);
        assert!(!contains(&raw, b"hello from A"));
        assert!(!contains(&raw, b"Shared"));
    }
}

fn base64_decode(s: &str) -> Vec<u8> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    STANDARD.decode(s.as_bytes()).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}
