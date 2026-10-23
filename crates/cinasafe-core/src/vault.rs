//! A Cinasafe vault on a stick: `vault.json`, the encrypted library and the
//! blobs (brief sections 4 and 5.2).

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::blob::{write_blob, BlobWriteOptions, HEADER_LEN};
use crate::crypto::{self, derive_blob_key, random_bytes, KdfParams, WRAPPED_LEN};
use crate::library::Library;
use crate::secret::{Password, SecretKey};
use crate::{BlobId, BlobReader, Error, Header, Result};

pub const VAULT_DIR: &str = ".cinasafe";
pub const VAULT_FILE: &str = "vault.json";
pub const LIBRARY_FILE: &str = "library.db.enc";
pub const FORMAT_NAME: &str = "cinasafe-vault";
/// Newest `vault.json` version this build reads and writes.
pub const VAULT_VERSION: u32 = 1;
const KIND_PASSWORD: &str = "password";
const KIND_RECOVERY: &str = "recovery";

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn b64_fixed<const N: usize>(s: &str, what: &'static str) -> Result<[u8; N]> {
    let v = Zeroizing::new(b64().decode(s).map_err(|_| Error::MalformedHeader(what))?);
    v.as_slice().try_into().map_err(|_| Error::MalformedHeader(what))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct KdfJson {
    alg: String,
    m_kib: u32,
    t: u32,
    p: u32,
    salt: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct WrapJson {
    kind: String,
    nonce: String,
    ciphertext: String,
}

/// `vault.json` exactly as in brief 4.3. Holds no titles or user data.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct VaultFile {
    format: String,
    version: u32,
    vault_id: String,
    created_at: String,
    app_version: String,
    kdf: KdfJson,
    wraps: Vec<WrapJson>,
}

impl VaultFile {
    fn kdf_params(&self) -> Result<KdfParams> {
        if self.kdf.alg != "argon2id" {
            return Err(Error::MalformedHeader("unsupported KDF"));
        }
        let p = KdfParams {
            m_kib: self.kdf.m_kib,
            t: self.kdf.t,
            p: self.kdf.p,
            salt: b64_fixed(&self.kdf.salt, "kdf salt")?,
        };
        p.validate()?;
        Ok(p)
    }

    fn set_kdf(&mut self, p: &KdfParams) {
        self.kdf = KdfJson { alg: "argon2id".into(), m_kib: p.m_kib, t: p.t, p: p.p, salt: b64().encode(p.salt) };
    }

    fn wrap(&self, kind: &str) -> Option<&WrapJson> {
        self.wraps.iter().find(|w| w.kind == kind)
    }

    fn set_wrap(&mut self, kind: &str, nonce: &[u8; 12], ct: &[u8; WRAPPED_LEN]) {
        self.wraps.retain(|w| w.kind != kind);
        self.wraps.push(WrapJson { kind: kind.into(), nonce: b64().encode(nonce), ciphertext: b64().encode(ct) });
        // Keep "password" first for readability.
        self.wraps.sort_by_key(|w| w.kind != KIND_PASSWORD);
    }
}

/// The 24 recovery words. Zeroized when dropped.
pub struct RecoveryWords(Zeroizing<String>);

impl RecoveryWords {
    fn from_entropy(entropy: &[u8; 32]) -> RecoveryWords {
        let m = bip39::Mnemonic::from_entropy(entropy).expect("32 bytes is valid BIP39 entropy");
        RecoveryWords(Zeroizing::new(m.words().collect::<Vec<_>>().join(" ")))
    }

    /// Parses 24 English BIP39 words (any case and spacing), checking the checksum.
    pub fn parse(s: &str) -> Result<RecoveryWords> {
        let normalized = Zeroizing::new(s.split_whitespace().map(str::to_lowercase).collect::<Vec<_>>().join(" "));
        let m = bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &normalized)
            .map_err(|_| Error::BadRecoveryWords)?;
        if m.word_count() != 24 {
            return Err(Error::BadRecoveryWords);
        }
        Ok(RecoveryWords(normalized))
    }

    fn entropy(&self) -> Zeroizing<[u8; 32]> {
        let m =
            bip39::Mnemonic::parse_in_normalized(bip39::Language::English, &self.0).expect("validated on construction");
        let (arr, len) = m.to_entropy_array();
        let mut out = Zeroizing::new([0u8; 32]);
        out.copy_from_slice(&arr[..len.min(32)]);
        let mut arr = arr;
        arr.zeroize();
        out
    }

    pub fn words(&self) -> Vec<&str> {
        self.0.split(' ').collect()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for RecoveryWords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryWords(..)")
    }
}

/// What `detect` finds at a path, without unlocking anything.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub version: u32,
    pub created_at: String,
    /// False when the vault was made by a newer Cinasafe.
    pub supported: bool,
    pub has_recovery: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WritePhase {
    Encrypting,
    Verifying,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityProgress {
    pub done: u64,
    pub total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub checked: u64,
    pub bytes: u64,
    /// Blobs that failed authentication (corrupt or tampered), with the reason.
    #[serde(skip)]
    pub corrupt: Vec<(BlobId, String)>,
    pub corrupt_count: usize,
    /// Referenced by the library but not on the stick.
    #[serde(skip)]
    pub missing: Vec<BlobId>,
    pub missing_count: usize,
    /// On the stick but referenced by nothing (e.g. an interrupted import).
    #[serde(skip)]
    pub orphaned: Vec<BlobId>,
    pub orphaned_count: usize,
    pub library_restored_from_backup: bool,
}

impl IntegrityReport {
    pub fn is_clean(&self) -> bool {
        self.corrupt.is_empty() && self.missing.is_empty()
    }
}

pub struct Vault {
    root: PathBuf,
    dir: PathBuf,
    vault_id: [u8; 16],
    master: SecretKey,
    file: VaultFile,
    library: Library,
    library_restored: bool,
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .expect("0 ns is valid")
        .format(&time::format_description::well_known::Rfc3339)
        .expect("RFC 3339 formatting of a UTC time cannot fail")
}

/// Writes `data` to `path` via a temp file, fsync and rename.
fn write_atomic(path: &Path, data: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn read_vault_file(dir: &Path) -> Result<VaultFile> {
    let text = match fs::read_to_string(dir.join(VAULT_FILE)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Error::NotAVault),
        Err(e) => return Err(e.into()),
    };
    let f: VaultFile = serde_json::from_str(&text).map_err(|_| Error::MalformedHeader("vault.json"))?;
    if f.format != FORMAT_NAME {
        return Err(Error::NotAVault);
    }
    if f.version > VAULT_VERSION {
        return Err(Error::VaultTooNew(f.version));
    }
    Ok(f)
}

/// A reader that hashes what passes through, reports progress and can be cancelled.
struct Tap<'a, R> {
    inner: R,
    hash: Sha256,
    done: u64,
    since_report: u64,
    report: &'a mut dyn FnMut(WritePhase, u64) -> bool,
    phase: WritePhase,
    cancelled: bool,
}

impl<R: Read> Read for Tap<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // Not `Interrupted`: readers retry that kind, which would loop forever.
        let cancelled = || io::Error::other("cancelled");
        if self.cancelled {
            return Err(cancelled());
        }
        let n = self.inner.read(buf)?;
        self.hash.update(&buf[..n]);
        self.done += n as u64;
        self.since_report += n as u64;
        if self.since_report >= 1 << 20 || n == 0 {
            self.since_report = 0;
            if !(self.report)(self.phase, self.done) {
                self.cancelled = true;
                return Err(cancelled());
            }
        }
        Ok(n)
    }
}

/// Everything that determines `vault.json`. Public (hidden) only so the format
/// test vectors can build one deterministically.
#[doc(hidden)]
pub struct HeaderInputs<'a> {
    pub vault_id: [u8; 16],
    pub master: &'a [u8; 32],
    pub params: KdfParams,
    pub password: &'a [u8],
    pub password_nonce: [u8; 12],
    /// Recovery entropy and wrap nonce.
    pub recovery: Option<([u8; 32], [u8; 12])>,
    pub created_at: String,
    pub app_version: String,
}

fn build_file(inp: &HeaderInputs<'_>) -> Result<VaultFile> {
    let mut file = VaultFile {
        format: FORMAT_NAME.into(),
        version: VAULT_VERSION,
        vault_id: b64().encode(inp.vault_id),
        created_at: inp.created_at.clone(),
        app_version: inp.app_version.clone(),
        kdf: KdfJson { alg: String::new(), m_kib: 0, t: 0, p: 0, salt: String::new() },
        wraps: Vec::new(),
    };
    file.set_kdf(&inp.params);
    let kek = crypto::password_kek(inp.password, &inp.params)?;
    let ct = crypto::wrap_key(&kek, &inp.vault_id, KIND_PASSWORD, &inp.password_nonce, inp.master);
    file.set_wrap(KIND_PASSWORD, &inp.password_nonce, &ct);
    if let Some((entropy, nonce)) = &inp.recovery {
        let rkek = crypto::recovery_kek(entropy, &inp.vault_id);
        let ct = crypto::wrap_key(&rkek, &inp.vault_id, KIND_RECOVERY, nonce, inp.master);
        file.set_wrap(KIND_RECOVERY, nonce, &ct);
    }
    Ok(file)
}

/// `vault.json` text for the given inputs (test vectors).
#[doc(hidden)]
pub fn vault_json(inp: &HeaderInputs<'_>) -> Result<String> {
    let f = build_file(inp)?;
    serde_json::to_string_pretty(&f).map_err(|_| Error::MalformedHeader("vault.json"))
}

/// The recovery words for 32 bytes of entropy (test vectors).
#[doc(hidden)]
pub fn recovery_words_for(entropy: &[u8; 32]) -> String {
    RecoveryWords::from_entropy(entropy).as_str().to_owned()
}

impl Vault {
    pub fn vault_dir(root: &Path) -> PathBuf {
        root.join(VAULT_DIR)
    }

    /// Looks for a vault at `root` without unlocking it.
    pub fn detect(root: &Path) -> Option<VaultInfo> {
        let dir = Self::vault_dir(root);
        let text = fs::read_to_string(dir.join(VAULT_FILE)).ok()?;
        let f: VaultFile = serde_json::from_str(&text).ok()?;
        (f.format == FORMAT_NAME).then(|| VaultInfo {
            version: f.version,
            created_at: f.created_at.clone(),
            supported: f.version <= VAULT_VERSION,
            has_recovery: f.wrap(KIND_RECOVERY).is_some(),
        })
    }

    /// Creates a vault in `<root>/.cinasafe`. Returns the recovery words once
    /// when `with_recovery` is set; they are never stored.
    pub fn create(root: &Path, password: &Password, with_recovery: bool) -> Result<(Vault, Option<RecoveryWords>)> {
        Self::create_with_params(root, password, with_recovery, KdfParams::new_default()?)
    }

    /// As [`create`](Self::create) with explicit KDF parameters (tests use cheap ones).
    pub fn create_with_params(
        root: &Path,
        password: &Password,
        with_recovery: bool,
        params: KdfParams,
    ) -> Result<(Vault, Option<RecoveryWords>)> {
        if password.is_empty() {
            return Err(Error::WrongPassword);
        }
        let dir = Self::vault_dir(root);
        if dir.join(VAULT_FILE).exists() {
            return Err(Error::VaultExists);
        }
        fs::create_dir_all(dir.join("blobs"))?;
        let vault_id: [u8; 16] = random_bytes()?;
        let master = SecretKey::new(*crypto::random_key()?);
        let recovery_entropy = if with_recovery { Some(Zeroizing::new(random_bytes::<32>()?)) } else { None };
        let file = build_file(&HeaderInputs {
            vault_id,
            master: master.expose(),
            params,
            password: password.as_bytes(),
            password_nonce: random_bytes()?,
            recovery: match &recovery_entropy {
                Some(e) => Some((**e, random_bytes()?)),
                None => None,
            },
            created_at: now_rfc3339(),
            app_version: env!("CARGO_PKG_VERSION").into(),
        })?;
        let words = recovery_entropy.as_deref().map(RecoveryWords::from_entropy);

        let mut vault = Vault {
            root: root.to_path_buf(),
            dir,
            vault_id,
            master,
            file,
            library: Library::new()?,
            library_restored: false,
        };
        vault.save_library()?;
        // vault.json last: a half-created vault has no header and isn't detected.
        vault.write_header()?;
        Ok((vault, words))
    }

    fn open_with_kek(root: &Path, file: VaultFile, kind: &str, kek: &[u8; 32]) -> Result<Vault> {
        let vault_id: [u8; 16] = b64_fixed(&file.vault_id, "vault id")?;
        let w = file.wrap(kind).ok_or(match kind {
            KIND_RECOVERY => Error::NoRecoveryKey,
            _ => Error::MalformedHeader("no password wrap"),
        })?;
        let nonce: [u8; 12] = b64_fixed(&w.nonce, "wrap nonce")?;
        let ct: [u8; WRAPPED_LEN] = b64_fixed(&w.ciphertext, "wrap ciphertext")?;
        let master = crypto::unwrap_key(kek, &vault_id, kind, &nonce, &ct).ok_or(match kind {
            KIND_RECOVERY => Error::WrongRecoveryKey,
            _ => Error::WrongPassword,
        })?;
        let dir = Self::vault_dir(root);
        let (library, library_restored) = Self::load_library(&dir, &master, &vault_id)?;
        Ok(Vault { root: root.to_path_buf(), dir, vault_id, master, file, library, library_restored })
    }

    pub fn unlock_with_password(root: &Path, password: &Password) -> Result<Vault> {
        let file = read_vault_file(&Self::vault_dir(root))?;
        let params = file.kdf_params()?;
        let kek = crypto::password_kek(password.as_bytes(), &params)?;
        Self::open_with_kek(root, file, KIND_PASSWORD, &kek)
    }

    pub fn unlock_with_recovery(root: &Path, words: &RecoveryWords) -> Result<Vault> {
        let file = read_vault_file(&Self::vault_dir(root))?;
        let vault_id: [u8; 16] = b64_fixed(&file.vault_id, "vault id")?;
        let kek = crypto::recovery_kek(&words.entropy(), &vault_id);
        Self::open_with_kek(root, file, KIND_RECOVERY, &kek)
    }

    /// True if `password` opens this vault (for "current password" prompts).
    pub fn check_password(&self, password: &Password) -> Result<bool> {
        let params = self.file.kdf_params()?;
        let kek = crypto::password_kek(password.as_bytes(), &params)?;
        let w = self.file.wrap(KIND_PASSWORD).ok_or(Error::MalformedHeader("no password wrap"))?;
        let nonce: [u8; 12] = b64_fixed(&w.nonce, "wrap nonce")?;
        let ct: [u8; WRAPPED_LEN] = b64_fixed(&w.ciphertext, "wrap ciphertext")?;
        Ok(crypto::unwrap_key(&kek, &self.vault_id, KIND_PASSWORD, &nonce, &ct)
            .is_some_and(|k| k.expose() == self.master.expose()))
    }

    /// Re-wraps the master key under a new password (fresh salt, current
    /// default cost). No media is re-encrypted.
    pub fn change_password(&mut self, new: &Password) -> Result<()> {
        self.change_password_with_params(new, KdfParams::new_default()?)
    }

    pub fn change_password_with_params(&mut self, new: &Password, params: KdfParams) -> Result<()> {
        if new.is_empty() {
            return Err(Error::WrongPassword);
        }
        let kek = crypto::password_kek(new.as_bytes(), &params)?;
        let nonce: [u8; 12] = random_bytes()?;
        let ct = crypto::wrap_key(&kek, &self.vault_id, KIND_PASSWORD, &nonce, self.master.expose());
        let mut file = self.file.clone();
        file.set_kdf(&params);
        file.set_wrap(KIND_PASSWORD, &nonce, &ct);
        file.app_version = env!("CARGO_PKG_VERSION").into();
        self.commit_header(file)
    }

    /// Replaces (or adds) the recovery key; the old words stop working.
    pub fn replace_recovery(&mut self) -> Result<RecoveryWords> {
        let entropy = Zeroizing::new(random_bytes::<32>()?);
        let words = RecoveryWords::from_entropy(&entropy);
        let kek = crypto::recovery_kek(&entropy, &self.vault_id);
        let nonce: [u8; 12] = random_bytes()?;
        let ct = crypto::wrap_key(&kek, &self.vault_id, KIND_RECOVERY, &nonce, self.master.expose());
        let mut file = self.file.clone();
        file.set_wrap(KIND_RECOVERY, &nonce, &ct);
        self.commit_header(file)?;
        Ok(words)
    }

    pub fn remove_recovery(&mut self) -> Result<()> {
        let mut file = self.file.clone();
        file.wraps.retain(|w| w.kind != KIND_RECOVERY);
        self.commit_header(file)
    }

    pub fn has_recovery(&self) -> bool {
        self.file.wrap(KIND_RECOVERY).is_some()
    }

    pub fn created_at(&self) -> &str {
        &self.file.created_at
    }

    fn commit_header(&mut self, file: VaultFile) -> Result<()> {
        let json = serde_json::to_vec_pretty(&file).map_err(|_| Error::MalformedHeader("vault.json"))?;
        write_atomic(&self.dir.join(VAULT_FILE), &json)?;
        self.file = file;
        Ok(())
    }

    fn write_header(&self) -> Result<()> {
        let json = serde_json::to_vec_pretty(&self.file).map_err(|_| Error::MalformedHeader("vault.json"))?;
        write_atomic(&self.dir.join(VAULT_FILE), &json)
    }

    /// Locks the vault: keys and the in-memory library are dropped and zeroized.
    pub fn lock(self) {
        drop(self);
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn library_mut(&mut self) -> &mut Library {
        &mut self.library
    }

    /// True when `library.db.enc` was unreadable and the `.bak` copy was used.
    pub fn library_restored_from_backup(&self) -> bool {
        self.library_restored
    }

    fn blob_key(&self, id: &BlobId) -> crate::crypto::Key32 {
        derive_blob_key(self.master.expose(), &self.vault_id, id)
    }

    pub fn blob_path(&self, id: BlobId) -> PathBuf {
        self.dir.join("blobs").join(id.rel_path())
    }

    pub fn open_blob(&self, id: BlobId) -> Result<BlobReader> {
        let f = match File::open(self.blob_path(id)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(Error::BlobMissing),
            Err(e) => return Err(e.into()),
        };
        BlobReader::open(f, &self.blob_key(&id), id)
    }

    /// Reads a small blob (artwork, subtitles) fully into memory.
    pub fn read_blob(&self, id: BlobId) -> Result<Zeroizing<Vec<u8>>> {
        let mut r = self.open_blob(id)?;
        let mut out = Zeroizing::new(Vec::with_capacity(r.len() as usize));
        r.read_to_end(&mut out)?;
        Ok(out)
    }

    /// Encrypts `size` bytes from `src` into a new blob, then re-reads and
    /// verifies every chunk before the blob becomes visible.
    pub fn write_blob(&self, src: impl Read, size: u64, opts: &BlobWriteOptions) -> Result<BlobId> {
        self.write_blob_with(src, size, opts, &mut |_, _| true)
    }

    /// As [`write_blob`](Self::write_blob), calling `progress(phase, bytes)`
    /// about every MiB. Returning `false` cancels and removes the partial blob.
    pub fn write_blob_with(
        &self,
        src: impl Read,
        size: u64,
        opts: &BlobWriteOptions,
        progress: &mut dyn FnMut(WritePhase, u64) -> bool,
    ) -> Result<BlobId> {
        let id = BlobId::random()?;
        let path = self.blob_path(id);
        fs::create_dir_all(path.parent().expect("blob paths have parents"))?;
        let part = path.with_extension("cblob.part");
        let key = self.blob_key(&id);

        let result = (|| -> Result<()> {
            let mut tap = Tap {
                inner: src,
                hash: Sha256::new(),
                done: 0,
                since_report: 0,
                report: &mut *progress,
                phase: WritePhase::Encrypting,
                cancelled: false,
            };
            let mut out = BufWriter::with_capacity(4 << 20, File::create(&part)?);
            let written = write_blob(&key, id, size, &mut tap, &mut out, opts);
            if tap.cancelled {
                return Err(Error::Cancelled);
            }
            written?;
            out.flush()?;
            out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
            let expected = tap.hash.finalize();

            // Verify: decrypt every chunk back and compare the plaintext hash.
            let reader = BlobReader::open(File::open(&part)?, &key, id)?;
            let mut check = Tap {
                inner: reader,
                hash: Sha256::new(),
                done: 0,
                since_report: 0,
                report: &mut *progress,
                phase: WritePhase::Verifying,
                cancelled: false,
            };
            let res = io::copy(&mut check, &mut io::sink());
            if check.cancelled {
                return Err(Error::Cancelled);
            }
            res?;
            if check.hash.finalize() != expected {
                return Err(Error::VerifyFailed);
            }
            Ok(())
        })();

        match result {
            Ok(()) => {
                fs::rename(&part, &path)?;
                Ok(id)
            }
            Err(e) => {
                let _ = fs::remove_file(&part);
                Err(e)
            }
        }
    }

    pub fn delete_blob(&self, id: BlobId) -> Result<()> {
        let path = self.blob_path(id);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        // Tidy empty fan-out folders; failure just leaves an empty folder.
        if let Some(cd) = path.parent() {
            if fs::remove_dir(cd).is_ok() {
                if let Some(ab) = cd.parent() {
                    let _ = fs::remove_dir(ab);
                }
            }
        }
        Ok(())
    }

    // ---- library persistence (brief 4.5) ------------------------------------

    fn load_library(dir: &Path, master: &SecretKey, vault_id: &[u8; 16]) -> Result<(Library, bool)> {
        let key = derive_blob_key(master.expose(), vault_id, &BlobId::LIBRARY);
        let try_load = |path: &Path| -> Result<Library> {
            let r = BlobReader::open(File::open(path)?, &key, BlobId::LIBRARY)?;
            let len = usize::try_from(r.len()).map_err(|_| Error::TooLarge)?;
            let lib = Library::load(BufReader::new(r), len)?;
            Ok(lib)
        };
        let main = dir.join(LIBRARY_FILE);
        match try_load(&main) {
            Ok(lib) => Ok((lib, false)),
            Err(Error::Library(crate::library::LibraryError::TooNew(v))) => Err(Error::VaultTooNew(v as u32)),
            Err(first) => match try_load(&main.with_extension("enc.bak")) {
                Ok(lib) => Ok((lib, true)),
                Err(_) => Err(Error::LibraryUnreadable(first.to_string())),
            },
        }
    }

    /// Encrypts the library and swaps it in: `.tmp`, fsync, current → `.bak`,
    /// `.tmp` → current. A crash at any point leaves a readable copy.
    pub fn save_library(&mut self) -> Result<()> {
        let data = self.library.serialize()?;
        let main = self.dir.join(LIBRARY_FILE);
        let tmp = main.with_extension("enc.tmp");
        let bak = main.with_extension("enc.bak");
        {
            let key = self.blob_key(&BlobId::LIBRARY);
            let mut out = BufWriter::new(File::create(&tmp)?);
            write_blob(&key, BlobId::LIBRARY, data.len() as u64, &data[..], &mut out, &BlobWriteOptions::default())?;
            out.flush()?;
            out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        }
        if main.exists() {
            // Windows can't rename onto an existing file.
            let _ = fs::remove_file(&bak);
            fs::rename(&main, &bak)?;
        }
        fs::rename(&tmp, &main)?;
        self.library.mark_saved();
        self.library_restored = false;
        Ok(())
    }

    pub fn save_library_if_dirty(&mut self) -> Result<bool> {
        if self.library.is_dirty() {
            self.save_library()?;
            return Ok(true);
        }
        Ok(false)
    }

    // ---- inspection ------------------------------------------------------------

    /// Blob ids and on-disk sizes of every blob on the stick.
    pub fn blobs_on_disk(&self) -> Result<Vec<(BlobId, u64)>> {
        let mut out = Vec::new();
        let root = self.dir.join("blobs");
        if !root.exists() {
            return Ok(out);
        }
        for ab in fs::read_dir(&root)? {
            let ab = ab?;
            if !ab.file_type()?.is_dir() {
                continue;
            }
            for cd in fs::read_dir(ab.path())? {
                let cd = cd?;
                if !cd.file_type()?.is_dir() {
                    continue;
                }
                for f in fs::read_dir(cd.path())? {
                    let f = f?;
                    let name = f.file_name();
                    let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".cblob")) else { continue };
                    if let Some(id) = BlobId::from_hex(stem) {
                        out.push((id, f.metadata()?.len()));
                    }
                }
            }
        }
        out.sort_by_key(|(id, _)| id.0);
        Ok(out)
    }

    /// Bytes the vault occupies on the stick (blobs, library, header).
    pub fn used_bytes(&self) -> Result<u64> {
        let mut n: u64 = self.blobs_on_disk()?.iter().map(|(_, s)| s).sum();
        for f in [VAULT_FILE, LIBRARY_FILE, "library.db.enc.bak"] {
            n += fs::metadata(self.dir.join(f)).map(|m| m.len()).unwrap_or(0);
        }
        Ok(n)
    }

    /// Header of a blob without decrypting anything (for listings).
    pub fn blob_header(&self, id: BlobId) -> Result<Header> {
        let mut buf = [0u8; HEADER_LEN];
        File::open(self.blob_path(id))?.read_exact(&mut buf)?;
        Header::decode(&buf)
    }

    /// Decrypts every blob end to end and cross-checks the library.
    pub fn integrity_check(&self, mut progress: impl FnMut(&IntegrityProgress)) -> Result<IntegrityReport> {
        let on_disk = self.blobs_on_disk()?;
        let referenced: std::collections::BTreeSet<[u8; 16]> =
            self.library.referenced_blobs()?.into_iter().map(|b| b.0).collect();
        let present: std::collections::BTreeSet<[u8; 16]> = on_disk.iter().map(|(b, _)| b.0).collect();
        let mut p = IntegrityProgress {
            total: on_disk.len() as u64,
            bytes_total: on_disk.iter().map(|(_, s)| s).sum(),
            ..Default::default()
        };
        let mut report = IntegrityReport { library_restored_from_backup: self.library_restored, ..Default::default() };
        progress(&p);
        let mut buf = vec![0u8; 1 << 20];
        for (id, size) in &on_disk {
            let res = (|| -> Result<u64> {
                let mut r = self.open_blob(*id)?;
                let mut n = 0u64;
                loop {
                    let k = r.read(&mut buf)?;
                    if k == 0 {
                        return Ok(n);
                    }
                    n += k as u64;
                }
            })();
            match res {
                Ok(n) => report.bytes += n,
                Err(e) => report.corrupt.push((*id, e.to_string())),
            }
            report.checked += 1;
            p.done += 1;
            p.bytes_done += size;
            progress(&p);
        }
        buf.zeroize();
        report.missing = referenced.difference(&present).map(|b| BlobId(*b)).collect();
        report.orphaned = present.difference(&referenced).map(|b| BlobId(*b)).collect();
        report.corrupt_count = report.corrupt.len();
        report.missing_count = report.missing.len();
        report.orphaned_count = report.orphaned.len();
        Ok(report)
    }
}

#[cfg(test)]
mod tests;
