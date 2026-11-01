use termin_vt::{
    Cell, Effect, EffectSink, Modes, Style, Terminal, TerminalConfig, GRAPHEME_BIT,
};

#[derive(Default)]
pub struct TestSink {
    pub writes: Vec<u8>,
    pub effects: Vec<Effect>,
}

impl EffectSink for TestSink {
    fn write_back(&mut self, bytes: &[u8]) {
        self.writes.extend_from_slice(bytes);
    }
    fn effect(&mut self, e: Effect) {
        self.effects.push(e);
    }
}

pub struct TestTerm {
    pub term: Terminal,
    pub sink: TestSink,
}

impl TestTerm {
    pub fn new(cols: u16, rows: u16) -> Self {
        let cfg = TerminalConfig {
            cols,
            rows,
            scrollback_lines: 10_000,
            cell_px: (8, 16),
            osc52: termin_vt::Osc52Policy::ReadWrite,
        };
        Self {
            term: Terminal::new(&cfg),
            sink: TestSink::default(),
        }
    }

    pub fn advance(&mut self, input: impl AsRef<[u8]>) {
        self.term.advance(input.as_ref(), &mut self.sink);
    }

    /// Cursor position as (row, col), 0-indexed.
    pub fn cursor(&self) -> (u16, u16) {
        let (col, row) = self.term.cursor_screen_pos();
        (row, col)
    }

    pub fn cursor_row(&self) -> u16 {
        self.term.cursor_screen_pos().1
    }

    pub fn cursor_col(&self) -> u16 {
        self.term.cursor_screen_pos().0
    }

    pub fn modes(&self) -> Modes {
        self.term.modes()
    }

    pub fn is_alt_screen(&self) -> bool {
        self.term.is_alt_screen()
    }

    #[allow(dead_code)]
    pub fn cell(&self, row: u16, col: u16) -> Option<&Cell> {
        self.term.cell_at(col, row)
    }

    pub fn char_at(&self, row: u16, col: u16) -> Option<char> {
        let cell = self.term.cell_at(col, row)?;
        if (cell.ch & GRAPHEME_BIT) != 0 {
            let idx = cell.ch & !GRAPHEME_BIT;
            self.term.grapheme(idx).and_then(|s| s.chars().next())
        } else {
            char::from_u32(cell.ch)
        }
    }

    pub fn row_text(&self, row: u16) -> String {
        let mut s = String::new();
        for col in 0..self.term.cols() {
            if let Some(ch) = self.char_at(row, col) {
                s.push(ch);
            }
        }
        s
    }

    pub fn row_text_trimmed(&self, row: u16) -> String {
        self.row_text(row).trim_end().to_string()
    }

    pub fn style_at(&self, row: u16, col: u16) -> Option<Style> {
        let cell = self.term.cell_at(col, row)?;
        self.term.styles().get(cell.style).copied()
    }

    pub fn last_write(&self) -> String {
        String::from_utf8_lossy(&self.sink.writes).into_owned()
    }

    pub fn take_writes(&mut self) -> String {
        let s = String::from_utf8_lossy(&self.sink.writes).into_owned();
        self.sink.writes.clear();
        s
    }

    pub fn clear_writes(&mut self) {
        self.sink.writes.clear();
    }
}
