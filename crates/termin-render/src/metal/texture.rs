//! Metal texture and atlas management: R8 mask atlas, BGRA color atlas, growth via blit, and uploads.
//!
//! See `docs/specs/text-and-rendering.md §2.7`.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLBlitCommandEncoder, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLDevice,
    MTLOrigin, MTLPixelFormat, MTLRegion, MTLResourceOptions, MTLSize, MTLStorageMode, MTLTexture,
    MTLTextureDescriptor, MTLTextureUsage,
};
use termin_text::{AtlasKind, AtlasUpload};

use super::RenderError;

pub const INITIAL_ATLAS_SIZE: u32 = 512;
pub const MAX_ATLAS_SIZE: u32 = 8192;

/// A GPU-resident texture atlas for glyphs or icons with on-demand growth.
pub struct AtlasTexture {
    pub texture: Retained<ProtocolObject<dyn MTLTexture>>,
    pub width: u32,
    pub height: u32,
    pub format: MTLPixelFormat,
}

impl AtlasTexture {
    /// Create a new atlas texture with given dimensions and pixel format.
    pub fn new(
        device: &ProtocolObject<dyn MTLDevice>,
        width: u32,
        height: u32,
        format: MTLPixelFormat,
    ) -> Result<Self, RenderError> {
        let texture = Self::create_raw_texture(device, width, height, format)?;
        Ok(Self {
            texture,
            width,
            height,
            format,
        })
    }

    fn create_raw_texture(
        device: &ProtocolObject<dyn MTLDevice>,
        width: u32,
        height: u32,
        format: MTLPixelFormat,
    ) -> Result<Retained<ProtocolObject<dyn MTLTexture>>, RenderError> {
        // SAFETY: Creating 2D texture descriptor with standard pixel format and dimensions.
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                format,
                width as usize,
                height as usize,
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
        desc.setStorageMode(MTLStorageMode::Shared);
        desc.setResourceOptions(MTLResourceOptions::MTLResourceStorageModeShared);

        device
            .newTextureWithDescriptor(&desc)
            .ok_or(RenderError::TextureCreationFailed)
    }

    /// Grow the atlas by doubling dimensions up to MAX_ATLAS_SIZE and blitting existing contents.
    ///
    /// See `docs/specs/text-and-rendering.md §2.7`.
    pub fn grow(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        queue: &ProtocolObject<dyn MTLCommandQueue>,
        min_width: u32,
        min_height: u32,
    ) -> Result<(), RenderError> {
        let mut new_w = self.width;
        let mut new_h = self.height;

        while new_w < min_width && new_w < MAX_ATLAS_SIZE {
            new_w = new_w.saturating_mul(2);
        }
        while new_h < min_height && new_h < MAX_ATLAS_SIZE {
            new_h = new_h.saturating_mul(2);
        }

        if new_w == self.width && new_h == self.height {
            return Ok(());
        }

        let new_texture = Self::create_raw_texture(device, new_w, new_h, self.format)?;

        // Blit existing pixels to new enlarged texture
        let cmd_buf = queue
            .commandBuffer()
            .ok_or(RenderError::BufferAllocationFailed)?;

        if let Some(blit) = cmd_buf.blitCommandEncoder() {
            let origin = MTLOrigin { x: 0, y: 0, z: 0 };
            let size = MTLSize {
                width: self.width as usize,
                height: self.height as usize,
                depth: 1,
            };

            // SAFETY: Source and destination textures are valid, sizes match original dimensions.
            unsafe {
                blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                    &self.texture,
                    0,
                    0,
                    origin,
                    size,
                    &new_texture,
                    0,
                    0,
                    origin,
                );
                blit.endEncoding();
            }
            cmd_buf.commit();
            // SAFETY: Synchronously waiting for blit copy to finish.
            unsafe {
                cmd_buf.waitUntilCompleted();
            }
        }

        self.texture = new_texture;
        self.width = new_w;
        self.height = new_h;
        Ok(())
    }

    /// Upload a rectangle of pixels into the atlas.
    pub fn upload(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        queue: &ProtocolObject<dyn MTLCommandQueue>,
        rect: [u32; 4],
        bytes: &[u8],
    ) -> Result<(), RenderError> {
        let [x, y, w, h] = rect;
        if w == 0 || h == 0 || bytes.is_empty() {
            return Ok(());
        }

        // Check if atlas needs growth
        if x + w > self.width || y + h > self.height {
            self.grow(device, queue, x + w, y + h)?;
        }

        let bytes_per_pixel = match self.format {
            MTLPixelFormat::R8Unorm => 1,
            MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB => 4,
            _ => 4,
        };
        let bytes_per_row = (w as usize) * bytes_per_pixel;

        let region = MTLRegion {
            origin: MTLOrigin {
                x: x as usize,
                y: y as usize,
                z: 0,
            },
            size: MTLSize {
                width: w as usize,
                height: h as usize,
                depth: 1,
            },
        };

        // SAFETY: The provided bytes slice contains at least bytes_per_row * h bytes.
        unsafe {
            self.texture
                .replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                    region,
                    0,
                    NonNull::new(bytes.as_ptr() as *mut _).unwrap(),
                    bytes_per_row,
                );
        }

        Ok(())
    }
}

/// Execute a batch of `AtlasUpload` commands into the mask and color atlases.
pub fn execute_atlas_uploads(
    device: &ProtocolObject<dyn MTLDevice>,
    queue: &ProtocolObject<dyn MTLCommandQueue>,
    mask_atlas: &mut AtlasTexture,
    color_atlas: &mut Option<AtlasTexture>,
    uploads: &[AtlasUpload],
    staging_data: &[u8],
) -> Result<(), RenderError> {
    for upload in uploads {
        let x = upload.rect.x as u32;
        let y = upload.rect.y as u32;
        let w = upload.rect.w as u32;
        let h = upload.rect.h as u32;

        if w == 0 || h == 0 {
            continue;
        }

        let offset = upload.bytes_offset;
        let len = upload.bytes_len;
        if offset + len > staging_data.len() {
            continue;
        }
        let slice = &staging_data[offset..offset + len];

        match upload.atlas {
            AtlasKind::Mask => {
                mask_atlas.upload(device, queue, [x, y, w, h], slice)?;
            }
            AtlasKind::Color => {
                if color_atlas.is_none() {
                    *color_atlas = Some(AtlasTexture::new(
                        device,
                        INITIAL_ATLAS_SIZE,
                        INITIAL_ATLAS_SIZE,
                        MTLPixelFormat::BGRA8Unorm,
                    )?);
                }
                if let Some(atlas) = color_atlas.as_mut() {
                    atlas.upload(device, queue, [x, y, w, h], slice)?;
                }
            }
        }
    }
    Ok(())
}
