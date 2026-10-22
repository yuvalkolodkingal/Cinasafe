//! Cinasafe core: key derivation and the encrypted blob format.
//!
//! Milestone 0 scope: only what the playback spike needs (the `.cblob` format,
//! per-blob key derivation and the streaming reader/writer). The vault header,
//! password unlock and library live in later milestones.

pub mod blob;
pub mod crypto;
#[cfg(feature = "spike-insecure-keyfile")]
pub mod spike;

pub use blob::{BlobId, BlobReader, BlobWriteOptions, ChunkSource, Header};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a Cinasafe blob")]
    BadMagic,
    #[error("unsupported blob format version {0}")]
    UnsupportedVersion(u8),
    #[error("malformed blob header: {0}")]
    MalformedHeader(&'static str),
    #[error("blob id in header does not match the requested blob")]
    BlobIdMismatch,
    #[error("blob chunk {0} failed authentication")]
    ChunkAuth(u64),
    #[error("source ended at {actual} bytes, expected {expected}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("blob too large for the chunk counter")]
    TooLarge,
    #[error("random number generator failed")]
    Rng,
}

pub type Result<T> = std::result::Result<T, Error>;
