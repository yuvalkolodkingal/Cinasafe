//! Key derivation. Everything here is a thin wrapper over RustCrypto crates.

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::blob::BlobId;
use crate::{Error, Result};

pub type Key32 = Zeroizing<[u8; 32]>;

pub const BLOB_KEY_INFO: &[u8] = b"cinasafe-blob-v1";

/// Per-blob key: `HKDF-SHA256(ikm = master_key, salt = vault_id, info = "cinasafe-blob-v1" || blob_id)`.
pub fn derive_blob_key(master_key: &[u8; 32], vault_id: &[u8; 16], blob_id: &BlobId) -> Key32 {
    let hk = Hkdf::<Sha256>::new(Some(vault_id), master_key);
    let mut info = [0u8; BLOB_KEY_INFO.len() + 16];
    info[..BLOB_KEY_INFO.len()].copy_from_slice(BLOB_KEY_INFO);
    info[BLOB_KEY_INFO.len()..].copy_from_slice(&blob_id.0);
    let mut okm = Zeroizing::new([0u8; 32]);
    hk.expand(&info, okm.as_mut()).expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).map_err(|_| Error::Rng)?;
    Ok(b)
}

pub fn random_key() -> Result<Key32> {
    let mut k = Zeroizing::new([0u8; 32]);
    getrandom::fill(k.as_mut()).map_err(|_| Error::Rng)?;
    Ok(k)
}

/// AAD prefix for key wraps: `"cinasafe-wrap-v1" || vault_id || kind`.
pub const WRAP_AAD_PREFIX: &[u8] = b"cinasafe-wrap-v1";
/// HKDF info for the recovery-key KEK.
pub const RECOVERY_INFO: &[u8] = b"cinasafe-recovery-v1";
/// A wrapped 32-byte key: ciphertext plus the 16-byte GCM tag.
pub const WRAPPED_LEN: usize = 48;

/// Argon2id parameters for turning a password into a KEK (brief 4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt: [u8; 16],
}

impl KdfParams {
    pub const DEFAULT_M_KIB: u32 = 256 * 1024;
    pub const DEFAULT_T: u32 = 3;
    pub const DEFAULT_P: u32 = 4;
    /// Readers refuse parameters outside these bounds, so a hostile
    /// `vault.json` can't make unlocking exhaust memory or run for hours.
    pub const MAX_M_KIB: u32 = 4 * 1024 * 1024;
    pub const MAX_T: u32 = 64;
    pub const MAX_P: u32 = 64;

    pub fn new_default() -> Result<KdfParams> {
        Ok(KdfParams { m_kib: Self::DEFAULT_M_KIB, t: Self::DEFAULT_T, p: Self::DEFAULT_P, salt: random_bytes()? })
    }

    pub fn validate(&self) -> Result<()> {
        let ok = (1..=Self::MAX_T).contains(&self.t)
            && (1..=Self::MAX_P).contains(&self.p)
            && self.m_kib >= 8 * self.p
            && self.m_kib <= Self::MAX_M_KIB;
        if ok {
            Ok(())
        } else {
            Err(Error::MalformedHeader("KDF parameters out of range"))
        }
    }
}

/// `Argon2id(password, salt, m, t, p) -> 32 bytes`.
pub fn password_kek(password: &[u8], params: &KdfParams) -> Result<Key32> {
    params.validate()?;
    let p = argon2::Params::new(params.m_kib, params.t, params.p, Some(32))
        .map_err(|_| Error::MalformedHeader("KDF parameters rejected"))?;
    let a = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p);
    let mut out = Zeroizing::new([0u8; 32]);
    a.hash_password_into(password, &params.salt, out.as_mut()).map_err(|_| Error::MalformedHeader("KDF failed"))?;
    Ok(out)
}

/// `HKDF-SHA256(ikm = entropy, salt = vault_id, info = "cinasafe-recovery-v1")`.
pub fn recovery_kek(entropy: &[u8; 32], vault_id: &[u8; 16]) -> Key32 {
    let hk = Hkdf::<Sha256>::new(Some(vault_id), entropy);
    let mut okm = Zeroizing::new([0u8; 32]);
    hk.expand(RECOVERY_INFO, okm.as_mut()).expect("32 bytes is a valid HKDF-SHA256 output length");
    okm
}

fn wrap_aad(vault_id: &[u8; 16], kind: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(WRAP_AAD_PREFIX.len() + 16 + kind.len());
    aad.extend_from_slice(WRAP_AAD_PREFIX);
    aad.extend_from_slice(vault_id);
    aad.extend_from_slice(kind.as_bytes());
    aad
}

/// Encrypts `key` under `kek` with AES-256-GCM. `kind` is `"password"` or `"recovery"`.
pub fn wrap_key(
    kek: &[u8; 32],
    vault_id: &[u8; 16],
    kind: &str,
    nonce: &[u8; 12],
    key: &[u8; 32],
) -> [u8; WRAPPED_LEN] {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let cipher = aes_gcm::Aes256Gcm::new(kek.into());
    let aad = wrap_aad(vault_id, kind);
    let ct = cipher
        .encrypt(aes_gcm::Nonce::from_slice(nonce), Payload { msg: key, aad: &aad })
        .expect("AES-GCM encryption of 32 bytes cannot fail");
    ct.try_into().expect("32-byte key + 16-byte tag")
}

/// Decrypts a wrapped key; `None` if the KEK is wrong or anything was altered.
pub fn unwrap_key(
    kek: &[u8; 32],
    vault_id: &[u8; 16],
    kind: &str,
    nonce: &[u8; 12],
    wrapped: &[u8; WRAPPED_LEN],
) -> Option<crate::secret::SecretKey> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let cipher = aes_gcm::Aes256Gcm::new(kek.into());
    let aad = wrap_aad(vault_id, kind);
    let pt =
        Zeroizing::new(cipher.decrypt(aes_gcm::Nonce::from_slice(nonce), Payload { msg: wrapped, aad: &aad }).ok()?);
    let bytes: [u8; 32] = pt.as_slice().try_into().ok()?;
    Some(crate::secret::SecretKey::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast_params() -> KdfParams {
        KdfParams { m_kib: 64, t: 1, p: 1, salt: [1; 16] }
    }

    #[test]
    fn wrap_roundtrip_and_binding() {
        let kek = [3u8; 32];
        let vid = [9u8; 16];
        let key = [5u8; 32];
        let nonce = [2u8; 12];
        let w = wrap_key(&kek, &vid, "password", &nonce, &key);
        assert_eq!(unwrap_key(&kek, &vid, "password", &nonce, &w).unwrap().expose(), &key);
        // Wrong KEK, wrong kind, wrong vault, flipped bit: all rejected.
        assert!(unwrap_key(&[4u8; 32], &vid, "password", &nonce, &w).is_none());
        assert!(unwrap_key(&kek, &vid, "recovery", &nonce, &w).is_none());
        assert!(unwrap_key(&kek, &[8u8; 16], "password", &nonce, &w).is_none());
        let mut bad = w;
        bad[0] ^= 1;
        assert!(unwrap_key(&kek, &vid, "password", &nonce, &bad).is_none());
    }

    #[test]
    fn password_kek_depends_on_everything() {
        let p = fast_params();
        let a = password_kek(b"hunter2", &p).unwrap();
        assert_eq!(*a, *password_kek(b"hunter2", &p).unwrap());
        assert_ne!(*a, *password_kek(b"hunter3", &p).unwrap());
        assert_ne!(*a, *password_kek(b"hunter2", &KdfParams { salt: [2; 16], ..p }).unwrap());
        assert_ne!(*a, *password_kek(b"hunter2", &KdfParams { t: 2, ..p }).unwrap());
    }

    #[test]
    fn rejects_hostile_params() {
        for bad in [
            KdfParams { m_kib: KdfParams::MAX_M_KIB + 1, ..fast_params() },
            KdfParams { t: 0, ..fast_params() },
            KdfParams { p: 0, ..fast_params() },
            KdfParams { m_kib: 4, p: 1, ..fast_params() },
        ] {
            assert!(password_kek(b"x", &bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn recovery_kek_is_hkdf() {
        let a = recovery_kek(&[1; 32], &[2; 16]);
        assert_ne!(*a, *recovery_kek(&[1; 32], &[3; 16]));
        assert_ne!(*a, *recovery_kek(&[4; 32], &[2; 16]));
    }
}
