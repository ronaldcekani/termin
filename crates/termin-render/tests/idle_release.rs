#![cfg(target_os = "macos")]

use termin_core::Rgba;
use termin_render::{DrawList, Layer, MetalRenderer, OffscreenTarget, QuadInst, Renderer};

#[test]
fn test_release_idle_resources() {
    let mut renderer = MetalRenderer::new().expect("failed to create MetalRenderer");
    let mut target = OffscreenTarget::new(renderer.device(), 400, 400, 1.0)
        .expect("failed to create OffscreenTarget");

    // Fill staging buffer
    renderer.stage_atlas_data(&vec![0xAA; 100_000]);

    // Build 5,000 quads to trigger buffer growth
    let mut list = DrawList::new();
    let mut layer = Layer::new(None);
    for i in 0..5_000 {
        layer.quads.push(QuadInst {
            rect: [i as f32, 0.0, 10.0, 10.0],
            color: 0xFFFFFFFF,
            border_color: 0,
            radius: 0.0,
            border: 0.0,
            shadow: 0.0,
            _pad: 0.0,
        });
    }
    list.push_layer(layer);

    renderer.set_active_texture(Some(target.texture.clone()));
    renderer.render(&mut target, &list, Rgba::from_rgba(0, 0, 0, 255));
    renderer.wait_idle();

    // Release idle resources
    renderer.release_idle_resources();

    // Staging buffer and instance buffers are released/shrunk
    // Subsequent small render proceeds cleanly with minimal buffers
    let mut small_list = DrawList::new();
    let mut small_layer = Layer::new(None);
    small_layer.quads.push(QuadInst {
        rect: [0.0, 0.0, 50.0, 50.0],
        color: 0xFF0000FF,
        border_color: 0,
        radius: 0.0,
        border: 0.0,
        shadow: 0.0,
        _pad: 0.0,
    });
    small_list.push_layer(small_layer);

    renderer.render(&mut target, &small_list, Rgba::from_rgba(0, 0, 0, 255));
    renderer.wait_idle();
}
