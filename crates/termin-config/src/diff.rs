//! Configuration diff computation using bitflags.
//!
//! See `docs/specs/config.md §4`.

use crate::schema::Config;
use bitflags::bitflags;

bitflags! {
    /// Bitflags representing changed sections between two configurations.
    ///
    /// See `docs/specs/config.md §4`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ConfigDiff: u32 {
        const FONT          = 1 << 0;
        const THEME         = 1 << 1;
        const WINDOW        = 1 << 2;
        const SIDEBAR       = 1 << 3;
        const TERMINAL      = 1 << 4;
        const SHELL         = 1 << 5;
        const BLOCKS        = 1 << 6;
        const EDITOR        = 1 << 7;
        const SPLIT         = 1 << 8;
        const PANE          = 1 << 9;
        const NOTIFICATIONS = 1 << 10;
        const KEYBINDINGS   = 1 << 11;
        const COLORS        = 1 << 12;
    }
}

impl ConfigDiff {
    /// Whether any font properties changed.
    pub fn font_changed(&self) -> bool {
        self.contains(Self::FONT)
    }

    /// Whether the theme or color overrides changed.
    pub fn theme_changed(&self) -> bool {
        self.contains(Self::THEME) || self.contains(Self::COLORS)
    }

    /// Whether window settings changed.
    pub fn window_changed(&self) -> bool {
        self.contains(Self::WINDOW)
    }

    /// Whether sidebar settings changed.
    pub fn sidebar_changed(&self) -> bool {
        self.contains(Self::SIDEBAR)
    }

    /// Whether terminal emulation settings changed.
    pub fn terminal_changed(&self) -> bool {
        self.contains(Self::TERMINAL)
    }

    /// Whether shell settings changed.
    pub fn shell_changed(&self) -> bool {
        self.contains(Self::SHELL)
    }

    /// Whether blocks settings changed.
    pub fn blocks_changed(&self) -> bool {
        self.contains(Self::BLOCKS)
    }

    /// Whether editor settings changed.
    pub fn editor_changed(&self) -> bool {
        self.contains(Self::EDITOR)
    }

    /// Whether split settings changed.
    pub fn split_changed(&self) -> bool {
        self.contains(Self::SPLIT)
    }

    /// Whether pane settings changed.
    pub fn pane_changed(&self) -> bool {
        self.contains(Self::PANE)
    }

    /// Whether notifications settings changed.
    pub fn notifications_changed(&self) -> bool {
        self.contains(Self::NOTIFICATIONS)
    }

    /// Whether keybindings changed.
    pub fn keybindings_changed(&self) -> bool {
        self.contains(Self::KEYBINDINGS)
    }

    /// Whether color overrides changed.
    pub fn colors_changed(&self) -> bool {
        self.contains(Self::COLORS)
    }
}

/// Compute diff between two configurations, returning bitflags of modified subsystems.
///
/// See `docs/specs/config.md §4`.
pub fn diff(old: &Config, new: &Config) -> ConfigDiff {
    let mut flags = ConfigDiff::empty();

    if old.font != new.font {
        flags |= ConfigDiff::FONT;
    }
    if old.theme != new.theme {
        flags |= ConfigDiff::THEME;
    }
    if old.window != new.window {
        flags |= ConfigDiff::WINDOW;
    }
    if old.sidebar != new.sidebar {
        flags |= ConfigDiff::SIDEBAR;
    }
    if old.terminal != new.terminal {
        flags |= ConfigDiff::TERMINAL;
    }
    if old.shell != new.shell {
        flags |= ConfigDiff::SHELL;
    }
    if old.blocks != new.blocks {
        flags |= ConfigDiff::BLOCKS;
    }
    if old.editor != new.editor {
        flags |= ConfigDiff::EDITOR;
    }
    if old.split != new.split {
        flags |= ConfigDiff::SPLIT;
    }
    if old.pane != new.pane {
        flags |= ConfigDiff::PANE;
    }
    if old.notifications != new.notifications {
        flags |= ConfigDiff::NOTIFICATIONS;
    }
    if old.keybindings != new.keybindings {
        flags |= ConfigDiff::KEYBINDINGS;
    }
    if old.colors != new.colors {
        flags |= ConfigDiff::COLORS;
    }

    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diff_flags() {
        let old = Config::default();
        let mut new = Config::default();
        new.font.size = 15.0;
        new.theme.dark = "my-dark".to_string();

        let d = diff(&old, &new);
        assert!(d.font_changed());
        assert!(d.theme_changed());
        assert!(!d.window_changed());
        assert!(!d.terminal_changed());
        assert_eq!(d, ConfigDiff::FONT | ConfigDiff::THEME);
    }
}
