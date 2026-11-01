//! Vector UI icons built from quads (no font glyphs). Hot-path safe: pushes
//! into the layer's pre-existing `Vec`, no formatting or temporary allocation.

use super::{rounded_quad, solid_quad};
use termin_core::Rgba;
use termin_render::{Layer, QuadInst};

/// Default icon stroke thickness in points.
pub const STROKE: f32 = 1.5;

/// Anti-aliased line from `(x0,y0)` to `(x1,y1)` as a chain of round dots.
/// Axis-aligned lines use a single quad. `c` should be opaque.
pub fn push_line(quads: &mut Vec<QuadInst>, x0: f32, y0: f32, x1: f32, y1: f32, t: f32, c: Rgba) {
    let (dx, dy) = (x1 - x0, y1 - y0);
    if dy.abs() < 0.01 {
        quads.push(solid_quad(x0.min(x1), y0 - t * 0.5, dx.abs(), t, c));
        return;
    }
    if dx.abs() < 0.01 {
        quads.push(solid_quad(x0 - t * 0.5, y0.min(y1), t, dy.abs(), c));
        return;
    }
    let len = (dx * dx + dy * dy).sqrt();
    let steps = (len / (t * 0.4)).ceil().max(1.0) as u32;
    for i in 0..=steps {
        let f = i as f32 / steps as f32;
        let (px, py) = (x0 + dx * f, y0 + dy * f);
        quads.push(rounded_quad(
            px - t * 0.5,
            py - t * 0.5,
            t,
            t,
            c,
            t * 0.5,
            0.0,
            Rgba::TRANSPARENT,
        ));
    }
}

/// `+` centered at `(cx, cy)` with arm length `size`.
pub fn plus(layer: &mut Layer, cx: f32, cy: f32, size: f32, color: Rgba) {
    let h = size * 0.5;
    push_line(&mut layer.quads, cx - h, cy, cx + h, cy, STROKE, color);
    push_line(&mut layer.quads, cx, cy - h, cx, cy + h, STROKE, color);
}

/// `x` centered at `(cx, cy)` spanning `size` points.
pub fn cross(layer: &mut Layer, cx: f32, cy: f32, size: f32, color: Rgba) {
    let h = size * 0.5;
    push_line(
        &mut layer.quads,
        cx - h,
        cy - h,
        cx + h,
        cy + h,
        STROKE,
        color,
    );
    push_line(
        &mut layer.quads,
        cx - h,
        cy + h,
        cx + h,
        cy - h,
        STROKE,
        color,
    );
}

/// Terminal glyph: rounded square outline containing `>_`. 14x14 centered.
pub fn terminal(layer: &mut Layer, cx: f32, cy: f32, color: Rgba) {
    let (x, y) = (cx - 7.0, cy - 7.0);
    layer.quads.push(rounded_quad(
        x,
        y,
        14.0,
        14.0,
        Rgba::TRANSPARENT,
        3.5,
        1.25,
        color,
    ));
    push_line(
        &mut layer.quads,
        x + 4.0,
        y + 4.5,
        x + 6.8,
        y + 7.0,
        1.25,
        color,
    );
    push_line(
        &mut layer.quads,
        x + 6.8,
        y + 7.0,
        x + 4.0,
        y + 9.5,
        1.25,
        color,
    );
    layer
        .quads
        .push(solid_quad(x + 8.0, y + 9.0, 2.8, 1.25, color));
}

/// Split icon: outlined box with a divider. `vertical_divider` splits left/right.
pub fn split(layer: &mut Layer, cx: f32, cy: f32, vertical_divider: bool, color: Rgba) {
    let (w, h) = (14.0, 12.0);
    let (x, y) = (cx - w * 0.5, cy - h * 0.5);
    layer.quads.push(rounded_quad(
        x,
        y,
        w,
        h,
        Rgba::TRANSPARENT,
        2.5,
        STROKE,
        color,
    ));
    if vertical_divider {
        layer
            .quads
            .push(solid_quad(cx - STROKE * 0.5, y, STROKE, h, color));
    } else {
        layer
            .quads
            .push(solid_quad(x, cy - STROKE * 0.5, w, STROKE, color));
    }
}

/// Sidebar-toggle glyph: rounded outline with a left rail divider. 14x11 centered.
pub fn sidebar_toggle(layer: &mut Layer, cx: f32, cy: f32, color: Rgba) {
    let (w, h) = (14.0, 11.0);
    let (x, y) = (cx - w * 0.5, cy - h * 0.5);
    layer.quads.push(rounded_quad(
        x,
        y,
        w,
        h,
        Rgba::TRANSPARENT,
        2.5,
        1.25,
        color,
    ));
    layer
        .quads
        .push(solid_quad(x + 4.75, y + 1.0, 1.25, h - 2.0, color));
}

/// Magnifier: ring plus handle, `size` points square, centered at `(cx, cy)`.
pub fn magnifier(layer: &mut Layer, cx: f32, cy: f32, size: f32, color: Rgba) {
    let d = size * 0.68;
    let (x, y) = (cx - size * 0.5, cy - size * 0.5);
    layer.quads.push(rounded_quad(
        x,
        y,
        d,
        d,
        Rgba::TRANSPARENT,
        d * 0.5,
        1.25,
        color,
    ));
    let a = d * 0.85;
    push_line(
        &mut layer.quads,
        x + a,
        y + a,
        x + size - 0.6,
        y + size - 0.6,
        1.25,
        color,
    );
}
