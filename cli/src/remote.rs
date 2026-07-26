//! Relay enrollment and sync — the networked CLI commands.
//!
//! All functions take an explicit config path (and, for sync, a [`LocalStore`]),
//! so they can be driven in tests against an in-process relay with temp files.
//! They return data; `main` owns the printing.

use std::path::Path;

use anyhow::{Context, Result};
use ed25519_dalek::SigningKey;
use note_core::{GroupKey, sync_once};
use note_protocol::{
    CreateGroupResponse, CreateInviteRequest, CreateInviteResponse, DeviceListResponse,
    EnrollRequest, EnrollResponse,
};
use rand::RngCore;
use rand::rngs::OsRng;

use crate::config::{PairingBlob, Settings, decode_blob, encode_blob, encode_key};
use crate::store::LocalStore;

fn new_signing_key() -> ([u8; 32], SigningKey) {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    (seed, sk)
}

/// Create a new group on the relay, enroll this device, generate the E2E key,
/// persist the config, and return a pairing blob for a second device.
pub async fn init(cfg_path: &Path, relay: &str, admin: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let base = relay.trim_end_matches('/').to_string();

    let group: CreateGroupResponse = client
        .post(format!("{base}/v1/groups"))
        .bearer_auth(admin)
        .send()
        .await?
        .error_for_status()
        .context("creating group (check --admin token and --relay URL)")?
        .json()
        .await?;

    let (seed, sk) = new_signing_key();
    let enrolled = enroll(
        &client,
        &base,
        &group.invite_code,
        &sk.verifying_key().to_bytes(),
    )
    .await?;

    let e2e = GroupKey::generate();
    let settings = Settings {
        relay_url: base.clone(),
        group_id: enrolled.group_id.clone(),
        device_id: enrolled.device_id,
        signing_seed: encode_key(&seed),
        e2e_key: encode_key(e2e.as_bytes()),
        last_seq: 0,
    };
    settings.save_to(cfg_path)?;

    let invite: CreateInviteResponse = client
        .post(format!("{base}/v1/invites"))
        .bearer_auth(admin)
        .json(&CreateInviteRequest {
            group_id: enrolled.group_id.clone(),
        })
        .send()
        .await?
        .error_for_status()
        .context("minting pairing invite")?
        .json()
        .await?;

    let blob = PairingBlob {
        v: 1,
        relay_url: base,
        group_id: enrolled.group_id,
        invite_code: invite.invite_code,
        e2e_key: encode_key(e2e.as_bytes()),
    };
    encode_blob(&blob)
}

/// Join an existing group from a pairing blob (the shared E2E key travels in it).
pub async fn pair(cfg_path: &Path, blob_str: &str) -> Result<()> {
    let blob = decode_blob(blob_str)?;
    let client = reqwest::Client::new();
    let base = blob.relay_url.trim_end_matches('/').to_string();

    let (seed, sk) = new_signing_key();
    let enrolled = enroll(
        &client,
        &base,
        &blob.invite_code,
        &sk.verifying_key().to_bytes(),
    )
    .await?;

    let settings = Settings {
        relay_url: base,
        group_id: enrolled.group_id,
        device_id: enrolled.device_id,
        signing_seed: encode_key(&seed),
        e2e_key: blob.e2e_key,
        last_seq: 0,
    };
    settings.save_to(cfg_path)?;
    Ok(())
}

/// Push local changes and pull remote ones; persist the new high-water seq.
/// Returns that seq.
pub async fn sync(cfg_path: &Path, store: &LocalStore) -> Result<u64> {
    let mut settings = Settings::load_from(cfg_path)?;
    let cfg = settings.to_sync_config()?;
    let mut doc = store.load()?;
    let new_seq = sync_once(&cfg, &mut doc, settings.last_seq)
        .await
        .context("sync failed")?;
    store.save(&mut doc)?;
    settings.last_seq = new_seq;
    settings.save_to(cfg_path)?;
    Ok(new_seq)
}

/// List the group's devices as `(device_id, is_this_device)` (admin).
pub async fn devices(cfg_path: &Path, admin: &str) -> Result<Vec<(String, bool)>> {
    let s = Settings::load_from(cfg_path)?;
    let base = s.relay_url.trim_end_matches('/');
    let resp: DeviceListResponse = reqwest::Client::new()
        .get(format!("{base}/v1/devices"))
        .query(&[("group_id", s.group_id.as_str())])
        .bearer_auth(admin)
        .send()
        .await?
        .error_for_status()
        .context("listing devices")?
        .json()
        .await?;
    Ok(resp
        .devices
        .into_iter()
        .map(|d| {
            let is_self = d.id == s.device_id;
            (d.id, is_self)
        })
        .collect())
}

/// Revoke a device so it can no longer sync (admin).
pub async fn revoke(cfg_path: &Path, admin: &str, device_id: &str) -> Result<()> {
    let s = Settings::load_from(cfg_path)?;
    let base = s.relay_url.trim_end_matches('/');
    reqwest::Client::new()
        .delete(format!("{base}/v1/devices/{device_id}"))
        .bearer_auth(admin)
        .send()
        .await?
        .error_for_status()
        .context("revoking device")?;
    Ok(())
}

async fn enroll(
    client: &reqwest::Client,
    base: &str,
    invite_code: &str,
    pubkey: &[u8; 32],
) -> Result<EnrollResponse> {
    let req = EnrollRequest {
        invite_code: invite_code.to_string(),
        device_pubkey: encode_key(pubkey),
    };
    let resp = client
        .post(format!("{base}/v1/enroll"))
        .json(&req)
        .send()
        .await?
        .error_for_status()
        .context("enrolling device")?;
    Ok(resp.json().await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands;
    use note_relay::state::AppState;
    use note_relay::storage::InMemoryStorage;
    use std::sync::Arc;
    use tokio::net::TcpListener;

    async fn spawn_relay(admin: &str) -> String {
        let state = AppState::new(Arc::new(InMemoryStorage::new()), Some(admin.to_string()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { note_relay::serve(listener, state).await.unwrap() });
        format!("http://{addr}")
    }

    fn temp_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pn-cli-e2e-{}-{}",
            std::process::id(),
            crate::store::now_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn cli_sync_and_revoke_end_to_end() {
        let base = spawn_relay("adm").await;
        let dir = temp_dir();
        let cfg_a = dir.join("a.json");
        let cfg_b = dir.join("b.json");
        let store_a = LocalStore::new(dir.join("a.automerge"));
        let store_b = LocalStore::new(dir.join("b.automerge"));

        // A: init, create a note, sync (push).
        let blob = init(&cfg_a, &base, "adm").await.unwrap();
        commands::new_note(&store_a, 1, Some("Shared"), Some("work"), Some("hi")).unwrap();
        sync(&cfg_a, &store_a).await.unwrap();

        // B: pair, sync (pull) — sees A's note.
        pair(&cfg_b, &blob).await.unwrap();
        sync(&cfg_b, &store_b).await.unwrap();
        let seen = commands::list(&store_b, None, None).unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].title, "Shared");
        assert_eq!(seen[0].folder, "work");

        // Revoke B; its next sync is rejected.
        let b_device = Settings::load_from(&cfg_b).unwrap().device_id;
        revoke(&cfg_a, "adm", &b_device).await.unwrap();
        assert!(sync(&cfg_b, &store_b).await.is_err());

        // A still lists exactly itself.
        let ds = devices(&cfg_a, "adm").await.unwrap();
        assert!(ds.iter().any(|(_, me)| *me));
        assert!(!ds.iter().any(|(id, _)| id == &b_device));

        let _ = std::fs::remove_dir_all(dir);
    }
}
