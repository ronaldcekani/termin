//! Per-pane tab strip layout and drawing.
use super::tab::{draw_tab, TabHits, TabState};
use crate::{
    draw::{draw_text_mono, solid_quad},
    terminal_view::row::TermFonts,
    theme::Tokens,
};
use termin_core::{Rect, TabId};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

#[derive(Clone, Debug)]
pub struct StripHits {
    pub tabs: Vec<(TabId, TabHits)>,
    pub new_tab: Rect,
    pub split_right: Rect,
    pub split_down: Rect,
}

/// Geometry shared by drawing and hit testing. Keep the active tab visible
/// when the strip cannot fit every tab.
pub struct StripLayout {
    pub first: usize,
    pub visible: usize,
    pub tab_width: f32,
    pub start_x: f32,
    pub actions_x: f32,
}

impl StripLayout {
    pub fn new(rect: Rect, count: usize, active: usize) -> Self {
        let start_x = rect.x + 6.0;
        let actions_x = rect.right() - 26.0 * 3.0 - 4.0;
        let available = (actions_x - start_x).max(0.0);
        let visible = if count == 0 || available == 0.0 {
            0
        } else {
            (((available + 2.0) / 74.0).floor() as usize)
                .max(1)
                .min(count)
        };
        let tab_width = if visible == 0 {
            0.0
        } else {
            ((available - 2.0 * (visible - 1) as f32) / visible as f32).clamp(0.0, 220.0)
        };
        let active = active.min(count.saturating_sub(1));
        let first = active.saturating_add(1).saturating_sub(visible);
        Self {
            first,
            visible,
            tab_width,
            start_x,
            actions_x,
        }
    }

    pub fn tab_rect(&self, rect: Rect, index: usize) -> Option<Rect> {
        let slot = index.checked_sub(self.first)?;
        if slot >= self.visible {
            return None;
        }
        Some(Rect::new(
            self.start_x + slot as f32 * (self.tab_width + 2.0),
            rect.y + 2.0,
            self.tab_width,
            (rect.h - 3.0).max(0.0),
        ))
    }
}

pub fn draw_tabstrip<B: FontBackend>(
    rect: Rect,
    tabs: &[(TabId, &str, TabState)],
    active: TabId,
    tokens: &Tokens,
    fonts: &TermFonts,
    glyphs: &mut GlyphCache<B>,
    layer: &mut Layer,
) -> StripHits {
    layer
        .quads
        .push(solid_quad(rect.x, rect.y, rect.w, rect.h, tokens.bg_chrome));
    layer.quads.push(solid_quad(
        rect.x,
        rect.bottom() - 1.0,
        rect.w,
        1.0,
        tokens.border_subtle,
    ));
    let action_w = 26.0;
    let active_index = tabs
        .iter()
        .position(|(id, _, _)| *id == active)
        .unwrap_or(0);
    let layout = StripLayout::new(rect, tabs.len(), active_index);
    let mut hits = Vec::with_capacity(layout.visible);
    for (index, &(id, title, mut state)) in tabs.iter().enumerate() {
        let Some(r) = layout.tab_rect(rect, index) else {
            continue;
        };
        state.active = id == active;
        hits.push((id, draw_tab(r, title, state, tokens, fonts, glyphs, layer)));
    }
    let new_tab = Rect::new(layout.actions_x, rect.y + 3.0, 24.0, 24.0);
    let split_right = Rect::new(layout.actions_x + action_w, rect.y + 3.0, 24.0, 24.0);
    let split_down = Rect::new(layout.actions_x + action_w * 2.0, rect.y + 3.0, 24.0, 24.0);
    for (r, label) in [(new_tab, "+"), (split_right, "⫿"), (split_down, "⊟")] {
        draw_text_mono(
            glyphs,
            fonts.font,
            &fonts.geo,
            label,
            r.x + 5.0,
            r.y + 4.0,
            16.0,
            tokens.icon_default,
            layer,
        );
    }
    StripHits {
        tabs: hits,
        new_tab,
        split_right,
        split_down,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crowded_strip_keeps_active_visible_and_clear_of_actions() {
        let rect = Rect::new(0.0, 0.0, 400.0, 30.0);
        let layout = StripLayout::new(rect, 20, 19);
        assert!(layout.tab_rect(rect, 19).is_some());
        assert!(layout.tab_rect(rect, 0).is_none());
        for i in layout.first..layout.first + layout.visible {
            assert!(layout.tab_rect(rect, i).unwrap().right() <= layout.actions_x);
        }
    }
}
