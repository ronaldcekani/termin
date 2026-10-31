//! Line location mapping between absolute line indices and hot/cold tiers.
//!
//! See `docs/specs/terminal-core.md §3.3`.

use crate::cell::AbsLine;

/// Physical location of an `AbsLine` in scrollback tiers.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LineLocation {
    /// In the hot ring buffer at index `0..hot_len`.
    Hot(usize),
    /// In the cold tier chunk.
    Cold {
        chunk_idx: usize,
        line_idx: u16,
        wrap_row: u16,
    },
    /// The line has been evicted.
    Evicted,
}

/// Helper for indexing and resolving `AbsLine` across cold and hot tiers.
#[derive(Clone, Debug, Default)]
pub struct IndexMapper {
    /// Absolute line number of the oldest line in history (ever or currently retained).
    pub base_abs: AbsLine,
}

impl IndexMapper {
    pub fn new() -> Self {
        Self {
            base_abs: AbsLine(0),
        }
    }
}
