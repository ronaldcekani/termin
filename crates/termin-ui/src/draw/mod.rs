//! Low-level DrawList helpers shared by the terminal view and widgets.
//!
//! Positions are logical points; glyph bitmap sizes are physical pixels
//! (see `docs/specs/text-and-rendering.md §3.2`).

pub mod icons;

use termin_core::Rgba;
use termin_render::{GlyphInst, Layer, QuadInst};
use termin_text::{AtlasKind, FontBackend, FontId, FontMetrics, GlyphCache, GlyphRef};

/// Top y of a `line_h`-high line box vertically centered in a box at `box_y` of height `box_h`.
#[inline]
pub fn center_y(box_y: f32, box_h: f32, line_h: f32) -> f32 {
    box_y + (box_h - line_h) * 0.5
}

/// Axis-aligned solid rectangle.
#[inline]
pub fn solid_quad(x: f32, y: f32, w: f32, h: f32, color: Rgba) -> QuadInst {
    QuadInst {
        rect: [x, y, w, h],
        color: color.0,
        border_color: 0,
        radius: 0.0,
        border: 0.0,
        shadow: 0.0,
        _pad: 0.0,
    }
}

/// Rounded rectangle with optional border ring.
#[allow(clippy::too_many_arguments)]
#[inline]
pub fn rounded_quad(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Rgba,
    radius: f32,
    border: f32,
    border_color: Rgba,
) -> QuadInst {
    QuadInst {
        rect: [x, y, w, h],
        color: color.0,
        border_color: border_color.0,
        radius,
        border,
        shadow: 0.0,
        _pad: 0.0,
    }
}

/// Translate a quad by `(dx, dy)`.
#[inline]
pub fn offset_quad(q: &QuadInst, dx: f32, dy: f32) -> QuadInst {
    let mut o = *q;
    o.rect[0] += dx;
    o.rect[1] += dy;
    o
}

/// Translate a glyph instance by `(dx, dy)`.
#[inline]
pub fn offset_glyph(g: &GlyphInst, dx: f32, dy: f32) -> GlyphInst {
    let mut o = *g;
    o.pos[0] += dx;
    o.pos[1] += dy;
    o
}

/// Vertical placement info used to position glyph bitmaps inside a text cell.
#[derive(Copy, Clone, Debug)]
pub struct CellGeometry {
    /// Cell width in points.
    pub cell_w: f32,
    /// Cell height in points.
    pub cell_h: f32,
    /// Baseline offset from the cell top, in points.
    pub baseline: f32,
    /// Backing scale factor (physical px per point).
    pub scale: f32,
}

impl CellGeometry {
    /// Cell height in physical pixels (rounded).
    #[inline]
    pub fn cell_px_h(&self) -> u32 {
        (self.cell_h * self.scale).round() as u32
    }
}

/// Rasterize (or fetch) `glyph` and build its instance for the cell whose
/// top-left corner is `(x, y)` in points. Returns the instance and whether it
/// belongs in the color atlas, or `None` for empty glyphs.
#[inline]
pub fn place_glyph<B: FontBackend>(
    cache: &mut GlyphCache<B>,
    font: FontId,
    glyph: GlyphRef,
    geo: &CellGeometry,
    x: f32,
    y: f32,
    color: Rgba,
) -> Option<(GlyphInst, bool)> {
    let g = cache.get_or_insert(font, glyph, geo.scale);
    if g.width == 0 || g.height == 0 {
        return None;
    }
    let inv = 1.0 / geo.scale;
    // Procedural glyphs (box drawing, blocks, braille) fill the whole cell bitmap.
    let full_cell = g.left == 0 && g.top == g.height as i32 && g.height == geo.cell_px_h();
    let gy = if full_cell {
        y
    } else {
        y + geo.baseline - g.top as f32 * inv
    };
    let gx = x + g.left as f32 * inv;
    let is_color = g.atlas == AtlasKind::Color;
    Some((
        GlyphInst {
            pos: [gx, gy],
            size: [
                g.width.min(u16::MAX as u32) as u16,
                g.height.min(u16::MAX as u32) as u16,
            ],
            uv: [
                g.uv[0],
                g.uv[1],
                g.uv[0].saturating_add(g.uv[2]),
                g.uv[1].saturating_add(g.uv[3]),
            ],
            color: if is_color { 0xFFFF_FFFF } else { color.0 },
        },
        is_color,
    ))
}

/// Draw a single line of monospace UI text into `layer`, truncating with `…`
/// when it would exceed `max_w`. Returns the drawn width in points.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_mono<B: FontBackend>(
    cache: &mut GlyphCache<B>,
    font: FontId,
    geo: &CellGeometry,
    text: &str,
    x: f32,
    y: f32,
    max_w: f32,
    color: Rgba,
    layer: &mut Layer,
) -> f32 {
    let max_cols = (max_w / geo.cell_w).floor() as usize;
    let n = text.chars().count();
    let truncated = n > max_cols;
    let take = if truncated {
        max_cols.saturating_sub(1)
    } else {
        n
    };
    let mut pen = x;
    let mut drawn = 0usize;
    let emit = |c: char, pen: f32, cache: &mut GlyphCache<B>, layer: &mut Layer| {
        if c == ' ' {
            return;
        }
        let gr = cache.backend.glyph(font, c);
        if let Some((inst, is_color)) = place_glyph(cache, font, gr, geo, pen, y, color) {
            if is_color {
                layer.color_glyphs.push(inst);
            } else {
                layer.glyphs.push(inst);
            }
        }
    };
    for c in text.chars().take(take) {
        emit(c, pen, cache, layer);
        pen += geo.cell_w;
        drawn += 1;
    }
    if truncated && max_cols > 0 {
        emit('…', pen, cache, layer);
        pen += geo.cell_w;
        drawn += 1;
    }
    let _ = drawn;
    pen - x
}

/// Proportional UI font (system sans) used for chrome text: tab titles, labels,
/// placeholders. `geo.cell_w` is the advance of `0` (useful as an em estimate);
/// `geo.cell_h` is the line height; `geo.baseline` is the baseline offset from the line top.
#[derive(Copy, Clone, Debug)]
pub struct UiFont {
    pub font: FontId,
    pub geo: CellGeometry,
}

impl UiFont {
    /// Derive line geometry from backend metrics (points) at backing `scale`.
    pub fn new(font: FontId, m: &FontMetrics, scale: f32) -> Self {
        let snap = |v: f32| (v * scale).round() / scale;
        let cell_h = snap(m.cell_height.max(1.0));
        let content = m.ascent + m.descent;
        let baseline = snap(((cell_h - content) * 0.5 + m.ascent).max(m.ascent.min(cell_h)));
        Self {
            font,
            geo: CellGeometry {
                cell_w: m.cell_width.max(1.0),
                cell_h,
                baseline,
                scale,
            },
        }
    }
}

/// Width in points of `text` set in the UI font (no truncation).
pub fn measure_text_ui<B: FontBackend>(cache: &mut GlyphCache<B>, ui: &UiFont, text: &str) -> f32 {
    let mut w = 0.0;
    for c in text.chars() {
        let gr = cache.backend.glyph(ui.font, c);
        w += cache.backend.glyph_advance(ui.font, gr);
    }
    w
}

/// Draw one line of proportional UI text with its line box top-left at `(x, y)` (points,
/// line height `ui.geo.cell_h`). Text wider than `max_w` is truncated with `…`. Pen positions
/// are snapped to device pixels so stems stay crisp. Returns the drawn width in points.
#[allow(clippy::too_many_arguments)]
pub fn draw_text_ui<B: FontBackend>(
    cache: &mut GlyphCache<B>,
    ui: &UiFont,
    text: &str,
    x: f32,
    y: f32,
    max_w: f32,
    color: Rgba,
    layer: &mut Layer,
) -> f32 {
    let scale = ui.geo.scale;
    let ellipsis = cache.backend.glyph(ui.font, '…');
    let ell_w = cache.backend.glyph_advance(ui.font, ellipsis);
    // Pass 1: total width and the longest prefix that leaves room for the ellipsis.
    let mut total = 0.0;
    let mut cut = 0usize;
    for (i, c) in text.char_indices() {
        if total + ell_w <= max_w {
            cut = i;
        }
        let gr = cache.backend.glyph(ui.font, c);
        total += cache.backend.glyph_advance(ui.font, gr);
    }
    let truncated = total > max_w;
    let end = if truncated { cut } else { text.len() };
    let mut pen = 0.0f32;
    let emit = |gr: GlyphRef, pen: f32, cache: &mut GlyphCache<B>, layer: &mut Layer| {
        let px = x + (pen * scale).round() / scale;
        if let Some((inst, is_color)) = place_glyph(cache, ui.font, gr, &ui.geo, px, y, color) {
            if is_color {
                layer.color_glyphs.push(inst);
            } else {
                layer.glyphs.push(inst);
            }
        }
    };
    for c in text[..end].chars() {
        let gr = cache.backend.glyph(ui.font, c);
        let adv = cache.backend.glyph_advance(ui.font, gr);
        if c != ' ' {
            emit(gr, pen, cache, layer);
        }
        pen += adv;
    }
    if truncated {
        emit(ellipsis, pen, cache, layer);
        pen += ell_w;
    }
    pen
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_y_centers_line_box() {
        assert_eq!(center_y(10.0, 38.0, 16.0), 21.0);
        assert_eq!(center_y(0.0, 24.0, 24.0), 0.0);
        // Line taller than the box overhangs symmetrically.
        assert_eq!(center_y(0.0, 10.0, 14.0), -2.0);
    }

    #[test]
    fn solid_quad_fields() {
        let q = solid_quad(1.0, 2.0, 3.0, 4.0, Rgba::from_rgb(1, 2, 3));
        assert_eq!(q.rect, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(q.color, Rgba::from_rgb(1, 2, 3).0);
        let o = offset_quad(&q, 10.0, 20.0);
        assert_eq!(o.rect, [11.0, 22.0, 3.0, 4.0]);
    }
}
