//! Folder-search dropdown anchored under the top-bar search field.

use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::{Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

/// Row height in points.
pub const ROW_H: f32 = 30.0;
/// Maximum rows visible at once.
pub const MAX_ROWS: usize = 8;
const PAD: f32 = 6.0;

/// One dropdown row.
pub struct FolderRow<'a> {
    /// Folder basename.
    pub name: &'a str,
    /// Full path with `~` for home.
    pub detail: &'a str,
    pub selected: bool,
}

/// Dropdown rect for `rows` visible rows (at least one, for the empty label).
pub fn dropdown_rect(search: Rect, rows: usize, window_w: f32) -> Rect {
    let w = search.w.max(360.0).min((window_w - 16.0).max(0.0));
    let x = (search.x + (search.w - w) * 0.5).clamp(8.0, (window_w - w - 8.0).max(8.0));
    let n = rows.clamp(1, MAX_ROWS) as f32;
    Rect::new(x, search.bottom() + 6.0, w, PAD * 2.0 + n * ROW_H)
}

/// Rect of row `i` inside `dropdown`.
pub fn row_rect(dropdown: Rect, i: usize) -> Rect {
    Rect::new(
        dropdown.x + PAD,
        dropdown.y + PAD + i as f32 * ROW_H,
        dropdown.w - 2.0 * PAD,
        ROW_H,
    )
}

/// Row index under `(x, y)` among `rows` visible rows.
pub fn row_at(dropdown: Rect, rows: usize, x: f32, y: f32) -> Option<usize> {
    (0..rows.min(MAX_ROWS)).find(|&i| {
        let r = row_rect(dropdown, i);
        x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
    })
}

/// Draw the dropdown. `empty_label` is shown when `rows` is empty.
pub fn draw_dropdown<B: FontBackend>(
    dropdown: Rect,
    rows: &[FolderRow<'_>],
    empty_label: &str,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    layer.quads.push(rounded_quad(
        dropdown.x,
        dropdown.y,
        dropdown.w,
        dropdown.h,
        tokens.bg_elevated,
        10.0,
        1.0,
        tokens.border_default,
    ));
    let cell_w = fonts.geo.cell_w;
    if rows.is_empty() {
        let r = row_rect(dropdown, 0);
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            empty_label,
            r.x + 10.0,
            r.y + (r.h - fonts.geo.cell_h) * 0.5,
            r.w - 20.0,
            tokens.text_tertiary,
            layer,
        );
        return;
    }
    for (i, row) in rows.iter().take(MAX_ROWS).enumerate() {
        let r = row_rect(dropdown, i);
        if row.selected {
            layer.quads.push(rounded_quad(
                r.x,
                r.y,
                r.w,
                r.h,
                tokens.bg_selected,
                6.0,
                0.0,
                Rgba::TRANSPARENT,
            ));
        }
        let ty = r.y + (r.h - fonts.geo.cell_h) * 0.5;
        let name_w = (r.w - 20.0).min(row.name.chars().count() as f32 * cell_w);
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            row.name,
            r.x + 10.0,
            ty,
            r.w - 20.0,
            tokens.text_primary,
            layer,
        );
        let dx = r.x + 10.0 + name_w + 2.0 * cell_w;
        if dx + 4.0 * cell_w < r.right() - 10.0 {
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                row.detail,
                dx,
                ty,
                r.right() - 10.0 - dx,
                tokens.text_tertiary,
                layer,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropdown_anchors_under_search_and_stays_on_screen() {
        let search = Rect::new(300.0, 6.0, 400.0, 26.0);
        let d = dropdown_rect(search, 3, 1000.0);
        assert!(d.y >= search.bottom());
        assert!(d.x >= 8.0 && d.right() <= 992.0);
        assert_eq!(dropdown_rect(search, 0, 1000.0).h, PAD * 2.0 + ROW_H);
        assert_eq!(
            dropdown_rect(search, 100, 1000.0).h,
            PAD * 2.0 + MAX_ROWS as f32 * ROW_H
        );
    }

    #[test]
    fn row_hit_testing() {
        let d = dropdown_rect(Rect::new(300.0, 6.0, 400.0, 26.0), 3, 1000.0);
        let r1 = row_rect(d, 1);
        assert_eq!(row_at(d, 3, r1.x + 5.0, r1.y + 5.0), Some(1));
        let r3 = row_rect(d, 3);
        assert_eq!(row_at(d, 3, r3.x + 5.0, r3.y + 5.0), None);
    }
}
