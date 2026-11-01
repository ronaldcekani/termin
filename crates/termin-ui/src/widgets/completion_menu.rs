//! Completion popup drawn above or below the editor anchor.
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::TermFonts,
    Tokens,
};
use termin_core::Rect;
use termin_editor::CompletionMenu;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

pub struct CompletionMenuWidget;

impl CompletionMenuWidget {
    pub fn render<B: FontBackend>(
        menu: &CompletionMenu,
        anchor: Rect,
        available: Rect,
        fonts: &TermFonts,
        glyphs: &mut GlyphCache<B>,
        tokens: &Tokens,
        layer: &mut Layer,
    ) {
        if menu.candidates.is_empty() {
            return;
        }
        let row_h = 26.0;
        let height = (menu.candidates.len().min(10) as f32 * row_h) + 8.0;
        let above_y = anchor.y - height;
        let below_y = anchor.bottom();
        let y = if above_y >= available.y {
            above_y
        } else if below_y + height <= available.bottom() {
            below_y
        } else {
            (available.bottom() - height).max(available.y)
        };
        let rect = Rect::new(
            anchor.x,
            y,
            anchor.w.clamp(280.0, 560.0).min(available.w),
            height.min(available.h),
        );
        layer.quads.push(rounded_quad(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            tokens.bg_elevated,
            8.0,
            1.0,
            tokens.border_subtle,
        ));
        for (idx, candidate) in menu.candidates.iter().take(10).enumerate() {
            let row_y = rect.y + 4.0 + idx as f32 * row_h;
            if idx == menu.selected {
                layer.quads.push(rounded_quad(
                    rect.x + 4.0,
                    row_y,
                    rect.w - 8.0,
                    row_h,
                    tokens.bg_selected,
                    4.0,
                    0.0,
                    tokens.bg_selected,
                ));
            }
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                &candidate.label,
                rect.x + 12.0,
                row_y + 5.0,
                rect.w * 0.55,
                tokens.text_primary,
                layer,
            );
            if let Some(detail) = &candidate.detail {
                draw_text_mono(
                    glyphs,
                    fonts.font,
                    &fonts.geo,
                    detail,
                    rect.x + rect.w * 0.60,
                    row_y + 5.0,
                    rect.w * 0.35,
                    tokens.text_tertiary,
                    layer,
                );
            }
        }
    }
}
