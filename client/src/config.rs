//! Sync configuration: the relay endpoint, this device's credentials, the shared
//! E2E key, and the sync high-water mark. Persisted as JSON separate from the
//! note store.
//!
//! The E2E key and the device signing seed are secret; this file is written with
//! owner-only permissions where the platform supports it.

use std::env;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::SigningKey;
use note_core::{GroupKey, SyncConfig};
use serde::{Deserialize, Serialize};

/// Persisted sync settings. Absent until `remote init` or `remote pair` runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Relay base URL, e.g. `http://127.0.0.1:8787`.
    pub relay_url: String,
    pub group_id: String,
    pub device_id: String,
    /// base64 Ed25519 signing seed (32 bytes) — transport credential.
    pub signing_seed: String,
    /// base64 shared E2E key (32 bytes) — content encryption.
    pub e2e_key: String,
    /// Bearer token for authenticated HTTP calls (attachment upload/download).
    #[serde(default)]
    pub device_token: String,
    /// Highest relay `seq` already applied locally.
    #[serde(default)]
    pub last_seq: u64,
}

/// A pairing payload — the data a QR code carries to add a device to a group.
/// Base64-encoded JSON for copy-paste in the CLI, stand-in for the scanned code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingBlob {
    pub v: u16,
    pub relay_url: String,
    pub group_id: String,
    pub invite_code: String,
    /// base64 shared E2E key (32 bytes).
    pub e2e_key: String,
}

pub fn config_path() -> PathBuf {
    if let Ok(p) = env::var("PN_CONFIG") {
        return PathBuf::from(p);
    }
    let base = env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".config")
        });
    base.join("plain-note").join("config.json")
}

impl Settings {
    /// Load from an explicit path — the injection seam for tests.
    pub fn load_from(path: &std::path::Path) -> Result<Self> {
        let bytes = fs::read(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow!("no sync config — run `pn remote init` or `pn remote pair <blob>` first")
            } else {
                anyhow!("reading config at {}: {e}", path.display())
            }
        })?;
        serde_json::from_slice(&bytes).context("parsing config")
    }

    /// Save to an explicit path — the injection seam for tests.
    pub fn save_to(&self, path: &std::path::Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let json = serde_json::to_vec_pretty(self)?;
        fs::write(path, json).with_context(|| format!("writing config at {}", path.display()))?;
        restrict_permissions(path);
        Ok(())
    }

    /// Derive the WebSocket sync URL from the relay base URL.
    pub fn ws_url(&self) -> String {
        let ws = self
            .relay_url
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        format!("{}/v1/sync", ws.trim_end_matches('/'))
    }

    /// Build the `note-core` sync config from persisted secrets.
    pub fn to_sync_config(&self) -> Result<SyncConfig> {
        let seed = decode_32(&self.signing_seed).context("signing_seed")?;
        let key = decode_32(&self.e2e_key).context("e2e_key")?;
        Ok(SyncConfig {
            relay_url: self.ws_url(),
            group_id: self.group_id.clone(),
            device_id: self.device_id.clone(),
            signing_key: SigningKey::from_bytes(&seed),
            e2e_key: GroupKey::from_bytes(key),
        })
    }
}

fn decode_32(b64: &str) -> Result<[u8; 32]> {
    let v = B64.decode(b64.as_bytes())?;
    v.try_into().map_err(|_| anyhow!("expected 32 bytes"))
}

pub fn encode_key(bytes: &[u8]) -> String {
    B64.encode(bytes)
}

pub fn encode_blob(blob: &PairingBlob) -> Result<String> {
    Ok(B64.encode(serde_json::to_vec(blob)?))
}

pub fn decode_blob(s: &str) -> Result<PairingBlob> {
    let bytes = B64
        .decode(s.trim().as_bytes())
        .map_err(|_| anyhow!("pairing blob is not valid base64"))?;
    let blob: PairingBlob = serde_json::from_slice(&bytes).context("parsing pairing blob")?;
    if blob.v != 1 {
        bail!("unsupported pairing blob version {}", blob.v);
    }
    Ok(blob)
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn unique_tmp(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("pn-{tag}-{}-{n}.json", std::process::id()))
    }

    fn sample_settings() -> Settings {
        Settings {
            relay_url: "http://127.0.0.1:8787".into(),
            group_id: "g".into(),
            device_id: "d".into(),
            signing_seed: encode_key(&[1u8; 32]),
            e2e_key: encode_key(&[2u8; 32]),
            device_token: String::new(),
            last_seq: 0,
        }
    }

    #[cfg(unix)]
    #[test]
    fn save_to_writes_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let path = unique_tmp("perms");
        sample_settings().save_to(&path).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "secret config must be owner-only");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn ws_url_upgrades_scheme_and_appends_path() {
        let mut s = sample_settings();
        s.relay_url = "http://host:1/".into();
        assert_eq!(s.ws_url(), "ws://host:1/v1/sync");
        s.relay_url = "https://host".into();
        assert_eq!(s.ws_url(), "wss://host/v1/sync");
    }

    #[test]
    fn decode_32_rejects_wrong_length_and_non_base64() {
        assert!(decode_32(&encode_key(&[0u8; 16])).is_err()); // too short
        assert!(decode_32("not base64!!").is_err());
        assert!(decode_32(&encode_key(&[0u8; 32])).is_ok());
    }

    #[test]
    fn blob_round_trips_and_rejects_bad_version_and_garbage() {
        let blob = PairingBlob {
            v: 1,
            relay_url: "r".into(),
            group_id: "g".into(),
            invite_code: "i".into(),
            e2e_key: encode_key(&[3u8; 32]),
        };
        assert_eq!(
            decode_blob(&encode_blob(&blob).unwrap()).unwrap().group_id,
            "g"
        );
        let v2 = PairingBlob {
            v: 2,
            ..blob.clone()
        };
        assert!(decode_blob(&encode_blob(&v2).unwrap()).is_err()); // version gate
        assert!(decode_blob("@@@not base64@@@").is_err());
        assert!(decode_blob(&B64.encode(b"not json")).is_err());
    }

    #[test]
    fn load_from_missing_is_friendly_and_garbage_fails() {
        let missing = unique_tmp("missing");
        let err = Settings::load_from(&missing).unwrap_err();
        assert!(err.to_string().contains("no sync config"), "{err}");
        let path = unique_tmp("garbage");
        fs::write(&path, b"not json").unwrap();
        assert!(Settings::load_from(&path).is_err());
        let _ = fs::remove_file(&path);
    }
}
