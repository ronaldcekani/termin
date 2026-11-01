//! Terminal view component (`termin-ui::terminal_view`).
//!
//! Handles row rendering, cursor drawing, selection highlights, and damage caching.
//! See `docs/specs/text-and-rendering.md §3` and `docs/specs/app-model-and-ui.md §5.1`.

pub mod cache;
pub mod palette;
pub mod row;

pub use cache::{PaneRenderCache, ViewKey};
pub use palette::TermPalette;
pub use row::{build_row, char_of_cell, hash_cells, RowCtx, RowInstances, TermFonts};

use termin_core::{Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
use termin_vt::{AbsPos, CursorShape, Terminal};

use crate::draw::{offset_glyph, offset_quad, rounded_quad, solid_quad};

/// View options for rendering a terminal pane.
#[derive(Clone, Debug)]
pub struct TerminalViewOpts {
    pub rect: Rect,
    pub focused: bool,
    pub cursor_blink_visible: bool,
    pub selection: Option<(AbsPos, AbsPos)>,
}

/// Helper to render the terminal grid into a DrawList layer.
pub struct TerminalView;

impl TerminalView {
    /// Render `cache` rows into `layer`, applying cursor and selection highlights.
    pub fn render<B: FontBackend>(
        term: &Terminal,
        cache: &PaneRenderCache,
        fonts: &TermFonts,
        palette: &TermPalette,
        opts: &TerminalViewOpts,
        glyphs: &mut GlyphCache<B>,
        layer: &mut Layer,
    ) {
        let cell_w = fonts.geo.cell_w;
        let cell_h = fonts.geo.cell_h;
        let rect = opts.rect;

        // Clip layer to pane bounds
        layer.clip = Some(rect);

        // 1. Render cached rows
        for (i, row) in cache.rows().iter().enumerate() {
            let row_y = rect.y + i as f32 * cell_h;
            if row_y + cell_h < rect.y || row_y > rect.bottom() {
                continue;
            }

            for q in &row.bg {
                layer.quads.push(offset_quad(q, rect.x, row_y));
            }
            for d in &row.deco {
                layer.quads.push(offset_quad(d, rect.x, row_y));
            }
            for g in &row.glyphs {
                layer.glyphs.push(offset_glyph(g, rect.x, row_y));
            }
            for cg in &row.color_glyphs {
                layer.color_glyphs.push(offset_glyph(cg, rect.x, row_y));
            }
        }

        // 2. Selection highlights
        if let Some((start, end)) = opts.selection {
            let (sel_start, sel_end) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            let sel_color = palette.selection_bg;

            let view_top_abs = term.scrollback_len().saturating_sub(term.display_scroll()) as i64;
            let rows = term.rows() as usize;
            let cols = term.cols() as usize;

            for row_idx in 0..rows {
                let abs_line = (view_top_abs + row_idx as i64) as u64;
                if abs_line < sel_start.line.0 || abs_line > sel_end.line.0 {
                    continue;
                }

                let col_start = if abs_line == sel_start.line.0 {
                    sel_start.col as usize
                } else {
                    0
                };
                let col_end = if abs_line == sel_end.line.0 {
                    (sel_end.col as usize + 1).min(cols)
                } else {
                    cols
                };

                if col_end > col_start {
                    let sx = rect.x + col_start as f32 * cell_w;
                    let sy = rect.y + row_idx as f32 * cell_h;
                    let sw = (col_end - col_start) as f32 * cell_w;
                    layer.quads.push(solid_quad(sx, sy, sw, cell_h, sel_color));
                }
            }
        }

        // 3. Cursor
        let cursor = term.cursor();
        if term.display_scroll() == 0 && cursor.visible && (opts.focused || !cursor.blink) {
            let (cursor_col, cursor_row) = term.cursor_screen_pos();
            let cx = rect.x + cursor_col as f32 * cell_w;
            let cy = rect.y + cursor_row as f32 * cell_h;

            if opts.focused {
                if opts.cursor_blink_visible || !cursor.blink {
                    match cursor.shape {
                        CursorShape::Block => {
                            layer
                                .quads
                                .push(solid_quad(cx, cy, cell_w, cell_h, palette.cursor));
                            // Draw character under cursor in cursor_text color
                            if let Some(cell) = term.cell_at(cursor_col, cursor_row) {
                                let ch = char_of_cell(cell, term);
                                if ch != ' ' {
                                    let gr = glyphs.backend.glyph(fonts.font, ch);
                                    if let Some((cg, is_color)) = crate::draw::place_glyph(
                                        glyphs,
                                        fonts.font,
                                        gr,
                                        &fonts.geo,
                                        cx,
                                        cy,
                                        palette.cursor_text,
                                    ) {
                                        if is_color {
                                            layer.color_glyphs.push(cg);
                                        } else {
                                            layer.glyphs.push(cg);
                                        }
                                    }
                                }
                            }
                        }
                        CursorShape::Underline => {
                            layer.quads.push(solid_quad(
                                cx,
                                cy + fonts.underline_y,
                                cell_w,
                                fonts.underline_h * 2.0,
                                palette.cursor,
                            ));
                        }
                        CursorShape::Bar => {
                            layer
                                .quads
                                .push(solid_quad(cx, cy, 2.0, cell_h, palette.cursor));
                        }
                    }
                }
            } else {
                // Unfocused pane draws hollow box cursor
                layer.quads.push(rounded_quad(
                    cx,
                    cy,
                    cell_w,
                    cell_h,
                    Rgba::TRANSPARENT,
                    0.0,
                    1.0,
                    palette.cursor,
                ));
            }
        }
    }
}
