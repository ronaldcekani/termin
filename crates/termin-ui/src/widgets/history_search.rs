//! History search panel for the editor.
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::TermFonts,
    Tokens,
};
use termin_core::Rect;
use termin_editor::HistorySearchResult;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

pub struct HistorySearchWidget;

impl HistorySearchWidget {
    #[allow(clippy::too_many_arguments)]
    pub fn render<B: FontBackend>(
        query: &str,
        results: &[HistorySearchResult],
        selected: usize,
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
            10.0,
            1.0,
            tokens.border_subtle,
        ));
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            query,
            rect.x + 12.0,
            rect.y + 10.0,
            rect.w - 24.0,
            tokens.text_primary,
            layer,
        );
        let rows = ((rect.h - 42.0) / 30.0).floor().max(0.0) as usize;
        for (idx, result) in results.iter().take(rows).enumerate() {
            let y = rect.y + 34.0 + idx as f32 * 30.0;
            if idx == selected {
                layer.quads.push(rounded_quad(
                    rect.x + 4.0,
                    y,
                    rect.w - 8.0,
                    28.0,
                    tokens.bg_selected,
                    5.0,
                    0.0,
                    tokens.bg_selected,
                ));
            }
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                &result.command,
                rect.x + 12.0,
                y + 5.0,
                rect.w * 0.68,
                tokens.text_primary,
                layer,
            );
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                &result.cwd,
                rect.x + rect.w * 0.72,
                y + 5.0,
                rect.w * 0.24,
                tokens.text_tertiary,
                layer,
            );
        }
    }
}
