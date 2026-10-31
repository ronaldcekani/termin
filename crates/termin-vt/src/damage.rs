//! Damage tracking and visible row dirty state.
//!
//! See `docs/specs/terminal-core.md §11`.

/// Damage tracking for visible rows, cursor, and scroll offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Damage {
    pub cursor_moved: bool,
    pub all: bool,
    pub scrolled: i32,
    pub rows: Vec<bool>,
}

impl Default for Damage {
    fn default() -> Self {
        Self {
            cursor_moved: false,
            all: true,
            scrolled: 0,
            rows: Vec::new(),
        }
    }
}

impl Damage {
    pub fn new(rows: u16) -> Self {
        Self {
            cursor_moved: false,
            all: true,
            scrolled: 0,
            rows: vec![true; rows as usize],
        }
    }

    /// Mark an individual row dirty.
    #[inline]
    pub fn mark_row(&mut self, row: u16) {
        if let Some(r) = self.rows.get_mut(row as usize) {
            *r = true;
        }
    }

    /// Mark everything dirty.
    pub fn mark_all(&mut self) {
        self.all = true;
        self.rows.fill(true);
    }

    /// Record a scroll offset delta.
    pub fn add_scroll(&mut self, delta: i32) {
        self.scrolled += delta;
    }

    /// Reset damage state after frame presentation.
    pub fn reset(&mut self) {
        self.cursor_moved = false;
        self.all = false;
        self.scrolled = 0;
        self.rows.fill(false);
    }

    /// Check if a row is dirty.
    #[inline]
    pub fn is_row_dirty(&self, row: u16) -> bool {
        self.all || self.rows.get(row as usize).copied().unwrap_or(true)
    }
}
