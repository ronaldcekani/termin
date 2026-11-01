//! Compact keyboard shortcut keycaps for palette rows.
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::TermFonts,
    theme::Tokens,
};
use termin_core::Rgba;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
pub fn draw<B: FontBackend>(
    text: &str,
    x: f32,
    y: f32,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) -> f32 {
    let mut pen = x;
    for key in text.split('+') {
        let w = (key.chars().count() as f32 * fonts.geo.cell_w + 10.0).max(16.0);
        layer.quads.push(rounded_quad(
            pen,
            y,
            w,
            18.0,
            tokens.bg_surface,
            3.0,
            0.0,
            Rgba::TRANSPARENT,
        ));
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            key,
            pen + 5.0,
            y + 3.0,
            w - 8.0,
            tokens.text_secondary,
            layer,
        );
        pen += w + 3.0;
    }
    pen - x
}
