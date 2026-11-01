#![cfg(target_os = "macos")]

//! Alloc-count verification test.
//!
//! Verifies that steady-state output rendering performs zero heap allocations per frame.
//! See `docs/specs/performance.md §2` and `§5`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use termin_core::{Rect, Rgba};
use termin_render::{
    DrawList, GlyphInst, Layer, MetalRenderer, OffscreenTarget, QuadInst, Renderer,
};

struct CountingAlloc;

static TRACK_ALLOCS: AtomicBool = AtomicBool::new(false);
static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACK_ALLOCS.load(Ordering::SeqCst) {
            ALLOC_COUNT.fetch_add(1, Ordering::SeqCst);
        }
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static ALLOCATOR: CountingAlloc = CountingAlloc;

#[test]
fn test_zero_steady_state_heap_allocations_per_frame() {
    let mut renderer = MetalRenderer::new().expect("failed to create MetalRenderer");
    let mut target = OffscreenTarget::new(renderer.device(), 1200, 800, 2.0)
        .expect("failed to create OffscreenTarget");
    renderer.set_active_texture(Some(target.texture.clone()));

    let clear = Rgba::from_rgba(0x18, 0x18, 0x18, 0xFF);

    // Build a realistic full terminal screen DrawList:
    // 40 rows x 100 columns = 4,000 glyphs, plus background quads and cursor
    let mut list = DrawList::new();
    let mut bg_layer = Layer::new(None);
    bg_layer.quads.push(QuadInst {
        rect: [0.0, 0.0, 1200.0, 800.0],
        color: 0x181818FF,
        border_color: 0,
        radius: 0.0,
        border: 0.0,
        shadow: 0.0,
        _pad: 0.0,
    });
    // Cursor quad
    bg_layer.quads.push(QuadInst {
        rect: [80.0, 40.0, 10.0, 20.0],
        color: 0x50A0FFFF,
        border_color: 0,
        radius: 1.0,
        border: 0.0,
        shadow: 0.0,
        _pad: 0.0,
    });
    list.push_layer(bg_layer);

    let mut text_layer = Layer::new(Some(Rect::new(0.0, 0.0, 1200.0, 800.0)));
    for r in 0..40 {
        for c in 0..100 {
            text_layer.glyphs.push(GlyphInst {
                pos: [c as f32 * 10.0, r as f32 * 20.0],
                size: [10, 20],
                uv: [0, 0, 10, 20],
                color: 0xE0E0E0FF,
            });
        }
    }
    list.push_layer(text_layer);

    // Warm-up frames (allocate Metal instance ring buffers, compile pipelines, etc.)
    for _ in 0..4 {
        renderer.render(&mut target, &list, clear);
        renderer.wait_idle();
    }

    // Begin tracking allocations in steady-state rendering
    ALLOC_COUNT.store(0, Ordering::SeqCst);
    TRACK_ALLOCS.store(true, Ordering::SeqCst);

    for frame in 1..=10 {
        let before = ALLOC_COUNT.load(Ordering::SeqCst);
        renderer.render(&mut target, &list, clear);
        renderer.wait_idle();
        let frame_allocs = ALLOC_COUNT.load(Ordering::SeqCst) - before;

        #[cfg(not(debug_assertions))]
        assert_eq!(
            frame_allocs, 0,
            "Frame {} performed {} heap allocations! Zero steady-state heap allocations budget violated.",
            frame, frame_allocs
        );

        #[cfg(debug_assertions)]
        assert!(
            frame_allocs <= 16,
            "Frame {} performed {} heap allocations in debug (exceeded debug allowance)",
            frame, frame_allocs
        );
    }

    TRACK_ALLOCS.store(false, Ordering::SeqCst);

    #[cfg(not(debug_assertions))]
    println!("Alloc-count gate passed: strictly 0 heap allocations across 10 steady-state frames.");
    #[cfg(debug_assertions)]
    println!("Alloc-count gate passed: debug assertions active (run with --release for strict 0 check).");
}
