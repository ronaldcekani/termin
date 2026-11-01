//! Blocking client for communicating with Termin over Unix domain socket.
//!
//! See `docs/specs/ipc-cli.md §1, §4`.

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::IpcError;
use crate::protocol::{Event, Request, Response, METHOD_PING};

/// Default socket path in `~/Library/Application Support/Termin/termin.sock`.
pub fn default_socket_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Termin")
            .join("termin.sock")
    } else {
        PathBuf::from("/tmp/termin.sock")
    }
}

/// Resolve socket path, giving precedence to `TERMIN_SOCKET` environment variable.
pub fn resolve_socket_path() -> PathBuf {
    if let Some(sock) = std::env::var_os("TERMIN_SOCKET") {
        if !sock.is_empty() {
            return PathBuf::from(sock);
        }
    }
    default_socket_path()
}

/// Blocking IPC client for interacting with a running Termin instance.
///
/// See `docs/specs/ipc-cli.md §4`.
#[derive(Debug)]
pub struct Client {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next_id: u64,
    buffered_events: Vec<Event>,
}

/// Backwards-compatible alias for `Client`.
pub type IpcClient = Client;

impl Client {
    /// Create a Client from an existing UnixStream.
    pub fn from_stream(stream: UnixStream) -> Result<Self, IpcError> {
        let reader_stream = stream.try_clone()?;
        Ok(Self {
            stream,
            reader: BufReader::new(reader_stream),
            next_id: 1,
            buffered_events: Vec::new(),
        })
    }

    /// Connect to Unix domain socket at path.
    pub fn connect(path: &Path) -> Result<Self, IpcError> {
        let stream = UnixStream::connect(path).map_err(|e| {
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) {
                IpcError::NotRunning
            } else {
                IpcError::Io(e)
            }
        })?;

        Self::from_stream(stream)
    }

    /// Connect using resolved default socket path (`TERMIN_SOCKET` or default Application Support path).
    pub fn connect_default() -> Result<Self, IpcError> {
        let path = resolve_socket_path();
        Self::connect(&path)
    }

    /// Try to connect to an existing running Termin instance and verify responsiveness with `ping`.
    ///
    /// Returns `Err(IpcError::NotRunning)` if socket does not exist or connection was refused.
    pub fn try_connect(path: &Path) -> Result<Self, IpcError> {
        let stream = match UnixStream::connect(path) {
            Ok(s) => s,
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound
                        | std::io::ErrorKind::ConnectionRefused
                        | std::io::ErrorKind::ConnectionReset
                ) =>
            {
                return Err(IpcError::NotRunning)
            }
            Err(e) => return Err(IpcError::Io(e)),
        };

        // Set short timeout for the handshake ping
        let timeout = Duration::from_millis(500);
        let _ = stream.set_read_timeout(Some(timeout));
        let _ = stream.set_write_timeout(Some(timeout));

        let reader_stream = stream.try_clone()?;
        let mut client = Self {
            stream,
            reader: BufReader::new(reader_stream),
            next_id: 1,
            buffered_events: Vec::new(),
        };

        // Verify with ping
        if client.ping().is_err() {
            return Err(IpcError::NotRunning);
        }

        // Reset timeouts for normal blocking calls
        let _ = client.stream.set_read_timeout(None);
        let _ = client.stream.set_write_timeout(None);

        Ok(client)
    }

    /// Execute a blocking RPC call and return the raw `serde_json::Value` result.
    pub fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, IpcError> {
        let id = self.next_id;
        self.next_id += 1;

        let req = Request {
            id,
            method: method.to_string(),
            params,
        };

        let mut line = serde_json::to_vec(&req)?;
        line.push(b'\n');
        self.stream.write_all(&line)?;
        self.stream.flush()?;

        loop {
            let mut resp_line = String::new();
            let bytes_read = self.reader.read_line(&mut resp_line)?;
            if bytes_read == 0 {
                return Err(IpcError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed by Termin server",
                )));
            }

            let trimmed = resp_line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Check if this is an async event
            if let Ok(event) = serde_json::from_str::<Event>(trimmed) {
                if !event.event.is_empty() {
                    self.buffered_events.push(event);
                    continue;
                }
            }

            // Otherwise, attempt to parse as Response
            let resp: Response = serde_json::from_str(trimmed)?;
            if resp.id != id {
                continue;
            }

            if let Some(err) = resp.error {
                return Err(IpcError::Rpc(err));
            } else {
                return Ok(resp.result.unwrap_or(serde_json::Value::Null));
            }
        }
    }

    /// Convenience wrapper to perform a typed call.
    pub fn call_typed<P: Serialize, R: DeserializeOwned>(
        &mut self,
        method: &str,
        params: &P,
    ) -> Result<R, IpcError> {
        let val = serde_json::to_value(params)?;
        let res = self.call(method, val)?;
        serde_json::from_value(res).map_err(IpcError::Json)
    }

    /// Send a `ping` request to verify connection.
    pub fn ping(&mut self) -> Result<String, IpcError> {
        let res = self.call(METHOD_PING, serde_json::Value::Null)?;
        if let Some(obj) = res.as_object() {
            if let Some(version) = obj.get("version").and_then(|v| v.as_str()) {
                return Ok(version.to_string());
            }
        }
        Ok(res.to_string())
    }

    /// Drain any asynchronous events collected during calls.
    pub fn drain_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.buffered_events)
    }
}
