//! Error types for session persistence and state restoration.

use std::fmt;
use std::io;

/// Error returned by session persistence or application state operations.
#[derive(Debug)]
pub enum PersistError {
    /// Standard I/O error during read, write, fsync, or rename.
    Io(io::Error),
    /// JSON serialization or deserialization error.
    Json(serde_json::Error),
    /// Snapshot version is newer than supported or invalid.
    UnsupportedVersion(u32),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "persistence I/O error: {}", e),
            Self::Json(e) => write!(f, "persistence JSON error: {}", e),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported snapshot version: {}, expected 1", v)
            }
        }
    }
}

impl std::error::Error for PersistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            Self::UnsupportedVersion(_) => None,
        }
    }
}

impl From<io::Error> for PersistError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for PersistError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
