//! Font resolution, shaping, glyph caching, and rasterization (`termin-text`).
//!
//! Provides the CoreText `FontBackend` implementation for macOS,
//! shelf atlas packing, CPU procedural glyph rasterization (box-drawing, blocks, braille, powerline),
//! glyph caching with staging upload buffers, and UI text layout with ellipsis truncation.
//!
//! See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §2, §4`.

pub mod atlas;
pub mod cache;
pub mod layout;
pub mod procedural;

#[cfg(target_os = "macos")]
pub mod coretext;

#[cfg(target_os = "macos")]
pub use coretext::CoreTextBackend;

pub use atlas::{AtlasRect, ShelfAtlas};
pub use cache::{CachedGlyph, GlyphCache};
pub use layout::{PositionedGlyph, TextLayout, TextLayoutCache, TruncateMode};
pub use procedural::{is_procedural, rasterize_procedural};

use termin_core::Rect;

/// Handle to a loaded font.
///
/// See `docs/architecture.md §3.5`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontId(pub u32);

/// Glyph reference within a resolved font.
///
/// See `docs/architecture.md §3.5`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlyphRef(pub u32);

/// Font weight values.
///
/// See `docs/specs/text-and-rendering.md §2.1`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum FontWeight {
    Thin,
    Light,
    #[default]
    Regular,
    Medium,
    SemiBold,
    Bold,
    ExtraBold,
    Black,
}

/// Font specification for resolution.
///
/// See `docs/specs/text-and-rendering.md §2.1`.
#[derive(Clone, Debug, PartialEq)]
pub struct FontSpec {
    pub family: Option<String>,
    pub size_pt: f32,
    pub weight: FontWeight,
    pub italic: bool,
}

impl Default for FontSpec {
    fn default() -> Self {
        Self {
            family: None,
            size_pt: 13.0,
            weight: FontWeight::Regular,
            italic: false,
        }
    }
}

/// Metrics of a resolved font.
///
/// See `docs/specs/text-and-rendering.md §2.2`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct FontMetrics {
    pub cell_width: f32,
    pub cell_height: f32,
    pub ascent: f32,
    pub descent: f32,
    pub leading: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
}

/// Shaped glyph output with placement offset and advance.
///
/// See `docs/architecture.md §3.5`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub glyph: GlyphRef,
    pub x_offset: f32,
    pub y_offset: f32,
    pub x_advance: f32,
    pub cluster: u32,
}

/// Destination atlas kind (grayscale mask or BGRA color).
///
/// See `docs/specs/text-and-rendering.md §2.7`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum AtlasKind {
    #[default]
    Mask,
    Color,
}

/// Information returned after rasterizing a glyph into pixel buffer.
///
/// See `docs/specs/text-and-rendering.md §2.5`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RasterInfo {
    Mask {
        width: u32,
        height: u32,
        left: i32,
        top: i32,
    },
    Color {
        width: u32,
        height: u32,
        left: i32,
        top: i32,
    },
}

/// Pixel staging buffer for rasterization.
///
/// See `docs/architecture.md §3.5`.
#[derive(Clone, Debug, Default)]
pub struct RasterBuf {
    pub data: Vec<u8>,
}

/// Upload command for copying rendered glyph pixels into the GPU atlas texture.
///
/// See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §2.7`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct AtlasUpload {
    pub atlas: AtlasKind,
    pub rect: Rect,
    pub bytes_offset: usize,
    pub bytes_len: usize,
}

/// Abstract font backend interface (CoreText on macOS, FreeType/DirectWrite later).
///
/// See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §2`.
pub trait FontBackend {
    fn resolve(&mut self, spec: &FontSpec) -> FontId;
    fn metrics(&self, f: FontId) -> FontMetrics;
    fn glyph(&mut self, f: FontId, c: char) -> GlyphRef;
    fn shape(&mut self, f: FontId, text: &str, ligatures: bool, out: &mut Vec<ShapedGlyph>);
    fn rasterize(&mut self, g: GlyphRef, scale: f32, out: &mut RasterBuf) -> RasterInfo;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockBackend;
    impl FontBackend for MockBackend {
        fn resolve(&mut self, _spec: &FontSpec) -> FontId {
            FontId(1)
        }
        fn metrics(&self, _f: FontId) -> FontMetrics {
            FontMetrics {
                cell_width: 8.0,
                cell_height: 16.0,
                ascent: 12.0,
                descent: 4.0,
                leading: 0.0,
                underline_position: 2.0,
                underline_thickness: 1.0,
            }
        }
        fn glyph(&mut self, _f: FontId, _c: char) -> GlyphRef {
            GlyphRef(42)
        }
        fn shape(&mut self, _f: FontId, _text: &str, _lig: bool, out: &mut Vec<ShapedGlyph>) {
            out.push(ShapedGlyph {
                glyph: GlyphRef(42),
                x_offset: 0.0,
                y_offset: 0.0,
                x_advance: 8.0,
                cluster: 0,
            });
        }
        fn rasterize(&mut self, _g: GlyphRef, _scale: f32, out: &mut RasterBuf) -> RasterInfo {
            out.data.resize(64, 255);
            RasterInfo::Mask {
                width: 8,
                height: 8,
                left: 0,
                top: 8,
            }
        }
    }

    #[test]
    fn test_glyph_cache_creation() {
        let mut cache = GlyphCache::new(MockBackend);
        let id = cache.backend.resolve(&FontSpec::default());
        assert_eq!(id, FontId(1));
        let m = cache.backend.metrics(id);
        assert_eq!(m.cell_width, 8.0);
    }
}
