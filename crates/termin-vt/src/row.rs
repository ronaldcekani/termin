//! Terminal row data structures and allocation recycling pool.
//!
//! See `docs/specs/terminal-core.md §2.4`.

use bitflags::bitflags;

use crate::cell::Cell;

bitflags! {
    /// Row state flags.
    ///
    /// See `docs/specs/terminal-core.md §2.4`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct RowFlags: u8 {
        /// Soft wrap continues into next row.
        const WRAPPED = 1 << 0;
        /// Row has been modified since last damage check.
        const DIRTY   = 1 << 1;
    }
}

/// A single row of cells in the terminal grid.
///
/// See `docs/specs/terminal-core.md §2.4`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub cells: Box<[Cell]>,
    pub len: u16,
    pub flags: RowFlags,
}

impl Row {
    /// Create a new empty row with given column count.
    pub fn new(cols: u16) -> Self {
        let cells = vec![Cell::default(); cols as usize].into_boxed_slice();
        Self {
            cells,
            len: 0,
            flags: RowFlags::empty(),
        }
    }

    /// Number of columns in this row.
    #[inline]
    pub fn cols(&self) -> u16 {
        self.cells.len() as u16
    }

    /// Reset all cells to blank and clear flags.
    #[inline]
    pub fn reset(&mut self, style: u16) {
        let blank = Cell::blank(style);
        self.cells.fill(blank);
        self.len = 0;
        self.flags = RowFlags::DIRTY;
    }

    /// Get cell at column.
    #[inline]
    pub fn get(&self, col: u16) -> &Cell {
        &self.cells[col as usize]
    }

    /// Get mutable cell reference at column.
    #[inline]
    pub fn get_mut(&mut self, col: u16) -> &mut Cell {
        self.flags.insert(RowFlags::DIRTY);
        &mut self.cells[col as usize]
    }

    /// Clear a range of columns `[start, end)` with a blank cell of `style`.
    pub fn clear_range(&mut self, start: u16, end: u16, style: u16) {
        let start = start as usize;
        let end = (end as usize).min(self.cells.len());
        if start < end {
            self.cells[start..end].fill(Cell::blank(style));
            self.flags.insert(RowFlags::DIRTY);
            self.update_len();
        }
    }

    /// Fast update of `len` when writing at column.
    #[inline]
    pub fn notify_write(&mut self, col: u16, width: u16) {
        let end = col + width;
        if end > self.len {
            self.len = end;
        }
    }

    /// Recompute `len` past the last non-blank cell.
    pub fn update_len(&mut self) {
        let mut last_non_blank = 0;
        for (i, cell) in self.cells.iter().enumerate() {
            if !cell.is_blank() {
                last_non_blank = i + 1;
            }
        }
        self.len = last_non_blank as u16;
    }

    /// Return slice of cells up to `len` (ignoring trailing implicit blanks).
    #[inline]
    pub fn content_cells(&self) -> &[Cell] {
        &self.cells[..self.len as usize]
    }

    /// True if row contains no printable characters and no wrap.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0 && !self.flags.contains(RowFlags::WRAPPED)
    }
}

/// View of a single row for iteration.
///
/// See `docs/specs/terminal-core.md §2.4`.
#[derive(Copy, Clone, Debug)]
pub struct RowView<'a> {
    pub cells: &'a [Cell],
    pub wrapped: bool,
}

/// Free list pool for recycling `Box<[Cell]>` allocations.
///
/// Ensures steady-state scrolling performs zero heap allocations.
#[derive(Default, Debug)]
pub struct RowPool {
    pool: Vec<Box<[Cell]>>,
    cols: u16,
}

impl RowPool {
    pub fn new(cols: u16) -> Self {
        Self {
            pool: Vec::with_capacity(64),
            cols,
        }
    }

    /// Allocate or reuse a row buffer.
    pub fn alloc(&mut self, cols: u16, style: u16) -> Row {
        if self.cols == cols {
            if let Some(mut cells) = self.pool.pop() {
                cells.fill(Cell::blank(style));
                return Row {
                    cells,
                    len: 0,
                    flags: RowFlags::DIRTY,
                };
            }
        } else {
            self.cols = cols;
            self.pool.clear();
        }

        let mut row = Row::new(cols);
        row.reset(style);
        row
    }

    /// Recycle an old row's cell buffer into the pool.
    pub fn recycle(&mut self, row: Row) {
        // Cap free list at 512 entries to avoid unbounded growth
        if row.cells.len() == self.cols as usize && self.pool.len() < 512 {
            self.pool.push(row.cells);
        }
    }

    /// Clear all pooled buffers.
    pub fn clear(&mut self) {
        self.pool.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_row_operations_and_update_len() {
        let mut row = Row::new(80);
        assert_eq!(row.len, 0);

        row.get_mut(5).ch = 'x' as u32;
        row.update_len();
        assert_eq!(row.len, 6);

        row.clear_range(0, 80, 0);
        assert_eq!(row.len, 0);
    }

    #[test]
    fn test_row_pool_recycling() {
        let mut pool = RowPool::new(80);
        let row = pool.alloc(80, 0);
        assert_eq!(row.cols(), 80);
        pool.recycle(row);
        assert_eq!(pool.pool.len(), 1);

        let row2 = pool.alloc(80, 0);
        assert_eq!(pool.pool.len(), 0);
        assert_eq!(row2.cols(), 80);
    }
}
