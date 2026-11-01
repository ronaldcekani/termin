//! Session lifecycle and terminal effect draining (`termin-app::session_ctl`).
//!
//! See `docs/architecture.md §4` and `docs/specs/pty-and-shell.md`.

use std::path::PathBuf;
use std::sync::Arc;
use termin_config::Config;
use termin_core::{PaneId, WorkspaceId};
use termin_pty::{ReactorHandle, Session, SessionState, Signal, SpawnSpec};
use termin_shell::{prepare_injection, resolve_shell, Env, SessionIds};
use termin_vt::{BlockEvent, Effect, Osc52Policy, Terminal, TerminalConfig};

use crate::model::types::{AttentionKind, Tab};

/// Spawn a new terminal session for `tab_id` and attach it to the tab.
pub fn spawn_session(
    tab: &mut Tab,
    pane_id: PaneId,
    workspace_id: WorkspaceId,
    reactor: &ReactorHandle<Terminal>,
    config: &Config,
    cols: u16,
    rows: u16,
) -> Result<Arc<Session<Terminal>>, std::io::Error> {
    let pref_shell = if config.shell.program.is_empty() {
        None
    } else {
        Some(config.shell.program.as_str())
    };
    let (prog, mut args) = resolve_shell(pref_shell);

    let cwd = tab.cwd.as_ref().map(PathBuf::from).unwrap_or_else(|| {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"))
    });

    let ids = SessionIds {
        workspace: workspace_id,
        pane: pane_id,
        tab: tab.id,
    };

    let socket_path = PathBuf::from("/tmp/termin.sock");
    let mut env = Env::build(&ids, &socket_path, &[], config.shell.integration);

    if config.shell.integration {
        if let Ok(shell_dir) = termin_shell::deploy_shell_scripts(None) {
            prepare_injection(&prog, config.shell.login, &shell_dir, &mut env, &mut args);
        }
    }

    let spec = SpawnSpec {
        prog,
        args,
        env,
        cwd,
        cols,
        rows,
        cell_px: (8, 16),
    };

    let term_cfg = TerminalConfig {
        cols,
        rows,
        scrollback_lines: config.terminal.scrollback_lines,
        cell_px: (8, 16),
        osc52: Osc52Policy::Write,
    };
    let term = Terminal::new(&term_cfg);

    let session = reactor.spawn(spec, term)?;
    tab.session = Some(Arc::clone(&session));

    Ok(session)
}

/// Drain accumulated effects from `session` into `tab`, handing OSC 133 block
/// events to `on_block`.
///
/// Returns `true` if any drained effect should cause a UI or model dirty state.
pub fn drain_session_effects(
    tab: &mut Tab,
    session: &Session<Terminal>,
    mut on_block: impl FnMut(BlockEvent),
) -> bool {
    let mut effects = Vec::new();
    session.drain_effects(&mut effects);

    if effects.is_empty() {
        return false;
    }

    for effect in effects {
        match effect {
            Effect::Title(t) => {
                tab.title.osc = Some(t.to_string());
            }
            Effect::Cwd(p) => {
                tab.cwd = Some(p.to_string());
            }
            Effect::Bell => {
                tab.attention.unread = true;
                tab.attention.kind = AttentionKind::Bell;
                tab.attention.at = Some(std::time::Instant::now());
            }
            Effect::Notify { title: _, body } => {
                tab.attention.unread = true;
                tab.attention.kind = AttentionKind::Notify;
                tab.attention.message = Some(body);
                tab.attention.at = Some(std::time::Instant::now());
            }
            Effect::Block(ev) => on_block(ev),
            _ => {}
        }
    }

    true
}

/// Ask a still-running child process group to exit on tab close.
///
/// `Session::kill(SIGHUP)` also schedules the reactor's two-second SIGKILL
/// escalation. The app retains the session handle until `SessionExited` arrives.
pub fn close_session(session: &Session<Terminal>) -> bool {
    if session.state() != SessionState::Running {
        return false;
    }
    session.kill(Signal(1));
    true
}
