# Cinasafe on-disk format, version 1

This document is the contract for the bytes on the stick. Changing anything here means bumping
the format version and saying so. Sections 1–2 are implemented in `cinasafe-core`
(`src/blob.rs`, `src/crypto.rs`) and pinned by test vectors. Section 3 is the brief's
specification, not implemented yet (Milestone 0 used a plaintext spike key file instead).

All multi-byte integers are little-endian unless marked big-endian. `||` is concatenation.

## 1. Per-blob key

```
blob_key = HKDF-SHA256(ikm = master_key (32 bytes),
                       salt = vault_id (16 bytes),
                       info = "cinasafe-blob-v1" || blob_id (16 bytes))   -> 32 bytes
```

`blob_id` is 16 random bytes, written as 32 lowercase hex characters in file names. The id of
16 zero bytes is reserved for the library database and is never assigned to anything else.

## 2. Blob container (`.cblob`)

Stored at `.cinasafe/blobs/<hex[0:2]>/<hex[2:4]>/<hex>.cblob`.

### Header (44 bytes)

| Offset | Size | Field |
| --- | --- | --- |
| 0 | 4 | Magic `CSFB` (ASCII) |
| 4 | 1 | Format version, `1` |
| 5 | 3 | Reserved, must be zero |
| 8 | 16 | Blob id |
| 24 | 8 | Plaintext size, u64 |
| 32 | 4 | Chunk size, u32 (default 1 048 576) |
| 36 | 8 | Nonce prefix, random per blob |

Readers reject: wrong magic, a version other than 1, non-zero reserved bytes, a chunk size of 0
or above 64 MiB (see OPEN_QUESTIONS Q8), more than 2³² chunks, and a blob id that differs from
the one requested.

### Chunks

`n = max(1, ceil(plaintext_size / chunk_size))`. A zero-length blob has one empty chunk.

Chunk `i` (0-based) holds plaintext bytes `[i·chunk_size, min((i+1)·chunk_size, plaintext_size))`
and is sealed with AES-256-GCM:

- key: `blob_key`
- nonce (12 bytes): `nonce_prefix || u32_be(i)`
- AAD (49 bytes): `header (44 bytes) || u32_be(i) || last` where `last` is `0x01` for chunk
  `n−1` and `0x00` otherwise
- stored as ciphertext followed by the 16-byte tag, at offset `44 + i·(chunk_size + 16)`

Sealed length is `44 + plaintext_size + 16·n`. Media blobs are then zero-padded to the next
multiple of 1 MiB; readers ignore anything after the last chunk. The library database blob is
not padded.

Binding the header, the index and the last-chunk flag into every chunk's AAD means a reader
detects chunk swapping, reordering, truncation, header edits and moving chunks
between blobs. Any chunk can be decrypted on its own, so seeking costs one chunk.

### Test vectors

`crates/cinasafe-core/tests/vectors/blob_vectors.json` lists inputs (master key, vault id,
blob id, nonce prefix, chunk size, plaintext) and the expected blob key. Each sits next to
its `.cblob` file. Two independent checks run against them:

- `cargo test -p cinasafe-core --test vectors` (the Rust implementation)
- `python3 -I crates/cinasafe-core/tests/vectors/verify_vectors.py` (Python `cryptography`, no
  Cinasafe code)

## 3. Vault header and keys (brief 4.2–4.3; not implemented yet)

- KEK from password: Argon2id, m = 262 144 KiB, t = 3, p = 4, 16-byte salt, 32-byte output.
- Master key: 32 bytes from the OS CSPRNG.
- Wrap: AES-256-GCM(KEK, master_key), AAD = `"cinasafe-wrap-v1" || vault_id || kind`.
- Recovery key: 24 BIP39 words (256 bits);
  KEK = HKDF-SHA256(ikm = entropy, salt = vault_id, info = "cinasafe-recovery-v1").
- `vault.json` layout as in brief 4.3. Byte-level details (string encoding of `kind` in the AAD,
  base64 variant) will be pinned here with vectors when the vault milestone implements them.
