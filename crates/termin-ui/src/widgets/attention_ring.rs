//! Animated attention outline for an unfocused pane.
//!
//! The caller owns timing and schedules frames only while the pulse is active.
//! Reduced motion suppresses pulsing, rendering a steady 100% alpha ring.

use crate::draw::rounded_quad;
use crate::theme::{MotionTokens, Tokens};
use termin_core::Rect;
use termin_render::Layer;

/// Draw an attention outline. `elapsed` is time since the attention was raised.
/// The first three cycles pulse (alpha 100% -> 35% -> 100%); afterward or under
/// reduced motion, the outline stays at steady 100% strength.
pub fn draw(rect: Rect, elapsed: f32, reduced_motion: bool, tokens: &Tokens, layer: &mut Layer) {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let motion = MotionTokens::new(reduced_motion);
    let alpha = motion.attention_alpha(elapsed);
    let color = with_alpha(tokens.border_attention, alpha);
    layer.quads.push(rounded_quad(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        tokens.bg_app,
        6.0,
        2.0,
        color,
    ));
}

fn with_alpha(color: termin_core::Rgba, alpha: f32) -> termin_core::Rgba {
    let a = ((color.a() as f32) * alpha).round() as u8;
    termin_core::Rgba::from_rgba(color.r(), color.g(), color.b(), a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attention_ring_drawing() {
        let tokens = Tokens::default();
        let mut layer = Layer::default();
        let rect = Rect::new(0.0, 0.0, 100.0, 100.0);

        // Under reduced motion
        draw(rect, 0.5, true, &tokens, &mut layer);
        assert_eq!(layer.quads.len(), 1);
        let quad_reduced = layer.quads.pop().unwrap();
        assert_eq!(quad_reduced.border_color.a(), tokens.border_attention.a());

        // Under normal motion at mid pulse (alpha is reduced to ~0.35)
        draw(rect, 0.55, false, &tokens, &mut layer);
        assert_eq!(layer.quads.len(), 1);
        let quad_pulsing = layer.quads.pop().unwrap();
        assert!(quad_pulsing.border_color.a() < tokens.border_attention.a());
    }
}
