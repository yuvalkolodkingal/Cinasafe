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
