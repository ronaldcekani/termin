//! Sans-IO server codec for JSON-lines IPC protocol.
//!
//! See `docs/specs/ipc-cli.md §2`.

use crate::error::{IpcError, MAX_LINE_BYTES};
use crate::protocol::{Event, Request, Response};

/// Sans-IO server codec for parsing requests and encoding responses.
///
/// See `docs/specs/ipc-cli.md §2`.
#[derive(Clone, Debug, Default)]
pub struct IpcCodec {
    buf: Vec<u8>,
}

impl IpcCodec {
    /// Create a new, empty codec instance.
    pub fn new() -> Self {
        Self { buf: Vec::new() }
    }

    /// Create a codec with pre-allocated buffer capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: Vec::with_capacity(capacity),
        }
    }

    /// Feed incoming raw bytes from socket into internal buffer.
    ///
    /// Returns an error if the accumulated buffer exceeds `MAX_LINE_BYTES` without a newline.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), IpcError> {
        self.buf.extend_from_slice(bytes);
        if self.buf.len() > MAX_LINE_BYTES && !self.buf.contains(&b'\n') {
            self.buf.clear();
            return Err(IpcError::LineTooLong);
        }
        Ok(())
    }

    /// Return the number of buffered bytes waiting to be decoded.
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Return true if the buffer has no pending bytes.
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Clear internal buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Extract next line from buffer if a newline delimiter exists.
    /// Strips trailing `\r` and `\n`.
    fn next_line(&mut self) -> Result<Option<Vec<u8>>, IpcError> {
        loop {
            let pos = match self.buf.iter().position(|&b| b == b'\n') {
                Some(p) => p,
                None => {
                    if self.buf.len() > MAX_LINE_BYTES {
                        self.buf.clear();
                        return Err(IpcError::LineTooLong);
                    }
                    return Ok(None);
                }
            };

            if pos > MAX_LINE_BYTES {
                self.buf.drain(..=pos);
                return Err(IpcError::LineTooLong);
            }

            let mut line = self.buf.drain(..=pos).collect::<Vec<u8>>();
            // Remove trailing '\n'
            line.pop();
            // Remove trailing '\r' if present
            if line.last() == Some(&b'\r') {
                line.pop();
            }

            // Skip completely empty lines
            if line.is_empty() {
                continue;
            }

            return Ok(Some(line));
        }
    }

    /// Decode next complete request if newline is found.
    pub fn decode_request(&mut self) -> Result<Option<Request>, IpcError> {
        match self.next_line()? {
            Some(line) => {
                let req: Request = serde_json::from_slice(&line)?;
                Ok(Some(req))
            }
            None => Ok(None),
        }
    }

    /// Decode next complete response if newline is found.
    pub fn decode_response(&mut self) -> Result<Option<Response>, IpcError> {
        match self.next_line()? {
            Some(line) => {
                let resp: Response = serde_json::from_slice(&line)?;
                Ok(Some(resp))
            }
            None => Ok(None),
        }
    }

    /// Decode next complete event if newline is found.
    pub fn decode_event(&mut self) -> Result<Option<Event>, IpcError> {
        match self.next_line()? {
            Some(line) => {
                let ev: Event = serde_json::from_slice(&line)?;
                Ok(Some(ev))
            }
            None => Ok(None),
        }
    }

    /// Encode request into a newline-terminated byte vector.
    pub fn encode_request(req: &Request) -> Result<Vec<u8>, IpcError> {
        let mut bytes = serde_json::to_vec(req)?;
        bytes.push(b'\n');
        Ok(bytes)
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
