//! Design tokens for Termin UI.
//!
//! See `docs/design/design-system.md`.

use termin_core::Rgba;

/// Theme tokens defining colors, metrics, and typography for UI presentation.
///
/// See `docs/design/design-system.md §2`.
#[derive(Clone, Debug, PartialEq)]
pub struct Tokens {
    pub bg_app: Rgba,
    pub bg_sidebar: Rgba,
    pub bg_chrome: Rgba,
    pub bg_surface: Rgba,
    pub bg_surface_hover: Rgba,
    pub bg_elevated: Rgba,
    pub bg_input: Rgba,

    pub text_primary: Rgba,
    pub text_secondary: Rgba,
    pub text_muted: Rgba,

    pub accent_tide: Rgba,
    pub accent_ember: Rgba,
    pub status_success: Rgba,
    pub status_danger: Rgba,
    pub status_info: Rgba,
}

impl Default for Tokens {
    fn default() -> Self {
        // Dark theme defaults from docs/design/design-system.md §2
        Self {
            bg_app: Rgba::from_rgb(0x0C, 0x0D, 0x0F),
            bg_sidebar: Rgba::from_rgb(0x11, 0x12, 0x15),
            bg_chrome: Rgba::from_rgb(0x11, 0x12, 0x15),
            bg_surface: Rgba::from_rgb(0x16, 0x18, 0x1B),
            bg_surface_hover: Rgba::from_rgb(0x1C, 0x1E, 0x22),
            bg_elevated: Rgba::from_rgb(0x1C, 0x1E, 0x22),
            bg_input: Rgba::from_rgb(0x11, 0x12, 0x15),

            text_primary: Rgba::from_rgb(0xE7, 0xEA, 0xEE),
            text_secondary: Rgba::from_rgb(0x9A, 0xA1, 0xAB),
            text_muted: Rgba::from_rgb(0x6E, 0x75, 0x80),

            accent_tide: Rgba::from_rgb(0x3F, 0xD0, 0xC9),
            accent_ember: Rgba::from_rgb(0xFF, 0xB5, 0x47),
            status_success: Rgba::from_rgb(0x4E, 0xD3, 0x8A),
            status_danger: Rgba::from_rgb(0xFF, 0x6B, 0x6B),
            status_info: Rgba::from_rgb(0x7A, 0xA7, 0xFF),
        }
    }
}
