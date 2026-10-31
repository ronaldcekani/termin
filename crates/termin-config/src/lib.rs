//! Configuration loading, schema, themes, and diffing (`termin-config`).
//!
//! See `docs/specs/config.md` and `docs/design/design-system.md`.

pub mod chord;
pub mod color;
pub mod diagnostic;
pub mod diff;
pub mod parse;
pub mod schema;
pub mod template;
pub mod theme;

pub use chord::{
    parse_chord, validate_keybinding_entry, ChordParseError, KeyChord,
};
pub use color::{
    contrast_ratio, format_hex_color, mix_linear, parse_color, relative_luminance,
    with_alpha, ColorParseError,
};
pub use diagnostic::{Diagnostic, DiagnosticLevel};
pub use diff::{diff, ConfigDiff};
pub use parse::{
    load, load_with_action_validator, parse, parse_with_action_validator,
};
pub use schema::{
    BlocksConfig, ColorsConfig, Config, EditorConfig, FontConfig, KeybindingDef,
    KeybindingsConfig, NotificationConfig, NotificationsConfig, PaneConfig,
    ShellConfig, SidebarConfig, SplitConfig, TerminalConfig,
    TerminalConfigSection, ThemeConfig, WindowConfig,
};
pub use template::{template, DEFAULT_CONFIG_TEMPLATE};
pub use theme::{
    clamp_text_secondary, derive_attention_color, derive_neutral_ramp, load_theme,
    parse_theme, TerminalTheme, Theme, ThemeMeta, BUILTIN_TERMIN_DARK,
    BUILTIN_TERMIN_LIGHT,
};
