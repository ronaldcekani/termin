//! Timing harness for block-pane rendering (`cargo test -p termin-ui --release
//! bench_block_pane -- --ignored --nocapture`). Mirrors the per-frame work of
//! `termin-app::frame::render_block_pane`.

use super::*;
use crate::terminal_view::{RowCtx, TermFonts, TermPalette};
use crate::Tokens;
use std::time::Instant;
use termin_core::Rect;
use termin_render::Layer;
use termin_text::{CoreTextBackend, FontBackend, FontSpec, GlyphCache};
use termin_vt::{Effect, EffectSink, Terminal, TerminalConfig};

struct Sink;
impl EffectSink for Sink {
    fn write_back(&mut self, _: &[u8]) {}
    fn effect(&mut self, _: Effect) {}
}

#[derive(Default)]
struct FrameState {
    cache: super::render::BlockRowCache,
}
impl FrameState {
    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        term: &Terminal,
        map: &VisualLineMap,
        rect: Rect,
        scroll_y: f32,
        cursor_line: Option<AbsLine>,
        ctx: &mut RowCtx<'_, CoreTextBackend>,
        layer: &mut Layer,
        cols: u16,
        _rows: u16,
        epoch: u32,
    ) {
        BlockView::render_rows(
            term,
            map,
            rect,
            scroll_y,
            cursor_line,
            super::render::RowCacheKey {
                cols,
                epoch,
                atlas_epoch: ctx.glyphs.epoch,
            },
            ctx,
            &mut self.cache,
            layer,
        );
    }
}

fn populate(term: &mut Terminal, blocks: usize, lines_per_block: usize) {
    let mut s = Vec::new();
    for b in 0..blocks {
        s.clear();
        s.extend_from_slice(b"\x1b]133;A\x07$ \x1b]133;B\x07");
        s.extend_from_slice(format!("\x1b]133;C;cmdline_url=ls%20-la%20dir{b}\x07").as_bytes());
        for l in 0..lines_per_block {
            s.extend_from_slice(
                format!(
                    "\x1b[34mdrwxr-xr-x\x1b[0m  5 user staff  160 Nov  1 12:00 \x1b[1;32mfile_{b}_{l}.rs\x1b[0m\r\n"
                )
                .as_bytes(),
            );
        }
        s.extend_from_slice(b"\x1b]133;D;0\x07");
        term.advance(&s, &mut Sink);
    }
    term.advance(b"\x1b]133;A\x07$ \x1b]133;B\x07", &mut Sink);
}

#[test]
#[ignore]
fn bench_block_pane() {
    let tokens = Tokens::default();
    let palette = TermPalette::from_theme(&termin_config::TerminalTheme::default(), false);
    let mut backend = CoreTextBackend::new();
    let font = backend.resolve(&FontSpec::default());
    let fonts = TermFonts::new(font, &backend.metrics(font), 2.0);
    let mut glyphs = GlyphCache::new(backend);
    for (cols, rows) in [(80u16, 24u16), (160, 50), (240, 70)] {
        let cfg = TerminalConfig {
            cols,
            rows,
            scrollback_lines: 10_000,
            ..TerminalConfig::default()
        };
        let mut term = Terminal::new(&cfg);
        populate(&mut term, 200, 45);
        let cell_h = fonts.geo.cell_h;
        let cell_w = fonts.geo.cell_w;
        let w0 = cols as f32 * cell_w + 16.0;
        let h = rows as f32 * cell_h;
        let mut map = VisualLineMap::default();
        let mut state = FrameState::default();
        let mut acc = [0f64; 3];
        let mut run = |dw: f32| {
            let t0 = Instant::now();
            let rect = Rect::new(0.0, 0.0, w0 + dw, h);
            let cursor = term.cursor();
            let bottom = AbsLine(cursor.pos.line.0 + 1);
            map.rebuild(
                term.blocks(),
                cell_h,
                cell_h + 12.0,
                cell_h + 8.0,
                term.first_hot_line(),
                bottom,
            );
            let max_scroll = (map.height - rect.h).max(0.0);
            let t1 = Instant::now();
            let mut layer = Layer::new(Some(rect));
            BlockView::render_chrome(
                term.blocks(),
                &map,
                rect,
                max_scroll,
                None,
                None,
                &fonts,
                &mut glyphs,
                &tokens,
                &mut layer,
            );
            let t2 = Instant::now();
            let mut ctx = RowCtx {
                glyphs: &mut glyphs,
                fonts: &fonts,
                palette: &palette,
            };
            state.render(
                &term, &map, rect, max_scroll, None, &mut ctx, &mut layer, cols, rows, 0,
            );
            let t3 = Instant::now();
            acc[0] += (t1 - t0).as_secs_f64();
            acc[1] += (t2 - t1).as_secs_f64();
            acc[2] += (t3 - t2).as_secs_f64();
            std::hint::black_box(&layer);
        };
        for i in 0..5 {
            run(i as f32);
        }
        let n = 300;
        let t = Instant::now();
        for i in 0..n {
            run((i % 40) as f32); // live-resize tick: width changes, grid does not
        }
        let per_phase: Vec<String> = acc
            .iter()
            .map(|a| format!("{:.1}", a * 1e6 / (n + 5) as f64))
            .collect();
        println!("  phases us (map, chrome, rows): {per_phase:?}");
        let per = t.elapsed().as_secs_f64() * 1e6 / n as f64;
        println!(
            "block pane {cols}x{rows} (200 blocks, 10k scrollback): {per:.1} us/frame, {} segments",
            map.segments.len()
        );
    }
}
