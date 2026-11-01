//! Reflow on resize for primary and alternate screens.
//!
//! See `docs/specs/terminal-core.md §7`.

use rustc_hash::FxHashMap;

use crate::block::BlockList;
use crate::cell::{AbsLine, Cell, CellFlags};
use crate::grid::Grid;
use crate::row::{Row, RowFlags, RowPool};
use crate::scrollback::Scrollback;

/// Result of a reflow operation.
pub struct ReflowResult {
    pub new_cursor_row: u16,
    pub new_cursor_col: u16,
}

/// Reflow primary screen grid and hot scrollback rows to `new_cols` and `new_rows`.
pub fn reflow_primary(
    grid: &mut Grid,
    scrollback: &mut Scrollback,
    blocks: &mut BlockList,
    new_cols: u16,
    new_rows: u16,
    pool: &mut RowPool,
) -> ReflowResult {
    let old_cols = grid.cols;
    let old_rows = grid.num_rows;

    if old_cols == new_cols && old_rows == new_rows {
        return ReflowResult {
            new_cursor_row: grid.cursor_row,
            new_cursor_col: grid.cursor_col,
        };
    }

    // Collect all rows: scrollback hot ring followed by grid screen rows
    let mut all_rows = Vec::with_capacity(scrollback.hot_len() + grid.rows.len());
    let hot_len = scrollback.hot_len();
    for row in scrollback.hot_ring.drain(..) {
        all_rows.push(row);
    }
    for row in grid.rows.drain(..) {
        all_rows.push(row);
    }

    // Logical lines: each logical line is a sequence of cells (without trailing blanks)
    // plus whether cursor is on it and at what character offset.
    struct LogicalLine {
        cells: Vec<Cell>,
        old_row_start: usize,
        old_row_count: usize,
        has_cursor: bool,
        cursor_char_offset: usize,
    }

    let mut logical_lines: Vec<LogicalLine> = Vec::new();
    let mut current_cells: Vec<Cell> = Vec::new();
    let mut line_old_start = 0;
    let mut line_old_count = 0;
    let mut line_has_cursor = false;
    let mut line_cursor_offset = 0;

    let cursor_global_row = hot_len + grid.cursor_row as usize;
    let cursor_col = grid.cursor_col as usize;

    for (row_idx, row) in all_rows.iter().enumerate() {
        if line_old_count == 0 {
            line_old_start = row_idx;
        }
        line_old_count += 1;

        if row_idx == cursor_global_row {
            line_has_cursor = true;
            line_cursor_offset = current_cells.len() + cursor_col.min(row.len as usize);
        }

        // Add non-trailing-blank cells of this row
        let content = row.content_cells();
        current_cells.extend_from_slice(content);

        let is_wrapped = row.flags.contains(RowFlags::WRAPPED);
        if !is_wrapped {
            // End of logical line
            logical_lines.push(LogicalLine {
                cells: std::mem::take(&mut current_cells),
                old_row_start: line_old_start,
                old_row_count: line_old_count,
                has_cursor: line_has_cursor,
                cursor_char_offset: line_cursor_offset,
            });
            line_old_count = 0;
            line_has_cursor = false;
            line_cursor_offset = 0;
        }
    }

    if line_old_count > 0 || !current_cells.is_empty() {
        logical_lines.push(LogicalLine {
            cells: current_cells,
            old_row_start: line_old_start,
            old_row_count: line_old_count,
            has_cursor: line_has_cursor,
            cursor_char_offset: line_cursor_offset,
        });
    }

    // Rewrap logical lines to `new_cols`
    let mut new_rows_list: Vec<Row> = Vec::new();
    let mut new_cursor_pos = (0u16, 0u16);
    let mut line_remap: FxHashMap<u64, u64> = FxHashMap::default();

    let base_line = scrollback.hot_base_line;

    for line in logical_lines {
        let new_start_row = new_rows_list.len();

        let mut row = pool.alloc(new_cols, 0);
        let mut col: u16 = 0;

        let mut cell_idx = 0;
        while cell_idx < line.cells.len() {
            let cell = line.cells[cell_idx];

            if line.has_cursor && cell_idx == line.cursor_char_offset {
                new_cursor_pos = (new_rows_list.len() as u16, col);
            }

            if cell.flags.contains(CellFlags::WIDE) {
                // Check if wide character fits on current row
                if col + 1 >= new_cols {
                    // Wrap to next row
                    row.flags.insert(RowFlags::WRAPPED);
                    row.update_len();
                    new_rows_list.push(row);

                    row = pool.alloc(new_cols, 0);
                    col = 0;
                }

                let c1 = row.get_mut(col);
                *c1 = cell;
                col += 1;

                // Wide spacer
                if col < new_cols {
                    let spacer = if cell_idx + 1 < line.cells.len() {
                        line.cells[cell_idx + 1]
                    } else {
                        Cell {
                            ch: ' ' as u32,
                            style: cell.style,
                            flags: CellFlags::WIDE_SPACER,
                        }
                    };
                    let c2 = row.get_mut(col);
                    *c2 = spacer;
                    col += 1;
                }
                cell_idx += 2;
            } else {
                if col >= new_cols {
                    row.flags.insert(RowFlags::WRAPPED);
                    row.update_len();
                    new_rows_list.push(row);

                    row = pool.alloc(new_cols, 0);
                    col = 0;
                }

                let c = row.get_mut(col);
                *c = cell;
                col += 1;
                cell_idx += 1;
            }
        }

        if line.has_cursor && cell_idx == line.cursor_char_offset {
            new_cursor_pos = (new_rows_list.len() as u16, col);
        }

        row.update_len();
        new_rows_list.push(row);

        // Record old row -> new row mapping for block AbsLines
        for offset in 0..line.old_row_count {
            let old_abs = base_line + (line.old_row_start + offset) as u64;
            let new_abs = base_line + new_start_row as u64;
            line_remap.insert(old_abs, new_abs);
        }
    }

    // Remap block line numbers
    blocks.remap(|abs| {
        if let Some(&new_abs) = line_remap.get(&abs.0) {
            AbsLine(new_abs)
        } else {
            abs
        }
    });

    // Split `new_rows_list` into hot scrollback and screen rows
    let target_screen_rows = new_rows as usize;
    let total_new_rows = new_rows_list.len();

    let screen_split = total_new_rows.saturating_sub(target_screen_rows);

    let mut screen_rows_vec: Vec<Row> = new_rows_list.split_off(screen_split);
    // Pad screen rows up to new_rows if fewer
    while screen_rows_vec.len() < target_screen_rows {
        screen_rows_vec.push(pool.alloc(new_cols, 0));
    }

    scrollback.hot_ring = new_rows_list.into();
    grid.rows = screen_rows_vec;
    grid.cols = new_cols;
    grid.num_rows = new_rows;
    grid.top_margin = 0;
    grid.bottom_margin = new_rows.saturating_sub(1);
    grid.left_margin = 0;
    grid.right_margin = new_cols.saturating_sub(1);

    // Compute final cursor row and col within screen
    let cursor_global = new_cursor_pos.0 as usize;
    let final_cursor_row = if cursor_global >= screen_split {
        (cursor_global - screen_split) as u16
    } else {
        0
    };
    let final_cursor_col = new_cursor_pos.1.min(new_cols.saturating_sub(1));

    grid.cursor_row = final_cursor_row.min(new_rows.saturating_sub(1));
    grid.cursor_col = final_cursor_col;
    grid.pending_wrap = false;

    ReflowResult {
        new_cursor_row: grid.cursor_row,
        new_cursor_col: grid.cursor_col,
    }
}

/// Truncate or pad alternate screen grid on resize without reflowing text.
pub fn resize_alt_screen(grid: &mut Grid, new_cols: u16, new_rows: u16, pool: &mut RowPool) {
    let old_cols = grid.cols;
    let old_rows = grid.num_rows;

    if old_cols == new_cols && old_rows == new_rows {
        return;
    }

    let mut new_rows_vec = Vec::with_capacity(new_rows as usize);
    for row in grid.rows.drain(..) {
        if new_rows_vec.len() < new_rows as usize {
            if row.cols() == new_cols {
                new_rows_vec.push(row);
            } else {
                let mut new_row = pool.alloc(new_cols, 0);
                let copy_len = (row.cols() as usize).min(new_cols as usize);
                new_row.cells[..copy_len].copy_from_slice(&row.cells[..copy_len]);
                new_row.update_len();
                pool.recycle(row);
                new_rows_vec.push(new_row);
            }
        } else {
            pool.recycle(row);
        }
    }

    while new_rows_vec.len() < new_rows as usize {
        new_rows_vec.push(pool.alloc(new_cols, 0));
    }

    grid.rows = new_rows_vec;
    grid.cols = new_cols;
    grid.num_rows = new_rows;
    grid.top_margin = 0;
    grid.bottom_margin = new_rows.saturating_sub(1);
    grid.left_margin = 0;
    grid.right_margin = new_cols.saturating_sub(1);
    grid.cursor_row = grid.cursor_row.min(new_rows.saturating_sub(1));
    grid.cursor_col = grid.cursor_col.min(new_cols.saturating_sub(1));
    grid.pending_wrap = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reflow_rewraps_and_preserves_text() {
        let mut grid = Grid::new(4, 2);
        let mut pool = RowPool::new(4);
        let mut sb = Scrollback::new(4, 2, 100);
        let mut blocks = BlockList::new();
        let mut graphemes = crate::style::GraphemeTable::new();

        grid.print_char(
            'a',
            0,
            crate::modes::Modes::AUTOWRAP,
            &mut pool,
            &mut graphemes,
        );
        grid.print_char(
            'b',
            0,
            crate::modes::Modes::AUTOWRAP,
            &mut pool,
            &mut graphemes,
        );
        grid.print_char(
            'c',
            0,
            crate::modes::Modes::AUTOWRAP,
            &mut pool,
            &mut graphemes,
        );
        grid.print_char(
            'd',
            0,
            crate::modes::Modes::AUTOWRAP,
            &mut pool,
            &mut graphemes,
        );
        grid.print_char(
            'e',
            0,
            crate::modes::Modes::AUTOWRAP,
            &mut pool,
            &mut graphemes,
        );

        // Grid had 4 cols, printed "abcde" -> row 0: "abcd", row 1: "e"
        assert_eq!(grid.rows[0].get(0).ch, 'a' as u32);
        assert_eq!(grid.rows[1].get(0).ch, 'e' as u32);

        // Reflow to width 8
        let mut pool8 = RowPool::new(8);
        reflow_primary(&mut grid, &mut sb, &mut blocks, 8, 2, &mut pool8);

        // In 8 cols, "abcde" should fit in a single row
        assert_eq!(grid.rows[0].get(0).ch, 'a' as u32);
        assert_eq!(grid.rows[0].get(4).ch, 'e' as u32);
    }
}
