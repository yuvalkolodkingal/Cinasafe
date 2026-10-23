use super::*;
use crate::library::{MovieMeta, NewMediaFile};

fn fast() -> KdfParams {
    KdfParams { m_kib: 64, t: 1, p: 1, salt: [7; 16] }
}

fn pw(s: &str) -> Password {
    Zeroizing::new(s.to_owned())
}

fn new_vault(with_recovery: bool) -> (tempfile::TempDir, Vault, Option<RecoveryWords>) {
    let dir = tempfile::tempdir().unwrap();
    let (v, w) = Vault::create_with_params(dir.path(), &pw("correct horse"), with_recovery, fast()).unwrap();
    (dir, v, w)
}

#[test]
fn create_unlock_and_wrong_password() {
    let (dir, v, words) = new_vault(true);
    assert_eq!(words.as_ref().unwrap().words().len(), 24);
    assert!(v.has_recovery());
    drop(v);
    let info = Vault::detect(dir.path()).unwrap();
    assert!(info.supported && info.has_recovery);
    assert!(Vault::unlock_with_password(dir.path(), &pw("correct horse")).is_ok());
    assert!(matches!(Vault::unlock_with_password(dir.path(), &pw("wrong")), Err(Error::WrongPassword)));
    assert!(matches!(Vault::create_with_params(dir.path(), &pw("x"), false, fast()), Err(Error::VaultExists)));
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(Vault::unlock_with_password(empty.path(), &pw("x")), Err(Error::NotAVault)));
    assert!(Vault::detect(empty.path()).is_none());
}

#[test]
fn recovery_words_unlock_and_can_be_replaced() {
    let (dir, v, words) = new_vault(true);
    let words = words.unwrap();
    // Case and spacing don't matter.
    let shouted = RecoveryWords::parse(&words.as_str().to_uppercase().replace(' ', "   ")).unwrap();
    assert!(Vault::unlock_with_recovery(dir.path(), &shouted).is_ok());

    let fresh = v.replace_recovery().unwrap();
    assert!(matches!(Vault::unlock_with_recovery(dir.path(), &words), Err(Error::WrongRecoveryKey)));
    assert!(Vault::unlock_with_recovery(dir.path(), &fresh).is_ok());

    v.remove_recovery().unwrap();
    assert!(matches!(Vault::unlock_with_recovery(dir.path(), &fresh), Err(Error::NoRecoveryKey)));
    assert!(matches!(RecoveryWords::parse("abandon abandon"), Err(Error::BadRecoveryWords)));
    // 24 valid words with a broken checksum.
    let mut bad: Vec<&str> = fresh.words();
    bad[23] = if bad[23] == "zoo" { "abandon" } else { "zoo" };
    assert!(RecoveryWords::parse(&bad.join(" ")).is_err());
}

#[test]
fn change_password_keeps_recovery_and_media() {
    let (dir, v, words) = new_vault(true);
    let id = v.write_blob(&b"hello"[..], 5, &BlobWriteOptions::default()).unwrap();
    assert!(v.check_password(&pw("correct horse")).unwrap());
    assert!(!v.check_password(&pw("nope")).unwrap());
    v.change_password_with_params(&pw("battery staple"), KdfParams { salt: [9; 16], ..fast() }).unwrap();
    drop(v);
    assert!(matches!(Vault::unlock_with_password(dir.path(), &pw("correct horse")), Err(Error::WrongPassword)));
    let v = Vault::unlock_with_password(dir.path(), &pw("battery staple")).unwrap();
    assert_eq!(&v.read_blob(id).unwrap()[..], b"hello");
    assert!(Vault::unlock_with_recovery(dir.path(), &words.unwrap()).is_ok());
}

#[test]
fn vault_json_matches_brief_and_leaks_nothing() {
    let (dir, v, _) = new_vault(false);
    let mf = v
        .library()
        .add_media_file(
            BlobId([3; 16]),
            &NewMediaFile { original_name: "Secret.Title.mkv".into(), ..Default::default() },
        )
        .unwrap();
    v.library().add_movie(mf, &MovieMeta { title: "Secret Title".into(), ..Default::default() }, None, None).unwrap();
    v.save_library().unwrap();
    let text = fs::read_to_string(dir.path().join(".cinasafe/vault.json")).unwrap();
    let j: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(j["format"], "cinasafe-vault");
    assert_eq!(j["version"], 1);
    assert_eq!(j["kdf"]["alg"], "argon2id");
    assert_eq!(j["wraps"][0]["kind"], "password");
    assert_eq!(b64().decode(j["wraps"][0]["ciphertext"].as_str().unwrap()).unwrap().len(), 48);
    assert_eq!(b64().decode(j["vault_id"].as_str().unwrap()).unwrap().len(), 16);
    for f in [".cinasafe/vault.json", ".cinasafe/library.db.enc"] {
        let raw = fs::read(dir.path().join(f)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"Secret"), "{f} leaks a title");
    }
}

#[test]
fn newer_vault_is_refused() {
    let (dir, v, _) = new_vault(false);
    drop(v);
    let p = dir.path().join(".cinasafe/vault.json");
    let mut j: serde_json::Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
    j["version"] = 2.into();
    fs::write(&p, j.to_string()).unwrap();
    assert!(!Vault::detect(dir.path()).unwrap().supported);
    assert!(matches!(Vault::unlock_with_password(dir.path(), &pw("correct horse")), Err(Error::VaultTooNew(2))));
}

#[test]
fn library_survives_and_falls_back_to_backup() {
    let (dir, v, _) = new_vault(false);
    let add = |v: &Vault, n: u8, t: &str| {
        let mf = v
            .library()
            .add_media_file(BlobId([n; 16]), &NewMediaFile { original_name: t.into(), ..Default::default() })
            .unwrap();
        v.library().add_movie(mf, &MovieMeta { title: t.into(), ..Default::default() }, None, None).unwrap();
        v.save_library().unwrap();
    };
    add(&v, 1, "First");
    add(&v, 2, "Second");
    drop(v);
    let titles = |v: &Vault| -> Vec<String> {
        v.library()
            .movies(Default::default(), None, crate::library::SortBy::Title)
            .unwrap()
            .into_iter()
            .map(|m| m.title)
            .collect()
    };
    let v = Vault::unlock_with_password(dir.path(), &pw("correct horse")).unwrap();
    assert_eq!(titles(&v), ["First", "Second"]);
    assert!(!v.library_restored_from_backup());
    drop(v);

    // Corrupt the current library: unlock uses the .bak (one save behind).
    let main = dir.path().join(".cinasafe/library.db.enc");
    let mut raw = fs::read(&main).unwrap();
    let last = raw.len() - 1;
    raw[last] ^= 0xff;
    fs::write(&main, raw).unwrap();
    let v = Vault::unlock_with_password(dir.path(), &pw("correct horse")).unwrap();
    assert!(v.library_restored_from_backup());
    assert_eq!(titles(&v), ["First"]);
    // The next save repairs it.
    v.save_library().unwrap();
    drop(v);
    let v = Vault::unlock_with_password(dir.path(), &pw("correct horse")).unwrap();
    assert!(!v.library_restored_from_backup());
    drop(v);

    // Both copies broken: a clear error, not an empty library.
    fs::write(&main, b"junk").unwrap();
    fs::write(main.with_extension("enc.bak"), b"junk").unwrap();
    assert!(matches!(Vault::unlock_with_password(dir.path(), &pw("correct horse")), Err(Error::LibraryUnreadable(_))));
}

#[test]
fn write_blob_verifies_and_cancels_cleanly() {
    let (_dir, v, _) = new_vault(false);
    let data: Vec<u8> = (0..3_500_000u32).map(|i| (i % 251) as u8).collect();
    let mut phases = Vec::new();
    let id = v
        .write_blob_with(&data[..], data.len() as u64, &BlobWriteOptions::media(), &mut |p, n| {
            phases.push((p, n));
            true
        })
        .unwrap();
    assert!(phases.iter().any(|(p, _)| *p == WritePhase::Encrypting));
    assert!(phases.iter().any(|(p, _)| *p == WritePhase::Verifying));
    assert_eq!(&v.read_blob(id).unwrap()[..], &data[..]);
    assert_eq!(fs::metadata(v.blob_path(id)).unwrap().len() % (1 << 20), 0, "media blobs are padded");

    let before = v.blobs_on_disk().unwrap().len();
    let r = v.write_blob_with(&data[..], data.len() as u64, &BlobWriteOptions::default(), &mut |_, n| n < 2_000_000);
    assert!(matches!(r, Err(Error::Cancelled)));
    assert_eq!(v.blobs_on_disk().unwrap().len(), before, "no partial blob left behind");
    let parts = walk_count(&v.dir().join("blobs"), ".part");
    assert_eq!(parts, 0);

    // Size lies are caught before anything is published.
    assert!(matches!(v.write_blob(&data[..10], 11, &BlobWriteOptions::default()), Err(Error::SizeMismatch { .. })));

    v.delete_blob(id).unwrap();
    assert!(matches!(v.open_blob(id), Err(Error::BlobMissing)));
    v.delete_blob(id).unwrap();
}

fn walk_count(dir: &Path, suffix: &str) -> usize {
    let mut n = 0;
    for e in fs::read_dir(dir).unwrap().flatten() {
        if e.file_type().unwrap().is_dir() {
            n += walk_count(&e.path(), suffix);
        } else if e.file_name().to_string_lossy().ends_with(suffix) {
            n += 1;
        }
    }
    n
}

#[test]
fn integrity_check_reports_problems() {
    let (_dir, v, _) = new_vault(false);
    let good = v.write_blob(&b"good"[..], 4, &BlobWriteOptions::default()).unwrap();
    let bad = v.write_blob(&[1u8; 5000][..], 5000, &BlobWriteOptions::default()).unwrap();
    let orphan = v.write_blob(&b"orphan"[..], 6, &BlobWriteOptions::default()).unwrap();
    for id in [good, bad] {
        v.library().add_media_file(id, &NewMediaFile { original_name: "x".into(), ..Default::default() }).unwrap();
    }
    v.library()
        .add_media_file(BlobId([0xee; 16]), &NewMediaFile { original_name: "gone".into(), ..Default::default() })
        .unwrap();

    let mut raw = fs::read(v.blob_path(bad)).unwrap();
    raw[HEADER_LEN + 10] ^= 1;
    fs::write(v.blob_path(bad), raw).unwrap();

    let mut ticks = 0;
    let r = v.integrity_check(|_| ticks += 1).unwrap();
    assert_eq!(r.checked, 3);
    assert_eq!(r.corrupt.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [bad]);
    assert_eq!(r.missing, [BlobId([0xee; 16])]);
    assert_eq!(r.orphaned, [orphan]);
    assert!(!r.is_clean());
    assert_eq!(ticks, 4);
    assert!(v.used_bytes().unwrap() > 0);
}
