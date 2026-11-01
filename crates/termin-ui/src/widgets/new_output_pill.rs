//! Scroll-to-bottom affordance shown while output arrives off-screen.
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::TermFonts,
    Tokens,
};
use termin_core::Rect;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

pub struct NewOutputPill;

impl NewOutputPill {
    pub fn render<B: FontBackend>(
        rect: Rect,
        fonts: &TermFonts,
        glyphs: &mut GlyphCache<B>,
        tokens: &Tokens,
        layer: &mut Layer,
    ) {
        layer.quads.push(rounded_quad(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            tokens.bg_elevated,
            rect.h * 0.5,
            0.0,
            tokens.border_subtle,
        ));
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            "↓  New output",
            rect.x + 10.0,
            rect.y + (rect.h - fonts.geo.cell_h) * 0.5,
            rect.w - 20.0,
            tokens.text_primary,
            layer,
        );
    }
}
