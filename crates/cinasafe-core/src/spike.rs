//! Milestone 0 stand-in for the vault: a master key stored in PLAINTEXT next to
//! the blobs. Exists only so the playback spike can exercise the real blob
//! format and stream bridge before password unlock is built.

use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};

use crate::blob::{write_blob, BlobWriteOptions};
use crate::crypto::{derive_blob_key, random_bytes, random_key, Key32};
use crate::{BlobId, BlobReader, Error, Result};

const KEY_FILE: &str = "SPIKE-INSECURE-KEY.txt";

pub struct SpikeVault {
    dir: PathBuf,
    vault_id: [u8; 16],
    master: Key32,
}

impl SpikeVault {
    fn dir_for(root: &Path) -> PathBuf {
        root.join(".cinasafe")
    }

    pub fn init(root: &Path) -> Result<SpikeVault> {
        let dir = Self::dir_for(root);
        fs::create_dir_all(dir.join("blobs"))?;
        let v = SpikeVault { dir, vault_id: random_bytes()?, master: random_key()? };
        let text = format!(
            "# Cinasafe Milestone 0 spike key. NOT SECURE. Delete with the test data.\nvault_id={}\nmaster_key={}\n",
            hex::encode(v.vault_id),
            hex::encode(*v.master)
        );
        fs::write(v.dir.join(KEY_FILE), text)?;
        Ok(v)
    }

    pub fn open(root: &Path) -> Result<SpikeVault> {
        let dir = Self::dir_for(root);
        let text = zeroize::Zeroizing::new(fs::read_to_string(dir.join(KEY_FILE))?);
        let field = |name: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix('='))
                .ok_or(Error::MalformedHeader("spike key file"))
        };
        let mut vault_id = [0u8; 16];
        let mut master = Key32::default();
        hex::decode_to_slice(field("vault_id")?, &mut vault_id).map_err(|_| Error::MalformedHeader("vault_id"))?;
        hex::decode_to_slice(field("master_key")?, master.as_mut())
            .map_err(|_| Error::MalformedHeader("master_key"))?;
        Ok(SpikeVault { dir, vault_id, master })
    }

    pub fn blob_path(&self, id: BlobId) -> PathBuf {
        self.dir.join("blobs").join(id.rel_path())
    }

    pub fn import(&self, src: &Path, opts: &BlobWriteOptions) -> Result<BlobId> {
        let id = BlobId::random()?;
        let path = self.blob_path(id);
        fs::create_dir_all(path.parent().unwrap())?;
        let input = File::open(src)?;
        let size = input.metadata()?.len();
        let tmp = path.with_extension("cblob.tmp");
        let mut out = std::io::BufWriter::with_capacity(4 << 20, File::create(&tmp)?);
        let key = derive_blob_key(&self.master, &self.vault_id, &id);
        write_blob(&key, id, size, BufReader::with_capacity(4 << 20, input), &mut out, opts)?;
        out.flush()?;
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        fs::rename(&tmp, &path)?;
        Ok(id)
    }

    pub fn open_blob(&self, id: BlobId) -> Result<BlobReader> {
        self.open_blob_from(id, File::open(self.blob_path(id))?)
    }

    /// Like [`open_blob`](Self::open_blob) over a caller-supplied source
    /// (the spike wraps the file to simulate slow sticks).
    pub fn open_blob_from(&self, id: BlobId, src: impl crate::ChunkSource) -> Result<BlobReader> {
        let key = derive_blob_key(&self.master, &self.vault_id, &id);
        BlobReader::open(src, &key, id)
    }
}
