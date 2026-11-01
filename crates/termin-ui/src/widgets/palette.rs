//! Command palette panel renderer. Query state and ranking live in termin-app.
use crate::{
    draw::{draw_text_ui, rounded_quad, solid_quad},
    terminal_view::TermFonts,
    theme::Tokens,
};
use termin_core::{Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
#[derive(Clone, Debug)]
pub struct PaletteRow<'a> {
    pub title: &'a str,
    pub detail: Option<&'a str>,
    pub shortcut: Option<&'a str>,
    pub selected: bool,
}
pub fn draw<B: FontBackend>(
    bounds: Rect,
    query: &str,
    rows: &[PaletteRow<'_>],
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    let w = bounds.w.clamp(320.0, 680.0);
    let h = (58.0 + rows.len().min(10) as f32 * 34.0).min(bounds.h);
    let x = bounds.x + (bounds.w - w) * 0.5;
    let y = bounds.y + bounds.h * 0.18;
    layer.quads.push(rounded_quad(
        x,
        y,
        w,
        h,
        tokens.bg_elevated,
        10.0,
        1.0,
        tokens.border_subtle,
    ));
    layer.quads.push(solid_quad(
        x + 1.0,
        y + 48.0,
        w - 2.0,
        1.0,
        tokens.border_subtle,
    ));
    draw_text_ui(
        glyphs,
        &fonts.ui,
        if query.is_empty() {
            "Search actions, workspaces, tabs…"
        } else {
            query
        },
        x + 16.0,
        y + 16.0,
        w - 32.0,
        tokens.text_primary,
        layer,
    );
    for (i, row) in rows.iter().take(10).enumerate() {
        let ry = y + 54.0 + i as f32 * 34.0;
        if row.selected {
            layer.quads.push(rounded_quad(
                x + 6.0,
                ry,
                w - 12.0,
                32.0,
                tokens.bg_selected,
                5.0,
                0.0,
                Rgba::TRANSPARENT,
            ));
        }
        draw_text_ui(
            glyphs,
            &fonts.ui,
            row.title,
            x + 16.0,
            ry + 8.0,
            w * 0.55,
            tokens.text_primary,
            layer,
        );
        if let Some(detail) = row.detail {
            draw_text_ui(
                glyphs,
                &fonts.ui,
                detail,
                x + w * 0.56,
                ry + 8.0,
                w * 0.25,
                tokens.text_tertiary,
                layer,
            );
        }
        if let Some(keys) = row.shortcut {
            draw_text_ui(
                glyphs,
                &fonts.ui,
                keys,
                x + w - 100.0,
                ry + 8.0,
                88.0,
                tokens.text_secondary,
                layer,
            );
        }
    }
}
