//! JSON-lines IPC protocol, client, and sans-IO server codec (`termin-ipc`).
//!
//! See `docs/specs/ipc-cli.md`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;

/// Incoming IPC JSON-RPC request.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Error details returned in an IPC response.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: String,
    pub message: String,
}

/// Outgoing IPC JSON-RPC response.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ResponseError>,
}

/// Outgoing asynchronous event emitted after subscription.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub event: String,
    pub data: serde_json::Value,
}

/// Error returned by IPC client or codec operations.
#[derive(Debug)]
pub enum IpcError {
    Io(io::Error),
    Json(serde_json::Error),
    Rpc(ResponseError),
}

impl fmt::Display for IpcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "IPC IO error: {}", e),
            Self::Json(e) => write!(f, "IPC JSON error: {}", e),
            Self::Rpc(err) => write!(f, "IPC RPC error [{}]: {}", err.code, err.message),
        }
    }
}

impl std::error::Error for IpcError {}

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

/// Sans-IO server codec for parsing requests and encoding responses.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, Default)]
pub struct IpcCodec {
    buf: Vec<u8>,
}

impl IpcCodec {
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Feed incoming raw bytes into buffer.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Decode next complete request if newline is found.
    pub fn decode_request(&mut self) -> Result<Option<Request>, IpcError> {
        if let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let line = self.buf.drain(..=pos).collect::<Vec<u8>>();
            let req: Request = serde_json::from_slice(&line)?;
            Ok(Some(req))
        } else {
            Ok(None)
        }
    }

    /// Encode response into a newline-terminated byte vector.
    pub fn encode_response(resp: &Response) -> Result<Vec<u8>, IpcError> {
        let mut bytes = serde_json::to_vec(resp)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Encode event into a newline-terminated byte vector.
    pub fn encode_event(event: &Event) -> Result<Vec<u8>, IpcError> {
        let mut bytes = serde_json::to_vec(event)?;
        bytes.push(b'\n');
        Ok(bytes)
    }
}

/// Blocking client for interacting with running Termin instance.
///
/// See `docs/specs/ipc-cli.md §4`.
pub struct IpcClient {
    stream: UnixStream,
    next_id: u64,
}

impl IpcClient {
    /// Connect to Unix domain socket at path.
    pub fn connect(path: &Path) -> Result<Self, IpcError> {
        let stream = UnixStream::connect(path)?;
        Ok(Self { stream, next_id: 1 })
    }

    /// Execute a blocking RPC call.
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

        let mut reader = BufReader::new(&self.stream);
        let mut resp_line = String::new();
        reader.read_line(&mut resp_line)?;

        let resp: Response = serde_json::from_str(&resp_line)?;
        if let Some(err) = resp.error {
            Err(IpcError::Rpc(err))
        } else {
            Ok(resp.result.unwrap_or(serde_json::Value::Null))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_codec_round_trip() {
        let mut codec = IpcCodec::new();
        let json = b"{\"id\":1,\"method\":\"ping\",\"params\":{}}\n";
        codec.feed(json);
        let req = codec.decode_request().unwrap().unwrap();
        assert_eq!(req.id, 1);
        assert_eq!(req.method, "ping");

        let resp = Response {
            id: 1,
            result: Some(serde_json::json!({"version": "0.1.0"})),
            error: None,
        };
        let encoded = IpcCodec::encode_response(&resp).unwrap();
        assert!(encoded.ends_with(b"\n"));
    }
}
