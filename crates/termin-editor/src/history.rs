//! History store: Termin TSV format, zsh/bash parsers, arena index, autosuggest, fuzzy search.
//!
//! See `docs/specs/input-editor.md §3`.

use std::hash::{Hash, Hasher};
use rustc_hash::FxHashMap;
use nucleo_matcher::{Config, Matcher, Utf32Str};

use crate::editor::Suggestion;

/// Default history retention cap.
pub const HISTORY_CAP: usize = 50_000;

/// Single history entry representation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub cwd: String,
    pub shell: String,
    pub command: String,
}

impl HistoryEntry {
    pub fn new(
        timestamp: u64,
        exit_code: Option<i32>,
        duration_ms: Option<u64>,
        cwd: impl Into<String>,
        shell: impl Into<String>,
        command: impl Into<String>,
    ) -> Self {
        Self {
            timestamp,
            exit_code,
            duration_ms,
            cwd: cwd.into(),
            shell: shell.into(),
            command: command.into(),
        }
    }
}

/// Compact index entry referencing offsets into the history arena string.
#[derive(Clone, Copy, Debug)]
pub struct HistoryItem {
    pub timestamp: u64,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub cwd_start: u32,
    pub cwd_len: u32,
    pub shell_start: u32,
    pub shell_len: u32,
    pub cmd_start: u32,
    pub cmd_len: u32,
}

/// Result of a fuzzy history search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistorySearchResult {
    pub index: usize,
    pub score: u32,
    pub command: String,
    pub cwd: String,
    pub timestamp: u64,
    pub exit_code: Option<i32>,
}

/// In-memory history index with arena string storage.
///
/// Stores entries newest-last. Strings live in a single arena `String` to avoid
/// per-entry heap allocations (~50k entries take ~2 MB).
#[derive(Clone, Debug, Default)]
pub struct HistoryIndex {
    arena: String,
    items: Vec<HistoryItem>,
    by_text: FxHashMap<u64, u32>,
}

impl HistoryIndex {
    /// Create a new empty history index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of entries stored.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True if index contains no entries.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.arena.clear();
        self.items.clear();
        self.by_text.clear();
    }

    /// Push an owned entry.
    pub fn push(&mut self, entry: HistoryEntry) {
        self.push_ref(
            entry.timestamp,
            entry.exit_code,
            entry.duration_ms,
            &entry.cwd,
            &entry.shell,
            &entry.command,
        );
    }

    /// Push an entry referencing borrowed strings.
    pub fn push_ref(
        &mut self,
        timestamp: u64,
        exit_code: Option<i32>,
        duration_ms: Option<u64>,
        cwd: &str,
        shell: &str,
        command: &str,
    ) {
        let cwd_start = self.arena.len() as u32;
        self.arena.push_str(cwd);
        let cwd_len = (self.arena.len() as u32) - cwd_start;

        let shell_start = self.arena.len() as u32;
        self.arena.push_str(shell);
        let shell_len = (self.arena.len() as u32) - shell_start;

        let cmd_start = self.arena.len() as u32;
        self.arena.push_str(command);
        let cmd_len = (self.arena.len() as u32) - cmd_start;

        let item_idx = self.items.len() as u32;
        self.items.push(HistoryItem {
            timestamp,
            exit_code,
            duration_ms,
            cwd_start,
            cwd_len,
            shell_start,
            shell_len,
            cmd_start,
            cmd_len,
        });

        let hash = hash_str(command);
        self.by_text.insert(hash, item_idx);
    }

    /// Retrieve full cloned HistoryEntry at index.
    pub fn get_entry(&self, index: usize) -> Option<HistoryEntry> {
        let item = self.items.get(index)?;
        Some(HistoryEntry {
            timestamp: item.timestamp,
            exit_code: item.exit_code,
            duration_ms: item.duration_ms,
            cwd: self.get_cwd(index)?.to_string(),
            shell: self.get_shell(index)?.to_string(),
            command: self.get_command(index)?.to_string(),
        })
    }

    /// Borrow command string at index.
    pub fn get_command(&self, index: usize) -> Option<&str> {
        let item = self.items.get(index)?;
        let start = item.cmd_start as usize;
        let end = start + (item.cmd_len as usize);
        self.arena.get(start..end)
    }

    /// Borrow cwd string at index.
    pub fn get_cwd(&self, index: usize) -> Option<&str> {
        let item = self.items.get(index)?;
        let start = item.cwd_start as usize;
        let end = start + (item.cwd_len as usize);
        self.arena.get(start..end)
    }

    /// Borrow shell string at index.
    pub fn get_shell(&self, index: usize) -> Option<&str> {
        let item = self.items.get(index)?;
        let start = item.shell_start as usize;
        let end = start + (item.shell_len as usize);
        self.arena.get(start..end)
    }

    /// Autosuggestion: find the most recent command starting with `buffer`.
    ///
    /// Scans backwards (newest to oldest), prioritizing entries from `cwd`.
    /// Early exits after scanning at most 20 000 entries.
    pub fn suggest(&self, buffer: &str, cwd: &str) -> Option<Suggestion> {
        if buffer.is_empty() || self.items.is_empty() {
            return None;
        }

        let max_scan = 20_000.min(self.items.len());
        let start_idx = self.items.len() - 1;
        let end_idx = self.items.len() - max_scan;

        // Pass 1: same cwd
        for idx in (end_idx..=start_idx).rev() {
            if let Some(entry_cwd) = self.get_cwd(idx) {
                if entry_cwd == cwd {
                    if let Some(cmd) = self.get_command(idx) {
                        if cmd.len() > buffer.len() && cmd.starts_with(buffer) {
                            return Some(Suggestion {
                                text: cmd[buffer.len()..].to_string(),
                                full_match: cmd.to_string(),
                            });
                        }
                    }
                }
            }
        }

        // Pass 2: any cwd
        for idx in (end_idx..=start_idx).rev() {
            if let Some(cmd) = self.get_command(idx) {
                if cmd.len() > buffer.len() && cmd.starts_with(buffer) {
                    return Some(Suggestion {
                        text: cmd[buffer.len()..].to_string(),
                        full_match: cmd.to_string(),
                    });
                }
            }
        }

        None
    }

    /// Up/Down prefix navigation.
    ///
    /// Finds next match starting with `prefix`. When `up` is true, navigates towards
    /// older entries; when false, navigates towards newer entries.
    /// Prefer entries with the same `current_cwd`.
    pub fn navigate_prefix(
        &self,
        prefix: &str,
        current_cwd: &str,
        current_index: Option<usize>,
        up: bool,
    ) -> Option<(usize, String)> {
        if self.items.is_empty() {
            return None;
        }

        let len = self.items.len();
        if up {
            let start = current_index.unwrap_or(len);
            if start == 0 {
                return None;
            }

            // Search older (backwards from start - 1 down to 0)
            // Same cwd first
            for i in (0..start).rev() {
                if let Some(cwd) = self.get_cwd(i) {
                    if cwd == current_cwd {
                        if let Some(cmd) = self.get_command(i) {
                            if cmd.starts_with(prefix) {
                                return Some((i, cmd.to_string()));
                            }
                        }
                    }
                }
            }

            // Then any cwd
            for i in (0..start).rev() {
                if let Some(cmd) = self.get_command(i) {
                    if cmd.starts_with(prefix) {
                        return Some((i, cmd.to_string()));
                    }
                }
            }
        } else {
            // Down: newer entries
            let start = current_index? + 1;

            for i in start..len {
                if let Some(cmd) = self.get_command(i) {
                    if cmd.starts_with(prefix) {
                        return Some((i, cmd.to_string()));
                    }
                }
            }
        }

        None
    }

    /// Fuzzy search across all unique commands using `nucleo-matcher`.
    pub fn search_fuzzy(&self, query: &str, max_results: usize) -> Vec<HistorySearchResult> {
        if self.items.is_empty() || max_results == 0 {
            return Vec::new();
        }

        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut results = Vec::new();

        // If query is empty, return newest entries
        if query.is_empty() {
            let mut seen = rustc_hash::FxHashSet::default();
            for i in (0..self.items.len()).rev() {
                if let Some(cmd) = self.get_command(i) {
                    if seen.insert(cmd) {
                        results.push(HistorySearchResult {
                            index: i,
                            score: 0,
                            command: cmd.to_string(),
                            cwd: self.get_cwd(i).unwrap_or("").to_string(),
                            timestamp: self.items[i].timestamp,
                            exit_code: self.items[i].exit_code,
                        });
                        if results.len() >= max_results {
                            break;
                        }
                    }
                }
            }
            return results;
        }

        let mut buf = Vec::new();
        let query_u32 = Utf32Str::new(query, &mut buf);
        let mut seen = rustc_hash::FxHashSet::default();

        for i in (0..self.items.len()).rev() {
            if let Some(cmd) = self.get_command(i) {
                if seen.insert(cmd) {
                    let mut cmd_buf = Vec::new();
                    let haystack = Utf32Str::new(cmd, &mut cmd_buf);
                    if let Some(score) = matcher.fuzzy_match(haystack, query_u32) {
                        results.push(HistorySearchResult {
                            index: i,
                            score: score as u32,
                            command: cmd.to_string(),
                            cwd: self.get_cwd(i).unwrap_or("").to_string(),
                            timestamp: self.items[i].timestamp,
                            exit_code: self.items[i].exit_code,
                        });
                    }
                }
            }
        }

        results.sort_by_key(|a| std::cmp::Reverse(a.score));
        results.truncate(max_results);
        results
    }
}

fn hash_str(s: &str) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    s.hash(&mut hasher);
    hasher.finish()
}

// ============================================================================
// Termin TSV Format
// ============================================================================

/// Format a history entry as a Termin TSV line:
/// `<unix_ts>\t<exit|->\t<duration_ms|->\t<cwd>\t<shell>\t<escaped_cmd>\n`
pub fn format_tsv_entry(entry: &HistoryEntry) -> String {
    let exit_str = match entry.exit_code {
        Some(c) => c.to_string(),
        None => "-".to_string(),
    };
    let dur_str = match entry.duration_ms {
        Some(d) => d.to_string(),
        None => "-".to_string(),
    };
    let escaped_cmd = escape_tsv_string(&entry.command);
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\n",
        entry.timestamp, exit_str, dur_str, entry.cwd, entry.shell, escaped_cmd
    )
}

/// Parse a single Termin TSV line.
pub fn parse_tsv_line(line: &str) -> Option<HistoryEntry> {
    let line = line.trim_end_matches(['\r', '\n']);
    if line.is_empty() {
        return None;
    }

    let mut parts = line.split('\t');
    let ts_str = parts.next()?;
    let exit_str = parts.next()?;
    let dur_str = parts.next()?;
    let cwd = parts.next()?;
    let shell = parts.next()?;
    let cmd_str = parts.next()?;

    let timestamp = ts_str.parse::<u64>().ok()?;
    let exit_code = if exit_str == "-" {
        None
    } else {
        exit_str.parse::<i32>().ok()
    };
    let duration_ms = if dur_str == "-" {
        None
    } else {
        dur_str.parse::<u64>().ok()
    };
    let command = unescape_tsv_string(cmd_str);

    Some(HistoryEntry {
        timestamp,
        exit_code,
        duration_ms,
        cwd: cwd.to_string(),
        shell: shell.to_string(),
        command,
    })
}

/// Parse an entire Termin TSV history file content.
pub fn parse_tsv(content: &str) -> Vec<HistoryEntry> {
    content.lines().filter_map(parse_tsv_line).collect()
}

/// Rotate Termin TSV history string: if entries > 2 * target, retain latest target entries.
pub fn rotate_tsv(content: &str, target_count: usize) -> Option<String> {
    let entries = parse_tsv(content);
    if entries.len() > 2 * target_count {
        let keep_start = entries.len() - target_count;
        let mut out = String::with_capacity(content.len() / 2);
        for entry in &entries[keep_start..] {
            out.push_str(&format_tsv_entry(entry));
        }
        Some(out)
    } else {
        None
    }
}

/// Escape `\`, `\n`, `\t`, `\r` for TSV storage.
pub fn escape_tsv_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// Unescape TSV string fields.
pub fn unescape_tsv_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

// ============================================================================
// Shell History Parsers (bash & zsh)
// ============================================================================

/// Parse plain bash history format, with optional `#<timestamp>` preceding lines.
pub fn parse_bash_history(content: &str, default_cwd: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut pending_ts: Option<u64> = None;

    for line in content.lines() {
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            continue;
        }

        if let Some(ts_part) = line.strip_prefix('#') {
            if let Ok(ts) = ts_part.parse::<u64>() {
                pending_ts = Some(ts);
                continue;
            }
        }

        let timestamp = pending_ts.take().unwrap_or(0);
        entries.push(HistoryEntry {
            timestamp,
            exit_code: None,
            duration_ms: None,
            cwd: default_cwd.to_string(),
            shell: "bash".to_string(),
            command: line.to_string(),
        });
    }

    entries
}

/// Parse zsh extended history format (`: <ts>:<dur>;cmd`), decoding zsh metafied bytes (`0x83`).
pub fn parse_zsh_history(content: &str, default_cwd: &str) -> Vec<HistoryEntry> {
    parse_zsh_history_bytes(content.as_bytes(), default_cwd)
}

/// Parse zsh extended history from raw bytes (handling metafied bytes accurately).
pub fn parse_zsh_history_bytes(bytes: &[u8], default_cwd: &str) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut i = 0;
    let len = bytes.len();

    while i < len {
        // Find line start
        while i < len && (bytes[i] == b'\n' || bytes[i] == b'\r') {
            i += 1;
        }
        if i >= len {
            break;
        }

        let mut line_bytes = Vec::new();

        // Read command, handling multi-line continuations (trailing `\`)
        while i < len {
            let next_nl = memchr::memchr(b'\n', &bytes[i..]).map(|pos| i + pos).unwrap_or(len);
            let mut chunk_end = next_nl;
            if chunk_end > i && bytes[chunk_end - 1] == b'\r' {
                chunk_end -= 1;
            }

            let is_continuation = chunk_end > i && bytes[chunk_end - 1] == b'\\';
            if is_continuation {
                line_bytes.extend_from_slice(&bytes[i..chunk_end - 1]);
                line_bytes.push(b'\n');
                i = if next_nl < len { next_nl + 1 } else { len };
            } else {
                line_bytes.extend_from_slice(&bytes[i..chunk_end]);
                i = if next_nl < len { next_nl + 1 } else { len };
                break;
            }
        }

        if line_bytes.is_empty() {
            continue;
        }

        // Unmetafy the decoded line bytes
        let unmetafied = decode_zsh_metafied(&line_bytes);
        let line_str = String::from_utf8_lossy(&unmetafied);

        // Check if extended format: `: <ts>:<dur>;<cmd>`
        if let Some(rest) = line_str.strip_prefix(": ") {
            if let Some(semi_pos) = rest.find(';') {
                let meta = &rest[..semi_pos];
                let cmd = &rest[semi_pos + 1..];

                let mut meta_parts = meta.split(':');
                let ts = meta_parts.next().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
                let dur = meta_parts.next().and_then(|s| s.trim().parse::<u64>().ok());

                entries.push(HistoryEntry {
                    timestamp: ts,
                    exit_code: None,
                    duration_ms: dur.map(|d| d * 1000),
                    cwd: default_cwd.to_string(),
                    shell: "zsh".to_string(),
                    command: cmd.to_string(),
                });
                continue;
            }
        }

        // Plain line fallback
        entries.push(HistoryEntry {
            timestamp: 0,
            exit_code: None,
            duration_ms: None,
            cwd: default_cwd.to_string(),
            shell: "zsh".to_string(),
            command: line_str.to_string(),
        });
    }

    entries
}

/// Decode zsh metafied bytes: `0x83` followed by byte `b` decodes to `b ^ 0x20`.
pub fn decode_zsh_metafied(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x83 && i + 1 < bytes.len() {
            out.push(bytes[i + 1] ^ 0x20);
            i += 2;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tsv_round_trip() {
        let entry = HistoryEntry {
            timestamp: 1730000000,
            exit_code: Some(0),
            duration_ms: Some(150),
            cwd: "/Users/dev/termin".to_string(),
            shell: "zsh".to_string(),
            command: "echo \"hello\tworld\"\ngit status".to_string(),
        };

        let tsv_line = format_tsv_entry(&entry);
        let parsed = parse_tsv_line(&tsv_line).expect("must parse");
        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_tsv_optional_fields() {
        let entry = HistoryEntry {
            timestamp: 1730000100,
            exit_code: None,
            duration_ms: None,
            cwd: "/tmp".to_string(),
            shell: "bash".to_string(),
            command: "pwd".to_string(),
        };

        let tsv_line = format_tsv_entry(&entry);
        assert!(tsv_line.contains("\t-\t-\t"));
        let parsed = parse_tsv_line(&tsv_line).unwrap();
        assert_eq!(entry, parsed);
    }

    #[test]
    fn test_bash_history_parsing() {
        let content = "#1730000001\nls -la\n#1730000002\ncargo test\npwd\n";
        let entries = parse_bash_history(content, "/home/dev");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].command, "ls -la");
        assert_eq!(entries[0].timestamp, 1730000001);
        assert_eq!(entries[1].command, "cargo test");
        assert_eq!(entries[1].timestamp, 1730000002);
        assert_eq!(entries[2].command, "pwd");
        assert_eq!(entries[2].timestamp, 0);
    }

    #[test]
    fn test_zsh_history_extended_and_metafied() {
        // ': 1730000005:2;echo café' where café has é (0xc3, 0xa9) metafied
        // In zsh: 0xc3 ^ 0x20 = 0xe3, so 0x83 0xe3
        let mut raw = b": 1730000005:2;echo caf".to_vec();
        raw.push(0x83);
        raw.push(0xc3 ^ 0x20); // encodes 0xc3
        raw.push(0x83);
        raw.push(0xa9 ^ 0x20); // encodes 0xa9
        raw.push(b'\n');

        let entries = parse_zsh_history_bytes(&raw, "/Users/dev");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].timestamp, 1730000005);
        assert_eq!(entries[0].duration_ms, Some(2000));
        assert_eq!(entries[0].command, "echo café");
    }

    #[test]
    fn test_zsh_multiline_continuation() {
        let raw = b": 1730000010:1;echo line1\\\necho line2\n";
        let entries = parse_zsh_history_bytes(raw, "/");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].command, "echo line1\necho line2");
    }

    #[test]
    fn test_history_autosuggest_speed() {
        let mut index = HistoryIndex::new();
        for i in 0..50_000 {
            index.push_ref(
                1730000000 + i,
                Some(0),
                Some(10),
                "/workspace/project",
                "zsh",
                &format!("cargo test package_{}", i),
            );
        }

        let start = std::time::Instant::now();
        let sug = index.suggest("cargo test package_4999", "/workspace/project");
        let elapsed = start.elapsed();

        assert!(sug.is_some());
        assert_eq!(sug.unwrap().full_match, "cargo test package_49999");
        // Must complete well under 0.5 ms (500 µs)
        assert!(elapsed.as_micros() < 500, "Elapsed was {:?}", elapsed);
    }

    #[test]
    fn test_history_fuzzy_search() {
        let mut index = HistoryIndex::new();
        index.push_ref(1, Some(0), Some(1), "/dir", "zsh", "git checkout main");
        index.push_ref(2, Some(0), Some(1), "/dir", "zsh", "git cherry-pick 12345");
        index.push_ref(3, Some(0), Some(1), "/dir", "zsh", "cargo build --release");

        let matches = index.search_fuzzy("gchm", 10);
        assert!(!matches.is_empty());
        assert_eq!(matches[0].command, "git checkout main");
    }
}
