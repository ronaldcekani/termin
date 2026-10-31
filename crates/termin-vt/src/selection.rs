//! Selection and text extraction across terminal lines.
//!
//! See `docs/specs/terminal-core.md §8`.

use termin_core::BlockId;

use crate::block::BlockList;
use crate::cell::{AbsLine, AbsPos, CellFlags};
use crate::grid::Grid;
use crate::row::RowFlags;
use crate::scrollback::Scrollback;
use crate::style::GraphemeTable;

/// Default word separator characters for word selection.
pub const DEFAULT_WORD_SEPARATORS: &str = " ,│`|:\"' ()[]{}<>\t";

/// Kind of selection.
///
/// See `docs/specs/terminal-core.md §8`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    Simple,
    Word,
    Line,
    Block(BlockId),
    Rect,
}

/// Selection across terminal lines.
///
/// See `docs/specs/terminal-core.md §8`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub kind: SelectionKind,
    pub anchor: AbsPos,
    pub head: AbsPos,
}

/// Extract text between two absolute lines `[start, end)`.
///
/// Joins soft-wrapped rows without newline and trims trailing spaces per logical line.
pub fn extract_text_range(
    start: AbsLine,
    end: AbsLine,
    grid: &Grid,
    scrollback: &Scrollback,
    graphemes: &GraphemeTable,
) -> String {
    let mut out = String::new();
    let mut line_buf = String::new();

    let start_val = start.0;
    let end_val = end.0;
    if start_val >= end_val {
        return out;
    }

    let hot_base = scrollback.hot_base_line;
    let screen_base = hot_base + scrollback.hot_len() as u64;

    for abs in start_val..end_val {
        let (row_opt, is_wrapped) = if abs >= screen_base {
            let idx = (abs - screen_base) as usize;
            if idx < grid.rows.len() {
                let r = &grid.rows[idx];
                (Some(r), r.flags.contains(RowFlags::WRAPPED))
            } else {
                (None, false)
            }
        } else if abs >= hot_base {
            let idx = (abs - hot_base) as usize;
            if let Some(r) = scrollback.get_hot(idx) {
                (Some(r), r.flags.contains(RowFlags::WRAPPED))
            } else {
                (None, false)
            }
        } else {
            // Cold tier
            (None, false)
        };

        if let Some(row) = row_opt {
            for cell in row.content_cells() {
                if cell.flags.contains(CellFlags::WIDE_SPACER) {
                    continue;
                }
                if let Some(g) = cell.grapheme_index().and_then(|i| graphemes.get(i)) {
                    line_buf.push_str(g);
                } else if let Some(c) = char::from_u32(cell.ch) {
                    line_buf.push(c);
                }
            }

            if !is_wrapped {
                // Trim trailing spaces and append newline
                let trimmed = line_buf.trim_end_matches(' ');
                out.push_str(trimmed);
                out.push('\n');
                line_buf.clear();
            }
        } else {
            // Check cold tier
            for chunk in &scrollback.cold_tier.chunks {
                let chunk_end = chunk.first_line + chunk.line_count as u64;
                if abs >= chunk.first_line && abs < chunk_end {
                    let line_idx = (abs - chunk.first_line) as u16;
                    if let Some(line) = chunk.get_line(line_idx) {
                        out.push_str(line.trim_end_matches(' '));
                        out.push('\n');
                    }
                    break;
                }
            }
        }
    }

    if !line_buf.is_empty() {
        out.push_str(line_buf.trim_end_matches(' '));
        out.push('\n');
    }

    out
}

/// Extract command text for a block.
pub fn extract_block_command(
    id: BlockId,
    blocks: &BlockList,
    grid: &Grid,
    scrollback: &Scrollback,
    graphemes: &GraphemeTable,
) -> Option<String> {
    let block = blocks.get(id)?;
    if let Some(cmd) = &block.command {
        return Some(cmd.to_string());
    }

    let start = block.input_start.unwrap_or(block.prompt_start);
    let end = block.output_start.or(block.end)?;
    let text = extract_text_range(start, end, grid, scrollback, graphemes);
    Some(text.trim().to_string())
}

/// Extract output text for a block.
pub fn extract_block_output(
    id: BlockId,
    blocks: &BlockList,
    grid: &Grid,
    scrollback: &Scrollback,
    graphemes: &GraphemeTable,
) -> Option<String> {
    let block = blocks.get(id)?;
    let start = block.output_start?;
    let end = block.end?;
    let text = extract_text_range(start, end, grid, scrollback, graphemes);
    Some(text)
}
