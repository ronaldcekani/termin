//! Terminal session management (`termin-pty::session`).
//!
//! See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §3`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use termin_core::TabId;

use crate::pty::{get_foreground_pgrp, PtyMaster};
use crate::reactor::ReactorCmd;
use crate::{Pid, PtyConsumer, SessionState, Signal};

/// Maximum write queue capacity per session (8 MiB) before runaway paste protection kicks in.
pub const MAX_WRITE_QUEUE_BYTES: usize = 8 * 1024 * 1024;

/// Normal drained write queue capacity (shrunk to <= 4 KiB).
pub const DRAINED_WRITE_QUEUE_CAPACITY: usize = 4096;

pub(crate) type ReactorCallback = Arc<dyn Fn(ReactorCmd) + Send + Sync>;

/// A terminal session managing one PTY and its child process.
///
/// See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §3`.
pub struct Session<C: PtyConsumer> {
    pub id: TabId,
    pub term: Mutex<C>,
    pub(crate) master: PtyMaster,
    pub(crate) pid: Pid,
    pub(crate) state: Mutex<SessionState>,
    pub(crate) effects: Mutex<Vec<C::Effect>>,
    pub(crate) write_queue: Mutex<VecDeque<u8>>,
    pub(crate) cmd_tx: Mutex<Option<ReactorCallback>>,
}

impl<C: PtyConsumer> Session<C> {
    /// Create a new session with an initial consumer, master PTY, and child PID.
    pub fn new(id: TabId, consumer: C, master: PtyMaster, pid: Pid) -> Self {
        Self {
            id,
            term: Mutex::new(consumer),
            master,
            pid,
            state: Mutex::new(SessionState::Running),
            effects: Mutex::new(Vec::new()),
            write_queue: Mutex::new(VecDeque::with_capacity(DRAINED_WRITE_QUEUE_CAPACITY)),
            cmd_tx: Mutex::new(None),
        }
    }

    /// Attach the reactor command notification callback.
    pub(crate) fn set_reactor_callback(&self, cb: ReactorCallback) {
        if let Ok(mut lock) = self.cmd_tx.lock() {
            *lock = Some(cb);
        }
    }

    /// Enqueue bytes to be written to PTY.
    ///
    /// If the write queue exceeds 8 MiB (runaway paste), writes are rejected with a warning.
    /// Flushed by the reactor background thread.
    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }

        let mut queue = match self.write_queue.lock() {
            Ok(q) => q,
            Err(_) => return,
        };

        if queue.len() + bytes.len() > MAX_WRITE_QUEUE_BYTES {
            log::warn!(
                "Session {}: write queue exceeded 8 MiB limit, rejected {} bytes",
                self.id.0,
                bytes.len()
            );
            return;
        }

        queue.extend(bytes);
        drop(queue);

        if let Ok(guard) = self.cmd_tx.lock() {
            if let Some(ref cb) = *guard {
                cb(ReactorCmd::ArmWrite(self.id));
            }
        }
    }

    /// Resize PTY terminal geometry.
    ///
    /// Updates `TIOCSWINSZ` on the master descriptor and notifies the attached consumer.
    pub fn resize(&self, cols: u16, rows: u16, px: (u16, u16)) {
        let _ = self.master.set_winsize(cols, rows, px);
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
    ///
    /// Returns `None` if the process has not attached the terminal yet or if `tcgetpgrp` fails.
    pub fn foreground_pid(&self) -> Option<Pid> {
        get_foreground_pgrp(self.master.raw_fd())
    }

    /// Send signal to child session process group.
    pub fn kill(&self, sig: Signal) {
        let pgrp = self.foreground_pid().unwrap_or(self.pid);
        let pid_val = rustix::process::Pid::from_raw(pgrp.0);
        if let Some(p) = pid_val {
            let rustix_sig = match sig.0 {
                1 => Some(rustix::process::Signal::Hup),
                2 => Some(rustix::process::Signal::Int),
                9 => Some(rustix::process::Signal::Kill),
                15 => Some(rustix::process::Signal::Term),
                _ => None,
            };
            if let Some(s) = rustix_sig {
                let _ = rustix::process::kill_process_group(p, s);
            }
        }

        // Notify reactor for potential kill escalation (SIGHUP -> 2s -> SIGKILL)
        if let Ok(guard) = self.cmd_tx.lock() {
            if let Some(ref cb) = *guard {
                cb(ReactorCmd::ScheduleKillEscalation {
                    tab_id: self.id,
                    pid: self.pid,
                });
            }
        }
    }

    /// Current execution state (running or exited with code).
    pub fn state(&self) -> SessionState {
        *self.state.lock().unwrap()
    }
}
