# Cinasafe on-disk format, version 1

This document is the contract for the bytes on the stick. Changing anything here means bumping
the format version and saying so. Everything below is implemented in `cinasafe-core`
(`src/blob.rs`, `src/crypto.rs`, `src/vault.rs`, `src/library/`) and pinned by test vectors
that an independent Python implementation re-checks
(`crates/cinasafe-core/tests/vectors/verify_vectors.py`).

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

## 3. Vault header and keys

### 3.1 Drive layout

```
<stick root>/
├── Cinasafe-Windows.exe, Cinasafe-Linux-*.AppImage, run-linux.sh, README.txt
└── .cinasafe/
    ├── vault.json            header (below); no titles or user data
    ├── library.db.enc        encrypted SQLite library (blob format, id = 16 zero bytes)
    ├── library.db.enc.bak    previous library version
    └── blobs/ab/cd/<id>.cblob
```

While a blob is being written it is `<id>.cblob.part`; it is renamed only after every chunk
has been decrypted back and its plaintext SHA-256 matches what was read from the source.

### 3.2 Keys

| Key | Derivation |
| --- | --- |
| Password KEK | Argon2id v1.3, `m = 262144 KiB`, `t = 3`, `p = 4`, 16-byte random salt, 32-byte output. Parameters are stored in `vault.json`. Readers accept `1 ≤ t ≤ 64`, `1 ≤ p ≤ 64`, `8·p ≤ m ≤ 4 GiB` and refuse anything else. |
| Master key | 32 bytes from the OS CSPRNG, generated once. |
| Recovery KEK | `HKDF-SHA256(ikm = entropy, salt = vault_id, info = "cinasafe-recovery-v1")`, where `entropy` is the 32 bytes encoded by the 24 BIP39 English words. |
| Blob key | Section 1. |

### 3.3 Key wraps

`ciphertext = AES-256-GCM(key = KEK, nonce = random 12 bytes, plaintext = master key, AAD = "cinasafe-wrap-v1" || vault_id (16 bytes) || kind)`,
where `kind` is the ASCII string `password` or `recovery` (no terminator). The 48-byte result
is the 32-byte ciphertext followed by the 16-byte tag. Changing the password or recovery key
rewrites only its wrap (and, for the password, the KDF salt and parameters).

### 3.4 `vault.json`

UTF-8 JSON as in brief 4.3. Binary fields are standard base64 with padding. `created_at` is
RFC 3339 UTC with second precision. `wraps` holds at most one entry per `kind`, `password`
first. Unknown fields are ignored. A `format` other than `cinasafe-vault` means "not a vault".
A `version` above 1 means "Update Cinasafe to open this vault", and nothing is opened. Example
from the vectors: `tests/vectors/password-and-recovery.vault.json`.

### 3.5 Library database

The library is an SQLite database (schema: `crates/cinasafe-core/src/library/schema.sql`,
`meta.schema_version = 1`), serialised with `sqlite3_serialize` and stored as a blob with the
reserved id of 16 zero bytes, unpadded, in `library.db.enc`. Saving writes
`library.db.enc.tmp`, fsyncs it, renames the current file to `library.db.enc.bak`, then
renames the temp file into place. Unlocking falls back to `.bak` when the main file fails to
decrypt, and says so. A newer `schema_version` is treated like a newer vault version.

### 3.6 Test vectors

`tests/vectors/vault_vectors.json` lists fixed inputs: password (non-ASCII on purpose),
master key, vault id, KDF parameters (a cheap `m = 1024 KiB, t = 2, p = 2` so the vectors run
fast), nonces, recovery entropy and words, plus both expected KEKs. They sit next to the exact
`vault.json` each produces. The Python check derives the password KEK with libargon2 (the
reference implementation) and unwraps the master key with both KEKs.
