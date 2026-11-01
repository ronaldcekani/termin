//! Scrollbar widget (`termin-ui::widgets::scrollbar`).
//!
//! Renders overlay scrollbars with optional tick marks for failed blocks and search matches.
//! See `docs/design/components.md §12`.

use termin_core::{Rect, Rgba};
use termin_render::Layer;

use crate::draw::{rounded_quad, solid_quad};
use crate::theme::Tokens;

/// Category of tick mark displayed on the scrollbar track.
///
/// See `docs/design/components.md §12`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ScrollbarTickKind {
    /// Failed command block (rendered in `danger`, 2 pt).
    FailedBlock,
    /// Search match position (rendered in `ember`, 2 pt).
    SearchMatch,
}

/// A tick mark positioned along the scrollbar track.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ScrollbarTick {
    /// Absolute line or row index within the `total` scrollable range.
    pub line: usize,
    /// Category / styling of the tick mark.
    pub kind: ScrollbarTickKind,
}

impl ScrollbarTick {
    /// Create a failed block tick mark at `line`.
    pub fn failed_block(line: usize) -> Self {
        Self {
            line,
            kind: ScrollbarTickKind::FailedBlock,
        }
    }

    /// Create a search match tick mark at `line`.
    pub fn search_match(line: usize) -> Self {
        Self {
            line,
            kind: ScrollbarTickKind::SearchMatch,
        }
    }
}

/// Scrollbar state and geometry.
#[derive(Clone, Debug)]
pub struct Scrollbar {
    pub rect: Rect,
    pub total: usize,
    pub visible: usize,
    pub offset: usize,
    pub ticks: Vec<ScrollbarTick>,
}

impl Scrollbar {
    pub fn new(rect: Rect, total: usize, visible: usize, offset: usize) -> Self {
        Self {
            rect,
            total,
            visible,
            offset,
            ticks: Vec::new(),
        }
    }

    /// Attach tick marks to the scrollbar.
    pub fn with_ticks(mut self, ticks: Vec<ScrollbarTick>) -> Self {
        self.ticks = ticks;
        self
    }

    /// Add a single tick mark.
    pub fn add_tick(&mut self, tick: ScrollbarTick) {
        self.ticks.push(tick);
    }

    /// Render tick marks along the scrollbar track into `layer`.
    ///
    /// Per `components.md §12`:
    /// Failed blocks: `danger` color, 2 pt height.
    /// Search matches: `ember` color, 2 pt height.
    pub fn render_ticks(&self, layer: &mut Layer, tokens: &Tokens) {
        if self.total == 0 || self.rect.h <= 0.0 || self.ticks.is_empty() {
            return;
        }

        const TICK_HEIGHT: f32 = 2.0;
        let track_h = (self.rect.h - TICK_HEIGHT).max(0.0);
        let max_line = (self.total.saturating_sub(1)).max(1) as f32;

        for tick in &self.ticks {
            let line_fraction = (tick.line as f32 / max_line).clamp(0.0, 1.0);
            let tick_y = self.rect.y + line_fraction * track_h;
            let color = match tick.kind {
                ScrollbarTickKind::FailedBlock => tokens.status_danger,
                ScrollbarTickKind::SearchMatch => tokens.accent_ember,
            };

            layer.quads.push(solid_quad(
                self.rect.x,
                tick_y,
                self.rect.w,
                TICK_HEIGHT,
                color,
            ));
        }
    }

    /// Render scrollbar thumb into `layer`.
    pub fn render(&self, layer: &mut Layer, thumb_color: Rgba) {
        if self.total <= self.visible || self.visible == 0 {
            return;
        }

        let ratio = self.visible as f32 / self.total as f32;
        let thumb_h = (self.rect.h * ratio).max(16.0);
        let max_scroll = (self.total - self.visible) as f32;
        let scroll_ratio = (self.offset as f32 / max_scroll).clamp(0.0, 1.0);
        let thumb_y = self.rect.y + scroll_ratio * (self.rect.h - thumb_h);

        let thumb_rect = Rect::new(self.rect.x, thumb_y, self.rect.w, thumb_h);
        layer.quads.push(rounded_quad(
            thumb_rect.x,
            thumb_rect.y,
            thumb_rect.w,
            thumb_rect.h,
            thumb_color,
            thumb_rect.w / 2.0,
            0.0,
            Rgba::TRANSPARENT,
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scrollbar_ticks_rendering() {
        let tokens = Tokens::default();
        let mut layer = Layer::default();
        let scrollbar = Scrollbar::new(Rect::new(100.0, 0.0, 6.0, 200.0), 100, 20, 0).with_ticks(vec![
            ScrollbarTick::failed_block(25),
            ScrollbarTick::search_match(50),
        ]);

        scrollbar.render_ticks(&mut layer, &tokens);
        assert_eq!(layer.quads.len(), 2);

        // Failed block tick: danger color, 2 pt height
        let failed_quad = &layer.quads[0];
        assert_eq!(failed_quad.color, tokens.status_danger.0);
        assert_eq!(failed_quad.rect[3], 2.0);
        assert_eq!(failed_quad.rect[2], 6.0);

        // Search match tick: ember color, 2 pt height
        let search_quad = &layer.quads[1];
        assert_eq!(search_quad.color, tokens.accent_ember.0);
        assert_eq!(search_quad.rect[3], 2.0);
        assert_eq!(search_quad.rect[2], 6.0);
    }
}
