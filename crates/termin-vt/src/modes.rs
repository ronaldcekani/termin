//! Terminal modes and cursor configuration.
//!
//! See `docs/specs/terminal-core.md §4`.

use bitflags::bitflags;

use crate::cell::AbsPos;

bitflags! {
    /// Active terminal modes.
    ///
    /// See `docs/specs/terminal-core.md §4`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct Modes: u32 {
        /// Alternate screen active (?47, ?1047, ?1049).
        const ALT_SCREEN        = 1 << 0;
        /// Bracketed paste mode (?2004).
        const BRACKETED_PASTE   = 1 << 1;
        /// Mouse X10 compatibility mode (?9).
        const MOUSE_X10         = 1 << 2;
        /// Mouse normal tracking mode (?1000).
        const MOUSE_NORMAL      = 1 << 3;
        /// Mouse button-event tracking (?1002).
        const MOUSE_BUTTON      = 1 << 4;
        /// Mouse any-event tracking (?1003).
        const MOUSE_ANY         = 1 << 5;
        /// Mouse SGR encoding (?1006).
        const MOUSE_SGR         = 1 << 6;
        /// Mouse SGR-pixels encoding (?1016).
        const MOUSE_SGR_PIXELS  = 1 << 7;
        /// Cursor keys application mode (?1 DECCKM).
        const DECCKM            = 1 << 8;
        /// Synchronized output (?2026).
        const SYNC_OUTPUT       = 1 << 9;
        /// Auto-wrap mode (?7 DECAWM, default on).
        const AUTOWRAP          = 1 << 10;
        /// Origin mode (?6 DECOM).
        const ORIGIN            = 1 << 11;
        /// Insert/replace mode (IRM 4).
        const INSERT            = 1 << 12;
        /// Line feed / new line mode (LNM 20).
        const LINE_FEED_NEW_LINE= 1 << 13;
        /// Reverse video / screen mode (?5 DECSCNM).
        const REVERSE_VIDEO     = 1 << 14;
        /// Cursor visibility (?25 DECTCEM, default on).
        const CURSOR_VISIBLE    = 1 << 15;
        /// Focus in/out reporting (?1004).
        const FOCUS_EVENTS      = 1 << 16;
        /// Alternate scroll mode (?1007).
        const ALT_SCROLL        = 1 << 17;
        /// Keypad application mode (DECKPAM).
        const DECKPAM           = 1 << 18;
    }
}

/// Visual shape of the cursor.
///
/// See `docs/specs/terminal-core.md §4`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
}

/// Full cursor state.
///
/// See `docs/specs/terminal-core.md §4`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub pos: AbsPos,
    pub shape: CursorShape,
    pub visible: bool,
    pub blink: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            pos: AbsPos::default(),
            shape: CursorShape::Block,
            visible: true,
            blink: true,
        }
    }
}

/// Option-as-Alt behavior for macOS.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum OptionAsAlt {
    None,
    #[default]
    Both,
    Left,
    Right,
}

/// Kitty keyboard protocol flag stack.
///
/// Flags:
/// 0b1: Disambiguate escape codes
/// 0b10: Report event types (press, release, repeat)
/// 0b100: Report alternate keys
/// 0b1000: Report all keys as escape codes
/// 0b10000: Report associated text
#[derive(Clone, Debug, Default)]
pub struct KittyKeyboard {
    stack: Vec<u8>,
}

impl KittyKeyboard {
    pub fn new() -> Self {
        Self {
            stack: vec![0],
        }
    }

    #[inline]
    pub fn current_flags(&self) -> u8 {
        *self.stack.last().unwrap_or(&0)
    }

    pub fn push(&mut self, flags: u8) {
        if self.stack.len() < 32 {
            self.stack.push(flags);
        }
    }

    pub fn pop(&mut self, count: usize) {
        let count = count.max(1);
        for _ in 0..count {
            if self.stack.len() > 1 {
                self.stack.pop();
            } else {
                self.stack[0] = 0;
                break;
            }
        }
    }

    pub fn set(&mut self, flags: u8, mode: u8) {
        // mode: 1 = replace, 2 = union, 3 = difference
        let current = self.current_flags();
        let new_val = match mode {
            1 => flags,
            2 => current | flags,
            3 => current & !flags,
            _ => flags,
        };
        if let Some(top) = self.stack.last_mut() {
            *top = new_val;
        }
    }
}
