//! Offscreen render target and readback for headless testing.
//!
//! See `docs/specs/text-and-rendering.md §5`.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLDevice, MTLOrigin, MTLPixelFormat, MTLRegion, MTLResourceOptions, MTLSize, MTLStorageMode,
    MTLTexture, MTLTextureDescriptor, MTLTextureUsage,
};
use termin_core::Size;
use crate::RenderTarget;

use super::RenderError;

/// An offscreen rendering target implementing `RenderTarget` for tests and snapshot verification.
pub struct OffscreenTarget {
    pub texture: Retained<ProtocolObject<dyn MTLTexture>>,
    pub size: Size,
    pub scale_factor: f32,
    pub width: u32,
    pub height: u32,
}

impl OffscreenTarget {
    /// Create a new offscreen target with physical dimensions (width × height) and logical scale factor.
    pub fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> Result<Self, RenderError> {
        // SAFETY: Creating 2D texture descriptor with standard pixel format and dimensions.
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm_sRGB,
                width as usize,
                height as usize,
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
        desc.setStorageMode(MTLStorageMode::Shared);
        desc.setResourceOptions(MTLResourceOptions::MTLResourceStorageModeShared);

        let texture = device
            .newTextureWithDescriptor(&desc)
            .ok_or(RenderError::TextureCreationFailed)?;

        let size = Size {
            w: (width as f32) / scale_factor,
            h: (height as f32) / scale_factor,
        };

        Ok(Self {
            texture,
            size,
            scale_factor,
            width,
            height,
        })
    }
}

impl RenderTarget for OffscreenTarget {
    fn size(&self) -> Size {
        self.size
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor
    }
}

/// Read back raw pixels from an MTLTexture and convert from BGRA to RGBA for testing.
pub fn read_texture_rgba(
    texture: &ProtocolObject<dyn MTLTexture>,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let bytes_per_row = (width as usize) * 4;
    let total_bytes = bytes_per_row * (height as usize);
    let mut pixels = vec![0u8; total_bytes];

    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: width as usize,
            height: height as usize,
            depth: 1,
        },
    };

    // SAFETY: pixels vector has exactly total_bytes allocated.
    unsafe {
        texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            NonNull::new(pixels.as_mut_ptr() as *mut _).unwrap(),
            bytes_per_row,
            region,
            0,
        );
    }

    // Convert BGRA to RGBA
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    pixels
}
