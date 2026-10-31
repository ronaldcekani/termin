//! Metal rendering pipeline and DrawList presentation (`termin-render`).
//!
//! See `docs/architecture.md §3.5` and `docs/specs/text-and-rendering.md §3`.

use termin_core::{Rect, Rgba, Size};
use termin_text::AtlasUpload;

/// Trait representing an active render target (e.g. `CAMetalLayer` drawable or offscreen texture).
///
/// See `docs/architecture.md §3.4` and `§3.5`.
pub trait RenderTarget {
    fn size(&self) -> Size;
    fn scale_factor(&self) -> f32;
}

/// Marker trait for Plain Old Data types safe to copy as bytes into GPU buffers.
///
/// # Safety
/// Implementing type must be `#[repr(C)]`, have no padding bytes containing uninitialized
/// data (or all bytes valid), and contain no references or non-Pod fields.
pub unsafe trait Pod: Copy + 'static {}

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

// SAFETY: QuadInst is #[repr(C)], 40 bytes, all fields are primitive numbers (f32, u32) with no invalid bit patterns.
unsafe impl Pod for QuadInst {}

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

// SAFETY: GlyphInst is #[repr(C)], 24 bytes, all fields are primitive numbers (f32, u16, u32) with no invalid bit patterns.
unsafe impl Pod for GlyphInst {}

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

    /// Upload rasterized glyph pixels with provided pixel buffer.
    fn upload_glyphs_with_data(&mut self, uploads: &[AtlasUpload], _data: &[u8]) {
        self.upload_glyphs(uploads);
    }

    /// Present a complete draw list into the target with clear color.
    fn render(&mut self, target: &mut dyn RenderTarget, list: &DrawList, clear: Rgba);

    /// Release staging and instance ring buffers when idle.
    fn release_idle_resources(&mut self);
}

#[cfg(target_os = "macos")]
pub mod metal;

#[cfg(target_os = "macos")]
pub use metal::{
    configure_layer, create_device, read_texture_rgba, MetalRenderer, OffscreenTarget, Pipelines,
    RenderError,
};
