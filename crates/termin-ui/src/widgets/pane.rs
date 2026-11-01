//! Pane widget with tab strip chrome, split drop zones, and content framing (`termin-ui::widgets::pane`).
//!
//! See `docs/design/components.md §3, §15`.

use termin_core::{Point, Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

use crate::draw::{draw_text_mono, rounded_quad, solid_quad};
use crate::terminal_view::row::TermFonts;
use crate::theme::Tokens;

pub const TAB_STRIP_HEIGHT: f32 = 38.0;

/// Drop zone within a pane when dragging a tab.
///
/// See `docs/design/components.md §15`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SplitDropZone {
    /// Split pane horizontally to the left.
    Left,
    /// Split pane horizontally to the right.
    Right,
    /// Split pane vertically to the top.
    Top,
    /// Split pane vertically to the bottom.
    Bottom,
    /// Drop into the existing pane as a new tab.
    Center,
}

impl SplitDropZone {
    /// Detect which drop zone a point falls into within `pane_rect`.
    ///
    /// The outer 25% margins trigger directional splits (Left, Right, Top, Bottom),
    /// while the central 50% triggers dropping into the pane's tab strip (Center).
    pub fn detect(pane_rect: Rect, pt: Point) -> Option<Self> {
        if !pane_rect.contains(pt) || pane_rect.w <= 0.0 || pane_rect.h <= 0.0 {
            return None;
        }

        let rel_x = (pt.x - pane_rect.x) / pane_rect.w;
        let rel_y = (pt.y - pane_rect.y) / pane_rect.h;

        let d_left = rel_x;
        let d_right = 1.0 - rel_x;
        let d_top = rel_y;
        let d_bottom = 1.0 - rel_y;

        const EDGE_THRESHOLD: f32 = 0.25;

        // If in the edge margins, pick the closest edge
        if d_left < EDGE_THRESHOLD && d_left <= d_top && d_left <= d_bottom {
            Some(Self::Left)
        } else if d_right < EDGE_THRESHOLD && d_right <= d_top && d_right <= d_bottom {
            Some(Self::Right)
        } else if d_top < EDGE_THRESHOLD {
            Some(Self::Top)
        } else if d_bottom < EDGE_THRESHOLD {
            Some(Self::Bottom)
        } else {
            Some(Self::Center)
        }
    }

    /// Compute the preview bounding rectangle for this drop zone.
    pub fn preview_rect(&self, pane_rect: Rect) -> Rect {
        match self {
            Self::Left => Rect::new(pane_rect.x, pane_rect.y, pane_rect.w * 0.5, pane_rect.h),
            Self::Right => Rect::new(
                pane_rect.x + pane_rect.w * 0.5,
                pane_rect.y,
                pane_rect.w * 0.5,
                pane_rect.h,
            ),
            Self::Top => Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, pane_rect.h * 0.5),
            Self::Bottom => Rect::new(
                pane_rect.x,
                pane_rect.y + pane_rect.h * 0.5,
                pane_rect.w,
                pane_rect.h * 0.5,
            ),
            Self::Center => pane_rect,
        }
    }

    /// Render the translucent `tide` drop preview quad into `layer`.
    ///
    /// Per `components.md §15`: draws a translucent `tide` fill with a 2 pt `tide` border.
    pub fn render_preview(&self, pane_rect: Rect, tokens: &Tokens, layer: &mut Layer) {
        let preview = self.preview_rect(pane_rect);
        if preview.w <= 0.0 || preview.h <= 0.0 {
            return;
        }

        // Translucent tide fill (~25% alpha)
        let fill = Rgba::from_rgba(
            tokens.accent_tide.r(),
            tokens.accent_tide.g(),
            tokens.accent_tide.b(),
            0x40,
        );
        let border = tokens.accent_tide;

        layer.quads.push(rounded_quad(
            preview.x, preview.y, preview.w, preview.h, fill, 4.0, 2.0, border,
        ));
    }
}

/// Render pane container chrome: tab strip, title, and inactive dimming.
pub struct PaneWidget;

impl PaneWidget {
    /// Compute the terminal content rect inside a pane's allocated bounds.
    pub fn content_rect(pane_rect: Rect) -> Rect {
        Rect::new(
            pane_rect.x,
            pane_rect.y + TAB_STRIP_HEIGHT,
            pane_rect.w,
            (pane_rect.h - TAB_STRIP_HEIGHT).max(0.0),
        )
    }

    /// Render tab strip and background chrome into `layer`.
    pub fn render_chrome<B: FontBackend>(
        pane_rect: Rect,
        focused: bool,
        tab_title: &str,
        tokens: &Tokens,
        fonts: &TermFonts,
        glyphs: &mut GlyphCache<B>,
        layer: &mut Layer,
    ) {
        let strip_rect = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);

        // Strip background
        layer.quads.push(solid_quad(
            strip_rect.x,
            strip_rect.y,
            strip_rect.w,
            strip_rect.h,
            tokens.bg_sidebar,
        ));

        // Active tab chip
        let chip_w = (pane_rect.w * 0.4).clamp(120.0, 240.0);
        let chip_rect = Rect::new(strip_rect.x + 4.0, strip_rect.y + 2.0, chip_w, 24.0);
        let chip_bg = if focused {
            tokens.bg_surface
        } else {
            tokens.bg_app
        };
        layer.quads.push(rounded_quad(
            chip_rect.x,
            chip_rect.y,
            chip_rect.w,
            chip_rect.h,
            chip_bg,
            4.0,
            0.0,
            Rgba::TRANSPARENT,
        ));

        // Tab title text
        let text_color = if focused {
            tokens.text_primary
        } else {
            tokens.text_secondary
        };
        let text_x = chip_rect.x + 8.0;
        let text_y = chip_rect.y + 5.0;
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            tab_title,
            text_x,
            text_y,
            chip_rect.w - 24.0,
            text_color,
            layer,
        );

        // New tab / split button indicators on the right
        let right_pen = strip_rect.right() - 20.0;
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            "+",
            right_pen,
            strip_rect.y + 5.0,
            16.0,
            tokens.text_muted,
            layer,
        );

        // Inactive dimming overlay for the pane content
        if !focused {
            let content = Self::content_rect(pane_rect);
            layer.quads.push(solid_quad(
                content.x,
                content.y,
                content.w,
                content.h,
                Rgba::from_rgba(0x0C, 0x0C, 0x0D, 0x30),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_drop_zones() {
        let pane = Rect::new(0.0, 0.0, 400.0, 200.0);

        // Outside
        assert_eq!(SplitDropZone::detect(pane, Point::new(-10.0, 50.0)), None);
        assert_eq!(SplitDropZone::detect(pane, Point::new(450.0, 50.0)), None);

        // Left edge (< 25% = 100pt)
        assert_eq!(
            SplitDropZone::detect(pane, Point::new(50.0, 100.0)),
            Some(SplitDropZone::Left)
        );

        // Right edge (> 300pt)
        assert_eq!(
            SplitDropZone::detect(pane, Point::new(350.0, 100.0)),
            Some(SplitDropZone::Right)
        );

        // Top edge (< 50pt)
        assert_eq!(
            SplitDropZone::detect(pane, Point::new(200.0, 20.0)),
            Some(SplitDropZone::Top)
        );

        // Bottom edge (> 150pt)
        assert_eq!(
            SplitDropZone::detect(pane, Point::new(200.0, 180.0)),
            Some(SplitDropZone::Bottom)
        );

        // Center
        assert_eq!(
            SplitDropZone::detect(pane, Point::new(200.0, 100.0)),
            Some(SplitDropZone::Center)
        );
    }

    #[test]
    fn test_drop_preview_rendering() {
        let pane = Rect::new(0.0, 0.0, 400.0, 200.0);
        let tokens = Tokens::default();
        let mut layer = Layer::default();

        let left_preview = SplitDropZone::Left.preview_rect(pane);
        assert_eq!(left_preview, Rect::new(0.0, 0.0, 200.0, 200.0));

        SplitDropZone::Left.render_preview(pane, &tokens, &mut layer);
        assert_eq!(layer.quads.len(), 1);
        let quad = &layer.quads[0];
        assert_eq!(quad.border_color, tokens.accent_tide.0);
        assert_eq!(quad.rect[2], 200.0);
        assert_eq!(quad.rect[3], 200.0);
    }
}
