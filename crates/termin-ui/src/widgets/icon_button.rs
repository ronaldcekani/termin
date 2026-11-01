//! Small custom drawn icon button.
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::Rect;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

pub fn draw_icon_button<B: FontBackend>(
    rect: Rect,
    glyph: &str,
    hovered: bool,
    tokens: &Tokens,
    fonts: &TermFonts,
    cache: &mut GlyphCache<B>,
    layer: &mut Layer,
) -> Rect {
    if hovered {
        layer.quads.push(rounded_quad(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            tokens.bg_surface_hover,
            4.0,
            0.0,
            tokens.bg_surface_hover,
        ));
    }
    draw_text_mono(
        cache,
        fonts.font,
        &fonts.geo,
        glyph,
        rect.x + (rect.w - fonts.geo.cell_w) * 0.5,
        rect.y + (rect.h - fonts.geo.cell_h) * 0.5,
        rect.w,
        if hovered {
            tokens.icon_hover
        } else {
            tokens.icon_default
        },
        layer,
    );
    rect
}
