//! Individual tab chip drawing.
use crate::{
    draw::{draw_text_mono, icons, rounded_quad, solid_quad},
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
        // Rounded top corners only: a rounded cap plus a square body that
        // runs flush into the strip's bottom edge.
        layer.quads.push(rounded_quad(
            rect.x,
            rect.y,
            rect.w,
            14.0_f32.min(rect.h),
            bg,
            7.0,
            0.0,
            Rgba::TRANSPARENT,
        ));
        layer
            .quads
            .push(solid_quad(rect.x, rect.y + 7.0, rect.w, rect.h - 7.0, bg));
    }
    if state.active {
        let color = if state.focused_pane {
            tokens.accent_tide
        } else {
            tokens.border_strong
        };
        layer
            .quads
            .push(solid_quad(rect.x, rect.bottom() - 2.0, rect.w, 2.0, color));
    }
    let cy = rect.y + (rect.h - 2.0) * 0.5;
    let icon_color = if state.attention {
        tokens.accent_ember
    } else if state.active {
        tokens.icon_hover
    } else {
        tokens.icon_default
    };
    icons::terminal(layer, rect.x + 16.0, cy, icon_color);
    if state.running && !state.attention {
        // Small live dot at the icon's top-right corner.
        layer.quads.push(rounded_quad(
            rect.x + 20.0,
            cy - 9.0,
            5.0,
            5.0,
            tokens.accent_tide,
            2.5,
            0.0,
            Rgba::TRANSPARENT,
        ));
    }
    let close = Rect::new(
        rect.right() - 23.0,
        rect.y + (rect.h - 18.0) * 0.5,
        18.0,
        18.0,
    );
    let show_close = state.active || state.hovered || state.close_hovered;
    let title_w = if show_close {
        rect.w - 58.0
    } else {
        rect.w - 40.0
    };
    draw_text_mono(
        glyphs,
        fonts.font,
        &fonts.geo,
        title,
        rect.x + 29.0,
        cy - fonts.geo.cell_h * 0.5,
        title_w.max(0.0),
        if state.active {
            tokens.text_primary
        } else if state.hovered {
            tokens.text_secondary
        } else {
            tokens.text_tertiary
        },
        layer,
    );
    if show_close {
        let cc = Rect::new(close.x, cy - 9.0, 18.0, 18.0);
        if state.close_hovered {
            layer.quads.push(rounded_quad(
                cc.x,
                cc.y,
                cc.w,
                cc.h,
                tokens.bg_surface_hover,
                4.0,
                0.0,
                Rgba::TRANSPARENT,
            ));
        }
        icons::cross(
            layer,
            cc.x + 9.0,
            cc.y + 9.0,
            7.0,
            if state.close_hovered {
                tokens.icon_hover
            } else {
                tokens.icon_default
            },
        );
    }
    TabHits { body: rect, close }
}
