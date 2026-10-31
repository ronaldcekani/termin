# Spec: PTY, IO Reactor, Shell Integration (`termin-pty`, `termin-shell`)

## 1. PTY spawning

Use `rustix::pty` (`openpt`, `grantpt`, `unlockpt`, `ptsname`) — no `libc` forkpty wrappers, no `portable-pty`.

Spawn sequence (`termin-pty::spawn`):

1. `openpt(O_RDWR | O_NOCTTY | O_CLOEXEC)`, `grantpt`, `unlockpt`, get slave name.
2. Set initial `TIOCSWINSZ` on the master (cols, rows, pixel w/h).
3. `fork` via `std::process::Command` with `pre_exec`:
   - `setsid()`, open slave, `TIOCSCTTY`, `dup2` slave to 0/1/2, close slave and master.
   - Reset signal mask and SIGPIPE/SIGCHLD dispositions to default.
   - `chdir(cwd)` (fallback `$HOME` on failure).
   - Use `posix_spawn` path is not possible with controlling TTY setup → fork is fine; `pre_exec` code must be async-signal-safe (no allocation).
4. Master fd: set `O_NONBLOCK`, keep `FD_CLOEXEC`.
5. Termios defaults come from the shell; we don't override except `IUTF8` set on the slave before exec.

Environment (built by `termin-shell::Env`):

| Var | Value |
|---|---|
| `TERM` | `xterm-256color` |
| `COLORTERM` | `truecolor` |
| `TERM_PROGRAM` | `termin` |
| `TERM_PROGRAM_VERSION` | crate version |
| `TERMIN_SOCKET` | IPC socket path |
| `TERMIN_WORKSPACE_ID`, `TERMIN_PANE_ID`, `TERMIN_TAB_ID` | ids (u32 decimal) |
| `TERMIN_SHELL_INTEGRATION` | `1` when integration injected |
| `LANG` | inherited; if unset, derived from `NSLocale` (e.g. `en_US.UTF-8`) |
| remove | `TERMIN_*` from parent env before setting fresh values; `TMUX`, `ZDOTDIR` handled per shell |

Shell selection order: `config.shell.program` → `$SHELL` → `getpwuid(getuid()).pw_shell` → `/bin/zsh`. Launched as a **login shell** (`argv[0] = "-zsh"`) by default, matching Terminal.app; configurable.

## 2. IO reactor

Single thread named `termin-io`. Owns a `kqueue`.

Registered sources:

| Filter | Source | Purpose |
|---|---|---|
| `EVFILT_READ` | each PTY master | output |
| `EVFILT_WRITE` | PTY master, only while its write queue is non-empty | flush input |
| `EVFILT_PROC` + `NOTE_EXIT` | each child pid | exit status (then `waitpid` to reap) |
| `EVFILT_USER` | ident 0 | wake from main thread (new writes, spawn, shutdown) |
| `EVFILT_VNODE` (`NOTE_WRITE|NOTE_DELETE|NOTE_RENAME|NOTE_EXTEND`) | config file + config dir | hot reload |
| `EVFILT_READ` | IPC listener + client sockets | CLI control |
| `EVFILT_TIMER` | one coalesced timer | sync-output timeout, read budget continuation |

Loop:

```
loop {
  n = kevent(changes, events, timeout = next_timer)
  for ev in events:
    PTY readable  → read up to 64 KiB into shared buf, repeat while budget remains (≤1 MiB / ≤4 ms per session per turn)
                    lock term; advance; unlock; mark session pending
    PTY writable  → write from queue; if empty, disable EVFILT_WRITE
    PROC exit     → waitpid; drain remaining output; session.state = Exited(code); mark pending
    USER          → process command channel (std::sync::mpsc::Receiver<ReactorCmd>)
    VNODE         → debounce 50 ms → push ReactorEvent::PathChanged
    IPC           → accept / read lines → push ReactorEvent::Ipc
  if any pending → wake main (coalesced via AtomicBool `main_wake_pending`)
}
```

Notes:
- **One read buffer** for all sessions (64 KiB). Write queues are `VecDeque<u8>` per session, shrunk to ≤ 4 KiB capacity after drain.
- Large pastes: written in chunks; if the PTY returns `EAGAIN`, keep `EVFILT_WRITE` armed.
- If a session's queue exceeds 8 MiB (runaway paste), new writes are rejected with a UI warning.
- Child exits but PTY still has data: read until `EIO`/EOF before marking exited.
- Threading primitive for main → reactor commands: `std::sync::mpsc` + `EVFILT_USER` trigger.
- Reactor → main: `ReactorEvent`s accumulate in a `Mutex<Vec<ReactorEvent>>` swapped out by main on wake (double-buffer to avoid realloc).

## 3. Session

See `architecture.md §3.3`. Additional behavior:

- `foreground_pid()`: `tcgetpgrp(master)` → process group leader. Used for tab titles ("vim", "npm"), close confirmation ("process X is running"), and sidebar metadata.
- `kill`: `SIGHUP` to the process group on tab close; after 2 s, `SIGKILL` if still alive (timer on reactor).
- Close confirmation required if foreground process ≠ shell and is not in `config.terminal.confirm_close_ignore` (default: `["zsh","bash","fish","sh","login","tmux"]`).

## 4. Shell integration

Scripts live in `resources/shell/` and are embedded into the binary with `include_str!`. On first launch (and when the embedded version hash changes) they are written to `~/Library/Application Support/Termin/shell/<version>/`. Writing on demand avoids shipping them as separate bundle files and avoids re-writing every launch.

### 4.1 Injection

**zsh** — ZDOTDIR trick (same approach as kitty/ghostty):
- Set `ZDOTDIR=<shell dir>/zsh`, and `TERMIN_ORIG_ZDOTDIR=<user's ZDOTDIR or unset marker>`.
- Our `zsh/.zshenv`: restores `ZDOTDIR` to original (or unsets), sources the user's `.zshenv`, then sources `termin-integration.zsh` **only if interactive** (`[[ -o interactive ]]`). Because ZDOTDIR is restored, the user's `.zprofile`/`.zshrc`/`.zlogin` load normally.
- Integration functions are added via `add-zsh-hook precmd/preexec` and run last (re-registered after `.zshrc` via a one-shot `precmd`).

**bash** — `--rcfile`/ENV approach:
- Interactive non-login: `bash --rcfile <shell dir>/bash/termin.bashrc`. Our rcfile sources `~/.bashrc` then integration.
- Login shell (default on macOS): bash ignores `--rcfile` for login shells. Use `bash --login` with `ENV` unset and set `TERMIN_BASH_INJECT=1`, plus `BASH_ENV` is non-interactive only → instead launch as `bash --rcfile <our> -i` with an emulation of login semantics: our rcfile first sources `/etc/profile`, then the first existing of `~/.bash_profile`, `~/.bash_login`, `~/.profile` (exactly bash's login order), then the integration. argv[0] stays `-bash` so `$0` looks like a login shell. Document the behavior in the user docs.
- Requires bash ≥ 3.2 (macOS system bash). Use only 3.2-compatible syntax (no `${var@Q}`, no associative arrays, no `PROMPT_COMMAND` arrays).

Opt-out: `config.shell.integration = false` → no injection, tab always in classic mode.

### 4.2 What the scripts emit

On every prompt:

```
precmd:
  OSC 133;D;<last exit>  ST        # skipped on very first prompt
  OSC 7;file://<host><cwd urlencoded> ST
  OSC 133;A ST                      # prepended to PS1 (zsh: %{...%}, bash: \[...\])
  ...user prompt...
  OSC 133;B ST                      # appended to PS1
preexec(cmd):
  OSC 133;C;cmdline_url=<urlencoded cmd> ST
```

Once per shell start: `OSC 6973;termin;shell=zsh;version=1;hist=<HISTFILE>` ST.

bash `preexec` implemented with a `DEBUG` trap guarded to fire once per command line (bash-preexec technique, minimal reimplementation, ~60 lines). Must not break existing `DEBUG` traps or `PROMPT_COMMAND` — chain them.

Secondary prompts (`PS2`) are wrapped with nothing; continuation lines stay part of the input region.

### 4.3 Block mode command submission

When the tab is in block mode and the shell is at a prompt (last mark is `B`):

1. User edits in the Termin input editor; keystrokes are **not** sent to the PTY.
2. On submit (Enter, no completion menu open), Termin writes:
   - zsh / bash ≥ 4.4 with bracketed paste on (`?2004` set by the line editor): `ESC[200~` + command + `ESC[201~` + `\r`.
   - bash 3.2 (no bracketed paste): command with each `\n` replaced by `\n` as-is, then `\r`. Multi-line commands execute line-by-line; documented limitation.
3. The shell echoes the command between `B` and `C`; those rows are hidden in block view (the block header shows the command text).
4. `Ctrl+C` in the editor clears the editor (never sent). `Ctrl+D` on empty editor sends `\x04` (exit shell).
5. If the shell does not reach `C` within 2 s of submit and remains at prompt (e.g. syntax error with PS2 continuation), switch the tab to **raw input** until next `A` so the user can interact with the shell's continuation prompt directly.

While a command runs (after `C`, before `D`), key input goes raw to the PTY (classic encoding). The editor is shown in a disabled "running" state, and typing into it is allowed as **type-ahead**: the text is held and becomes the editor contents when the prompt returns (Warp behavior). Configurable: `editor.type_ahead = true`.

### 4.4 Mode detection

- Tab starts in classic mode. First `133;A` → block mode becomes available; the UI switches to block mode at the next `B`.
- Alt screen (`?1049`) → classic rendering of the alt grid while active; returns to block view on exit.
- User toggle: `Action::ToggleBlockMode` per tab.
- If `133` marks stop arriving (e.g. user ran `ssh`, `docker exec`, nested shell without integration): when a `C` is followed by output containing a new prompt but no `D` for > 5 s while the foreground process is not the shell, the UI keeps the running block open and shows raw input (classic behavior inside the block). This is the natural outcome of rule 4.3 and needs no special casing beyond "running block accepts raw keys".

## 5. Testing

- Integration tests (macOS + Linux CI) spawning `/bin/sh -c 'printf ...'` and asserting output arrives, exit codes, resize ioctl effect (`stty size`).
- Shell script tests: run zsh and bash (3.2 and 5.x if available) under a PTY with integration injected, run `echo hi; false`, assert mark sequence `A B C D;0 A B C D;1` and OSC 7.
- Verify user `.zshrc` with `ZDOTDIR` customizations still loads (fixture home dir).
- Stress: `yes | head -c 500M` throughput through reactor + parser; main thread frame times stay < 8 ms.
