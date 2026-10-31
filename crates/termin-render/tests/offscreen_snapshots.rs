#![cfg(target_os = "macos")]

use termin_core::{Rect, Rgba};
use termin_render::{
    read_texture_rgba, DrawList, GlyphInst, Layer, MetalRenderer, OffscreenTarget, QuadInst,
    Renderer,
};
use termin_text::{AtlasKind, AtlasUpload};

#[test]
fn test_offscreen_rounded_rect_border_shadow() {
    let mut renderer = MetalRenderer::new().expect("failed to create MetalRenderer");
    let target = OffscreenTarget::new(renderer.device(), 200, 200, 1.0)
        .expect("failed to create OffscreenTarget");

    // Clear background: dark navy #101820
    let clear = Rgba::from_rgba(0x10, 0x18, 0x20, 0xFF);

    // Quad: rect [40, 40, 120, 80], corner radius 12.0, border 3.0, shadow 10.0
    // Fill color: green #20A040
    // Border color: white #FFFFFF
    let quad = QuadInst {
        rect: [40.0, 40.0, 120.0, 80.0],
        color: 0x20A040FF,
        border_color: 0xFFFFFFFF,
        radius: 12.0,
        border: 3.0,
        shadow: 10.0,
        _pad: 0.0,
    };

    let mut list = DrawList::new();
    let mut layer = Layer::new(None);
    layer.quads.push(quad);
    list.push_layer(layer);

    renderer
        .render_offscreen(&target, &list, clear)
        .expect("render_offscreen failed");

    let pixels = read_texture_rgba(&target.texture, 200, 200);

    let get_pixel = |x: usize, y: usize| -> (u8, u8, u8, u8) {
        let idx = (y * 200 + x) * 4;
        (pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3])
    };

    // 1. Center of the rect (x=100, y=80) should match fill color #20A040
    let center = get_pixel(100, 80);
    assert!((center.0 as i32 - 0x20).abs() <= 5, "Red channel at center: {}", center.0);
    assert!((center.1 as i32 - 0xA0).abs() <= 5, "Green channel at center: {}", center.1);
    assert!((center.2 as i32 - 0x40).abs() <= 5, "Blue channel at center: {}", center.2);

    // 2. On the straight border (e.g. x=41, y=80) should be close to white border color #FFFFFF
    let border_px = get_pixel(41, 80);
    assert!(border_px.0 > 200, "Border red: {}", border_px.0);
    assert!(border_px.1 > 200, "Border green: {}", border_px.1);
    assert!(border_px.2 > 200, "Border blue: {}", border_px.2);

    // 3. Far outside (x=10, y=10) should be untouched clear background #101820
    let far_outside = get_pixel(10, 10);
    assert!((far_outside.0 as i32 - 0x10).abs() <= 3, "Far outside red: {}", far_outside.0);
    assert!((far_outside.1 as i32 - 0x18).abs() <= 3, "Far outside green: {}", far_outside.1);
    assert!((far_outside.2 as i32 - 0x20).abs() <= 3, "Far outside blue: {}", far_outside.2);

    // 4. In the soft shadow halo outside the edge (x=33, y=80):
    // Shadow is dark, so it darkens the background below the clear color
    let shadow_px = get_pixel(33, 80);
    assert!(
        shadow_px.0 <= far_outside.0 && shadow_px.1 <= far_outside.1 && shadow_px.2 <= far_outside.2,
        "Shadow pixel {:?} should be darker than or equal to clear background {:?}",
        shadow_px,
        far_outside
    );
}

#[test]
fn test_offscreen_synthetic_atlas_glyphs() {
    let mut renderer = MetalRenderer::new().expect("failed to create MetalRenderer");
    let target = OffscreenTarget::new(renderer.device(), 100, 100, 1.0)
        .expect("failed to create OffscreenTarget");

    let clear = Rgba::from_rgba(0x00, 0x00, 0x00, 0xFF);

    // Synthetic 8×8 solid white mask in staging buffer
    let glyph_bitmap = vec![0xFFu8; 8 * 8];
    let upload = AtlasUpload {
        atlas: AtlasKind::Mask,
        rect: Rect::new(0.0, 0.0, 8.0, 8.0),
        bytes_offset: 0,
        bytes_len: 64,
    };

    renderer.stage_atlas_data(&glyph_bitmap);
    renderer.upload_glyphs(&[upload]);

    // Draw glyph with bright red text color #FF2020FF at (20, 20)
    let glyph = GlyphInst {
        pos: [20.0, 20.0],
        size: [8, 8],
        uv: [0, 0, 8, 8],
        color: 0xFF2020FF,
    };

    let mut list = DrawList::new();
    let mut layer = Layer::new(None);
    layer.glyphs.push(glyph);
    list.push_layer(layer);

    renderer
        .render_offscreen(&target, &list, clear)
        .expect("render_offscreen failed");

    let pixels = read_texture_rgba(&target.texture, 100, 100);

    let get_pixel = |x: usize, y: usize| -> (u8, u8, u8, u8) {
        let idx = (y * 100 + x) * 4;
        (pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3])
    };

    // Pixel inside the glyph (x=24, y=24) should be bright red
    let glyph_px = get_pixel(24, 24);
    assert!(glyph_px.0 > 240, "Glyph red: {}", glyph_px.0);
    assert!(glyph_px.1 < 60, "Glyph green: {}", glyph_px.1);
    assert!(glyph_px.2 < 60, "Glyph blue: {}", glyph_px.2);

    // Pixel outside glyph (x=5, y=5) should remain black
    let outside = get_pixel(5, 5);
    assert_eq!(outside, (0, 0, 0, 255));
}

#[test]
fn test_offscreen_layered_scissor_clipping() {
    let mut renderer = MetalRenderer::new().expect("failed to create MetalRenderer");
    let target = OffscreenTarget::new(renderer.device(), 100, 100, 1.0)
        .expect("failed to create OffscreenTarget");

    let clear = Rgba::from_rgba(0x00, 0x00, 0x00, 0xFF);

    let mut list = DrawList::new();

    // Layer with scissor clip rect [30, 30, 40, 40]
    let clip = Rect::new(30.0, 30.0, 40.0, 40.0);
    let mut layer = Layer::new(Some(clip));

    // Fullscreen quad spanning [0, 0, 100, 100] with bright yellow fill #FFFF00
    layer.quads.push(QuadInst {
        rect: [0.0, 0.0, 100.0, 100.0],
        color: 0xFFFF00FF,
        border_color: 0,
        radius: 0.0,
        border: 0.0,
        shadow: 0.0,
        _pad: 0.0,
    });
    list.push_layer(layer);

    renderer
        .render_offscreen(&target, &list, clear)
        .expect("render_offscreen failed");

    let pixels = read_texture_rgba(&target.texture, 100, 100);

    let get_pixel = |x: usize, y: usize| -> (u8, u8, u8, u8) {
        let idx = (y * 100 + x) * 4;
        (pixels[idx], pixels[idx + 1], pixels[idx + 2], pixels[idx + 3])
    };

    // Pixel inside scissor region (x=50, y=50) must be yellow
    let inside = get_pixel(50, 50);
    assert!(inside.0 > 240, "Inside red: {}", inside.0);
    assert!(inside.1 > 240, "Inside green: {}", inside.1);
    assert!(inside.2 < 30, "Inside blue: {}", inside.2);

    // Pixel outside scissor region (x=10, y=10) must remain black clear color
    let outside = get_pixel(10, 10);
    assert_eq!(outside, (0, 0, 0, 255), "Pixel outside scissor was not clipped");

    // Pixel outside to the bottom-right (x=80, y=80) must remain black
    let outside_br = get_pixel(80, 80);
    assert_eq!(outside_br, (0, 0, 0, 255), "Pixel outside scissor was not clipped");
}
