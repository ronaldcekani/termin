//! Find overlay bar for the focused pane.
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::TermFonts,
    theme::Tokens,
};
use termin_core::{Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
#[allow(clippy::too_many_arguments)]
pub fn draw<B: FontBackend>(
    rect: Rect,
    query: &str,
    count: Option<(usize, usize)>,
    case_sensitive: bool,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    let x = rect.right() - 320.0;
    let y = rect.y + 8.0;
    layer.quads.push(rounded_quad(
        x,
        y,
        312.0,
        32.0,
        tokens.bg_elevated,
        6.0,
        1.0,
        tokens.border_subtle,
    ));
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        "⌕",
        x + 8.0,
        y + 8.0,
        18.0,
        tokens.icon_default,
        layer,
    );
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        query,
        x + 30.0,
        y + 8.0,
        145.0,
        tokens.text_primary,
        layer,
    );
    let count_text = count
        .map(|(a, b)| format!("{a}/{b}"))
        .unwrap_or_else(|| "0/0".to_owned());
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        &count_text,
        x + 178.0,
        y + 8.0,
        42.0,
        if count.is_some() {
            tokens.text_tertiary
        } else {
            tokens.status_danger
        },
        layer,
    );
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        if case_sensitive { "Aa" } else { "aa" },
        x + 224.0,
        y + 8.0,
        28.0,
        tokens.text_secondary,
        layer,
    );
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        "↑ ↓ ×",
        x + 254.0,
        y + 8.0,
        54.0,
        Rgba(tokens.text_secondary.0),
        layer,
    );
}
