//! Terminal core state machine (`termin-vt`).
//!
//! Pure-Rust terminal state machine: grid, scrollback, modes, OSC, blocks.
//! See `docs/architecture.md §3.2` and `docs/specs/terminal-core.md`.

pub mod block;
pub mod cell;
pub mod damage;
pub mod grid;
pub mod input;
pub mod modes;
pub mod osc;
pub mod parser;
pub mod reflow;
pub mod row;
pub mod scrollback;
pub mod search;
pub mod selection;
pub mod style;
pub mod view;

use smallvec::SmallVec;
use termin_core::{BlockId, KeyEvent, PtyConsumer, Rgba};
use vte::Parser;

pub use block::{Block, BlockEvent, BlockFlags, BlockList};
pub use cell::{AbsLine, AbsPos, Attrs, Cell, CellFlags, Color, Style, GRAPHEME_BIT};
pub use damage::Damage;
pub use input::CellMouseEvent;
pub use modes::{CursorShape, CursorState, KittyKeyboard, Modes, OptionAsAlt};
pub use row::{Row, RowFlags, RowPool, RowView};
pub use scrollback::cold::{ColdChunk, ColdTier, StyleRun};
pub use scrollback::index::{IndexMapper, LineLocation};
pub use scrollback::Scrollback;
pub use search::{Match, SearchProgress, SearchQuery};
pub use selection::{Selection, SelectionKind, DEFAULT_WORD_SEPARATORS};
pub use style::{GraphemeTable, HyperlinkTable, StyleTable, MAX_STYLES};
pub use view::{GridView, ScrollPos};

/// Line range for text extraction.
///
/// See `docs/specs/terminal-core.md §8`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LineRange {
    pub start: AbsLine,
    pub end: AbsLine,
}

/// OSC 52 clipboard policy.
///
/// See `docs/specs/terminal-core.md §5`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Osc52Policy {
    None,
    #[default]
    Write,
    ReadWrite,
}

/// Configuration for creating a `Terminal` instance.
///
/// See `docs/architecture.md §3.2` and `docs/specs/terminal-core.md`.
#[derive(Clone, Debug)]
pub struct TerminalConfig {
    pub scrollback_lines: u32,
    pub cols: u16,
    pub rows: u16,
    pub cell_px: (u16, u16),
    pub osc52: Osc52Policy,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 10_000,
            cols: 80,
            rows: 24,
            cell_px: (8, 16),
            osc52: Osc52Policy::Write,
        }
    }
}

/// Side-effects produced during VT sequence parsing.
///
/// See `docs/architecture.md §3.2` and `docs/specs/terminal-core.md §5`.
#[non_exhaustive]
pub enum Effect {
    Title(Box<str>),
    Bell,
    Notify { title: Box<str>, body: Box<str> },
    Cwd(Box<str>),
    ClipboardSet(Box<[u8]>),
    ClipboardRequest,
    Block(BlockEvent),
    CursorShape(CursorShape),
    Hyperlink,
    ModeChanged(Modes),
    ColorQuery(u8),
}

/// Consumer sink for side-effects from terminal parsing.
///
/// See `docs/architecture.md §3.2`.
pub trait EffectSink {
    fn write_back(&mut self, bytes: &[u8]);
    fn effect(&mut self, e: Effect);
}

/// Terminal emulator state machine.
///
/// See `docs/architecture.md §3.2` and `docs/specs/terminal-core.md`.
pub struct Terminal {
    cfg: TerminalConfig,
    generation: u64,
    damage: Damage,
    modes: Modes,
    cursor_shape: CursorShape,
    cursor_blink: bool,
    kitty: KittyKeyboard,
    modify_other_keys: u8,
    option_as_alt: OptionAsAlt,
    word_separators: String,
    grid: grid::Grid,
    alt_grid: grid::Grid,
    is_alt_screen: bool,
    scrollback: Scrollback,
    /// Number of hot scrollback rows the display is scrolled above the live screen.
    display_scroll: usize,
    /// Reused decoded cold rows for `view_with_cold`; bottom rendering does not touch it.
    cold_view_rows: Vec<Row>,
    blocks: BlockList,
    styles: StyleTable,
    graphemes: GraphemeTable,
    hyperlinks: HyperlinkTable,
    row_pool: RowPool,
    parser: Parser,
    parser_in_ground: bool,
    palette: [Rgba; 256],
    default_fg: Rgba,
    default_bg: Rgba,
    current_style: Style,
    current_style_id: u16,
    selection: Option<Selection>,
}

impl Terminal {
    /// Create a new terminal instance with configuration.
    pub fn new(cfg: &TerminalConfig) -> Self {
        let palette = default_ansi_palette();
        let default_fg = Rgba(0xffffffff);
        let default_bg = Rgba(0x000000ff);

        let grid = grid::Grid::new(cfg.cols, cfg.rows);
        let alt_grid = grid::Grid::new(cfg.cols, cfg.rows);
        let scrollback = Scrollback::new(cfg.cols, cfg.rows, cfg.scrollback_lines);
        let row_pool = RowPool::new(cfg.cols);
        let damage = Damage::new(cfg.rows);

        Self {
            cfg: cfg.clone(),
            generation: 1,
            damage,
            modes: Modes::AUTOWRAP | Modes::CURSOR_VISIBLE,
            cursor_shape: CursorShape::Block,
            cursor_blink: true,
            kitty: KittyKeyboard::new(),
            modify_other_keys: 0,
            option_as_alt: OptionAsAlt::Both,
            word_separators: DEFAULT_WORD_SEPARATORS.to_string(),
            grid,
            alt_grid,
            is_alt_screen: false,
            scrollback,
            display_scroll: 0,
            cold_view_rows: Vec::new(),
            blocks: BlockList::new(),
            styles: StyleTable::new(),
            graphemes: GraphemeTable::new(),
            hyperlinks: HyperlinkTable::new(),
            row_pool,
            parser: Parser::new(),
            parser_in_ground: true,
            palette,
            default_fg,
            default_bg,
            current_style: Style::default(),
            current_style_id: 0,
            selection: None,
        }
    }

    /// Advance terminal state by parsing incoming PTY bytes.
    pub fn advance(&mut self, bytes: &[u8], sink: &mut impl EffectSink) {
        // Keep a scrolled-up viewport anchored to the same history when new rows
        // are appended. `display_scroll` is measured from the live bottom, so it
        // must grow by the number of rows pushed while the user is away from it.
        let old_total_pushed = self.scrollback.total_pushed;
        let had_dirty = self.damage.all
            || self.damage.cursor_moved
            || self.damage.scrolled != 0
            || self.damage.rows.iter().any(|&d| d);

        {
            let mut handler = parser::ParserHandler {
                grid: &mut self.grid,
                alt_grid: &mut self.alt_grid,
                is_alt_screen: self.is_alt_screen,
                scrollback: &mut self.scrollback,
                blocks: &mut self.blocks,
                styles: &mut self.styles,
                graphemes: &mut self.graphemes,
                hyperlinks: &mut self.hyperlinks,
                row_pool: &mut self.row_pool,
                damage: &mut self.damage,
                modes: &mut self.modes,
                kitty: &mut self.kitty,
                modify_other_keys: &mut self.modify_other_keys,
                cursor_shape: &mut self.cursor_shape,
                cursor_blink: &mut self.cursor_blink,
                current_style: self.current_style,
                current_style_id: self.current_style_id,
                sink,
                cfg: &self.cfg,
                palette: &self.palette,
                default_fg: self.default_fg,
                default_bg: self.default_bg,
                dcs_active: false,
                dcs_bytes: Vec::new(),
                in_ground: self.parser_in_ground,
            };

            let mut idx = 0;
            let len = bytes.len();
            while idx < len {
                let b = bytes[idx];
                if handler.in_ground && !handler.dcs_active {
                    if (0x20..=0x7e).contains(&b) && handler.can_fast_ascii() {
                        let max_cols = {
                            let g = if handler.is_alt_screen {
                                &*handler.alt_grid
                            } else {
                                &*handler.grid
                            };
                            (g.cols.saturating_sub(1).saturating_sub(g.cursor_col)) as usize
                        };
                        if max_cols > 0 {
                            let limit = (idx + max_cols).min(len);
                            let mut end = idx + 1;
                            while end < limit {
                                let nb = bytes[end];
                                if !(0x20..=0x7e).contains(&nb) {
                                    break;
                                }
                                end += 1;
                            }
                            handler.print_ascii_slice(&bytes[idx..end]);
                            idx = end;
                            continue;
                        }
                    } else if b == b'\n' || b == b'\r' {
                        vte::Perform::execute(&mut handler, b);
                        idx += 1;
                        continue;
                    }
                }
                if b == 0x1b || b >= 0x80 {
                    handler.in_ground = false;
                }
                self.parser.advance(&mut handler, &bytes[idx..idx + 1]);
                idx += 1;
            }

            self.is_alt_screen = handler.is_alt_screen;
            self.current_style = handler.current_style;
            self.current_style_id = handler.current_style_id;
            self.parser_in_ground = handler.in_ground;
        }
        let pushed = self
            .scrollback
            .total_pushed
            .saturating_sub(old_total_pushed);
        if self.display_scroll > 0 {
            self.display_scroll = self.display_scroll.saturating_add(pushed as usize);
        }
        self.clamp_display_scroll();

        // Bumping generation: suppressed while ?2026 synchronized output is active
        if !self.modes.contains(Modes::SYNC_OUTPUT) {
            let has_dirty = self.damage.all
                || self.damage.cursor_moved
                || self.damage.scrolled != 0
                || self.damage.rows.iter().any(|&d| d);
            if has_dirty && (!had_dirty || !bytes.is_empty()) {
                self.generation += 1;
            }
        }
    }

    /// Resize grid dimensions and pixel geometry.
    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) {
        self.cfg.cols = cols;
        self.cfg.rows = rows;
        self.cfg.cell_px = cell_px;

        if self.is_alt_screen {
            reflow::resize_alt_screen(&mut self.alt_grid, cols, rows, &mut self.row_pool);
        } else {
            reflow::reflow_primary(
                &mut self.grid,
                &mut self.scrollback,
                &mut self.blocks,
                cols,
                rows,
                &mut self.row_pool,
            );
        }

        self.scrollback.resize(cols, rows);
        self.clamp_display_scroll();
        self.damage = Damage::new(rows);
        self.generation += 1;
    }

    /// Generation counter incremented on any visible state change.
    #[inline]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Current damage accumulated since last `reset_damage`.
    #[inline]
    pub fn damage(&self) -> &Damage {
        &self.damage
    }

    /// Reset damage after presentation.
    #[inline]
    pub fn reset_damage(&mut self) {
        self.damage.reset();
    }

    /// Iterate over visible rows for a given scroll position.
    pub fn view(&self, scroll: ScrollPos) -> GridView<'_> {
        if self.is_alt_screen {
            return GridView::Screen {
                rows: &self.alt_grid.rows,
                idx: 0,
            };
        }

        match scroll {
            ScrollPos::Bottom => GridView::Screen {
                rows: &self.grid.rows,
                idx: 0,
            },
            ScrollPos::Lines(lines) => {
                let screen_rows = self.grid.rows.len();
                let hot_len = self.scrollback.hot_len();
                let lines = usize::try_from(lines).unwrap_or(usize::MAX).min(hot_len);

                if lines == 0 {
                    return GridView::Screen {
                        rows: &self.grid.rows,
                        idx: 0,
                    };
                }

                let mut views = Vec::with_capacity(screen_rows);
                // Treat history followed by the live screen as one row sequence and
                // select the screen-sized window ending `lines` rows before bottom.
                let start = hot_len.saturating_sub(lines);
                for i in start..start.saturating_add(screen_rows) {
                    if i < hot_len {
                        if let Some(r) = self.scrollback.get_hot(i) {
                            views.push(RowView {
                                cells: &r.cells,
                                wrapped: r.flags.contains(RowFlags::WRAPPED),
                            });
                        }
                    } else if let Some(r) = self.grid.rows.get(i - hot_len) {
                        views.push(RowView {
                            cells: &r.cells,
                            wrapped: r.flags.contains(RowFlags::WRAPPED),
                        });
                    }
                }

                GridView::Scrolled { views, idx: 0 }
            }
        }
    }

    /// Iterate a viewport that can include cold scrollback.
    ///
    /// Unlike `view`, this method requires mutable access because cold logical
    /// lines are decoded into the bounded per-tab chunk cache and a viewport-sized
    /// scratch buffer. `ScrollPos::Lines` counts physical rows above the live
    /// screen. Bottom rendering should continue to use `view(ScrollPos::Bottom)`.
    pub fn view_with_cold(&mut self, scroll: ScrollPos) -> GridView<'_> {
        if self.is_alt_screen {
            return GridView::Screen {
                rows: &self.alt_grid.rows,
                idx: 0,
            };
        }

        let cols = self.cfg.cols;
        let screen_rows = self.grid.rows.len();
        let hot_len = self.scrollback.hot_len();
        let lines = match scroll {
            ScrollPos::Bottom => 0,
            ScrollPos::Lines(lines) => usize::try_from(lines).unwrap_or(usize::MAX),
        };
        if lines == 0 {
            return GridView::Screen {
                rows: &self.grid.rows,
                idx: 0,
            };
        }

        let mut cold_len = 0usize;
        for chunk in &self.scrollback.cold_tier.chunks {
            cold_len = cold_len.saturating_add(chunk.rows_at_width(cols) as usize);
        }
        let pending_chunk = self.scrollback.cold_builder.snapshot_chunk();
        let pending_rows = pending_chunk
            .as_ref()
            .map_or(0, |chunk| chunk.rows_at_width(cols) as usize);
        cold_len = cold_len.saturating_add(pending_rows);

        let total = cold_len.saturating_add(hot_len).saturating_add(screen_rows);
        let start = total
            .saturating_sub(screen_rows)
            .saturating_sub(lines.min(total.saturating_sub(screen_rows)));
        let end = start.saturating_add(screen_rows).min(total);

        // Only materialize cold rows intersecting this viewport. Sealed chunks
        // use the existing four-entry decoded LRU; builder data is bounded to
        // one chunk and decoded only when the viewport reaches it.
        self.cold_view_rows.clear();
        let cold_end = end.min(cold_len);
        if start < cold_end {
            let mut chunk_start = 0usize;
            for chunk_idx in 0..self.scrollback.cold_tier.chunks.len() {
                let chunk_rows =
                    self.scrollback.cold_tier.chunks[chunk_idx].rows_at_width(cols) as usize;
                let chunk_end = chunk_start.saturating_add(chunk_rows);
                let overlap_start = start.max(chunk_start);
                let overlap_end = cold_end.min(chunk_end);
                if overlap_start < overlap_end {
                    if let Some(rows) = self.scrollback.cold_tier.get_decoded_rows(
                        chunk_idx,
                        cols,
                        &mut self.styles,
                        &mut self.graphemes,
                    ) {
                        self.cold_view_rows.extend_from_slice(
                            &rows[overlap_start - chunk_start..overlap_end - chunk_start],
                        );
                    }
                }
                chunk_start = chunk_end;
            }

            let pending_start = chunk_start;
            let pending_end = pending_start.saturating_add(pending_rows);
            let overlap_start = start.max(pending_start);
            let overlap_end = cold_end.min(pending_end);
            if overlap_start < overlap_end {
                if let Some(chunk) = pending_chunk {
                    let rows = chunk.decode_all_rows(cols, &mut self.styles, &mut self.graphemes);
                    self.cold_view_rows.extend_from_slice(
                        &rows[overlap_start - pending_start..overlap_end - pending_start],
                    );
                }
            }
        }

        let mut views = Vec::with_capacity(screen_rows);
        for idx in start..end {
            if idx < cold_len {
                let row = &self.cold_view_rows[idx - start];
                views.push(RowView {
                    cells: &row.cells,
                    wrapped: row.flags.contains(RowFlags::WRAPPED),
                });
            } else if idx < cold_len + hot_len {
                if let Some(row) = self.scrollback.get_hot(idx - cold_len) {
                    views.push(RowView {
                        cells: &row.cells,
                        wrapped: row.flags.contains(RowFlags::WRAPPED),
                    });
                }
            } else if let Some(row) = self.grid.rows.get(idx - cold_len - hot_len) {
                views.push(RowView {
                    cells: &row.cells,
                    wrapped: row.flags.contains(RowFlags::WRAPPED),
                });
            }
        }
        GridView::Scrolled { views, idx: 0 }
    }

    /// Active modes (alt screen, bracketed paste, mouse, etc.).
    #[inline]
    pub fn modes(&self) -> Modes {
        self.modes
    }

    /// Cursor position, shape, visibility.
    pub fn cursor(&self) -> CursorState {
        let (row, col) = if self.is_alt_screen {
            (self.alt_grid.cursor_row, self.alt_grid.cursor_col)
        } else {
            (self.grid.cursor_row, self.grid.cursor_col)
        };

        let base = self.scrollback.hot_base_line + self.scrollback.hot_len() as u64;
        let line = AbsLine(base + row as u64);

        CursorState {
            pos: AbsPos { line, col },
            shape: self.cursor_shape,
            visible: self.modes.contains(Modes::CURSOR_VISIBLE),
            blink: self.cursor_blink,
        }
    }

    /// List of command execution blocks.
    #[inline]
    pub fn blocks(&self) -> &BlockList {
        &self.blocks
    }

    /// Interned style table.
    #[inline]
    pub fn styles(&self) -> &StyleTable {
        &self.styles
    }

    /// Extract text across an absolute line range.
    pub fn text_range(&self, range: LineRange) -> String {
        selection::extract_text_range(
            range.start,
            range.end,
            &self.grid,
            &self.scrollback,
            &self.graphemes,
        )
    }

    /// Perform a backward search starting from `from`.
    pub fn search(&self, query: &SearchQuery, from: AbsPos) -> Option<Match> {
        match search::search_incremental(
            query,
            Some(from),
            usize::MAX,
            &self.grid,
            &self.scrollback,
            &self.graphemes,
        ) {
            SearchProgress::Found(m) => Some(m),
            _ => None,
        }
    }

    /// Incremental search step with line budget.
    pub fn search_step(
        &self,
        query: &SearchQuery,
        cursor: Option<AbsPos>,
        budget_lines: usize,
    ) -> SearchProgress {
        search::search_incremental(
            query,
            cursor,
            budget_lines,
            &self.grid,
            &self.scrollback,
            &self.graphemes,
        )
    }

    /// Encode key event into VT escape sequence bytes.
    pub fn encode_key(&self, ev: &KeyEvent) -> SmallVec<[u8; 16]> {
        input::encode_key(
            ev,
            self.modes,
            &self.kitty,
            self.modify_other_keys,
            self.option_as_alt,
        )
    }

    /// Encode mouse event into VT escape sequence bytes.
    pub fn encode_mouse(&self, ev: &CellMouseEvent) -> Option<SmallVec<[u8; 16]>> {
        input::encode_mouse(ev, self.modes)
    }

    /// Set theme color palette (for OSC 4/10/11 queries).
    pub fn set_palette(&mut self, palette: &[Rgba; 256], default_fg: Rgba, default_bg: Rgba) {
        self.palette = *palette;
        self.default_fg = default_fg;
        self.default_bg = default_bg;
    }

    /// Set option-as-alt macOS behavior.
    pub fn set_option_as_alt(&mut self, opt: OptionAsAlt) {
        self.option_as_alt = opt;
    }

    /// Set custom word separators for word selection.
    pub fn set_word_separators(&mut self, seps: &str) {
        self.word_separators = seps.to_string();
    }

    /// Extract command text for a block.
    pub fn block_command_text(&self, id: BlockId) -> Option<String> {
        selection::extract_block_command(
            id,
            &self.blocks,
            &self.grid,
            &self.scrollback,
            &self.graphemes,
        )
    }

    /// Extract output text for a block.
    pub fn block_output_text(&self, id: BlockId) -> Option<String> {
        selection::extract_block_output(
            id,
            &self.blocks,
            &self.grid,
            &self.scrollback,
            &self.graphemes,
        )
    }

    /// Oldest absolute line still addressable through [`Terminal::row_at`].
    #[inline]
    pub fn first_hot_line(&self) -> AbsLine {
        AbsLine(self.scrollback.hot_base_line)
    }

    /// Cells of the primary-screen row at absolute `line` (hot scrollback or live
    /// screen). Returns `None` for cold or evicted lines and lines past the screen.
    pub fn row_at(&self, line: AbsLine) -> Option<&[Cell]> {
        let idx = line.0.checked_sub(self.scrollback.hot_base_line)?;
        let idx = usize::try_from(idx).ok()?;
        let hot_len = self.scrollback.hot_len();
        if idx < hot_len {
            self.scrollback.get_hot(idx).map(|r| &r.cells[..])
        } else {
            self.grid.rows.get(idx - hot_len).map(|r| &r.cells[..])
        }
    }

    /// Total number of lines currently retained (hot scrollback + visible screen).
    pub fn total_lines(&self) -> u64 {
        self.scrollback.hot_base_line
            + self.scrollback.hot_len() as u64
            + self.grid.rows.len() as u64
    }

    /// Resolve a grapheme-cluster index stored in a cell (see `Cell::grapheme_index`).
    #[inline]
    pub fn grapheme(&self, idx: u32) -> Option<&str> {
        self.graphemes.get(idx)
    }

    pub fn cols(&self) -> u16 {
        self.cfg.cols
    }

    pub fn rows(&self) -> u16 {
        self.cfg.rows
    }

    pub fn is_alt_screen(&self) -> bool {
        self.is_alt_screen
    }

    pub fn cursor_screen_pos(&self) -> (u16, u16) {
        if self.is_alt_screen {
            (self.alt_grid.cursor_col, self.alt_grid.cursor_row)
        } else {
            (self.grid.cursor_col, self.grid.cursor_row)
        }
    }

    pub fn scrollback_len(&self) -> usize {
        (self.total_lines().saturating_sub(self.cfg.rows as u64)) as usize
    }

    pub fn cell_at(&self, col: u16, row: u16) -> Option<&Cell> {
        let grid = if self.is_alt_screen {
            &self.alt_grid
        } else {
            &self.grid
        };
        grid.rows
            .get(row as usize)
            .and_then(|r| r.cells.get(col as usize))
    }

    pub fn selection(&self) -> Option<&Selection> {
        self.selection.as_ref()
    }

    pub fn set_selection(&mut self, sel: Option<Selection>) {
        self.selection = sel;
    }

    pub fn select_word(&mut self, pos: AbsPos) {
        self.selection = Some(Selection {
            kind: SelectionKind::Word,
            anchor: pos,
            head: pos,
        });
    }

    pub fn select_line(&mut self, abs_line: u64) {
        self.selection = Some(Selection {
            kind: SelectionKind::Line,
            anchor: AbsPos::new(abs_line, 0),
            head: AbsPos::new(abs_line, self.cols().saturating_sub(1)),
        });
    }

    pub fn selection_text(&self, sel: &Selection) -> String {
        let (start, end) = if sel.anchor <= sel.head {
            (sel.anchor, sel.head)
        } else {
            (sel.head, sel.anchor)
        };
        selection::extract_text_range(
            start.line,
            AbsLine(end.line.0 + 1),
            &self.grid,
            &self.scrollback,
            &self.graphemes,
        )
    }

    pub fn scroll_display(&mut self, delta: i32) {
        if self.is_alt_screen || delta == 0 {
            return;
        }

        let old = self.display_scroll;
        self.display_scroll = if delta > 0 {
            self.display_scroll.saturating_add(delta as usize)
        } else {
            self.display_scroll
                .saturating_sub(delta.unsigned_abs() as usize)
        }
        .min(self.retained_history_rows());

        if old != self.display_scroll {
            self.damage.all = true;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Current persistent display offset, in rows above the live screen.
    #[inline]
    pub fn display_scroll(&self) -> usize {
        if self.is_alt_screen {
            0
        } else {
            self.display_scroll.min(self.retained_history_rows())
        }
    }

    /// Scroll position suitable for passing to `view` or a renderer cache.
    #[inline]
    pub fn display_scroll_pos(&self) -> ScrollPos {
        match self.display_scroll() {
            0 => ScrollPos::Bottom,
            lines => ScrollPos::Lines(lines as u64),
        }
    }

    /// Iterate the current persistent display view.
    #[inline]
    pub fn display_view(&self) -> GridView<'_> {
        self.view(self.display_scroll_pos())
    }

    /// Iterate the persistent display view, including cold history when needed.
    /// Use this in UI code that supports scrolling beyond the hot ring.
    #[inline]
    pub fn display_view_with_cold(&mut self) -> GridView<'_> {
        self.view_with_cold(self.display_scroll_pos())
    }

    fn clamp_display_scroll(&mut self) {
        self.display_scroll = self.display_scroll.min(self.retained_history_rows());
    }

    fn retained_history_rows(&self) -> usize {
        let cols = self.cfg.cols;
        let cold_rows = self
            .scrollback
            .cold_tier
            .chunks
            .iter()
            .map(|chunk| chunk.rows_at_width(cols) as usize)
            .sum::<usize>();
        let pending_rows = self
            .scrollback
            .cold_builder
            .snapshot_chunk()
            .map_or(0, |chunk| chunk.rows_at_width(cols) as usize);
        cold_rows
            .saturating_add(pending_rows)
            .saturating_add(self.scrollback.hot_len())
    }
}

impl PtyConsumer for Terminal {
    type Effect = Effect;

    fn consume(&mut self, bytes: &[u8], write_back: &mut Vec<u8>, effects: &mut Vec<Self::Effect>) {
        struct Sink<'a> {
            wb: &'a mut Vec<u8>,
            eff: &'a mut Vec<Effect>,
        }
        impl<'a> EffectSink for Sink<'a> {
            fn write_back(&mut self, bytes: &[u8]) {
                self.wb.extend_from_slice(bytes);
            }
            fn effect(&mut self, e: Effect) {
                self.eff.push(e);
            }
        }
        self.advance(
            bytes,
            &mut Sink {
                wb: write_back,
                eff: effects,
            },
        );
    }

    fn resize(&mut self, cols: u16, rows: u16, px: (u16, u16)) {
        self.resize(cols, rows, px);
    }

    fn generation(&self) -> u64 {
        self.generation()
    }
}

/// Generate default 256 ANSI colors palette.
fn default_ansi_palette() -> [Rgba; 256] {
    let mut p = [Rgba(0); 256];

    // Standard 16 colors
    let standard: [u32; 16] = [
        0x000000ff, // 0 Black
        0xcd0000ff, // 1 Red
        0x00cd00ff, // 2 Green
        0xcdcd00ff, // 3 Yellow
        0x0000eeff, // 4 Blue
        0xcd00cdff, // 5 Magenta
        0x00cdcdff, // 6 Cyan
        0xe5e5e5ff, // 7 White
        0x7f7f7fff, // 8 Bright Black
        0xff0000ff, // 9 Bright Red
        0x00ff00ff, // 10 Bright Green
        0xffff00ff, // 11 Bright Yellow
        0x5c5cffff, // 12 Bright Blue
        0xff00ffff, // 13 Bright Magenta
        0x00ffffff, // 14 Bright Cyan
        0xffffffff, // 15 Bright White
    ];
    for (i, &c) in standard.iter().enumerate() {
        p[i] = Rgba(c);
    }

    // 6x6x6 color cube (indices 16..231)
    let steps: [u8; 6] = [0, 95, 135, 175, 215, 255];
    for r in 0..6 {
        for g in 0..6 {
            for b in 0..6 {
                let idx = 16 + 36 * r + 6 * g + b;
                let val = ((steps[r] as u32) << 24)
                    | ((steps[g] as u32) << 16)
                    | ((steps[b] as u32) << 8)
                    | 0xff;
                p[idx] = Rgba(val);
            }
        }
    }

    // 24 grayscale colors (indices 232..255)
    for i in 0..24 {
        let val = 8 + i as u8 * 10;
        let c = ((val as u32) << 24) | ((val as u32) << 16) | ((val as u32) << 8) | 0xff;
        p[232 + i] = Rgba(c);
    }

    p
}

#[cfg(test)]
mod tests {
    use super::*;

    struct RecordingSink {
        written: Vec<u8>,
        effects: Vec<Effect>,
    }

    impl EffectSink for RecordingSink {
        fn write_back(&mut self, bytes: &[u8]) {
            self.written.extend_from_slice(bytes);
        }
        fn effect(&mut self, e: Effect) {
            self.effects.push(e);
        }
    }

    #[test]
    fn test_terminal_creation_and_view() {
        let cfg = TerminalConfig::default();
        let term = Terminal::new(&cfg);
        assert_eq!(term.generation(), 1);
        let view = term.view(ScrollPos::Bottom);
        assert_eq!(view.count(), cfg.rows as usize);
    }

    #[test]
    fn test_terminal_advance_bumps_generation() {
        let cfg = TerminalConfig::default();
        let mut term = Terminal::new(&cfg);
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };
        term.advance(b"abc", &mut sink);
        assert_eq!(term.generation(), 2);
    }

    #[test]
    fn display_scrolling_persists_and_clamps_to_available_history() {
        let cfg = TerminalConfig {
            cols: 4,
            rows: 2,
            scrollback_lines: 100,
            ..TerminalConfig::default()
        };
        let mut term = Terminal::new(&cfg);
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };
        term.advance(b"1\r\n2\r\n3\r\n4\r\n", &mut sink);

        let bottom: Vec<_> = term.display_view().collect();
        assert_eq!(bottom.len(), 2);
        assert_eq!(bottom[0].cells[0].ch, b'4' as u32);

        term.scroll_display(1);
        assert_eq!(term.display_scroll(), 1);
        assert_eq!(term.display_scroll_pos(), ScrollPos::Lines(1));
        let scrolled: Vec<_> = term.display_view().collect();
        assert_eq!(scrolled.len(), 2);
        assert_eq!(scrolled[0].cells[0].ch, b'3' as u32);

        term.scroll_display(i32::MAX);
        assert_eq!(
            term.display_scroll(),
            term.scrollback_len().min(term.scrollback.hot_len())
        );
        term.scroll_display(i32::MIN);
        assert_eq!(term.display_scroll(), 0);
        assert_eq!(term.display_scroll_pos(), ScrollPos::Bottom);
    }

    #[test]
    fn scrolled_view_stays_anchored_when_new_output_arrives() {
        let cfg = TerminalConfig {
            cols: 4,
            rows: 2,
            scrollback_lines: 100,
            ..TerminalConfig::default()
        };
        let mut term = Terminal::new(&cfg);
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };

        term.advance(b"1\r\n2\r\n3\r\n4\r\n", &mut sink);
        term.scroll_display(1);
        let before: Vec<_> = term.display_view().map(|row| row.cells[0].ch).collect();
        assert_eq!(before, vec![b'3' as u32, b'4' as u32]);

        term.advance(b"5\r\n", &mut sink);
        let after: Vec<_> = term.display_view().map(|row| row.cells[0].ch).collect();
        assert_eq!(after, before);
    }

    #[test]
    fn cached_view_includes_sealed_and_active_cold_history_by_physical_row() {
        let cfg = TerminalConfig {
            cols: 3,
            rows: 2,
            scrollback_lines: 100,
            ..TerminalConfig::default()
        };
        let mut term = Terminal::new(&cfg);

        let mut sealed = scrollback::cold::ColdBuilder::new(0);
        let mut row = Row::new(3);
        for (col, ch) in "old".chars().enumerate() {
            row.get_mut(col as u16).ch = ch as u32;
        }
        row.flags.insert(RowFlags::WRAPPED);
        row.update_len();
        sealed.push_row(&row, &term.styles, &term.graphemes);
        let mut row = Row::new(3);
        row.get_mut(0).ch = 'e' as u32;
        row.get_mut(1).ch = 'r' as u32;
        row.update_len();
        sealed.push_row(&row, &term.styles, &term.graphemes);
        term.scrollback.cold_tier.push_chunk(sealed.seal());

        // This logical line remains in the active builder, as happens before a
        // chunk reaches its sealing threshold.
        let mut row = Row::new(3);
        for (col, ch) in "new".chars().enumerate() {
            row.get_mut(col as u16).ch = ch as u32;
        }
        row.flags.insert(RowFlags::WRAPPED);
        row.update_len();
        term.scrollback
            .cold_builder
            .push_row(&row, &term.styles, &term.graphemes);
        let mut row = Row::new(3);
        row.get_mut(0).ch = 'e' as u32;
        row.get_mut(1).ch = 'r' as u32;
        row.update_len();
        term.scrollback
            .cold_builder
            .push_row(&row, &term.styles, &term.graphemes);

        let mut hot = Row::new(3);
        hot.get_mut(0).ch = 'H' as u32;
        hot.update_len();
        term.scrollback.hot_ring.push_back(hot);

        assert_eq!(term.view_with_cold(ScrollPos::Bottom).count(), 2);
        assert!(term.cold_view_rows.is_empty());

        let rows: Vec<_> = term
            .view_with_cold(ScrollPos::Lines(3))
            .map(|row| row.cells[0].ch)
            .collect();
        // Three rows above the screen are the active-builder wrap rows and the
        // hot row. The viewport therefore crosses both cold/hot boundaries.
        assert_eq!(rows, vec![b'n' as u32, b'e' as u32]);

        let rows: Vec<_> = term
            .view_with_cold(ScrollPos::Lines(2))
            .map(|row| row.cells[0].ch)
            .collect();
        assert_eq!(rows, vec![b'e' as u32, b'H' as u32]);
        assert_eq!(term.cold_view_rows.len(), 1);

        let rows: Vec<_> = term
            .view_with_cold(ScrollPos::Lines(4))
            .map(|row| row.cells[0].ch)
            .collect();
        assert_eq!(rows, vec![b'e' as u32, b'n' as u32]);

        let rows: Vec<_> = term
            .view_with_cold(ScrollPos::Lines(5))
            .map(|row| row.cells[0].ch)
            .collect();
        assert_eq!(rows, vec![b'o' as u32, b'e' as u32]);
        assert!(term.cold_view_rows.len() <= cfg.rows as usize);

        term.scroll_display(5);
        assert_eq!(term.display_scroll(), 5);
        let rows: Vec<_> = term
            .display_view_with_cold()
            .map(|row| row.cells[0].ch)
            .collect();
        assert_eq!(rows, vec![b'o' as u32, b'e' as u32]);
    }

    #[test]
    fn test_sgr_formatting_and_colors() {
        let cfg = TerminalConfig::default();
        let mut term = Terminal::new(&cfg);
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };

        // Bold red text: \x1b[1;31mHello\x1b[0m
        term.advance(b"\x1b[1;31mHello\x1b[0m", &mut sink);

        let view = term.view(ScrollPos::Bottom);
        let first_row = view.into_iter().next().unwrap();
        let first_cell = &first_row.cells[0];

        assert_eq!(first_cell.ch, 'H' as u32);
        let style = term.styles().get(first_cell.style).unwrap();
        assert!(style.attrs.contains(Attrs::BOLD));
        assert_eq!(style.fg, Color::Indexed(1));
    }

    #[test]
    fn test_alt_screen_switch() {
        let cfg = TerminalConfig::default();
        let mut term = Terminal::new(&cfg);
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };

        // Enter alt screen: \x1b[?1049h
        term.advance(b"\x1b[?1049hAlt text", &mut sink);
        assert!(term.is_alt_screen());
        assert!(term.modes().contains(Modes::ALT_SCREEN));

        // Exit alt screen: \x1b[?1049l
        term.advance(b"\x1b[?1049l", &mut sink);
        assert!(!term.is_alt_screen());
        assert!(!term.modes().contains(Modes::ALT_SCREEN));
    }

    #[test]
    fn test_row_at_addresses_scrollback_and_screen() {
        let mut term = Terminal::new(&TerminalConfig {
            cols: 10,
            rows: 2,
            scrollback_lines: 100,
            cell_px: (8, 16),
            osc52: Osc52Policy::Write,
        });
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };
        term.advance(b"a\r\nb\r\nc", &mut sink);
        let first = term.first_hot_line();
        let ch = |line: u64| {
            term.row_at(AbsLine(first.0 + line))
                .and_then(|cells| cells.first())
                .map(|c| c.ch)
        };
        assert_eq!(ch(0), Some('a' as u32));
        assert_eq!(ch(1), Some('b' as u32));
        assert_eq!(ch(2), Some('c' as u32));
        assert!(term.row_at(AbsLine(first.0 + 3)).is_none());
    }

    #[test]
    fn test_osc_133_blocks() {
        let cfg = TerminalConfig::default();
        let mut term = Terminal::new(&cfg);
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };

        // Prompt start: OSC 133;A ST
        term.advance(b"\x1b]133;A\x1b\\", &mut sink);
        assert_eq!(term.blocks().len(), 1);

        // Command start: OSC 133;C ST
        term.advance(b"\x1b]133;C;cmdline_url=echo%20hi\x1b\\", &mut sink);

        // Output text
        term.advance(b"hi\r\n", &mut sink);

        // Command finished: OSC 133;D;0 ST
        term.advance(b"\x1b]133;D;0\x1b\\", &mut sink);

        let block = term.blocks().iter().next().unwrap();
        assert_eq!(block.exit, Some(0));
        assert_eq!(block.command.as_deref(), Some("echo hi"));
    }

    #[test]
    fn test_synchronized_output_suppresses_generation() {
        let cfg = TerminalConfig::default();
        let mut term = Terminal::new(&cfg);
        let mut sink = RecordingSink {
            written: Vec::new(),
            effects: Vec::new(),
        };

        // Enable ?2026 synchronized output
        term.advance(b"\x1b[?2026h", &mut sink);
        let gen_before = term.generation();

        // Output inside synchronized block
        term.advance(b"sync content", &mut sink);
        assert_eq!(term.generation(), gen_before);

        // Reset ?2026
        term.advance(b"\x1b[?2026l", &mut sink);
        assert!(term.generation() > gen_before);
    }
}
