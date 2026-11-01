//! Workspace sidebar rendering primitives (app-model-and-ui.md §3).

use crate::{
    draw::{draw_text_mono, rounded_quad, solid_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::{Rect, Rgba, WorkspaceId};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

/// Offset of the first workspace card below the sidebar's top edge.
pub const CARDS_TOP: f32 = 8.0;

/// Data supplied by the app for one workspace card.
pub struct SidebarWorkspace<'a> {
    pub id: WorkspaceId,
    pub name: &'a str,
    pub status: Option<&'a str>,
    pub git_branch: Option<&'a str>,
    pub git_dirty: bool,
    pub paths: &'a [String],
    pub ports: &'a [u16],
    pub unread: usize,
    pub active: bool,
}

/// Render a window's workspace list. Returns workspace card rectangles in sidebar order.
pub fn draw_sidebar<B: FontBackend>(
    sidebar: Rect,
    workspaces: &[SidebarWorkspace<'_>],
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) -> Vec<(WorkspaceId, Rect)> {
    if sidebar.w <= 0.0 {
        return Vec::new();
    }
    layer.quads.push(solid_quad(
        sidebar.x,
        sidebar.y,
        sidebar.w,
        sidebar.h,
        tokens.bg_sidebar,
    ));
    layer.quads.push(solid_quad(
        sidebar.right() - 1.0,
        sidebar.y,
        1.0,
        sidebar.h,
        tokens.border_subtle,
    ));
    let mut hits = Vec::with_capacity(workspaces.len());
    let mut y = sidebar.y + CARDS_TOP;
    for ws in workspaces {
        let id = ws.id;
        let focused = ws.active;
        let unread = ws.unread;
        let status = ws.status;
        let line_count = 1
            + usize::from(status.is_some())
            + usize::from(ws.git_branch.is_some())
            + ws.paths.iter().take(3).count()
            + usize::from(!ws.ports.is_empty());
        let h = (18.0 + line_count as f32 * 18.0).max(46.0);
        let rect = Rect::new(sidebar.x + 7.0, y, sidebar.w - 14.0, h);
        if focused {
            layer.quads.push(rounded_quad(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                tokens.bg_selected,
                7.0,
                0.0,
                Rgba::TRANSPARENT,
            ));
            layer.quads.push(rounded_quad(
                rect.x + 4.0,
                rect.y + 9.0,
                3.0,
                rect.h - 18.0,
                tokens.accent_tide,
                1.5,
                0.0,
                Rgba::TRANSPARENT,
            ));
        }
        if unread > 0 {
            layer.quads.push(rounded_quad(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                tokens.bg_attention,
                7.0,
                1.0,
                tokens.border_attention,
            ));
        }
        let name = if ws.name.is_empty() {
            ws.paths
                .first()
                .map(|p| p.rsplit('/').next().unwrap_or(p))
                .unwrap_or("Workspace")
        } else {
            ws.name
        };
        let mut ty = rect.y + 9.0;
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            name,
            rect.x + 17.0,
            ty,
            rect.w - 56.0,
            tokens.text_primary,
            layer,
        );
        if unread > 0 {
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                &unread.to_string(),
                rect.right() - 28.0,
                ty,
                20.0,
                tokens.accent_ember,
                layer,
            );
        }
        ty += 18.0;
        if let Some(s) = status {
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                s,
                rect.x + 17.0,
                ty,
                rect.w - 28.0,
                if unread > 0 {
                    tokens.accent_ember
                } else {
                    tokens.text_secondary
                },
                layer,
            );
            ty += 18.0;
        }
        if let Some(branch) = ws.git_branch {
            let mut label = String::from("⎇ ");
            label.push_str(branch);
            if ws.git_dirty {
                label.push('*');
            }
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                &label,
                rect.x + 17.0,
                ty,
                rect.w - 28.0,
                tokens.text_secondary,
                layer,
            );
            ty += 18.0;
        }
        for path in ws.paths.iter().take(3) {
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                path,
                rect.x + 17.0,
                ty,
                rect.w - 28.0,
                tokens.text_tertiary,
                layer,
            );
            ty += 18.0;
        }
        if !ws.ports.is_empty() {
            let mut p = String::new();
            for port in ws.ports.iter().take(4) {
                if !p.is_empty() {
                    p.push(' ');
                }
                use std::fmt::Write;
                let _ = write!(p, ":{port}");
            }
            if ws.ports.len() > 4 {
                use std::fmt::Write;
                let _ = write!(p, " +{}", ws.ports.len() - 4);
            }
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                &p,
                rect.x + 17.0,
                ty,
                rect.w - 28.0,
                tokens.text_secondary,
                layer,
            );
        }
        hits.push((id, rect));
        y += h + 6.0;
    }
    hits
}
