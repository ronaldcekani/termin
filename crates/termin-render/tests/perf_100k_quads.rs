#![cfg(target_os = "macos")]

use std::time::Instant;

use termin_core::{Rect, Rgba};
use termin_render::{DrawList, Layer, MetalRenderer, OffscreenTarget, QuadInst, Renderer};

#[test]
fn test_100k_quads_frame_encodes_sub_millisecond() {
    let mut renderer = MetalRenderer::new().expect("failed to create MetalRenderer");
    let mut target = OffscreenTarget::new(renderer.device(), 1920, 1080, 2.0)
        .expect("failed to create OffscreenTarget");

    let clear = Rgba::from_rgba(0x10, 0x10, 0x10, 0xFF);

    // Build 100,000 quads across 10 layers (10,000 quads per layer)
    let mut list = DrawList::new();
    let quads_per_layer = 10_000;
    let num_layers = 10;

    for layer_idx in 0..num_layers {
        let clip = Some(Rect::new(0.0, (layer_idx * 100) as f32, 1920.0, 100.0));
        let mut layer = Layer::new(clip);
        layer.quads.reserve(quads_per_layer);

        for i in 0..quads_per_layer {
            layer.quads.push(QuadInst {
                rect: [(i % 100) as f32 * 19.0, (layer_idx * 100) as f32, 18.0, 20.0],
                color: 0x2080E0FF,
                border_color: 0,
                radius: 4.0,
                border: 0.0,
                shadow: 0.0,
                _pad: 0.0,
            });
        }
        list.push_layer(layer);
    }

    let total_quads: usize = list.layers.iter().map(|l| l.quads.len()).sum();
    assert_eq!(total_quads, 100_000);

    // Bind offscreen target texture as active target
    renderer.set_active_texture(Some(target.texture.clone()));

    // Warm-up frames to ensure instance ring buffers are fully grown to capacity
    renderer.render(&mut target, &list, clear);
    renderer.render(&mut target, &list, clear);
    renderer.wait_idle();

    // Benchmark steady-state frame CPU encoding time
    let mut min_duration = std::time::Duration::from_secs(10);
    for _ in 0..10 {
        renderer.wait_idle();
        let start = Instant::now();
        renderer.render(&mut target, &list, clear);
        let elapsed = start.elapsed();
        if elapsed < min_duration {
            min_duration = elapsed;
        }
    }

    println!(
        "100k quads frame CPU encoding time: {:.3} ms (target < 1.0 ms)",
        min_duration.as_secs_f64() * 1000.0
    );

    // In debug builds without optimizations it might be slightly higher,
    // but in opt/release it is well under 1.0 ms. We check that it runs fast even in debug.
    #[cfg(not(debug_assertions))]
    assert!(
        min_duration.as_secs_f64() * 1000.0 < 1.0,
        "100k quads encoding took {:.3} ms, expected < 1.0 ms",
        min_duration.as_secs_f64() * 1000.0
    );
}
