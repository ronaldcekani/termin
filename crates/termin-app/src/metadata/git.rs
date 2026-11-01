//! Lightweight git metadata collection. Calls are intended for the worker thread.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitMetadata {
    pub root: Option<PathBuf>,
    pub branch: Option<String>,
    pub dirty: bool,
}

/// Read branch metadata without invoking git, walking upward from `cwd`.
/// Supports linked worktrees whose `.git` is a `gitdir:` file.
pub fn collect(cwd: &Path, check_dirty: bool) -> GitMetadata {
    let Some((root, git_dir)) = find_repo(cwd) else {
        return GitMetadata::default();
    };
    let head = fs::read_to_string(git_dir.join("HEAD")).ok();
    let branch = head.as_deref().and_then(parse_head);
    let dirty = check_dirty && git_dir.join("index").exists() && git_status_dirty(&root);
    GitMetadata {
        root: Some(root),
        branch,
        dirty,
    }
}

fn find_repo(cwd: &Path) -> Option<(PathBuf, PathBuf)> {
    for dir in cwd.ancestors() {
        let marker = dir.join(".git");
        if marker.is_dir() {
            return Some((dir.to_path_buf(), marker));
        }
        if let Ok(contents) = fs::read_to_string(&marker) {
            if let Some(path) = contents.trim().strip_prefix("gitdir:") {
                let path = PathBuf::from(path.trim());
                let git_dir = if path.is_absolute() {
                    path
                } else {
                    dir.join(path)
                };
                if git_dir.is_dir() {
                    return Some((dir.to_path_buf(), git_dir));
                }
            }
        }
    }
    None
}

fn parse_head(head: &str) -> Option<String> {
    let value = head.trim();
    let branch = value.strip_prefix("ref: refs/heads/")?;
    (!branch.is_empty()).then(|| branch.to_owned())
}

fn git_status_dirty(root: &Path) -> bool {
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::OpenOptionsExt;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let temp_path = std::env::temp_dir().join(format!(
        "termin-git-status-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let Ok(output) = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp_path)
    else {
        return false;
    };
    let Ok(mut child) = Command::new("git")
        .args(["status", "--porcelain=v1", "-uno", "-z"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(Stdio::null())
        .spawn()
    else {
        let _ = fs::remove_file(&temp_path);
        return false;
    };

    // Avoid an unbounded worker stall on broken filesystems or hooks.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let _ = fs::remove_file(&temp_path);
                    return false;
                }
                drop(child);
                let dirty = fs::metadata(&temp_path)
                    .map(|meta| meta.len() > 0)
                    .unwrap_or(false);
                let _ = fs::remove_file(temp_path);
                return dirty;
            }
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&temp_path);
                return false;
            }
        }
    }
}

/// Mtime of `.git/HEAD`, useful for invalidating a caller's branch cache.
pub fn head_mtime(root: &Path) -> Option<SystemTime> {
    let (_, git_dir) = find_repo(root)?;
    fs::metadata(git_dir.join("HEAD")).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_only_local_branch_refs() {
        assert_eq!(parse_head("ref: refs/heads/main\n"), Some("main".into()));
        assert_eq!(parse_head("a1b2c3\n"), None);
    }

    #[test]
    fn large_status_output_does_not_block_and_is_dirty() {
        use std::process::Command;
        let dir = std::env::temp_dir().join(format!("termin-git-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&dir)
                .status()
                .unwrap();
            assert!(status.success());
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "termin@example.invalid"]);
        git(&["config", "user.name", "Termin Test"]);
        for i in 0..1_200 {
            fs::write(dir.join(format!("file-{i:04}")), "baseline\n").unwrap();
        }
        git(&["add", "."]);
        git(&["commit", "-qm", "baseline"]);
        for i in 0..1_200 {
            fs::write(dir.join(format!("file-{i:04}")), "changed\n").unwrap();
        }
        assert!(git_status_dirty(&dir));
        let _ = fs::remove_dir_all(dir);
    }
}
