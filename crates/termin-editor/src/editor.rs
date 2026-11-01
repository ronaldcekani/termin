//! Command buffer, cursor movement, selection, undo/redo, and editing commands.
//!
//! See `docs/specs/input-editor.md §1`.

use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

use crate::lexer::{tokenize, Token};

/// Maximum allowed length of the command buffer in bytes (1 MiB).
pub const MAX_BUFFER_SIZE: usize = 1024 * 1024;

/// Maximum entries kept on the undo or redo stack.
pub const MAX_UNDO_ENTRIES: usize = 200;

/// Autosuggestion ghost text state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    /// Ghost text suffix to display after cursor.
    pub text: String,
    /// Full command string that matched.
    pub full_match: String,
}

/// A completion candidate item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub detail: Option<String>,
    pub insert_text: String,
    pub score: u32,
}

/// Completion popup menu state.
#[derive(Clone, Debug, Default)]
pub struct CompletionMenu {
    pub candidates: Vec<Candidate>,
    pub selected: usize,
}

impl CompletionMenu {
    pub fn new(candidates: Vec<Candidate>) -> Self {
        Self {
            candidates,
            selected: 0,
        }
    }

    pub fn select_next(&mut self) {
        if !self.candidates.is_empty() {
            self.selected = (self.selected + 1) % self.candidates.len();
        }
    }

    pub fn select_prev(&mut self) {
        if !self.candidates.is_empty() {
            if self.selected == 0 {
                self.selected = self.candidates.len() - 1;
            } else {
                self.selected -= 1;
            }
        }
    }

    pub fn selected_candidate(&self) -> Option<&Candidate> {
        self.candidates.get(self.selected)
    }
}

/// History navigation state for up/down prefix filtering.
#[derive(Clone, Debug)]
pub struct HistoryNav {
    /// Initial text before cursor when navigation started.
    pub prefix: String,
    /// Preserved original buffer text before history nav.
    pub original_text: String,
    /// Preserved original cursor position.
    pub original_cursor: usize,
    /// Index in history currently displayed (if any).
    pub index: usize,
}

/// Undo and redo stack with word/time coalescing.
#[derive(Clone, Debug, Default)]
pub struct UndoStack {
    pub undo: Vec<(String, usize)>,
    pub redo: Vec<(String, usize)>,
    last_edit: Option<std::time::Instant>,
    in_word: bool,
}

impl UndoStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a change checkpoint before an edit.
    pub fn record_edit(&mut self, text: &str, cursor: usize, is_boundary: bool) {
        let now = std::time::Instant::now();
        let timeout = self
            .last_edit
            .map_or(true, |t| now.duration_since(t).as_secs() >= 1);

        if timeout || is_boundary || !self.in_word {
            self.push_undo(text, cursor);
            self.in_word = !is_boundary;
        }

        self.last_edit = Some(now);
        self.redo.clear();
    }

    /// Force a distinct undo checkpoint.
    pub fn push_checkpoint(&mut self, text: &str, cursor: usize) {
        self.push_undo(text, cursor);
        self.in_word = false;
        self.last_edit = Some(std::time::Instant::now());
        self.redo.clear();
    }

    fn push_undo(&mut self, text: &str, cursor: usize) {
        if self.undo.len() >= MAX_UNDO_ENTRIES {
            self.undo.remove(0);
        }
        self.undo.push((text.to_string(), cursor));
    }

    /// Undo to previous state.
    pub fn undo(&mut self, text: &mut String, cursor: &mut usize) -> bool {
        if let Some((prev_text, prev_cursor)) = self.undo.pop() {
            if self.redo.len() >= MAX_UNDO_ENTRIES {
                self.redo.remove(0);
            }
            self.redo.push((text.clone(), *cursor));
            *text = prev_text;
            *cursor = prev_cursor.min(text.len());
            self.in_word = false;
            self.last_edit = None;
            true
        } else {
            false
        }
    }

    /// Redo to next state.
    pub fn redo(&mut self, text: &mut String, cursor: &mut usize) -> bool {
        if let Some((next_text, next_cursor)) = self.redo.pop() {
            if self.undo.len() >= MAX_UNDO_ENTRIES {
                self.undo.remove(0);
            }
            self.undo.push((text.clone(), *cursor));
            *text = next_text;
            *cursor = next_cursor.min(text.len());
            self.in_word = false;
            self.last_edit = None;
            true
        } else {
            false
        }
    }
}

/// Complete input editor state for one tab.
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
    pub kill_ring: String,
}

impl EditorState {
    /// Create an empty editor state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Recompute highlighting tokens for current buffer text.
    pub fn retokenize(&mut self) {
        self.tokens = tokenize(&self.text);
    }

    /// True if there is an active text selection.
    pub fn has_selection(&self) -> bool {
        match self.anchor {
            Some(a) => a != self.cursor,
            None => false,
        }
    }

    /// Returns the selection range in bytes, if active.
    pub fn selection_range(&self) -> Option<Range<usize>> {
        match self.anchor {
            Some(a) if a != self.cursor => {
                let start = a.min(self.cursor);
                let end = a.max(self.cursor);
                Some(start..end)
            }
            _ => None,
        }
    }

    /// Returns the currently selected text, if any.
    pub fn selected_text(&self) -> Option<&str> {
        self.selection_range().map(|r| &self.text[r])
    }

    /// Delete selected text if any exists. Returns true if selection was deleted.
    pub fn delete_selection(&mut self) -> bool {
        if let Some(r) = self.selection_range() {
            self.undo.push_checkpoint(&self.text, self.cursor);
            self.text.drain(r.clone());
            self.cursor = r.start;
            self.anchor = None;
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else {
            false
        }
    }

    /// Insert text at current cursor location (replacing active selection).
    pub fn insert(&mut self, text: &str) {
        let _ = self.try_insert(text);
    }

    /// Attempt to insert text. Enforces 1 MiB buffer size cap.
    pub fn try_insert(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return true;
        }

        let sel_len = self.selection_range().map_or(0, |r| r.len());
        if self.text.len() - sel_len + text.len() > MAX_BUFFER_SIZE {
            return false;
        }

        let is_boundary = text
            .chars()
            .any(|c| c.is_whitespace() || !c.is_alphanumeric());
        self.undo.record_edit(&self.text, self.cursor, is_boundary);

        if let Some(r) = self.selection_range() {
            self.text.drain(r.clone());
            self.cursor = r.start;
            self.anchor = None;
        }

        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
        self.clamp_cursor();
        self.preferred_col = None;
        self.suggestion = None;
        self.retokenize();
        true
    }

    /// Insert newline (`Shift+Enter` or `Opt+Enter`).
    pub fn insert_newline(&mut self) {
        self.insert("\n");
    }

    /// Backspace (`⌫`): delete char before cursor, or delete selection.
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }

        if self.cursor > 0 {
            let prev = self.prev_grapheme_offset(self.cursor);
            self.undo.record_edit(&self.text, self.cursor, true);
            self.text.drain(prev..self.cursor);
            self.cursor = prev;
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else {
            false
        }
    }

    /// Delete forward (`⌦` or `Ctrl+D`): delete char after cursor, or delete selection.
    pub fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }

        if self.cursor < self.text.len() {
            let next = self.next_grapheme_offset(self.cursor);
            self.undo.record_edit(&self.text, self.cursor, true);
            self.text.drain(self.cursor..next);
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else {
            false
        }
    }

    /// Delete word backward (`⌥⌫` / `Ctrl+W`).
    pub fn delete_word_backward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }

        if self.cursor > 0 {
            let target = self.find_word_backward_offset(self.cursor);
            let killed = self.text[target..self.cursor].to_string();
            self.kill_ring = killed;
            self.undo.push_checkpoint(&self.text, self.cursor);
            self.text.drain(target..self.cursor);
            self.cursor = target;
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else {
            false
        }
    }

    /// Delete word forward (`⌥⌦`).
    pub fn delete_word_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }

        if self.cursor < self.text.len() {
            let target = self.find_word_forward_offset(self.cursor);
            self.undo.push_checkpoint(&self.text, self.cursor);
            self.text.drain(self.cursor..target);
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else {
            false
        }
    }

    /// Delete to line start (`⌘⌫` / `Ctrl+U`).
    pub fn delete_to_line_start(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }

        let start = self.line_start_offset(self.cursor);
        if start < self.cursor {
            let killed = self.text[start..self.cursor].to_string();
            self.kill_ring = killed;
            self.undo.push_checkpoint(&self.text, self.cursor);
            self.text.drain(start..self.cursor);
            self.cursor = start;
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else {
            false
        }
    }

    /// Delete to line end (`Ctrl+K`).
    pub fn delete_to_line_end(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }

        let end = self.line_end_offset(self.cursor);
        if self.cursor < end {
            let killed = self.text[self.cursor..end].to_string();
            self.kill_ring = killed;
            self.undo.push_checkpoint(&self.text, self.cursor);
            self.text.drain(self.cursor..end);
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else if self.cursor == end && end < self.text.len() {
            // Delete the newline character itself
            let next = self.next_grapheme_offset(self.cursor);
            self.kill_ring = "\n".to_string();
            self.undo.push_checkpoint(&self.text, self.cursor);
            self.text.drain(self.cursor..next);
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
            true
        } else {
            false
        }
    }

    /// Emacs yank (`Ctrl+Y`): insert kill-ring text.
    pub fn yank(&mut self) {
        if !self.kill_ring.is_empty() {
            let text = self.kill_ring.clone();
            self.insert(&text);
        }
    }

    /// Clear editor buffer (`Ctrl+C`).
    /// Preserves cleared text for undo (`Cmd+Z`) and kill ring.
    pub fn clear(&mut self) {
        if !self.text.is_empty() {
            self.undo.push_checkpoint(&self.text, self.cursor);
            self.kill_ring = self.text.clone();
            self.text.clear();
            self.cursor = 0;
            self.anchor = None;
            self.tokens.clear();
            self.suggestion = None;
            self.menu = None;
            self.history_nav = None;
            self.preferred_col = None;
        }
    }

    /// Undo last edit (`Cmd+Z`).
    pub fn undo(&mut self) -> bool {
        let changed = self.undo.undo(&mut self.text, &mut self.cursor);
        if changed {
            self.anchor = None;
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
        }
        changed
    }

    /// Redo last edit (`Cmd+Shift+Z`).
    pub fn redo(&mut self) -> bool {
        let changed = self.undo.redo(&mut self.text, &mut self.cursor);
        if changed {
            self.anchor = None;
            self.preferred_col = None;
            self.suggestion = None;
            self.retokenize();
        }
        changed
    }

    // --- Navigation ---

    fn update_selection(&mut self, select: bool, prev_cursor: usize) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(prev_cursor);
            }
        } else {
            self.anchor = None;
        }
    }

    /// Move left by one grapheme cluster.
    pub fn move_left(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = self.prev_grapheme_offset(self.cursor);
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move right by one grapheme cluster.
    pub fn move_right(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = self.next_grapheme_offset(self.cursor);
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move word left (`⌥←`).
    pub fn move_word_left(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = self.find_word_backward_offset(self.cursor);
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move word right (`⌥→`).
    pub fn move_word_right(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = self.find_word_forward_offset(self.cursor);
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move to line start (`⌘←` / `Ctrl+A`).
    pub fn move_to_line_start(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = self.line_start_offset(self.cursor);
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move to line end (`⌘→` / `Ctrl+E`).
    pub fn move_to_line_end(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = self.line_end_offset(self.cursor);
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move to buffer start (`⌘↑`).
    pub fn move_to_buffer_start(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = 0;
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move to buffer end (`⌘↓`).
    pub fn move_to_buffer_end(&mut self, select: bool) {
        let prev = self.cursor;
        self.cursor = self.text.len();
        self.update_selection(select, prev);
        self.preferred_col = None;
    }

    /// Move up one line in multi-line commands.
    /// Returns false if cursor was already on the first line (so caller can trigger history nav).
    pub fn move_up(&mut self, select: bool) -> bool {
        let cur_line_start = self.line_start_offset(self.cursor);
        if cur_line_start == 0 {
            return false;
        }

        let prev_line_end = cur_line_start - 1;
        let prev_line_start = self.line_start_offset(prev_line_end);

        let cur_col = self.cursor - cur_line_start;
        let target_col = self.preferred_col.unwrap_or(cur_col as u16) as usize;
        self.preferred_col = Some(target_col as u16);

        let prev_cursor = self.cursor;
        let new_offset = (prev_line_start + target_col).min(prev_line_end);
        self.cursor = self.clamp_to_grapheme(new_offset);
        self.update_selection(select, prev_cursor);
        true
    }

    /// Move down one line in multi-line commands.
    /// Returns false if cursor was already on the last line (so caller can trigger history nav).
    pub fn move_down(&mut self, select: bool) -> bool {
        let cur_line_end = self.line_end_offset(self.cursor);
        if cur_line_end == self.text.len() {
            return false;
        }

        let next_line_start = cur_line_end + 1;
        let next_line_end = self.line_end_offset(next_line_start);

        let cur_line_start = self.line_start_offset(self.cursor);
        let cur_col = self.cursor - cur_line_start;
        let target_col = self.preferred_col.unwrap_or(cur_col as u16) as usize;
        self.preferred_col = Some(target_col as u16);

        let prev_cursor = self.cursor;
        let new_offset = (next_line_start + target_col).min(next_line_end);
        self.cursor = self.clamp_to_grapheme(new_offset);
        self.update_selection(select, prev_cursor);
        true
    }

    // --- Mouse Actions ---

    /// Mouse single click: set cursor at nearest grapheme boundary.
    pub fn mouse_click(&mut self, byte_offset: usize) {
        self.cursor = self.clamp_to_grapheme(byte_offset);
        self.anchor = None;
        self.preferred_col = None;
    }

    /// Mouse drag: extend or create selection.
    pub fn mouse_drag(&mut self, byte_offset: usize) {
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = self.clamp_to_grapheme(byte_offset);
        self.preferred_col = None;
    }

    /// Mouse double click: select word at byte_offset.
    pub fn mouse_double_click(&mut self, byte_offset: usize) {
        let offset = self.clamp_to_grapheme(byte_offset);
        let start = self.find_word_backward_offset(offset);
        let end = self.find_word_forward_offset(offset);
        self.anchor = Some(start);
        self.cursor = end;
        self.preferred_col = None;
    }

    /// Mouse triple click: select entire line at byte_offset.
    pub fn mouse_triple_click(&mut self, byte_offset: usize) {
        let offset = self.clamp_to_grapheme(byte_offset);
        let start = self.line_start_offset(offset);
        let end = self.line_end_offset(offset);
        self.anchor = Some(start);
        self.cursor = end;
        self.preferred_col = None;
    }

    // --- Suggestions & Completions ---

    /// Accept the entire suggestion ghost text (`→` or `Ctrl+F` at end of buffer).
    pub fn accept_suggestion(&mut self) -> bool {
        if self.cursor == self.text.len() {
            if let Some(sug) = self.suggestion.take() {
                self.insert(&sug.text);
                return true;
            }
        }
        false
    }

    /// Accept the next word from the suggestion (`Opt+→` at end of buffer).
    pub fn accept_suggestion_word(&mut self) -> bool {
        if self.cursor == self.text.len() {
            if let Some(mut sug) = self.suggestion.take() {
                let text = &sug.text;
                if text.is_empty() {
                    return false;
                }
                // Take leading whitespace if any, then first word + any trailing space
                let mut word_end = 0;
                let mut seen_non_ws = false;
                let mut in_trailing_ws = false;
                for (i, c) in text.char_indices() {
                    if c.is_whitespace() {
                        if seen_non_ws {
                            in_trailing_ws = true;
                            word_end = i + c.len_utf8();
                        }
                    } else if in_trailing_ws {
                        break;
                    } else {
                        seen_non_ws = true;
                        word_end = i + c.len_utf8();
                    }
                }

                let word = text[..word_end].to_string();
                let remaining = text[word_end..].to_string();

                self.insert(&word);

                if !remaining.is_empty() {
                    sug.text = remaining;
                    self.suggestion = Some(sug);
                }
                return true;
            }
        }
        false
    }

    /// Clear suggestion ghost text.
    pub fn clear_suggestion(&mut self) {
        self.suggestion = None;
    }

    /// Handle Escape key: closes completion menu -> clears suggestion.
    pub fn handle_escape(&mut self) -> bool {
        if self.menu.is_some() {
            self.menu = None;
            true
        } else if self.suggestion.is_some() {
            self.suggestion = None;
            true
        } else {
            false
        }
    }

    /// Submit current command text.
    ///
    /// Trims trailing whitespace-only lines.
    /// Returns the command string (empty if input is whitespace/empty).
    pub fn submit(&mut self) -> String {
        let trimmed = trim_trailing_blank_lines(&self.text);
        let result = trimmed.to_string();

        self.text.clear();
        self.cursor = 0;
        self.anchor = None;
        self.tokens.clear();
        self.suggestion = None;
        self.menu = None;
        self.history_nav = None;
        self.preferred_col = None;

        result
    }

    // --- Boundary Helpers ---

    fn clamp_cursor(&mut self) {
        self.cursor = self.clamp_to_grapheme(self.cursor);
    }

    fn clamp_to_grapheme(&self, offset: usize) -> usize {
        if offset == 0 || self.text.is_empty() {
            return 0;
        }
        if offset >= self.text.len() {
            return self.text.len();
        }
        let mut prev = 0;
        for (start, _) in self.text.grapheme_indices(true) {
            if start == offset {
                return offset;
            }
            if start > offset {
                if offset - prev <= start - offset {
                    return prev;
                } else {
                    return start;
                }
            }
            prev = start;
        }
        prev
    }

    fn prev_grapheme_offset(&self, offset: usize) -> usize {
        if offset == 0 {
            return 0;
        }
        let mut prev = 0;
        for (start, _) in self.text.grapheme_indices(true) {
            if start >= offset {
                return prev;
            }
            prev = start;
        }
        prev
    }

    fn next_grapheme_offset(&self, offset: usize) -> usize {
        if offset >= self.text.len() {
            return self.text.len();
        }
        for (start, _) in self.text.grapheme_indices(true) {
            if start > offset {
                return start;
            }
        }
        self.text.len()
    }

    fn line_start_offset(&self, offset: usize) -> usize {
        let safe = offset.min(self.text.len());
        match self.text[..safe].rfind('\n') {
            Some(nl) => nl + 1,
            None => 0,
        }
    }

    fn line_end_offset(&self, offset: usize) -> usize {
        let safe = offset.min(self.text.len());
        match self.text[safe..].find('\n') {
            Some(nl) => safe + nl,
            None => self.text.len(),
        }
    }

    fn find_word_backward_offset(&self, offset: usize) -> usize {
        let slice = &self.text[..offset.min(self.text.len())];
        if slice.is_empty() {
            return 0;
        }

        let mut chars: Vec<(usize, char)> = slice.char_indices().collect();
        // Skip trailing whitespace backwards
        while let Some(&(_, c)) = chars.last() {
            if c.is_whitespace() {
                chars.pop();
            } else {
                break;
            }
        }

        if chars.is_empty() {
            return 0;
        }

        let &(_, last_c) = chars.last().unwrap();
        let is_word_char = last_c.is_alphanumeric() || last_c == '_' || last_c == '-';

        while let Some(&(_, c)) = chars.last() {
            let cur_word = c.is_alphanumeric() || c == '_' || c == '-';
            if cur_word == is_word_char && !c.is_whitespace() {
                chars.pop();
            } else {
                break;
            }
        }

        chars.last().map_or(0, |&(idx, c)| idx + c.len_utf8())
    }

    fn find_word_forward_offset(&self, offset: usize) -> usize {
        let safe = offset.min(self.text.len());
        let slice = &self.text[safe..];
        if slice.is_empty() {
            return self.text.len();
        }

        let mut iter = slice.char_indices().peekable();
        // Skip leading whitespace
        while let Some(&(_, c)) = iter.peek() {
            if c.is_whitespace() {
                iter.next();
            } else {
                break;
            }
        }

        if let Some(&(_, first_c)) = iter.peek() {
            let is_word_char = first_c.is_alphanumeric() || first_c == '_' || first_c == '-';
            while let Some(&(i, c)) = iter.peek() {
                let cur_word = c.is_alphanumeric() || c == '_' || c == '-';
                if cur_word == is_word_char && !c.is_whitespace() {
                    iter.next();
                } else {
                    return safe + i;
                }
            }
        }

        self.text.len()
    }
}

/// Trim trailing blank lines (lines containing only whitespace) from the command text.
fn trim_trailing_blank_lines(text: &str) -> &str {
    let mut end = text.len();
    while end > 0 {
        let line_start = match text[..end].rfind('\n') {
            Some(nl) => nl + 1,
            None => 0,
        };
        let line = &text[line_start..end];
        if line.chars().all(|c| c.is_whitespace()) {
            if line_start > 0 {
                end = line_start - 1; // strip the \n as well
            } else {
                return "";
            }
        } else {
            break;
        }
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_editor_navigation_and_selection() {
        let mut ed = EditorState::new();
        ed.insert("cargo test --workspace");
        assert_eq!(ed.cursor, 22);

        // Word left
        ed.move_word_left(false);
        assert_eq!(ed.cursor, 11); // at '--workspace'

        // Word left with selection
        ed.move_word_left(true);
        assert_eq!(ed.cursor, 6); // at 'test'
        assert_eq!(ed.selected_text(), Some("test "));

        // Replace selection
        ed.insert("build ");
        assert_eq!(ed.text, "cargo build --workspace");
        assert!(!ed.has_selection());
    }

    #[test]
    fn test_editor_delete_and_emacs() {
        let mut ed = EditorState::new();
        ed.insert("git commit -m 'initial'");

        // Line start (Ctrl+A)
        ed.move_to_line_start(false);
        assert_eq!(ed.cursor, 0);

        // Delete forward (Ctrl+D)
        ed.delete_forward();
        assert_eq!(ed.text, "it commit -m 'initial'");

        // Undo
        ed.undo();
        assert_eq!(ed.text, "git commit -m 'initial'");

        // Kill to line end (Ctrl+K)
        ed.move_word_right(false); // after git
        ed.delete_to_line_end();
        assert_eq!(ed.text, "git");
        assert_eq!(ed.kill_ring, " commit -m 'initial'");

        // Yank (Ctrl+Y)
        ed.yank();
        assert_eq!(ed.text, "git commit -m 'initial'");
    }

    #[test]
    fn test_editor_multiline_and_submit() {
        let mut ed = EditorState::new();
        ed.insert("echo hello\n  \n");
        let res = ed.submit();
        assert_eq!(res, "echo hello");
        assert_eq!(ed.text, "");
        assert_eq!(ed.cursor, 0);
    }

    #[test]
    fn test_editor_suggestion_acceptance() {
        let mut ed = EditorState::new();
        ed.insert("car");
        ed.suggestion = Some(Suggestion {
            text: "go build".to_string(),
            full_match: "cargo build".to_string(),
        });

        // Accept word
        ed.accept_suggestion_word();
        assert_eq!(ed.text, "cargo ");
        assert_eq!(ed.suggestion.as_ref().unwrap().text, "build");

        // Accept rest
        ed.accept_suggestion();
        assert_eq!(ed.text, "cargo build");
        assert!(ed.suggestion.is_none());
    }
}
