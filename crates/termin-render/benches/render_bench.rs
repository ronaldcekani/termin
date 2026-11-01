//! Renderer DrawList & 4K row-build benchmarks (`cargo bench -p termin-render`).
//!
//! Benchmarks full-screen 4K (2 panes × 300×80) and incremental DrawList construction
//! (`docs/specs/performance.md §3, §4`).

use std::hint::black_box;
use std::time::Instant;
use termin_core::Rect;
use termin_render::{DrawList, GlyphInst, Layer, QuadInst};

fn main() {
    let mut best_full_ms = f64::MAX;
    for _ in 0..25 {
        let start = Instant::now();
        let mut list = DrawList::new();
        for pane in 0..2 {
            let x0 = pane as f32 * 3000.0;
            let mut layer = Layer::new(Some(Rect::new(x0, 0.0, 3000.0, 1600.0)));
            layer.quads.push(QuadInst {
                rect: [x0, 0.0, 3000.0, 1600.0],
                color: 0x101216FF,
                border_color: 0,
                radius: 0.0,
                border: 0.0,
                shadow: 0.0,
                _pad: 0.0,
            });
            layer.glyphs.reserve(300 * 80);
            for r in 0..80u16 {
                for c in 0..300u16 {
                    layer.glyphs.push(GlyphInst {
                        pos: [x0 + c as f32 * 10.0, r as f32 * 20.0],
                        size: [10, 20],
                        uv: [0, 0, 10, 20],
                        color: 0xE6EDF3FF,
                    });
                }
            }
            list.push_layer(layer);
        }
        black_box(list);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if ms < best_full_ms {
            best_full_ms = ms;
        }
    }
    println!(
        "bench render/full_4k_2panes_300x80: {:>8.3} ms (budget <= 2.0 ms)",
        best_full_ms
    );
}
