//! Single-threaded IO Reactor owning a kqueue (`termin-pty::reactor`).
//!
//! See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §2`.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;
use rustix::event::kqueue::{
    kevent, kqueue, Event, EventFilter, EventFlags, ProcessEvents, UserDefinedFlags, UserFlags,
    VnodeEvents,
};
use rustix::fs::{open, Mode, OFlags};
use rustix::process::{waitpid, WaitOptions};
use termin_core::{TabId, WakeHandle};

use crate::pty::spawn;
use crate::session::DRAINED_WRITE_QUEUE_CAPACITY;
use crate::{
    IpcConnId, Pid, PtyConsumer, ReactorEvent, Session, SessionState, SpawnSpec, WatchToken,
};

/// Commands sent to the reactor IO thread.
pub(crate) enum ReactorCmd {
    RegisterSession(Box<dyn ErasedSessionRegister>),
    ArmWrite(TabId),
    WatchPath {
        path: PathBuf,
        token: WatchToken,
    },
    ListenUnix {
        path: PathBuf,
    },
    ScheduleKillEscalation {
        tab_id: TabId,
        pid: Pid,
    },
    Shutdown,
}

pub(crate) trait ErasedSessionRegister: Send + Sync {
    fn register(self: Box<Self>, reactor: &mut ReactorLoop) -> io::Result<()>;
}

struct SessionRegistration<C: PtyConsumer> {
    session: Arc<Session<C>>,
}

impl<C: PtyConsumer> ErasedSessionRegister for SessionRegistration<C> {
    fn register(self: Box<Self>, reactor: &mut ReactorLoop) -> io::Result<()> {
        reactor.register_session(self.session)
    }
}

/// Cloneable handle to communicate with the IO reactor thread.
///
/// See `docs/architecture.md §3.3`.
pub struct ReactorHandle<C: PtyConsumer> {
    cmd_tx: Sender<ReactorCmd>,
    kq_raw: RawFd,
    events: Arc<Mutex<Vec<ReactorEvent>>>,
    wake_pending: Arc<AtomicBool>,
    _phantom: std::marker::PhantomData<C>,
}

impl<C: PtyConsumer> Clone for ReactorHandle<C> {
    fn clone(&self) -> Self {
        Self {
            cmd_tx: self.cmd_tx.clone(),
            kq_raw: self.kq_raw,
            events: Arc::clone(&self.events),
            wake_pending: Arc::clone(&self.wake_pending),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<C: PtyConsumer> ReactorHandle<C> {
    fn trigger_user_event(&self) {
        let trigger = Event::new(
            EventFilter::User {
                ident: 0,
                flags: UserFlags::TRIGGER,
                user_flags: UserDefinedFlags::new(0),
            },
            EventFlags::empty(),
            0,
        );
        unsafe {
            // SAFETY: Triggering EVFILT_USER on valid kqueue fd
            let res = kevent(
                BorrowedFd::borrow_raw(self.kq_raw),
                &[trigger],
                &mut Vec::new(),
                Some(Duration::ZERO),
            );
            if let Err(e) = res {
                eprintln!("trigger_user_event error: {:?}", e);
            }
        }
    }

    /// Spawn a new child process with PTY and attach a consumer.
    pub fn spawn(&self, spec: SpawnSpec, consumer: C) -> io::Result<Arc<Session<C>>> {
        let child = spawn(&spec)?;
        let id = TabId::next();
        let session = Arc::new(Session::new(id, consumer, child.master, child.pid));

        let tx = self.cmd_tx.clone();
        let kq_raw = self.kq_raw;
        let cb: Arc<dyn Fn(ReactorCmd) + Send + Sync> = Arc::new(move |cmd| {
            if tx.send(cmd).is_ok() {
                let trigger = Event::new(
                    EventFilter::User {
                        ident: 0,
                        flags: UserFlags::TRIGGER,
                        user_flags: UserDefinedFlags::new(0),
                    },
                    EventFlags::empty(),
                    0,
                );
                unsafe {
                    // SAFETY: Triggering user event from session callback
                    let _ = kevent(
                        BorrowedFd::borrow_raw(kq_raw),
                        &[trigger],
                        &mut Vec::new(),
                        Some(Duration::ZERO),
                    );
                }
            }
        });

        session.set_reactor_callback(cb);

        let reg = SessionRegistration {
            session: Arc::clone(&session),
        };
        self.cmd_tx
            .send(ReactorCmd::RegisterSession(Box::new(reg)))
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?;
        self.trigger_user_event();

        Ok(session)
    }

    /// Watch a path for change notifications (e.g. config hot reload).
    pub fn watch_path(&self, path: &Path, token: WatchToken) {
        let _ = self.cmd_tx.send(ReactorCmd::WatchPath {
            path: path.to_path_buf(),
            token,
        });
        self.trigger_user_event();
    }

    /// Listen on a Unix domain socket for IPC commands.
    pub fn listen_unix(&self, path: &Path) -> io::Result<()> {
        self.cmd_tx
            .send(ReactorCmd::ListenUnix {
                path: path.to_path_buf(),
            })
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?;
        self.trigger_user_event();
        Ok(())
    }

    /// Shut down the reactor thread.
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(ReactorCmd::Shutdown);
        self.trigger_user_event();
    }

    /// Drain accumulated reactor events without reallocating destination vector.
    pub fn drain_events(&self, out: &mut Vec<ReactorEvent>) {
        if let Ok(mut lock) = self.events.lock() {
            out.append(&mut *lock);
        }
        self.wake_pending.store(false, Ordering::SeqCst);
    }
}

/// Type-erased session wrapper for reactor state management.
trait ActiveSession: Send + Sync {
    fn master_fd(&self) -> RawFd;
    fn consume_bytes(&self, bytes: &[u8]) -> (usize, Vec<u8>);
    fn get_pending_writes(&self, out: &mut Vec<u8>) -> usize;
    fn advance_write_queue(&self, written: usize) -> bool;
    fn mark_exited(&self, code: i32);
}

impl<C: PtyConsumer> ActiveSession for Session<C> {
    fn master_fd(&self) -> RawFd {
        self.master.raw_fd()
    }

    fn consume_bytes(&self, bytes: &[u8]) -> (usize, Vec<u8>) {
        let mut write_back = Vec::new();
        let mut effects = Vec::new();

        if let Ok(mut term) = self.term.lock() {
            term.consume(bytes, &mut write_back, &mut effects);
        }

        let num_effects = effects.len();
        if !effects.is_empty() {
            if let Ok(mut eff_lock) = self.effects.lock() {
                eff_lock.append(&mut effects);
            }
        }

        (num_effects, write_back)
    }

    fn get_pending_writes(&self, out: &mut Vec<u8>) -> usize {
        if let Ok(queue) = self.write_queue.lock() {
            out.extend(queue.iter().copied());
            queue.len()
        } else {
            0
        }
    }

    fn advance_write_queue(&self, written: usize) -> bool {
        if let Ok(mut queue) = self.write_queue.lock() {
            let to_drain = written.min(queue.len());
            queue.drain(..to_drain);
            if queue.is_empty() {
                if queue.capacity() > DRAINED_WRITE_QUEUE_CAPACITY {
                    queue.shrink_to(DRAINED_WRITE_QUEUE_CAPACITY);
                }
                true
            } else {
                false
            }
        } else {
            true
        }
    }

    fn mark_exited(&self, code: i32) {
        if let Ok(mut st) = self.state.lock() {
            *st = SessionState::Exited(code);
        }
    }
}

/// Reactor IO loop state running on the `termin-io` thread.
pub(crate) struct ReactorLoop {
    kq: OwnedFd,
    cmd_rx: Receiver<ReactorCmd>,
    sessions: FxHashMap<TabId, Arc<dyn ActiveSession>>,
    fd_to_tab: FxHashMap<RawFd, TabId>,
    pid_to_tab: FxHashMap<i32, TabId>,
    write_armed: FxHashMap<TabId, bool>,
    vnodes: FxHashMap<RawFd, (WatchToken, PathBuf)>,
    vnode_debounce: FxHashMap<WatchToken, Instant>,
    kill_escalations: FxHashMap<TabId, (Pid, Instant)>,
    ipc_listeners: FxHashMap<RawFd, UnixListener>,
    ipc_clients: FxHashMap<RawFd, (IpcConnId, UnixStream)>,
    next_ipc_id: u64,
    events: Arc<Mutex<Vec<ReactorEvent>>>,
    wake_pending: Arc<AtomicBool>,
    wake: WakeHandle,
}

impl ReactorLoop {
    fn new(
        kq: OwnedFd,
        cmd_rx: Receiver<ReactorCmd>,
        events: Arc<Mutex<Vec<ReactorEvent>>>,
        wake_pending: Arc<AtomicBool>,
        wake: WakeHandle,
    ) -> Self {
        Self {
            kq,
            cmd_rx,
            sessions: FxHashMap::default(),
            fd_to_tab: FxHashMap::default(),
            pid_to_tab: FxHashMap::default(),
            write_armed: FxHashMap::default(),
            vnodes: FxHashMap::default(),
            vnode_debounce: FxHashMap::default(),
            kill_escalations: FxHashMap::default(),
            ipc_listeners: FxHashMap::default(),
            ipc_clients: FxHashMap::default(),
            next_ipc_id: 1,
            events,
            wake_pending,
            wake,
        }
    }

    fn register_session<C: PtyConsumer>(&mut self, session: Arc<Session<C>>) -> io::Result<()> {
        let tab_id = session.id;
        let master_fd = session.master.raw_fd();
        let pid = session.pid.0;

        let read_ev = Event::new(
            EventFilter::Read(master_fd),
            EventFlags::ADD | EventFlags::ENABLE,
            tab_id.0 as isize,
        );

        let proc_ev = Event::new(
            EventFilter::Proc {
                pid: rustix::process::Pid::from_raw(pid).unwrap(),
                flags: ProcessEvents::EXIT,
            },
            EventFlags::ADD | EventFlags::ENABLE,
            tab_id.0 as isize,
        );

        unsafe {
            // SAFETY: Registering master PTY and child PID filters on kqueue
            kevent(
                &self.kq,
                &[read_ev, proc_ev],
                &mut Vec::new(),
                Some(Duration::ZERO),
            )
            .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
        }

        self.sessions.insert(tab_id, session);
        self.fd_to_tab.insert(master_fd, tab_id);
        self.pid_to_tab.insert(pid, tab_id);
        self.write_armed.insert(tab_id, false);

        Ok(())
    }

    fn arm_write(&mut self, tab_id: TabId) {
        if let Some(session) = self.sessions.get(&tab_id) {
            let master_fd = session.master_fd();
            let armed = self.write_armed.get(&tab_id).copied().unwrap_or(false);
            if !armed {
                let write_ev = Event::new(
                    EventFilter::Write(master_fd),
                    EventFlags::ADD | EventFlags::ENABLE,
                    tab_id.0 as isize,
                );
                unsafe {
                    // SAFETY: Arming write event on master fd
                    let _ = kevent(
                        &self.kq,
                        &[write_ev],
                        &mut Vec::new(),
                        Some(Duration::ZERO),
                    );
                }
                self.write_armed.insert(tab_id, true);
            }
        }
    }

    fn disarm_write(&mut self, tab_id: TabId) {
        if let Some(session) = self.sessions.get(&tab_id) {
            let master_fd = session.master_fd();
            let armed = self.write_armed.get(&tab_id).copied().unwrap_or(false);
            if armed {
                let write_ev = Event::new(
                    EventFilter::Write(master_fd),
                    EventFlags::DELETE,
                    tab_id.0 as isize,
                );
                unsafe {
                    // SAFETY: Disarming write event on master fd
                    let _ = kevent(
                        &self.kq,
                        &[write_ev],
                        &mut Vec::new(),
                        Some(Duration::ZERO),
                    );
                }
                self.write_armed.insert(tab_id, false);
            }
        }
    }

    fn push_event(&mut self, ev: ReactorEvent) {
        if let Ok(mut lock) = self.events.lock() {
            lock.push(ev);
        }
        if !self.wake_pending.swap(true, Ordering::SeqCst) {
            self.wake.wake();
        }
    }

    fn run(&mut self) {
        let mut read_buf = [0u8; 65536];
        let mut write_buf = Vec::new();
        let mut kevent_list = Vec::with_capacity(64);

        loop {
            // Determine shortest timer for kevent timeout
            let now = Instant::now();
            let mut timeout: Option<Duration> = None;

            for deadline in self.vnode_debounce.values() {
                let d = deadline.saturating_duration_since(now);
                timeout = Some(timeout.map_or(d, |cur| cur.min(d)));
            }

            for (_, deadline) in self.kill_escalations.values() {
                let d = deadline.saturating_duration_since(now);
                timeout = Some(timeout.map_or(d, |cur| cur.min(d)));
            }

            kevent_list.clear();
            let _n = unsafe {
                // SAFETY: Polling kqueue with reusable event vector and calculated timeout
                match kevent(&self.kq, &[], &mut kevent_list, timeout) {
                    Ok(n) => n,
                    Err(e) => {
                        if e.raw_os_error() == 4 {
                            // EINTR
                            0
                        } else {
                            log::error!("kevent failed: {:?}", e);
                            0
                        }
                    }
                }
            };

            // Process timed-out vnode debounces
            let now = Instant::now();
            let mut fired_tokens = Vec::new();
            self.vnode_debounce.retain(|token, deadline| {
                if now >= *deadline {
                    fired_tokens.push(*token);
                    false
                } else {
                    true
                }
            });
            for tok in fired_tokens {
                self.push_event(ReactorEvent::PathChanged(tok));
            }

            // Process kill escalations (SIGHUP -> SIGKILL after 2s)
            let mut dead_tabs = Vec::new();
            self.kill_escalations.retain(|tab_id, (pid, deadline)| {
                if now >= *deadline {
                    if let Some(p) = rustix::process::Pid::from_raw(pid.0) {
                        let _ = rustix::process::kill_process_group(
                            p,
                            rustix::process::Signal::Kill,
                        );
                    }
                    dead_tabs.push(*tab_id);
                    false
                } else {
                    true
                }
            });

            // Process triggered events
            for ev in &kevent_list {
                match ev.filter() {
                    EventFilter::Read(fd) => {
                        // Check if fd is a PTY master
                        if let Some(&tab_id) = self.fd_to_tab.get(&fd) {
                            if let Some(session) = self.sessions.get(&tab_id).cloned() {
                                let mut total_bytes = 0;
                                let start = Instant::now();
                                let mut had_output = false;

                                while total_bytes < 1024 * 1024 && start.elapsed().as_millis() < 4 {
                                    // SAFETY: fd is an open PTY master descriptor managed by reactor
                                    let b_fd = unsafe { BorrowedFd::borrow_raw(fd) };
                                    match rustix::io::read(b_fd, &mut read_buf) {
                                        Ok(0) => {
                                            let del_ev = Event::new(
                                                EventFilter::Read(fd),
                                                EventFlags::DELETE,
                                                0,
                                            );
                                            unsafe {
                                                // SAFETY: Deregistering EOF read filter on master fd
                                                let _ = kevent(
                                                    &self.kq,
                                                    &[del_ev],
                                                    &mut Vec::new(),
                                                    Some(Duration::ZERO),
                                                );
                                            }
                                            break;
                                        }
                                        Ok(bytes_read) => {
                                            total_bytes += bytes_read;
                                            had_output = true;
                                            let (_, write_back) = session.consume_bytes(&read_buf[..bytes_read]);
                                            if !write_back.is_empty() {
                                                // Feed write-back into session write queue
                                                // SAFETY: fd is an open PTY master descriptor managed by reactor
                                                let write_fd = unsafe { BorrowedFd::borrow_raw(fd) };
                                                let _ = rustix::io::write(write_fd, &write_back);
                                            }
                                        }
                                        Err(rustix::io::Errno::AGAIN) => break,
                                        Err(rustix::io::Errno::IO) => {
                                            let del_ev = Event::new(
                                                EventFilter::Read(fd),
                                                EventFlags::DELETE,
                                                0,
                                            );
                                            unsafe {
                                                // SAFETY: Deregistering EIO read filter on closed slave
                                                let _ = kevent(
                                                    &self.kq,
                                                    &[del_ev],
                                                    &mut Vec::new(),
                                                    Some(Duration::ZERO),
                                                );
                                            }
                                            break;
                                        }
                                        Err(_) => break,
                                    }
                                }

                                if had_output {
                                    self.push_event(ReactorEvent::SessionOutput(tab_id));
                                }
                            }
                        } else if let Some(listener) = self.ipc_listeners.get(&fd) {
                            // Accept new IPC connection
                            if let Ok((stream, _)) = listener.accept() {
                                let _ = stream.set_nonblocking(true);
                                let client_fd = stream.as_raw_fd();
                                let conn_id = IpcConnId(self.next_ipc_id);
                                self.next_ipc_id += 1;

                                let ev = Event::new(
                                    EventFilter::Read(client_fd),
                                    EventFlags::ADD | EventFlags::ENABLE,
                                    conn_id.0 as isize,
                                );
                                unsafe {
                                    // SAFETY: Registering new IPC client socket on kqueue
                                    let _ = kevent(
                                        &self.kq,
                                        &[ev],
                                        &mut Vec::new(),
                                        Some(Duration::ZERO),
                                    );
                                }
                                self.ipc_clients.insert(client_fd, (conn_id, stream));
                            }
                        } else if let Some((conn_id, stream)) = self.ipc_clients.get_mut(&fd) {
                            // Read from IPC client socket
                            use std::io::Read;
                            let mut buf = [0u8; 4096];
                            let cid = *conn_id;
                            let mut to_remove = false;
                            match stream.read(&mut buf) {
                                Ok(0) => {
                                    to_remove = true;
                                }
                                Ok(n) => {
                                    self.push_event(ReactorEvent::Ipc(cid, buf[..n].to_vec()));
                                }
                                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {}
                                Err(_) => {
                                    to_remove = true;
                                }
                            }
                            if to_remove {
                                self.ipc_clients.remove(&fd);
                            }
                        }
                    }
                    EventFilter::Write(fd) => {
                        if let Some(&tab_id) = self.fd_to_tab.get(&fd) {
                            if let Some(session) = self.sessions.get(&tab_id).cloned() {
                                write_buf.clear();
                                session.get_pending_writes(&mut write_buf);
                                if !write_buf.is_empty() {
                                    // SAFETY: fd is an open PTY master descriptor managed by reactor
                                    let write_fd = unsafe { BorrowedFd::borrow_raw(fd) };
                                    match rustix::io::write(write_fd, &write_buf) {
                                        Ok(n) => {
                                            let is_empty = session.advance_write_queue(n);
                                            if is_empty {
                                                self.disarm_write(tab_id);
                                            }
                                        }
                                        Err(rustix::io::Errno::AGAIN) => {}
                                        Err(_) => {
                                            self.disarm_write(tab_id);
                                        }
                                    }
                                } else {
                                    self.disarm_write(tab_id);
                                }
                            }
                        }
                    }
                    EventFilter::Proc { pid, flags: _ } => {
                        let pid_raw = pid.as_raw_nonzero().get();
                        if let Some(&tab_id) = self.pid_to_tab.get(&pid_raw) {
                            if let Ok(Some(status)) = waitpid(Some(pid), WaitOptions::NOHANG) {
                                let exit_code = status.exit_status().unwrap_or(0) as i32;
                                if let Some(session) = self.sessions.get(&tab_id).cloned() {
                                    // Drain any remaining output from master
                                    let master_fd = session.master_fd();
                                    // SAFETY: master_fd is an open PTY master descriptor managed by reactor
                                    let b_master = unsafe { BorrowedFd::borrow_raw(master_fd) };
                                    while let Ok(n) = rustix::io::read(b_master, &mut read_buf) {
                                        if n == 0 {
                                            break;
                                        }
                                        let _ = session.consume_bytes(&read_buf[..n]);
                                    }
                                    session.mark_exited(exit_code);
                                }
                                self.push_event(ReactorEvent::SessionExited(tab_id, exit_code));
                            }
                        }
                    }
                    EventFilter::User { .. } => {
                        // USER event triggered: process all pending commands
                    }
                    EventFilter::Vnode { vnode, flags: _ } => {
                        let fd = vnode.as_raw_fd();
                        if let Some((token, _)) = self.vnodes.get(&fd) {
                            self.vnode_debounce.insert(*token, Instant::now() + Duration::from_millis(50));
                        }
                    }
                    _ => {}
                }
            }

            // Process all pending commands from command channel
            while let Ok(cmd) = self.cmd_rx.try_recv() {
                match cmd {
                    ReactorCmd::RegisterSession(reg) => {
                        let _ = reg.register(self);
                    }
                    ReactorCmd::ArmWrite(tab_id) => {
                        self.arm_write(tab_id);
                    }
                    ReactorCmd::WatchPath { path, token } => {
                        if let Ok(fd) = open(
                            &path,
                            OFlags::RDONLY | OFlags::CLOEXEC,
                            Mode::empty(),
                        ) {
                            let raw_fd = fd.as_raw_fd();
                            let ev = Event::new(
                                EventFilter::Vnode {
                                    vnode: raw_fd,
                                    flags: VnodeEvents::WRITE
                                        | VnodeEvents::DELETE
                                        | VnodeEvents::RENAME
                                        | VnodeEvents::EXTEND,
                                },
                                EventFlags::ADD | EventFlags::ENABLE | EventFlags::CLEAR,
                                token.0 as isize,
                            );
                            unsafe {
                                // SAFETY: Registering vnode filter on open path
                                let _ = kevent(
                                    &self.kq,
                                    &[ev],
                                    &mut Vec::new(),
                                    Some(Duration::ZERO),
                                );
                            }
                            self.vnodes.insert(raw_fd, (token, path));
                            // Store OwnedFd to keep descriptor alive
                            std::mem::forget(fd);
                        }
                    }
                    ReactorCmd::ListenUnix { path } => {
                        let _ = std::fs::remove_file(&path);
                        if let Ok(listener) = UnixListener::bind(&path) {
                            let _ = listener.set_nonblocking(true);
                            let raw_fd = listener.as_raw_fd();
                            let ev = Event::new(
                                EventFilter::Read(raw_fd),
                                EventFlags::ADD | EventFlags::ENABLE,
                                0,
                            );
                            unsafe {
                                // SAFETY: Registering IPC listener on kqueue
                                let _ = kevent(
                                    &self.kq,
                                    &[ev],
                                    &mut Vec::new(),
                                    Some(Duration::ZERO),
                                );
                            }
                            self.ipc_listeners.insert(raw_fd, listener);
                        }
                    }
                    ReactorCmd::ScheduleKillEscalation { tab_id, pid } => {
                        self.kill_escalations.insert(
                            tab_id,
                            (pid, Instant::now() + Duration::from_secs(2)),
                        );
                    }
                    ReactorCmd::Shutdown => {
                        return;
                    }
                }
            }
        }
    }
}

/// IO Reactor running on a background thread named `termin-io`.
///
/// See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §2`.
pub struct Reactor<C: PtyConsumer> {
    _phantom: std::marker::PhantomData<C>,
}

impl<C: PtyConsumer> Reactor<C> {
    /// Start the reactor IO thread with a handle to wake the main thread.
    pub fn start(wake: WakeHandle) -> io::Result<ReactorHandle<C>> {
        let kq = kqueue().map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
        let kq_raw = kq.as_raw_fd();

        // Register initial EVFILT_USER ident 0 on kqueue for waking from main
        let user_ev = Event::new(
            EventFilter::User {
                ident: 0,
                flags: UserFlags::empty(),
                user_flags: UserDefinedFlags::new(0),
            },
            EventFlags::ADD | EventFlags::ENABLE | EventFlags::CLEAR,
            0,
        );
        unsafe {
            // SAFETY: Registering initial user filter on newly created kqueue
            kevent(&kq, &[user_ev], &mut Vec::new(), Some(Duration::ZERO))
                .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
        }

        let (cmd_tx, cmd_rx) = channel();
        let events = Arc::new(Mutex::new(Vec::new()));
        let wake_pending = Arc::new(AtomicBool::new(false));

        let handle = ReactorHandle {
            cmd_tx,
            kq_raw,
            events: Arc::clone(&events),
            wake_pending: Arc::clone(&wake_pending),
            _phantom: std::marker::PhantomData,
        };

        let mut reactor_loop =
            ReactorLoop::new(kq, cmd_rx, events, wake_pending, wake);

        std::thread::Builder::new()
            .name("termin-io".to_string())
            .spawn(move || {
                reactor_loop.run();
            })?;

        Ok(handle)
    }
}
