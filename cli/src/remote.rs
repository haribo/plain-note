//! Relay enrollment and sync — the networked CLI commands.

use anyhow::{Context, Result};
use ed25519_dalek::SigningKey;
use note_core::{GroupKey, sync_once};
use note_protocol::{
    CreateGroupResponse, CreateInviteRequest, CreateInviteResponse, EnrollRequest, EnrollResponse,
};
use rand::RngCore;
use rand::rngs::OsRng;

use crate::config::{PairingBlob, Settings, decode_blob, encode_blob, encode_key};
use crate::store;

fn new_signing_key() -> ([u8; 32], SigningKey) {
    let mut seed = [0u8; 32];
    OsRng.fill_bytes(&mut seed);
    let sk = SigningKey::from_bytes(&seed);
    (seed, sk)
}

/// Create a new group on the relay, enroll this device, generate the E2E key,
/// and print a pairing blob for a second device.
pub async fn init(relay: &str, admin: &str) -> Result<()> {
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
    settings.save()?;

    // An extra invite lets a second device join without the admin token.
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
    println!("Sync initialized. Pair another device with:\n");
    println!("  pn remote pair {}\n", encode_blob(&blob)?);
    Ok(())
}

/// Join an existing group from a pairing blob (the shared E2E key travels in it).
pub async fn pair(blob_str: &str) -> Result<()> {
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
    settings.save()?;
    println!("Paired. Run `pn sync`.");
    Ok(())
}

/// Push local changes and pull remote ones, then persist the new high-water seq.
pub async fn sync() -> Result<()> {
    let mut settings = Settings::load()?;
    let cfg = settings.to_sync_config()?;
    let mut s = store::load()?;
    let new_seq = sync_once(&cfg, &mut s, settings.last_seq)
        .await
        .context("sync failed")?;
    store::save(&mut s)?;
    settings.last_seq = new_seq;
    settings.save()?;
    println!("synced (seq {new_seq})");
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
