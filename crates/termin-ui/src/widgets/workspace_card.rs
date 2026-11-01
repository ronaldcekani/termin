//! Workspace card component; sidebar collection and hit routing are app-owned.
use crate::{
    draw::{draw_text_mono, rounded_quad, solid_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::{Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

#[derive(Clone, Copy, Debug, Default)]
pub struct WorkspaceCardState {
    pub active: bool,
    pub hovered: bool,
    pub dragging: bool,
    pub attention: bool,
}

#[allow(clippy::too_many_arguments)]
pub fn draw_workspace_card<B: FontBackend>(
    rect: Rect,
    title: &str,
    status: Option<&str>,
    git: Option<&str>,
    paths: &[&str],
    ports: &[u16],
    badge: usize,
    state: WorkspaceCardState,
    tokens: &Tokens,
    fonts: &TermFonts,
    cache: &mut GlyphCache<B>,
    layer: &mut Layer,
) {
    if state.active || state.hovered {
        layer.quads.push(rounded_quad(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            if state.active {
                tokens.bg_selected
            } else {
                tokens.bg_surface_hover
            },
            6.0,
            0.0,
            Rgba::TRANSPARENT,
        ));
    }
    if state.active {
        layer.quads.push(solid_quad(
            rect.x + 4.0,
            rect.y + 8.0,
            2.0,
            rect.h - 16.0,
            tokens.accent_tide,
        ));
    }
    if state.attention {
        layer.quads.push(rounded_quad(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            tokens.bg_attention,
            6.0,
            1.0,
            tokens.border_attention,
        ));
    }
    let mut y = rect.y + 8.0;
    draw_text_mono(
        cache,
        fonts.font,
        &fonts.geo,
        title,
        rect.x + 12.0,
        y,
        rect.w - 44.0,
        tokens.text_primary,
        layer,
    );
    if badge > 0 {
        draw_text_mono(
            cache,
            fonts.font,
            &fonts.geo,
            &badge.to_string(),
            rect.right() - 28.0,
            y,
            20.0,
            tokens.accent_ember,
            layer,
        );
    }
    y += 18.0;
    for (line, color) in status
        .into_iter()
        .map(|x| (x, tokens.text_secondary))
        .chain(git.into_iter().map(|x| (x, tokens.text_secondary)))
        .chain(paths.iter().take(3).map(|x| (*x, tokens.text_tertiary)))
    {
        draw_text_mono(
            cache,
            fonts.font,
            &fonts.geo,
            line,
            rect.x + 12.0,
            y,
            rect.w - 20.0,
            color,
            layer,
        );
        y += 18.0;
    }
    if !ports.is_empty() {
        let mut s = String::new();
        for p in ports.iter().take(4) {
            use std::fmt::Write;
            let _ = write!(s, " :{p}");
        }
        if ports.len() > 4 {
            use std::fmt::Write;
            let _ = write!(s, " +{}", ports.len() - 4);
        }
        draw_text_mono(
            cache,
            fonts.font,
            &fonts.geo,
            &s,
            rect.x + 12.0,
            y,
            rect.w - 20.0,
            tokens.text_secondary,
            layer,
        );
    }
    let _ = state.dragging;
}
