//! Built-in completion generators (filepaths, git refs, executables, ssh, history).
//!
//! See `docs/specs/input-editor.md §4.3`.

use rustc_hash::{FxHashMap, FxHashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::editor::Candidate;
use crate::history::HistoryIndex;

struct DirCacheEntry {
    cached_at: Instant,
    entries: Vec<(String, bool)>, // (name, is_dir)
}

static DIR_CACHE: Mutex<Option<FxHashMap<PathBuf, DirCacheEntry>>> = Mutex::new(None);

fn get_dir_entries_cached(dir_path: &Path) -> Vec<(String, bool)> {
    let now = Instant::now();
    let ttl = Duration::from_secs(2);

    if let Ok(mut guard) = DIR_CACHE.lock() {
        let map = guard.get_or_insert_with(FxHashMap::default);

        // Prune older entries if cache grows large
        if map.len() > 100 {
            map.retain(|_, v| now.duration_since(v.cached_at) < ttl);
        }

        if let Some(entry) = map.get(dir_path) {
            if now.duration_since(entry.cached_at) < ttl {
                return entry.entries.clone();
            }
        }

        // Read from filesystem
        if let Ok(rd) = fs::read_dir(dir_path) {
            let mut list = Vec::new();
            for item in rd.flatten() {
                let name = item.file_name().to_string_lossy().to_string();
                let is_dir = item.file_type().is_ok_and(|ft| ft.is_dir());
                list.push((name, is_dir));
            }
            map.insert(
                dir_path.to_path_buf(),
                DirCacheEntry {
                    cached_at: now,
                    entries: list.clone(),
                },
            );
            return list;
        }
    }

    Vec::new()
}

/// Generate file and folder candidates.
pub fn generate_filepaths(cwd: &str, token: &str, folders_only: bool) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let expanded_token = expand_tilde(token);

    let (dir_path, prefix, prefix_is_tilde) = if let Some(slash_pos) = expanded_token.rfind('/') {
        let (dir_str, file_prefix) = expanded_token.split_at(slash_pos + 1);
        let base_dir = if dir_str.starts_with('/') {
            PathBuf::from(dir_str)
        } else {
            Path::new(cwd).join(dir_str)
        };
        (base_dir, file_prefix, token.starts_with('~'))
    } else {
        (PathBuf::from(cwd), expanded_token.as_str(), false)
    };

    let cached_entries = get_dir_entries_cached(&dir_path);
    let show_hidden = prefix.starts_with('.');
    let prefix_lower = prefix.to_lowercase();
    let mut dir_entries = Vec::new();

    for (name_str, is_dir) in cached_entries {
        if !show_hidden && name_str.starts_with('.') {
            continue;
        }

        if !prefix.is_empty() {
            let name_lower = name_str.to_lowercase();
            if !name_lower.starts_with(&prefix_lower)
                && !is_fuzzy_subsequence(&prefix_lower, &name_lower)
            {
                continue;
            }
        }

        if folders_only && !is_dir {
            continue;
        }

        dir_entries.push((name_str, is_dir));
    }

    // Sort entries alphabetically
    dir_entries.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, is_dir) in dir_entries {
        let label = if is_dir {
            format!("{}/", name)
        } else {
            name.clone()
        };

        let insert_text = if let Some(slash_pos) = token.rfind('/') {
            let prefix_dir = &token[..=slash_pos];
            if is_dir {
                format!("{}{}/", prefix_dir, escape_path(&name))
            } else {
                format!("{}{}", prefix_dir, escape_path(&name))
            }
        } else if prefix_is_tilde {
            if is_dir {
                format!("~/{}/", escape_path(&name))
            } else {
                format!("~/{}", escape_path(&name))
            }
        } else if is_dir {
            format!("{}/", escape_path(&name))
        } else {
            escape_path(&name)
        };

        candidates.push(Candidate {
            label,
            detail: if is_dir {
                Some("directory".to_string())
            } else {
                Some("file".to_string())
            },
            insert_text,
            score: 0,
        });
    }

    candidates
}

/// Generate executable names from `$PATH`.
pub fn generate_executables(path_env: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut seen = FxHashSet::default();

    for dir in path_env.split(':') {
        if dir.is_empty() {
            continue;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_file() || file_type.is_symlink() {
                        let name = entry.file_name().to_string_lossy().to_string();
                        if seen.insert(name.clone()) {
                            candidates.push(Candidate {
                                label: name.clone(),
                                detail: Some("executable".to_string()),
                                insert_text: name,
                                score: 0,
                            });
                        }
                    }
                }
            }
        }
    }

    candidates
}

/// Generate environment variables.
pub fn generate_env_vars(env_vars: &[(String, String)], token: &str) -> Vec<Candidate> {
    let has_dollar = token.starts_with('$');
    let mut candidates = Vec::new();

    for (k, v) in env_vars {
        let insert_text = if has_dollar {
            format!("${}", k)
        } else {
            k.clone()
        };
        candidates.push(Candidate {
            label: format!("${}", k),
            detail: Some(v.clone()),
            insert_text,
            score: 0,
        });
    }

    candidates
}

/// Generate git branch names directly from `.git/` directory.
pub fn generate_git_branches(cwd: &str) -> Vec<Candidate> {
    let git_dir = match find_git_dir(cwd) {
        Some(d) => d,
        None => return Vec::new(),
    };

    let mut branches = FxHashSet::default();

    // 1. Loose refs in .git/refs/heads/
    let heads_dir = git_dir.join("refs/heads");
    collect_refs_recursive(&heads_dir, "", &mut branches);

    // 2. Packed refs
    let packed_refs = git_dir.join("packed-refs");
    if let Ok(content) = fs::read_to_string(packed_refs) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some(pos) = line.find(" refs/heads/") {
                let branch = &line[pos + " refs/heads/".len()..];
                branches.insert(branch.to_string());
            }
        }
    }

    let mut list: Vec<String> = branches.into_iter().collect();
    list.sort();

    list.into_iter()
        .map(|b| Candidate {
            label: b.clone(),
            detail: Some("git branch".to_string()),
            insert_text: b,
            score: 0,
        })
        .collect()
}

/// Generate git tag names directly from `.git/` directory.
pub fn generate_git_tags(cwd: &str) -> Vec<Candidate> {
    let git_dir = match find_git_dir(cwd) {
        Some(d) => d,
        None => return Vec::new(),
    };

    let mut tags = FxHashSet::default();

    // 1. Loose tags in .git/refs/tags/
    let tags_dir = git_dir.join("refs/tags");
    collect_refs_recursive(&tags_dir, "", &mut tags);

    // 2. Packed refs
    let packed_refs = git_dir.join("packed-refs");
    if let Ok(content) = fs::read_to_string(packed_refs) {
        for line in content.lines() {
            let line = line.trim();
            if line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            if let Some(pos) = line.find(" refs/tags/") {
                let tag = &line[pos + " refs/tags/".len()..];
                tags.insert(tag.to_string());
            }
        }
    }

    let mut list: Vec<String> = tags.into_iter().collect();
    list.sort();

    list.into_iter()
        .map(|t| Candidate {
            label: t.clone(),
            detail: Some("git tag".to_string()),
            insert_text: t,
            score: 0,
        })
        .collect()
}

/// Generate git remote names directly from `.git/config`.
pub fn generate_git_remotes(cwd: &str) -> Vec<Candidate> {
    let git_dir = match find_git_dir(cwd) {
        Some(d) => d,
        None => return Vec::new(),
    };

    let config_path = git_dir.join("config");
    let content = match fs::read_to_string(config_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut remotes = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[remote \"") {
            if let Some(name) = rest.strip_suffix("\"]") {
                remotes.push(Candidate {
                    label: name.to_string(),
                    detail: Some("git remote".to_string()),
                    insert_text: name.to_string(),
                    score: 0,
                });
            }
        }
    }

    remotes
}

/// Generate SSH host candidates from `~/.ssh/config` and `~/.ssh/known_hosts`.
pub fn generate_ssh_hosts() -> Vec<Candidate> {
    let mut hosts = FxHashSet::default();
    let home = std::env::var("HOME").unwrap_or_default();
    if home.is_empty() {
        return Vec::new();
    }

    let ssh_dir = Path::new(&home).join(".ssh");

    // 1. ~/.ssh/config
    let config_path = ssh_dir.join("config");
    if let Ok(content) = fs::read_to_string(config_path) {
        for line in content.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("Host ") {
                for host in rest.split_whitespace() {
                    if !host.contains('*') && !host.contains('?') && !host.starts_with('!') {
                        hosts.insert(host.to_string());
                    }
                }
            }
        }
    }

    // 2. ~/.ssh/known_hosts
    let known_hosts_path = ssh_dir.join("known_hosts");
    if let Ok(content) = fs::read_to_string(known_hosts_path) {
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
                continue;
            }
            if let Some(first_field) = line.split_whitespace().next() {
                for h in first_field.split(',') {
                    let cleaned = if let Some(bracketed) = h.strip_prefix('[') {
                        if let Some(pos) = bracketed.find(']') {
                            &bracketed[..pos]
                        } else {
                            h
                        }
                    } else {
                        h
                    };
                    if !cleaned.starts_with('|') && !cleaned.is_empty() {
                        hosts.insert(cleaned.to_string());
                    }
                }
            }
        }
    }

    let mut list: Vec<String> = hosts.into_iter().collect();
    list.sort();

    list.into_iter()
        .map(|h| Candidate {
            label: h.clone(),
            detail: Some("ssh host".to_string()),
            insert_text: h,
            score: 0,
        })
        .collect()
}

/// Generate history argument candidates for a command.
pub fn generate_history_args(history: &HistoryIndex, cmd_name: &str) -> Vec<Candidate> {
    let mut freqs: rustc_hash::FxHashMap<String, u32> = rustc_hash::FxHashMap::default();
    let max_scan = 5_000.min(history.len());

    for i in (history.len().saturating_sub(max_scan)..history.len()).rev() {
        if let Some(cmd) = history.get_command(i) {
            let parts: Vec<&str> = cmd.split_whitespace().collect();
            if !parts.is_empty() && parts[0] == cmd_name {
                for &arg in &parts[1..] {
                    if !arg.starts_with('-') {
                        *freqs.entry(arg.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let mut items: Vec<(String, u32)> = freqs.into_iter().collect();
    items.sort_by_key(|a| std::cmp::Reverse(a.1));

    items
        .into_iter()
        .take(50)
        .map(|(arg, count)| Candidate {
            label: arg.clone(),
            detail: Some(format!("used {}x", count)),
            insert_text: arg,
            score: 0,
        })
        .collect()
}

// ============================================================================
// Async Generator Parsers
// ============================================================================

/// Parse NUL-separated `git status -z` output into candidates.
pub fn parse_git_status_porcelain_z(bytes: &[u8]) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        // Records are: "XY <path>\0" or "R  <path>\0<orig_path>\0"
        if i + 3 > len {
            break;
        }

        let status_bytes = &bytes[i..i + 2];
        let status_str = String::from_utf8_lossy(status_bytes).to_string();
        i += 3; // skip status and space

        let path_end = memchr::memchr(b'\0', &bytes[i..])
            .map(|pos| i + pos)
            .unwrap_or(len);
        let path = String::from_utf8_lossy(&bytes[i..path_end]).to_string();
        i = if path_end < len { path_end + 1 } else { len };

        // For renames (R), next field is the original path
        if status_bytes[0] == b'R' && i < len {
            let orig_end = memchr::memchr(b'\0', &bytes[i..])
                .map(|pos| i + pos)
                .unwrap_or(len);
            i = if orig_end < len { orig_end + 1 } else { len };
        }

        if !path.is_empty() {
            candidates.push(Candidate {
                label: path.clone(),
                detail: Some(format!("git status [{}]", status_str)),
                insert_text: escape_path(&path),
                score: 0,
            });
        }
    }

    candidates
}

/// Parse npm scripts from `package.json` content.
pub fn parse_npm_scripts(json_content: &str) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(json_content) {
        if let Some(scripts) = val.get("scripts").and_then(|s| s.as_object()) {
            for (name, cmd_val) in scripts {
                let detail = cmd_val.as_str().map(|s| s.to_string());
                candidates.push(Candidate {
                    label: name.clone(),
                    detail,
                    insert_text: name.clone(),
                    score: 0,
                });
            }
        }
    }
    candidates
}

/// Parse Makefile target names without regex.
pub fn parse_make_targets(makefile_content: &str) -> Vec<Candidate> {
    let mut targets = FxHashSet::default();

    for line in makefile_content.lines() {
        // Targets start at col 0, contain ':' but not '='
        if line.starts_with('\t') || line.starts_with(' ') || line.starts_with('#') {
            continue;
        }
        if let Some(colon_pos) = line.find(':') {
            let before_colon = &line[..colon_pos].trim();
            if before_colon.is_empty() || before_colon.contains('=') {
                continue;
            }
            for t in before_colon.split_whitespace() {
                if !t.starts_with('.') && !t.contains('$') && !t.contains('%') {
                    targets.insert(t.to_string());
                }
            }
        }
    }

    let mut list: Vec<String> = targets.into_iter().collect();
    list.sort();

    list.into_iter()
        .map(|t| Candidate {
            label: t.clone(),
            detail: Some("make target".to_string()),
            insert_text: t,
            score: 0,
        })
        .collect()
}

// ============================================================================
// Helpers
// ============================================================================

fn find_git_dir(start: &str) -> Option<PathBuf> {
    let mut cur = PathBuf::from(start);
    loop {
        let candidate = cur.join(".git");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if candidate.is_file() {
            // Git submodule or worktree: points to gitdir
            if let Ok(content) = fs::read_to_string(&candidate) {
                if let Some(rest) = content.trim().strip_prefix("gitdir: ") {
                    let gitdir_path = cur.join(rest);
                    if gitdir_path.exists() {
                        return Some(gitdir_path);
                    }
                }
            }
        }
        if !cur.pop() {
            break;
        }
    }
    None
}

fn collect_refs_recursive(dir: &Path, prefix: &str, out: &mut FxHashSet<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let sub_prefix = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{}/{}", prefix, name)
            };
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    collect_refs_recursive(&entry.path(), &sub_prefix, out);
                } else if ft.is_file() {
                    out.insert(sub_prefix);
                }
            }
        }
    }
}

fn is_fuzzy_subsequence(needle: &str, haystack: &str) -> bool {
    let mut it = haystack.chars();
    for nc in needle.chars() {
        if !it.any(|hc| hc == nc) {
            return false;
        }
    }
    true
}

fn expand_tilde(path: &str) -> String {
    if path == "~" {
        std::env::var("HOME").unwrap_or_else(|_| "~".to_string())
    } else if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            format!("{}/{}", home, rest)
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    }
}

fn escape_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_whitespace()
            || matches!(
                c,
                '(' | ')' | '[' | ']' | '{' | '}' | '&' | ';' | '*' | '?' | '$'
            )
        {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_npm_scripts() {
        let json = r#"{
            "name": "my-app",
            "scripts": {
                "start": "node index.js",
                "build": "tsc",
                "test": "jest"
            }
        }"#;
        let candidates = parse_npm_scripts(json);
        assert_eq!(candidates.len(), 3);
        let names: Vec<&str> = candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(names.contains(&"start"));
        assert!(names.contains(&"build"));
        assert!(names.contains(&"test"));
    }

    #[test]
    fn test_parse_make_targets() {
        let makefile = "all: build test\n\nbuild:\n\tcargo build\n\ntest:\n\tcargo test\n\n.PHONY: all build test\n";
        let targets = parse_make_targets(makefile);
        let names: Vec<&str> = targets.iter().map(|c| c.label.as_str()).collect();
        assert!(names.contains(&"all"));
        assert!(names.contains(&"build"));
        assert!(names.contains(&"test"));
        assert!(!names.contains(&".PHONY"));
    }

    #[test]
    fn test_parse_git_status_z() {
        let mut raw = b" M src/lib.rs\0".to_vec();
        raw.extend_from_slice(b"?? newfile.txt\0");
        let candidates = parse_git_status_porcelain_z(&raw);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].label, "src/lib.rs");
        assert_eq!(candidates[1].label, "newfile.txt");
    }
}
