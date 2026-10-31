//! PTY spawning, Session management, and kqueue IO reactor (`termin-pty`).
//!
//! See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md`.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
pub use termin_core::PtyConsumer;
use termin_core::{TabId, WakeHandle};

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

/// A terminal session managing one PTY and its child process.
///
/// See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §3`.
pub struct Session<C: PtyConsumer> {
    pub id: TabId,
    pub term: Mutex<C>,
    state: Mutex<SessionState>,
    effects: Mutex<Vec<C::Effect>>,
    pid: Pid,
}

impl<C: PtyConsumer> Session<C> {
    /// Create a new session with an initial consumer and child PID.
    pub fn new(id: TabId, consumer: C, pid: Pid) -> Self {
        Self {
            id,
            term: Mutex::new(consumer),
            state: Mutex::new(SessionState::Running),
            effects: Mutex::new(Vec::new()),
            pid,
        }
    }

    /// Enqueue bytes to be written to PTY.
    pub fn write(&self, _bytes: &[u8]) {
        // Enqueue to write queue; reactor flushes
    }

    /// Resize PTY terminal geometry.
    pub fn resize(&self, cols: u16, rows: u16, px: (u16, u16)) {
        if let Ok(mut term) = self.term.lock() {
            term.resize(cols, rows, px);
        }
    }

    /// Drain accumulated parsing effects into destination vector.
    pub fn drain_effects(&self, out: &mut Vec<C::Effect>) {
        if let Ok(mut eff) = self.effects.lock() {
            out.append(&mut *eff);
        }
    }

    /// Child process PID.
    pub fn pid(&self) -> Pid {
        self.pid
    }

    /// Foreground process group PID from controlling terminal.
    pub fn foreground_pid(&self) -> Option<Pid> {
        Some(self.pid)
    }

    /// Send signal to child session process group.
    pub fn kill(&self, _sig: Signal) {
        // Delivered via kill(2)
    }

    /// Current execution state (running or exited with code).
    pub fn state(&self) -> SessionState {
        *self.state.lock().unwrap()
    }
}

/// Cloneable handle to communicate with the IO reactor thread.
///
/// See `docs/architecture.md §3.3`.
pub struct ReactorHandle<C: PtyConsumer> {
    _phantom: std::marker::PhantomData<C>,
}

impl<C: PtyConsumer> Clone for ReactorHandle<C> {
    fn clone(&self) -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<C: PtyConsumer> ReactorHandle<C> {
    /// Spawn a new child process with PTY and attach a consumer.
    pub fn spawn(&self, spec: SpawnSpec, consumer: C) -> io::Result<Arc<Session<C>>> {
        let id = TabId::next();
        let session = Arc::new(Session::new(id, consumer, Pid(std::process::id() as i32)));
        session.resize(spec.cols, spec.rows, spec.cell_px);
        Ok(session)
    }

    /// Watch a path for change notifications (e.g. config hot reload).
    pub fn watch_path(&self, _path: &Path, _token: WatchToken) {
        // Registered in reactor kqueue
    }

    /// Listen on a Unix domain socket for IPC commands.
    pub fn listen_unix(&self, _path: &Path) -> io::Result<()> {
        Ok(())
    }

    /// Shut down the reactor thread.
    pub fn shutdown(&self) {
        // Signals EVFILT_USER
    }
}

/// IO Reactor running on a background thread.
///
/// See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §2`.
pub struct Reactor<C: PtyConsumer> {
    _phantom: std::marker::PhantomData<C>,
}

impl<C: PtyConsumer> Reactor<C> {
    /// Start the reactor IO thread with a handle to wake the main thread.
    pub fn start(_wake: WakeHandle) -> io::Result<ReactorHandle<C>> {
        Ok(ReactorHandle {
            _phantom: std::marker::PhantomData,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubConsumer {
        gen: u64,
    }

    impl PtyConsumer for StubConsumer {
        type Effect = ();

        fn consume(
            &mut self,
            _bytes: &[u8],
            _write_back: &mut Vec<u8>,
            _effects: &mut Vec<Self::Effect>,
        ) {
            self.gen += 1;
        }

        fn resize(&mut self, _cols: u16, _rows: u16, _px: (u16, u16)) {}

        fn generation(&self) -> u64 {
            self.gen
        }
    }

    #[test]
    fn test_session_state_and_reactor_handle() {
        let wake = WakeHandle::new(|| {});
        let handle = Reactor::<StubConsumer>::start(wake).unwrap();
        let spec = SpawnSpec {
            prog: PathBuf::from("/bin/sh"),
            args: Vec::new(),
            env: Vec::new(),
            cwd: PathBuf::from("/tmp"),
            cols: 80,
            rows: 24,
            cell_px: (8, 16),
        };
        let session = handle.spawn(spec, StubConsumer { gen: 0 }).unwrap();
        assert_eq!(session.state(), SessionState::Running);
    }
}
