//! JSON-lines IPC protocol, client, and sans-IO server codec (`termin-ipc`).
//!
//! See `docs/specs/ipc-cli.md`.

pub mod client;
pub mod codec;
pub mod error;
pub mod protocol;

pub use client::{default_socket_path, resolve_socket_path, Client, IpcClient};
pub use codec::IpcCodec;
pub use error::{IpcError, MAX_LINE_BYTES};
pub use protocol::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::thread;

    #[test]
    fn test_codec_request_round_trip() {
        let mut codec = IpcCodec::new();
        let req = Request::new(
            42,
            "split",
            serde_json::json!({"direction": "right", "pane": 12}),
        );
        let encoded = IpcCodec::encode_request(&req).unwrap();
        assert!(encoded.ends_with(b"\n"));

        codec.feed(&encoded).unwrap();
        let decoded = codec.decode_request().unwrap().expect("request decoded");
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.method, "split");
        assert_eq!(decoded.params["direction"], "right");
        assert_eq!(decoded.params["pane"], 12);
    }

    #[test]
    fn test_codec_response_and_event_round_trip() {
        let mut codec = IpcCodec::new();

        // Response
        let resp = Response::ok(1, serde_json::json!({"version": "0.1.0"}));
        let encoded_resp = IpcCodec::encode_response(&resp).unwrap();
        codec.feed(&encoded_resp).unwrap();
        let decoded_resp = codec.decode_response().unwrap().expect("response decoded");
        assert_eq!(decoded_resp.id, 1);
        assert_eq!(decoded_resp.result.unwrap()["version"], "0.1.0");

        // Error response
        let err_resp = Response::error(2, "not_found", "pane 99");
        let encoded_err = IpcCodec::encode_response(&err_resp).unwrap();
        codec.feed(&encoded_err).unwrap();
        let decoded_err = codec
            .decode_response()
            .unwrap()
            .expect("error response decoded");
        assert_eq!(decoded_err.id, 2);
        assert_eq!(decoded_err.error.unwrap().code, "not_found");

        // Event
        let event = Event::new("attention", serde_json::json!({"tab": 5}));
        let encoded_ev = IpcCodec::encode_event(&event).unwrap();
        codec.feed(&encoded_ev).unwrap();
        let decoded_ev = codec.decode_event().unwrap().expect("event decoded");
        assert_eq!(decoded_ev.event, "attention");
        assert_eq!(decoded_ev.data["tab"], 5);
    }

    #[test]
    fn test_codec_line_too_long() {
        let mut codec = IpcCodec::new();
        // Buffer larger than 1 MiB without newline
        let huge = vec![b'a'; MAX_LINE_BYTES + 10];
        let res = codec.feed(&huge);
        assert!(
            res.is_err(),
            "Exceeding max line without newline should error"
        );
    }

    #[test]
    fn test_codec_partial_chunks() {
        let mut codec = IpcCodec::new();
        let json = b"{\"id\":10,\"method\":\"ping\",\"params\":{}}\n";

        // Feed byte by byte
        for &b in &json[..json.len() - 1] {
            codec.feed(&[b]).unwrap();
            assert!(codec.decode_request().unwrap().is_none());
        }

        // Feed terminating newline
        codec.feed(b"\n").unwrap();
        let req = codec
            .decode_request()
            .unwrap()
            .expect("should decode after newline");
        assert_eq!(req.id, 10);
        assert_eq!(req.method, "ping");
    }

    #[test]
    fn test_codec_malformed_json_recovery() {
        let mut codec = IpcCodec::new();
        codec
            .feed(b"this is not valid json\n{\"id\":1,\"method\":\"ok\",\"params\":{}}\n")
            .unwrap();

        // First line is malformed
        let err = codec.decode_request();
        assert!(err.is_err());

        // Second line succeeds
        let next = codec
            .decode_request()
            .unwrap()
            .expect("valid request decoded");
        assert_eq!(next.id, 1);
        assert_eq!(next.method, "ok");
    }

    #[test]
    fn test_client_server_unix_socket() {
        let temp_dir = PathBuf::from("/tmp");
        let socket_name = format!("termin-test-{}.sock", std::process::id());
        let socket_path = temp_dir.join(socket_name);
        let _ = std::fs::remove_file(&socket_path);

        // Attempt real Unix domain socket bind/connect.
        // If sandbox environment prevents connect() with PermissionDenied, use UnixStream::pair().
        let bind_res = UnixListener::bind(&socket_path);
        let (server_stream, client_stream) = match bind_res {
            Ok(listener) => {
                let s_path = socket_path.clone();
                let server_thread = thread::spawn(move || {
                    let (stream, _) = listener.accept()?;
                    Ok::<_, std::io::Error>(stream)
                });

                match std::os::unix::net::UnixStream::connect(&s_path) {
                    Ok(c_stream) => {
                        let s_stream = server_thread.join().unwrap().expect("accept ok");
                        (s_stream, c_stream)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        let _ = std::fs::remove_file(&s_path);
                        // Fallback to socket pair in sandboxed execution
                        let (s, c) = UnixStream::pair().expect("socketpair");
                        (s, c)
                    }
                    Err(e) => panic!("connect failed: {:?}", e),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                let (s, c) = UnixStream::pair().expect("socketpair");
                (s, c)
            }
            Err(e) => panic!("bind failed: {:?}", e),
        };

        let handle = thread::spawn(move || {
            let mut reader = BufReader::new(server_stream.try_clone().unwrap());
            let mut writer = server_stream;

            // Handle ping
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let req: Request = serde_json::from_str(&line).unwrap();
            assert_eq!(req.method, "ping");

            let resp = Response::ok(req.id, serde_json::json!({"version": "0.1.0"}));
            let encoded = IpcCodec::encode_response(&resp).unwrap();
            writer.write_all(&encoded).unwrap();
            writer.flush().unwrap();

            // Handle split
            line.clear();
            reader.read_line(&mut line).unwrap();
            let req2: Request = serde_json::from_str(&line).unwrap();
            assert_eq!(req2.method, "split");
            let resp2 = Response::ok(req2.id, serde_json::json!({"pane": 2, "tab": 3}));
            let encoded2 = IpcCodec::encode_response(&resp2).unwrap();
            writer.write_all(&encoded2).unwrap();
            writer.flush().unwrap();

            // Handle error case
            line.clear();
            reader.read_line(&mut line).unwrap();
            let req3: Request = serde_json::from_str(&line).unwrap();
            let resp3 = Response::error(req3.id, "not_found", "pane 99 does not exist");
            let encoded3 = IpcCodec::encode_response(&resp3).unwrap();
            writer.write_all(&encoded3).unwrap();
            writer.flush().unwrap();
        });

        let mut client = Client::from_stream(client_stream).expect("client init");

        // 1. Call ping
        let version = client.ping().expect("ping succeeds");
        assert_eq!(version, "0.1.0");

        // 2. Call split
        let res = client
            .call("split", serde_json::json!({"direction": "right"}))
            .expect("split call succeeds");
        assert_eq!(res["pane"], 2);
        assert_eq!(res["tab"], 3);

        // 3. Call with error response
        let err = client
            .call("split", serde_json::json!({"pane": 99}))
            .expect_err("should return RPC error");
        match err {
            IpcError::Rpc(e) => {
                assert_eq!(e.code, "not_found");
                assert_eq!(e.message, "pane 99 does not exist");
            }
            other => panic!("expected Rpc error, got {:?}", other),
        }

        handle.join().unwrap();
        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn test_single_instance_try_connect() {
        let non_existent = PathBuf::from("/tmp/termin-non-existent-socket-12345.sock");
        let res = Client::try_connect(&non_existent);
        assert!(res.is_err());
        assert!(res.unwrap_err().is_not_running());
    }
}
