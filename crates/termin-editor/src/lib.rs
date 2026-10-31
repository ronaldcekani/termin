//! Input editor state, shell tokenizer, history, and completions (`termin-editor`).
//!
//! See `docs/specs/input-editor.md`.

use std::ops::Range;

/// Index of command context within a multi-command or pipeline input.
///
/// See `docs/specs/input-editor.md §2`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CmdIdx(pub u32);

/// Shell syntax token kinds.
///
/// See `docs/specs/input-editor.md §2`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum TokenKind {
    Command,
    Argument,
    Flag,
    String,
    Variable,
    Operator,
    Comment,
    Path,
    Subshell,
    Whitespace,
    Error,
}

/// Token with byte range and command context.
///
/// See `docs/specs/input-editor.md §2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub range: Range<usize>,
    pub ctx: CmdIdx,
}

/// Undo/redo stack.
///
/// See `docs/specs/input-editor.md §1`.
#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    pub undo: Vec<(String, usize)>,
    pub redo: Vec<(String, usize)>,
}

/// Autosuggestion ghost text.
///
/// See `docs/specs/input-editor.md §3.2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub text: String,
    pub full_match: String,
}

/// A completion candidate item.
///
/// See `docs/specs/input-editor.md §4.1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub score: u32,
}

/// Completion popup menu state.
///
/// See `docs/specs/input-editor.md §4`.
#[derive(Clone, Debug, Default)]
pub struct CompletionMenu {
    pub candidates: Vec<Candidate>,
    pub selected: usize,
}

/// History navigation state for up/down filtering.
///
/// See `docs/specs/input-editor.md §3.2`.
#[derive(Clone, Debug)]
pub struct HistoryNav {
    pub prefix: String,
    pub index: usize,
}

/// Complete input editor state for one tab.
///
/// See `docs/specs/input-editor.md §1`.
#[derive(Clone, Debug, Default)]
pub struct EditorState {
    pub text: String,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub undo: UndoStack,
    pub tokens: Vec<Token>,
    pub suggestion: Option<Suggestion>,
    pub menu: Option<CompletionMenu>,
    pub history_nav: Option<HistoryNav>,
    pub preferred_col: Option<u16>,
}

impl EditorState {
    /// Create an empty editor state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert text at current cursor location.
    pub fn insert(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.tokens = tokenize(&self.text);
    }

    /// Submit current command text.
    pub fn submit(&mut self) -> String {
        let cmd = std::mem::take(&mut self.text);
        self.cursor = 0;
        self.anchor = None;
        self.tokens.clear();
        self.suggestion = None;
        self.menu = None;
        cmd
    }
}

/// Tokenize shell input string into typed tokens.
///
/// See `docs/specs/input-editor.md §2`.
pub fn tokenize(input: &str) -> Vec<Token> {
    if input.is_empty() {
        return Vec::new();
    }
    // Minimal stub tokenizer for Phase 0
    vec![Token {
        kind: TokenKind::Command,
        range: 0..input.len(),
        ctx: CmdIdx(0),
    }]
}

/// Single history entry.
///
/// See `docs/specs/input-editor.md §3.1`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub exit_code: Option<i32>,
    pub duration_ms: Option<u64>,
    pub cwd: String,
    pub shell: String,
    pub command: String,
}

/// In-memory history index.
///
/// See `docs/specs/input-editor.md §3.1`.
#[derive(Clone, Debug, Default)]
pub struct HistoryIndex {
    pub entries: Vec<HistoryEntry>,
}

impl HistoryIndex {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn push(&mut self, entry: HistoryEntry) {
        self.entries.push(entry);
    }
}

/// Context passed to completion engine.
///
/// See `docs/specs/input-editor.md §4.1`.
#[derive(Clone, Debug)]
pub struct CompletionContext {
    pub cwd: String,
    pub env_path: String,
    pub shell_kind: String,
}

/// Result produced by completion engine.
///
/// See `docs/specs/input-editor.md §4.1`.
#[derive(Clone, Debug, Default)]
pub struct CompletionResult {
    pub candidates: Vec<Candidate>,
    pub replace_range: Range<usize>,
}

/// Query completion engine for candidates at current cursor offset.
///
/// See `docs/specs/input-editor.md §4`.
pub fn complete(_buffer: &str, cursor: usize, _ctx: &CompletionContext) -> CompletionResult {
    CompletionResult {
        candidates: Vec::new(),
        replace_range: cursor..cursor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_insert_and_submit() {
        let mut editor = EditorState::new();
        editor.insert("ls -la");
        assert_eq!(editor.text, "ls -la");
        assert_eq!(editor.cursor, 6);
        let cmd = editor.submit();
        assert_eq!(cmd, "ls -la");
        assert_eq!(editor.text, "");
        assert_eq!(editor.cursor, 0);
    }
}
