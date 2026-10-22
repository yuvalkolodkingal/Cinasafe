#!/usr/bin/env python3
"""Independent check of the .cblob test vectors (docs/FORMAT.md section 4).

Re-derives each blob key with HKDF-SHA256 and decrypts every chunk with
AES-256-GCM using the Python `cryptography` package, without any Cinasafe code.

    python3 -I crates/cinasafe-core/tests/vectors/verify_vectors.py
"""
import json
import pathlib
import struct
import sys

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF

HERE = pathlib.Path(__file__).resolve().parent
HEADER_LEN = 44
TAG_LEN = 16


def check(v):
    master = bytes.fromhex(v["master_key"])
    vault_id = bytes.fromhex(v["vault_id"])
    blob_id = bytes.fromhex(v["blob_id"])
    key = HKDF(hashes.SHA256(), 32, vault_id, b"cinasafe-blob-v1" + blob_id).derive(master)
    assert key.hex() == v["blob_key"], "blob key"

    data = (HERE / v["file"]).read_bytes()
    header = data[:HEADER_LEN]
    magic, version, reserved, hid, size, chunk, prefix = struct.unpack("<4sB3s16sQI8s", header)
    assert magic == b"CSFB" and version == 1 and reserved == b"\0\0\0", "header fields"
    assert hid == blob_id and chunk == v["chunk_size"], "header ids"
    assert prefix.hex() == v["nonce_prefix"], "nonce prefix"

    count = max(1, -(-size // chunk))
    aead = AESGCM(key)
    out = b""
    for i in range(count):
        plain_len = min(chunk, size - i * chunk)
        off = HEADER_LEN + i * (chunk + TAG_LEN)
        sealed = data[off : off + plain_len + TAG_LEN]
        nonce = prefix + struct.pack(">I", i)
        aad = header + struct.pack(">I", i) + bytes([1 if i == count - 1 else 0])
        out += aead.decrypt(nonce, sealed, aad)
    assert len(data) == HEADER_LEN + size + TAG_LEN * count, "sealed length"
    assert out.hex() == v["plaintext"], "plaintext"


def main():
    vectors = json.loads((HERE / "blob_vectors.json").read_text())
    for v in vectors:
        check(v)
        print(f"ok  {v['name']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
