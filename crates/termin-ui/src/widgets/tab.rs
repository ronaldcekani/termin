//! Individual tab chip drawing.
use crate::{
    draw::{draw_text_mono, rounded_quad, solid_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::{Rect, Rgba};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

#[derive(Clone, Copy, Debug, Default)]
pub struct TabState {
    pub active: bool,
    pub focused_pane: bool,
    pub hovered: bool,
    pub attention: bool,
    pub running: bool,
    pub close_hovered: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TabHits {
    pub body: Rect,
    pub close: Rect,
}

pub fn draw_tab<B: FontBackend>(
    rect: Rect,
    title: &str,
    state: TabState,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) -> TabHits {
    let bg = if state.active {
        tokens.bg_app
    } else if state.hovered {
        tokens.bg_surface
    } else {
        Rgba::TRANSPARENT
    };
    if bg != Rgba::TRANSPARENT {
        layer.quads.push(rounded_quad(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            bg,
            5.0,
            0.0,
            Rgba::TRANSPARENT,
        ));
    }
    if state.active && state.focused_pane {
        layer.quads.push(solid_quad(
            rect.x + 6.0,
            rect.y,
            rect.w - 12.0,
            2.0,
            tokens.accent_tide,
        ));
    }
    let lead = if state.attention {
        "●"
    } else if state.running {
        "◌"
    } else {
        "⌘"
    };
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        lead,
        rect.x + 9.0,
        rect.y + (rect.h - fonts.geo.cell_h) * 0.5,
        16.0,
        if state.attention {
            tokens.accent_ember
        } else {
            tokens.icon_default
        },
        layer,
    );
    let close = Rect::new(
        rect.right() - 23.0,
        rect.y + (rect.h - 18.0) * 0.5,
        18.0,
        18.0,
    );
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        title,
        rect.x + 29.0,
        rect.y + (rect.h - fonts.geo.cell_h) * 0.5,
        (rect.w - 58.0).max(0.0),
        if state.active {
            tokens.text_primary
        } else {
            tokens.text_secondary
        },
        layer,
    );
    if state.active || state.hovered || state.close_hovered {
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            "×",
            close.x + 3.0,
            close.y + 1.0,
            14.0,
            tokens.text_tertiary,
            layer,
        );
    }
    TabHits { body: rect, close }
}
