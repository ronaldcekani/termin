//! Two-tier scrollback implementation: hot ring + cold chunks.
//!
//! See `docs/specs/terminal-core.md §3`.

pub mod cold;
pub mod index;

use std::collections::VecDeque;

use crate::cell::AbsLine;
use crate::row::{Row, RowPool};
use crate::scrollback::cold::{ColdBuilder, ColdTier};
use crate::scrollback::index::LineLocation;
use crate::style::{GraphemeTable, StyleTable};

/// Two-tier scrollback coordinator.
#[derive(Debug)]
pub struct Scrollback {
    /// Hot ring buffer storing recent scrollback rows.
    pub hot_ring: VecDeque<Row>,
    /// Cold chunked tier storing older history.
    pub cold_tier: ColdTier,
    /// Cold chunk builder accumulating rows into logical lines.
    pub cold_builder: ColdBuilder,
    /// Buffer pool for recycling rows.
    pub row_pool: RowPool,
    /// Absolute line number of the first line currently stored in the hot ring.
    pub hot_base_line: u64,
    /// Absolute line number of the very first line retained (cold or hot).
    pub first_retained_line: u64,
    /// Total rows pushed to scrollback.
    pub total_pushed: u64,
    /// Configured max cold lines.
    pub max_scrollback: u32,
    /// Target capacity of hot ring.
    pub hot_capacity: usize,
}

impl Scrollback {
    pub fn new(cols: u16, screen_rows: u16, max_scrollback: u32) -> Self {
        let hot_scrollback = (2 * screen_rows as usize).clamp(128, 1024);
        let hot_capacity = hot_scrollback;
        Self {
            hot_ring: VecDeque::with_capacity(hot_capacity),
            cold_tier: ColdTier::new(max_scrollback),
            cold_builder: ColdBuilder::new(0),
            row_pool: RowPool::new(cols),
            hot_base_line: 0,
            first_retained_line: 0,
            total_pushed: 0,
            max_scrollback,
            hot_capacity,
        }
    }

    /// Update screen dimensions and recalculate hot capacity.
    pub fn resize(&mut self, cols: u16, screen_rows: u16) {
        self.hot_capacity = (2 * screen_rows as usize).clamp(128, 1024);
        self.cold_tier.invalidate_cache();
        self.row_pool = RowPool::new(cols);
    }

    /// Push an evicted screen row into the hot scrollback ring.
    pub fn push(&mut self, row: Row, styles: &StyleTable, graphemes: &GraphemeTable) {
        self.total_pushed += 1;

        if self.hot_ring.len() >= self.hot_capacity {
            // Evict oldest hot row into cold tier
            if let Some(oldest) = self.hot_ring.pop_front() {
                self.hot_base_line += 1;
                if let Some(chunk) = self.cold_builder.push_row(&oldest, styles, graphemes) {
                    self.cold_tier.push_chunk(chunk);
                }
                // Recycle the evicted row allocation
                self.row_pool.recycle(oldest);
            }
        }

        self.hot_ring.push_back(row);
    }

    /// Push an evicted screen row into the hot scrollback ring, recycling any
    /// evicted hot-ring row into `pool`.
    #[inline]
    pub fn push_into_pool(
        &mut self,
        row: Row,
        styles: &StyleTable,
        graphemes: &GraphemeTable,
        pool: &mut RowPool,
    ) {
        self.total_pushed += 1;

        if self.hot_ring.len() >= self.hot_capacity {
            if let Some(oldest) = self.hot_ring.pop_front() {
                self.hot_base_line += 1;
                if let Some(chunk) = self.cold_builder.push_row(&oldest, styles, graphemes) {
                    self.cold_tier.push_chunk(chunk);
                }
                pool.recycle(oldest);
            }
        }

        self.hot_ring.push_back(row);
    }

    /// Number of rows currently in the hot scrollback ring.
    #[inline]
    pub fn hot_len(&self) -> usize {
        self.hot_ring.len()
    }

    /// Clear all scrollback (e.g. on ED 3).
    pub fn clear(&mut self) {
        for row in self.hot_ring.drain(..) {
            self.row_pool.recycle(row);
        }
        self.cold_tier.clear();
        self.cold_builder = ColdBuilder::new(self.total_pushed);
        self.hot_base_line = self.total_pushed;
        self.first_retained_line = self.total_pushed;
    }

    /// Map `AbsLine` to its location in hot or cold tiers.
    pub fn locate(&self, abs: AbsLine) -> LineLocation {
        let abs_val = abs.0;
        if abs_val < self.first_retained_line || abs_val >= self.total_pushed {
            return LineLocation::Evicted;
        }

        if abs_val >= self.hot_base_line {
            let offset = (abs_val - self.hot_base_line) as usize;
            if offset < self.hot_ring.len() {
                return LineLocation::Hot(offset);
            }
        }

        // Search in cold chunks
        for (chunk_idx, chunk) in self.cold_tier.chunks.iter().enumerate() {
            let chunk_end = chunk.first_line + chunk.line_count as u64;
            if abs_val >= chunk.first_line && abs_val < chunk_end {
                let line_idx = (abs_val - chunk.first_line) as u16;
                return LineLocation::Cold {
                    chunk_idx,
                    line_idx,
                    wrap_row: 0,
                };
            }
        }

        LineLocation::Evicted
    }

    /// Get a reference to a row in the hot ring by index (0 is oldest hot row).
    #[inline]
    pub fn get_hot(&self, idx: usize) -> Option<&Row> {
        self.hot_ring.get(idx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollback_push_and_evict_to_cold() {
        let mut sb = Scrollback::new(80, 24, 1000);
        sb.hot_capacity = 5; // small for testing

        let styles = StyleTable::new();
        let graphemes = GraphemeTable::new();

        for i in 0..10 {
            let mut row = sb.row_pool.alloc(80, 0);
            row.get_mut(0).ch = (b'0' + i) as u32;
            row.update_len();
            sb.push(row, &styles, &graphemes);
        }

        assert_eq!(sb.hot_len(), 5);
        assert_eq!(sb.total_pushed, 10);
        assert_eq!(sb.hot_base_line, 5);

        // Check latest row in hot ring
        let last_hot = sb.hot_ring.back().unwrap();
        assert_eq!(last_hot.get(0).ch, '9' as u32);
    }

    #[test]
    fn long_soft_wrapped_line_is_not_split_when_evicted_past_chunk_threshold() {
        let mut sb = Scrollback::new(80, 24, 10_000);
        sb.hot_capacity = 1;
        let styles = StyleTable::new();
        let graphemes = GraphemeTable::new();
        let row_count = 820usize;

        for idx in 0..row_count {
            let mut row = Row::new(80);
            for col in 0..80 {
                row.get_mut(col).ch = 'a' as u32;
            }
            row.update_len();
            if idx + 1 < row_count {
                row.flags.insert(crate::row::RowFlags::WRAPPED);
            }
            sb.push(row, &styles, &graphemes);
        }

        // All but the final row have been evicted. The builder has crossed
        // 64 KiB but must retain the still-open logical line.
        assert!(sb.cold_tier.chunks.is_empty());
        let pending = sb.cold_builder.snapshot_chunk().unwrap();
        assert_eq!(pending.line_count, 1);
        assert_eq!(pending.get_line(0).unwrap().len(), (row_count - 1) * 80);

        // Evicting the final non-WRAPPED row completes and seals the whole line.
        sb.push(Row::new(80), &styles, &graphemes);
        assert_eq!(sb.cold_tier.chunks.len(), 1);
        let sealed = &sb.cold_tier.chunks[0];
        assert_eq!(sealed.line_count, 1);
        assert_eq!(sealed.get_line(0).unwrap().len(), row_count * 80);
        assert!(sealed.get_line(0).unwrap().chars().all(|ch| ch == 'a'));
    }
}
