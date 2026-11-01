//! macOS keyboard input translation, modifiers, option-as-alt, and selector mapping.
//!
//! See `docs/specs/platform-macos.md §4`.

use objc2_app_kit::NSEventModifierFlags;
use termin_core::{Key, Mods, NamedKey};

/// Configures how the macOS Option key is treated for terminal shortcuts vs text composition.
///
/// See `docs/specs/platform-macos.md §4`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum OptionAsAlt {
    /// Left Option key acts as Alt / Meta; Right Option produces composed characters.
    #[default]
    Left,
    /// Right Option key acts as Alt / Meta; Left Option produces composed characters.
    Right,
    /// Both Option keys act as Alt / Meta.
    Both,
    /// Neither Option key acts as Alt (both produce composed characters).
    None,
}

impl OptionAsAlt {
    /// Parse an `OptionAsAlt` mode from string.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "left" => Self::Left,
            "right" => Self::Right,
            "both" => Self::Both,
            "none" => Self::None,
            _ => Self::Left,
        }
    }

    /// Determines whether an Option key press with given left/right state should act as Alt.
    #[inline]
    pub fn should_treat_as_alt(&self, left_alt: bool, right_alt: bool) -> bool {
        match self {
            Self::Left => left_alt,
            Self::Right => right_alt,
            Self::Both => left_alt || right_alt,
            Self::None => false,
        }
    }
}

/// Raw macOS device-dependent modifier bitmask constants.
pub mod device_modifiers {
    pub const NX_DEVICELCTLKEYMASK: usize = 0x0001;
    pub const NX_DEVICELSHIFTKEYMASK: usize = 0x0002;
    pub const NX_DEVICERSHIFTKEYMASK: usize = 0x0004;
    pub const NX_DEVICELCMDKEYMASK: usize = 0x0008;
    pub const NX_DEVICERCMDKEYMASK: usize = 0x0010;
    pub const NX_DEVICELALTKEYMASK: usize = 0x0020;
    pub const NX_DEVICERALTKEYMASK: usize = 0x0040;
    pub const NX_DEVICERCTLKEYMASK: usize = 0x2000;
}

/// Convert AppKit `NSEventModifierFlags` to `termin_core::Mods`.
pub fn extract_mods(flags: NSEventModifierFlags) -> Mods {
    let mut mods = Mods::empty();
    if flags.contains(NSEventModifierFlags::NSEventModifierFlagShift) {
        mods.insert(Mods::SHIFT);
    }
    if flags.contains(NSEventModifierFlags::NSEventModifierFlagControl) {
        mods.insert(Mods::CTRL);
    }
    if flags.contains(NSEventModifierFlags::NSEventModifierFlagOption) {
        mods.insert(Mods::ALT);
    }
    if flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand) {
        mods.insert(Mods::CMD);
    }
    if flags.contains(NSEventModifierFlags::NSEventModifierFlagCapsLock) {
        mods.insert(Mods::CAPS);
    }
    if flags.contains(NSEventModifierFlags::NSEventModifierFlagFunction) {
        mods.insert(Mods::FN);
    }
    mods
}

/// Translate a macOS virtual keycode to a `NamedKey` if it is a recognized special key.
pub fn translate_keycode(code: u16) -> Option<NamedKey> {
    match code {
        0x24 | 0x4C => Some(NamedKey::Enter), // Return or Keypad Enter
        0x30 => Some(NamedKey::Tab),          // Tab
        0x31 => Some(NamedKey::Space),        // Space
        0x33 => Some(NamedKey::Backspace),    // Backspace
        0x35 => Some(NamedKey::Escape),       // Escape
        0x75 => Some(NamedKey::Delete),       // Forward Delete
        0x7B => Some(NamedKey::ArrowLeft),    // Left
        0x7C => Some(NamedKey::ArrowRight),   // Right
        0x7D => Some(NamedKey::ArrowDown),    // Down
        0x7E => Some(NamedKey::ArrowUp),      // Up
        0x73 => Some(NamedKey::Home),         // Home
        0x77 => Some(NamedKey::End),          // End
        0x74 => Some(NamedKey::PageUp),       // Page Up
        0x79 => Some(NamedKey::PageDown),     // Page Down
        0x72 => Some(NamedKey::Insert),       // Help / Insert
        0x7A => Some(NamedKey::F(1)),
        0x78 => Some(NamedKey::F(2)),
        0x63 => Some(NamedKey::F(3)),
        0x76 => Some(NamedKey::F(4)),
        0x60 => Some(NamedKey::F(5)),
        0x61 => Some(NamedKey::F(6)),
        0x62 => Some(NamedKey::F(7)),
        0x64 => Some(NamedKey::F(8)),
        0x65 => Some(NamedKey::F(9)),
        0x6D => Some(NamedKey::F(10)),
        0x67 => Some(NamedKey::F(11)),
        0x6F => Some(NamedKey::F(12)),
        0x69 => Some(NamedKey::F(13)),
        0x6B => Some(NamedKey::F(14)),
        0x71 => Some(NamedKey::F(15)),
        0x6A => Some(NamedKey::F(16)),
        0x40 => Some(NamedKey::F(17)),
        0x4F => Some(NamedKey::F(18)),
        0x50 => Some(NamedKey::F(19)),
        0x5A => Some(NamedKey::F(20)),
        _ => None,
    }
}

/// Translate Cocoa private-use function key unicode codepoint to `NamedKey`.
pub fn translate_private_unicode(c: char) -> Option<NamedKey> {
    match c as u32 {
        0xF700 => Some(NamedKey::ArrowUp),
        0xF701 => Some(NamedKey::ArrowDown),
        0xF702 => Some(NamedKey::ArrowLeft),
        0xF703 => Some(NamedKey::ArrowRight),
        0xF704..=0xF717 => {
            let num = ((c as u32) - 0xF704 + 1) as u8;
            Some(NamedKey::F(num))
        }
        0xF727 => Some(NamedKey::Insert),
        0xF728 => Some(NamedKey::Delete),
        0xF729 => Some(NamedKey::Home),
        0xF72B => Some(NamedKey::End),
        0xF72C => Some(NamedKey::PageUp),
        0xF72D => Some(NamedKey::PageDown),
        _ => None,
    }
}

/// Map an AppKit command selector name (from `doCommandBySelector:`) to a `NamedKey`.
///
/// See `docs/specs/platform-macos.md §4`.
pub fn selector_to_named_key(sel_str: &str) -> Option<NamedKey> {
    match sel_str {
        "insertNewline:" | "insertLineBreak:" | "insertNewlineIgnoringFieldEditor:" => {
            Some(NamedKey::Enter)
        }
        "insertTab:" | "insertTabIgnoringFieldEditor:" => Some(NamedKey::Tab),
        "insertBacktab:" => Some(NamedKey::Tab),
        "deleteBackward:" | "deleteBackwardByDecomposingPreviousCharacter:" => {
            Some(NamedKey::Backspace)
        }
        "deleteForward:" => Some(NamedKey::Delete),
        "cancelOperation:" => Some(NamedKey::Escape),
        "moveUp:" | "moveUpAndModifySelection:" => Some(NamedKey::ArrowUp),
        "moveDown:" | "moveDownAndModifySelection:" => Some(NamedKey::ArrowDown),
        "moveLeft:" | "moveLeftAndModifySelection:" => Some(NamedKey::ArrowLeft),
        "moveRight:" | "moveRightAndModifySelection:" => Some(NamedKey::ArrowRight),
        "moveToBeginningOfLine:"
        | "moveToBeginningOfLineAndModifySelection:"
        | "scrollToBeginningOfDocument:"
        | "moveToBeginningOfDocument:"
        | "moveToBeginningOfParagraph:" => Some(NamedKey::Home),
        "moveToEndOfLine:"
        | "moveToEndOfLineAndModifySelection:"
        | "scrollToEndOfDocument:"
        | "moveToEndOfDocument:"
        | "moveToEndOfParagraph:" => Some(NamedKey::End),
        "pageUp:" | "scrollPageUp:" => Some(NamedKey::PageUp),
        "pageDown:" | "scrollPageDown:" => Some(NamedKey::PageDown),
        _ => None,
    }
}

/// Parse a human-readable key equivalent shortcut (e.g. "⌘⇧N", "⌘⌥W", "Ctrl+T")
/// into an AppKit key equivalent string and modifier flags.
pub fn parse_shortcut(shortcut: &str) -> (String, NSEventModifierFlags) {
    let mut flags = NSEventModifierFlags::empty();
    let mut key_chars = String::new();

    // Check for symbol-based representation
    let mut remaining = shortcut.trim();

    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix('⌘') {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagCommand);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix('⇧') {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagShift);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix('⌥') {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagOption);
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix('⌃') {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagControl);
            remaining = rest;
        } else if remaining.to_ascii_lowercase().starts_with("cmd+") {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagCommand);
            remaining = &remaining[4..];
        } else if remaining.to_ascii_lowercase().starts_with("command+") {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagCommand);
            remaining = &remaining[8..];
        } else if remaining.to_ascii_lowercase().starts_with("shift+") {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagShift);
            remaining = &remaining[6..];
        } else if remaining.to_ascii_lowercase().starts_with("alt+")
            || remaining.to_ascii_lowercase().starts_with("opt+")
            || remaining.to_ascii_lowercase().starts_with("option+")
        {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagOption);
            if remaining.to_ascii_lowercase().starts_with("option+") {
                remaining = &remaining[7..];
            } else {
                remaining = &remaining[4..];
            }
        } else if remaining.to_ascii_lowercase().starts_with("ctrl+")
            || remaining.to_ascii_lowercase().starts_with("control+")
        {
            flags.insert(NSEventModifierFlags::NSEventModifierFlagControl);
            if remaining.to_ascii_lowercase().starts_with("control+") {
                remaining = &remaining[8..];
            } else {
                remaining = &remaining[5..];
            }
        } else {
            key_chars.push_str(remaining);
            break;
        }
    }

    // Default to Command if standard shortcut has no explicit modifier specified
    if flags.is_empty() && !key_chars.is_empty() && shortcut.contains('⌘') {
        flags.insert(NSEventModifierFlags::NSEventModifierFlagCommand);
    }

    // If key equivalent is uppercase letter and shift wasn't explicitly included, lowercase it unless Shift is set
    let final_key = if key_chars.len() == 1 {
        let ch = key_chars.chars().next().unwrap();
        if ch.is_ascii_uppercase() {
            if !flags.contains(NSEventModifierFlags::NSEventModifierFlagShift) {
                ch.to_ascii_lowercase().to_string()
            } else {
                ch.to_string()
            }
        } else {
            ch.to_string()
        }
    } else {
        key_chars
    };

    (final_key, flags)
}

/// Convert logical character or keycode to `Key`.
pub fn resolve_key(code: u16, chars: &str) -> Key {
    if let Some(named) = translate_keycode(code) {
        return Key::Named(named);
    }

    if let Some(ch) = chars.chars().next() {
        if let Some(named) = translate_private_unicode(ch) {
            return Key::Named(named);
        }
        Key::Char(ch)
    } else {
        Key::Named(NamedKey::Space)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keycode_translation() {
        assert_eq!(translate_keycode(0x24), Some(NamedKey::Enter));
        assert_eq!(translate_keycode(0x4C), Some(NamedKey::Enter));
        assert_eq!(translate_keycode(0x30), Some(NamedKey::Tab));
        assert_eq!(translate_keycode(0x31), Some(NamedKey::Space));
        assert_eq!(translate_keycode(0x33), Some(NamedKey::Backspace));
        assert_eq!(translate_keycode(0x35), Some(NamedKey::Escape));
        assert_eq!(translate_keycode(0x75), Some(NamedKey::Delete));
        assert_eq!(translate_keycode(0x7B), Some(NamedKey::ArrowLeft));
        assert_eq!(translate_keycode(0x7C), Some(NamedKey::ArrowRight));
        assert_eq!(translate_keycode(0x7D), Some(NamedKey::ArrowDown));
        assert_eq!(translate_keycode(0x7E), Some(NamedKey::ArrowUp));
        assert_eq!(translate_keycode(0x73), Some(NamedKey::Home));
        assert_eq!(translate_keycode(0x77), Some(NamedKey::End));
        assert_eq!(translate_keycode(0x74), Some(NamedKey::PageUp));
        assert_eq!(translate_keycode(0x79), Some(NamedKey::PageDown));
        assert_eq!(translate_keycode(0x7A), Some(NamedKey::F(1)));
        assert_eq!(translate_keycode(0x6F), Some(NamedKey::F(12)));
        assert_eq!(translate_keycode(0x5A), Some(NamedKey::F(20)));
        assert_eq!(translate_keycode(0x00), None); // 'a' keycode
    }

    #[test]
    fn test_private_unicode_translation() {
        assert_eq!(
            translate_private_unicode('\u{F700}'),
            Some(NamedKey::ArrowUp)
        );
        assert_eq!(
            translate_private_unicode('\u{F701}'),
            Some(NamedKey::ArrowDown)
        );
        assert_eq!(translate_private_unicode('\u{F704}'), Some(NamedKey::F(1)));
        assert_eq!(translate_private_unicode('\u{F705}'), Some(NamedKey::F(2)));
        assert_eq!(
            translate_private_unicode('\u{F728}'),
            Some(NamedKey::Delete)
        );
        assert_eq!(translate_private_unicode('a'), None);
    }

    #[test]
    fn test_selector_mapping() {
        assert_eq!(
            selector_to_named_key("insertNewline:"),
            Some(NamedKey::Enter)
        );
        assert_eq!(selector_to_named_key("insertTab:"), Some(NamedKey::Tab));
        assert_eq!(
            selector_to_named_key("deleteBackward:"),
            Some(NamedKey::Backspace)
        );
        assert_eq!(
            selector_to_named_key("cancelOperation:"),
            Some(NamedKey::Escape)
        );
        assert_eq!(
            selector_to_named_key("moveLeft:"),
            Some(NamedKey::ArrowLeft)
        );
        assert_eq!(
            selector_to_named_key("moveRight:"),
            Some(NamedKey::ArrowRight)
        );
        assert_eq!(
            selector_to_named_key("moveToBeginningOfLine:"),
            Some(NamedKey::Home)
        );
        assert_eq!(
            selector_to_named_key("moveToEndOfLine:"),
            Some(NamedKey::End)
        );
        assert_eq!(selector_to_named_key("unknownSelector:"), None);
    }

    #[test]
    fn test_option_as_alt_logic() {
        let left = OptionAsAlt::Left;
        assert!(left.should_treat_as_alt(true, false));
        assert!(!left.should_treat_as_alt(false, true));
        assert!(left.should_treat_as_alt(true, true));
        assert!(!left.should_treat_as_alt(false, false));

        let right = OptionAsAlt::Right;
        assert!(!right.should_treat_as_alt(true, false));
        assert!(right.should_treat_as_alt(false, true));
        assert!(right.should_treat_as_alt(true, true));

        let both = OptionAsAlt::Both;
        assert!(both.should_treat_as_alt(true, false));
        assert!(both.should_treat_as_alt(false, true));
        assert!(both.should_treat_as_alt(true, true));
        assert!(!both.should_treat_as_alt(false, false));

        let none = OptionAsAlt::None;
        assert!(!none.should_treat_as_alt(true, false));
        assert!(!none.should_treat_as_alt(false, true));
        assert!(!none.should_treat_as_alt(true, true));

        assert_eq!(OptionAsAlt::parse("left"), OptionAsAlt::Left);
        assert_eq!(OptionAsAlt::parse("Right"), OptionAsAlt::Right);
        assert_eq!(OptionAsAlt::parse("BOTH"), OptionAsAlt::Both);
        assert_eq!(OptionAsAlt::parse("none"), OptionAsAlt::None);
        assert_eq!(OptionAsAlt::parse("invalid"), OptionAsAlt::Left);
    }

    #[test]
    fn test_parse_shortcut() {
        let (key, flags) = parse_shortcut("⌘⇧N");
        assert_eq!(key, "N");
        assert!(flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand));
        assert!(flags.contains(NSEventModifierFlags::NSEventModifierFlagShift));

        let (key, flags) = parse_shortcut("⌘T");
        assert_eq!(key, "t");
        assert!(flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand));
        assert!(!flags.contains(NSEventModifierFlags::NSEventModifierFlagShift));

        let (key, flags) = parse_shortcut("Cmd+Shift+P");
        assert_eq!(key, "P");
        assert!(flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand));
        assert!(flags.contains(NSEventModifierFlags::NSEventModifierFlagShift));

        let (key, flags) = parse_shortcut("⌥W");
        assert_eq!(key, "w");
        assert!(flags.contains(NSEventModifierFlags::NSEventModifierFlagOption));
    }

    #[test]
    fn test_extract_mods() {
        let mut flags = NSEventModifierFlags::empty();
        flags.insert(NSEventModifierFlags::NSEventModifierFlagCommand);
        flags.insert(NSEventModifierFlags::NSEventModifierFlagShift);

        let mods = extract_mods(flags);
        assert!(mods.contains(Mods::CMD));
        assert!(mods.contains(Mods::SHIFT));
        assert!(!mods.contains(Mods::CTRL));
        assert!(!mods.contains(Mods::ALT));
    }
}
