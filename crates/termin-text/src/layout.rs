//! UI proportional text layout and LRU measurement cache.
//!
//! Provides proportional UI font layout with max width constraint and
//! ellipsis truncation (end ellipsis `…` and middle ellipsis for paths).
//! Cached with an LRU capacity of 512 entries.
//!
//! See `docs/specs/text-and-rendering.md §4`.

use rustc_hash::FxHashMap;
use std::collections::VecDeque;
use std::hash::{Hash, Hasher};

use crate::{FontBackend, FontId, GlyphRef};

/// Truncation mode for UI text exceeding maximum width.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum TruncateMode {
    #[default]
    None,
    /// Truncate with ellipsis at the end (`foo…`).
    End,
    /// Truncate with ellipsis in the middle for file paths (`~/Proj…/main.rs`).
    Middle,
}

/// A positioned glyph within a laid out text run.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PositionedGlyph {
    pub glyph: GlyphRef,
    pub x: f32,
    pub y: f32,
    pub x_advance: f32,
}

/// A complete laid out line of UI text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextLayout {
    pub width: f32,
    pub height: f32,
    pub glyphs: Vec<PositionedGlyph>,
    pub is_truncated: bool,
}

/// LRU cache for UI text layouts (capacity 512).
pub struct TextLayoutCache {
    cache: FxHashMap<u64, TextLayout>,
    lru: VecDeque<u64>,
    capacity: usize,
}

impl Default for TextLayoutCache {
    fn default() -> Self {
        Self::new(512)
    }
}

impl TextLayoutCache {
    /// Create a new layout cache with specified capacity (default 512).
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: FxHashMap::default(),
            lru: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Clear all cached layouts.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.lru.clear();
    }

    /// Get or compute a text layout with optional max width and truncation.
    pub fn layout<B: FontBackend>(
        &mut self,
        backend: &mut B,
        font: FontId,
        text: &str,
        max_width: Option<f32>,
        mode: TruncateMode,
    ) -> &TextLayout {
        let max_w_key = max_width.map(|w| (w * 10.0).round() as i32).unwrap_or(-1);

        let mut hasher = rustc_hash::FxHasher::default();
        text.hash(&mut hasher);
        font.0.hash(&mut hasher);
        max_w_key.hash(&mut hasher);
        mode.hash(&mut hasher);
        let key = hasher.finish();

        if !self.cache.contains_key(&key) {
            let layout = compute_layout(backend, font, text, max_width, mode);
            if self.cache.len() >= self.capacity {
                if let Some(old_key) = self.lru.pop_front() {
                    self.cache.remove(&old_key);
                }
            }
            self.lru.push_back(key);
            self.cache.insert(key, layout);
        }

        self.cache.get(&key).expect("cached layout must exist")
    }
}

fn compute_layout<B: FontBackend>(
    backend: &mut B,
    font: FontId,
    text: &str,
    max_width: Option<f32>,
    mode: TruncateMode,
) -> TextLayout {
    let metrics = backend.metrics(font);
    let cell_w = metrics.cell_width;

    if text.is_empty() {
        return TextLayout {
            width: 0.0,
            height: metrics.cell_height,
            glyphs: Vec::new(),
            is_truncated: false,
        };
    }

    let full_width = text.chars().count() as f32 * cell_w;

    let (final_text, is_truncated) = if let Some(max_w) = max_width {
        if full_width > max_w && mode != TruncateMode::None {
            let ellipsis_w = cell_w;
            let avail_w = (max_w - ellipsis_w).max(0.0);
            let avail_chars = (avail_w / cell_w).floor() as usize;

            let truncated = match mode {
                TruncateMode::End => {
                    let mut s: String = text.chars().take(avail_chars).collect();
                    s.push('…');
                    s
                }
                TruncateMode::Middle => {
                    let total_chars = text.chars().count();
                    if avail_chars <= 1 || total_chars <= avail_chars {
                        let mut s: String = text.chars().take(avail_chars).collect();
                        s.push('…');
                        s
                    } else {
                        let half = avail_chars / 2;
                        let rem = avail_chars - half;
                        let prefix: String = text.chars().take(half).collect();
                        let suffix: String = text.chars().skip(total_chars - rem).collect();
                        format!("{}…{}", prefix, suffix)
                    }
                }
                TruncateMode::None => text.to_string(),
            };
            (truncated, true)
        } else {
            (text.to_string(), false)
        }
    } else {
        (text.to_string(), false)
    };

    let mut glyphs = Vec::with_capacity(final_text.len());
    let mut cur_x = 0.0f32;

    for ch in final_text.chars() {
        let g = backend.glyph(font, ch);
        glyphs.push(PositionedGlyph {
            glyph: g,
            x: cur_x,
            y: 0.0,
            x_advance: cell_w,
        });
        cur_x += cell_w;
    }

    TextLayout {
        width: cur_x,
        height: metrics.cell_height,
        glyphs,
        is_truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend;
    impl FontBackend for MockBackend {
        fn resolve(&mut self, _spec: &crate::FontSpec) -> FontId {
            FontId(0)
        }
        fn metrics(&self, _f: FontId) -> crate::FontMetrics {
            crate::FontMetrics {
                cell_width: 10.0,
                cell_height: 20.0,
                ascent: 15.0,
                descent: 5.0,
                leading: 0.0,
                underline_position: 2.0,
                underline_thickness: 1.0,
            }
        }
        fn glyph(&mut self, _f: FontId, c: char) -> GlyphRef {
            GlyphRef(c as u32)
        }
        fn shape(
            &mut self,
            _f: FontId,
            _text: &str,
            _lig: bool,
            _out: &mut Vec<crate::ShapedGlyph>,
        ) {
        }
        fn rasterize(
            &mut self,
            _g: GlyphRef,
            _scale: f32,
            _out: &mut crate::RasterBuf,
        ) -> crate::RasterInfo {
            crate::RasterInfo::Mask {
                width: 10,
                height: 20,
                left: 0,
                top: 20,
            }
        }
    }

    #[test]
    fn test_layout_no_truncation() {
        let mut backend = MockBackend;
        let mut cache = TextLayoutCache::default();
        let layout = cache.layout(&mut backend, FontId(0), "hello", None, TruncateMode::None);
        assert_eq!(layout.glyphs.len(), 5);
        assert_eq!(layout.width, 50.0);
        assert!(!layout.is_truncated);
    }

    #[test]
    fn test_layout_end_truncation() {
        let mut backend = MockBackend;
        let mut cache = TextLayoutCache::default();
        // 5 chars = 50px, max_w = 35px -> should fit 2 chars + '…' = 30px
        let layout = cache.layout(
            &mut backend,
            FontId(0),
            "hello",
            Some(35.0),
            TruncateMode::End,
        );
        assert!(layout.is_truncated);
        assert_eq!(layout.glyphs.len(), 3);
        assert_eq!(layout.width, 30.0);
        assert_eq!(layout.glyphs[2].glyph, GlyphRef('…' as u32));
    }

    #[test]
    fn test_layout_middle_truncation_for_paths() {
        let mut backend = MockBackend;
        let mut cache = TextLayoutCache::default();
        let path = "~/Projects/termin/src/main.rs";
        // Max width 120px (12 chars = 11 chars + '…')
        let layout = cache.layout(
            &mut backend,
            FontId(0),
            path,
            Some(120.0),
            TruncateMode::Middle,
        );
        assert!(layout.is_truncated);
        assert_eq!(layout.glyphs.len(), 12);
        assert_eq!(layout.width, 120.0);
        // Middle character must be '…'
        let has_ellipsis = layout
            .glyphs
            .iter()
            .any(|g| g.glyph == GlyphRef('…' as u32));
        assert!(has_ellipsis);
    }

    #[test]
    fn test_layout_cache_lru() {
        let mut backend = MockBackend;
        let mut cache = TextLayoutCache::new(2);
        let _ = cache.layout(&mut backend, FontId(0), "one", None, TruncateMode::None);
        let _ = cache.layout(&mut backend, FontId(0), "two", None, TruncateMode::None);
        assert_eq!(cache.cache.len(), 2);
        let _ = cache.layout(&mut backend, FontId(0), "three", None, TruncateMode::None);
        assert_eq!(cache.cache.len(), 2);
    }
}
