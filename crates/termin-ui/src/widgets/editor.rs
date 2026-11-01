//! Draw-only input editor widget (state and event routing live in the app).

use crate::{
    draw::{draw_text_mono, draw_text_ui, solid_quad},
    terminal_view::TermFonts,
    Tokens,
};
use termin_core::Rect;
use termin_editor::{EditorState, TokenKind};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

pub struct EditorWidget;

impl EditorWidget {
    #[allow(clippy::too_many_arguments)]
    pub fn render<B: FontBackend>(
        state: &EditorState,
        rect: Rect,
        focused: bool,
        running: bool,
        fonts: &TermFonts,
        glyphs: &mut GlyphCache<B>,
        tokens: &Tokens,
        layer: &mut Layer,
    ) {
        layer.clip = Some(rect);
        layer
            .quads
            .push(solid_quad(rect.x, rect.y, rect.w, rect.h, tokens.bg_input));
        layer.quads.push(solid_quad(
            rect.x,
            rect.y,
            rect.w,
            1.0,
            tokens.border_default,
        ));
        let line_h = fonts.geo.cell_h.max(1.0);
        let max_lines = ((rect.h / line_h).floor() as usize).clamp(1, 12);
        let first = state.text.split('\n').count().saturating_sub(max_lines);
        for (visible, line) in state.text.split('\n').enumerate().skip(first) {
            let y = rect.y + 8.0 + (visible - first) as f32 * line_h;
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                line,
                rect.x + 14.0,
                y,
                rect.w - 28.0,
                tokens.text_primary,
                layer,
            );
        }
        if state.text.is_empty() {
            draw_text_ui(
                glyphs,
                &fonts.ui,
                "Type a command…",
                rect.x + 14.0,
                rect.y + 8.0 + (line_h - fonts.ui.geo.cell_h) * 0.5,
                rect.w - 28.0,
                tokens.text_tertiary,
                layer,
            );
        }
        if let Some(range) = state.selection_range() {
            let before = &state.text[..range.start.min(state.text.len())];
            let selected =
                &state.text[range.start.min(state.text.len())..range.end.min(state.text.len())];
            let prefix_cols = before.chars().rev().take_while(|c| *c != '\n').count();
            let selected_cols = selected.chars().take_while(|c| *c != '\n').count();
            let row = before
                .bytes()
                .filter(|b| *b == b'\n')
                .count()
                .min(max_lines - 1);
            if selected_cols > 0 {
                layer.quads.push(solid_quad(
                    rect.x + 14.0 + prefix_cols as f32 * fonts.geo.cell_w,
                    rect.y + 8.0 + row as f32 * line_h,
                    selected_cols as f32 * fonts.geo.cell_w,
                    line_h,
                    tokens.selection,
                ));
            }
        }
        if let Some(suggestion) = &state.suggestion {
            let before = state
                .text
                .get(..state.cursor.min(state.text.len()))
                .unwrap_or("");
            let col = before.rsplit('\n').next().unwrap_or("").chars().count();
            let row = before
                .bytes()
                .filter(|b| *b == b'\n')
                .count()
                .min(max_lines - 1);
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                &suggestion.text,
                rect.x + 14.0 + col as f32 * fonts.geo.cell_w,
                rect.y + 8.0 + row as f32 * line_h,
                rect.w,
                tokens.syntax_ghost,
                layer,
            );
        }
        // Token colors are drawn over the base text only for non-default syntax.
        for token in &state.tokens {
            let Some(text) = state.text.get(token.range.clone()) else {
                continue;
            };
            let before = &state.text[..token.range.start];
            let col = before.rsplit('\n').next().unwrap_or("").chars().count();
            let row = before.bytes().filter(|b| *b == b'\n').count();
            if row < first || row >= first + max_lines {
                continue;
            }
            let color = match token.kind {
                TokenKind::Command => tokens.syntax_command,
                TokenKind::Argument | TokenKind::Subshell => tokens.syntax_argument,
                TokenKind::Flag => tokens.syntax_flag,
                TokenKind::String => tokens.syntax_string,
                TokenKind::Variable => tokens.syntax_variable,
                TokenKind::Operator => tokens.syntax_operator,
                TokenKind::Path => tokens.syntax_path,
                TokenKind::Comment => tokens.syntax_comment,
                TokenKind::Error => tokens.syntax_command_invalid,
                TokenKind::Whitespace => continue,
            };
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                text,
                rect.x + 14.0 + col as f32 * fonts.geo.cell_w,
                rect.y + 8.0 + (row - first) as f32 * line_h,
                rect.w,
                color,
                layer,
            );
        }
        if focused && !running {
            let before = state
                .text
                .get(..state.cursor.min(state.text.len()))
                .unwrap_or("");
            let col = before.rsplit('\n').next().unwrap_or("").chars().count();
            let row = before
                .bytes()
                .filter(|b| *b == b'\n')
                .count()
                .saturating_sub(first)
                .min(max_lines - 1);
            layer.quads.push(solid_quad(
                rect.x + 14.0 + col as f32 * fonts.geo.cell_w,
                rect.y + 8.0 + row as f32 * line_h,
                2.0,
                line_h,
                tokens.cursor,
            ));
        }
        if running {
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                "Running · input goes to process",
                rect.x + 14.0,
                rect.bottom() - line_h,
                rect.w - 28.0,
                tokens.text_tertiary,
                layer,
            );
        }
    }
}
