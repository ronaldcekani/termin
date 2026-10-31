//! Cold tier scrollback chunking and storage.
//!
//! See `docs/specs/terminal-core.md §3.2`.

use std::cell::Cell as StdCell;
use std::collections::VecDeque;
use unicode_width::UnicodeWidthChar;

use crate::cell::{CellFlags, Style};
use crate::row::Row;
use crate::style::{GraphemeTable, StyleTable};

/// A run of bytes sharing the same resolved style.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleRun {
    pub byte_len: u16,
    pub style: Style,
}

/// A compact immutable chunk of historical scrollback lines.
///
/// Stores UTF-8 text with resolved style runs, width-independent so reflow is free.
/// See `docs/specs/terminal-core.md §3.2`.
#[derive(Clone, Debug)]
pub struct ColdChunk {
    pub first_line: u64,
    pub line_count: u16,
    pub text: Box<[u8]>,
    pub line_ends: Box<[u32]>,
    pub runs: Box<[StyleRun]>,
    pub wrapped_rows_cache: StdCell<Option<(u16, u32)>>,
    pub compressed: bool,
}

impl ColdChunk {
    /// Retrieve slice of text for line index within chunk (0..line_count).
    pub fn get_line(&self, idx: u16) -> Option<&str> {
        if idx >= self.line_count {
            return None;
        }
        let start = if idx == 0 {
            0
        } else {
            self.line_ends[(idx - 1) as usize] as usize
        };
        let end = self.line_ends[idx as usize] as usize;
        std::str::from_utf8(&self.text[start..end]).ok()
    }

    /// Invalidate width cache on terminal resize.
    #[inline]
    pub fn invalidate_cache(&self) {
        self.wrapped_rows_cache.set(None);
    }

    /// Calculate total wrapped rows this chunk occupies at `cols` width.
    pub fn rows_at_width(&self, cols: u16) -> u32 {
        if let Some((cached_cols, cached_rows)) = self.wrapped_rows_cache.get() {
            if cached_cols == cols {
                return cached_rows;
            }
        }

        let mut total_rows = 0;
        for i in 0..self.line_count {
            if let Some(line) = self.get_line(i) {
                let mut current_col = 0;
                let mut line_rows = 1;
                for ch in line.chars() {
                    let w = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                    if w == 0 {
                        continue;
                    }
                    if current_col + w > cols {
                        line_rows += 1;
                        current_col = w;
                    } else {
                        current_col += w;
                    }
                }
                total_rows += line_rows;
            } else {
                total_rows += 1;
            }
        }

        self.wrapped_rows_cache.set(Some((cols, total_rows)));
        total_rows
    }

    /// Decode all rows in this chunk at `cols` width into a list of `Row`s.
    pub fn decode_all_rows(
        &self,
        cols: u16,
        styles: &mut StyleTable,
        _graphemes: &mut GraphemeTable,
    ) -> Vec<Row> {
        let mut decoded = Vec::new();
        let mut run_idx = 0;
        let mut run_byte_offset = 0;
        let mut text_offset = 0;

        for line_idx in 0..self.line_count {
            let line_start = if line_idx == 0 {
                0
            } else {
                self.line_ends[(line_idx - 1) as usize] as usize
            };
            let line_end = self.line_ends[line_idx as usize] as usize;

            let mut row = Row::new(cols);
            let mut col = 0;

            let line_slice = &self.text[line_start..line_end];
            let mut byte_idx = 0;

            while byte_idx < line_slice.len() {
                // Determine current style from runs
                let current_global_byte = text_offset + byte_idx;
                while run_idx < self.runs.len() {
                    let run = &self.runs[run_idx];
                    let run_end = run_byte_offset + run.byte_len as usize;
                    if current_global_byte < run_end {
                        break;
                    }
                    run_byte_offset = run_end;
                    run_idx += 1;
                }

                let style = if run_idx < self.runs.len() {
                    styles.insert(self.runs[run_idx].style)
                } else {
                    0
                };

                let ch_str = std::str::from_utf8(&line_slice[byte_idx..]).unwrap_or("");
                if let Some(ch) = ch_str.chars().next() {
                    let ch_len = ch.len_utf8();
                    byte_idx += ch_len;

                    let w = UnicodeWidthChar::width(ch).unwrap_or(1) as u16;
                    if w == 0 {
                        // Combining character
                        if col > 0 {
                            // In simple cold decode, if previous cell exists, attach or leave
                        }
                        continue;
                    }

                    if col + w > cols {
                        // Soft wrap to next row
                        row.flags.insert(crate::row::RowFlags::WRAPPED);
                        row.update_len();
                        decoded.push(row);

                        row = Row::new(cols);
                        col = 0;
                    }

                    if w == 2 {
                        let c1 = row.get_mut(col);
                        c1.ch = ch as u32;
                        c1.style = style;
                        c1.flags = CellFlags::WIDE;

                        if col + 1 < cols {
                            let c2 = row.get_mut(col + 1);
                            c2.ch = ' ' as u32;
                            c2.style = style;
                            c2.flags = CellFlags::WIDE_SPACER;
                        }
                        col += 2;
                    } else {
                        let c = row.get_mut(col);
                        c.ch = ch as u32;
                        c.style = style;
                        c.flags = CellFlags::empty();
                        col += 1;
                    }
                } else {
                    break;
                }
            }

            text_offset += line_slice.len();
            row.update_len();
            decoded.push(row);
        }

        decoded
    }
}

/// Accumulator building `ColdChunk`s from logical lines.
#[derive(Default, Debug)]
pub struct ColdBuilder {
    first_line: u64,
    current_line: u64,
    lines: u16,
    text: Vec<u8>,
    line_ends: Vec<u32>,
    runs: Vec<StyleRun>,
    // Current logical line buffer
    pending_text: String,
    pending_runs: Vec<StyleRun>,
}

impl ColdBuilder {
    pub fn new(start_line: u64) -> Self {
        Self {
            first_line: start_line,
            current_line: start_line,
            lines: 0,
            text: Vec::with_capacity(4096),
            line_ends: Vec::with_capacity(256),
            runs: Vec::with_capacity(256),
            pending_text: String::with_capacity(128),
            pending_runs: Vec::with_capacity(16),
        }
    }

    /// Feed a row being evicted from the hot tier.
    ///
    /// If `row` is wrapped, its text continues into the current logical line.
    /// If not wrapped, seals the current logical line.
    pub fn push_row(
        &mut self,
        row: &Row,
        styles: &StyleTable,
        graphemes: &GraphemeTable,
    ) -> Option<ColdChunk> {
        let is_wrapped = row.flags.contains(crate::row::RowFlags::WRAPPED);

        let cells = row.content_cells();
        let mut cell_idx = 0;

        while cell_idx < cells.len() {
            let current_style_idx = cells[cell_idx].style;
            let style = styles.get_or_default(current_style_idx);
            let start_byte_len = self.pending_text.len();

            while cell_idx < cells.len() && cells[cell_idx].style == current_style_idx {
                let cell = &cells[cell_idx];
                cell_idx += 1;
                if cell.flags.contains(CellFlags::WIDE_SPACER) {
                    continue;
                }

                if cell.ch < 128 {
                    self.pending_text.push(cell.ch as u8 as char);
                } else if let Some(g) = cell.grapheme_index().and_then(|i| graphemes.get(i)) {
                    self.pending_text.push_str(g);
                } else if let Some(c) = char::from_u32(cell.ch) {
                    let mut buf = [0u8; 4];
                    self.pending_text.push_str(c.encode_utf8(&mut buf));
                } else {
                    self.pending_text.push(' ');
                }
            }

            let added_bytes = (self.pending_text.len() - start_byte_len) as u16;
            if added_bytes > 0 {
                self.add_run(added_bytes, style);
            }
        }

        if !is_wrapped {
            // End of logical line
            self.finish_line();
        }

        if self.lines >= 256 || self.text.len() >= 64 * 1024 {
            Some(self.seal())
        } else {
            None
        }
    }

    fn add_run(&mut self, byte_len: u16, style: Style) {
        if let Some(last) = self.pending_runs.last_mut() {
            if last.style == style && (last.byte_len as usize + byte_len as usize) <= u16::MAX as usize {
                last.byte_len += byte_len;
                return;
            }
        }
        self.pending_runs.push(StyleRun { byte_len, style });
    }

    fn finish_line(&mut self) {
        self.text.extend_from_slice(self.pending_text.as_bytes());
        self.line_ends.push(self.text.len() as u32);
        self.runs.append(&mut self.pending_runs);
        self.pending_text.clear();
        self.lines += 1;
        self.current_line += 1;
    }

    /// Seal current accumulated data into a `ColdChunk`.
    pub fn seal(&mut self) -> ColdChunk {
        if !self.pending_text.is_empty() {
            self.finish_line();
        }

        let chunk = ColdChunk {
            first_line: self.first_line,
            line_count: self.lines,
            text: std::mem::take(&mut self.text).into_boxed_slice(),
            line_ends: std::mem::take(&mut self.line_ends).into_boxed_slice(),
            runs: std::mem::take(&mut self.runs).into_boxed_slice(),
            wrapped_rows_cache: StdCell::new(None),
            compressed: false,
        };

        self.first_line = self.current_line;
        self.lines = 0;
        chunk
    }

    pub fn is_empty(&self) -> bool {
        self.lines == 0 && self.pending_text.is_empty()
    }
}

/// Cold tier holding sealed chunks with an LRU decode cache.
#[derive(Debug)]
pub struct ColdTier {
    pub chunks: VecDeque<ColdChunk>,
    pub total_lines: u64,
    pub max_lines: u32,
    // LRU cache: (chunk_idx, cols, decoded_rows) up to 4 entries
    decoded_cache: VecDeque<(usize, u16, Vec<Row>)>,
}

impl ColdTier {
    pub fn new(max_lines: u32) -> Self {
        Self {
            chunks: VecDeque::new(),
            total_lines: 0,
            max_lines,
            decoded_cache: VecDeque::with_capacity(4),
        }
    }

    /// Push sealed chunk and enforce `max_lines` limit.
    pub fn push_chunk(&mut self, chunk: ColdChunk) {
        self.total_lines += chunk.line_count as u64;
        self.chunks.push_back(chunk);
        self.enforce_limit();
    }

    fn enforce_limit(&mut self) {
        while self.total_lines > self.max_lines as u64 && self.chunks.len() > 1 {
            if let Some(oldest) = self.chunks.pop_front() {
                self.total_lines = self.total_lines.saturating_sub(oldest.line_count as u64);
                self.decoded_cache.clear();
            }
        }
    }

    pub fn invalidate_cache(&self) {
        for chunk in &self.chunks {
            chunk.invalidate_cache();
        }
    }

    /// Clear all cold scrollback.
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.total_lines = 0;
        self.decoded_cache.clear();
    }

    /// Get decoded rows for chunk at `chunk_idx` at `cols` width.
    pub fn get_decoded_rows(
        &mut self,
        chunk_idx: usize,
        cols: u16,
        styles: &mut StyleTable,
        graphemes: &mut GraphemeTable,
    ) -> Option<&[Row]> {
        if chunk_idx >= self.chunks.len() {
            return None;
        }

        // Check cache
        if let Some(pos) = self
            .decoded_cache
            .iter()
            .position(|(idx, c, _)| *idx == chunk_idx && *c == cols)
        {
            let entry = self.decoded_cache.remove(pos).unwrap();
            self.decoded_cache.push_front(entry);
            return Some(&self.decoded_cache.front().unwrap().2);
        }

        // Decode chunk
        let chunk = &self.chunks[chunk_idx];
        let decoded = chunk.decode_all_rows(cols, styles, graphemes);

        if self.decoded_cache.len() >= 4 {
            self.decoded_cache.pop_back();
        }
        self.decoded_cache.push_front((chunk_idx, cols, decoded));
        Some(&self.decoded_cache.front().unwrap().2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cold_chunk_builder_and_decode() {
        let styles = StyleTable::new();
        let graphemes = GraphemeTable::new();
        let mut builder = ColdBuilder::new(0);

        let mut r1 = Row::new(80);
        r1.get_mut(0).ch = 'H' as u32;
        r1.get_mut(1).ch = 'i' as u32;
        r1.flags.insert(crate::row::RowFlags::WRAPPED);
        r1.update_len();
        builder.push_row(&r1, &styles, &graphemes);

        let mut r2 = Row::new(80);
        r2.get_mut(0).ch = '!' as u32;
        r2.update_len();
        builder.push_row(&r2, &styles, &graphemes);

        let chunk = builder.seal();
        assert_eq!(chunk.line_count, 1);
        assert_eq!(chunk.get_line(0), Some("Hi!"));

        let mut dec_styles = StyleTable::new();
        let mut dec_graphemes = GraphemeTable::new();
        let decoded = chunk.decode_all_rows(80, &mut dec_styles, &mut dec_graphemes);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].get(0).ch, 'H' as u32);
        assert_eq!(decoded[0].get(1).ch, 'i' as u32);
        assert_eq!(decoded[0].get(2).ch, '!' as u32);
    }
}
