//! Tooltip drawing primitive. Delay and hover ownership remain in the app.
use crate::{
    draw::{draw_text_ui, measure_text_ui, rounded_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::Rect;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

pub fn draw_tooltip<B: FontBackend>(
    rect: Rect,
    text: &str,
    shortcut: Option<&str>,
    tokens: &Tokens,
    fonts: &TermFonts,
    cache: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    layer.quads.push(rounded_quad(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        tokens.bg_elevated,
        4.0,
        1.0,
        tokens.border_subtle,
    ));
    let shortcut_w = shortcut.map_or(0.0, |s| measure_text_ui(cache, &fonts.ui, s) + 8.0);
    draw_text_ui(
        cache,
        &fonts.ui,
        text,
        rect.x + 8.0,
        rect.y + (rect.h - fonts.ui.geo.cell_h) * 0.5,
        (rect.w - 16.0 - shortcut_w).max(0.0),
        tokens.text_primary,
        layer,
    );
    if let Some(s) = shortcut {
        draw_text_ui(
            cache,
            &fonts.ui,
            s,
            rect.right() - shortcut_w - 4.0,
            rect.y + (rect.h - fonts.ui.geo.cell_h) * 0.5,
            shortcut_w,
            tokens.text_tertiary,
            layer,
        );
    }
}
