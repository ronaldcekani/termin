//! Screen grid management, cursor positioning, margins, and character printing.
//!
//! See `docs/specs/terminal-core.md §2` and `§4`.

use unicode_width::UnicodeWidthChar;

use crate::cell::{Cell, CellFlags, GRAPHEME_BIT};
use crate::modes::Modes;
use crate::row::{Row, RowFlags, RowPool};
use crate::style::GraphemeTable;

/// Charset designation for G0-G3.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Charset {
    #[default]
    Ascii,
    DecSpecialGraphics,
    Uk,
}

/// Saved cursor state for DECSC / DECRC.
#[derive(Clone, Debug)]
pub struct SavedCursor {
    pub row: u16,
    pub col: u16,
    pub style: u16,
    pub charset_g0: Charset,
    pub origin_mode: bool,
    pub autowrap: bool,
}

impl Default for SavedCursor {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            style: 0,
            charset_g0: Charset::Ascii,
            origin_mode: false,
            autowrap: true,
        }
    }
}

/// Active screen grid buffer (primary or alternate).
#[derive(Debug)]
pub struct Grid {
    pub rows: Vec<Row>,
    pub cols: u16,
    pub num_rows: u16,
    // Cursor coordinates (0-indexed)
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub pending_wrap: bool,
    // Scroll margins (0-indexed, inclusive)
    pub top_margin: u16,
    pub bottom_margin: u16,
    pub left_margin: u16,
    pub right_margin: u16,
    // Saved cursor state
    pub saved_cursor: SavedCursor,
    // Tab stops
    pub tabs: Vec<bool>,
    // Charset mapping
    pub g0: Charset,
    pub g1: Charset,
    pub g2: Charset,
    pub g3: Charset,
    pub gl: u8, // 0 for G0, 1 for G1
    pub single_shift: Option<u8>,
}

impl Grid {
    pub fn new(cols: u16, rows: u16) -> Self {
        let mut grid_rows = Vec::with_capacity(rows as usize);
        for _ in 0..rows {
            grid_rows.push(Row::new(cols));
        }

        let mut tabs = vec![false; cols as usize];
        for (i, tab) in tabs.iter_mut().enumerate() {
            if i % 8 == 0 && i > 0 {
                *tab = true;
            }
        }

        Self {
            rows: grid_rows,
            cols,
            num_rows: rows,
            cursor_row: 0,
            cursor_col: 0,
            pending_wrap: false,
            top_margin: 0,
            bottom_margin: rows.saturating_sub(1),
            left_margin: 0,
            right_margin: cols.saturating_sub(1),
            saved_cursor: SavedCursor::default(),
            tabs,
            g0: Charset::Ascii,
            g1: Charset::DecSpecialGraphics,
            g2: Charset::Ascii,
            g3: Charset::Ascii,
            gl: 0,
            single_shift: None,
        }
    }

    /// Reset grid dimensions and clear content.
    pub fn reset_size(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.num_rows = rows;
        self.rows.clear();
        for _ in 0..rows {
            self.rows.push(Row::new(cols));
        }
        self.top_margin = 0;
        self.bottom_margin = rows.saturating_sub(1);
        self.left_margin = 0;
        self.right_margin = cols.saturating_sub(1);
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
        self.pending_wrap = false;

        self.tabs = vec![false; cols as usize];
        for (i, tab) in self.tabs.iter_mut().enumerate() {
            if i % 8 == 0 && i > 0 {
                *tab = true;
            }
        }
    }

    /// Check if cursor addressing is in origin mode (DECOM).
    #[inline]
    pub fn is_origin(&self, modes: Modes) -> bool {
        modes.contains(Modes::ORIGIN)
    }

    /// Move cursor to absolute row and column, respecting origin mode and margins.
    pub fn set_cursor_pos(&mut self, row: u16, col: u16, modes: Modes) {
        self.pending_wrap = false;
        if self.is_origin(modes) {
            let actual_row = (self.top_margin + row).min(self.bottom_margin);
            let actual_col = (self.left_margin + col).min(self.right_margin);
            self.cursor_row = actual_row;
            self.cursor_col = actual_col;
        } else {
            self.cursor_row = row.min(self.num_rows.saturating_sub(1));
            self.cursor_col = col.min(self.cols.saturating_sub(1));
        }
    }

    /// Move cursor to absolute row, respecting origin mode and vertical margins.
    pub fn set_cursor_row(&mut self, row: u16, modes: Modes) {
        self.pending_wrap = false;
        if self.is_origin(modes) {
            self.cursor_row = (self.top_margin + row).min(self.bottom_margin);
        } else {
            self.cursor_row = row.min(self.num_rows.saturating_sub(1));
        }
    }

    /// Move cursor to absolute column, respecting origin mode and horizontal margins.
    pub fn set_cursor_col(&mut self, col: u16, modes: Modes) {
        self.pending_wrap = false;
        if self.is_origin(modes) {
            self.cursor_col = (self.left_margin + col).min(self.right_margin);
        } else {
            self.cursor_col = col.min(self.cols.saturating_sub(1));
        }
    }

    /// Move cursor relative to current position.
    pub fn move_cursor_rel(&mut self, d_row: i32, d_col: i32, modes: Modes) {
        self.pending_wrap = false;
        let (min_row, max_row) = if self.is_origin(modes) {
            (self.top_margin as i32, self.bottom_margin as i32)
        } else {
            (0, self.num_rows.saturating_sub(1) as i32)
        };

        let new_row = (self.cursor_row as i32 + d_row).clamp(min_row, max_row);
        let new_col = (self.cursor_col as i32 + d_col).clamp(0, self.cols.saturating_sub(1) as i32);

        self.cursor_row = new_row as u16;
        self.cursor_col = new_col as u16;
    }

    /// Move cursor to next tab stop.
    pub fn move_to_next_tab(&mut self) {
        self.pending_wrap = false;
        for c in (self.cursor_col + 1)..self.cols {
            if self.tabs.get(c as usize).copied().unwrap_or(false) {
                self.cursor_col = c;
                return;
            }
        }
        self.cursor_col = self.cols.saturating_sub(1);
    }

    /// Move cursor to previous tab stop.
    pub fn move_to_prev_tab(&mut self) {
        self.pending_wrap = false;
        if self.cursor_col == 0 {
            return;
        }
        for c in (0..self.cursor_col).rev() {
            if self.tabs.get(c as usize).copied().unwrap_or(false) {
                self.cursor_col = c;
                return;
            }
        }
        self.cursor_col = 0;
    }

    /// Set tab stop at current cursor column.
    pub fn set_tab_stop(&mut self) {
        if (self.cursor_col as usize) < self.tabs.len() {
            self.tabs[self.cursor_col as usize] = true;
        }
    }

    /// Clear tab stop at current column.
    pub fn clear_tab_stop(&mut self) {
        if (self.cursor_col as usize) < self.tabs.len() {
            self.tabs[self.cursor_col as usize] = false;
        }
    }

    /// Clear all tab stops.
    pub fn clear_all_tab_stops(&mut self) {
        self.tabs.fill(false);
    }

    /// Set vertical scrolling margins (DECSTBM).
    pub fn set_scroll_region(&mut self, top: u16, bottom: u16, modes: Modes) {
        let top = top.min(self.num_rows.saturating_sub(1));
        let bottom = bottom.min(self.num_rows.saturating_sub(1));
        if top < bottom {
            self.top_margin = top;
            self.bottom_margin = bottom;
        } else {
            self.top_margin = 0;
            self.bottom_margin = self.num_rows.saturating_sub(1);
        }
        self.set_cursor_pos(0, 0, modes);
    }

    /// Scroll up by 1 line within vertical margins without heap allocation.
    #[inline]
    pub fn scroll_up_one(&mut self, style: u16, pool: &mut RowPool) -> Option<Row> {
        let top = self.top_margin as usize;
        let bottom = self.bottom_margin as usize;
        let is_full_screen = top == 0 && bottom == self.num_rows as usize - 1;
        let row = self.rows.remove(top);
        let new_row = pool.alloc(self.cols, style);
        self.rows.insert(bottom, new_row);
        if is_full_screen {
            Some(row)
        } else {
            pool.recycle(row);
            None
        }
    }

    /// Scroll up by `count` lines within vertical margins.
    ///
    /// If margins cover the full screen (0..num_rows - 1), returns the evicted rows
    /// so the caller can push them to scrollback!
    pub fn scroll_up(&mut self, count: u16, style: u16, pool: &mut RowPool) -> Vec<Row> {
        let top = self.top_margin as usize;
        let bottom = self.bottom_margin as usize;
        let count = (count as usize).min(bottom - top + 1);

        let mut evicted = Vec::with_capacity(count);
        let is_full_screen = top == 0 && bottom == self.num_rows as usize - 1;

        for _ in 0..count {
            let row = self.rows.remove(top);
            if is_full_screen {
                evicted.push(row);
            } else {
                pool.recycle(row);
            }

            let new_row = pool.alloc(self.cols, style);
            self.rows.insert(bottom, new_row);
        }

        evicted
    }

    /// Scroll down by `count` lines within vertical margins.
    pub fn scroll_down(&mut self, count: u16, style: u16, pool: &mut RowPool) {
        let top = self.top_margin as usize;
        let bottom = self.bottom_margin as usize;
        let count = (count as usize).min(bottom - top + 1);

        for _ in 0..count {
            let row = self.rows.remove(bottom);
            pool.recycle(row);

            let new_row = pool.alloc(self.cols, style);
            self.rows.insert(top, new_row);
        }
    }

    /// Erase in display (ED).
    pub fn erase_display(&mut self, mode: u8, style: u16) {
        self.pending_wrap = false;
        let cur_row = self.cursor_row as usize;
        let cur_col = self.cursor_col;

        match mode {
            0 => {
                // Cursor to end of screen
                if cur_row < self.rows.len() {
                    self.rows[cur_row].clear_range(cur_col, self.cols, style);
                    for r in (cur_row + 1)..self.rows.len() {
                        self.rows[r].reset(style);
                    }
                }
            }
            1 => {
                // Beginning of screen to cursor
                for r in 0..cur_row {
                    self.rows[r].reset(style);
                }
                if cur_row < self.rows.len() {
                    self.rows[cur_row].clear_range(0, cur_col + 1, style);
                }
            }
            2 | 3 => {
                // Entire screen
                for row in &mut self.rows {
                    row.reset(style);
                }
            }
            _ => {}
        }
    }

    /// Erase in line (EL).
    pub fn erase_line(&mut self, mode: u8, style: u16) {
        self.pending_wrap = false;
        let cur_row = self.cursor_row as usize;
        if cur_row >= self.rows.len() {
            return;
        }

        let cur_col = self.cursor_col;
        match mode {
            0 => {
                // Cursor to end of line
                self.rows[cur_row].clear_range(cur_col, self.cols, style);
            }
            1 => {
                // Start of line to cursor
                self.rows[cur_row].clear_range(0, cur_col + 1, style);
            }
            2 => {
                // Entire line
                self.rows[cur_row].reset(style);
            }
            _ => {}
        }
    }

    /// Erase `count` characters at cursor without moving cursor (ECH).
    pub fn erase_chars(&mut self, count: u16, style: u16) {
        let cur_row = self.cursor_row as usize;
        if cur_row < self.rows.len() {
            let start = self.cursor_col;
            let end = (start + count).min(self.cols);
            self.rows[cur_row].clear_range(start, end, style);
        }
    }

    /// Insert `count` blank characters at cursor (ICH).
    pub fn insert_chars(&mut self, count: u16, style: u16) {
        let cur_row = self.cursor_row as usize;
        if cur_row >= self.rows.len() {
            return;
        }

        let row = &mut self.rows[cur_row];
        let col = self.cursor_col as usize;
        let count = (count as usize).min(row.cells.len() - col);

        for i in ((col + count)..row.cells.len()).rev() {
            row.cells[i] = row.cells[i - count];
        }
        for i in col..(col + count) {
            row.cells[i] = Cell::blank(style);
        }
        row.flags.insert(RowFlags::DIRTY);
        row.update_len();
    }

    /// Delete `count` characters at cursor (DCH).
    pub fn delete_chars(&mut self, count: u16, style: u16) {
        let cur_row = self.cursor_row as usize;
        if cur_row >= self.rows.len() {
            return;
        }

        let row = &mut self.rows[cur_row];
        let col = self.cursor_col as usize;
        let count = (count as usize).min(row.cells.len() - col);

        for i in col..(row.cells.len() - count) {
            row.cells[i] = row.cells[i + count];
        }
        for i in (row.cells.len() - count)..row.cells.len() {
            row.cells[i] = Cell::blank(style);
        }
        row.flags.insert(RowFlags::DIRTY);
        row.update_len();
    }

    /// Translate a char according to the active charset.
    fn map_char(&mut self, c: char) -> char {
        let charset = if let Some(ss) = self.single_shift.take() {
            match ss {
                2 => self.g2,
                3 => self.g3,
                _ => self.g0,
            }
        } else if self.gl == 1 {
            self.g1
        } else {
            self.g0
        };

        if charset == Charset::DecSpecialGraphics {
            map_dec_special_graphics(c)
        } else if charset == Charset::Uk && c == '#' {
            '£'
        } else {
            c
        }
    }

    /// Print a character into the grid, returning an evicted row if a scroll occurred.
    pub fn print_char(
        &mut self,
        c: char,
        style: u16,
        modes: Modes,
        pool: &mut RowPool,
        graphemes: &mut GraphemeTable,
    ) -> Option<Row> {
        let mut evicted = None;
        let autowrap = modes.contains(Modes::AUTOWRAP);
        let insert = modes.contains(Modes::INSERT);

        let is_ascii_fast = c.is_ascii()
            && self.gl == 0
            && self.single_shift.is_none()
            && self.g0 == Charset::Ascii;

        let (c, w) = if is_ascii_fast {
            (c, 1u16)
        } else {
            let mapped = self.map_char(c);
            let width = UnicodeWidthChar::width(mapped).unwrap_or(1) as u16;
            (mapped, width)
        };

        // Combining mark (zero width)
        if w == 0 {
            if self.cursor_col > 0 || self.cursor_row > 0 {
                let target_col = if self.cursor_col > 0 {
                    self.cursor_col - 1
                } else {
                    self.cols - 1
                };
                let target_row = if self.cursor_col > 0 {
                    self.cursor_row
                } else {
                    self.cursor_row - 1
                };

                let cell = self.rows[target_row as usize].get_mut(target_col);
                if cell.flags.contains(CellFlags::WIDE_SPACER) && target_col > 0 {
                    // Attach to preceding wide character
                    let base_cell = self.rows[target_row as usize].get_mut(target_col - 1);
                    attach_combining(base_cell, c, graphemes);
                } else {
                    attach_combining(cell, c, graphemes);
                }
            }
            return evicted;
        }

        // Handle delayed autowrap
        if self.pending_wrap {
            self.pending_wrap = false;
            // Mark previous line as wrapped
            let cur_row = self.cursor_row as usize;
            if cur_row < self.rows.len() {
                self.rows[cur_row].flags.insert(RowFlags::WRAPPED);
                let last_col = self.cols.saturating_sub(1);
                self.rows[cur_row]
                    .get_mut(last_col)
                    .flags
                    .insert(CellFlags::WRAPLINE);
            }

            if self.cursor_row >= self.bottom_margin {
                evicted = self.scroll_up_one(style, pool);
            } else {
                self.cursor_row += 1;
            }
            self.cursor_col = self.left_margin;
        }

        // If wide char doesn't fit on this row, wrap early
        if w == 2 && self.cursor_col + 1 >= self.cols && autowrap {
            let cur_row = self.cursor_row as usize;
            if cur_row < self.rows.len() {
                self.rows[cur_row].flags.insert(RowFlags::WRAPPED);
                self.rows[cur_row]
                    .get_mut(self.cursor_col)
                    .flags
                    .insert(CellFlags::WRAPLINE);
            }
            if self.cursor_row >= self.bottom_margin {
                evicted = self.scroll_up_one(style, pool);
            } else {
                self.cursor_row += 1;
            }
            self.cursor_col = self.left_margin;
        }

        let cur_row = self.cursor_row as usize;
        if cur_row >= self.rows.len() {
            return evicted;
        }

        if insert {
            self.insert_chars(w, style);
        }

        let col = self.cursor_col;
        if w == 2 {
            let c1 = self.rows[cur_row].get_mut(col);
            c1.ch = c as u32;
            c1.style = style;
            c1.flags = CellFlags::WIDE;

            if col + 1 < self.cols {
                let c2 = self.rows[cur_row].get_mut(col + 1);
                c2.ch = ' ' as u32;
                c2.style = style;
                c2.flags = CellFlags::WIDE_SPACER;
            }

            if col + 2 >= self.cols {
                if autowrap {
                    self.pending_wrap = true;
                }
                self.cursor_col = self.cols.saturating_sub(1);
            } else {
                self.cursor_col += 2;
            }
        } else {
            let cell = self.rows[cur_row].get_mut(col);
            cell.ch = c as u32;
            cell.style = style;
            cell.flags = CellFlags::empty();

            if col + 1 >= self.cols {
                if autowrap {
                    self.pending_wrap = true;
                }
                self.cursor_col = self.cols.saturating_sub(1);
            } else {
                self.cursor_col += 1;
            }
        }

        self.rows[cur_row].notify_write(col, w);
        evicted
    }

    /// Fast-path bulk write of printable ASCII bytes (`0x20..=0x7E`) that fit on the current row
    /// without wrapping or charset translation.
    #[inline]
    pub fn print_ascii_run(&mut self, bytes: &[u8], style: u16) {
        let cur_row = self.cursor_row as usize;
        let col = self.cursor_col as usize;
        let n = bytes.len();
        let row = &mut self.rows[cur_row];
        let slice = &mut row.cells[col..col + n];
        for (cell, &b) in slice.iter_mut().zip(bytes.iter()) {
            cell.ch = b as u32;
            cell.style = style;
            cell.flags = CellFlags::empty();
        }
        row.notify_write(col as u16, n as u16);
        self.cursor_col += n as u16;
    }

    /// Save cursor state (DECSC).
    pub fn save_cursor(&mut self, style: u16, modes: Modes) {
        self.saved_cursor = SavedCursor {
            row: self.cursor_row,
            col: self.cursor_col,
            style,
            charset_g0: self.g0,
            origin_mode: self.is_origin(modes),
            autowrap: modes.contains(Modes::AUTOWRAP),
        };
    }

    /// Restore cursor state (DECRC).
    pub fn restore_cursor(&mut self, modes: Modes) -> (u16, Modes) {
        self.cursor_row = self.saved_cursor.row.min(self.num_rows.saturating_sub(1));
        self.cursor_col = self.saved_cursor.col.min(self.cols.saturating_sub(1));
        self.pending_wrap = false;
        self.g0 = self.saved_cursor.charset_g0;

        let mut new_modes = modes;
        new_modes.set(Modes::ORIGIN, self.saved_cursor.origin_mode);
        new_modes.set(Modes::AUTOWRAP, self.saved_cursor.autowrap);

        (self.saved_cursor.style, new_modes)
    }
}

fn attach_combining(cell: &mut Cell, mark: char, graphemes: &mut GraphemeTable) {
    if let Some(idx) = cell.grapheme_index() {
        if let Some(existing) = graphemes.get(idx) {
            let mut combined = String::from(existing);
            combined.push(mark);
            let new_idx = graphemes.insert(&combined);
            cell.ch = GRAPHEME_BIT | new_idx;
        }
    } else if let Some(base_char) = char::from_u32(cell.ch) {
        let mut combined = String::new();
        combined.push(base_char);
        combined.push(mark);
        let new_idx = graphemes.insert(&combined);
        cell.ch = GRAPHEME_BIT | new_idx;
    }
}

fn map_dec_special_graphics(c: char) -> char {
    match c {
        '`' => '◆', // diamond
        'a' => '▒', // checkerboard
        'b' => '␉', // HT
        'c' => '␌', // FF
        'd' => '␍', // CR
        'e' => '␊', // LF
        'f' => '°', // degree
        'g' => '±', // plus/minus
        'h' => '␤', // NL
        'i' => '␋', // VT
        'j' => '┘', // lower right corner
        'k' => '┐', // upper right corner
        'l' => '┌', // upper left corner
        'm' => '└', // lower left corner
        'n' => '┼', // cross
        'o' => '⎺', // scan 1
        'p' => '⎻', // scan 3
        'q' => '─', // horizontal line
        'r' => '⎼', // scan 7
        's' => '⎽', // scan 9
        't' => '├', // left tee
        'u' => '┤', // right tee
        'v' => '┴', // bottom tee
        'w' => '┬', // top tee
        'x' => '│', // vertical line
        'y' => '≤', // <=
        'z' => '≥', // >=
        '{' => 'π', // pi
        '|' => '≠', // not equal
        '}' => '£', // pound
        '~' => '·', // centered dot
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_print_char_and_wrap() {
        let mut grid = Grid::new(5, 3);
        let mut pool = RowPool::new(5);
        let mut graphemes = GraphemeTable::new();
        let modes = Modes::AUTOWRAP;

        for c in "Hello".chars() {
            grid.print_char(c, 0, modes, &mut pool, &mut graphemes);
        }

        assert_eq!(grid.cursor_col, 4);
        assert!(grid.pending_wrap);

        // Next character triggers wrap to row 1
        grid.print_char('!', 0, modes, &mut pool, &mut graphemes);
        assert_eq!(grid.cursor_row, 1);
        assert_eq!(grid.cursor_col, 1);
        assert!(grid.rows[0].flags.contains(RowFlags::WRAPPED));
    }

    #[test]
    fn test_dec_special_graphics() {
        assert_eq!(map_dec_special_graphics('q'), '─');
        assert_eq!(map_dec_special_graphics('x'), '│');
        assert_eq!(map_dec_special_graphics('a'), '▒');
    }
}
