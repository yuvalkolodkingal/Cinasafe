//! The `.cblob` encrypted container (docs/FORMAT.md, brief section 4.4).
//!
//! A blob is a 44-byte header followed by AES-256-GCM sealed chunks. Every chunk
//! authenticates the header, its own index and a last-chunk flag, so chunks can
//! be decrypted independently (instant seeking) but cannot be swapped,
//! reordered, truncated or moved between blobs.

use std::collections::VecDeque;
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use zeroize::Zeroizing;

use crate::crypto::random_bytes;
use crate::{Error, Result};

pub const MAGIC: [u8; 4] = *b"CSFB";
pub const FORMAT_VERSION: u8 = 1;
pub const HEADER_LEN: usize = 44;
pub const TAG_LEN: usize = 16;
pub const DEFAULT_CHUNK_SIZE: u32 = 1 << 20;
/// Readers refuse larger chunks so a hostile header cannot force huge allocations.
pub const MAX_CHUNK_SIZE: u32 = 64 << 20;
/// Media blobs are zero-padded on disk to a multiple of this.
pub const PAD_MULTIPLE: u64 = 1 << 20;
/// Decrypted chunks kept per reader (brief 5.2).
pub const CACHE_CHUNKS: usize = 16;
/// Chunks decrypted ahead of the read position on a worker thread (brief 5.2).
pub const PREFETCH_CHUNKS: u64 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlobId(pub [u8; 16]);

impl BlobId {
    /// Reserved id of the library database blob.
    pub const LIBRARY: BlobId = BlobId([0u8; 16]);

    pub fn random() -> Result<BlobId> {
        loop {
            let id = BlobId(random_bytes()?);
            if id != Self::LIBRARY {
                return Ok(id);
            }
        }
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Option<BlobId> {
        if s.len() != 32 || !s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return None;
        }
        let mut id = [0u8; 16];
        hex::decode_to_slice(s, &mut id).ok()?;
        Some(BlobId(id))
    }

    /// Path relative to `.cinasafe/blobs/`: `ab/cd/<id>.cblob`.
    pub fn rel_path(&self) -> PathBuf {
        let h = self.to_hex();
        [&h[0..2], &h[2..4], &format!("{h}.cblob")].iter().collect()
    }
}

impl fmt::Debug for BlobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlobId({})", self.to_hex())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub blob_id: BlobId,
    pub plaintext_size: u64,
    pub chunk_size: u32,
    pub nonce_prefix: [u8; 8],
}

impl Header {
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut h = [0u8; HEADER_LEN];
        h[0..4].copy_from_slice(&MAGIC);
        h[4] = FORMAT_VERSION;
        h[8..24].copy_from_slice(&self.blob_id.0);
        h[24..32].copy_from_slice(&self.plaintext_size.to_le_bytes());
        h[32..36].copy_from_slice(&self.chunk_size.to_le_bytes());
        h[36..44].copy_from_slice(&self.nonce_prefix);
        h
    }

    pub fn decode(h: &[u8; HEADER_LEN]) -> Result<Header> {
        if h[0..4] != MAGIC {
            return Err(Error::BadMagic);
        }
        if h[4] != FORMAT_VERSION {
            return Err(Error::UnsupportedVersion(h[4]));
        }
        if h[5..8] != [0, 0, 0] {
            return Err(Error::MalformedHeader("reserved bytes are not zero"));
        }
        let header = Header {
            blob_id: BlobId(h[8..24].try_into().unwrap()),
            plaintext_size: u64::from_le_bytes(h[24..32].try_into().unwrap()),
            chunk_size: u32::from_le_bytes(h[32..36].try_into().unwrap()),
            nonce_prefix: h[36..44].try_into().unwrap(),
        };
        if header.chunk_size == 0 || header.chunk_size > MAX_CHUNK_SIZE {
            return Err(Error::MalformedHeader("chunk size out of range"));
        }
        if header.chunk_count_checked().is_none() {
            return Err(Error::MalformedHeader("too many chunks"));
        }
        Ok(header)
    }

    fn chunk_count_checked(&self) -> Option<u64> {
        let n = self.plaintext_size.div_ceil(self.chunk_size as u64).max(1);
        // Chunk indices are encoded as u32 in the nonce and AAD.
        (n <= u32::MAX as u64 + 1).then_some(n)
    }

    /// Number of chunks; a zero-length blob has exactly one empty chunk.
    pub fn chunk_count(&self) -> u64 {
        self.chunk_count_checked().expect("validated on construction")
    }

    pub fn chunk_plain_len(&self, i: u64) -> usize {
        let start = i * self.chunk_size as u64;
        (self.plaintext_size - start).min(self.chunk_size as u64) as usize
    }

    pub fn chunk_offset(&self, i: u64) -> u64 {
        HEADER_LEN as u64 + i * (self.chunk_size as u64 + TAG_LEN as u64)
    }

    /// Header plus all sealed chunks, excluding padding.
    pub fn sealed_len(&self) -> u64 {
        HEADER_LEN as u64 + self.plaintext_size + self.chunk_count() * TAG_LEN as u64
    }
}

fn chunk_nonce(prefix: &[u8; 8], i: u64) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..8].copy_from_slice(prefix);
    n[8..].copy_from_slice(&(i as u32).to_be_bytes());
    n
}

fn chunk_aad(header: &[u8; HEADER_LEN], i: u64, last: bool) -> [u8; HEADER_LEN + 5] {
    let mut a = [0u8; HEADER_LEN + 5];
    a[..HEADER_LEN].copy_from_slice(header);
    a[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&(i as u32).to_be_bytes());
    a[HEADER_LEN + 4] = last as u8;
    a
}

#[derive(Clone, Debug)]
pub struct BlobWriteOptions {
    pub chunk_size: u32,
    /// Zero-pad the file to a multiple of 1 MiB (media blobs).
    pub pad: bool,
    /// Fixed nonce prefix; only for test vectors. `None` draws a random one.
    pub nonce_prefix: Option<[u8; 8]>,
}

impl Default for BlobWriteOptions {
    fn default() -> Self {
        BlobWriteOptions { chunk_size: DEFAULT_CHUNK_SIZE, pad: false, nonce_prefix: None }
    }
}

impl BlobWriteOptions {
    pub fn media() -> Self {
        BlobWriteOptions { pad: true, ..Default::default() }
    }
}

/// Encrypts exactly `size` bytes from `src` into `dst`. Returns bytes written.
/// Fails with [`Error::SizeMismatch`] if `src` is shorter or longer than `size`.
pub fn write_blob<R: Read, W: Write>(
    key: &[u8; 32],
    blob_id: BlobId,
    size: u64,
    mut src: R,
    mut dst: W,
    opts: &BlobWriteOptions,
) -> Result<u64> {
    if opts.chunk_size == 0 || opts.chunk_size > MAX_CHUNK_SIZE {
        return Err(Error::MalformedHeader("chunk size out of range"));
    }
    let header = Header {
        blob_id,
        plaintext_size: size,
        chunk_size: opts.chunk_size,
        nonce_prefix: match opts.nonce_prefix {
            Some(p) => p,
            None => random_bytes()?,
        },
    };
    let count = header.chunk_count_checked().ok_or(Error::TooLarge)?;
    let header_bytes = header.encode();
    let cipher = Aes256Gcm::new(key.into());
    dst.write_all(&header_bytes)?;

    let mut buf = Zeroizing::new(Vec::with_capacity(opts.chunk_size as usize + TAG_LEN));
    let mut consumed = 0u64;
    for i in 0..count {
        let len = header.chunk_plain_len(i);
        buf.clear();
        buf.resize(len, 0);
        let got = read_full(&mut src, &mut buf[..])?;
        consumed += got as u64;
        if got < len {
            return Err(Error::SizeMismatch { expected: size, actual: consumed });
        }
        let nonce = chunk_nonce(&header.nonce_prefix, i);
        let aad = chunk_aad(&header_bytes, i, i + 1 == count);
        cipher.encrypt_in_place(Nonce::from_slice(&nonce), &aad, &mut *buf).map_err(|_| Error::TooLarge)?;
        dst.write_all(&buf)?;
    }
    let mut probe = [0u8; 1];
    if read_full(&mut src, &mut probe)? != 0 {
        return Err(Error::SizeMismatch { expected: size, actual: size + 1 });
    }

    let mut written = header.sealed_len();
    if opts.pad {
        let padded = written.div_ceil(PAD_MULTIPLE) * PAD_MULTIPLE;
        let zeros = [0u8; 64 * 1024];
        while written < padded {
            let n = (padded - written).min(zeros.len() as u64) as usize;
            dst.write_all(&zeros[..n])?;
            written += n as u64;
        }
    }
    dst.flush()?;
    Ok(written)
}

fn read_full<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// Positional reads, so the prefetch thread and the reader can share one handle.
pub trait ChunkSource: Send + Sync + 'static {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()>;
}

impl ChunkSource for std::fs::File {
    #[cfg(unix)]
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        std::os::unix::fs::FileExt::read_exact_at(self, buf, offset)
    }

    #[cfg(windows)]
    fn read_exact_at(&self, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
        use std::os::windows::fs::FileExt;
        while !buf.is_empty() {
            match self.seek_read(buf, offset) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    buf = &mut buf[n..];
                    offset += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

impl ChunkSource for Vec<u8> {
    fn read_exact_at(&self, buf: &mut [u8], offset: u64) -> io::Result<()> {
        let start = usize::try_from(offset).map_err(|_| io::ErrorKind::UnexpectedEof)?;
        let src =
            start.checked_add(buf.len()).and_then(|end| self.get(start..end)).ok_or(io::ErrorKind::UnexpectedEof)?;
        buf.copy_from_slice(src);
        Ok(())
    }
}

type Chunk = Arc<Zeroizing<Vec<u8>>>;

#[derive(Clone, Copy, Debug, Default)]
pub struct ReaderStats {
    pub chunks_decrypted: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
}

struct CacheState {
    /// Front is least recently used.
    entries: VecDeque<(u64, Chunk)>,
    in_flight: Vec<u64>,
    stats: ReaderStats,
}

struct Shared {
    src: Box<dyn ChunkSource>,
    cipher: Aes256Gcm,
    header: Header,
    header_bytes: [u8; HEADER_LEN],
    state: Mutex<CacheState>,
    ready: Condvar,
}

impl Shared {
    fn get_chunk(&self, i: u64, counts_as_read: bool) -> Result<Chunk> {
        let mut st = self.state.lock().unwrap();
        loop {
            if let Some(pos) = st.entries.iter().position(|(idx, _)| *idx == i) {
                let entry = st.entries.remove(pos).unwrap();
                let chunk = entry.1.clone();
                st.entries.push_back(entry);
                if counts_as_read {
                    st.stats.cache_hits += 1;
                }
                return Ok(chunk);
            }
            if st.in_flight.contains(&i) {
                st = self.ready.wait(st).unwrap();
                continue;
            }
            if counts_as_read {
                st.stats.cache_misses += 1;
            }
            st.in_flight.push(i);
            break;
        }
        drop(st);

        let result = self.decrypt_chunk(i);

        let mut st = self.state.lock().unwrap();
        st.in_flight.retain(|&x| x != i);
        let out = result.map(|plain| {
            let chunk: Chunk = Arc::new(plain);
            st.stats.chunks_decrypted += 1;
            st.entries.push_back((i, chunk.clone()));
            // Evicted chunks zeroize when their last Arc drops.
            while st.entries.len() > CACHE_CHUNKS {
                st.entries.pop_front();
            }
            chunk
        });
        self.ready.notify_all();
        out
    }

    fn decrypt_chunk(&self, i: u64) -> Result<Zeroizing<Vec<u8>>> {
        let h = &self.header;
        let len = h.chunk_plain_len(i);
        let mut buf = Zeroizing::new(vec![0u8; len + TAG_LEN]);
        self.src.read_exact_at(&mut buf, h.chunk_offset(i)).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                Error::ChunkAuth(i)
            } else {
                Error::Io(e)
            }
        })?;
        let nonce = chunk_nonce(&h.nonce_prefix, i);
        let aad = chunk_aad(&self.header_bytes, i, i + 1 == h.chunk_count());
        self.cipher.decrypt_in_place(Nonce::from_slice(&nonce), &aad, &mut *buf).map_err(|_| Error::ChunkAuth(i))?;
        Ok(buf)
    }
}

/// Plaintext `Read + Seek` view of a blob. Decrypts only the chunks a read
/// touches, caches the last [`CACHE_CHUNKS`] and prefetches ahead.
pub struct BlobReader {
    shared: Arc<Shared>,
    pos: u64,
    prefetch_tx: Option<SyncSender<u64>>,
    prefetched_up_to: u64,
    worker: Option<JoinHandle<()>>,
}

impl BlobReader {
    pub fn open(src: impl ChunkSource, key: &[u8; 32], expected_id: BlobId) -> Result<BlobReader> {
        Self::open_with(src, key, expected_id, true)
    }

    pub fn open_with(src: impl ChunkSource, key: &[u8; 32], expected_id: BlobId, prefetch: bool) -> Result<BlobReader> {
        let mut header_bytes = [0u8; HEADER_LEN];
        src.read_exact_at(&mut header_bytes, 0).map_err(|e| {
            if e.kind() == io::ErrorKind::UnexpectedEof {
                Error::MalformedHeader("file shorter than header")
            } else {
                Error::Io(e)
            }
        })?;
        let header = Header::decode(&header_bytes)?;
        if header.blob_id != expected_id {
            return Err(Error::BlobIdMismatch);
        }
        let shared = Arc::new(Shared {
            src: Box::new(src),
            cipher: Aes256Gcm::new(key.into()),
            header,
            header_bytes,
            state: Mutex::new(CacheState {
                entries: VecDeque::with_capacity(CACHE_CHUNKS + 1),
                in_flight: Vec::new(),
                stats: ReaderStats::default(),
            }),
            ready: Condvar::new(),
        });

        let (prefetch_tx, worker) = if prefetch && header.chunk_count() > 1 {
            let (tx, rx) = mpsc::sync_channel::<u64>(PREFETCH_CHUNKS as usize * 2);
            let s = shared.clone();
            let handle = std::thread::Builder::new().name("cblob-prefetch".into()).spawn(move || {
                for i in rx {
                    // Errors surface again when the reader itself asks for the chunk.
                    let _ = s.get_chunk(i, false);
                }
            })?;
            (Some(tx), Some(handle))
        } else {
            (None, None)
        };

        Ok(BlobReader { shared, pos: 0, prefetch_tx, prefetched_up_to: 0, worker })
    }

    pub fn header(&self) -> &Header {
        &self.shared.header
    }

    pub fn len(&self) -> u64 {
        self.shared.header.plaintext_size
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn stats(&self) -> ReaderStats {
        self.shared.state.lock().unwrap().stats
    }

    fn prefetch_after(&mut self, i: u64) {
        let Some(tx) = &self.prefetch_tx else { return };
        let last = self.shared.header.chunk_count() - 1;
        let want_to = (i + PREFETCH_CHUNKS).min(last);
        // After a backwards seek, start requesting again from the new position.
        let from = if self.prefetched_up_to > i && self.prefetched_up_to <= want_to {
            self.prefetched_up_to + 1
        } else {
            i + 1
        };
        for j in from..=want_to {
            match tx.try_send(j) {
                Ok(()) => self.prefetched_up_to = j,
                Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => break,
            }
        }
    }
}

impl Read for BlobReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let size = self.shared.header.plaintext_size;
        if buf.is_empty() || self.pos >= size {
            return Ok(0);
        }
        let cs = self.shared.header.chunk_size as u64;
        let i = self.pos / cs;
        let off = (self.pos % cs) as usize;
        let chunk = self.shared.get_chunk(i, true).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let n = buf.len().min(chunk.len() - off);
        buf[..n].copy_from_slice(&chunk[off..off + n]);
        self.pos += n as u64;
        self.prefetch_after(i);
        Ok(n)
    }
}

impl Seek for BlobReader {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let size = self.shared.header.plaintext_size as i128;
        let target = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => size + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if target < 0 || target > u64::MAX as i128 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek out of range"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

impl Drop for BlobReader {
    fn drop(&mut self) {
        // Closing the channel ends the worker; joining makes sure its chunk
        // references are gone so the cache zeroizes before we return.
        self.prefetch_tx.take();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
        if let Ok(mut st) = self.shared.state.lock() {
            st.entries.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seal(data: &[u8], chunk: u32, id: BlobId, key: &[u8; 32], pad: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let opts = BlobWriteOptions { chunk_size: chunk, pad, nonce_prefix: None };
        write_blob(key, id, data.len() as u64, data, &mut out, &opts).unwrap();
        out
    }

    fn pattern(n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + i / 251) as u8).collect()
    }

    #[test]
    fn roundtrip_sizes() {
        let key = [7u8; 32];
        let id = BlobId([1; 16]);
        for &n in &[0usize, 1, 15, 16, 17, 31, 32, 33, 1000] {
            let data = pattern(n);
            let sealed = seal(&data, 16, id, &key, false);
            let header = Header::decode(sealed[..HEADER_LEN].try_into().unwrap()).unwrap();
            assert_eq!(sealed.len() as u64, header.sealed_len(), "n={n}");
            let mut r = BlobReader::open(sealed, &key, id).unwrap();
            let mut back = Vec::new();
            r.read_to_end(&mut back).unwrap();
            assert_eq!(back, data, "n={n}");
        }
    }

    #[test]
    fn padding_is_ignored() {
        let key = [7u8; 32];
        let id = BlobId([2; 16]);
        let data = pattern(5000);
        let sealed = seal(&data, 1024, id, &key, true);
        assert_eq!(sealed.len() as u64 % PAD_MULTIPLE, 0);
        let mut back = Vec::new();
        BlobReader::open(sealed, &key, id).unwrap().read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
    }

    #[test]
    fn random_seeks_match() {
        let key = [9u8; 32];
        let id = BlobId([3; 16]);
        let data = pattern(100_000);
        let sealed = seal(&data, 4096, id, &key, true);
        let mut r = BlobReader::open(sealed, &key, id).unwrap();
        let mut state = 12345u64;
        for _ in 0..500 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let pos = (state >> 33) % (data.len() as u64 + 10);
            let len = ((state >> 13) % 9000) as usize;
            r.seek(SeekFrom::Start(pos)).unwrap();
            let mut buf = vec![0u8; len];
            let mut got = 0;
            loop {
                let n = r.read(&mut buf[got..]).unwrap();
                if n == 0 {
                    break;
                }
                got += n;
            }
            let p = (pos as usize).min(data.len());
            let end = (p + len).min(data.len());
            assert_eq!(&buf[..got], &data[p..end]);
        }
    }

    #[test]
    fn rejects_wrong_size_source() {
        let key = [0u8; 32];
        let id = BlobId([4; 16]);
        let data = pattern(100);
        let opts = BlobWriteOptions { chunk_size: 16, ..Default::default() };
        let short = write_blob(&key, id, 101, &data[..], Vec::new(), &opts);
        assert!(matches!(short, Err(Error::SizeMismatch { .. })));
        let long = write_blob(&key, id, 99, &data[..], Vec::new(), &opts);
        assert!(matches!(long, Err(Error::SizeMismatch { .. })));
    }

    fn read_all(sealed: Vec<u8>, key: &[u8; 32], id: BlobId) -> Result<Vec<u8>> {
        let mut r = BlobReader::open_with(sealed, key, id, false)?;
        let mut out = Vec::new();
        r.read_to_end(&mut out).map_err(|e| match e.into_inner() {
            Some(inner) => *inner.downcast::<Error>().unwrap(),
            None => Error::ChunkAuth(u64::MAX),
        })?;
        Ok(out)
    }

    #[test]
    fn detects_tampering() {
        let key = [5u8; 32];
        let id = BlobId([6; 16]);
        let data = pattern(64);
        let good = seal(&data, 16, id, &key, false);
        assert_eq!(read_all(good.clone(), &key, id).unwrap(), data);

        // Wrong key.
        assert!(matches!(read_all(good.clone(), &[6u8; 32], id), Err(Error::ChunkAuth(0))));
        // Wrong expected id.
        assert!(matches!(read_all(good.clone(), &key, BlobId([7; 16])), Err(Error::BlobIdMismatch)));

        // Header tampering: nonce prefix and plaintext size are bound into every chunk.
        let mut t = good.clone();
        t[40] ^= 1;
        assert!(matches!(read_all(t, &key, id), Err(Error::ChunkAuth(0))));
        let mut t = good.clone();
        t[24] = 48; // claim 48 bytes: chunk 2 would wrongly become the last
        assert!(read_all(t, &key, id).is_err());

        // Ciphertext bit flip.
        let mut t = good.clone();
        t[HEADER_LEN + 3] ^= 0x80;
        assert!(matches!(read_all(t, &key, id), Err(Error::ChunkAuth(0))));

        // Swap chunks 0 and 1.
        let c = 16 + TAG_LEN;
        let mut t = good.clone();
        let (a, b) = (HEADER_LEN, HEADER_LEN + c);
        let first: Vec<u8> = t[a..a + c].to_vec();
        t.copy_within(b..b + c, a);
        t[b..b + c].copy_from_slice(&first);
        assert!(matches!(read_all(t, &key, id), Err(Error::ChunkAuth(0))));

        // Truncation: drop the final chunk entirely, then claim a shorter size.
        let mut t = good.clone();
        t.truncate(HEADER_LEN + 3 * c);
        assert!(matches!(read_all(t.clone(), &key, id), Err(Error::ChunkAuth(3))));
        t[24] = 48;
        assert!(read_all(t, &key, id).is_err());

        // Bad magic / version / reserved.
        let mut t = good.clone();
        t[0] = b'X';
        assert!(matches!(read_all(t, &key, id), Err(Error::BadMagic)));
        let mut t = good.clone();
        t[4] = 2;
        assert!(matches!(read_all(t, &key, id), Err(Error::UnsupportedVersion(2))));
        let mut t = good;
        t[6] = 1;
        assert!(matches!(read_all(t, &key, id), Err(Error::MalformedHeader(_))));
    }

    #[test]
    fn blob_id_hex_and_path() {
        let id = BlobId::from_hex("abcdef0123456789abcdef0123456789").unwrap();
        assert_eq!(id.rel_path(), PathBuf::from("ab/cd/abcdef0123456789abcdef0123456789.cblob"));
        assert!(BlobId::from_hex("ABCDEF0123456789ABCDEF0123456789").is_none());
        assert!(BlobId::from_hex("../../etc/passwd").is_none());
        assert_ne!(BlobId::random().unwrap(), BlobId::LIBRARY);
    }
}
