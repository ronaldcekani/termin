//! Split divider drawing and hit geometry.
use crate::{draw::solid_quad, theme::Tokens};
use termin_core::{Rect, Rgba};
use termin_render::Layer;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DividerHit {
    pub visual: Rect,
    pub hit: Rect,
}

pub fn draw_divider(
    rect: Rect,
    vertical: bool,
    hovered: bool,
    tokens: &Tokens,
    layer: &mut Layer,
) -> DividerHit {
    let color = if hovered {
        tokens.accent_tide
    } else {
        tokens.border_default
    };
    let visual = if vertical {
        Rect::new(
            rect.x + (rect.w - if hovered { 2.0 } else { 1.0 }) * 0.5,
            rect.y,
            if hovered { 2.0 } else { 1.0 },
            rect.h,
        )
    } else {
        Rect::new(
            rect.x,
            rect.y + (rect.h - if hovered { 2.0 } else { 1.0 }) * 0.5,
            rect.w,
            if hovered { 2.0 } else { 1.0 },
        )
    };
    layer
        .quads
        .push(solid_quad(visual.x, visual.y, visual.w, visual.h, color));
    let hit = if vertical {
        Rect::new(rect.x - 3.0, rect.y, rect.w.max(6.0), rect.h)
    } else {
        Rect::new(rect.x, rect.y - 3.0, rect.w, rect.h.max(6.0))
    };
    let _ = Rgba::TRANSPARENT;
    DividerHit { visual, hit }
}
