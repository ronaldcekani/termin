//! Terminal core state machine (`termin-vt`).
//!
//! Pure-Rust terminal state machine: grid, scrollback, modes, OSC, blocks.
//! See `docs/architecture.md §3.2` and `docs/specs/terminal-core.md`.

use bitflags::bitflags;
use smallvec::SmallVec;
use std::collections::VecDeque;
use termin_core::{BlockId, KeyEvent, MouseEvent, PtyConsumer};

/// Absolute line index in scrollback history.
///
/// See `docs/specs/terminal-core.md §3.3`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct AbsLine(pub u64);

/// Absolute position (line, column) in terminal space.
///
/// See `docs/specs/terminal-core.md §3.3`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AbsPos {
    pub line: AbsLine,
    pub col: u16,
}

bitflags! {
    /// Per-cell flags (wide char, wrap, protection, etc.).
    ///
    /// See `docs/specs/terminal-core.md §2.1`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct CellFlags: u16 {
        /// First half of a 2-column wide character.
        const WIDE        = 1 << 0;
        /// Second half of a 2-column wide character (renders nothing).
        const WIDE_SPACER = 1 << 1;
        /// Soft-wrap continuing into next row.
        const WRAPLINE    = 1 << 2;
        /// DECSCA protected cell.
        const PROTECTED   = 1 << 3;
        /// Dirty flag hint.
        const DIRTY_HINT  = 1 << 4;
    }
}

/// A single cell in the terminal grid (8 bytes).
///
/// See `docs/specs/terminal-core.md §2.1`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// Unicode scalar, or high-bit grapheme table index.
    pub ch: u32,
    /// Index into `StyleTable`.
    pub style: u16,
    /// Cell flags (wide, wrap, protected).
    pub flags: CellFlags,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ' as u32,
            style: 0,
            flags: CellFlags::empty(),
        }
    }
}

/// Color specification for cell style.
///
/// See `docs/specs/terminal-core.md §2.2`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

bitflags! {
    /// Text style attributes (bold, italic, underline, etc.).
    ///
    /// See `docs/specs/terminal-core.md §2.2`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Attrs: u16 {
        const BOLD      = 1 << 0;
        const DIM       = 1 << 1;
        const ITALIC    = 1 << 2;
        const UNDERLINE = 1 << 3;
        const BLINK     = 1 << 4;
        const INVERSE   = 1 << 5;
        const HIDDEN    = 1 << 6;
        const STRIKE    = 1 << 7;
        const OVERLINE  = 1 << 8;
    }
}

/// Cell visual style.
///
/// See `docs/specs/terminal-core.md §2.2`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub ul_color: Color,
    pub attrs: Attrs,
    pub link: u16,
}

/// Interned style table.
///
/// See `docs/specs/terminal-core.md §2.2`.
#[derive(Clone, Debug, Default)]
pub struct StyleTable {
    styles: Vec<Style>,
}

impl StyleTable {
    pub fn new() -> Self {
        Self {
            styles: vec![Style::default()],
        }
    }

    pub fn get(&self, idx: u16) -> Option<&Style> {
        self.styles.get(idx as usize)
    }

    pub fn insert(&mut self, style: Style) -> u16 {
        if let Some(pos) = self.styles.iter().position(|s| s == &style) {
            pos as u16
        } else {
            let idx = self.styles.len() as u16;
            self.styles.push(style);
            idx
        }
    }
}

/// View of a single terminal row.
///
/// See `docs/specs/terminal-core.md §2.4`.
pub struct RowView<'a> {
    pub cells: &'a [Cell],
    pub wrapped: bool,
}

/// Iterator over visible rows.
///
/// See `docs/architecture.md §3.2` and `docs/specs/terminal-core.md §1`.
pub struct GridView<'a> {
    rows: &'a [Vec<Cell>],
    idx: usize,
}

impl<'a> Iterator for GridView<'a> {
    type Item = RowView<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx < self.rows.len() {
            let row = &self.rows[self.idx];
            self.idx += 1;
            Some(RowView {
                cells: row.as_slice(),
                wrapped: false,
            })
        } else {
            None
        }
    }
}

/// Cursor shape.
///
/// See `docs/specs/terminal-core.md §4`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

/// Cursor state.
///
/// See `docs/specs/terminal-core.md §4`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub pos: AbsPos,
    pub shape: CursorShape,
    pub visible: bool,
    pub blink: bool,
}

bitflags! {
    /// Active terminal modes.
    ///
    /// See `docs/specs/terminal-core.md §4`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
    pub struct Modes: u32 {
        const ALT_SCREEN        = 1 << 0;
        const BRACKETED_PASTE   = 1 << 1;
        const MOUSE_X10         = 1 << 2;
        const MOUSE_NORMAL      = 1 << 3;
        const MOUSE_BUTTON      = 1 << 4;
        const MOUSE_ANY         = 1 << 5;
        const MOUSE_SGR         = 1 << 6;
        const MOUSE_SGR_PIXELS  = 1 << 7;
        const DECCKM            = 1 << 8;
        const SYNC_OUTPUT       = 1 << 9;
    }
}

/// Damage tracking for visible rows and cursor.
///
/// See `docs/specs/terminal-core.md §11`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Damage {
    pub cursor_moved: bool,
    pub all: bool,
    pub scrolled: i32,
}

/// Scroll position for viewing terminal contents.
///
/// See `docs/specs/terminal-core.md §3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScrollPos {
    Bottom,
    Lines(u64),
}

/// Line range for text extraction.
///
/// See `docs/specs/terminal-core.md §8`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct LineRange {
    pub start: AbsLine,
    pub end: AbsLine,
}

/// Substring or pattern search query.
///
/// See `docs/specs/terminal-core.md §9`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub text: String,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// Search match location.
///
/// See `docs/specs/terminal-core.md §9`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub start: AbsPos,
    pub end: AbsPos,
}

/// Selection kind.
///
/// See `docs/specs/terminal-core.md §8`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    Simple,
    Word,
    Line,
    Block(BlockId),
    Rect,
}

/// Selection in terminal grid.
///
/// See `docs/specs/terminal-core.md §8`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub kind: SelectionKind,
    pub anchor: AbsPos,
    pub head: AbsPos,
}

/// Mouse event translated to cell coordinates.
///
/// See `docs/specs/terminal-core.md §10`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CellMouseEvent {
    pub pos: AbsPos,
    pub event: MouseEvent,
}

/// OSC 52 clipboard policy.
///
/// See `docs/specs/terminal-core.md §5`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Osc52Policy {
    None,
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

/// FinalTerm / OSC 133 command block events.
///
/// See `docs/specs/terminal-core.md §6`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockEvent {
    PromptStarted(BlockId),
    CommandStarted(BlockId),
    CommandFinished(BlockId, i32),
}

/// A command execution block.
///
/// See `docs/specs/terminal-core.md §6.2`.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: BlockId,
    pub prompt_start: AbsLine,
    pub input_start: Option<AbsLine>,
    pub output_start: Option<AbsLine>,
    pub end: Option<AbsLine>,
    pub command: Option<Box<str>>,
    pub exit: Option<i32>,
    pub cwd: Option<Box<str>>,
}

/// List of command execution blocks.
///
/// See `docs/specs/terminal-core.md §6.2`.
#[derive(Clone, Debug, Default)]
pub struct BlockList {
    blocks: VecDeque<Block>,
}

impl BlockList {
    pub fn new() -> Self {
        Self {
            blocks: VecDeque::new(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
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
    cursor: CursorState,
    blocks: BlockList,
    styles: StyleTable,
    rows: Vec<Vec<Cell>>,
}

impl Terminal {
    /// Create a new terminal instance with configuration.
    pub fn new(cfg: &TerminalConfig) -> Self {
        let rows = vec![vec![Cell::default(); cfg.cols as usize]; cfg.rows as usize];
        Self {
            cfg: cfg.clone(),
            generation: 1,
            damage: Damage {
                cursor_moved: false,
                all: true,
                scrolled: 0,
            },
            modes: Modes::empty(),
            cursor: CursorState {
                pos: AbsPos::default(),
                shape: CursorShape::Block,
                visible: true,
                blink: true,
            },
            blocks: BlockList::new(),
            styles: StyleTable::new(),
            rows,
        }
    }

    /// Advance terminal state by parsing incoming PTY bytes.
    pub fn advance(&mut self, _bytes: &[u8], _sink: &mut impl EffectSink) {
        // Stub implementation for Phase 0
        self.generation += 1;
        self.damage.all = true;
    }

    /// Resize grid dimensions and pixel geometry.
    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) {
        self.cfg.cols = cols;
        self.cfg.rows = rows;
        self.cfg.cell_px = cell_px;
        self.rows = vec![vec![Cell::default(); cols as usize]; rows as usize];
        self.generation += 1;
        self.damage.all = true;
    }

    /// Generation counter incremented on any visible state change.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Current damage accumulated since last `reset_damage`.
    pub fn damage(&self) -> &Damage {
        &self.damage
    }

    /// Reset damage after presentation.
    pub fn reset_damage(&mut self) {
        self.damage = Damage::default();
    }

    /// Iterate over visible rows for a given scroll position.
    pub fn view(&self, _scroll: ScrollPos) -> GridView<'_> {
        GridView {
            rows: &self.rows,
            idx: 0,
        }
    }

    /// Active modes (alt screen, bracketed paste, mouse, etc.).
    pub fn modes(&self) -> Modes {
        self.modes
    }

    /// Cursor position, shape, visibility.
    pub fn cursor(&self) -> CursorState {
        self.cursor
    }

    /// List of command execution blocks.
    pub fn blocks(&self) -> &BlockList {
        &self.blocks
    }

    /// Interned style table.
    pub fn styles(&self) -> &StyleTable {
        &self.styles
    }

    /// Extract text across a line range.
    pub fn text_range(&self, _range: LineRange) -> String {
        String::new()
    }

    /// Perform a search from a starting position.
    pub fn search(&self, _query: &SearchQuery, _from: AbsPos) -> Option<Match> {
        None
    }

    /// Encode key event into VT escape sequence bytes.
    pub fn encode_key(&self, ev: &KeyEvent) -> SmallVec<[u8; 16]> {
        let mut out = SmallVec::new();
        if let termin_core::Key::Char(c) = ev.key {
            let mut buf = [0u8; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
        out
    }

    /// Encode mouse event into VT escape sequence bytes.
    pub fn encode_mouse(&self, _ev: &CellMouseEvent) -> Option<SmallVec<[u8; 16]>> {
        None
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
}
