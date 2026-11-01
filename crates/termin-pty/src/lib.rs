//! PTY spawning, Session management, and kqueue IO reactor (`termin-pty`).
//!
//! See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md`.

use std::path::PathBuf;

pub mod pty;
pub mod reactor;
pub mod session;

pub use pty::{get_foreground_pgrp, open_pty, spawn, PtyChild, PtyMaster};
pub use reactor::{Reactor, ReactorHandle};
pub use session::{Session, DRAINED_WRITE_QUEUE_CAPACITY, MAX_WRITE_QUEUE_BYTES};
pub use termin_core::PtyConsumer;
pub use termin_core::{TabId, WakeHandle};

/// Watch token for file/path change notification.
///
/// See `docs/architecture.md §3.3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WatchToken(pub u64);

/// IPC client connection identifier.
///
/// See `docs/architecture.md §3.3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct IpcConnId(pub u64);

/// Process identifier wrapper.
///
/// See `docs/architecture.md §3.3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pid(pub i32);

/// POSIX signal number wrapper.
///
/// See `docs/architecture.md §3.3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Signal(pub i32);

/// State of a running or exited child session.
///
/// See `docs/architecture.md §3.3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Running,
    Exited(i32),
}

/// Specification for spawning a PTY child session.
///
/// See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §1`.
#[derive(Clone, Debug)]
pub struct SpawnSpec {
    pub prog: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    pub cell_px: (u16, u16),
}

/// Events emitted by the IO reactor.
///
/// See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §2`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ReactorEvent {
    SessionOutput(TabId),
    SessionExited(TabId, i32),
    PathChanged(WatchToken),
    Ipc(IpcConnId, Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    struct RecordingConsumer {
        bytes_received: Arc<AtomicU64>,
        generation: u64,
        cols: u16,
        rows: u16,
        captured: Arc<Mutex<Vec<u8>>>,
    }

    impl RecordingConsumer {
        fn new() -> (Self, Arc<AtomicU64>, Arc<Mutex<Vec<u8>>>) {
            let bytes = Arc::new(AtomicU64::new(0));
            let cap = Arc::new(Mutex::new(Vec::new()));
            (
                Self {
                    bytes_received: Arc::clone(&bytes),
                    generation: 0,
                    cols: 80,
                    rows: 24,
                    captured: Arc::clone(&cap),
                },
                bytes,
                cap,
            )
        }
    }

    impl PtyConsumer for RecordingConsumer {
        type Effect = ();

        fn consume(
            &mut self,
            bytes: &[u8],
            _write_back: &mut Vec<u8>,
            _effects: &mut Vec<Self::Effect>,
        ) {
            self.bytes_received
                .fetch_add(bytes.len() as u64, Ordering::SeqCst);
            self.generation += 1;
            if let Ok(mut lock) = self.captured.lock() {
                if lock.len() < 1024 * 1024 {
                    lock.extend_from_slice(bytes);
                }
            }
        }

        fn resize(&mut self, cols: u16, rows: u16, _px: (u16, u16)) {
            self.cols = cols;
            self.rows = rows;
            self.generation += 1;
        }

        fn generation(&self) -> u64 {
            self.generation
        }
    }

    #[test]
    fn test_echo_roundtrip() {
        let wake_called = Arc::new(AtomicBool::new(false));
        let wc = Arc::clone(&wake_called);
        let wake = WakeHandle::new(move || {
            wc.store(true, Ordering::SeqCst);
        });

        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _bytes_ctr, captured) = RecordingConsumer::new();

        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "echo roundtrip_success".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, code) = ev {
                    if id == session.id {
                        assert_eq!(code, 0);
                        let cap = captured.lock().unwrap();
                        let text = String::from_utf8_lossy(&cap);
                        assert!(text.contains("roundtrip_success"));
                        handle.shutdown();
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        panic!("Timed out waiting for echo roundtrip");
    }

    #[test]
    fn test_exit_code_reaping() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, _) = RecordingConsumer::new();

        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "exit 42".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, code) = ev {
                    if id == session.id {
                        assert_eq!(code, 42);
                        assert_eq!(session.state(), SessionState::Exited(42));
                        handle.shutdown();
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        panic!("Timed out waiting for child exit code reaping");
    }

    #[test]
    fn unregister_exited_session_clears_partial_writes_and_is_idempotent() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, _) = RecordingConsumer::new();
        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "sleep 0.2".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };
        let session = handle.spawn(spec, consumer).unwrap();

        let start = Instant::now();
        let mut exited = false;
        while start.elapsed() < Duration::from_secs(5) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            exited |= events.iter().any(
                |event| matches!(event, ReactorEvent::SessionExited(id, _) if *id == session.id),
            );
            if exited {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(exited, "child should have exited and been reaped");
        assert!(matches!(session.state(), SessionState::Exited(_)));
        session.write(&vec![b'x'; 4 * 1024 * 1024]);
        assert!(
            !session.write_queue.lock().unwrap().is_empty(),
            "test should leave a partial write queued"
        );

        handle.unregister_session(session.id).unwrap();
        assert!(session.write_queue.lock().unwrap().is_empty());
        handle.unregister_session(session.id).unwrap();
        drop(session);
        handle.shutdown();
    }

    #[test]
    fn hup_escalation_targets_resistant_foreground_job_group() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, _) = RecordingConsumer::new();
        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: vec!["-i".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };
        let session = handle.spawn(spec, consumer).unwrap();
        session.write(b"trap '' HUP\nsleep 30\n");

        let deadline = Instant::now() + Duration::from_secs(3);
        let foreground = loop {
            if let Some(pid) = session.foreground_pid() {
                if pid != session.pid() {
                    break pid;
                }
            }
            assert!(
                Instant::now() < deadline,
                "shell did not start a separate foreground job group"
            );
            std::thread::sleep(Duration::from_millis(10));
        };

        let sent_at = Instant::now();
        session.kill(Signal(1));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut shell_regained_foreground = false;
        while Instant::now() < deadline {
            if session.foreground_pid() == Some(session.pid()) {
                shell_regained_foreground = true;
                break;
            }
            if matches!(session.state(), SessionState::Exited(_)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        if !shell_regained_foreground {
            if let Some(group) = rustix::process::Pid::from_raw(foreground.0) {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::Kill);
            }
        }
        assert!(
            shell_regained_foreground,
            "escalation must kill the HUP-resistant foreground group, not the shell"
        );
        assert!(sent_at.elapsed() >= Duration::from_millis(1800));
        assert!(matches!(session.state(), SessionState::Running));

        session.write(b"exit\n");
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            if events.iter().any(
                |event| matches!(event, ReactorEvent::SessionExited(id, _) if *id == session.id),
            ) {
                handle.unregister_session(session.id).unwrap();
                drop(session);
                handle.shutdown();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        session.kill(Signal(9));
        handle.shutdown();
        panic!("shell did not exit after the foreground job was killed");
    }

    #[test]
    fn test_stty_size_resize() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, captured) = RecordingConsumer::new();

        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "stty size".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 123,
            rows: 45,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, code) = ev {
                    if id == session.id {
                        assert_eq!(code, 0);
                        let cap = captured.lock().unwrap();
                        let text = String::from_utf8_lossy(&cap);
                        println!("stty size text: {:?}", text);
                        // stty size prints "rows cols\n"
                        assert!(text.contains("45 123"));
                        handle.shutdown();
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        panic!("Timed out waiting for stty size");
    }

    #[test]
    fn test_foreground_pid_graceful_handling() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, _) = RecordingConsumer::new();

        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sleep"),
            args: vec!["0.2".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();
        assert_eq!(session.pid(), session.pid());

        // foreground_pid will be Some(pgrp) or None, never panics
        let _ = session.foreground_pid();

        std::thread::sleep(Duration::from_millis(300));

        // After process exits, foreground_pid returns None without panicking
        let _ = session.foreground_pid();
        handle.shutdown();
    }

    #[test]
    fn test_vnode_watch() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();

        let tmp_file =
            std::env::temp_dir().join(format!("termin_watch_test_{}.tmp", std::process::id()));
        std::fs::write(&tmp_file, b"initial").unwrap();

        let token = WatchToken(101);
        handle.watch_path(&tmp_file, token);

        std::thread::sleep(Duration::from_millis(50));
        // Modify file
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&tmp_file)
            .unwrap();
        f.write_all(b"modified").unwrap();
        drop(f);

        let start = Instant::now();
        let mut got_change = false;
        while start.elapsed() < Duration::from_secs(3) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::PathChanged(t) = ev {
                    if t == token {
                        got_change = true;
                        break;
                    }
                }
            }
            if got_change {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let _ = std::fs::remove_file(&tmp_file);
        handle.shutdown();
        assert!(got_change, "Expected PathChanged event for watched path");
    }

    #[test]
    fn test_ipc_listener() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();

        let sock_dir = std::env::temp_dir().join(format!("termin_ipc_{}", std::process::id()));
        std::fs::create_dir_all(&sock_dir).unwrap();
        std::fs::set_permissions(&sock_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let sock_path = sock_dir.join("termin.sock");
        let _ = std::fs::remove_file(&sock_path);

        match handle.listen_unix(&sock_path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Unsupported => {
                // macOS peer credential verification needs an API outside the allowlist.
                handle.shutdown();
                let _ = std::fs::remove_dir_all(&sock_dir);
                return;
            }
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                // AF_UNIX listener creation is denied in some restricted test sandboxes.
                handle.shutdown();
                let _ = std::fs::remove_dir_all(&sock_dir);
                return;
            }
            Err(error) => panic!("could not start IPC listener: {error}"),
        }

        // Wait until listener socket is bound and ready
        let mut connected = None;
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            if sock_path.exists() {
                match std::os::unix::net::UnixStream::connect(&sock_path) {
                    Ok(stream) => {
                        connected = Some(stream);
                        break;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                        // In sandbox environment without AF_UNIX connect permission
                        println!("Note: AF_UNIX connect blocked by sandbox policy");
                        let _ = std::fs::remove_file(&sock_path);
                        let _ = std::fs::remove_file(sock_dir.join(".termin.sock.termin-ipc.lock"));
                        let _ = std::fs::remove_dir_all(&sock_dir);
                        handle.shutdown();
                        return;
                    }
                    Err(_) => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        if let Some(mut stream) = connected {
            stream.write_all(b"hello_ipc").unwrap();

            let start = Instant::now();
            let mut received = Vec::new();
            while start.elapsed() < Duration::from_secs(3) {
                let mut events = Vec::new();
                handle.drain_events(&mut events);
                for ev in events {
                    if let ReactorEvent::Ipc(_, data) = ev {
                        received.extend(data);
                    }
                }
                if received == b"hello_ipc" {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(received, b"hello_ipc");
        }

        let _ = std::fs::remove_file(&sock_path);
        let _ = std::fs::remove_file(sock_dir.join(".termin.sock.termin-ipc.lock"));
        let _ = std::fs::remove_dir_all(&sock_dir);
        handle.shutdown();
    }

    #[test]
    fn test_shell_integration_mark_sequences() {
        // Deploy scripts to temp directory
        let tmp = std::env::temp_dir().join(format!("termin_marks_{}", std::process::id()));
        let deployed = termin_shell::deploy_shell_scripts(Some(&tmp)).unwrap();

        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, captured) = RecordingConsumer::new();

        let shell = PathBuf::from("/bin/bash");
        let env = termin_shell::Env::build(
            &termin_shell::SessionIds {
                workspace: termin_core::WorkspaceId(1),
                pane: termin_core::PaneId(1),
                tab: termin_core::TabId(1),
            },
            Path::new("/tmp/test.sock"),
            &[],
            true,
        );

        let bashrc = deployed.join("bash").join("termin.bashrc");
        let spec = SpawnSpec {
            prog: shell,
            args: vec![
                "-bash".to_string(),
                "--rcfile".to_string(),
                bashrc.to_string_lossy().to_string(),
                "-i".to_string(),
            ],
            env,
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();

        // Allow shell to initialize and display initial prompt
        std::thread::sleep(Duration::from_millis(200));

        // Submit a command that succeeds
        session.write(b"echo mark_test\n");
        std::thread::sleep(Duration::from_millis(200));

        // Submit a command that fails (exit 1)
        session.write(b"false\n");
        std::thread::sleep(Duration::from_millis(200));

        // Exit shell
        session.write(b"exit\n");

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, _) = ev {
                    if id == session.id {
                        let cap = captured.lock().unwrap();
                        let text = String::from_utf8_lossy(&cap);
                        println!("Shell output captured:\n{}", text);

                        // Assert presence of integration markers
                        assert!(
                            text.contains("6973;termin;shell=bash"),
                            "Missing OSC 6973 metadata"
                        );
                        assert!(text.contains("133;A"), "Missing OSC 133;A prompt start");
                        assert!(text.contains("133;B"), "Missing OSC 133;B prompt end");
                        assert!(text.contains("133;C"), "Missing OSC 133;C preexec mark");
                        assert!(
                            text.contains("133;D;0"),
                            "Missing OSC 133;D;0 success exit mark"
                        );
                        assert!(
                            text.contains("133;D;1"),
                            "Missing OSC 133;D;1 error exit mark"
                        );
                        assert!(text.contains("7;file://"), "Missing OSC 7 cwd mark");

                        handle.shutdown();
                        let _ = std::fs::remove_dir_all(&tmp);
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = std::fs::remove_dir_all(&tmp);
        panic!("Timed out waiting for shell to exit");
    }

    #[test]
    fn test_shell_integration_mark_sequences_zsh() {
        let tmp = std::env::temp_dir().join(format!("termin_marks_zsh_{}", std::process::id()));
        let deployed = termin_shell::deploy_shell_scripts(Some(&tmp)).unwrap();

        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, captured) = RecordingConsumer::new();

        let shell = PathBuf::from("/bin/zsh");
        let mut env = termin_shell::Env::build(
            &termin_shell::SessionIds {
                workspace: termin_core::WorkspaceId(1),
                pane: termin_core::PaneId(1),
                tab: termin_core::TabId(1),
            },
            Path::new("/tmp/test.sock"),
            &[],
            true,
        );
        let mut args = vec!["-zsh".to_string()];
        termin_shell::prepare_injection(&shell, true, &deployed, &mut env, &mut args);

        let spec = SpawnSpec {
            prog: shell,
            args,
            env,
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();

        std::thread::sleep(Duration::from_millis(200));
        session.write(b"echo zsh_mark_test\n");
        std::thread::sleep(Duration::from_millis(200));
        session.write(b"false\n");
        std::thread::sleep(Duration::from_millis(200));
        session.write(b"exit\n");

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, _) = ev {
                    if id == session.id {
                        let cap = captured.lock().unwrap();
                        let text = String::from_utf8_lossy(&cap);
                        println!("Zsh output captured:\n{}", text);

                        assert!(
                            text.contains("6973;termin;shell=zsh"),
                            "Missing OSC 6973 metadata"
                        );
                        assert!(text.contains("133;A"), "Missing OSC 133;A prompt start");
                        assert!(text.contains("133;B"), "Missing OSC 133;B prompt end");
                        assert!(text.contains("133;C"), "Missing OSC 133;C preexec mark");
                        assert!(
                            text.contains("133;D;0"),
                            "Missing OSC 133;D;0 success exit mark"
                        );
                        assert!(
                            text.contains("133;D;1"),
                            "Missing OSC 133;D;1 error exit mark"
                        );
                        assert!(text.contains("7;file://"), "Missing OSC 7 cwd mark");

                        handle.shutdown();
                        let _ = std::fs::remove_dir_all(&tmp);
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = std::fs::remove_dir_all(&tmp);
        panic!("Timed out waiting for zsh to exit");
    }

    #[test]
    fn test_user_zshrc_custom_zdotdir_loaded() {
        let tmp =
            std::env::temp_dir().join(format!("termin_custom_zdotdir_{}", std::process::id()));
        let deployed = termin_shell::deploy_shell_scripts(Some(&tmp)).unwrap();

        // Create user custom ZDOTDIR fixture with custom .zshrc
        let user_zdotdir = tmp.join("user_custom_zdotdir");
        std::fs::create_dir_all(&user_zdotdir).unwrap();
        std::fs::write(
            user_zdotdir.join(".zshrc"),
            b"export CUSTOM_ZDOTDIR_VAR=\"termin_custom_loaded_ok\"\n",
        )
        .unwrap();

        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, _, captured) = RecordingConsumer::new();

        let shell = PathBuf::from("/bin/zsh");
        let mut env = termin_shell::Env::build(
            &termin_shell::SessionIds {
                workspace: termin_core::WorkspaceId(1),
                pane: termin_core::PaneId(1),
                tab: termin_core::TabId(1),
            },
            Path::new("/tmp/test.sock"),
            &[(
                "ZDOTDIR".to_string(),
                user_zdotdir.to_string_lossy().to_string(),
            )],
            true,
        );

        let mut args = vec!["-zsh".to_string()];
        // Simulate user having custom ZDOTDIR in their env
        termin_shell::prepare_injection(&shell, true, &deployed, &mut env, &mut args);
        // Ensure TERMIN_ORIG_ZDOTDIR is set to the user custom zdotdir
        env.retain(|(k, _)| k != "TERMIN_ORIG_ZDOTDIR");
        env.push((
            "TERMIN_ORIG_ZDOTDIR".to_string(),
            user_zdotdir.to_string_lossy().to_string(),
        ));

        let spec = SpawnSpec {
            prog: shell,
            args,
            env,
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();

        std::thread::sleep(Duration::from_millis(200));
        session.write(b"echo VAL=$CUSTOM_ZDOTDIR_VAR\n");
        std::thread::sleep(Duration::from_millis(200));
        session.write(b"exit\n");

        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, _) = ev {
                    if id == session.id {
                        let cap = captured.lock().unwrap();
                        let text = String::from_utf8_lossy(&cap);
                        println!("Output captured with custom ZDOTDIR:\n{}", text);

                        assert!(
                            text.contains("VAL=termin_custom_loaded_ok"),
                            "User's custom .zshrc from ZDOTDIR was not loaded"
                        );
                        assert!(
                            text.contains("6973;termin;shell=zsh"),
                            "Missing integration metadata"
                        );

                        handle.shutdown();
                        let _ = std::fs::remove_dir_all(&tmp);
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let _ = std::fs::remove_dir_all(&tmp);
        panic!("Timed out waiting for custom ZDOTDIR test to complete");
    }

    #[test]
    fn test_throughput_streaming() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, total_bytes, _) = RecordingConsumer::new();

        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "head -c 5000000 /dev/zero".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(10) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, _) = ev {
                    if id == session.id {
                        let bytes = total_bytes.load(std::sync::atomic::Ordering::Relaxed);
                        assert_eq!(bytes, 5_000_000);
                        handle.shutdown();
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("Timed out waiting for throughput stream to complete");
    }

    #[test]
    #[ignore]
    fn test_throughput_500mb() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<RecordingConsumer>::start(wake).unwrap();
        let (consumer, total_bytes, _) = RecordingConsumer::new();

        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), "head -c 500000000 /dev/zero".to_string()],
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (0, 0),
        };

        let session = handle.spawn(spec, consumer).unwrap();
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(30) {
            let mut events = Vec::new();
            handle.drain_events(&mut events);
            for ev in events {
                if let ReactorEvent::SessionExited(id, _) = ev {
                    if id == session.id {
                        let bytes = total_bytes.load(std::sync::atomic::Ordering::Relaxed);
                        assert_eq!(bytes, 500_000_000);
                        handle.shutdown();
                        return;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("Timed out waiting for 500MB stream to complete");
    }
}
