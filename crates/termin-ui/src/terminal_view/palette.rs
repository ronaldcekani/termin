//! Terminal color resolution (palette, default colors, cell attributes).

use termin_config::TerminalTheme;
use termin_core::Rgba;
use termin_vt::{Attrs, Color, Style};

/// Resolved terminal palette derived from the active theme.
#[derive(Clone, Debug)]
pub struct TermPalette {
    pub fg: Rgba,
    pub bg: Rgba,
    /// Full xterm 256 color table (0–15 theme ANSI, 16–231 cube, 232–255 ramp).
    pub table: [Rgba; 256],
    pub cursor: Rgba,
    pub cursor_text: Rgba,
    pub selection_bg: Rgba,
    pub selection_fg: Option<Rgba>,
    pub bold_is_bright: bool,
}

impl TermPalette {
    /// Build from a theme's `[terminal]` section.
    pub fn from_theme(t: &TerminalTheme, bold_is_bright: bool) -> Self {
        let mut table = [Rgba::BLACK; 256];
        table[..16].copy_from_slice(&t.ansi);
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        for i in 0..216usize {
            let (r, g, b) = (i / 36, (i / 6) % 6, i % 6);
            table[16 + i] = Rgba::from_rgb(LEVELS[r], LEVELS[g], LEVELS[b]);
        }
        for i in 0..24usize {
            let v = (8 + 10 * i) as u8;
            table[232 + i] = Rgba::from_rgb(v, v, v);
        }
        Self {
            fg: t.foreground,
            bg: t.background,
            table,
            cursor: t.cursor.unwrap_or(t.foreground),
            cursor_text: t.cursor_text.unwrap_or(t.background),
            selection_bg: t
                .selection_bg
                .unwrap_or(Rgba::from_rgba(0x80, 0x80, 0x80, 0x60)),
            selection_fg: t.selection_fg,
            bold_is_bright,
        }
    }

    /// Resolve a foreground color.
    #[inline]
    pub fn resolve_fg(&self, c: Color, bold: bool) -> Rgba {
        match c {
            Color::Default => self.fg,
            Color::Indexed(i) => {
                let i = if bold && self.bold_is_bright && i < 8 {
                    i + 8
                } else {
                    i
                };
                self.table[i as usize]
            }
            Color::Rgb(r, g, b) => Rgba::from_rgb(r, g, b),
        }
    }

    /// Resolve a background color.
    #[inline]
    pub fn resolve_bg(&self, c: Color) -> Rgba {
        match c {
            Color::Default => self.bg,
            Color::Indexed(i) => self.table[i as usize],
            Color::Rgb(r, g, b) => Rgba::from_rgb(r, g, b),
        }
    }

    /// Final (fg, bg) for a style after inverse / dim handling.
    #[inline]
    pub fn resolve_style(&self, st: &Style) -> (Rgba, Rgba) {
        let bold = st.attrs.contains(Attrs::BOLD);
        let mut fg = self.resolve_fg(st.fg, bold);
        let mut bg = self.resolve_bg(st.bg);
        if st.attrs.contains(Attrs::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if st.attrs.contains(Attrs::DIM) {
            fg = mix(fg, bg);
        }
        (fg, bg)
    }
}

/// 50/50 channel mix (used for dim text).
#[inline]
fn mix(a: Rgba, b: Rgba) -> Rgba {
    let m = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;
    Rgba::from_rgba(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()), a.a())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_and_ramp() {
        let p = TermPalette::from_theme(&TerminalTheme::default(), false);
        assert_eq!(p.table[16], Rgba::from_rgb(0, 0, 0));
        assert_eq!(p.table[231], Rgba::from_rgb(255, 255, 255));
        assert_eq!(p.table[232], Rgba::from_rgb(8, 8, 8));
        assert_eq!(p.table[255], Rgba::from_rgb(238, 238, 238));
    }

    #[test]
    fn bold_bright_and_inverse() {
        let mut p = TermPalette::from_theme(&TerminalTheme::default(), true);
        assert_eq!(p.resolve_fg(Color::Indexed(1), true), p.table[9]);
        assert_eq!(p.resolve_fg(Color::Indexed(1), false), p.table[1]);
        p.bold_is_bright = false;
        let st = Style {
            attrs: Attrs::INVERSE,
            ..Style::default()
        };
        let (fg, bg) = p.resolve_style(&st);
        assert_eq!(fg, p.bg);
        assert_eq!(bg, p.fg);
    }
}
