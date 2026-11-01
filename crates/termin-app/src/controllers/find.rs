//! Incremental find state and search stepping (Phase 3C).
use crate::{action::Action, model::AppModel};
use termin_vt::{AbsLine, AbsPos, Match, SearchProgress, Terminal};

/// Bound each search to the newest 100k absolute rows. This covers the v1
/// scrollback search budget and prevents work from growing with session age.
const MAX_SEARCH_LINES: u64 = 100_000;

#[derive(Clone, Debug, Default)]
pub struct FindState {
    pub query: String,
    pub case_sensitive: bool,
    pub open: bool,
    pub current: Option<Match>,
    /// Match offsets are currently character offsets from `text_range`, not
    /// terminal-cell columns. Do not use them for rendering highlights or selection.
    pub matches: Vec<Match>,
    pub selected_match: usize,
    scan_line: u64,
    start_line: u64,
    total_lines: u64,
    pub searched_lines: usize,
    pub done: bool,
}
impl FindState {
    pub fn open(&mut self) {
        self.open = true;
        self.reset_scan();
    }
    pub fn close(&mut self) {
        self.open = false;
    }
    pub fn set_query(&mut self, query: impl Into<String>, case_sensitive: bool) {
        self.query = query.into();
        self.case_sensitive = case_sensitive;
        self.reset_scan();
    }
    fn reset_scan(&mut self) {
        self.current = None;
        self.matches.clear();
        self.scan_line = 0;
        self.start_line = 0;
        self.total_lines = 0;
        self.selected_match = 0;
        self.searched_lines = 0;
        self.done = false;
    }
    /// Search at most `line_budget` rows; call again next frame on `InProgress`.
    /// This forward scan supports next/previous because VT's current API only searches backward.
    pub fn step(&mut self, terminal: &Terminal, line_budget: usize) -> SearchProgress {
        if self.query.is_empty() {
            self.reset_scan();
            self.done = true;
            return SearchProgress::Done;
        }
        if self.total_lines == 0 {
            self.total_lines = terminal.total_lines();
            self.start_line = self.total_lines.saturating_sub(MAX_SEARCH_LINES);
            self.scan_line = self.start_line;
        }
        let needle: Vec<char> = if self.case_sensitive {
            self.query.chars().collect()
        } else {
            self.query.chars().flat_map(char::to_lowercase).collect()
        };
        let end = self
            .scan_line
            .saturating_add(line_budget.max(1) as u64)
            .min(self.total_lines);
        while self.scan_line < end {
            let line = self.scan_line;
            let text = terminal.text_range(termin_vt::LineRange {
                start: AbsLine(line),
                end: AbsLine(line + 1),
            });
            let chars: Vec<char> = text.trim_end_matches('\n').chars().collect();
            let mut folded: Vec<char> = Vec::with_capacity(chars.len());
            let mut folded_to_char: Vec<usize> = Vec::with_capacity(chars.len());
            for (char_idx, ch) in chars.iter().copied().enumerate() {
                if self.case_sensitive {
                    folded.push(ch);
                    folded_to_char.push(char_idx);
                } else {
                    for lower in ch.to_lowercase() {
                        folded.push(lower);
                        folded_to_char.push(char_idx);
                    }
                }
            }
            if !needle.is_empty() && needle.len() <= folded.len() {
                for idx in 0..=folded.len() - needle.len() {
                    if folded[idx..idx + needle.len()] == needle {
                        let start = folded_to_char[idx];
                        let end = folded_to_char[idx + needle.len() - 1] + 1;
                        // VT positions are terminal columns. `text_range` loses
                        // cell-width metadata, so these character offsets are
                        // intentionally not advertised as usable highlight positions.
                        self.matches.push(Match {
                            start: AbsPos::new(line, start.min(u16::MAX as usize) as u16),
                            end: AbsPos::new(line, end.min(u16::MAX as usize) as u16),
                        });
                    }
                }
            }
            self.scan_line += 1;
        }
        self.searched_lines = self
            .scan_line
            .saturating_sub(self.start_line)
            .min(usize::MAX as u64) as usize;
        if self.scan_line < self.total_lines {
            SearchProgress::InProgress(AbsPos::new(self.scan_line, 0))
        } else {
            self.done = true;
            self.current = self.matches.first().copied();
            SearchProgress::Done
        }
    }
    pub fn next(&mut self) {
        if !self.matches.is_empty() {
            self.selected_match = (self.selected_match + 1) % self.matches.len();
            self.current = self.matches.get(self.selected_match).copied();
        }
    }
    pub fn previous(&mut self) {
        if !self.matches.is_empty() {
            self.selected_match = self
                .selected_match
                .checked_sub(1)
                .unwrap_or(self.matches.len() - 1);
            self.current = self.matches.get(self.selected_match).copied();
        }
    }
}

/// Action routing needs runtime-owned FindState and is completed by app integration.
pub fn handle(_model: &mut AppModel, _action: &Action) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_vt::{Effect, EffectSink, TerminalConfig};

    #[derive(Default)]
    struct Sink;
    impl EffectSink for Sink {
        fn write_back(&mut self, _: &[u8]) {}
        fn effect(&mut self, _: Effect) {}
    }

    #[test]
    fn query_resets_scan_and_navigation_wraps() {
        let mut f = FindState::default();
        f.open();
        f.query = "x".into();
        f.searched_lines = 42;
        f.matches = vec![
            Match {
                start: AbsPos::new(1, 0),
                end: AbsPos::new(1, 1),
            },
            Match {
                start: AbsPos::new(2, 0),
                end: AbsPos::new(2, 1),
            },
        ];
        f.current = f.matches.first().copied();
        f.next();
        assert_eq!(f.current, f.matches.get(1).copied());
        f.next();
        assert_eq!(f.current, f.matches.first().copied());
        f.previous();
        assert_eq!(f.current, f.matches.get(1).copied());
        f.set_query("y", true);
        assert_eq!(f.searched_lines, 0);
        assert!(f.open);
        assert!(f.current.is_none());
    }

    #[test]
    fn scans_real_terminal_content_and_navigates_repeated_matches() {
        let mut terminal = Terminal::new(&TerminalConfig::default());
        terminal.advance(b"needle\r\nother\r\nneedle", &mut Sink);
        let mut find = FindState::default();
        find.set_query("needle", true);
        while matches!(find.step(&terminal, 2), SearchProgress::InProgress(_)) {}
        assert_eq!(find.matches.len(), 2);
        let first = find.current;
        find.next();
        assert_ne!(find.current, first);
        find.previous();
        assert_eq!(find.current, first);
    }

    #[test]
    fn case_insensitive_search_handles_non_ascii_letters() {
        let mut terminal = Terminal::new(&TerminalConfig::default());
        terminal.advance("ÄPFEL".as_bytes(), &mut Sink);
        let mut find = FindState::default();
        find.set_query("äpfel", false);
        assert_eq!(find.step(&terminal, 32), SearchProgress::Done);
        assert_eq!(find.matches.len(), 1);
    }

    #[test]
    fn search_window_is_bounded_by_absolute_session_age() {
        let mut terminal = Terminal::new(&TerminalConfig::default());
        let output = vec![b'\n'; 100_010];
        terminal.advance(&output, &mut Sink);
        let mut find = FindState::default();
        find.set_query("missing", true);
        assert!(matches!(
            find.step(&terminal, 17),
            SearchProgress::InProgress(_)
        ));
        assert_eq!(
            find.start_line,
            terminal.total_lines().saturating_sub(MAX_SEARCH_LINES)
        );
        assert_eq!(find.searched_lines, 17);
    }
}
