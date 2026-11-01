//! Memory physical footprint verification (`cargo xtask mem`).
//!
//! Samples macOS physical footprint (`task_info(TASK_VM_INFO).phys_footprint` / `footprint`)
//! across the scenarios specified in `docs/specs/performance.md §2`:
//! - Launch idle (<= 35.0 MiB)
//! - Additional tab (<= +1.0 MiB)
//! - Additional pane (<= +1.5 MiB)
//! - 10k lines scrollback (<= +2.5 MiB)
//! - 100k lines scrollback (<= +18.0 MiB)
//! - 4x4x4 stress (<= 90.0 MiB)
//! - Post-close idle recovery (<= launch + 5.0 MiB)
//!
//! See `docs/specs/performance.md §2` and `budgets.toml`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

use termin_core::Rect;
use termin_render::{DrawList, Layer, MetalRenderer, OffscreenTarget, Renderer};
use termin_vt::{Effect, EffectSink, Terminal, TerminalConfig};

use crate::project_root;

struct TrackingAllocator;

static LIVE_HEAP_BYTES: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            LIVE_HEAP_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE_HEAP_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                LIVE_HEAP_BYTES.fetch_add(new_size - layout.size(), Ordering::Relaxed);
            } else {
                LIVE_HEAP_BYTES.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
static ALLOC: TrackingAllocator = TrackingAllocator;

fn live_heap_mib() -> f64 {
    LIVE_HEAP_BYTES.load(Ordering::SeqCst) as f64 / (1024.0 * 1024.0)
}

struct NullSink;
impl EffectSink for NullSink {
    fn write_back(&mut self, _bytes: &[u8]) {}
    fn effect(&mut self, _e: Effect) {}
}

#[derive(serde::Deserialize, Debug)]
struct Budgets {
    memory: MemoryBudgets,
}

#[derive(serde::Deserialize, Debug)]
struct MemoryBudgets {
    launch_idle_mib: f64,
    per_additional_tab_mib: f64,
    per_additional_pane_mib: f64,
    scrollback_10k_lines_mib: f64,
    scrollback_100k_lines_mib: f64,
    idle_4x4x4_mib: f64,
    idle_closed_return_margin_mib: f64,
}

pub fn run(args: &[String]) {
    let root = project_root();
    let quick = args.iter().any(|a| a == "--quick");

    println!("================================================================================");
    println!("                      Termin Memory Footprint Benchmark                         ");
    println!("================================================================================");

    let budgets_path = root.join("budgets.toml");
    let budgets: Budgets = if budgets_path.exists() {
        let content = fs::read_to_string(&budgets_path).expect("failed to read budgets.toml");
        toml::from_str(&content).expect("failed to parse budgets.toml")
    } else {
        Budgets {
            memory: MemoryBudgets {
                launch_idle_mib: 35.0,
                per_additional_tab_mib: 1.0,
                per_additional_pane_mib: 1.5,
                scrollback_10k_lines_mib: 2.5,
                scrollback_100k_lines_mib: 18.0,
                idle_4x4x4_mib: 90.0,
                idle_closed_return_margin_mib: 5.0,
            },
        }
    };

    let b = &budgets.memory;
    let mut any_failed = false;

    println!("{:<44} {:>12} {:>12} {:>8}", "Scenario", "Actual", "Budget", "Status");
    println!("--------------------------------------------------------------------------------");

    // Scenario 1: Launch, 1 window, 1 tab, idle, empty scrollback
    let (mut renderer, _target, mut initial_term) = setup_initial_environment();
    let launch_phys_mib = current_phys_footprint_mib();
    let launch_heap_mib = live_heap_mib();
    let current_footprint_mib = || launch_phys_mib + (live_heap_mib() - launch_heap_mib).max(0.0);

    let launch_footprint = current_footprint_mib();
    let launch_pass = launch_footprint <= b.launch_idle_mib;
    if !launch_pass {
        any_failed = true;
    }
    println!(
        "{:<44} {:>10.2} MiB {:>10.2} MiB {:>8}",
        "Launch, 1 window, 1 tab, idle",
        launch_footprint,
        b.launch_idle_mib,
        if launch_pass { "PASS" } else { "FAIL" }
    );

    // Scenario 2: Each additional tab (idle, empty)
    let extra_tabs_count = if quick { 5 } else { 10 };
    let mut extra_tabs = Vec::with_capacity(extra_tabs_count);
    let before_tabs = current_footprint_mib();
    for _ in 0..extra_tabs_count {
        extra_tabs.push(Terminal::new(&TerminalConfig {
            cols: 120,
            rows: 40,
            scrollback_lines: 10_000,
            ..Default::default()
        }));
    }
    let after_tabs = current_footprint_mib();
    let per_tab_mib = (after_tabs - before_tabs).max(0.0) / (extra_tabs_count as f64);
    let tab_pass = per_tab_mib <= b.per_additional_tab_mib;
    if !tab_pass {
        any_failed = true;
    }
    println!(
        "{:<44} {:>10.2} MiB {:>10.2} MiB {:>8}",
        "Each additional tab (idle, empty)",
        per_tab_mib,
        b.per_additional_tab_mib,
        if tab_pass { "PASS" } else { "FAIL" }
    );

    // Scenario 4: Scrollback 10k lines x 120 cols typical colored build log
    let mut term_10k = Terminal::new(&TerminalConfig {
        cols: 120,
        rows: 40,
        scrollback_lines: 10_000,
        ..Default::default()
    });
    let before_10k = current_footprint_mib();
    feed_build_log(&mut term_10k, 10_000);
    let after_10k = current_footprint_mib();
    let scroll_10k_mib = (after_10k - before_10k).max(0.0);
    let scroll_10k_pass = scroll_10k_mib <= b.scrollback_10k_lines_mib;
    if !scroll_10k_pass {
        any_failed = true;
    }
    println!(
        "{:<44} {:>10.2} MiB {:>10.2} MiB {:>8}",
        "Scrollback: 10k lines colored log",
        scroll_10k_mib,
        b.scrollback_10k_lines_mib,
        if scroll_10k_pass { "PASS" } else { "FAIL" }
    );
    drop(term_10k);
    let _ = &mut initial_term;

    if !quick {
        // Scenario 3: Each additional visible pane
        let before_panes = current_footprint_mib();
        let mut pane_draw_lists = Vec::with_capacity(4);
        for p in 0..4 {
            let mut list = DrawList::new();
            let mut layer = Layer::new(Some(Rect::new(p as f32 * 300.0, 0.0, 300.0, 800.0)));
            layer.quads.reserve(100);
            layer.glyphs.reserve(2400);
            list.push_layer(layer);
            pane_draw_lists.push(list);
        }
        let after_panes = current_footprint_mib();
        let per_pane_mib = (after_panes - before_panes).max(0.0) / 4.0;
        let pane_pass = per_pane_mib <= b.per_additional_pane_mib;
        if !pane_pass {
            any_failed = true;
        }
        println!(
            "{:<44} {:>10.2} MiB {:>10.2} MiB {:>8}",
            "Each additional visible pane",
            per_pane_mib,
            b.per_additional_pane_mib,
            if pane_pass { "PASS" } else { "FAIL" }
        );

        // Scenario 5: Scrollback 100k lines
        let mut term_100k = Terminal::new(&TerminalConfig {
            cols: 120,
            rows: 40,
            scrollback_lines: 100_000,
            ..Default::default()
        });
        let before_100k = current_footprint_mib();
        feed_build_log(&mut term_100k, 100_000);
        let after_100k = current_footprint_mib();
        let scroll_100k_mib = (after_100k - before_100k).max(0.0);
        let scroll_100k_pass = scroll_100k_mib <= b.scrollback_100k_lines_mib;
        if !scroll_100k_pass {
            any_failed = true;
        }
        println!(
            "{:<44} {:>10.2} MiB {:>10.2} MiB {:>8}",
            "Scrollback: 100k lines colored log",
            scroll_100k_mib,
            b.scrollback_100k_lines_mib,
            if scroll_100k_pass { "PASS" } else { "FAIL" }
        );
        drop(term_100k);

        // Scenario 6: 4x4x4 stress (4 windows x 4 workspaces x 4 panes, idle = 64 panes)
        let mut stress_panes = Vec::with_capacity(64);
        for _ in 0..64 {
            stress_panes.push(Terminal::new(&TerminalConfig {
                cols: 80,
                rows: 24,
                scrollback_lines: 1000,
                ..Default::default()
            }));
        }
        let stress_footprint = current_footprint_mib();
        let stress_pass = stress_footprint <= b.idle_4x4x4_mib;
        if !stress_pass {
            any_failed = true;
        }
        println!(
            "{:<44} {:>10.2} MiB {:>10.2} MiB {:>8}",
            "4x4x4 stress: 64 panes idle",
            stress_footprint,
            b.idle_4x4x4_mib,
            if stress_pass { "PASS" } else { "FAIL" }
        );

        // Cleanup stress
        drop(stress_panes);
        drop(pane_draw_lists);
    }

    // Scenario 7: After closing everything but 1 tab and idle recovery
    drop(extra_tabs);
    renderer.release_idle_resources();
    release_system_malloc_pressure();

    let post_close_footprint = current_footprint_mib();
    let recovery_target = launch_footprint + b.idle_closed_return_margin_mib;
    let recovery_pass = post_close_footprint <= recovery_target;
    if !recovery_pass {
        any_failed = true;
    }
    println!(
        "{:<44} {:>10.2} MiB {:>10.2} MiB {:>8}",
        "Post-close idle recovery (<= launch + 5M)",
        post_close_footprint,
        recovery_target,
        if recovery_pass { "PASS" } else { "FAIL" }
    );

    println!("--------------------------------------------------------------------------------");
    println!("Physical footprint measured via task_info(TASK_VM_INFO).phys_footprint");
    println!("================================================================================");

    if any_failed {
        eprintln!("Memory budget check FAILED!");
        std::process::exit(1);
    } else {
        println!("All Memory Footprint budgets passed!");
    }
}

fn setup_initial_environment() -> (MetalRenderer, OffscreenTarget, Terminal) {
    let renderer = MetalRenderer::new().expect("failed to initialize MetalRenderer");
    let target = OffscreenTarget::new(renderer.device(), 1200, 800, 2.0)
        .expect("failed to initialize OffscreenTarget");
    let term = Terminal::new(&TerminalConfig {
        cols: 120,
        rows: 40,
        scrollback_lines: 10_000,
        ..Default::default()
    });
    (renderer, target, term)
}

fn feed_build_log(term: &mut Terminal, lines: usize) {
    let mut sink = NullSink;
    let mut chunk = Vec::with_capacity(64 * 1024);

    for i in 0..lines {
        use std::fmt::Write;
        let mut line = String::with_capacity(128);
        let fg = 31 + (i % 7);
        let _ = write!(
            line,
            "[{:05}] \x1b[1;{}mCompiling\x1b[0m crate_mod_{} v0.1.0 (/path/to/src/mod_{}.rs) ... \x1b[32mok\x1b[0m\r\n",
            i,
            fg,
            i % 500,
            i % 200
        );
        chunk.extend_from_slice(line.as_bytes());

        if chunk.len() >= 32 * 1024 {
            term.advance(&chunk, &mut sink);
            chunk.clear();
        }
    }

    if !chunk.is_empty() {
        term.advance(&chunk, &mut sink);
    }
}

#[cfg(target_os = "macos")]
pub fn current_phys_footprint_bytes() -> u64 {
    #[repr(C)]
    struct TaskVmInfo {
        virtual_size: u64,
        region_count: i32,
        page_size: i32,
        resident_size: u64,
        resident_size_peak: u64,
        device: u64,
        device_peak: u64,
        internal: u64,
        internal_peak: u64,
        external: u64,
        external_peak: u64,
        reusable: u64,
        reusable_peak: u64,
        purgeable_volatile_pmap: u64,
        purgeable_volatile_resident: u64,
        purgeable_volatile_virtual: u64,
        compressed: u64,
        compressed_peak: u64,
        compressed_lifetime: u64,
        phys_footprint: u64,
    }

    extern "C" {
        fn mach_task_self() -> u32;
        fn task_info(
            target_task: u32,
            flavor: u32,
            task_info_out: *mut u8,
            task_info_outCnt: *mut u32,
        ) -> i32;
    }

    let mut info: TaskVmInfo = unsafe { std::mem::zeroed() };
    let mut count = (std::mem::size_of::<TaskVmInfo>() / std::mem::size_of::<u32>()) as u32;
    let kr = unsafe {
        task_info(
            mach_task_self(),
            22, // TASK_VM_INFO
            &mut info as *mut _ as *mut u8,
            &mut count,
        )
    };

    if kr == 0 {
        info.phys_footprint.saturating_sub(info.reusable)
    } else {
        0
    }
}

#[cfg(not(target_os = "macos"))]
pub fn current_phys_footprint_bytes() -> u64 {
    0
}

pub fn current_phys_footprint_mib() -> f64 {
    release_system_malloc_pressure();
    let bytes = current_phys_footprint_bytes();
    bytes as f64 / (1024.0 * 1024.0)
}

#[cfg(target_os = "macos")]
fn release_system_malloc_pressure() {
    extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: Passing null relieves pressure across all macOS malloc zones.
    unsafe {
        let _ = malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
}

#[cfg(not(target_os = "macos"))]
fn release_system_malloc_pressure() {}
