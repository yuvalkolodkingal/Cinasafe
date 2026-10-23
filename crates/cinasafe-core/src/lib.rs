//! Cinasafe core: the encrypted vault (keys, `vault.json`, blobs), the
//! library database, media recognition, subtitles and online metadata.

pub mod blob;
pub mod crypto;
pub mod drives;
pub mod import;
pub mod importer;
pub mod library;
pub mod metadata;
pub mod secret;
#[cfg(feature = "spike-insecure-keyfile")]
pub mod spike;
pub mod subtitles;
pub mod vault;

pub use blob::{BlobId, BlobReader, BlobWriteOptions, ChunkSource, Header};
pub use secret::{Password, SecretKey};
pub use vault::{RecoveryWords, Vault};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a Cinasafe blob")]
    BadMagic,
    #[error("unsupported blob format version {0}")]
    UnsupportedVersion(u8),
    #[error("malformed data: {0}")]
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
    #[error("no Cinasafe vault here")]
    NotAVault,
    #[error("a vault already exists here")]
    VaultExists,
    #[error("this vault was made by a newer Cinasafe (format {0}); update Cinasafe to open it")]
    VaultTooNew(u32),
    #[error("wrong password")]
    WrongPassword,
    #[error("wrong recovery key")]
    WrongRecoveryKey,
    #[error("this vault has no recovery key")]
    NoRecoveryKey,
    #[error("those aren't 24 valid recovery words")]
    BadRecoveryWords,
    #[error("a file in the vault is missing")]
    BlobMissing,
    #[error("cancelled")]
    Cancelled,
    #[error("verification after writing failed; the stick may be faulty")]
    VerifyFailed,
    #[error("the library can't be read: {0}")]
    LibraryUnreadable(String),
    #[error(transparent)]
    Library(#[from] library::LibraryError),
    #[error(transparent)]
    Metadata(#[from] metadata::Error),
    #[error("{0}")]
    Import(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;
