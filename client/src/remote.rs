//! Relay enrollment and sync — the networked CLI commands.
//!
//! All functions take an explicit config path (and, for sync, a [`LocalStore`]),
//! so they can be driven in tests against an in-process relay with temp files.
//! They return data; `main` owns the printing.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use ed25519_dalek::SigningKey;
use note_core::{GroupKey, Timestamp, attachment_id, open_attachment, seal_attachment, sync_once};
use note_protocol::{
    CreateGroupResponse, CreateInviteRequest, CreateInviteResponse, DeviceListResponse,
    EnrollRequest, EnrollResponse,
};
use rand::RngCore;
use rand::rngs::OsRng;

use crate::config::{PairingBlob, Settings, decode_blob, encode_blob, encode_key};
use crate::store::{LocalStore, resolve_id};

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
        device_token: enrolled.device_token,
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
        device_token: enrolled.device_token,
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
    // Sync a snapshot without holding the store lock across the network, then
    // merge the result back under an exclusive lock. Concurrent local edits made
    // during the sync are preserved by the CRDT merge, not clobbered.
    let mut doc = store.load()?;
    let new_seq = sync_once(&cfg, &mut doc, settings.last_seq)
        .await
        .context("sync failed")?;
    store.update(|disk| {
        disk.merge(&mut doc)?;
        Ok(())
    })?;
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

/// Encrypt a file, upload the blob, and reference it from a note. Returns the
/// attachment id.
pub async fn attach(
    cfg_path: &Path,
    store: &LocalStore,
    now: Timestamp,
    note_prefix: &str,
    file: &Path,
) -> Result<String> {
    let settings = Settings::load_from(cfg_path)?;
    let sc = settings.to_sync_config()?;
    let group = id16(&sc.group_id)?;
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow!("invalid file name"))?
        .to_string();
    let data = std::fs::read(file).with_context(|| format!("reading {}", file.display()))?;

    let envelope = seal_attachment(&sc.e2e_key, group, &data);
    let id = attachment_id(&envelope);

    let base = settings.relay_url.trim_end_matches('/');
    reqwest::Client::new()
        .put(format!("{base}/v1/attachments/{id}"))
        .bearer_auth(&settings.device_token)
        .body(envelope)
        .send()
        .await?
        .error_for_status()
        .context("uploading attachment")?;

    store.update(|doc| {
        let note = resolve_id(doc, note_prefix)?;
        doc.add_attachment(&note, &id, &filename, now)?;
        Ok(())
    })?;
    Ok(id)
}

/// Download and decrypt a note's attachment to a file. Returns the written path.
pub async fn fetch(
    cfg_path: &Path,
    store: &LocalStore,
    note_prefix: &str,
    att_prefix: &str,
    out: Option<PathBuf>,
) -> Result<PathBuf> {
    let settings = Settings::load_from(cfg_path)?;
    let sc = settings.to_sync_config()?;
    let group = id16(&sc.group_id)?;

    let (id, filename) = store.read(|doc| {
        let note = resolve_id(doc, note_prefix)?;
        let n = doc
            .get_note(&note)?
            .ok_or_else(|| anyhow!("note not found"))?;
        let mut matches = n
            .attachments
            .into_iter()
            .filter(|(id, _)| id.starts_with(att_prefix));
        match (matches.next(), matches.next()) {
            (Some(a), None) => Ok(a),
            (None, _) => Err(anyhow!("no attachment matches '{att_prefix}'")),
            (Some(_), Some(_)) => Err(anyhow!("attachment id '{att_prefix}' is ambiguous")),
        }
    })?;

    let base = settings.relay_url.trim_end_matches('/');
    let envelope = reqwest::Client::new()
        .get(format!("{base}/v1/attachments/{id}"))
        .bearer_auth(&settings.device_token)
        .send()
        .await?
        .error_for_status()
        .context("downloading attachment")?
        .bytes()
        .await?;

    let plaintext =
        open_attachment(&sc.e2e_key, group, &envelope).context("decrypting attachment")?;
    let path = out.unwrap_or_else(|| PathBuf::from(&filename));
    std::fs::write(&path, plaintext).with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}

fn id16(hex_str: &str) -> Result<[u8; 16]> {
    let v = hex::decode(hex_str).map_err(|_| anyhow!("invalid group id"))?;
    v.try_into()
        .map_err(|_| anyhow!("group id must be 16 bytes"))
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
        use std::sync::atomic::{AtomicU64, Ordering};
        static CTR: AtomicU64 = AtomicU64::new(0);
        // Counter guarantees a distinct dir per test even under parallel runs.
        let dir = std::env::temp_dir().join(format!(
            "pn-cli-e2e-{}-{}",
            std::process::id(),
            CTR.fetch_add(1, Ordering::Relaxed)
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

        // A: init, create a folder + note in it, sync (push).
        let blob = init(&cfg_a, &base, "adm").await.unwrap();
        let work = commands::create_folder(&store_a, 1, "work", None).unwrap();
        commands::new_note(&store_a, 1, Some("Shared"), Some(work.as_str()), Some("hi")).unwrap();
        sync(&cfg_a, &store_a).await.unwrap();

        // B: pair, sync (pull) — sees A's note and folder.
        pair(&cfg_b, &blob).await.unwrap();
        sync(&cfg_b, &store_b).await.unwrap();
        let seen = commands::list(&store_b, None, None).unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].title, "Shared");
        assert_eq!(
            commands::folder_path(&store_b, &seen[0].folder).unwrap(),
            "work"
        );

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

    #[tokio::test]
    async fn cli_attachment_round_trip() {
        let base = spawn_relay("adm").await;
        let dir = temp_dir();
        let cfg_a = dir.join("a.json");
        let cfg_b = dir.join("b.json");
        let store_a = LocalStore::new(dir.join("a.automerge"));
        let store_b = LocalStore::new(dir.join("b.automerge"));

        // A: create a note, attach a binary file, sync.
        let blob = init(&cfg_a, &base, "adm").await.unwrap();
        let note = commands::new_note(&store_a, 1, Some("WithImage"), None, None).unwrap();
        let src = dir.join("photo.bin");
        let contents = b"\x00\x01\x02 binary attachment payload \xff".to_vec();
        std::fs::write(&src, &contents).unwrap();
        let aid = attach(&cfg_a, &store_a, 2, note.as_str(), &src)
            .await
            .unwrap();
        sync(&cfg_a, &store_a).await.unwrap();

        // B: pair, sync (gets the ref via CRDT), then download + decrypt the blob.
        pair(&cfg_b, &blob).await.unwrap();
        sync(&cfg_b, &store_b).await.unwrap();
        let atts = commands::attachments(&store_b, note.as_str()).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].0, aid);
        assert_eq!(atts[0].1, "photo.bin");

        let out = dir.join("fetched.bin");
        fetch(
            &cfg_b,
            &store_b,
            note.as_str(),
            &aid[..8],
            Some(out.clone()),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), contents);

        let _ = std::fs::remove_dir_all(dir);
    }
}
