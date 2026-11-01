//! `PaneRenderCache`: per-pane cache of row instances with hash/scroll reuse
//! (`docs/specs/text-and-rendering.md §3.4`).
//!
//! Rows are re-built only when their cell content hash changed. This makes the
//! cache correct regardless of how precisely the terminal reports damage; the
//! terminal's `Damage.scrolled` hint is used to rotate rows so scrolling output
//! only rebuilds the newly exposed rows.

use termin_text::FontBackend;
use termin_vt::{Cell, ScrollPos, Terminal};

use super::row::{build_row, hash_cells, RowCtx, RowInstances};

/// Inputs that, when changed, invalidate every cached row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ViewKey {
    pub cols: u16,
    pub rows: u16,
    /// Font / theme / scale epoch maintained by the app.
    pub epoch: u32,
    /// `GlyphCache::epoch` (atlas coordinates become invalid when it changes).
    pub atlas_epoch: u64,
}

enum Act {
    Keep,
    Clone(usize),
    Rebuild,
}

/// Per-pane cache of visible row instances.
#[derive(Default)]
pub struct PaneRenderCache {
    rows: Vec<RowInstances>,
    generation: u64,
    scroll: Option<ScrollPos>,
    key: Option<ViewKey>,
    hashes: Vec<u64>,
    acts: Vec<Act>,
    /// Scratch cells for a scrolled view. Copying releases the mutable VT view
    /// before row construction needs immutable access to its style tables.
    scrolled_cells: Vec<Vec<Cell>>,
    /// Number of rows rebuilt by the last `sync` (for tests/benches/diagnostics).
    pub rebuilt: usize,
}

impl PaneRenderCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cached rows, top to bottom.
    #[inline]
    pub fn rows(&self) -> &[RowInstances] {
        &self.rows
    }

    /// Force a full rebuild on next sync.
    pub fn invalidate(&mut self) {
        self.key = None;
    }

    /// Synchronize with the terminal. Returns `true` if anything changed.
    #[allow(clippy::needless_range_loop)]
    pub fn sync<B: FontBackend>(
        &mut self,
        term: &mut Terminal,
        scroll: ScrollPos,
        key: ViewKey,
        ctx: &mut RowCtx<'_, B>,
    ) -> bool {
        self.rebuilt = 0;
        let same_key = self.key == Some(key);
        if same_key && self.generation == term.generation() && self.scroll == Some(scroll) {
            return false;
        }

        let nrows = term.rows() as usize;
        let mut old = std::mem::take(&mut self.rows);

        // Rotate on terminal scroll when the view follows the live screen.
        if same_key && scroll == ScrollPos::Bottom && self.scroll == Some(ScrollPos::Bottom) {
            let d = term.damage().scrolled;
            if d < 0 {
                let n = (-d) as usize;
                if n < old.len() {
                    old.rotate_left(n);
                }
            }
        }

        // Cold history needs a mutable terminal to decode. Copy only the
        // viewport into reusable scratch before consulting terminal styles.
        if scroll != ScrollPos::Bottom {
            self.scrolled_cells.resize_with(nrows, Vec::new);
            for row in &mut self.scrolled_cells {
                row.clear();
            }
            for (out, row) in self
                .scrolled_cells
                .iter_mut()
                .zip(term.view_with_cold(scroll))
            {
                out.extend_from_slice(row.cells);
            }
        }

        // Pass 1: hash visible rows.
        self.hashes.clear();
        if scroll == ScrollPos::Bottom {
            for rv in term.view(scroll) {
                self.hashes.push(hash_cells(rv.cells));
            }
        } else {
            self.hashes
                .extend(self.scrolled_cells.iter().map(|row| hash_cells(row)));
        }
        self.hashes.resize(nrows, 0);

        // Pass 2: decide per-row action.
        self.acts.clear();
        let usable = same_key;
        for (i, &h) in self.hashes.iter().enumerate() {
            let act = if usable && old.get(i).map(|r| r.hash) == Some(h) {
                Act::Keep
            } else if usable {
                match old.iter().position(|r| r.hash == h) {
                    Some(j) => Act::Clone(j),
                    None => Act::Rebuild,
                }
            } else {
                Act::Rebuild
            };
            self.acts.push(act);
        }

        // Clone sources first (before rows are moved out of `old`).
        let mut cloned: Vec<(usize, RowInstances)> = Vec::new();
        for (i, act) in self.acts.iter().enumerate() {
            if let Act::Clone(j) = act {
                cloned.push((i, old[*j].clone()));
            }
        }
        let mut cloned = cloned.into_iter().peekable();

        // Spares: old rows that are not kept, reused as allocation pools.
        let mut spares: Vec<RowInstances> = Vec::new();
        let mut kept: Vec<Option<RowInstances>> = Vec::with_capacity(nrows);
        kept.resize_with(nrows, || None);
        for (i, mut r) in old.drain(..).enumerate() {
            if i < nrows && matches!(self.acts[i], Act::Keep) {
                kept[i] = Some(std::mem::take(&mut r));
            } else {
                spares.push(r);
            }
        }
        // `Keep` rows above were moved by value; restore content (take replaced with default).
        // (mem::take moved the content into `kept`; `r` is now empty and discarded.)

        let mut next: Vec<RowInstances> = Vec::with_capacity(nrows);
        // Row cells are borrowed from `term` during rebuild, so iterate the view once.
        let mut view = (scroll == ScrollPos::Bottom).then(|| term.view(scroll));
        for i in 0..nrows {
            let cells = if let Some(view) = view.as_mut() {
                view.next().map(|row| row.cells)
            } else {
                self.scrolled_cells.get(i).map(Vec::as_slice)
            };
            match (&self.acts[i], kept[i].take()) {
                (Act::Keep, Some(r)) => next.push(r),
                (Act::Clone(_), _) => {
                    if cloned.peek().map(|(k, _)| *k) == Some(i) {
                        if let Some((_, r)) = cloned.next() {
                            next.push(r);
                        }
                    } else {
                        next.push(RowInstances::default());
                    }
                }
                _ => {
                    let mut r = spares.pop().unwrap_or_default();
                    if let Some(cells) = cells {
                        build_row(cells, term.styles(), term, ctx, &mut r);
                        r.hash = self.hashes[i];
                    }
                    self.rebuilt += 1;
                    next.push(r);
                }
            }
        }
        drop(view);

        self.rows = next;
        self.generation = term.generation();
        self.scroll = Some(scroll);
        self.key = Some(key);
        term.reset_damage();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::TermFonts;
    use termin_text::{CoreTextBackend, FontBackend, FontSpec, GlyphCache};
    use termin_vt::{Effect, EffectSink, TerminalConfig};

    struct Sink;
    impl EffectSink for Sink {
        fn write_back(&mut self, _: &[u8]) {}
        fn effect(&mut self, _: Effect) {}
    }

    #[test]
    fn cache_builds_rows_from_cold_scrollback() {
        let config = TerminalConfig {
            cols: 12,
            rows: 2,
            scrollback_lines: 1000,
            ..TerminalConfig::default()
        };
        let mut term = Terminal::new(&config);
        term.advance(b"oldest\r\n", &mut Sink);
        for _ in 0..220 {
            term.advance(b"later\r\n", &mut Sink);
        }
        term.scroll_display(i32::MAX);
        let pos = term.display_scroll_pos();
        let expected: Vec<_> = term
            .view_with_cold(pos)
            .map(|row| hash_cells(row.cells))
            .collect();
        let mut backend = CoreTextBackend::new();
        let font = backend.resolve(&FontSpec::default());
        let fonts = TermFonts::new(font, &backend.metrics(font), 1.0);
        let mut glyphs = GlyphCache::new(backend);
        let palette =
            super::super::TermPalette::from_theme(&termin_config::TerminalTheme::default(), false);
        let mut ctx = RowCtx {
            glyphs: &mut glyphs,
            fonts: &fonts,
            palette: &palette,
        };
        let mut cache = PaneRenderCache::new();
        cache.sync(
            &mut term,
            pos,
            ViewKey {
                cols: 12,
                rows: 2,
                epoch: 0,
                atlas_epoch: 0,
            },
            &mut ctx,
        );
        assert_eq!(
            cache.rows().iter().map(|row| row.hash).collect::<Vec<_>>(),
            expected
        );
    }
}
