//! Configuration schema types and default values.
//!
//! See `docs/specs/config.md §2`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Font configuration section `[font]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
    pub ligatures: bool,
    pub thicken: bool,
    pub bold_is_bright: bool,
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: String::new(),
            size: 13.0,
            line_height: 1.2,
            ligatures: true,
            thicken: true,
            bold_is_bright: false,
        }
    }
}

/// Theme configuration section `[theme]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeConfig {
    pub name: String,
    pub light: String,
    pub dark: String,
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "auto".to_string(),
            light: "termin-light".to_string(),
            dark: "termin-dark".to_string(),
        }
    }
}

/// Window configuration section `[window]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowConfig {
    pub padding: [f32; 2],
    pub opacity: f32,
    pub blur: bool,
    pub restore_session: bool,
    pub quit_on_last_window_close: bool,
    pub colorspace: String,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            padding: [10.0, 14.0],
            opacity: 1.0,
            blur: false,
            restore_session: true,
            quit_on_last_window_close: false,
            colorspace: "srgb".to_string(),
        }
    }
}

/// Sidebar configuration section `[sidebar]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SidebarConfig {
    pub visible: bool,
    pub width: f32,
    pub show_git: bool,
    pub show_ports: bool,
    pub show_paths: bool,
}

impl Default for SidebarConfig {
    fn default() -> Self {
        Self {
            visible: true,
            width: 240.0,
            show_git: true,
            show_ports: true,
            show_paths: true,
        }
    }
}

/// Terminal emulator configuration section `[terminal]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    pub scrollback_lines: u32,
    pub cursor_style: String,
    pub cursor_blink: bool,
    pub option_as_alt: String,
    pub word_separators: String,
    pub osc52: String,
    pub warn_multiline_paste: bool,
    pub minimum_contrast: f32,
    pub confirm_close_ignore: Vec<String>,
}

pub type TerminalConfigSection = TerminalConfig;

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            scrollback_lines: 10_000,
            cursor_style: "bar".to_string(),
            cursor_blink: true,
            option_as_alt: "left".to_string(),
            word_separators: " ,│`|:\"'()[]{}<>\t".to_string(),
            osc52: "write".to_string(),
            warn_multiline_paste: true,
            minimum_contrast: 1.0,
            confirm_close_ignore: vec![
                "zsh".into(),
                "bash".into(),
                "fish".into(),
                "sh".into(),
                "login".into(),
                "tmux".into(),
            ],
        }
    }
}

/// Shell launch and integration configuration section `[shell]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub program: String,
    pub args: Vec<String>,
    pub login: bool,
    pub integration: bool,
    pub env: HashMap<String, String>,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            program: String::new(),
            args: Vec::new(),
            login: true,
            integration: true,
            env: HashMap::new(),
        }
    }
}

/// Command blocks configuration section `[blocks]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlocksConfig {
    pub enabled: bool,
    pub show_duration: bool,
    pub sticky_header: bool,
}

impl Default for BlocksConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_duration: true,
            sticky_header: true,
        }
    }
}

/// Input editor configuration section `[editor]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorConfig {
    pub type_ahead: bool,
    pub auto_complete: bool,
    pub history_same_cwd_first: bool,
    pub syntax_highlighting: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            type_ahead: true,
            auto_complete: false,
            history_same_cwd_first: true,
            syntax_highlighting: true,
        }
    }
}

/// Split behavior configuration section `[split]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SplitConfig {
    pub inherit_cwd: bool,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self { inherit_cwd: true }
    }
}

/// Pane display and focus configuration section `[pane]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneConfig {
    pub inactive_dim: f32,
    pub focus_follows_mouse: bool,
}

impl Default for PaneConfig {
    fn default() -> Self {
        Self {
            inactive_dim: 0.0,
            focus_follows_mouse: false,
        }
    }
}

/// Notifications and attention configuration section `[notifications]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationsConfig {
    pub bell: String,
    pub system: bool,
    pub long_command_secs: u64,
    pub dock_badge: bool,
}

pub type NotificationConfig = NotificationsConfig;

impl Default for NotificationsConfig {
    fn default() -> Self {
        Self {
            bell: "attention".to_string(),
            system: true,
            long_command_secs: 10,
            dock_badge: true,
        }
    }
}

/// Keybinding definition: simple string action or complex table with when/text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeybindingDef {
    Simple(String),
    Complex {
        action: String,
        #[serde(default)]
        when: Option<String>,
        #[serde(default)]
        text: Option<String>,
    },
}

impl KeybindingDef {
    /// Return the action name bound to this chord.
    pub fn action(&self) -> &str {
        match self {
            Self::Simple(a) => a.as_str(),
            Self::Complex { action, .. } => action.as_str(),
        }
    }

    /// Optional condition clause: "editor", "terminal", "block_selected", "always".
    pub fn when(&self) -> Option<&str> {
        match self {
            Self::Simple(_) => None,
            Self::Complex { when, .. } => when.as_deref(),
        }
    }

    /// Optional text payload (e.g. for `send_text` action).
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Simple(_) => None,
            Self::Complex { text, .. } => text.as_deref(),
        }
    }

    /// Whether this definition unbinds the chord ("none").
    pub fn is_none(&self) -> bool {
        self.action() == "none"
    }
}

/// Keybindings configuration section `[keybindings]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeybindingsConfig(pub HashMap<String, KeybindingDef>);

impl Default for KeybindingsConfig {
    fn default() -> Self {
        let mut map = HashMap::new();
        map.insert(
            "cmd+d".to_string(),
            KeybindingDef::Simple("split_right".to_string()),
        );
        map.insert(
            "cmd+shift+d".to_string(),
            KeybindingDef::Simple("split_down".to_string()),
        );
        Self(map)
    }
}

impl std::ops::Deref for KeybindingsConfig {
    type Target = HashMap<String, KeybindingDef>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for KeybindingsConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Color overrides configuration section `[colors]`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ColorsConfig(pub HashMap<String, String>);

impl std::ops::Deref for ColorsConfig {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ColorsConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// Root application configuration.
///
/// See `docs/specs/config.md §2`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: FontConfig,
    pub theme: ThemeConfig,
    pub window: WindowConfig,
    pub sidebar: SidebarConfig,
    pub terminal: TerminalConfig,
    pub shell: ShellConfig,
    pub blocks: BlocksConfig,
    pub editor: EditorConfig,
    pub split: SplitConfig,
    pub pane: PaneConfig,
    pub notifications: NotificationsConfig,
    pub keybindings: KeybindingsConfig,
    pub colors: ColorsConfig,
}
