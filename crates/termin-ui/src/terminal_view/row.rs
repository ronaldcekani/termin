//! Row instance building: cells → quads and glyph instances in row-local coordinates.
//!
//! Instance `x` is pane-local (column × cell width); instance `y` is relative to the
//! row's top edge, so rows can be reordered (scroll reuse) and translated without
//! rebuilding (`docs/specs/text-and-rendering.md §3.4`).

use termin_core::Rgba;
use termin_render::{GlyphInst, QuadInst};
use termin_text::{FontBackend, FontId, FontMetrics, GlyphCache};
use termin_vt::{Attrs, Cell, CellFlags, StyleTable, Terminal};

use super::palette::TermPalette;
use crate::draw::{place_glyph, solid_quad, CellGeometry};

/// Font + cell geometry for the terminal grid.
#[derive(Copy, Clone, Debug)]
pub struct TermFonts {
    pub font: FontId,
    pub geo: CellGeometry,
    /// Underline top offset from the row top, in points.
    pub underline_y: f32,
    pub underline_h: f32,
}

impl TermFonts {
    /// Derive grid geometry from CoreText-style metrics (all values in points).
    pub fn new(font: FontId, m: &FontMetrics, scale: f32) -> Self {
        let snap = |v: f32| (v * scale).round() / scale;
        let cell_w = m.cell_width.max(1.0);
        let cell_h = m.cell_height.max(1.0);
        let content = m.ascent + m.descent;
        let baseline = snap(((cell_h - content) * 0.5 + m.ascent).max(m.ascent.min(cell_h)));
        let thick = snap(m.underline_thickness.max(1.0 / scale));
        let ul = snap(baseline - m.underline_position).min(cell_h - thick);
        Self {
            font,
            geo: CellGeometry {
                cell_w,
                cell_h,
                baseline,
                scale,
            },
            underline_y: ul,
            underline_h: thick,
        }
    }
}

/// Everything the row builder needs besides the cells themselves.
pub struct RowCtx<'a, B: FontBackend> {
    pub glyphs: &'a mut GlyphCache<B>,
    pub fonts: &'a TermFonts,
    pub palette: &'a TermPalette,
}

/// Cached draw instances for one visible row.
#[derive(Clone, Debug, Default)]
pub struct RowInstances {
    pub bg: Vec<QuadInst>,
    pub glyphs: Vec<GlyphInst>,
    pub color_glyphs: Vec<GlyphInst>,
    pub deco: Vec<QuadInst>,
    /// Content hash of the cells this row was built from.
    pub hash: u64,
}

impl RowInstances {
    fn clear(&mut self) {
        self.bg.clear();
        self.glyphs.clear();
        self.color_glyphs.clear();
        self.deco.clear();
    }
}

/// Fx-style content hash over a row's cells.
#[inline]
pub fn hash_cells(cells: &[Cell]) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let mut h: u64 = cells.len() as u64;
    for c in cells {
        let v = (c.ch as u64) | ((c.style as u64) << 32) | ((c.flags.bits() as u64) << 48);
        h = (h.rotate_left(5) ^ v).wrapping_mul(K);
    }
    h
}

#[inline]
fn cell_char(cell: &Cell, term: &Terminal) -> char {
    if let Some(i) = cell.grapheme_index() {
        term.grapheme(i)
            .and_then(|s| s.chars().next())
            .unwrap_or(' ')
    } else {
        match char::from_u32(cell.ch) {
            Some('\0') | None => ' ',
            Some(c) => c,
        }
    }
}

/// Character at a cell (used by the cursor overlay).
pub fn char_of_cell(cell: &Cell, term: &Terminal) -> char {
    cell_char(cell, term)
}

/// Rebuild `out` from `cells`.
pub fn build_row<B: FontBackend>(
    cells: &[Cell],
    styles: &StyleTable,
    term: &Terminal,
    ctx: &mut RowCtx<'_, B>,
    out: &mut RowInstances,
) {
    out.clear();
    out.hash = hash_cells(cells);
    let geo = ctx.fonts.geo;
    let font = ctx.fonts.font;
    let default_bg = ctx.palette.bg;
    let cell_w = geo.cell_w;

    let mut run_start = 0usize;
    let mut run_color = default_bg;
    let mut last_style = u16::MAX;
    let mut fg = ctx.palette.fg;
    let mut bg = default_bg;
    let mut attrs = Attrs::empty();
    let mut ul_color: Option<Rgba> = None;

    for (col, cell) in cells.iter().enumerate() {
        if cell.style != last_style {
            last_style = cell.style;
            let st = styles.get_or_default(cell.style);
            let (f, b) = ctx.palette.resolve_style(&st);
            fg = f;
            bg = b;
            attrs = st.attrs;
            ul_color = match st.ul_color {
                termin_vt::Color::Default => None,
                c => Some(ctx.palette.resolve_bg(c)),
            };
        }

        // Background run merging.
        if bg != run_color {
            if run_color != default_bg {
                out.bg.push(solid_quad(
                    run_start as f32 * cell_w,
                    0.0,
                    (col - run_start) as f32 * cell_w,
                    geo.cell_h,
                    run_color,
                ));
            }
            run_start = col;
            run_color = bg;
        }

        if cell.flags.contains(CellFlags::WIDE_SPACER) {
            continue;
        }

        let x = col as f32 * cell_w;
        let ch = cell_char(cell, term);

        if ch != ' ' && !attrs.contains(Attrs::HIDDEN) && !attrs.contains(Attrs::BLINK) {
            let gr = ctx.glyphs.backend.glyph_styled(
                font,
                ch,
                attrs.contains(Attrs::BOLD),
                attrs.contains(Attrs::ITALIC),
            );
            if let Some((inst, is_color)) = place_glyph(ctx.glyphs, font, gr, &geo, x, 0.0, fg) {
                if is_color {
                    out.color_glyphs.push(inst);
                } else {
                    out.glyphs.push(inst);
                }
            }
        } else if ch != ' ' && !attrs.contains(Attrs::HIDDEN) {
            // Blinking text is rendered steady (no timers when idle).
            let gr = ctx.glyphs.backend.glyph(font, ch);
            if let Some((inst, is_color)) = place_glyph(ctx.glyphs, font, gr, &geo, x, 0.0, fg) {
                if is_color {
                    out.color_glyphs.push(inst);
                } else {
                    out.glyphs.push(inst);
                }
            }
        }

        // Decorations.
        let w = if cell.flags.contains(CellFlags::WIDE) {
            cell_w * 2.0
        } else {
            cell_w
        };
        let dcol = ul_color.unwrap_or(fg);
        if attrs.intersects(
            Attrs::UNDERLINE
                | Attrs::DOUBLE_UNDERLINE
                | Attrs::CURLY_UNDERLINE
                | Attrs::DOTTED_UNDERLINE
                | Attrs::DASHED_UNDERLINE,
        ) {
            out.deco.push(solid_quad(
                x,
                ctx.fonts.underline_y,
                w,
                ctx.fonts.underline_h,
                dcol,
            ));
            if attrs.contains(Attrs::DOUBLE_UNDERLINE) {
                let y2 = (ctx.fonts.underline_y - 2.0 * ctx.fonts.underline_h).max(0.0);
                out.deco
                    .push(solid_quad(x, y2, w, ctx.fonts.underline_h, dcol));
            }
        }
        if attrs.contains(Attrs::STRIKE) {
            out.deco.push(solid_quad(
                x,
                (geo.baseline * 0.62 - ctx.fonts.underline_h * 0.5).max(0.0),
                w,
                ctx.fonts.underline_h,
                fg,
            ));
        }
        if attrs.contains(Attrs::OVERLINE) {
            out.deco
                .push(solid_quad(x, 0.0, w, ctx.fonts.underline_h, fg));
        }
    }

    if run_color != default_bg {
        out.bg.push(solid_quad(
            run_start as f32 * cell_w,
            0.0,
            (cells.len() - run_start) as f32 * cell_w,
            geo.cell_h,
            run_color,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_changes_with_content() {
        let a = vec![Cell::default(); 8];
        let mut b = a.clone();
        assert_eq!(hash_cells(&a), hash_cells(&b));
        b[3] = Cell::new('x', 0, CellFlags::empty());
        assert_ne!(hash_cells(&a), hash_cells(&b));
        b[3] = Cell::new(' ', 1, CellFlags::empty());
        assert_ne!(hash_cells(&a), hash_cells(&b));
    }
}
