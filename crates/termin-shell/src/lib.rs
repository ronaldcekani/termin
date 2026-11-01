//! Shell integration scripts and environment/argv builder (`termin-shell`).
//!
//! See `docs/specs/pty-and-shell.md §1, §4`.

use std::collections::hash_map::DefaultHasher;
use std::ffi::CStr;
use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};
use termin_core::{PaneId, TabId, WorkspaceId};

/// Embedded zsh integration script.
pub const SHELL_INTEGRATION_ZSH: &str =
    include_str!("../../../resources/shell/termin-integration.zsh");

/// Embedded bash integration script.
pub const SHELL_INTEGRATION_BASH: &str =
    include_str!("../../../resources/shell/termin-integration.bash");

/// Wrapper for zsh .zshenv to implement the ZDOTDIR injection technique.
///
/// See `docs/specs/pty-and-shell.md §4.1`.
pub const ZSH_ENV_WRAPPER: &str = r#"# Termin ZDOTDIR wrapper for zsh
if [[ -n "$TERMIN_ORIG_ZDOTDIR" ]]; then
  export ZDOTDIR="$TERMIN_ORIG_ZDOTDIR"
  unset TERMIN_ORIG_ZDOTDIR
else
  unset ZDOTDIR
fi

if [[ -f "${ZDOTDIR:-$HOME}/.zshenv" ]]; then
  source "${ZDOTDIR:-$HOME}/.zshenv" || true
fi

if [[ -o interactive ]] && [[ -z "$TERMIN_SHELL_INTEGRATION_LOADED" ]]; then
  _dir="@TERMIN_DIR@"
  [[ -z "$_dir" ]] && _dir="$TERMIN_SHELL_DIR"
  if [[ -f "$_dir/termin-integration.zsh" ]]; then
    source "$_dir/termin-integration.zsh"
  fi
  unset _dir
fi
"#;

/// Wrapper for bash --rcfile to implement login and interactive injection.
///
/// See `docs/specs/pty-and-shell.md §4.1`.
pub const BASH_RC_WRAPPER: &str = r#"# Termin rcfile wrapper for bash
if [ -n "$TERMIN_LOGIN_EMULATION" ]; then
  unset TERMIN_LOGIN_EMULATION
  [ -r /etc/profile ] && . /etc/profile || true
  if [ -r ~/.bash_profile ]; then
    . ~/.bash_profile || true
  elif [ -r ~/.bash_login ]; then
    . ~/.bash_login || true
  elif [ -r ~/.profile ]; then
    . ~/.profile || true
  fi
else
  [ -r ~/.bashrc ] && . ~/.bashrc || true
fi

if [ -z "$TERMIN_SHELL_INTEGRATION_LOADED" ]; then
  _dir="@TERMIN_DIR@"
  [ -z "$_dir" ] && _dir="$TERMIN_SHELL_DIR"
  if [ -f "$_dir/termin-integration.bash" ]; then
    . "$_dir/termin-integration.bash"
  fi
  unset _dir
fi
"#;

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
        // Inherit parent environment except remove any existing TERMIN_* variables
        let mut env: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| !k.starts_with("TERMIN_"))
            .collect();

        // Ensure LANG is set
        if !env.iter().any(|(k, v)| k == "LANG" && !v.is_empty()) {
            env.push(("LANG".to_string(), "en_US.UTF-8".to_string()));
        }

        // Set standard terminal variables
        Self::set_or_replace(&mut env, "TERM", "xterm-256color");
        Self::set_or_replace(&mut env, "COLORTERM", "truecolor");
        Self::set_or_replace(&mut env, "TERM_PROGRAM", "termin");
        Self::set_or_replace(&mut env, "TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        Self::set_or_replace(&mut env, "TERMIN_SOCKET", &socket_path.to_string_lossy());
        Self::set_or_replace(
            &mut env,
            "TERMIN_WORKSPACE_ID",
            &ids.workspace.0.to_string(),
        );
        Self::set_or_replace(&mut env, "TERMIN_PANE_ID", &ids.pane.0.to_string());
        Self::set_or_replace(&mut env, "TERMIN_TAB_ID", &ids.tab.0.to_string());

        if integration {
            Self::set_or_replace(&mut env, "TERMIN_SHELL_INTEGRATION", "1");
        }

        for (k, v) in custom {
            Self::set_or_replace(&mut env, k, v);
        }

        env
    }

    fn set_or_replace(env: &mut Vec<(String, String)>, key: &str, val: &str) {
        if let Some(pos) = env.iter().position(|(k, _)| k == key) {
            env[pos].1 = val.to_string();
        } else {
            env.push((key.to_string(), val.to_string()));
        }
    }
}

/// Compute a 16-hex-digit version hash for the embedded shell scripts.
pub fn script_version_hash() -> String {
    let mut hasher = DefaultHasher::new();
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    SHELL_INTEGRATION_ZSH.hash(&mut hasher);
    SHELL_INTEGRATION_BASH.hash(&mut hasher);
    ZSH_ENV_WRAPPER.hash(&mut hasher);
    BASH_RC_WRAPPER.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Deploy embedded shell integration scripts to disk on demand.
///
/// Targets `~/Library/Application Support/Termin/shell/<version_hash>/` by default,
/// or `base_dir/shell/<version_hash>/` if `base_dir` is supplied (useful for tests).
///
/// See `docs/specs/pty-and-shell.md §4`.
pub fn deploy_shell_scripts(base_dir: Option<&Path>) -> io::Result<PathBuf> {
    let target_dir = if let Some(base) = base_dir {
        base.join("shell").join(script_version_hash())
    } else {
        let home = std::env::var("HOME").map_err(|e| io::Error::new(io::ErrorKind::NotFound, e))?;
        PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("Termin")
            .join("shell")
            .join(script_version_hash())
    };

    let zsh_dir = target_dir.join("zsh");
    let bash_dir = target_dir.join("bash");

    let marker = target_dir.join(".ready");
    if marker.exists() {
        return Ok(target_dir);
    }

    std::fs::create_dir_all(&zsh_dir)?;
    std::fs::create_dir_all(&bash_dir)?;

    std::fs::write(
        target_dir.join("termin-integration.zsh"),
        SHELL_INTEGRATION_ZSH,
    )?;
    std::fs::write(
        target_dir.join("termin-integration.bash"),
        SHELL_INTEGRATION_BASH,
    )?;

    let target_str = target_dir.to_string_lossy();
    let zsh_wrapper = ZSH_ENV_WRAPPER.replace("@TERMIN_DIR@", &target_str);
    let bash_wrapper = BASH_RC_WRAPPER.replace("@TERMIN_DIR@", &target_str);

    std::fs::write(zsh_dir.join(".zshenv"), zsh_wrapper)?;
    std::fs::write(bash_dir.join("termin.bashrc"), bash_wrapper)?;

    std::fs::write(marker, b"ok")?;

    Ok(target_dir)
}

/// Configure environment and command-line arguments to inject shell integration.
///
/// See `docs/specs/pty-and-shell.md §4.1`.
pub fn prepare_injection(
    shell_path: &Path,
    is_login: bool,
    integration_dir: &Path,
    env: &mut Vec<(String, String)>,
    args: &mut Vec<String>,
) {
    let file_name = shell_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");

    env.push((
        "TERMIN_SHELL_DIR".to_string(),
        integration_dir.to_string_lossy().to_string(),
    ));

    if file_name.contains("zsh") {
        // ZDOTDIR trick
        let orig_zdotdir = std::env::var("ZDOTDIR").ok();
        if let Some(orig) = orig_zdotdir {
            if !orig.is_empty() {
                env.push(("TERMIN_ORIG_ZDOTDIR".to_string(), orig));
            }
        }
        let zsh_dir = integration_dir.join("zsh");
        env.push(("ZDOTDIR".to_string(), zsh_dir.to_string_lossy().to_string()));
    } else if file_name.contains("bash") {
        let rcfile = integration_dir.join("bash").join("termin.bashrc");
        if is_login {
            // Emulate login semantics via our rcfile and -i
            env.push(("TERMIN_LOGIN_EMULATION".to_string(), "1".to_string()));
            args.push("--rcfile".to_string());
            args.push(rcfile.to_string_lossy().to_string());
            args.push("-i".to_string());
        } else {
            args.push("--rcfile".to_string());
            args.push(rcfile.to_string_lossy().to_string());
        }
    }
}

/// Resolve user preferred or default system shell.
///
/// Selection order: `preferred` -> `$SHELL` -> `getpwuid` -> `/bin/zsh`.
/// Default is a login shell (`argv[0] = "-<name>"`).
///
/// See `docs/specs/pty-and-shell.md §1`.
pub fn resolve_shell(preferred: Option<&str>) -> (PathBuf, Vec<String>) {
    // 1. config.shell.program
    if let Some(p) = preferred {
        if !p.is_empty() {
            let prog = PathBuf::from(p);
            let name = prog
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("zsh")
                .to_string();
            return (prog, vec![format!("-{}", name)]);
        }
    }

    // 2. $SHELL
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

    // 3. getpwuid(getuid()).pw_shell
    if let Some(sh) = get_user_shell() {
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

    // 4. Default fallback /bin/zsh
    (PathBuf::from("/bin/zsh"), vec!["-zsh".to_string()])
}

fn get_user_shell() -> Option<String> {
    extern "C" {
        fn getuid() -> u32;
        fn getpwuid(uid: u32) -> *const Passwd;
    }

    #[repr(C)]
    struct Passwd {
        pw_name: *const std::os::raw::c_char,
        pw_passwd: *const std::os::raw::c_char,
        pw_uid: u32,
        pw_gid: u32,
        pw_change: std::os::raw::c_long,
        pw_class: *const std::os::raw::c_char,
        pw_gecos: *const std::os::raw::c_char,
        pw_dir: *const std::os::raw::c_char,
        pw_shell: *const std::os::raw::c_char,
        pw_expire: std::os::raw::c_long,
    }

    unsafe {
        let uid = getuid();
        let pwd = getpwuid(uid);
        if pwd.is_null() || (*pwd).pw_shell.is_null() {
            None
        } else {
            let c_str = CStr::from_ptr((*pwd).pw_shell);
            c_str.to_str().ok().map(|s| s.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_scripts_present() {
        assert!(!SHELL_INTEGRATION_ZSH.is_empty());
        assert!(!SHELL_INTEGRATION_BASH.is_empty());
        assert!(SHELL_INTEGRATION_ZSH.contains("133;A"));
        assert!(SHELL_INTEGRATION_BASH.contains("133;A"));
        assert!(SHELL_INTEGRATION_ZSH.contains("133;B"));
        assert!(SHELL_INTEGRATION_BASH.contains("133;B"));
        assert!(SHELL_INTEGRATION_ZSH.contains("133;C"));
        assert!(SHELL_INTEGRATION_BASH.contains("133;C"));
        assert!(SHELL_INTEGRATION_ZSH.contains("133;D"));
        assert!(SHELL_INTEGRATION_BASH.contains("133;D"));
        assert!(SHELL_INTEGRATION_ZSH.contains("6973;termin"));
        assert!(SHELL_INTEGRATION_BASH.contains("6973;termin"));
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
        assert!(env
            .iter()
            .any(|(k, v)| k == "COLORTERM" && v == "truecolor"));
        assert!(env
            .iter()
            .any(|(k, v)| k == "TERM_PROGRAM" && v == "termin"));
        assert!(env.iter().any(|(k, v)| k == "TERMIN_TAB_ID" && v == "3"));
        assert!(env
            .iter()
            .any(|(k, v)| k == "TERMIN_SHELL_INTEGRATION" && v == "1"));
        assert!(env.iter().any(|(k, _)| k == "LANG"));
    }

    #[test]
    fn test_shell_resolution() {
        let (prog, args) = resolve_shell(Some("/bin/bash"));
        assert_eq!(prog, PathBuf::from("/bin/bash"));
        assert_eq!(args, vec!["-bash".to_string()]);

        let (prog2, _) = resolve_shell(None);
        assert!(prog2.exists());
    }

    #[test]
    fn test_deploy_shell_scripts() {
        let tmp = std::env::temp_dir().join(format!("termin_test_shell_{}", std::process::id()));
        let deployed = deploy_shell_scripts(Some(&tmp)).unwrap();
        assert!(deployed.join("termin-integration.zsh").exists());
        assert!(deployed.join("termin-integration.bash").exists());
        assert!(deployed.join("zsh/.zshenv").exists());
        assert!(deployed.join("bash/termin.bashrc").exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_prepare_injection_zsh() {
        let mut env = Vec::new();
        let mut args = vec!["-zsh".to_string()];
        let path = Path::new("/tmp/termin/shell/v1");
        prepare_injection(Path::new("/bin/zsh"), true, path, &mut env, &mut args);
        assert!(env
            .iter()
            .any(|(k, v)| k == "ZDOTDIR" && v.ends_with("/zsh")));
    }

    #[test]
    fn test_prepare_injection_bash() {
        let mut env = Vec::new();
        let mut args = vec!["-bash".to_string()];
        let path = Path::new("/tmp/termin/shell/v1");
        prepare_injection(Path::new("/bin/bash"), true, path, &mut env, &mut args);
        assert!(env
            .iter()
            .any(|(k, v)| k == "TERMIN_LOGIN_EMULATION" && v == "1"));
        assert!(args.iter().any(|a| a == "--rcfile"));
        assert!(args.iter().any(|a| a == "-i"));
    }
}
