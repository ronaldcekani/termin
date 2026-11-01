//! Glyph cache managing atlas allocation, rasterization, and GPU upload queues.
//!
//! Handles grayscale mask and color atlas textures, tracks rasterized glyph slots,
//! buffers uploads into a reusable per-frame staging buffer, and doubles atlas textures on demand.
//!
//! See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §2.7`.

use rustc_hash::FxHashMap;
use std::collections::HashMap;

use crate::atlas::ShelfAtlas;
use crate::{AtlasKind, AtlasUpload, FontBackend, FontId, GlyphRef, RasterBuf, RasterInfo};

/// Cached glyph slot metadata in the atlas texture.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct CachedGlyph {
    pub atlas: AtlasKind,
    /// Texture coordinates [x, y, w, h] in the atlas texture.
    pub uv: [u16; 4],
    /// Glyph horizontal bearing offset in physical pixels.
    pub left: i32,
    /// Glyph vertical bearing offset in physical pixels.
    pub top: i32,
    /// Width of the rasterized glyph in physical pixels.
    pub width: u32,
    /// Height of the rasterized glyph in physical pixels.
    pub height: u32,
}

/// Glyph cache managing atlas textures and producing upload lists.
pub struct GlyphCache<B: FontBackend> {
    pub backend: B,
    /// Legacy char-to-glyph slot mapping for contract compatibility.
    pub slots: HashMap<(FontId, char), GlyphRef>,
    /// Mask atlas for grayscale text (starts 512x512, doubles up to 8192²).
    mask_atlas: ShelfAtlas,
    /// Lazily created color atlas for emoji (BGRA8).
    color_atlas: Option<ShelfAtlas>,
    /// Cache mapping (FontId, GlyphRef, subpixel_bin) to CachedGlyph.
    cached_glyphs: FxHashMap<(FontId, GlyphRef, u8), CachedGlyph>,
    /// Reusable staging buffer for current frame atlas uploads.
    staging_buf: Vec<u8>,
    /// Temporary rasterization buffer.
    raster_buf: RasterBuf,
    /// Queued atlas texture uploads.
    pending_uploads: Vec<AtlasUpload>,
    /// Bumped whenever cached glyph slots are invalidated (`clear`), so holders of
    /// atlas coordinates (row caches) know to rebuild.
    pub epoch: u64,
}

impl<B: FontBackend> GlyphCache<B> {
    /// Create a new glyph cache with the provided font backend.
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            slots: HashMap::new(),
            mask_atlas: ShelfAtlas::new(512, 512, 8192),
            color_atlas: None,
            cached_glyphs: FxHashMap::default(),
            staging_buf: Vec::new(),
            raster_buf: RasterBuf::default(),
            pending_uploads: Vec::new(),
            epoch: 0,
        }
    }

    /// Drain all queued atlas uploads for GPU texture copying.
    pub fn drain_uploads(&mut self, out: &mut Vec<AtlasUpload>) {
        out.append(&mut self.pending_uploads);
    }

    /// Read-only access to current staging buffer bytes.
    #[inline]
    pub fn staging_bytes(&self) -> &[u8] {
        &self.staging_buf
    }

    /// Clear the staging buffer after GPU frame encode.
    #[inline]
    pub fn clear_staging(&mut self) {
        self.staging_buf.clear();
    }

    /// Current mask atlas dimensions (width, height).
    #[inline]
    pub fn mask_atlas_size(&self) -> (u32, u32) {
        (self.mask_atlas.width(), self.mask_atlas.height())
    }

    /// Current color atlas dimensions (width, height), if initialized.
    #[inline]
    pub fn color_atlas_size(&self) -> Option<(u32, u32)> {
        self.color_atlas.as_ref().map(|a| (a.width(), a.height()))
    }

    /// Returns true if either atlas grew in size.
    pub fn did_grow(&self) -> bool {
        self.mask_atlas.did_grow() || self.color_atlas.as_ref().is_some_and(|a| a.did_grow())
    }

    /// Resets the atlas growth flags after GPU texture realloc / blit.
    pub fn reset_grow(&mut self) {
        self.mask_atlas.reset_grow();
        if let Some(ref mut ca) = self.color_atlas {
            ca.reset_grow();
        }
    }

    /// Clear all cached glyphs and atlas textures (e.g. on font size change or full atlas).
    pub fn clear(&mut self) {
        self.epoch += 1;
        self.mask_atlas.clear();
        if let Some(ref mut ca) = self.color_atlas {
            ca.clear();
        }
        self.cached_glyphs.clear();
        self.slots.clear();
        self.staging_buf.clear();
        self.pending_uploads.clear();
    }

    /// Look up or rasterize a glyph and return its cached atlas slot.
    pub fn get_or_insert(&mut self, font: FontId, glyph: GlyphRef, scale: f32) -> CachedGlyph {
        let key = (font, glyph, 0u8);
        if let Some(&cached) = self.cached_glyphs.get(&key) {
            return cached;
        }

        // Rasterize via backend
        let raster_info = self.backend.rasterize(glyph, scale, &mut self.raster_buf);

        let (kind, w, h, left, top) = match raster_info {
            RasterInfo::Mask {
                width,
                height,
                left,
                top,
            } => (AtlasKind::Mask, width, height, left, top),
            RasterInfo::Color {
                width,
                height,
                left,
                top,
            } => (AtlasKind::Color, width, height, left, top),
        };

        // Empty glyph (e.g. space) requires no atlas slot
        if w == 0 || h == 0 {
            let empty_cached = CachedGlyph {
                atlas: kind,
                uv: [0, 0, 0, 0],
                left,
                top,
                width: 0,
                height: 0,
            };
            self.cached_glyphs.insert(key, empty_cached);
            return empty_cached;
        }

        // Allocate slot in appropriate atlas
        let alloc_rect = match kind {
            AtlasKind::Mask => {
                if let Some(r) = self.mask_atlas.allocate(w, h) {
                    r
                } else {
                    // Evict and clear if full at max size
                    self.clear();
                    self.mask_atlas.allocate(w, h).unwrap_or_default()
                }
            }
            AtlasKind::Color => {
                let color_atlas = self
                    .color_atlas
                    .get_or_insert_with(|| ShelfAtlas::new(512, 512, 8192));
                if let Some(r) = color_atlas.allocate(w, h) {
                    r
                } else {
                    self.clear();
                    let color_atlas = self
                        .color_atlas
                        .get_or_insert_with(|| ShelfAtlas::new(512, 512, 8192));
                    color_atlas.allocate(w, h).unwrap_or_default()
                }
            }
        };

        // Copy rendered pixels into staging buffer
        let offset = self.staging_buf.len();
        let bytes_len = self.raster_buf.data.len();
        self.staging_buf.extend_from_slice(&self.raster_buf.data);

        // Queue upload for renderer
        self.pending_uploads.push(AtlasUpload {
            atlas: kind,
            rect: alloc_rect.into(),
            bytes_offset: offset,
            bytes_len,
        });

        let cached = CachedGlyph {
            atlas: kind,
            uv: [
                alloc_rect.x as u16,
                alloc_rect.y as u16,
                alloc_rect.w as u16,
                alloc_rect.h as u16,
            ],
            left,
            top,
            width: w,
            height: h,
        };

        self.cached_glyphs.insert(key, cached);
        cached
    }

    /// Convenience lookup by character scalar.
    pub fn get_or_insert_char(&mut self, font: FontId, c: char, scale: f32) -> CachedGlyph {
        let glyph = self.backend.glyph(font, c);
        self.slots.insert((font, c), glyph);
        self.get_or_insert(font, glyph, scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FontMetrics, FontSpec, ShapedGlyph};

    struct MockBackend;
    impl FontBackend for MockBackend {
        fn resolve(&mut self, _spec: &FontSpec) -> FontId {
            FontId(0)
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
        fn glyph(&mut self, _f: FontId, c: char) -> GlyphRef {
            GlyphRef(c as u32)
        }
        fn shape(&mut self, _f: FontId, _text: &str, _lig: bool, _out: &mut Vec<ShapedGlyph>) {}
        fn rasterize(&mut self, g: GlyphRef, _scale: f32, out: &mut RasterBuf) -> RasterInfo {
            if g.0 == ' ' as u32 {
                out.data.clear();
                return RasterInfo::Mask {
                    width: 0,
                    height: 0,
                    left: 0,
                    top: 0,
                };
            }
            if g.0 == 0x1F680 {
                // Rocket emoji (color)
                out.data.resize(16 * 16 * 4, 255);
                return RasterInfo::Color {
                    width: 16,
                    height: 16,
                    left: 0,
                    top: 16,
                };
            }
            out.data.resize(8 * 16, 255);
            RasterInfo::Mask {
                width: 8,
                height: 16,
                left: 0,
                top: 16,
            }
        }
    }

    #[test]
    fn test_glyph_cache_uploads_and_staging() {
        let mut cache = GlyphCache::new(MockBackend);
        let font = FontId(0);

        let g_a = cache.get_or_insert_char(font, 'A', 2.0);
        assert_eq!(g_a.atlas, AtlasKind::Mask);
        assert_eq!(g_a.width, 8);
        assert_eq!(g_a.height, 16);

        // Verify upload queued
        let mut uploads = Vec::new();
        cache.drain_uploads(&mut uploads);
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].bytes_len, 8 * 16);

        // Re-lookup should be cached (no new uploads)
        let g_a2 = cache.get_or_insert_char(font, 'A', 2.0);
        assert_eq!(g_a, g_a2);
        uploads.clear();
        cache.drain_uploads(&mut uploads);
        assert_eq!(uploads.len(), 0);

        // Color emoji
        let g_rocket = cache.get_or_insert_char(font, '\u{1F680}', 2.0);
        assert_eq!(g_rocket.atlas, AtlasKind::Color);
        cache.drain_uploads(&mut uploads);
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].atlas, AtlasKind::Color);

        // Space (empty)
        let g_space = cache.get_or_insert_char(font, ' ', 2.0);
        assert_eq!(g_space.width, 0);
        assert_eq!(g_space.height, 0);
    }
}
