//! Error types for Termin IPC protocol and client operations.

use crate::protocol::ResponseError;
use std::fmt;
use std::io;

/// Maximum line length allowed in IPC protocol (1 MiB).
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// Error returned by IPC client, server, or codec operations.
#[derive(Debug)]
pub enum IpcError {
    /// Standard I/O error.
    Io(io::Error),
    /// JSON serialization or deserialization error.
    Json(serde_json::Error),
    /// Error returned by remote RPC server.
    Rpc(ResponseError),
    /// Incoming line exceeded maximum allowed size (1 MiB).
    LineTooLong,
    /// Target Termin daemon / instance is not running or socket is missing.
    NotRunning,
    /// General protocol violation or unexpected message.
    Protocol(String),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IPC I/O error: {}", e),
            Self::Json(e) => write!(f, "IPC JSON error: {}", e),
            Self::Rpc(err) => write!(f, "IPC RPC error [{}]: {}", err.code, err.message),
            Self::LineTooLong => write!(
                f,
                "IPC line length exceeded limit of {} bytes",
                MAX_LINE_BYTES
            ),
            Self::NotRunning => write!(f, "Termin daemon is not running"),
            Self::Protocol(msg) => write!(f, "IPC protocol error: {}", msg),
        }
    }
}

impl std::error::Error for IpcError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for IpcError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for IpcError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}

impl IpcError {
    /// Whether this error indicates that Termin is not running or the socket cannot be reached.
    pub fn is_not_running(&self) -> bool {
        match self {
            Self::NotRunning => true,
            Self::Io(e) => matches!(
                e.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::BrokenPipe
            ),
            _ => false,
        }
    }
}
