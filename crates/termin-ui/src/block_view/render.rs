//! Block chrome drawing helpers.
use super::{SegmentKind, VisualLineMap};
use crate::{
    draw::{draw_text_mono, solid_quad},
    terminal_view::TermFonts,
    Tokens,
};
use termin_core::{BlockId, Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};
use termin_vt::BlockList;

pub struct BlockView;

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
                let cwd = block.cwd.as_deref().unwrap_or("~");
                draw_text_mono(
                    glyphs,
                    fonts.font,
                    &fonts.geo,
                    cwd,
                    rect.x + 8.0,
                    y + 2.0,
                    rect.w * 0.45,
                    tokens.text_tertiary,
                    layer,
                );
                if let Some(code) = block.exit.filter(|c| *c != 0) {
                    let mut buf = [0u8; 16];
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        "exit",
                        rect.right() - 64.0,
                        y + 2.0,
                        28.0,
                        tokens.status_danger,
                        layer,
                    );
                    let txt = format_exit(code, &mut buf);
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        txt,
                        rect.right() - 32.0,
                        y + 2.0,
                        28.0,
                        tokens.status_danger,
                        layer,
                    );
                } else if block.started_at.is_some() && block.finished_at.is_none() {
                    draw_text_mono(
                        glyphs,
                        fonts.font,
                        &fonts.geo,
                        "● running",
                        rect.right() - 88.0,
                        y + 2.0,
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
                        rect.x + 8.0,
                        y + 2.0,
                        rect.w - 16.0,
                        tokens.syntax_command,
                        layer,
                    );
                }
            }
            previous = Some(id);
        }
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
