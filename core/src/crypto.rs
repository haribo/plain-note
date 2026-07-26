//! End-to-end encryption primitives.
//!
//! All content leaving a device is sealed here before it reaches the relay. The
//! relay never holds a key, so it can never open an envelope.
//!
//! - AEAD: XChaCha20-Poly1305. The 24-byte random nonce lets independent devices
//!   encrypt without coordinating a nonce counter — a hard requirement for a
//!   multi-device group where two devices may seal concurrently while offline.
//! - The E2E key is a random 32-byte symmetric key shared between the devices of
//!   a sync group via QR-code pairing; it never transits the relay.
//!
//! Wire layout and the AAD binding are specified in `docs/sync-protocol.md` § 2.

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use rand::rngs::OsRng;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Envelope format version, the first byte of every sealed blob.
pub const ENVELOPE_VERSION: u8 = 1;
/// XChaCha20-Poly1305 key length.
pub const KEY_LEN: usize = 32;
/// XChaCha20-Poly1305 nonce length.
pub const NONCE_LEN: usize = 24;
/// Fixed-size id (group, device) length.
pub const ID_LEN: usize = 16;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("key must be {KEY_LEN} bytes")]
    InvalidKeyLength,
    #[error("envelope shorter than its header")]
    TruncatedEnvelope,
    #[error("unsupported envelope version {0}")]
    UnsupportedVersion(u8),
    /// Wrong key, tampered ciphertext, or an AAD that does not match. The three
    /// are deliberately indistinguishable to the caller.
    #[error("authentication failed")]
    AuthenticationFailed,
}

/// What a sealed blob carries. Bound into the AAD so a ciphertext cannot be
/// reinterpreted as a different kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// An encrypted Automerge change.
    Change = 0x01,
    /// An encrypted attachment blob.
    Attachment = 0x02,
}

/// Associated data authenticated (not encrypted) alongside every envelope. It
/// binds a ciphertext to its group, its origin device, and its kind, so the
/// relay cannot replay a blob into another group or slot. Values that only exist
/// after sealing (relay-assigned `seq`, `attachment_id = hash(ciphertext)`) are
/// intentionally excluded — they are verified independently.
#[derive(Debug, Clone, Copy)]
pub struct Aad {
    pub group_id: [u8; ID_LEN],
    pub device_id: [u8; ID_LEN],
    pub kind: Kind,
}

impl Aad {
    fn to_bytes(self) -> [u8; ID_LEN * 2 + 1] {
        let mut out = [0u8; ID_LEN * 2 + 1];
        out[..ID_LEN].copy_from_slice(&self.group_id);
        out[ID_LEN] = self.kind as u8;
        out[ID_LEN + 1..].copy_from_slice(&self.device_id);
        out
    }
}

/// The 32-byte symmetric E2E key shared by a sync group. Zeroized on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct GroupKey([u8; KEY_LEN]);

impl GroupKey {
    /// Generate a fresh random group key from the OS CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_LEN];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Raw key bytes, e.g. to serialize into a QR pairing payload.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        // Length is guaranteed by the fixed-size array, so this never fails.
        XChaCha20Poly1305::new_from_slice(&self.0).expect("key length is fixed")
    }
}

/// Seal `plaintext` into an envelope: `version || nonce || ciphertext`.
pub fn seal(key: &GroupKey, aad: Aad, plaintext: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);

    let aad_bytes = aad.to_bytes();
    let ciphertext = key
        .cipher()
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad_bytes,
            },
        )
        .expect("XChaCha20-Poly1305 encryption is infallible for valid inputs");

    let mut envelope = Vec::with_capacity(1 + NONCE_LEN + ciphertext.len());
    envelope.push(ENVELOPE_VERSION);
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&ciphertext);
    envelope
}

/// Open an envelope produced by [`seal`] with the same key and AAD.
pub fn open(key: &GroupKey, aad: Aad, envelope: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let header = 1 + NONCE_LEN;
    if envelope.len() < header {
        return Err(CryptoError::TruncatedEnvelope);
    }
    let version = envelope[0];
    if version != ENVELOPE_VERSION {
        return Err(CryptoError::UnsupportedVersion(version));
    }
    let nonce = &envelope[1..header];
    let ciphertext = &envelope[header..];

    let aad_bytes = aad.to_bytes();
    key.cipher()
        .decrypt(
            XNonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad: &aad_bytes,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aad() -> Aad {
        Aad {
            group_id: [1u8; ID_LEN],
            device_id: [2u8; ID_LEN],
            kind: Kind::Change,
        }
    }

    #[test]
    fn roundtrip() {
        let key = GroupKey::generate();
        let msg = b"the quick brown fox";
        let env = seal(&key, aad(), msg);
        assert_eq!(env[0], ENVELOPE_VERSION);
        assert_eq!(open(&key, aad(), &env).unwrap(), msg);
    }

    #[test]
    fn empty_plaintext_roundtrips() {
        let key = GroupKey::generate();
        let env = seal(&key, aad(), b"");
        assert_eq!(open(&key, aad(), &env).unwrap(), b"");
    }

    #[test]
    fn wrong_key_fails() {
        let env = seal(&GroupKey::generate(), aad(), b"secret");
        let err = open(&GroupKey::generate(), aad(), &env).unwrap_err();
        assert_eq!(err, CryptoError::AuthenticationFailed);
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = GroupKey::generate();
        let mut env = seal(&key, aad(), b"secret");
        let last = env.len() - 1;
        env[last] ^= 0xff;
        assert_eq!(
            open(&key, aad(), &env).unwrap_err(),
            CryptoError::AuthenticationFailed
        );
    }

    #[test]
    fn mismatched_aad_fails() {
        let key = GroupKey::generate();
        let env = seal(&key, aad(), b"secret");

        let other_group = Aad {
            group_id: [9u8; ID_LEN],
            ..aad()
        };
        assert_eq!(
            open(&key, other_group, &env).unwrap_err(),
            CryptoError::AuthenticationFailed
        );

        let other_kind = Aad {
            kind: Kind::Attachment,
            ..aad()
        };
        assert_eq!(
            open(&key, other_kind, &env).unwrap_err(),
            CryptoError::AuthenticationFailed
        );
    }

    #[test]
    fn truncated_envelope_is_rejected() {
        let key = GroupKey::generate();
        assert_eq!(
            open(&key, aad(), &[ENVELOPE_VERSION; 4]).unwrap_err(),
            CryptoError::TruncatedEnvelope
        );
    }

    #[test]
    fn unknown_version_is_rejected() {
        let key = GroupKey::generate();
        let mut env = seal(&key, aad(), b"secret");
        env[0] = 0xfe;
        assert_eq!(
            open(&key, aad(), &env).unwrap_err(),
            CryptoError::UnsupportedVersion(0xfe)
        );
    }

    #[test]
    fn nonce_is_random_per_seal() {
        let key = GroupKey::generate();
        let a = seal(&key, aad(), b"same");
        let b = seal(&key, aad(), b"same");
        // Same plaintext, different nonce => different envelopes.
        assert_ne!(a, b);
    }
}
