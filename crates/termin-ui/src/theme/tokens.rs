//! Design tokens for Termin UI.
//!
//! See `docs/design/design-system.md`.

use std::collections::HashMap;
use termin_config::{
    clamp_text_secondary, contrast_ratio, derive_attention_color, derive_neutral_ramp,
    mix_linear, parse_color, parse_theme, relative_luminance, with_alpha, Theme,
    BUILTIN_TERMIN_DARK, BUILTIN_TERMIN_LIGHT,
};
use termin_core::Rgba;

/// Theme tokens defining colors, metrics, and typography for UI presentation.
///
/// See `docs/design/design-system.md §2`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tokens {
    // Semantic backgrounds
    pub bg_app: Rgba,
    pub bg_sidebar: Rgba,
    pub bg_chrome: Rgba,
    pub bg_surface: Rgba,
    pub bg_surface_hover: Rgba,
    pub bg_elevated: Rgba,
    pub bg_input: Rgba,
    pub bg_selected: Rgba,
    pub bg_attention: Rgba,
    pub bg_danger: Rgba,

    // Borders
    pub border_subtle: Rgba,
    pub border_default: Rgba,
    pub border_strong: Rgba,
    pub border_focus: Rgba,
    pub border_attention: Rgba,

    // Typography colors
    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_tertiary: Rgba,
    pub text_muted: Rgba,
    pub text_disabled: Rgba,
    pub text_on_accent: Rgba,
    pub text_link: Rgba,

    // Icons
    pub icon_default: Rgba,
    pub icon_hover: Rgba,

    // Cursor and selection
    pub cursor: Rgba,
    pub cursor_text: Rgba,
    pub selection: Rgba,

    // Search highlights
    pub search_match: Rgba,
    pub search_current: Rgba,

    // Scrollbar and shadows
    pub scrollbar: Rgba,
    pub shadow: Rgba,

    // Input editor syntax highlighting
    pub syntax_command: Rgba,
    pub syntax_command_invalid: Rgba,
    pub syntax_argument: Rgba,
    pub syntax_flag: Rgba,
    pub syntax_string: Rgba,
    pub syntax_variable: Rgba,
    pub syntax_operator: Rgba,
    pub syntax_path: Rgba,
    pub syntax_comment: Rgba,
    pub syntax_ghost: Rgba,

    // Brand and semantic hues
    pub accent_tide: Rgba,
    pub accent_ember: Rgba,
    pub status_success: Rgba,
    pub status_danger: Rgba,
    pub status_info: Rgba,

    // Terminal ANSI palette
    pub ansi: [Rgba; 16],
}

impl Default for Tokens {
    fn default() -> Self {
        Self::dark()
    }
}

impl Tokens {
    /// Return the standard Termin Dark design tokens.
    pub fn dark() -> Self {
        let theme = parse_theme(BUILTIN_TERMIN_DARK).expect("valid built-in dark theme");
        Self::from_theme(&theme)
    }

    /// Return the standard Termin Light design tokens.
    pub fn light() -> Self {
        let theme = parse_theme(BUILTIN_TERMIN_LIGHT).expect("valid built-in light theme");
        Self::from_theme(&theme)
    }

    /// Construct design tokens from a loaded theme, applying derivation rules
    /// for any missing UI tokens.
    pub fn from_theme(theme: &Theme) -> Self {
        Self::from_theme_with_overrides(theme, &HashMap::new())
    }

    /// Construct design tokens from a theme and user color overrides.
    ///
    /// Implements derivation rules from `docs/design/design-system.md §2.4`.
    pub fn from_theme_with_overrides(
        theme: &Theme,
        overrides: &HashMap<String, String>,
    ) -> Self {
        let bg = theme.terminal.background;
        let fg = theme.terminal.foreground;
        let is_dark = theme.is_dark();

        let resolve_str = |key: &str| -> Option<&str> {
            overrides
                .get(key)
                .map(|s| s.as_str())
                .or_else(|| theme.ui.get(key).map(|s| s.as_str()))
        };

        // Accent hue (Tide): ui.accent, ui."accent.tide", or ansi[6] (cyan)
        let accent = resolve_str("accent.tide")
            .or_else(|| resolve_str("accent"))
            .and_then(|s| parse_color(s).ok())
            .unwrap_or(theme.terminal.ansi[6]);

        // Attention hue (Ember): ui."accent.ember", ui.attention, or ansi[3] if contrast >= 3:1 else Ember
        let attention = resolve_str("accent.ember")
            .or_else(|| resolve_str("attention"))
            .and_then(|s| parse_color(s).ok())
            .unwrap_or_else(|| {
                derive_attention_color(bg, theme.terminal.ansi[3], is_dark)
            });

        // Danger hue: ui."status.danger", ui.danger, or ansi[1] (red)
        let danger = resolve_str("status.danger")
            .or_else(|| resolve_str("danger"))
            .and_then(|s| parse_color(s).ok())
            .unwrap_or(theme.terminal.ansi[1]);

        // Success hue: ui."status.success", ui.success, or ansi[2] (green)
        let success = resolve_str("status.success")
            .or_else(|| resolve_str("success"))
            .and_then(|s| parse_color(s).ok())
            .unwrap_or(theme.terminal.ansi[2]);

        // Info hue: ui."status.info", ui.info, or cyan/blue
        let info = resolve_str("status.info")
            .or_else(|| resolve_str("info"))
            .and_then(|s| parse_color(s).ok())
            .unwrap_or_else(|| {
                if is_dark {
                    Rgba::from_rgb(0x7A, 0xA7, 0xFF)
                } else {
                    Rgba::from_rgb(0x2F, 0x6B, 0xDB)
                }
            });

        // Neutral ramp n0..n11
        let ramp = derive_neutral_ramp(bg, fg);

        // Clamped text.secondary (must be >= 4.5:1 vs bg.sidebar = n1)
        let derived_secondary = clamp_text_secondary(ramp[1], fg, ramp[9]);

        let get_color = |key: &str, default: Rgba| -> Rgba {
            resolve_str(key)
                .and_then(|s| parse_color(s).ok())
                .unwrap_or(default)
        };

        // Text on solid accent fill
        let default_on_accent = if relative_luminance(accent) > 0.4 {
            Rgba::from_rgb(0x06, 0x2B, 0x29)
        } else {
            Rgba::WHITE
        };

        let cursor_val = get_color(
            "cursor",
            theme.terminal.cursor.unwrap_or(accent),
        );
        let cursor_text_val = get_color(
            "cursor_text",
            theme.terminal.cursor_text.unwrap_or(bg),
        );
        let selection_val = get_color(
            "selection",
            theme
                .terminal
                .selection_bg
                .unwrap_or_else(|| with_alpha(accent, 0.28)),
        );

        let default_flag = if is_dark {
            Rgba::from_rgb(0x8F, 0xB7, 0xFF)
        } else {
            Rgba::from_rgb(0x2F, 0x6B, 0xDB)
        };
        let default_string = if is_dark {
            Rgba::from_rgb(0xB8, 0xE0, 0x7A)
        } else {
            Rgba::from_rgb(0x2D, 0x7D, 0x18)
        };
        let default_variable = if is_dark {
            Rgba::from_rgb(0xE3, 0xA6, 0xFF)
        } else {
            Rgba::from_rgb(0x8B, 0x36, 0xB8)
        };
        let default_shadow = if is_dark {
            Rgba::from_rgba(0, 0, 0, 115)
        } else {
            Rgba::from_rgba(0, 0, 0, 51)
        };

        let tertiary = get_color("text.tertiary", ramp[8]);

        let border_focus_val = get_color("border.focus", accent);
        let border_attention_val = get_color("border.attention", attention);

        // Ensure WCAG 1.4.11 minimum 3:1 contrast against app background
        let ensure_contrast_3 = |color: Rgba, background: Rgba| -> Rgba {
            if contrast_ratio(color, background) >= 3.0 {
                return color;
            }
            let is_dark_bg = relative_luminance(background) < 0.5;
            let target = if is_dark_bg { Rgba::WHITE } else { Rgba::BLACK };
            let mut t = 0.0f32;
            let mut c = color;
            while contrast_ratio(c, background) < 3.0 && t < 1.0 {
                t = (t + 0.02f32).min(1.0f32);
                c = mix_linear(color, target, t);
            }
            c
        };

        let border_focus = ensure_contrast_3(border_focus_val, ramp[0]);
        let border_attention = ensure_contrast_3(border_attention_val, ramp[0]);

        Self {
            bg_app: get_color("bg.app", ramp[0]),
            bg_sidebar: get_color("bg.sidebar", ramp[1]),
            bg_chrome: get_color("bg.chrome", ramp[1]),
            bg_surface: get_color("bg.surface", ramp[2]),
            bg_surface_hover: get_color("bg.surface.hover", ramp[3]),
            bg_elevated: get_color("bg.elevated", ramp[3]),
            bg_input: get_color("bg.input", ramp[1]),
            bg_selected: get_color("bg.selected", with_alpha(accent, 0.14)),
            bg_attention: get_color("bg.attention", with_alpha(attention, 0.10)),
            bg_danger: get_color("bg.danger", with_alpha(danger, 0.08)),

            border_subtle: get_color("border.subtle", ramp[3]),
            border_default: get_color("border.default", ramp[4]),
            border_strong: get_color("border.strong", ramp[6]),
            border_focus,
            border_attention,

            text_primary: get_color("text.primary", fg),
            text_secondary: get_color("text.secondary", derived_secondary),
            text_tertiary: tertiary,
            text_muted: tertiary,
            text_disabled: get_color("text.disabled", ramp[7]),
            text_on_accent: get_color("text.on_accent", default_on_accent),
            text_link: get_color("text.link", accent),

            icon_default: get_color("icon.default", ramp[8]),
            icon_hover: get_color("icon.hover", ramp[10]),

            cursor: cursor_val,
            cursor_text: cursor_text_val,
            selection: selection_val,

            search_match: get_color("search.match", with_alpha(attention, 0.30)),
            search_current: get_color("search.current", with_alpha(attention, 0.65)),

            scrollbar: get_color("scrollbar", with_alpha(ramp[6], 0.70)),
            shadow: get_color("shadow", default_shadow),

            syntax_command: get_color("syntax.command", fg),
            syntax_command_invalid: get_color("syntax.command.invalid", danger),
            syntax_argument: get_color("syntax.argument", ramp[10]),
            syntax_flag: get_color("syntax.flag", default_flag),
            syntax_string: get_color("syntax.string", default_string),
            syntax_variable: get_color("syntax.variable", default_variable),
            syntax_operator: get_color("syntax.operator", accent),
            syntax_path: get_color("syntax.path", ramp[10]),
            syntax_comment: get_color("syntax.comment", ramp[7]),
            syntax_ghost: get_color("syntax.ghost", ramp[7]),

            accent_tide: accent,
            accent_ember: attention,
            status_success: success,
            status_danger: danger,
            status_info: info,

            ansi: theme.terminal.ansi,
        }
    }
}

/// Description of a contrast rule violation.
#[derive(Clone, Debug, PartialEq)]
pub struct ContrastViolation {
    pub foreground_name: &'static str,
    pub background_name: &'static str,
    pub actual_contrast: f32,
    pub required_contrast: f32,
}

/// Verify that theme tokens comply with WCAG AA and design system contrast rules.
///
/// See `docs/design/design-system.md §2.6`:
/// - `text.primary` on surfaces >= 4.5:1
/// - `text.secondary` on surfaces >= 4.5:1
/// - `text.tertiary` on surfaces >= 3.0:1
/// - `border.focus` and `border.attention` >= 3.0:1
pub fn check_theme_contrast(tokens: &Tokens) -> Vec<ContrastViolation> {
    let mut violations = Vec::new();

    let mut check = |fg_name: &'static str,
                     fg: Rgba,
                     bg_name: &'static str,
                     bg: Rgba,
                     min_ratio: f32| {
        let ratio = contrast_ratio(fg, bg);
        // Small tolerance of 0.05 for float precision
        if ratio < min_ratio - 0.05 {
            violations.push(ContrastViolation {
                foreground_name: fg_name,
                background_name: bg_name,
                actual_contrast: ratio,
                required_contrast: min_ratio,
            });
        }
    };

    // 1. Text primary: WCAG AA requires >= 4.5:1
    check("text.primary", tokens.text_primary, "bg.app", tokens.bg_app, 4.5);
    check(
        "text.primary",
        tokens.text_primary,
        "bg.sidebar",
        tokens.bg_sidebar,
        4.5,
    );
    check(
        "text.primary",
        tokens.text_primary,
        "bg.chrome",
        tokens.bg_chrome,
        4.5,
    );
    check(
        "text.primary",
        tokens.text_primary,
        "bg.surface",
        tokens.bg_surface,
        4.5,
    );
    check(
        "text.primary",
        tokens.text_primary,
        "bg.input",
        tokens.bg_input,
        4.5,
    );

    // 2. Text secondary: WCAG AA requires >= 4.5:1
    check(
        "text.secondary",
        tokens.text_secondary,
        "bg.app",
        tokens.bg_app,
        4.5,
    );
    check(
        "text.secondary",
        tokens.text_secondary,
        "bg.sidebar",
        tokens.bg_sidebar,
        4.5,
    );

    // 3. Text tertiary: >= 3.0:1
    check(
        "text.tertiary",
        tokens.text_tertiary,
        "bg.app",
        tokens.bg_app,
        3.0,
    );
    check(
        "text.tertiary",
        tokens.text_tertiary,
        "bg.sidebar",
        tokens.bg_sidebar,
        3.0,
    );

    // 4. Focus and attention indicators (WCAG 1.4.11 non-text contrast >= 3.0:1)
    check(
        "border.focus",
        tokens.border_focus,
        "bg.app",
        tokens.bg_app,
        3.0,
    );
    check(
        "border.attention",
        tokens.border_attention,
        "bg.app",
        tokens.bg_app,
        3.0,
    );

    violations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_contrast() {
        let dark = Tokens::dark();
        let dark_violations = check_theme_contrast(&dark);
        assert!(
            dark_violations.is_empty(),
            "Dark theme contrast violations: {:?}",
            dark_violations
        );

        let light = Tokens::light();
        let light_violations = check_theme_contrast(&light);
        assert!(
            light_violations.is_empty(),
            "Light theme contrast violations: {:?}",
            light_violations
        );
    }

    #[test]
    fn test_derived_tokens_from_minimal_terminal_theme() {
        // Theme with only terminal colors (no [ui] section)
        let theme_str = r##"
        [meta]
        name = "minimal-dark"
        variant = "dark"

        [terminal]
        foreground = "#FFFFFF"
        background = "#101010"
        ansi = [
            "#000000", "#FF0000", "#00FF00", "#FFFF00",
            "#0000FF", "#FF00FF", "#00FFFF", "#FFFFFF",
            "#555555", "#FF5555", "#55FF55", "#FFFF55",
            "#5555FF", "#FF55FF", "#55FFFF", "#FFFFFF",
        ]
        "##;

        let theme = parse_theme(theme_str).unwrap();
        let tokens = Tokens::from_theme(&theme);

        assert_eq!(tokens.bg_app, Rgba::from_rgb(0x10, 0x10, 0x10));
        assert_eq!(tokens.text_primary, Rgba::WHITE);
        assert_eq!(tokens.accent_tide, Rgba::from_rgb(0x00, 0xFF, 0xFF)); // cyan

        let violations = check_theme_contrast(&tokens);
        assert!(
            violations.is_empty(),
            "Derived tokens contrast violations: {:?}",
            violations
        );
    }
}
