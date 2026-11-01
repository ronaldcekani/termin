//! Keybinding table and chord lookup (`termin-app::keymap`).
//!
//! See `docs/specs/config.md §2` and `docs/specs/app-model-and-ui.md §6, §7`.

use termin_config::chord::{parse_chord, KeyChord};
use termin_config::Config;
use termin_core::{Key, KeyEvent, Mods, NamedKey};

use crate::action::Action;

/// Focus/UI condition for activating a keybinding.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WhenContext {
    Always,
    Editor,
    Terminal,
    BlockSelected,
}

impl WhenContext {
    pub fn parse(s: &str) -> Self {
        match s {
            "editor" => WhenContext::Editor,
            "terminal" => WhenContext::Terminal,
            "block_selected" => WhenContext::BlockSelected,
            _ => WhenContext::Always,
        }
    }

    /// Whether this binding condition is satisfied given current state.
    pub fn matches(self, current: WhenContext) -> bool {
        match (self, current) {
            (WhenContext::Always, _) => true,
            (a, b) => a == b,
        }
    }
}

/// A registered keybinding entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub chord: KeyChord,
    pub action: Action,
    pub when: WhenContext,
}

/// Keybinding resolver combining defaults with user configuration overlay.
#[derive(Clone, Debug)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self::new()
    }
}

impl Keymap {
    /// Create keymap populated with built-in default bindings.
    pub fn new() -> Self {
        let mut km = Self {
            bindings: Vec::new(),
        };
        km.populate_defaults();
        km
    }

    /// Build keymap with user overrides from configuration.
    pub fn from_config(config: &Config) -> Self {
        let mut km = Self::new();
        km.apply_config(config);
        km
    }

    /// Apply user configuration overrides on top of existing bindings.
    pub fn apply_config(&mut self, config: &Config) {
        for (chord_str, def) in &config.keybindings.0 {
            let chord = match parse_chord(chord_str) {
                Ok(c) => c,
                Err(_) => continue,
            };

            // Remove existing binding for this chord if any
            self.bindings.retain(|b| b.chord != chord);

            if def.is_none() {
                continue;
            }

            let action_str = def.action();
            let action = match Action::parse(action_str) {
                Ok(a) => a,
                Err(_) => {
                    // Check if it's send_text with text
                    if action_str == "send_text" {
                        Action::SendText(def.text().unwrap_or("").to_string())
                    } else {
                        continue;
                    }
                }
            };

            let when = match def.when() {
                Some(w) => WhenContext::parse(w),
                None => WhenContext::Always,
            };

            self.bindings.push(Binding {
                chord,
                action,
                when,
            });
        }
    }

    /// Find matching action for a key event in `context`.
    pub fn lookup(&self, ev: &KeyEvent, context: WhenContext) -> Option<Action> {
        let chord = KeyChord::new(ev.mods, ev.key);
        if let Some(action) = self.lookup_chord(chord, context) {
            return Some(action);
        }
        // macOS reports the shifted character for Shift chords ("D", "}"), while
        // chords are stored unshifted ("cmd+shift+d", "cmd+shift+]").
        let base = unshifted(ev.mods, ev.key);
        if base != ev.key {
            return self.lookup_chord(KeyChord::new(ev.mods, base), context);
        }
        None
    }

    fn lookup_chord(&self, chord: KeyChord, context: WhenContext) -> Option<Action> {
        // User bindings (added later) take precedence over earlier bindings
        for b in self.bindings.iter().rev() {
            if b.chord == chord && b.when.matches(context) {
                return Some(b.action.clone());
            }
        }
        None
    }

    fn add(&mut self, mods: Mods, key: Key, action: Action, when: WhenContext) {
        self.bindings.push(Binding {
            chord: KeyChord::new(mods, key),
            action,
            when,
        });
    }

    fn populate_defaults(&mut self) {
        use crate::model::Direction::*;
        use Action::*;

        let cmd = Mods::CMD;
        let cmd_shift = Mods::CMD | Mods::SHIFT;
        let cmd_alt = Mods::CMD | Mods::ALT;
        let cmd_ctrl = Mods::CMD | Mods::CTRL;
        let ctrl = Mods::CTRL;
        let ctrl_shift = Mods::CTRL | Mods::SHIFT;

        // --- Tabs ---
        self.add(cmd, Key::Char('t'), NewTab, WhenContext::Always);
        self.add(cmd, Key::Char('w'), CloseTab, WhenContext::Always);
        self.add(cmd_shift, Key::Char(']'), NextTab, WhenContext::Always);
        self.add(cmd_shift, Key::Char('['), PrevTab, WhenContext::Always);
        self.add(
            ctrl,
            Key::Named(NamedKey::Tab),
            NextTab,
            WhenContext::Always,
        );
        self.add(
            ctrl_shift,
            Key::Named(NamedKey::Tab),
            PrevTab,
            WhenContext::Always,
        );

        // --- Splits & Panes ---
        self.add(cmd, Key::Char('d'), SplitRight, WhenContext::Always);
        self.add(cmd_shift, Key::Char('d'), SplitDown, WhenContext::Always);
        self.add(
            cmd_shift,
            Key::Named(NamedKey::Enter),
            ZoomPane,
            WhenContext::Always,
        );

        // Geometric focus navigation: ⌘⌥ + arrows
        self.add(
            cmd_alt,
            Key::Named(NamedKey::ArrowLeft),
            FocusPane(Left),
            WhenContext::Always,
        );
        self.add(
            cmd_alt,
            Key::Named(NamedKey::ArrowRight),
            FocusPane(Right),
            WhenContext::Always,
        );
        self.add(
            cmd_alt,
            Key::Named(NamedKey::ArrowUp),
            FocusPane(Up),
            WhenContext::Always,
        );
        self.add(
            cmd_alt,
            Key::Named(NamedKey::ArrowDown),
            FocusPane(Down),
            WhenContext::Always,
        );

        // Split resizing: ⌘⌃ + arrows
        self.add(
            cmd_ctrl,
            Key::Named(NamedKey::ArrowLeft),
            ResizeSplit {
                dir: Left,
                percent: 5,
            },
            WhenContext::Always,
        );
        self.add(
            cmd_ctrl,
            Key::Named(NamedKey::ArrowRight),
            ResizeSplit {
                dir: Right,
                percent: 5,
            },
            WhenContext::Always,
        );
        self.add(
            cmd_ctrl,
            Key::Named(NamedKey::ArrowUp),
            ResizeSplit {
                dir: Up,
                percent: 5,
            },
            WhenContext::Always,
        );
        self.add(
            cmd_ctrl,
            Key::Named(NamedKey::ArrowDown),
            ResizeSplit {
                dir: Down,
                percent: 5,
            },
            WhenContext::Always,
        );

        // --- Windows & Workspaces ---
        self.add(cmd, Key::Char('n'), NewWindow, WhenContext::Always);
        self.add(cmd_shift, Key::Char('n'), NewWorkspace, WhenContext::Always);
        self.add(cmd, Key::Char('b'), ToggleSidebar, WhenContext::Always);
        for i in 1..=9 {
            if let Some(c) = char::from_digit(i, 10) {
                self.add(
                    cmd,
                    Key::Char(c),
                    SelectWorkspace(i as usize),
                    WhenContext::Always,
                );
            }
        }

        // --- Text & Clipboard ---
        self.add(cmd, Key::Char('c'), Copy, WhenContext::Always);
        self.add(cmd, Key::Char('v'), Paste, WhenContext::Always);
        self.add(cmd, Key::Char('a'), SelectAll, WhenContext::Always);
        self.add(cmd, Key::Char('k'), ClearBuffer, WhenContext::Always);

        // --- Search & Overlays ---
        self.add(cmd, Key::Char('f'), Find, WhenContext::Always);
        self.add(cmd, Key::Char('g'), FindNext, WhenContext::Always);
        self.add(cmd_shift, Key::Char('g'), FindPrev, WhenContext::Always);
        self.add(
            cmd_shift,
            Key::Char('p'),
            CommandPalette,
            WhenContext::Always,
        );
        self.add(cmd, Key::Char('p'), GotoPalette, WhenContext::Always);
        self.add(ctrl, Key::Char('r'), HistorySearch, WhenContext::Editor);

        // --- Zoom & Font ---
        self.add(cmd, Key::Char('+'), FontIncrease, WhenContext::Always);
        self.add(cmd, Key::Char('='), FontIncrease, WhenContext::Always);
        self.add(cmd, Key::Char('-'), FontDecrease, WhenContext::Always);
        self.add(cmd, Key::Char('0'), FontReset, WhenContext::Always);

        // --- Block & Prompt Navigation ---
        self.add(
            cmd,
            Key::Named(NamedKey::ArrowUp),
            SelectPrevBlock,
            WhenContext::Always,
        );
        self.add(
            cmd,
            Key::Named(NamedKey::ArrowDown),
            SelectNextBlock,
            WhenContext::Always,
        );
        self.add(
            cmd_shift,
            Key::Char('u'),
            JumpToAttention,
            WhenContext::Always,
        );

        // --- App ---
        self.add(cmd, Key::Char('q'), Quit, WhenContext::Always);
    }
}

/// Map a Shift-produced character back to its unshifted key (US layout for symbols).
fn unshifted(mods: Mods, key: Key) -> Key {
    let Key::Char(c) = key else {
        return key;
    };
    if !mods.contains(Mods::SHIFT) {
        return key;
    }
    let base = match c {
        'A'..='Z' => c.to_ascii_lowercase(),
        '!' => '1',
        '@' => '2',
        '#' => '3',
        '$' => '4',
        '%' => '5',
        '^' => '6',
        '&' => '7',
        '*' => '8',
        '(' => '9',
        ')' => '0',
        '_' => '-',
        '+' => '=',
        '{' => '[',
        '}' => ']',
        '|' => '\\',
        ':' => ';',
        '"' => '\'',
        '<' => ',',
        '>' => '.',
        '?' => '/',
        '~' => '`',
        _ => c,
    };
    Key::Char(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_config::KeybindingDef;

    #[test]
    fn shifted_characters_match_unshifted_chords() {
        let km = Keymap::new();
        let ev = |key, mods| KeyEvent {
            key,
            physical: 0,
            mods,
            text: None,
            repeat: false,
            pressed: true,
        };
        let cmd_shift = Mods::CMD | Mods::SHIFT;
        assert_eq!(
            km.lookup(&ev(Key::Char('D'), cmd_shift), WhenContext::Always),
            Some(Action::SplitDown)
        );
        assert_eq!(
            km.lookup(&ev(Key::Char('N'), cmd_shift), WhenContext::Always),
            Some(Action::NewWorkspace)
        );
        assert_eq!(
            km.lookup(&ev(Key::Char('}'), cmd_shift), WhenContext::Always),
            Some(Action::NextTab)
        );
        // Without Shift, an uppercase character is not rewritten.
        assert_eq!(
            km.lookup(&ev(Key::Char('d'), Mods::CMD), WhenContext::Always),
            Some(Action::SplitRight)
        );
    }

    #[test]
    fn default_keymap_lookup() {
        let km = Keymap::new();
        let ev = KeyEvent {
            key: Key::Char('t'),
            physical: 0,
            mods: Mods::CMD,
            text: None,
            repeat: false,
            pressed: true,
        };
        assert_eq!(km.lookup(&ev, WhenContext::Always), Some(Action::NewTab));

        let ev_arrow = KeyEvent {
            key: Key::Named(NamedKey::ArrowLeft),
            physical: 0,
            mods: Mods::CMD | Mods::ALT,
            text: None,
            repeat: false,
            pressed: true,
        };
        assert_eq!(
            km.lookup(&ev_arrow, WhenContext::Terminal),
            Some(Action::FocusPane(crate::model::Direction::Left))
        );
    }

    #[test]
    fn config_override() {
        let mut config = Config::default();
        config.keybindings.0.insert(
            "cmd+t".to_string(),
            KeybindingDef::Simple("new_window".to_string()),
        );
        let km = Keymap::from_config(&config);
        let ev = KeyEvent {
            key: Key::Char('t'),
            physical: 0,
            mods: Mods::CMD,
            text: None,
            repeat: false,
            pressed: true,
        };
        assert_eq!(km.lookup(&ev, WhenContext::Always), Some(Action::NewWindow));
    }
}
