//! Configuration loading, schema, and diffing (`termin-config`).
//!
//! See `docs/specs/config.md`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::Path;

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalConfigSection {
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

impl Default for TerminalConfigSection {
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationConfig {
    pub bell: String,
    pub system: bool,
    pub long_command_secs: u64,
    pub dock_badge: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            bell: "attention".to_string(),
            system: true,
            long_command_secs: 10,
            dock_badge: true,
        }
    }
}

/// Root application configuration.
///
/// See `docs/specs/config.md §2`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub font: FontConfig,
    #[serde(default)]
    pub theme: ThemeConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub sidebar: SidebarConfig,
    #[serde(default)]
    pub terminal: TerminalConfigSection,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub blocks: BlocksConfig,
    #[serde(default)]
    pub editor: EditorConfig,
    #[serde(default)]
    pub split: SplitConfig,
    #[serde(default)]
    pub pane: PaneConfig,
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub keybindings: HashMap<String, String>,
    #[serde(default)]
    pub colors: HashMap<String, String>,
}

/// Diff summary between two configurations.
///
/// See `docs/specs/config.md §4`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConfigDiff {
    pub font_changed: bool,
    pub theme_changed: bool,
    pub window_changed: bool,
    pub terminal_changed: bool,
}

/// Error type for configuration loading and parsing.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "config IO error: {}", e),
            Self::Parse(e) => write!(f, "config parse error: {}", e),
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<toml::de::Error> for ConfigError {
    fn from(e: toml::de::Error) -> Self {
        Self::Parse(e)
    }
}

/// Parse configuration from a TOML string.
///
/// See `docs/specs/config.md`.
pub fn parse(toml_str: &str) -> Result<Config, ConfigError> {
    let cfg: Config = toml::from_str(toml_str)?;
    Ok(cfg)
}

/// Load configuration from a file path.
///
/// See `docs/specs/config.md §1`.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let content = std::fs::read_to_string(path)?;
    parse(&content)
}

/// Default commented configuration template.
///
/// See `docs/specs/config.md §1`.
pub fn template() -> &'static str {
    "# Termin user configuration\n[font]\nsize = 13.0\n"
}

/// Compute diff between two configurations.
///
/// See `docs/specs/config.md §4`.
pub fn diff(old: &Config, new: &Config) -> ConfigDiff {
    ConfigDiff {
        font_changed: old.font != new.font,
        theme_changed: old.theme != new.theme,
        window_changed: old.window != new.window,
        terminal_changed: old.terminal != new.terminal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_parses() {
        let toml_str = "[font]\nsize = 14.0\n";
        let cfg = parse(toml_str).unwrap();
        assert_eq!(cfg.font.size, 14.0);
        assert_eq!(cfg.font.ligatures, true);
    }

    #[test]
    fn test_diff() {
        let c1 = Config::default();
        let mut c2 = Config::default();
        c2.font.size = 16.0;
        let d = diff(&c1, &c2);
        assert!(d.font_changed);
        assert!(!d.theme_changed);
    }
}
