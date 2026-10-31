//! macOS Metal rendering implementation for Termin.
//!
//! See `docs/specs/text-and-rendering.md §3` and `docs/architecture.md §3.5`.

pub mod buffer;
pub mod device;
pub mod pipeline;
pub mod renderer;
pub mod target;
pub mod texture;

pub use device::{configure_layer, create_device};
pub use pipeline::Pipelines;
pub use renderer::MetalRenderer;
pub use target::{read_texture_rgba, OffscreenTarget};

use std::fmt;

/// Hand-written error enum for Metal rendering failures.
///
/// See `docs/architecture.md §6`.
#[derive(Debug)]
pub enum RenderError {
    NoMetalDevice,
    ShaderCompilationFailed(String),
    PipelineCreationFailed(String),
    BufferAllocationFailed,
    TextureCreationFailed,
    DrawableUnavailable,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoMetalDevice => write!(f, "no Metal device found"),
            Self::ShaderCompilationFailed(msg) => write!(f, "shader compilation failed: {msg}"),
            Self::PipelineCreationFailed(msg) => write!(f, "pipeline creation failed: {msg}"),
            Self::BufferAllocationFailed => write!(f, "failed to allocate Metal buffer"),
            Self::TextureCreationFailed => write!(f, "failed to create Metal texture"),
            Self::DrawableUnavailable => write!(f, "next drawable is unavailable"),
        }
    }
}

impl std::error::Error for RenderError {}
