//! Non-modal window banner renderer.
use crate::{
    draw::{draw_text_mono, solid_quad},
    terminal_view::TermFonts,
    theme::Tokens,
};
use termin_core::Rect;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerTone {
    Error,
    Info,
}
#[allow(clippy::too_many_arguments)]
pub fn draw<B: FontBackend>(
    rect: Rect,
    message: &str,
    action: Option<&str>,
    tone: BannerTone,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    let color = match tone {
        BannerTone::Error => tokens.status_danger,
        BannerTone::Info => tokens.status_info,
    };
    layer
        .quads
        .push(solid_quad(rect.x, rect.y, rect.w, 32.0, tokens.bg_elevated));
    layer
        .quads
        .push(solid_quad(rect.x, rect.y, 3.0, 32.0, color));
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        message,
        rect.x + 12.0,
        rect.y + 9.0,
        rect.w - 120.0,
        tokens.text_primary,
        layer,
    );
    if let Some(label) = action {
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            label,
            rect.right() - 100.0,
            rect.y + 9.0,
            76.0,
            tokens.text_link,
            layer,
        );
    }
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        "×",
        rect.right() - 20.0,
        rect.y + 9.0,
        14.0,
        tokens.text_secondary,
        layer,
    );
}
