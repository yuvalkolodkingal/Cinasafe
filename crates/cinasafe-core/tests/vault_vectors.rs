//! Format test vectors for `vault.json` (brief 4.2–4.3, 13). The fixed inputs
//! produce a byte-exact `vault.json`; `vectors/verify_vectors.py` re-derives
//! both KEKs and unwraps the master key with independent implementations
//! (libargon2 and Python `cryptography`).
//!
//! Regenerate after an intentional, versioned change:
//! `CINASAFE_REGEN_VECTORS=1 cargo test -p cinasafe-core --test vault_vectors`.

use std::path::PathBuf;

use cinasafe_core::crypto::{password_kek, recovery_kek, KdfParams};
use cinasafe_core::vault::{recovery_words_for, vault_json, HeaderInputs};
use cinasafe_core::{Password, RecoveryWords, Vault};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct VaultVector {
    name: String,
    password: String,
    master_key: String,
    vault_id: String,
    kdf_m_kib: u32,
    kdf_t: u32,
    kdf_p: u32,
    kdf_salt: String,
    password_nonce: String,
    password_kek: String,
    recovery_entropy: Option<String>,
    recovery_words: Option<String>,
    recovery_nonce: Option<String>,
    recovery_kek: Option<String>,
    created_at: String,
    file: String,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

fn build(name: &str, with_recovery: bool) -> (VaultVector, String) {
    let master: [u8; 32] = std::array::from_fn(|i| 0x40 + i as u8);
    let vault_id: [u8; 16] = std::array::from_fn(|i| 0xb0 + i as u8);
    // Small memory cost keeps the vector cheap; production uses 256 MiB, t=3, p=4.
    let params = KdfParams { m_kib: 1024, t: 2, p: 2, salt: std::array::from_fn(|i| 0x20 + i as u8) };
    let password = "correct horse battery staple — סיסמה";
    let password_nonce: [u8; 12] = std::array::from_fn(|i| 0x60 + i as u8);
    let entropy: [u8; 32] = std::array::from_fn(|i| 0x80 + i as u8);
    let recovery_nonce: [u8; 12] = std::array::from_fn(|i| 0x70 + i as u8);
    let created_at = "2026-10-22T14:00:00Z".to_string();
    let json = vault_json(&HeaderInputs {
        vault_id,
        master: &master,
        params,
        password: password.as_bytes(),
        password_nonce,
        recovery: with_recovery.then_some((entropy, recovery_nonce)),
        created_at: created_at.clone(),
        app_version: "0.1.0".into(),
    })
    .unwrap();
    let v = VaultVector {
        name: name.into(),
        password: password.into(),
        master_key: hex::encode(master),
        vault_id: hex::encode(vault_id),
        kdf_m_kib: params.m_kib,
        kdf_t: params.t,
        kdf_p: params.p,
        kdf_salt: hex::encode(params.salt),
        password_nonce: hex::encode(password_nonce),
        password_kek: hex::encode(*password_kek(password.as_bytes(), &params).unwrap()),
        recovery_entropy: with_recovery.then(|| hex::encode(entropy)),
        recovery_words: with_recovery.then(|| recovery_words_for(&entropy)),
        recovery_nonce: with_recovery.then(|| hex::encode(recovery_nonce)),
        recovery_kek: with_recovery.then(|| hex::encode(*recovery_kek(&entropy, &vault_id))),
        created_at,
        file: format!("{name}.vault.json"),
    };
    (v, json + "\n")
}

#[test]
fn vault_vectors() {
    let regen = std::env::var_os("CINASAFE_REGEN_VECTORS").is_some();
    let index = dir().join("vault_vectors.json");
    let built = vec![build("password-only", false), build("password-and-recovery", true)];
    if regen {
        for (v, json) in &built {
            std::fs::write(dir().join(&v.file), json).unwrap();
        }
        let vs: Vec<&VaultVector> = built.iter().map(|(v, _)| v).collect();
        std::fs::write(&index, serde_json::to_string_pretty(&vs).unwrap() + "\n").unwrap();
    }
    let stored: Vec<VaultVector> = serde_json::from_str(&std::fs::read_to_string(&index).unwrap()).unwrap();
    assert_eq!(stored.len(), built.len());
    for (s, (v, json)) in stored.iter().zip(&built) {
        assert_eq!(s, v, "vector metadata changed: {}", v.name);
        assert_eq!(&std::fs::read_to_string(dir().join(&s.file)).unwrap(), json, "vault.json changed: {}", v.name);

        // The checked-in file unlocks with the documented password and words.
        let root = tempfile::tempdir().unwrap();
        let vdir = root.path().join(".cinasafe");
        std::fs::create_dir_all(&vdir).unwrap();
        std::fs::write(vdir.join("vault.json"), json).unwrap();
        // No library yet: unlocking gets as far as reading it, which proves the key unwrapped.
        let err = Vault::unlock_with_password(root.path(), &Password::new(s.password.clone())).err().unwrap();
        assert!(matches!(err, cinasafe_core::Error::LibraryUnreadable(_)), "{err:?}");
        let wrong = Vault::unlock_with_password(root.path(), &Password::new("nope".into())).err().unwrap();
        assert!(matches!(wrong, cinasafe_core::Error::WrongPassword), "{wrong:?}");
        if let Some(words) = &s.recovery_words {
            let err = Vault::unlock_with_recovery(root.path(), &RecoveryWords::parse(words).unwrap()).err().unwrap();
            assert!(matches!(err, cinasafe_core::Error::LibraryUnreadable(_)), "{err:?}");
        }
    }
}
