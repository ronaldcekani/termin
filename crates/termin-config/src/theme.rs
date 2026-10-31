//! Theme definition, parsing, loading, and token derivation.
//!
//! See `docs/specs/config.md §3` and `docs/design/design-system.md §2`.

use crate::color::{contrast_ratio, mix_linear, parse_color, relative_luminance};
use crate::diagnostic::Diagnostic;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use termin_core::Rgba;

pub const BUILTIN_TERMIN_DARK: &str = include_str!("../../../resources/themes/termin-dark.toml");
pub const BUILTIN_TERMIN_LIGHT: &str = include_str!("../../../resources/themes/termin-light.toml");

/// Theme metadata section `[meta]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeMeta {
    pub name: String,
    pub variant: String, // "dark" | "light"
}

impl Default for ThemeMeta {
    fn default() -> Self {
        Self {
            name: "custom".to_string(),
            variant: "dark".to_string(),
        }
    }
}

/// Terminal palette section `[terminal]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalTheme {
    #[serde(with = "crate::color::serde_rgba")]
    pub foreground: Rgba,
    #[serde(with = "crate::color::serde_rgba")]
    pub background: Rgba,
    #[serde(default, with = "crate::color::serde_opt_rgba")]
    pub cursor: Option<Rgba>,
    #[serde(default, with = "crate::color::serde_opt_rgba")]
    pub cursor_text: Option<Rgba>,
    #[serde(default, with = "crate::color::serde_opt_rgba")]
    pub selection_bg: Option<Rgba>,
    #[serde(default, with = "crate::color::serde_opt_rgba")]
    pub selection_fg: Option<Rgba>,
    #[serde(with = "crate::color::serde_ansi_array")]
    pub ansi: [Rgba; 16],
}

impl Default for TerminalTheme {
    fn default() -> Self {
        // Defaults to Termin Dark palette (see docs/design/design-system.md §2.5)
        Self {
            foreground: Rgba::from_rgb(0xE7, 0xEA, 0xEE),
            background: Rgba::from_rgb(0x0C, 0x0D, 0x0F),
            cursor: Some(Rgba::from_rgb(0x3F, 0xD0, 0xC9)),
            cursor_text: Some(Rgba::from_rgb(0x0C, 0x0D, 0x0F)),
            selection_bg: Some(Rgba::from_rgba(0x3F, 0xD0, 0xC9, 71)),
            selection_fg: Some(Rgba::from_rgb(0xE7, 0xEA, 0xEE)),
            ansi: [
                Rgba::from_rgb(0x1C, 0x1E, 0x22), // 0: black
                Rgba::from_rgb(0xFF, 0x6B, 0x6B), // 1: red
                Rgba::from_rgb(0x4E, 0xD3, 0x8A), // 2: green
                Rgba::from_rgb(0xFF, 0xC8, 0x61), // 3: yellow
                Rgba::from_rgb(0x6F, 0xA0, 0xFF), // 4: blue
                Rgba::from_rgb(0xD5, 0x8C, 0xFF), // 5: magenta
                Rgba::from_rgb(0x3F, 0xD0, 0xC9), // 6: cyan
                Rgba::from_rgb(0xC4, 0xC9, 0xD0), // 7: white
                Rgba::from_rgb(0x52, 0x58, 0x62), // 8: bright black
                Rgba::from_rgb(0xFF, 0x8C, 0x8C), // 9: bright red
                Rgba::from_rgb(0x7B, 0xE3, 0xA8), // 10: bright green
                Rgba::from_rgb(0xFF, 0xD9, 0x8A), // 11: bright yellow
                Rgba::from_rgb(0x97, 0xBB, 0xFF), // 12: bright blue
                Rgba::from_rgb(0xE3, 0xB0, 0xFF), // 13: bright magenta
                Rgba::from_rgb(0x7D, 0xE2, 0xDC), // 14: bright cyan
                Rgba::from_rgb(0xF4, 0xF6, 0xF8), // 15: bright white
            ],
        }
    }
}

/// Complete theme definition containing metadata, terminal palette, and optional UI tokens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    #[serde(default)]
    pub meta: ThemeMeta,
    pub terminal: TerminalTheme,
    #[serde(default)]
    pub ui: HashMap<String, String>,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            meta: ThemeMeta {
                name: "termin-dark".to_string(),
                variant: "dark".to_string(),
            },
            terminal: TerminalTheme::default(),
            ui: HashMap::new(),
        }
    }
}

impl Theme {
    /// Whether this theme is dark variant.
    pub fn is_dark(&self) -> bool {
        if self.meta.variant.eq_ignore_ascii_case("light") {
            false
        } else if self.meta.variant.eq_ignore_ascii_case("dark") {
            true
        } else {
            relative_luminance(self.terminal.background)
                < relative_luminance(self.terminal.foreground)
        }
    }
}

/// Parse theme TOML string into a `Theme` struct.
pub fn parse_theme(src: &str) -> Result<Theme, Diagnostic> {
    #[derive(Deserialize)]
    struct RawTheme {
        #[serde(default)]
        meta: Option<ThemeMeta>,
        terminal: Option<RawTerminalTheme>,
        #[serde(default)]
        ui: Option<HashMap<String, String>>,
    }

    #[derive(Deserialize)]
    struct RawTerminalTheme {
        foreground: String,
        background: String,
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        cursor_text: Option<String>,
        #[serde(default)]
        selection_bg: Option<String>,
        #[serde(default)]
        selection_fg: Option<String>,
        ansi: Vec<String>,
    }

    let raw: RawTheme = toml::from_str(src).map_err(|e| {
        Diagnostic::error(format!("failed to parse theme TOML: {}", e))
    })?;

    let term_raw = raw.terminal.ok_or_else(|| {
        Diagnostic::error("theme must contain a [terminal] section")
    })?;

    let fg = parse_color(&term_raw.foreground).map_err(|e| {
        Diagnostic::error(format!("invalid terminal.foreground color: {}", e))
    })?;
    let bg = parse_color(&term_raw.background).map_err(|e| {
        Diagnostic::error(format!("invalid terminal.background color: {}", e))
    })?;
    let cursor = match term_raw.cursor {
        Some(s) => Some(parse_color(&s).map_err(|e| {
            Diagnostic::error(format!("invalid terminal.cursor color: {}", e))
        })?),
        None => None,
    };
    let cursor_text = match term_raw.cursor_text {
        Some(s) => Some(parse_color(&s).map_err(|e| {
            Diagnostic::error(format!("invalid terminal.cursor_text color: {}", e))
        })?),
        None => None,
    };
    let selection_bg = match term_raw.selection_bg {
        Some(s) => Some(parse_color(&s).map_err(|e| {
            Diagnostic::error(format!("invalid terminal.selection_bg color: {}", e))
        })?),
        None => None,
    };
    let selection_fg = match term_raw.selection_fg {
        Some(s) => Some(parse_color(&s).map_err(|e| {
            Diagnostic::error(format!("invalid terminal.selection_fg color: {}", e))
        })?),
        None => None,
    };

    if term_raw.ansi.len() != 16 {
        return Err(Diagnostic::error(format!(
            "terminal.ansi must contain 16 colors, found {}",
            term_raw.ansi.len()
        )));
    }

    let mut ansi = [Rgba::BLACK; 16];
    for (i, c_str) in term_raw.ansi.iter().enumerate() {
        ansi[i] = parse_color(c_str).map_err(|e| {
            Diagnostic::error(format!("invalid color in terminal.ansi[{}]: {}", i, e))
        })?;
    }

    let is_dark = relative_luminance(bg) < relative_luminance(fg);
    let meta = raw.meta.unwrap_or_else(|| ThemeMeta {
        name: "custom".to_string(),
        variant: if is_dark { "dark".into() } else { "light".into() },
    });

    Ok(Theme {
        meta,
        terminal: TerminalTheme {
            foreground: fg,
            background: bg,
            cursor,
            cursor_text,
            selection_bg,
            selection_fg,
            ansi,
        },
        ui: raw.ui.unwrap_or_default(),
    })
}

/// Load theme by name from built-in themes or the user theme directory.
///
/// See `docs/specs/config.md §3, §5`.
pub fn load_theme(name: &str, user_dir: &Path) -> Result<Theme, Diagnostic> {
    match name {
        "termin-dark" => parse_theme(BUILTIN_TERMIN_DARK),
        "termin-light" => parse_theme(BUILTIN_TERMIN_LIGHT),
        "auto" => {
            // "auto" defaults to termin-dark in pure context; platform selects based on appearance
            parse_theme(BUILTIN_TERMIN_DARK)
        }
        _ => {
            // Check user_dir/name.toml, user_dir/themes/name.toml, and user_dir/name
            let candidate1 = user_dir.join(format!("{}.toml", name));
            let candidate2 = user_dir.join("themes").join(format!("{}.toml", name));
            let candidate3 = user_dir.join(name);

            let path = if candidate1.is_file() {
                candidate1
            } else if candidate2.is_file() {
                candidate2
            } else if candidate3.is_file() {
                candidate3
            } else {
                return Err(Diagnostic::error(format!(
                    "theme '{}' not found in {}",
                    name,
                    user_dir.display()
                ))
                .with_key(name));
            };

            let content = std::fs::read_to_string(&path).map_err(|e| {
                Diagnostic::error(format!(
                    "failed to read theme file {}: {}",
                    path.display(),
                    e
                ))
                .with_key(name)
            })?;

            parse_theme(&content)
        }
    }
}

/// Derive neutral ramp n0..n11 from background and foreground colors.
///
/// See `docs/design/design-system.md §2.4`:
/// - `n0 = bg`
/// - `n1..n5 = mix(bg, fg, [3,6,9,13,18]%)`
/// - `n6..n11 = mix(bg, fg, [28,40,52,64,80,93]%)`
pub fn derive_neutral_ramp(bg: Rgba, fg: Rgba) -> [Rgba; 12] {
    let percentages = [
        0.0, 0.03, 0.06, 0.09, 0.13, 0.18, 0.28, 0.40, 0.52, 0.64, 0.80, 0.93,
    ];
    let mut ramp = [Rgba::BLACK; 12];
    ramp[0] = bg;
    for i in 1..12 {
        ramp[i] = mix_linear(bg, fg, percentages[i]);
    }
    ramp
}

/// Derive attention color from yellow ANSI color and background.
///
/// See `docs/design/design-system.md §2.4`:
/// `ember = ui.attention or ansi[3]` (yellow) if its contrast vs. bg ≥ 3:1 else `#FFB547` (or `#D98500` for light).
pub fn derive_attention_color(bg: Rgba, yellow: Rgba, is_dark: bool) -> Rgba {
    if contrast_ratio(yellow, bg) >= 3.0 {
        yellow
    } else if is_dark {
        Rgba::from_rgb(0xFF, 0xB5, 0x47)
    } else {
        Rgba::from_rgb(0xD9, 0x85, 0x00)
    }
}

/// Clamp text.secondary contrast to at least 4.5:1 against sidebar background.
///
/// Pushes factor towards foreground if needed.
pub fn clamp_text_secondary(bg_sidebar: Rgba, fg: Rgba, initial_secondary: Rgba) -> Rgba {
    if contrast_ratio(initial_secondary, bg_sidebar) >= 4.5 {
        return initial_secondary;
    }
    let mut t: f32 = 0.64;
    let mut sec = initial_secondary;
    while contrast_ratio(sec, bg_sidebar) < 4.5 && t < 1.0 {
        t = (t + 0.02f32).min(1.0f32);
        sec = mix_linear(bg_sidebar, fg, t);
    }
    sec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_dark_loads() {
        let theme = parse_theme(BUILTIN_TERMIN_DARK).unwrap();
        assert_eq!(theme.meta.name, "termin-dark");
        assert_eq!(theme.meta.variant, "dark");
        assert_eq!(theme.terminal.background, Rgba::from_rgb(0x0C, 0x0D, 0x0F));
    }

    #[test]
    fn test_builtin_light_loads() {
        let theme = parse_theme(BUILTIN_TERMIN_LIGHT).unwrap();
        assert_eq!(theme.meta.name, "termin-light");
        assert_eq!(theme.meta.variant, "light");
        assert_eq!(theme.terminal.background, Rgba::from_rgb(0xFF, 0xFF, 0xFF));
    }

    #[test]
    fn test_builtin_themes_contrast() {
        let dark = parse_theme(BUILTIN_TERMIN_DARK).unwrap();
        let bg_dark = dark.terminal.background;
        let fg_dark = dark.terminal.foreground;

        // text.primary >= 7:1
        assert!(contrast_ratio(fg_dark, bg_dark) >= 7.0);

        // ANSI colors >= 4.5:1 (except black/bright black indices 0 and 8)
        for i in [1, 2, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14, 15] {
            let ratio = contrast_ratio(dark.terminal.ansi[i], bg_dark);
            assert!(
                ratio >= 4.5,
                "ANSI color {} failed contrast: actual {}",
                i,
                ratio
            );
        }

        let light = parse_theme(BUILTIN_TERMIN_LIGHT).unwrap();
        let bg_light = light.terminal.background;
        let fg_light = light.terminal.foreground;

        // text.primary >= 7:1
        assert!(contrast_ratio(fg_light, bg_light) >= 7.0);

        // ANSI normal colors >= 3.0:1 against white background
        for i in [0, 1, 2, 3, 4, 5, 6] {
            let ratio = contrast_ratio(light.terminal.ansi[i], bg_light);
            assert!(
                ratio >= 3.0,
                "Light ANSI color {} failed contrast: actual {}",
                i,
                ratio
            );
        }
    }

    #[test]
    fn test_theme_derivation_rules() {
        // Test with arbitrary low-contrast terminal colors to test clamping
        let bg = Rgba::from_rgb(0x20, 0x20, 0x20);
        let fg = Rgba::from_rgb(0xF0, 0xF0, 0xF0);
        let ramp = derive_neutral_ramp(bg, fg);
        assert_eq!(ramp[0], bg);

        let bg_sidebar = ramp[1];
        let clamped_secondary = clamp_text_secondary(bg_sidebar, fg, ramp[9]);
        assert!(
            contrast_ratio(clamped_secondary, bg_sidebar) >= 4.5,
            "clamped secondary must meet >= 4.5:1 contrast against sidebar"
        );

        // Attention derivation test
        let low_contrast_yellow = Rgba::from_rgb(0x30, 0x30, 0x20);
        let attention = derive_attention_color(bg, low_contrast_yellow, true);
        assert_eq!(attention, Rgba::from_rgb(0xFF, 0xB5, 0x47)); // falls back to Ember
    }
}
