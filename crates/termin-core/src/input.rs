//! Input event types (keyboard, mouse, scroll).
//!
//! See `docs/architecture.md §3.1`.

use crate::geometry::Point;
use crate::small_str::SmallStr;
use bitflags::bitflags;

bitflags! {
    /// Modifier keys bitflags.
    ///
    /// See `docs/architecture.md §3.1`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct Mods: u8 {
        const SHIFT = 1 << 0;
        const CTRL  = 1 << 1;
        const ALT   = 1 << 2;
        const CMD   = 1 << 3;
        const CAPS  = 1 << 4;
        const FN    = 1 << 5;
    }
}

/// Named non-character keys.
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum NamedKey {
    Enter,
    Tab,
    Backspace,
    Escape,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    F(u8),
}

/// Logical key (character or named special key).
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Char(char),
    Named(NamedKey),
}

/// Keyboard event.
///
/// See `docs/architecture.md §3.1`.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyEvent {
    /// Logical key, layout-aware.
    pub key: Key,
    /// Platform keycode (for kitty protocol / keybinding by position).
    pub physical: u16,
    /// Active modifier keys.
    pub mods: Mods,
    /// Text produced (after dead keys/IME), None for non-text keys.
    pub text: Option<SmallStr>,
    /// Whether this is an auto-repeat event.
    pub repeat: bool,
    /// True if key down, false for release.
    pub pressed: bool,
}

/// Mouse button identifier.
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u8),
}

/// Mouse event kind.
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum MouseKind {
    Down(MouseButton),
    Up(MouseButton),
    Move,
    Drag(MouseButton),
}

/// Mouse event with position and button state.
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct MouseEvent {
    pub kind: MouseKind,
    pub pos: Point,
    pub mods: Mods,
    pub click_count: u8,
}

/// Scroll phase reported by touchpad or smooth scrolling devices.
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ScrollPhase {
    MayBegin,
    Began,
    Changed,
    Ended,
    Cancelled,
    None,
}

/// Scroll wheel or touchpad event.
///
/// See `docs/architecture.md §3.1`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScrollEvent {
    pub pos: Point,
    pub dx: f32,
    pub dy: f32,
    pub precise: bool,
    pub phase: ScrollPhase,
    pub mods: Mods,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mods_bitflags() {
        let mut m = Mods::empty();
        m.insert(Mods::CMD);
        m.insert(Mods::SHIFT);
        assert!(m.contains(Mods::CMD));
        assert!(m.contains(Mods::SHIFT));
        assert!(!m.contains(Mods::CTRL));
    }

    #[test]
    fn test_key_event() {
        let ev = KeyEvent {
            key: Key::Char('c'),
            physical: 8,
            mods: Mods::CMD,
            text: Some(SmallStr::from("c")),
            repeat: false,
            pressed: true,
        };
        assert_eq!(ev.key, Key::Char('c'));
        assert!(ev.mods.contains(Mods::CMD));
    }
}
