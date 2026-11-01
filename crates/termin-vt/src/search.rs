//! Substring search across hot and cold scrollback tiers.
//!
//! See `docs/specs/terminal-core.md §9`.

use memchr::memmem::Finder;

use crate::cell::{AbsLine, AbsPos, CellFlags};
use crate::grid::Grid;
use crate::scrollback::Scrollback;
use crate::style::GraphemeTable;

/// Substring or pattern search query.
///
/// See `docs/specs/terminal-core.md §9`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// Search match location.
///
/// See `docs/specs/terminal-core.md §9`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub start: AbsPos,
    pub end: AbsPos,
}

/// Progress returned by incremental search.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SearchProgress {
    Done,
    Found(Match),
    InProgress(AbsPos),
}

/// Search backward from `from` pos towards older history.
pub fn search_incremental(
    query: &SearchQuery,
    cursor: Option<AbsPos>,
    budget_lines: usize,
    grid: &Grid,
    scrollback: &Scrollback,
    graphemes: &GraphemeTable,
) -> SearchProgress {
    if query.text.is_empty() {
        return SearchProgress::Done;
    }

    let hot_base = scrollback.hot_base_line;
    let screen_base = hot_base + scrollback.hot_len() as u64;
    let total_lines = screen_base + grid.rows.len() as u64;

    let start_pos = cursor.unwrap_or(AbsPos {
        line: AbsLine(total_lines.saturating_sub(1)),
        col: grid.cols,
    });

    let mut current_line = start_pos.line.0;
    let mut lines_scanned = 0;

    let needle = if query.case_sensitive {
        query.text.clone()
    } else {
        query.text.to_lowercase()
    };
    let finder = Finder::new(needle.as_bytes());

    let mut row_buf = String::with_capacity(256);

    while lines_scanned < budget_lines {
        if current_line >= total_lines {
            if current_line == 0 {
                return SearchProgress::Done;
            }
            current_line -= 1;
            continue;
        }

        row_buf.clear();

        // Extract row text
        let row_opt = if current_line >= screen_base {
            let idx = (current_line - screen_base) as usize;
            grid.rows.get(idx)
        } else if current_line >= hot_base {
            let idx = (current_line - hot_base) as usize;
            scrollback.get_hot(idx)
        } else {
            None
        };

        if let Some(row) = row_opt {
            for cell in row.content_cells() {
                if cell.flags.contains(CellFlags::WIDE_SPACER) {
                    continue;
                }
                if let Some(g) = cell.grapheme_index().and_then(|i| graphemes.get(i)) {
                    row_buf.push_str(g);
                } else if let Some(c) = char::from_u32(cell.ch) {
                    row_buf.push(c);
                }
            }
        } else {
            // Cold tier chunk check
            for chunk in &scrollback.cold_tier.chunks {
                let chunk_end = chunk.first_line + chunk.line_count as u64;
                if current_line >= chunk.first_line && current_line < chunk_end {
                    let idx = (current_line - chunk.first_line) as u16;
                    if let Some(line) = chunk.get_line(idx) {
                        row_buf.push_str(line);
                    }
                    break;
                }
            }
        }

        let hay = if query.case_sensitive {
            row_buf.clone()
        } else {
            row_buf.to_lowercase()
        };

        // Scan row for needle
        if let Some(byte_idx) = finder.find(hay.as_bytes()) {
            // Calculate column offset from byte offset
            let char_offset = hay[..byte_idx].chars().count() as u16;
            let match_len_chars = needle.chars().count() as u16;

            let m = Match {
                start: AbsPos {
                    line: AbsLine(current_line),
                    col: char_offset,
                },
                end: AbsPos {
                    line: AbsLine(current_line),
                    col: char_offset + match_len_chars,
                },
            };
            return SearchProgress::Found(m);
        }

        lines_scanned += 1;
        if current_line == 0 {
            return SearchProgress::Done;
        }
        current_line -= 1;
    }

    SearchProgress::InProgress(AbsPos {
        line: AbsLine(current_line),
        col: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::row::RowPool;

    #[test]
    fn test_search_finds_needle() {
        let mut grid = Grid::new(80, 24);
        let mut pool = RowPool::new(80);
        let sb = Scrollback::new(80, 24, 1000);
        let mut graphemes = GraphemeTable::new();

        for c in "Hello world".chars() {
            grid.print_char(
                c,
                0,
                crate::modes::Modes::AUTOWRAP,
                &mut pool,
                &mut graphemes,
            );
        }

        let query = SearchQuery {
            text: "world".into(),
            case_sensitive: true,
            whole_word: false,
        };

        let res = search_incremental(&query, None, 100, &grid, &sb, &graphemes);
        if let SearchProgress::Found(m) = res {
            assert_eq!(m.start.col, 6);
            assert_eq!(m.end.col, 11);
        } else {
            panic!("expected found match");
        }
    }
}
