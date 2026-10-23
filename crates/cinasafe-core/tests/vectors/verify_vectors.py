#!/usr/bin/env python3
"""Independent check of the format test vectors (docs/FORMAT.md).

Blobs: re-derives each blob key with HKDF-SHA256 and decrypts every chunk with
AES-256-GCM. vault.json: re-derives the password KEK with the reference
Argon2 (libargon2 via `argon2-cffi`) and the recovery KEK with HKDF, then
unwraps the master key. Uses Python `cryptography`, no Cinasafe code.

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


def check_vault(v):
    import base64

    from argon2.low_level import Type, hash_secret_raw

    f = json.loads((HERE / v["file"]).read_text())
    assert f["format"] == "cinasafe-vault" and f["version"] == 1, "format/version"
    vault_id = base64.b64decode(f["vault_id"])
    assert vault_id.hex() == v["vault_id"], "vault id"
    kdf = f["kdf"]
    assert (kdf["alg"], kdf["m_kib"], kdf["t"], kdf["p"]) == ("argon2id", v["kdf_m_kib"], v["kdf_t"], v["kdf_p"])
    salt = base64.b64decode(kdf["salt"])
    kek = hash_secret_raw(
        v["password"].encode(), salt, time_cost=kdf["t"], memory_cost=kdf["m_kib"],
        parallelism=kdf["p"], hash_len=32, type=Type.ID, version=19,
    )
    assert kek.hex() == v["password_kek"], "password KEK"
    wraps = {w["kind"]: w for w in f["wraps"]}

    def unwrap(kind, kek):
        w = wraps[kind]
        aad = b"cinasafe-wrap-v1" + vault_id + kind.encode()
        return AESGCM(kek).decrypt(base64.b64decode(w["nonce"]), base64.b64decode(w["ciphertext"]), aad)

    assert unwrap("password", kek).hex() == v["master_key"], "password unwrap"
    if v["recovery_entropy"]:
        entropy = bytes.fromhex(v["recovery_entropy"])
        rkek = HKDF(hashes.SHA256(), 32, vault_id, b"cinasafe-recovery-v1").derive(entropy)
        assert rkek.hex() == v["recovery_kek"], "recovery KEK"
        assert unwrap("recovery", rkek).hex() == v["master_key"], "recovery unwrap"
        assert len(v["recovery_words"].split()) == 24, "24 words"
    else:
        assert "recovery" not in wraps, "unexpected recovery wrap"


def main():
    for v in json.loads((HERE / "blob_vectors.json").read_text()):
        check(v)
        print(f"ok  blob   {v['name']}")
    for v in json.loads((HERE / "vault_vectors.json").read_text()):
        check_vault(v)
        print(f"ok  vault  {v['name']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
