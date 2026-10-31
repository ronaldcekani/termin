//! PTY creation and child process spawning (`termin-pty::pty`).
//!
//! See `docs/specs/pty-and-shell.md §1`.

use std::ffi::CString;
use std::io;
use std::os::fd::{AsFd, AsRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};
use std::sync::Arc;

use rustix::fs::{fcntl_getfl, fcntl_setfd, fcntl_setfl, open, FdFlags, Mode, OFlags};
use rustix::pty::{grantpt, openpt, ptsname, unlockpt, OpenptFlags};
use rustix::termios::{tcgetattr, tcsetattr, tcsetwinsize, InputModes, OptionalActions, Winsize};

use crate::{Pid, SpawnSpec};

/// Representation of an open PTY master descriptor.
pub struct PtyMaster {
    fd: Arc<OwnedFd>,
}

impl PtyMaster {
    /// Create a new `PtyMaster` from an `OwnedFd`.
    pub fn new(fd: OwnedFd) -> Self {
        Self { fd: Arc::new(fd) }
    }

    /// Access the underlying `Arc<OwnedFd>`.
    pub fn fd_arc(&self) -> Arc<OwnedFd> {
        Arc::clone(&self.fd)
    }

    /// Get raw file descriptor.
    pub fn raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// Set terminal window size.
    pub fn set_winsize(&self, cols: u16, rows: u16, px: (u16, u16)) -> io::Result<()> {
        let ws = Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: px.0,
            ws_ypixel: px.1,
        };
        tcsetwinsize(&*self.fd, ws).map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))
    }
}

impl AsFd for PtyMaster {
    fn as_fd(&self) -> std::os::fd::BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

impl AsRawFd for PtyMaster {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

/// A spawned child process connected to a PTY master.
pub struct PtyChild {
    pub master: PtyMaster,
    pub child: Child,
    pub pid: Pid,
}

extern "C" {
    fn ioctl(fd: std::os::raw::c_int, request: std::os::raw::c_ulong, ...) -> std::os::raw::c_int;
    fn setsid() -> std::os::raw::c_int;
    fn dup2(oldfd: std::os::raw::c_int, newfd: std::os::raw::c_int) -> std::os::raw::c_int;
    fn close(fd: std::os::raw::c_int) -> std::os::raw::c_int;
    #[link_name = "open"]
    fn libc_open(path: *const std::os::raw::c_char, oflag: std::os::raw::c_int, ...) -> std::os::raw::c_int;
    fn chdir(path: *const std::os::raw::c_char) -> std::os::raw::c_int;
    fn sigemptyset(set: *mut libc_sigset_t) -> std::os::raw::c_int;
    fn sigprocmask(how: std::os::raw::c_int, set: *const libc_sigset_t, oset: *mut libc_sigset_t) -> std::os::raw::c_int;
    fn signal(signum: std::os::raw::c_int, handler: usize) -> usize;
    fn tcsetpgrp(fd: std::os::raw::c_int, pgrp: std::os::raw::c_int) -> std::os::raw::c_int;
    fn tcgetpgrp(fd: std::os::raw::c_int) -> std::os::raw::c_int;
    fn getpid() -> std::os::raw::c_int;
}

#[repr(C)]
struct libc_sigset_t {
    _raw: u32,
}

const TIOCSCTTY: std::os::raw::c_ulong = 0x20007461;
const TIOCSWINSZ: std::os::raw::c_ulong = 0x80087467;
const SIG_DFL: usize = 0;
const SIG_IGN: usize = 1;
const SIG_SETMASK: std::os::raw::c_int = 3;

#[repr(C)]
struct libc_winsize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

/// Open a pseudo-terminal pair (master, slave path).
///
/// Implements `docs/specs/pty-and-shell.md §1`:
/// 1. `openpt(O_RDWR | O_NOCTTY)`, set `FD_CLOEXEC`, `grantpt`, `unlockpt`, get slave name.
/// 2. Set initial `TIOCSWINSZ` on master.
pub fn open_pty(cols: u16, rows: u16, px: (u16, u16)) -> io::Result<(PtyMaster, CString)> {
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;

    fcntl_setfd(&master, FdFlags::CLOEXEC)
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;

    grantpt(&master).map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
    unlockpt(&master).map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;

    let slave_name = ptsname(&master, Vec::new())
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;

    // Open slave temporarily to initialize BSD line discipline on macOS
    let slave = open(
        &slave_name,
        OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;

    // Set IUTF8 input mode on the slave
    if let Ok(mut termios) = tcgetattr(&slave) {
        termios.input_modes |= InputModes::IUTF8;
        let _ = tcsetattr(&slave, OptionalActions::Now, &termios);
    }

    let pty_master = PtyMaster::new(master);
    pty_master.set_winsize(cols, rows, px)?;

    // Drop temporary slave fd
    drop(slave);

    Ok((pty_master, slave_name))
}

/// Spawn a child process connected to a newly created PTY master.
///
/// Follows the complete sequence from `docs/specs/pty-and-shell.md §1`.
pub fn spawn(spec: &SpawnSpec) -> io::Result<PtyChild> {
    let (master, slave_c_name) = open_pty(spec.cols, spec.rows, spec.cell_px)?;

    let master_raw = master.raw_fd();

    // Prepare paths as CStrings for async-signal-safe calls in pre_exec
    let cwd_c = CString::new(spec.cwd.as_os_str().to_string_lossy().as_bytes())
        .unwrap_or_else(|_| CString::new("/tmp").unwrap());

    let home_c = std::env::var("HOME")
        .ok()
        .and_then(|h| CString::new(h.into_bytes()).ok())
        .unwrap_or_else(|| CString::new("/tmp").unwrap());

    let mut cmd = Command::new(&spec.prog);
    let mut args_iter = spec.args.iter();

    // If first argument is a login shell indicator (e.g. "-zsh" or "-bash"), set it as arg0
    if let Some(first) = spec.args.first() {
        if first.starts_with('-') && first.len() > 1 && !first.starts_with("--") {
            let has_rcfile = spec.args.iter().any(|a| a == "--rcfile");
            let is_bash = spec.prog.to_string_lossy().contains("bash");
            if !(is_bash && has_rcfile) {
                let prog_name = spec.prog.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if first[1..] == *prog_name || prog_name.ends_with(&first[1..]) {
                    cmd.arg0(first);
                    args_iter.next();
                }
            } else {
                args_iter.next();
            }
        }
    }

    cmd.args(args_iter);

    // Clear environment and replace with specified environment
    cmd.env_clear();
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }

    let ws = libc_winsize {
        ws_row: spec.rows,
        ws_col: spec.cols,
        ws_xpixel: spec.cell_px.0,
        ws_ypixel: spec.cell_px.1,
    };

    unsafe {
        cmd.pre_exec(move || {
            // SAFETY: In child process after fork. Only async-signal-safe libc calls without heap allocation.
            // Temporarily ignore job-control signals so tcsetpgrp does not suspend the child
            signal(22 /* SIGTTOU */, SIG_IGN);
            signal(21 /* SIGTTIN */, SIG_IGN);

            if setsid() < 0 {
                return Err(io::Error::last_os_error());
            }

            let s_fd = libc_open(slave_c_name.as_ptr(), 2 /* O_RDWR */);
            if s_fd < 0 {
                return Err(io::Error::last_os_error());
            }

            // Set initial window size on slave
            ioctl(s_fd, TIOCSWINSZ, &ws);

            // Acquire controlling terminal
            ioctl(s_fd, TIOCSCTTY, 0);
            tcsetpgrp(s_fd, getpid());

            // Duplicate slave to standard descriptors
            dup2(s_fd, 0);
            dup2(s_fd, 1);
            dup2(s_fd, 2);
            if s_fd > 2 {
                close(s_fd);
            }

            // Master has CLOEXEC, but close explicitly to be certain
            close(master_raw);

            // Reset signal mask
            let mut set: libc_sigset_t = std::mem::zeroed();
            sigemptyset(&mut set);
            sigprocmask(SIG_SETMASK, &set, std::ptr::null_mut());

            // Reset signal dispositions to default
            signal(1 /* SIGHUP */, SIG_DFL);
            signal(2 /* SIGINT */, SIG_DFL);
            signal(3 /* SIGQUIT */, SIG_DFL);
            signal(13 /* SIGPIPE */, SIG_DFL);
            signal(15 /* SIGTERM */, SIG_DFL);
            signal(20 /* SIGCHLD */, SIG_DFL);
            signal(21 /* SIGTTIN */, SIG_DFL);
            signal(22 /* SIGTTOU */, SIG_DFL);

            // Change directory with fallback to HOME
            if chdir(cwd_c.as_ptr()) < 0 {
                chdir(home_c.as_ptr());
            }

            Ok(())
        });
    }

    let child = cmd.spawn()?;
    let pid = Pid(child.id() as i32);

    // Set master fd to non-blocking mode
    let fl = fcntl_getfl(&master)
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
    fcntl_setfl(&master, fl | OFlags::NONBLOCK)
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;

    Ok(PtyChild {
        master,
        child,
        pid,
    })
}

/// Retrieve the foreground process group PID for the controlling terminal.
///
/// On macOS, `tcgetpgrp` returns -1 with `ENOTTY` if no process has acquired the controlling
/// terminal yet, or 0 if no foreground process group exists. In these cases `None` is returned.
pub fn get_foreground_pgrp(fd: RawFd) -> Option<Pid> {
    let pgrp = unsafe {
        // SAFETY: Calling tcgetpgrp on open master file descriptor
        tcgetpgrp(fd)
    };
    if pgrp > 0 {
        Some(Pid(pgrp))
    } else {
        None
    }
}
