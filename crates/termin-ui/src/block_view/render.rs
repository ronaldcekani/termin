//! Block chrome drawing helpers.
use super::{SegmentKind, VisualLineMap};
use crate::{
    draw::{draw_text_mono, offset_glyph, offset_quad, solid_quad},
    terminal_view::{build_row, hash_cells, RowCtx, RowInstances, TermFonts},
    Tokens,
};
use rustc_hash::FxHashMap;
use termin_core::{BlockId, Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
use termin_vt::{AbsLine, BlockList, Terminal};

pub struct BlockView;

/// Horizontal inset of block chrome and output rows inside the pane.
pub const ROW_INSET: f32 = 8.0;

/// Split `path` into ("~" prefix present, remainder) relative to `$HOME`.
pub fn home_relative(path: &str) -> (bool, &str) {
    let Some(home) = std::env::var_os("HOME") else {
        return (false, path);
    };
    let Some(home) = home.to_str() else {
        return (false, path);
    };
    match path.strip_prefix(home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with('/')) => {
            (true, rest)
        }
        _ => (false, path),
    }
}

impl BlockView {
    /// Draw headers and command rows. Terminal output rows remain the terminal
    /// renderer's responsibility and use the same absolute line ranges.
    #[allow(clippy::too_many_arguments)]
    pub fn render_chrome<B: FontBackend>(
        blocks: &BlockList,
        map: &VisualLineMap,
        rect: Rect,
        scroll_y: f32,
        selected: Option<BlockId>,
        hovered: Option<BlockId>,
        fonts: &TermFonts,
        glyphs: &mut GlyphCache<B>,
        tokens: &Tokens,
        layer: &mut Layer,
    ) {
        layer.clip = Some(rect);
        let mut previous = None;
        for segment in map.visible(scroll_y - 1.0, scroll_y + rect.h + 1.0) {
            let Some(id) = segment.block else { continue };
            let Some(block) = blocks.get(id) else {
                continue;
            };
            let y = rect.y + segment.y - scroll_y;
            if y + segment.h < rect.y || y > rect.bottom() {
                continue;
            }
            if segment.kind == SegmentKind::Header {
                let is_selected = selected == Some(id);
                let is_hovered = hovered == Some(id);
                if block.exit.is_some_and(|code| code != 0) {
                    layer
                        .quads
                        .push(solid_quad(rect.x, y, rect.w, segment.h, tokens.bg_danger));
                }
                if is_hovered {
                    layer.quads.push(solid_quad(
                        rect.x,
                        y,
                        rect.w,
                        segment.h,
                        Rgba::from_rgba(128, 128, 128, 24),
                    ));
                }
                if is_selected {
                    layer
                        .quads
                        .push(solid_quad(rect.x, y, rect.w, segment.h, tokens.bg_selected));
                }
                if block.exit.is_some_and(|code| code != 0) || is_selected {
                    layer.quads.push(solid_quad(
                        rect.x,
                        y,
                        rect.w.min(2.0),
                        segment.h,
                        if is_selected {
                            tokens.accent_tide
                        } else {
                            tokens.status_danger
                        },
                    ));
                }
                let text_y = y + (segment.h - fonts.geo.cell_h) * 0.5;
                let cwd = block.cwd.as_deref().unwrap_or("~");
                let (tilde, rest) = home_relative(cwd);
                let mut x = rect.x + ROW_INSET;
                if tilde {
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        "~",
                        x,
                        text_y,
                        fonts.geo.cell_w,
                        tokens.text_tertiary,
                        layer,
                    );
                    x += fonts.geo.cell_w;
                }
                draw_text_mono(
                    glyphs,
                    fonts.font,
                    &fonts.geo,
                    rest,
                    x,
                    text_y,
                    rect.w * 0.6,
                    tokens.text_tertiary,
                    layer,
                );
                if let Some(code) = block.exit.filter(|c| *c != 0) {
                    let mut buf = [0u8; 16];
                    let txt = format_exit(code, &mut buf);
                    let cell_w = fonts.geo.cell_w;
                    let code_w = txt.len() as f32 * cell_w;
                    let code_x = rect.right() - ROW_INSET - code_w;
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        "exit",
                        code_x - 5.0 * cell_w,
                        text_y,
                        4.0 * cell_w,
                        tokens.status_danger,
                        layer,
                    );
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        txt,
                        code_x,
                        text_y,
                        code_w,
                        tokens.status_danger,
                        layer,
                    );
                } else if block.started_at.is_some() && block.finished_at.is_none() {
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        "● running",
                        rect.right() - ROW_INSET - 9.0 * fonts.geo.cell_w,
                        text_y,
                        80.0,
                        tokens.accent_tide,
                        layer,
                    );
                }
                if previous.is_some() {
                    layer
                        .quads
                        .push(solid_quad(rect.x, y, rect.w, 1.0, tokens.border_subtle));
                }
            } else if segment.kind == SegmentKind::Command {
                if let Some(cmd) = block.command.as_deref() {
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        cmd,
                        rect.x + ROW_INSET,
                        y + (segment.h - fonts.geo.cell_h) * 0.5,
                        rect.w - 2.0 * ROW_INSET,
                        tokens.syntax_command,
                        layer,
                    );
                }
            }
            previous = Some(id);
        }
    }
}

/// Inputs that, when changed, invalidate every cached block row.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RowCacheKey {
    pub cols: u16,
    /// Font / theme / scale epoch maintained by the app.
    pub epoch: u32,
    /// `GlyphCache::epoch` (atlas coordinates become invalid when it changes).
    pub atlas_epoch: u64,
}

struct CachedRow {
    inst: RowInstances,
    used: u64,
    built: bool,
    /// `Terminal::generation` at which the row was last verified against its cells.
    verified: u64,
}

/// Per-tab cache of block-mode row instances keyed by absolute line. A row is
/// re-shaped only when its cell content hash changed or the [`RowCacheKey`] did,
/// so scrolling, live-resize ticks between reflows and idle redraws are cheap.
#[derive(Default)]
pub struct BlockRowCache {
    rows: FxHashMap<u64, CachedRow>,
    key: Option<RowCacheKey>,
    frame: u64,
    /// Rows rebuilt by the last `render_rows` (diagnostics/tests).
    pub rebuilt: usize,
}

impl BlockRowCache {
    /// Retain at most this many rows beyond the visible set between frames.
    const SLACK: usize = 256;

    pub fn new() -> Self {
        Self::default()
    }

    /// Force a rebuild of every row on next use.
    pub fn invalidate(&mut self) {
        self.key = None;
    }
}

impl std::fmt::Debug for BlockRowCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockRowCache")
            .field("rows", &self.rows.len())
            .finish()
    }
}

impl BlockView {
    /// Draw the terminal rows of every visible `Rows` segment, taking each row's
    /// instances from `cache` (building them only on a miss) and offsetting them
    /// into place. Returns the on-screen origin of `cursor_line`'s row when visible.
    #[allow(clippy::too_many_arguments)]
    pub fn render_rows<B: FontBackend>(
        term: &Terminal,
        map: &VisualLineMap,
        rect: Rect,
        scroll_y: f32,
        cursor_line: Option<AbsLine>,
        key: RowCacheKey,
        ctx: &mut RowCtx<'_, B>,
        cache: &mut BlockRowCache,
        layer: &mut Layer,
    ) -> Option<(f32, f32)> {
        let line_h = map.line_height;
        let mut cursor_at = None;
        cache.rebuilt = 0;
        if cache.key != Some(key) {
            cache.rows.clear();
            cache.key = Some(key);
        }
        cache.frame = cache.frame.wrapping_add(1);
        let frame = cache.frame;
        let mut visible_rows = 0usize;
        let term_gen = term.generation();
        for segment in map.visible(scroll_y - 1.0, scroll_y + rect.h + 1.0) {
            let SegmentKind::Rows { first, end } = segment.kind else {
                continue;
            };
            let seg_y = rect.y + segment.y - scroll_y;
            // Only rows intersecting the viewport.
            let skip = ((rect.y - seg_y) / line_h).floor().max(0.0) as u64;
            let mut line = first.0 + skip;
            while line < end.0 {
                let row_y = seg_y + (line - first.0) as f32 * line_h;
                if row_y > rect.bottom() {
                    break;
                }
                let row_x = rect.x + ROW_INSET;
                if cursor_line == Some(AbsLine(line)) {
                    cursor_at = Some((row_x, row_y));
                }
                if let Some(cells) = term.row_at(AbsLine(line)) {
                    visible_rows += 1;
                    let entry = cache.rows.entry(line).or_insert_with(|| CachedRow {
                        inst: RowInstances::default(),
                        used: 0,
                        built: false,
                        verified: 0,
                    });
                    entry.used = frame;
                    // Cells only change through `Terminal::advance`/`resize`, which bump
                    // the generation; skip even hashing while it is unchanged.
                    if !entry.built || entry.verified != term_gen {
                        if !entry.built || entry.inst.hash != hash_cells(cells) {
                            build_row(cells, term.styles(), term, ctx, &mut entry.inst);
                            entry.built = true;
                            cache.rebuilt += 1;
                        }
                        entry.verified = term_gen;
                    }
                    let inst = &entry.inst;
                    for q in &inst.bg {
                        layer.quads.push(offset_quad(q, row_x, row_y));
                    }
                    for d in &inst.deco {
                        layer.quads.push(offset_quad(d, row_x, row_y));
                    }
                    for g in &inst.glyphs {
                        layer.glyphs.push(offset_glyph(g, row_x, row_y));
                    }
                    for cg in &inst.color_glyphs {
                        layer.color_glyphs.push(offset_glyph(cg, row_x, row_y));
                    }
                }
                line += 1;
            }
        }
        if cache.rows.len() > visible_rows + BlockRowCache::SLACK {
            cache.rows.retain(|_, r| r.used == frame);
        }
        cursor_at
    }
}

fn format_exit(code: i32, buf: &mut [u8; 16]) -> &str {
    let mut i = buf.len();
    let mut n = code.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if code < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    std::str::from_utf8(&buf[i..]).unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal_view::{TermFonts, TermPalette};
    use termin_text::{CoreTextBackend, FontBackend, FontSpec, GlyphCache};
    use termin_vt::{Effect, EffectSink, TerminalConfig};

    struct Sink;
    impl EffectSink for Sink {
        fn write_back(&mut self, _: &[u8]) {}
        fn effect(&mut self, _: Effect) {}
    }

    #[test]
    fn row_cache_rebuilds_only_changed_rows() {
        let mut term = Terminal::new(&TerminalConfig {
            cols: 20,
            rows: 6,
            ..TerminalConfig::default()
        });
        term.advance(
            b"\x1b]133;A\x07\x1b]133;B\x07\x1b]133;C;cmdline_url=ls\x07",
            &mut Sink,
        );
        term.advance(b"one\r\ntwo\r\nthree\r\n", &mut Sink);
        let mut backend = CoreTextBackend::new();
        let font = backend.resolve(&FontSpec::default());
        let fonts = TermFonts::new(font, &backend.metrics(font), 1.0);
        let mut glyphs = GlyphCache::new(backend);
        let palette = TermPalette::from_theme(&termin_config::TerminalTheme::default(), false);
        let cursor = term.cursor();
        let bottom = AbsLine(cursor.pos.line.0 + 1);
        let mut map = VisualLineMap::default();
        map.rebuild(
            term.blocks(),
            16.0,
            20.0,
            20.0,
            term.first_hot_line(),
            bottom,
        );
        let rect = Rect::new(0.0, 0.0, 400.0, 400.0);
        let mut cache = BlockRowCache::new();
        let mut run =
            |term: &Terminal, map: &VisualLineMap, cache: &mut BlockRowCache, k: RowCacheKey| {
                let mut layer = Layer::new(Some(rect));
                let mut ctx = RowCtx {
                    glyphs: &mut glyphs,
                    fonts: &fonts,
                    palette: &palette,
                };
                BlockView::render_rows(term, map, rect, 0.0, None, k, &mut ctx, cache, &mut layer);
                layer.glyphs.len()
            };
        let k = RowCacheKey {
            cols: 20,
            epoch: 0,
            atlas_epoch: 0,
        };
        let n1 = run(&term, &map, &mut cache, k);
        let first = cache.rebuilt;
        assert!(first >= 3 && n1 > 0);
        let n2 = run(&term, &map, &mut cache, k);
        assert_eq!(cache.rebuilt, 0);
        assert_eq!(n1, n2);
        // Key change (e.g. scale/atlas epoch) rebuilds everything.
        run(&term, &map, &mut cache, RowCacheKey { epoch: 1, ..k });
        assert_eq!(cache.rebuilt, first);
        // Appending a row bumps the generation; unchanged rows hash-hit.
        term.advance(b"four\r\n", &mut Sink);
        let bottom = AbsLine(term.cursor().pos.line.0 + 1);
        map.rebuild(
            term.blocks(),
            16.0,
            20.0,
            20.0,
            term.first_hot_line(),
            bottom,
        );
        run(&term, &map, &mut cache, RowCacheKey { epoch: 1, ..k });
        assert!(cache.rebuilt >= 1 && cache.rebuilt < first);
    }
}
