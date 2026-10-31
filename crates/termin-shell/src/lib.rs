//! Shell integration scripts and environment/argv builder (`termin-shell`).
//!
//! See `docs/specs/pty-and-shell.md §1, §4`.

use std::path::{Path, PathBuf};
use termin_core::{PaneId, TabId, WorkspaceId};

/// Embedded zsh integration script.
///
/// See `docs/specs/pty-and-shell.md §4`.
pub const SHELL_INTEGRATION_ZSH: &str =
    include_str!("../../../resources/shell/termin-integration.zsh");

/// Embedded bash integration script.
///
/// See `docs/specs/pty-and-shell.md §4`.
pub const SHELL_INTEGRATION_BASH: &str =
    include_str!("../../../resources/shell/termin-integration.bash");

/// Identifiers identifying the session context.
///
/// See `docs/specs/pty-and-shell.md §1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SessionIds {
    pub workspace: WorkspaceId,
    pub pane: PaneId,
    pub tab: TabId,
}

/// Environment variable builder for spawned shells.
///
/// See `docs/specs/pty-and-shell.md §1`.
pub struct Env;

impl Env {
    /// Build the environment vector for a child session.
    pub fn build(
        ids: &SessionIds,
        socket_path: &Path,
        custom: &[(String, String)],
        integration: bool,
    ) -> Vec<(String, String)> {
        let mut env = vec![
            ("TERM".to_string(), "xterm-256color".to_string()),
            ("COLORTERM".to_string(), "truecolor".to_string()),
            ("TERM_PROGRAM".to_string(), "termin".to_string()),
            (
                "TERM_PROGRAM_VERSION".to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
            ),
            (
                "TERMIN_SOCKET".to_string(),
                socket_path.to_string_lossy().to_string(),
            ),
            (
                "TERMIN_WORKSPACE_ID".to_string(),
                ids.workspace.0.to_string(),
            ),
            ("TERMIN_PANE_ID".to_string(), ids.pane.0.to_string()),
            ("TERMIN_TAB_ID".to_string(), ids.tab.0.to_string()),
        ];

        if integration {
            env.push(("TERMIN_SHELL_INTEGRATION".to_string(), "1".to_string()));
        }

        for (k, v) in custom {
            env.push((k.clone(), v.clone()));
        }

        env
    }
}

/// Resolve user preferred or default system shell.
///
/// Selection order: `preferred` -> `$SHELL` -> `/bin/zsh`.
/// See `docs/specs/pty-and-shell.md §1`.
pub fn resolve_shell(preferred: Option<&str>) -> (PathBuf, Vec<String>) {
    if let Some(p) = preferred {
        if !p.is_empty() {
            return (PathBuf::from(p), vec![format!("-{}", p)]);
        }
    }

    if let Ok(sh) = std::env::var("SHELL") {
        if !sh.is_empty() {
            let prog = PathBuf::from(&sh);
            let name = prog
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("zsh")
                .to_string();
            return (prog, vec![format!("-{}", name)]);
        }
    }

    (PathBuf::from("/bin/zsh"), vec!["-zsh".to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_scripts_present() {
        assert!(!SHELL_INTEGRATION_ZSH.is_empty());
        assert!(!SHELL_INTEGRATION_BASH.is_empty());
    }

    #[test]
    fn test_env_building() {
        let ids = SessionIds {
            workspace: WorkspaceId(1),
            pane: PaneId(2),
            tab: TabId(3),
        };
        let env = Env::build(&ids, Path::new("/tmp/termin.sock"), &[], true);
        assert!(env
            .iter()
            .any(|(k, v)| k == "TERM" && v == "xterm-256color"));
        assert!(env.iter().any(|(k, v)| k == "TERMIN_TAB_ID" && v == "3"));
        assert!(env
            .iter()
            .any(|(k, v)| k == "TERMIN_SHELL_INTEGRATION" && v == "1"));
    }
}
