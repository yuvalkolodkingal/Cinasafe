//! `AppError`: what every command rejects with. `code` doubles as the UI's
//! i18n key (`errors.<code>`); `message` is an English detail that never
//! contains titles, file names or paths from the vault.

use cinasafe_core::drives::DriveError;
use cinasafe_core::library::LibraryError;
use cinasafe_core::metadata;
use cinasafe_core::Error as CoreError;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
}

pub type Result<T> = std::result::Result<T, AppError>;

impl AppError {
    pub fn new(code: &'static str, message: impl Into<String>) -> AppError {
        AppError { code, message: message.into() }
    }

    pub fn locked() -> AppError {
        AppError::new("locked", "the vault is locked")
    }

    pub fn not_found() -> AppError {
        AppError::new("notFound", "not found")
    }
}

impl From<CoreError> for AppError {
    fn from(e: CoreError) -> AppError {
        let e = match e {
            CoreError::Metadata(m) => return m.into(),
            other => other,
        };
        let code = match &e {
            CoreError::WrongPassword => "wrongPassword",
            CoreError::WrongRecoveryKey => "wrongRecoveryKey",
            CoreError::BadRecoveryWords => "badRecoveryWords",
            CoreError::NoRecoveryKey => "noRecoveryKey",
            CoreError::VaultTooNew(_) => "vaultTooNew",
            CoreError::NotAVault => "notAVault",
            CoreError::VaultExists => "vaultExists",
            CoreError::Cancelled => "cancelled",
            CoreError::VerifyFailed => "verifyFailed",
            CoreError::LibraryUnreadable(_) => "libraryUnreadable",
            CoreError::Library(LibraryError::NotFound) => "notFound",
            CoreError::Import("movie lookups are off" | "lookups are off") => "lookupsOff",
            CoreError::Io(io) if io.kind() == std::io::ErrorKind::NotFound => "driveRemoved",
            CoreError::Io(_) | CoreError::BlobMissing => "io",
            _ => "internal",
        };
        AppError::new(code, e.to_string())
    }
}

impl From<LibraryError> for AppError {
    fn from(e: LibraryError) -> AppError {
        match e {
            LibraryError::NotFound => AppError::not_found(),
            other => AppError::new("internal", other.to_string()),
        }
    }
}

impl From<metadata::Error> for AppError {
    fn from(e: metadata::Error) -> AppError {
        let code = match &e {
            metadata::Error::Unauthorized => "unauthorized",
            metadata::Error::NotFound => "notFound",
            metadata::Error::MoviesUnsupported(_) => "lookupsOff",
            _ => "network",
        };
        AppError::new(code, e.to_string())
    }
}

impl From<DriveError> for AppError {
    fn from(e: DriveError) -> AppError {
        let code = match &e {
            DriveError::FormatCancelled => "formatCancelled",
            DriveError::FormatFailed(_) => "formatFailed",
            DriveError::BadLabel => "badLabel",
            DriveError::Unavailable(_) | DriveError::Io(_) => "io",
        };
        AppError::new(code, e.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> AppError {
        AppError::new("io", e.to_string())
    }
}

impl From<cinasafe_player::Error> for AppError {
    fn from(e: cinasafe_player::Error) -> AppError {
        match e {
            cinasafe_player::Error::Library(m) => AppError::new("playerUnavailable", m),
            other => AppError::new("player", other.to_string()),
        }
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> AppError {
        AppError::new("internal", e.to_string())
    }
}
