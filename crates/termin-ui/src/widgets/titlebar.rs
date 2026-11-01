//! Warp-style window top bar: traffic-light gap, sidebar toggle, new-workspace
//! button and the centered folder-search field (`docs/design/components.md §1`).

use crate::{
    draw::{draw_text_mono, icons, rounded_quad, solid_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
    widgets::text_field::TextField,
};
use termin_core::Rect;
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

/// Height of the top bar (shared with the macOS traffic-light row).
pub const TITLEBAR_H: f32 = 38.0;
/// Width reserved at the bar's left for the traffic-light buttons.
pub const TRAFFIC_LIGHTS_W: f32 = 78.0;
const BTN: f32 = 24.0;
const SEARCH_H: f32 = 26.0;
const MARGIN: f32 = 12.0;

/// Geometry of the bar's controls, shared by drawing, hit testing and drag regions.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TitlebarLayout {
    pub toggle: Rect,
    pub new_workspace: Rect,
    pub search: Rect,
}

impl TitlebarLayout {
    pub fn new(window_w: f32) -> Self {
        let y = (TITLEBAR_H - BTN) * 0.5;
        let toggle = Rect::new(TRAFFIC_LIGHTS_W, y, BTN, BTN);
        let new_workspace = Rect::new(toggle.right() + 4.0, y, BTN, BTN);
        let left = new_workspace.right() + MARGIN;
        let avail = (window_w - left - MARGIN).max(0.0);
        let w = (window_w * 0.4).clamp(240.0, 560.0).min(avail);
        let max_x = (window_w - MARGIN - w).max(left);
        let x = ((window_w - w) * 0.5).clamp(left, max_x);
        let search = Rect::new(x, (TITLEBAR_H - SEARCH_H) * 0.5, w, SEARCH_H);
        Self {
            toggle,
            new_workspace,
            search,
        }
    }
}

/// What the bar shows this frame.
pub struct TitlebarView<'a> {
    pub sidebar_visible: bool,
    /// `Some` while the search field is focused.
    pub query: Option<&'a TextField>,
    pub caret_visible: bool,
}

/// Slice of `text` holding at most `cols` chars starting at char index `start`.
fn visible_slice(text: &str, start: usize, cols: usize) -> &str {
    let b0 = text
        .char_indices()
        .nth(start)
        .map_or(text.len(), |(b, _)| b);
    let rest = &text[b0..];
    let b1 = rest.char_indices().nth(cols).map_or(rest.len(), |(b, _)| b);
    &rest[..b1]
}

/// Draw the bar into `layer` (clip it to the bar rect).
pub fn draw_titlebar<B: FontBackend>(
    window_w: f32,
    view: &TitlebarView<'_>,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    let l = TitlebarLayout::new(window_w);
    layer
        .quads
        .push(solid_quad(0.0, 0.0, window_w, TITLEBAR_H, tokens.bg_chrome));
    layer.quads.push(solid_quad(
        0.0,
        TITLEBAR_H - 1.0,
        window_w,
        1.0,
        tokens.border_subtle,
    ));
    let cy = TITLEBAR_H * 0.5;
    let toggle_color = if view.sidebar_visible {
        tokens.icon_hover
    } else {
        tokens.icon_default
    };
    icons::sidebar_toggle(layer, l.toggle.x + l.toggle.w * 0.5, cy, toggle_color);
    icons::plus(
        layer,
        l.new_workspace.x + l.new_workspace.w * 0.5,
        cy,
        11.0,
        tokens.icon_default,
    );

    let s = l.search;
    if s.w <= 0.0 {
        return;
    }
    let active = view.query.is_some();
    layer.quads.push(rounded_quad(
        s.x,
        s.y,
        s.w,
        s.h,
        if active {
            tokens.bg_input
        } else {
            tokens.bg_surface
        },
        7.0,
        1.0,
        if active {
            tokens.accent_tide
        } else {
            tokens.border_subtle
        },
    ));
    icons::magnifier(layer, s.x + 15.0, cy, 11.0, tokens.icon_default);
    let tx = s.x + 29.0;
    let ty = cy - fonts.geo.cell_h * 0.5;
    let cols = (((s.right() - 10.0 - tx) / fonts.geo.cell_w).floor() as usize).max(1);
    match view.query {
        Some(field) if !field.text().is_empty() => {
            let start = field.view_start(cols);
            if let Some((a, b)) = field.selection() {
                let (a, b) = (a.max(start), b.min(start + cols));
                if b > a {
                    layer.quads.push(solid_quad(
                        tx + (a - start) as f32 * fonts.geo.cell_w,
                        ty,
                        (b - a) as f32 * fonts.geo.cell_w,
                        fonts.geo.cell_h,
                        tokens.bg_selected,
                    ));
                }
            }
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                visible_slice(field.text(), start, cols),
                tx,
                ty,
                cols as f32 * fonts.geo.cell_w + 0.5,
                tokens.text_primary,
                layer,
            );
            if view.caret_visible {
                layer.quads.push(solid_quad(
                    tx + (field.caret() - start) as f32 * fonts.geo.cell_w,
                    ty,
                    1.5,
                    fonts.geo.cell_h,
                    tokens.text_primary,
                ));
            }
        }
        _ => {
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                "Search folders…",
                tx,
                ty,
                cols as f32 * fonts.geo.cell_w + 0.5,
                tokens.text_tertiary,
                layer,
            );
            if active && view.caret_visible {
                layer.quads.push(solid_quad(
                    tx,
                    ty,
                    1.5,
                    fonts.geo.cell_h,
                    tokens.text_primary,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_is_centered_and_clamped() {
        let l = TitlebarLayout::new(1000.0);
        assert_eq!(l.search.w, 400.0);
        assert!((l.search.x + l.search.w * 0.5 - 500.0).abs() < 0.01);
        assert_eq!(TitlebarLayout::new(3000.0).search.w, 560.0);
        assert_eq!(TitlebarLayout::new(500.0).search.w, 240.0);
    }

    #[test]
    fn narrow_window_never_overlaps_buttons_or_overflows() {
        for w in [0.0, 100.0, 200.0, 320.0, 400.0, 500.0] {
            let l = TitlebarLayout::new(w);
            assert!(l.search.w >= 0.0);
            if l.search.w > 0.0 {
                assert!(l.search.x >= l.new_workspace.right());
                assert!(l.search.right() <= w + 0.01);
            }
        }
    }

    #[test]
    fn visible_slice_is_char_safe() {
        assert_eq!(visible_slice("héllo wörld", 1, 4), "éllo");
        assert_eq!(visible_slice("abc", 5, 2), "");
        assert_eq!(visible_slice("abc", 0, 10), "abc");
    }
}
