//! Palette modal scrim.
use crate::draw::solid_quad;
use termin_core::{Rect, Rgba};
use termin_render::Layer;
pub fn draw(rect: Rect, layer: &mut Layer) {
    layer.quads.push(solid_quad(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        Rgba::from_rgba(0, 0, 0, 0x88),
    ));
}
