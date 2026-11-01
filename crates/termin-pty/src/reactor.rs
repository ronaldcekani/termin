//! Single-threaded IO Reactor owning a kqueue (`termin-pty::reactor`).
//!
//! See `docs/architecture.md §3.3` and `docs/specs/pty-and-shell.md §2`.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
#[cfg(not(target_os = "macos"))]
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
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

#[cfg(not(target_os = "macos"))]
#[derive(Debug)]
struct BoundIpcListener {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
    startup_lock: std::fs::File,
}

#[cfg(not(target_os = "macos"))]
impl BoundIpcListener {
    fn cleanup(self) {
        let Self {
            listener,
            path,
            device,
            inode,
            startup_lock,
        } = self;
        drop(listener);
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_socket() && meta.dev() == device && meta.ino() == inode {
                let _ = std::fs::remove_file(path);
            }
        }
        drop(startup_lock);
    }
}

/// Bind under a per-directory startup lock, preserving non-socket collisions.
#[cfg(not(target_os = "macos"))]
fn bind_ipc_listener(path: &Path) -> io::Result<BoundIpcListener> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.exists() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let parent_meta = std::fs::symlink_metadata(parent)?;
    let parent_mode = parent_meta.permissions().mode();
    if !parent_meta.file_type().is_dir()
        || parent_meta.uid() != rustix::process::geteuid().as_raw() as u32
        || parent_mode & 0o077 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC socket parent must be an owned, private directory",
        ));
    }

    // All Termin instances serialize startup decisions for this socket path. The
    // lock lives beside the socket, inside the already validated private directory.
    let mut lock_name = std::ffi::OsString::from(".");
    lock_name.push(
        path.file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("termin.sock")),
    );
    lock_name.push(".termin-ipc.lock");
    let lock_path = parent.join(lock_name);
    let lock_fd = open(
        &lock_path,
        OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::from_bits_truncate(0o600),
    )?;
    let lock_file = std::fs::File::from(lock_fd);
    let lock_meta = lock_file.metadata()?;
    if !lock_meta.file_type().is_file()
        || lock_meta.uid() != rustix::process::geteuid().as_raw() as u32
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC startup lock is not an owned regular file",
        ));
    }
    rustix::fs::fchmod(&lock_file, Mode::from_bits_truncate(0o600))
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
    if let Err(error) = rustix::fs::flock(
        &lock_file,
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
    ) {
        if error == rustix::io::Errno::AGAIN {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "IPC socket is owned by another Termin instance",
            ));
        }
        return Err(io::Error::from_raw_os_error(error.raw_os_error()));
    }

    match UnixListener::bind(path) {
        Ok(listener) => {
            if let Err(error) =
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            {
                drop(listener);
                return Err(error);
            }
            let metadata = std::fs::symlink_metadata(path)?;
            if !metadata.file_type().is_socket() {
                drop(listener);
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "bound IPC path is not a socket",
                ));
            }
            Ok(BoundIpcListener {
                listener,
                path: path.to_path_buf(),
                device: metadata.dev(),
                inode: metadata.ino(),
                startup_lock: lock_file,
            })
        }
        Err(bind_error) => {
            let metadata = match std::fs::symlink_metadata(path) {
                Ok(metadata) => metadata,
                Err(_) => return Err(bind_error),
            };
            if !metadata.file_type().is_socket() {
                return Err(bind_error);
            }
            match UnixStream::connect(path) {
                Ok(_) => Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "IPC socket is active",
                )),
                // The startup lock serializes all Termin bind/register attempts.
                Err(error) if error.kind() == io::ErrorKind::ConnectionRefused => {
                    let current = std::fs::symlink_metadata(path)?;
                    if !current.file_type().is_socket()
                        || current.dev() != metadata.dev()
                        || current.ino() != metadata.ino()
                    {
                        return Err(io::Error::new(
                            io::ErrorKind::AlreadyExists,
                            "IPC socket changed during stale check",
                        ));
                    }
                    std::fs::remove_file(path)?;
                    let listener = UnixListener::bind(path)?;
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
                    let metadata = std::fs::symlink_metadata(path)?;
                    Ok(BoundIpcListener {
                        listener,
                        path: path.to_path_buf(),
                        device: metadata.dev(),
                        inode: metadata.ino(),
                        startup_lock: lock_file,
                    })
                }
                Err(_) => Err(bind_error),
            }
        }
    }
}

fn same_uid_peer(stream: &UnixStream) -> io::Result<bool> {
    #[cfg(target_os = "linux")]
    {
        let cred = rustix::net::sockopt::get_socket_peercred(stream)
            .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
        Ok(cred.uid == rustix::process::geteuid())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = stream;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "safe Unix peer credential API is unavailable on this platform",
        ))
    }
}

/// Commands sent to the reactor IO thread.
pub(crate) enum ReactorCmd {
    RegisterSession(Box<dyn ErasedSessionRegister>),
    ArmWrite(TabId),
    WatchPath {
        path: PathBuf,
        token: WatchToken,
    },
    #[cfg(not(target_os = "macos"))]
    ListenUnix {
        path: PathBuf,
        result: Sender<io::Result<()>>,
    },
    #[allow(dead_code)]
    ScheduleKillEscalation {
        tab_id: TabId,
        pid: Pid,
    },
    UnregisterSession {
        tab_id: TabId,
        result: Option<Sender<io::Result<()>>>,
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
        self.listen_unix_checked(path)
    }

    /// Start listening and return the actual bind/registration result.
    pub fn listen_unix_checked(&self, path: &Path) -> io::Result<()> {
        #[cfg(target_os = "macos")]
        {
            let _ = path;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "safe same-uid peer credentials are unavailable in the allowlisted rustix API",
            ))
        }
        #[cfg(not(target_os = "macos"))]
        {
            let (result_tx, result_rx) = channel();
            self.cmd_tx
                .send(ReactorCmd::ListenUnix {
                    path: path.to_path_buf(),
                    result: result_tx,
                })
                .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?;
            self.trigger_user_event();
            result_rx
                .recv()
                .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?
        }
    }

    /// Remove an exited session from the reactor after its `SessionExited` event.
    /// The call waits for reactor cleanup to finish; the caller should then drop its `Arc<Session>`.
    pub fn unregister_session(&self, tab_id: TabId) -> io::Result<()> {
        let (result_tx, result_rx) = channel();
        self.cmd_tx
            .send(ReactorCmd::UnregisterSession {
                tab_id,
                result: Some(result_tx),
            })
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?;
        self.trigger_user_event();
        result_rx
            .recv()
            .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, e.to_string()))?
    }

    /// Enqueue session cleanup without waiting on the IO thread. Call after `SessionExited`.
    pub fn request_unregister_session(&self, tab_id: TabId) -> io::Result<()> {
        self.cmd_tx
            .send(ReactorCmd::UnregisterSession {
                tab_id,
                result: None,
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
            // Reset while holding the queue lock. Otherwise a producer can
            // enqueue an event after the drain but before the reset, observe
            // the old pending flag, and leave the new event without a wake.
            self.wake_pending.store(false, Ordering::SeqCst);
        }
    }
}

/// Type-erased session wrapper for reactor state management.
trait ActiveSession: Send + Sync {
    fn master_fd(&self) -> RawFd;
    fn consume_bytes(&self, bytes: &[u8]) -> (usize, Vec<u8>);
    fn get_pending_writes(&self, out: &mut Vec<u8>) -> usize;
    fn advance_write_queue(&self, written: usize) -> bool;
    fn mark_exited(&self, code: i32);
    fn is_exited(&self) -> bool;
    fn pid(&self) -> i32;
    fn clear_pending_writes(&self);
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

    fn is_exited(&self) -> bool {
        self.state
            .lock()
            .map(|state| matches!(*state, SessionState::Exited(_)))
            .unwrap_or(false)
    }

    fn pid(&self) -> i32 {
        self.pid.0
    }

    fn clear_pending_writes(&self) {
        if let Ok(mut queue) = self.write_queue.lock() {
            queue.clear();
            if queue.capacity() > DRAINED_WRITE_QUEUE_CAPACITY {
                queue.shrink_to(DRAINED_WRITE_QUEUE_CAPACITY);
            }
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
    #[cfg(not(target_os = "macos"))]
    ipc_startup_locks: Vec<std::fs::File>,
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
            #[cfg(not(target_os = "macos"))]
            ipc_startup_locks: Vec::new(),
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
                    let _ = kevent(&self.kq, &[write_ev], &mut Vec::new(), Some(Duration::ZERO));
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
                    let _ = kevent(&self.kq, &[write_ev], &mut Vec::new(), Some(Duration::ZERO));
                }
                self.write_armed.insert(tab_id, false);
            }
        }
    }

    fn unregister_session(&mut self, tab_id: TabId) -> io::Result<()> {
        let Some(session) = self.sessions.get(&tab_id).cloned() else {
            return Ok(());
        };
        if !session.is_exited() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "session is still running",
            ));
        }
        let fd = session.master_fd();
        let pid = session.pid();
        let mut filters = vec![EventFilter::Read(fd), EventFilter::Write(fd)];
        if let Some(process_id) = rustix::process::Pid::from_raw(pid) {
            filters.push(EventFilter::Proc {
                pid: process_id,
                flags: ProcessEvents::EXIT,
            });
        }
        for filter in filters {
            let event = Event::new(filter, EventFlags::DELETE, 0);
            // SAFETY: Removing kqueue filters for this session's still-owned descriptors/PID.
            if let Err(error) =
                unsafe { kevent(&self.kq, &[event], &mut Vec::new(), Some(Duration::ZERO)) }
            {
                // Read/write filters may already have been deleted at EOF, and the
                // process filter is automatically removed after NOTE_EXIT.
                log::debug!("session {} filter cleanup: {error}", tab_id.0);
            }
        }
        self.sessions.remove(&tab_id);
        self.fd_to_tab.remove(&fd);
        self.pid_to_tab.remove(&pid);
        self.write_armed.remove(&tab_id);
        session.clear_pending_writes();
        Ok(())
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
                        let _ =
                            rustix::process::kill_process_group(p, rustix::process::Signal::Kill);
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
                                            let (_, write_back) =
                                                session.consume_bytes(&read_buf[..bytes_read]);
                                            if !write_back.is_empty() {
                                                // Feed write-back into session write queue
                                                // SAFETY: fd is an open PTY master descriptor managed by reactor
                                                let write_fd =
                                                    unsafe { BorrowedFd::borrow_raw(fd) };
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
                            match listener.accept() {
                                Ok((stream, _)) => {
                                    match same_uid_peer(&stream) {
                                        Ok(true) => {}
                                        Ok(false) => continue,
                                        Err(error) => {
                                            log::warn!(
                                                "could not verify IPC peer credentials: {error}"
                                            );
                                            continue;
                                        }
                                    }
                                    if let Err(error) = stream.set_nonblocking(true) {
                                        log::warn!(
                                            "could not make IPC client nonblocking: {error}"
                                        );
                                        continue;
                                    }
                                    let client_fd = stream.as_raw_fd();
                                    let conn_id = IpcConnId(self.next_ipc_id);
                                    self.next_ipc_id += 1;

                                    let ev = Event::new(
                                        EventFilter::Read(client_fd),
                                        EventFlags::ADD | EventFlags::ENABLE,
                                        conn_id.0 as isize,
                                    );
                                    let registration = unsafe {
                                        // SAFETY: Registering new IPC client socket on kqueue
                                        kevent(
                                            &self.kq,
                                            &[ev],
                                            &mut Vec::new(),
                                            Some(Duration::ZERO),
                                        )
                                    };
                                    if let Err(error) = registration {
                                        log::warn!(
                                            "could not register IPC client with kqueue: {error}"
                                        );
                                        continue;
                                    }
                                    self.ipc_clients.insert(client_fd, (conn_id, stream));
                                }
                                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                                Err(error) => log::warn!("IPC accept failed: {error}"),
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
                                self.pid_to_tab.remove(&pid_raw);
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
                            self.vnode_debounce
                                .insert(*token, Instant::now() + Duration::from_millis(50));
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
                        if let Ok(fd) = open(&path, OFlags::RDONLY | OFlags::CLOEXEC, Mode::empty())
                        {
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
                                let _ =
                                    kevent(&self.kq, &[ev], &mut Vec::new(), Some(Duration::ZERO));
                            }
                            self.vnodes.insert(raw_fd, (token, path));
                            // Store OwnedFd to keep descriptor alive
                            std::mem::forget(fd);
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    ReactorCmd::ListenUnix { path, result } => {
                        let bind_result = bind_ipc_listener(&path).and_then(|binding| {
                            if let Err(error) = binding.listener.set_nonblocking(true) {
                                binding.cleanup();
                                return Err(error);
                            }
                            let listener = binding.listener.as_raw_fd();
                            let ev = Event::new(
                                EventFilter::Read(listener),
                                EventFlags::ADD | EventFlags::ENABLE,
                                0,
                            );
                            let registration = unsafe {
                                // SAFETY: Registering IPC listener on kqueue
                                kevent(&self.kq, &[ev], &mut Vec::new(), Some(Duration::ZERO))
                                    .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))
                            };
                            if let Err(error) = registration {
                                binding.cleanup();
                                return Err(error);
                            }
                            let BoundIpcListener {
                                listener,
                                startup_lock,
                                ..
                            } = binding;
                            self.ipc_listeners.insert(listener.as_raw_fd(), listener);
                            // Keep the lock for the listener lifetime. A competing
                            // starter can then distinguish our active socket from a stale one
                            // without relying on a potentially-backlogged connect probe.
                            self.ipc_startup_locks.push(startup_lock);
                            Ok(())
                        });
                        let _ = result.send(bind_result);
                    }
                    ReactorCmd::ScheduleKillEscalation { tab_id, pid } => {
                        self.kill_escalations
                            .insert(tab_id, (pid, Instant::now() + Duration::from_secs(2)));
                    }
                    ReactorCmd::UnregisterSession { tab_id, result } => {
                        let cleanup = self.unregister_session(tab_id);
                        if let Some(result) = result {
                            let _ = result.send(cleanup);
                        } else if let Err(error) = cleanup {
                            log::warn!("could not unregister session {}: {error}", tab_id.0);
                        }
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

        let mut reactor_loop = ReactorLoop::new(kq, cmd_rx, events, wake_pending, wake);

        std::thread::Builder::new()
            .name("termin-io".to_string())
            .spawn(move || {
                reactor_loop.run();
            })?;

        Ok(handle)
    }
}

#[cfg(all(test, not(target_os = "macos")))]
mod ipc_listener_tests {
    use super::*;

    fn path(label: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "termin-{label}-{}-{}.sock",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        (dir.join("termin.sock"), dir)
    }

    #[test]
    fn refuses_to_remove_non_socket_collision() {
        let (socket_path, dir) = path("collision");
        std::fs::write(&socket_path, b"owned by someone else").unwrap();
        let error = bind_ipc_listener(&socket_path).unwrap_err();
        assert!(
            error.kind() == io::ErrorKind::AddrInUse
                || error.kind() == io::ErrorKind::AlreadyExists
        );
        assert_eq!(
            std::fs::read(&socket_path).unwrap(),
            b"owned by someone else"
        );
        std::fs::remove_file(socket_path).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn active_socket_is_preserved_and_new_socket_is_private() {
        let (socket_path, dir) = path("active");
        let active = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                let _ = std::fs::remove_dir_all(dir);
                return;
            }
            Err(error) => panic!("could not create test socket: {error}"),
        };
        assert!(bind_ipc_listener(&socket_path).is_err());
        assert!(UnixStream::connect(&socket_path).is_ok());
        drop(active);
        std::fs::remove_file(&socket_path).unwrap();

        let listener = match bind_ipc_listener(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                let _ = std::fs::remove_dir_all(dir);
                return;
            }
            Err(error) => panic!("could not create test socket: {error}"),
        };
        assert_eq!(
            std::fs::metadata(&socket_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        drop(listener);
        std::fs::remove_file(socket_path).unwrap();
        std::fs::remove_file(dir.join(".termin.sock.termin-ipc.lock")).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reclaims_stale_socket_and_serializes_concurrent_startup() {
        let (socket_path, dir) = path("stale");
        let listener = match UnixListener::bind(&socket_path) {
            Ok(listener) => listener,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                let _ = std::fs::remove_dir_all(dir);
                return;
            }
            Err(error) => panic!("could not create test socket: {error}"),
        };
        drop(listener);
        let binding = bind_ipc_listener(&socket_path).unwrap();
        assert_eq!(
            std::fs::metadata(&socket_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert!(UnixStream::connect(&socket_path).is_ok());
        drop(binding);
        let _ = std::fs::remove_file(&socket_path);

        let (socket_path, dir2) = path("concurrent");
        let socket_path = Arc::new(socket_path);
        drop(UnixListener::bind(&*socket_path).unwrap());
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut workers = Vec::new();
        for _ in 0..2 {
            let p = Arc::clone(&socket_path);
            let b = Arc::clone(&barrier);
            workers.push(std::thread::spawn(move || {
                b.wait();
                bind_ipc_listener(&p)
            }));
        }
        barrier.wait();
        let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
        assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
        assert_eq!(outcomes.iter().filter(|r| r.is_err()).count(), 1);
        drop(outcomes);
        let _ = std::fs::remove_file(&*socket_path);
        std::fs::remove_file(dir.join(".termin.sock.termin-ipc.lock")).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_file(dir2.join(".termin.sock.termin-ipc.lock")).unwrap();
        std::fs::remove_dir_all(dir2).unwrap();
    }

    #[test]
    fn rejects_symlink_or_non_private_parent() {
        let (_socket_path, dir) = path("parent");
        let public_dir = std::env::temp_dir().join(format!("termin-public-{}", std::process::id()));
        std::fs::create_dir_all(&public_dir).unwrap();
        std::fs::set_permissions(&public_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            bind_ipc_listener(&public_dir.join("termin.sock"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        let link = dir.join("link");
        std::os::unix::fs::symlink(&public_dir, &link).unwrap();
        assert_eq!(
            bind_ipc_listener(&link.join("termin.sock"))
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        std::fs::remove_file(link).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
        std::fs::remove_dir_all(public_dir).unwrap();
    }
}
