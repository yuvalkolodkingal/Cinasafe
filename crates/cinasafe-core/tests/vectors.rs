//! Format test vectors for `.cblob` and the per-blob key derivation.
//!
//! The vectors pin the on-disk format. `vectors/verify_vectors.py` checks the
//! same files with an independent implementation (Python `cryptography`).
//! To regenerate after an intentional, versioned format change:
//! `CINASAFE_REGEN_VECTORS=1 cargo test -p cinasafe-core --test vectors`.

use std::io::Read;
use std::path::PathBuf;

use cinasafe_core::blob::{write_blob, BlobWriteOptions, DEFAULT_CHUNK_SIZE};
use cinasafe_core::crypto::derive_blob_key;
use cinasafe_core::{BlobId, BlobReader};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Vector {
    name: String,
    master_key: String,
    vault_id: String,
    blob_id: String,
    blob_key: String,
    nonce_prefix: String,
    chunk_size: u32,
    plaintext: String,
    file: String,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/vectors")
}

fn arr<const N: usize>(s: &str) -> [u8; N] {
    hex::decode(s).unwrap().try_into().unwrap()
}

fn cases() -> Vec<(&'static str, u32, Vec<u8>)> {
    vec![
        ("empty", 16, Vec::new()),
        ("two-full-chunks", 16, (0u8..32).collect()),
        ("partial-last-chunk", 16, b"Cinasafe test vector: three chunks".to_vec()),
        ("hebrew-utf8-default-chunk", DEFAULT_CHUNK_SIZE, "שלום, עולם! Hello".as_bytes().to_vec()),
    ]
}

fn build(name: &str, chunk_size: u32, plaintext: &[u8], idx: u8) -> (Vector, Vec<u8>) {
    let master_key: [u8; 32] = std::array::from_fn(|i| i as u8);
    let vault_id: [u8; 16] = std::array::from_fn(|i| 0xa0 + i as u8);
    let blob_id = BlobId(std::array::from_fn(|i| if i == 15 { idx } else { 0x10 + i as u8 }));
    let nonce_prefix: [u8; 8] = std::array::from_fn(|i| 0xf0 + i as u8 + idx);
    let key = derive_blob_key(&master_key, &vault_id, &blob_id);
    let mut sealed = Vec::new();
    let opts = BlobWriteOptions { chunk_size, pad: false, nonce_prefix: Some(nonce_prefix) };
    write_blob(&key, blob_id, plaintext.len() as u64, plaintext, &mut sealed, &opts).unwrap();
    let v = Vector {
        name: name.into(),
        master_key: hex::encode(master_key),
        vault_id: hex::encode(vault_id),
        blob_id: blob_id.to_hex(),
        blob_key: hex::encode(*key),
        nonce_prefix: hex::encode(nonce_prefix),
        chunk_size,
        plaintext: hex::encode(plaintext),
        file: format!("{name}.cblob"),
    };
    (v, sealed)
}

#[test]
fn blob_vectors() {
    let regen = std::env::var_os("CINASAFE_REGEN_VECTORS").is_some();
    let index_path = dir().join("blob_vectors.json");
    let mut built = Vec::new();
    for (i, (name, cs, pt)) in cases().into_iter().enumerate() {
        let (v, sealed) = build(name, cs, &pt, i as u8);
        if regen {
            std::fs::write(dir().join(&v.file), &sealed).unwrap();
        }
        built.push((v, sealed));
    }
    if regen {
        let vs: Vec<&Vector> = built.iter().map(|(v, _)| v).collect();
        std::fs::write(&index_path, serde_json::to_string_pretty(&vs).unwrap() + "\n").unwrap();
    }

    let stored: Vec<Vector> = serde_json::from_str(&std::fs::read_to_string(&index_path).unwrap()).unwrap();
    assert_eq!(stored.len(), built.len());
    for (s, (v, sealed)) in stored.iter().zip(&built) {
        assert_eq!(s, v, "vector metadata changed: {}", v.name);
        let on_disk = std::fs::read(dir().join(&s.file)).unwrap();
        assert_eq!(&on_disk, sealed, "sealed bytes changed: {}", v.name);

        // Decrypt the checked-in file from the documented inputs alone.
        let key = derive_blob_key(&arr(&s.master_key), &arr(&s.vault_id), &BlobId(arr(&s.blob_id)));
        assert_eq!(hex::encode(*key), s.blob_key);
        let mut r = BlobReader::open(on_disk, &key, BlobId(arr(&s.blob_id))).unwrap();
        let mut back = Vec::new();
        r.read_to_end(&mut back).unwrap();
        assert_eq!(hex::encode(back), s.plaintext);
    }
}
