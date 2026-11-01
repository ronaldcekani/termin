//! Compact context chips above the input editor (`components.md §5, §15`).
use crate::{
    draw::{draw_text_mono, rounded_quad},
    terminal_view::TermFonts,
    Tokens,
};
use termin_core::{Point, Rect};
use termin_render::Layer;
use termin_text::{FontBackend, GlyphCache};

/// Semantic kind of context chip rendered above the input editor.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChipKind {
    /// Current working directory chip (clicking opens recent directories `cd` menu).
    Cwd,
    /// Git branch chip.
    GitBranch,
    /// Last non-zero exit status chip.
    ExitStatus,
}

/// Hit region returned when rendering context chips.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ChipHit {
    pub kind: ChipKind,
    pub rect: Rect,
}

pub struct ContextChips;

impl ContextChips {
    pub fn render<B: FontBackend>(
        items: &[&str],
        rect: Rect,
        fonts: &TermFonts,
        glyphs: &mut GlyphCache<B>,
        tokens: &Tokens,
        layer: &mut Layer,
    ) -> Vec<ChipHit> {
        let mut hits = Vec::with_capacity(items.len());
        let mut x = rect.x;
        for (idx, item) in items.iter().enumerate() {
            let width = ((item.chars().count() as f32 * fonts.geo.cell_w) + 16.0)
                .clamp(32.0, rect.right() - x);
            if width <= 0.0 {
                break;
            }
            let chip_rect = Rect::new(x, rect.y, width, rect.h);
            layer.quads.push(rounded_quad(
                chip_rect.x,
                chip_rect.y,
                chip_rect.w,
                chip_rect.h,
                tokens.bg_surface,
                4.0,
                0.0,
                tokens.border_subtle,
            ));
            draw_text_mono(
                glyphs,
                fonts.font,
                &fonts.geo,
                item,
                x + 8.0,
                rect.y + (rect.h - fonts.geo.cell_h) * 0.5,
                width - 16.0,
                tokens.text_secondary,
                layer,
            );
            let kind = match idx {
                0 => ChipKind::Cwd,
                1 => ChipKind::GitBranch,
                _ => ChipKind::ExitStatus,
            };
            hits.push(ChipHit {
                kind,
                rect: chip_rect,
            });
            x += width + 6.0;
            if x >= rect.right() {
                break;
            }
        }
        hits
    }

    /// Hit-test a click position against rendered chip hit regions.
    pub fn hit_test(hits: &[ChipHit], pt: Point) -> Option<ChipKind> {
        hits.iter()
            .find(|h| h.rect.contains(pt))
            .map(|h| h.kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cwd_chip_hit_test() {
        let hits = vec![
            ChipHit {
                kind: ChipKind::Cwd,
                rect: Rect::new(10.0, 5.0, 90.0, 20.0),
            },
            ChipHit {
                kind: ChipKind::GitBranch,
                rect: Rect::new(106.0, 5.0, 60.0, 20.0),
            },
        ];
        assert_eq!(
            ContextChips::hit_test(&hits, Point::new(40.0, 12.0)),
            Some(ChipKind::Cwd)
        );
        assert_eq!(
            ContextChips::hit_test(&hits, Point::new(120.0, 12.0)),
            Some(ChipKind::GitBranch)
        );
        assert_eq!(ContextChips::hit_test(&hits, Point::new(200.0, 12.0)), None);
    }
}
