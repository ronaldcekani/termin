//! Ephemeral confirmation toast renderer.
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::TermFonts,
    theme::Tokens,
};
use termin_core::Rect;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
pub fn draw<B: FontBackend>(
    rect: Rect,
    message: &str,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    let w = (message.chars().count() as f32 * fonts.geo.cell_w + 28.0).clamp(160.0, 420.0);
    let x = rect.x + (rect.w - w) * 0.5;
    let y = rect.bottom() - 52.0;
    layer.quads.push(rounded_quad(
        x,
        y,
        w,
        28.0,
        tokens.bg_elevated,
        8.0,
        0.0,
        tokens.border_subtle,
    ));
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        message,
        x + 14.0,
        y + 7.0,
        w - 28.0,
        tokens.text_primary,
        layer,
    );
}
