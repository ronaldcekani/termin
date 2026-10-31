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
    blocks: BlockList,
    styles: StyleTable,
    graphemes: GraphemeTable,
    hyperlinks: HyperlinkTable,
    row_pool: RowPool,
    parser: Parser,
    palette: [Rgba; 256],
    default_fg: Rgba,
    default_bg: Rgba,
    current_style: Style,
    current_style_id: u16,
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
            blocks: BlockList::new(),
            styles: StyleTable::new(),
            graphemes: GraphemeTable::new(),
            hyperlinks: HyperlinkTable::new(),
            row_pool,
            parser: Parser::new(),
            palette,
            default_fg,
            default_bg,
            current_style: Style::default(),
            current_style_id: 0,
        }
    }

    /// Advance terminal state by parsing incoming PTY bytes.
    pub fn advance(&mut self, bytes: &[u8], sink: &mut impl EffectSink) {
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
            };

            self.parser.advance(&mut handler, bytes);

            self.is_alt_screen = handler.is_alt_screen;
            self.current_style = handler.current_style;
            self.current_style_id = handler.current_style_id;
        }

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
                let lines = (lines as usize).min(hot_len);

                if lines == 0 {
                    return GridView::Screen {
                        rows: &self.grid.rows,
                        idx: 0,
                    };
                }

                let mut views = Vec::with_capacity(screen_rows);
                let hot_start = hot_len.saturating_sub(lines);

                for i in hot_start..hot_len {
                    if let Some(r) = self.scrollback.get_hot(i) {
                        views.push(RowView {
                            cells: &r.cells,
                            wrapped: r.flags.contains(RowFlags::WRAPPED),
                        });
                    }
                }

                let remaining = screen_rows.saturating_sub(views.len());
                for i in 0..remaining.min(self.grid.rows.len()) {
                    let r = &self.grid.rows[i];
                    views.push(RowView {
                        cells: &r.cells,
                        wrapped: r.flags.contains(RowFlags::WRAPPED),
                    });
                }

                GridView::Scrolled { views, idx: 0 }
            }
        }
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

    /// Total number of lines currently retained (hot scrollback + visible screen).
    pub fn total_lines(&self) -> u64 {
        self.scrollback.hot_base_line
            + self.scrollback.hot_len() as u64
            + self.grid.rows.len() as u64
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
