//! Keybinding chord parser and validation.
//!
//! See `docs/specs/config.md §2` and `docs/specs/app-model-and-ui.md §7`.

use crate::diagnostic::Diagnostic;
use std::fmt;
use termin_core::{Key, Mods, NamedKey};

/// Error when parsing a keybinding chord.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChordParseError {
    Empty,
    MissingKey,
    MultipleKeys(String, String),
    UnknownModifier(String),
    UnknownKey(String),
}

impl fmt::Display for ChordParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "chord string cannot be empty"),
            Self::MissingKey => write!(f, "chord is missing key name"),
            Self::MultipleKeys(k1, k2) => {
                write!(f, "multiple keys specified in chord: '{}' and '{}'", k1, k2)
            }
            Self::UnknownModifier(m) => write!(f, "unknown modifier '{}'", m),
            Self::UnknownKey(k) => write!(f, "unknown key name '{}'", k),
        }
    }
}

impl std::error::Error for ChordParseError {}

/// A parsed key chord consisting of modifiers and a logical key.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub mods: Mods,
    pub key: Key,
}

impl KeyChord {
    /// Create a new key chord.
    pub const fn new(mods: Mods, key: Key) -> Self {
        Self { mods, key }
    }

    /// Format as canonical lowercase chord string, e.g. "cmd+shift+d".
    pub fn to_chord_string(&self) -> String {
        let mut parts = Vec::new();
        if self.mods.contains(Mods::CMD) {
            parts.push("cmd");
        }
        if self.mods.contains(Mods::CTRL) {
            parts.push("ctrl");
        }
        if self.mods.contains(Mods::ALT) {
            parts.push("alt");
        }
        if self.mods.contains(Mods::SHIFT) {
            parts.push("shift");
        }
        if self.mods.contains(Mods::FN) {
            parts.push("fn");
        }

        let key_str: String = match self.key {
            Key::Char(c) => c.to_lowercase().to_string(),
            Key::Named(nk) => match nk {
                NamedKey::Enter => "enter".into(),
                NamedKey::Tab => "tab".into(),
                NamedKey::Backspace => "backspace".into(),
                NamedKey::Escape => "escape".into(),
                NamedKey::Space => "space".into(),
                NamedKey::ArrowUp => "up".into(),
                NamedKey::ArrowDown => "down".into(),
                NamedKey::ArrowLeft => "left".into(),
                NamedKey::ArrowRight => "right".into(),
                NamedKey::Home => "home".into(),
                NamedKey::End => "end".into(),
                NamedKey::PageUp => "pageup".into(),
                NamedKey::PageDown => "pagedown".into(),
                NamedKey::Insert => "insert".into(),
                NamedKey::Delete => "delete".into(),
                NamedKey::F(n) => format!("f{}", n),
                _ => "unknown".into(),
            },
        };
        parts.push(&key_str);
        parts.join("+")
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_chord_string())
    }
}

/// Parse a keybinding chord string (e.g. "cmd+shift+d", "ctrl+r", "cmd+1").
///
/// Modifiers: `cmd` (or `super`/`command`), `ctrl` (or `control`),
/// `alt` (or `option`/`opt`), `shift`, `fn`.
pub fn parse_chord(s: &str) -> Result<KeyChord, ChordParseError> {
    let s = s.trim();
    if s.is_empty() {
        return Err(ChordParseError::Empty);
    }

    let mut mods = Mods::empty();
    let mut key: Option<Key> = None;

    // Special case for "+" key at the end, e.g. "cmd++" or "ctrl+shift++"
    let (body, trailing_plus) = if let Some(stripped) = s.strip_suffix("++") {
        (stripped, true)
    } else {
        (s, false)
    };

    let tokens = body.split('+');
    for raw_token in tokens {
        let token = raw_token.trim().to_lowercase();
        if token.is_empty() {
            continue;
        }

        match token.as_str() {
            "cmd" | "command" | "super" => mods |= Mods::CMD,
            "ctrl" | "control" => mods |= Mods::CTRL,
            "alt" | "option" | "opt" => mods |= Mods::ALT,
            "shift" => mods |= Mods::SHIFT,
            "fn" => mods |= Mods::FN,
            _ => {
                let parsed_key = parse_key_name(&token)?;
                if let Some(existing) = key {
                    return Err(ChordParseError::MultipleKeys(
                        format!("{:?}", existing),
                        token,
                    ));
                }
                key = Some(parsed_key);
            }
        }
    }

    if trailing_plus {
        if let Some(existing) = key {
            return Err(ChordParseError::MultipleKeys(
                format!("{:?}", existing),
                "+".into(),
            ));
        }
        key = Some(Key::Char('+'));
    }

    match key {
        Some(k) => Ok(KeyChord::new(mods, k)),
        None => Err(ChordParseError::MissingKey),
    }
}

fn parse_key_name(s: &str) -> Result<Key, ChordParseError> {
    match s {
        "enter" | "return" => Ok(Key::Named(NamedKey::Enter)),
        "tab" => Ok(Key::Named(NamedKey::Tab)),
        "backspace" => Ok(Key::Named(NamedKey::Backspace)),
        "escape" | "esc" => Ok(Key::Named(NamedKey::Escape)),
        "space" => Ok(Key::Named(NamedKey::Space)),
        "up" | "arrowup" | "arrow_up" => Ok(Key::Named(NamedKey::ArrowUp)),
        "down" | "arrowdown" | "arrow_down" => Ok(Key::Named(NamedKey::ArrowDown)),
        "left" | "arrowleft" | "arrow_left" => Ok(Key::Named(NamedKey::ArrowLeft)),
        "right" | "arrowright" | "arrow_right" => Ok(Key::Named(NamedKey::ArrowRight)),
        "home" => Ok(Key::Named(NamedKey::Home)),
        "end" => Ok(Key::Named(NamedKey::End)),
        "pageup" | "page_up" | "pgup" => Ok(Key::Named(NamedKey::PageUp)),
        "pagedown" | "page_down" | "pgdn" => Ok(Key::Named(NamedKey::PageDown)),
        "insert" | "ins" => Ok(Key::Named(NamedKey::Insert)),
        "delete" | "del" => Ok(Key::Named(NamedKey::Delete)),
        "plus" => Ok(Key::Char('+')),
        "minus" => Ok(Key::Char('-')),
        "equal" | "equals" => Ok(Key::Char('=')),
        _ => {
            // Function keys f1..f24
            if let Some(num_str) = s.strip_prefix('f') {
                if let Ok(n) = num_str.parse::<u8>() {
                    if (1..=24).contains(&n) {
                        return Ok(Key::Named(NamedKey::F(n)));
                    }
                }
            }

            // Single unicode character
            let mut chars = s.chars();
            if let (Some(c), None) = (chars.next(), chars.next()) {
                Ok(Key::Char(c))
            } else {
                Err(ChordParseError::UnknownKey(s.to_string()))
            }
        }
    }
}

/// Validate a single keybinding entry against an injected action validation function.
pub fn validate_keybinding_entry<F>(
    chord_str: &str,
    action: &str,
    when: Option<&str>,
    is_valid_action: F,
) -> Vec<Diagnostic>
where
    F: Fn(&str) -> bool,
{
    let mut diags = Vec::new();
    if let Err(err) = parse_chord(chord_str) {
        diags.push(
            Diagnostic::error(format!("invalid keybinding chord '{}': {}", chord_str, err))
                .with_key(chord_str),
        );
    }

    if action != "none" && !is_valid_action(action) {
        diags.push(
            Diagnostic::error(format!(
                "unknown action '{}' for keybinding chord '{}'",
                action, chord_str
            ))
            .with_key(chord_str),
        );
    }

    if let Some(w) = when {
        match w {
            "always" | "editor" | "terminal" | "block_selected" => {}
            other => {
                diags.push(
                    Diagnostic::warning(format!(
                        "unknown 'when' condition '{}' for keybinding chord '{}'",
                        other, chord_str
                    ))
                    .with_key(chord_str),
                );
            }
        }
    }

    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_chord_modifiers() {
        let chord = parse_chord("cmd+shift+d").unwrap();
        assert_eq!(chord.mods, Mods::CMD | Mods::SHIFT);
        assert_eq!(chord.key, Key::Char('d'));
        assert_eq!(chord.to_chord_string(), "cmd+shift+d");

        let chord2 = parse_chord("ctrl+r").unwrap();
        assert_eq!(chord2.mods, Mods::CTRL);
        assert_eq!(chord2.key, Key::Char('r'));

        let chord3 = parse_chord("option+up").unwrap();
        assert_eq!(chord3.mods, Mods::ALT);
        assert_eq!(chord3.key, Key::Named(NamedKey::ArrowUp));
        assert_eq!(chord3.to_chord_string(), "alt+up");
    }

    #[test]
    fn test_parse_chord_trailing_plus() {
        let chord = parse_chord("cmd++").unwrap();
        assert_eq!(chord.mods, Mods::CMD);
        assert_eq!(chord.key, Key::Char('+'));
    }

    #[test]
    fn test_validate_action_injection() {
        let validator = |action: &str| matches!(action, "split_right" | "split_down" | "send_text");
        let diags = validate_keybinding_entry("cmd+d", "split_right", Some("terminal"), validator);
        assert!(diags.is_empty());

        let diags_bad_action =
            validate_keybinding_entry("cmd+d", "non_existent_action", None, validator);
        assert_eq!(diags_bad_action.len(), 1);
        assert_eq!(
            diags_bad_action[0].level,
            crate::diagnostic::DiagnosticLevel::Error
        );

        let diags_bad_when =
            validate_keybinding_entry("cmd+d", "split_right", Some("invalid_when"), validator);
        assert_eq!(diags_bad_when.len(), 1);
        assert_eq!(
            diags_bad_when[0].level,
            crate::diagnostic::DiagnosticLevel::Warning
        );
    }
}
