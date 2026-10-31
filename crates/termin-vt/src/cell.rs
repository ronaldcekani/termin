//! Cell representation and attributes.
//!
//! See `docs/specs/terminal-core.md §2.1` and `§2.2`.

use bitflags::bitflags;

/// Absolute line index in scrollback history.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct AbsLine(pub u64);

/// Absolute position (line, column) in terminal space.
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

/// Bit indicating that `Cell::ch` is an index into `GraphemeTable` rather than a Unicode scalar.
pub const GRAPHEME_BIT: u32 = 1 << 31;

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
    #[inline]
    fn default() -> Self {
        Self {
            ch: ' ' as u32,
            style: 0,
            flags: CellFlags::empty(),
        }
    }
}

impl Cell {
    #[inline]
    pub const fn new(ch: char, style: u16, flags: CellFlags) -> Self {
        Self {
            ch: ch as u32,
            style,
            flags,
        }
    }

    #[inline]
    pub const fn blank(style: u16) -> Self {
        Self {
            ch: ' ' as u32,
            style,
            flags: CellFlags::empty(),
        }
    }

    #[inline]
    pub const fn is_blank(&self) -> bool {
        self.ch == ' ' as u32 && self.flags.is_empty()
    }

    #[inline]
    pub const fn is_grapheme(&self) -> bool {
        (self.ch & GRAPHEME_BIT) != 0
    }

    #[inline]
    pub const fn grapheme_index(&self) -> Option<u32> {
        if self.is_grapheme() {
            Some(self.ch & !GRAPHEME_BIT)
        } else {
            None
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
        const BOLD              = 1 << 0;
        const DIM               = 1 << 1;
        const ITALIC            = 1 << 2;
        const UNDERLINE         = 1 << 3;
        const BLINK             = 1 << 4;
        const INVERSE           = 1 << 5;
        const HIDDEN            = 1 << 6;
        const STRIKE            = 1 << 7;
        const OVERLINE          = 1 << 8;
        const DOUBLE_UNDERLINE  = 1 << 9;
        const CURLY_UNDERLINE   = 1 << 10;
        const DOTTED_UNDERLINE  = 1 << 11;
        const DASHED_UNDERLINE  = 1 << 12;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cell_size() {
        assert_eq!(std::mem::size_of::<Cell>(), 8);
        assert_eq!(std::mem::align_of::<Cell>(), 4);
    }

    #[test]
    fn test_grapheme_bit() {
        let mut cell = Cell::default();
        assert!(!cell.is_grapheme());
        assert_eq!(cell.grapheme_index(), None);

        cell.ch = GRAPHEME_BIT | 42;
        assert!(cell.is_grapheme());
        assert_eq!(cell.grapheme_index(), Some(42));
    }
}
