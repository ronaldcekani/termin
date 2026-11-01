//! Block chrome drawing helpers.
use super::{SegmentKind, VisualLineMap};
use crate::{
    draw::{draw_text_mono, offset_glyph, offset_quad, solid_quad},
    terminal_view::{build_row, RowCtx, RowInstances, TermFonts},
    Tokens,
};
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
        for segment in &map.segments {
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

impl BlockView {
    /// Draw the terminal rows of every visible `Rows` segment, building each row
    /// into `scratch` (reused across rows and frames) and offsetting it into place.
    /// Returns the on-screen origin of `cursor_line`'s row when it is visible.
    #[allow(clippy::too_many_arguments)]
    pub fn render_rows<B: FontBackend>(
        term: &Terminal,
        map: &VisualLineMap,
        rect: Rect,
        scroll_y: f32,
        cursor_line: Option<AbsLine>,
        ctx: &mut RowCtx<'_, B>,
        scratch: &mut RowInstances,
        layer: &mut Layer,
    ) -> Option<(f32, f32)> {
        let line_h = map.line_height;
        let mut cursor_at = None;
        for segment in &map.segments {
            let SegmentKind::Rows { first, end } = segment.kind else {
                continue;
            };
            let seg_y = rect.y + segment.y - scroll_y;
            if seg_y + segment.h < rect.y || seg_y > rect.bottom() {
                continue;
            }
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
                    build_row(cells, term.styles(), term, ctx, scratch);
                    for q in &scratch.bg {
                        layer.quads.push(offset_quad(q, row_x, row_y));
                    }
                    for d in &scratch.deco {
                        layer.quads.push(offset_quad(d, row_x, row_y));
                    }
                    for g in &scratch.glyphs {
                        layer.glyphs.push(offset_glyph(g, row_x, row_y));
                    }
                    for cg in &scratch.color_glyphs {
                        layer.color_glyphs.push(offset_glyph(cg, row_x, row_y));
                    }
                }
                line += 1;
            }
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
