//! Metal rendering pipeline and DrawList presentation (`termin-render`).
//!
//! See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §3`.

use termin_core::{Rect, Rgba};
use termin_platform::RenderTarget;
use termin_text::AtlasUpload;

/// Instanced quad primitive for background fills, rounded rectangles, borders, and shadows (40 bytes).
///
/// See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §3.2`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct QuadInst {
    pub rect: [f32; 4],
    pub color: u32,
    pub border_color: u32,
    pub radius: f32,
    pub border: f32,
    pub shadow: f32,
    pub _pad: f32,
}

// Compile-time size assertion: exactly 40 bytes
const _: () = assert!(std::mem::size_of::<QuadInst>() == 40);

/// Instanced glyph primitive referencing atlas coordinates (24 bytes).
///
/// See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §3.2`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GlyphInst {
    pub pos: [f32; 2],
    pub size: [u16; 2],
    pub uv: [u16; 4],
    pub color: u32,
}

// Compile-time size assertion: exactly 24 bytes
const _: () = assert!(std::mem::size_of::<GlyphInst>() == 24);

/// A single rendering layer with optional scissor clipping and instance arrays.
///
/// See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §3.3`.
#[derive(Clone, Debug, Default)]
pub struct Layer {
    pub clip: Option<Rect>,
    pub quads: Vec<QuadInst>,
    pub glyphs: Vec<GlyphInst>,
    pub color_glyphs: Vec<GlyphInst>,
    pub icons: Vec<GlyphInst>,
}

impl Layer {
    pub fn new(clip: Option<Rect>) -> Self {
        Self {
            clip,
            quads: Vec::new(),
            glyphs: Vec::new(),
            color_glyphs: Vec::new(),
            icons: Vec::new(),
        }
    }
}

/// A draw list composed of ordered rendering layers.
///
/// See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §3.3`.
#[derive(Clone, Debug, Default)]
pub struct DrawList {
    pub layers: Vec<Layer>,
}

impl DrawList {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn push_layer(&mut self, layer: Layer) {
        self.layers.push(layer);
    }

    pub fn clear(&mut self) {
        self.layers.clear();
    }
}

/// Abstract renderer interface (Metal on macOS, Vulkan/D3D11 later).
///
/// See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §3`.
pub trait Renderer {
    /// Upload rasterized glyph pixels to GPU atlas textures.
    fn upload_glyphs(&mut self, uploads: &[AtlasUpload]);

    /// Present a complete draw list into the target with clear color.
    fn render(&mut self, target: &mut dyn RenderTarget, list: &DrawList, clear: Rgba);

    /// Release staging and instance ring buffers when idle.
    fn release_idle_resources(&mut self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instance_struct_sizes() {
        assert_eq!(std::mem::size_of::<QuadInst>(), 40);
        assert_eq!(std::mem::size_of::<GlyphInst>(), 24);
    }

    #[test]
    fn test_draw_list_push() {
        let mut list = DrawList::new();
        let mut layer = Layer::new(None);
        layer.quads.push(QuadInst {
            rect: [0.0, 0.0, 100.0, 50.0],
            color: 0xFFFFFFFF,
            border_color: 0,
            radius: 4.0,
            border: 1.0,
            shadow: 0.0,
            _pad: 0.0,
        });
        list.push_layer(layer);
        assert_eq!(list.layers.len(), 1);
        assert_eq!(list.layers[0].quads.len(), 1);
    }
}
