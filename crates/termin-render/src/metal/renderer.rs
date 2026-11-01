//! Main Metal renderer implementation and DrawList execution.
//!
//! See `docs/specs/text-and-rendering.md §3` and `docs/architecture.md §3.5`.

use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{
    MTLClearColor, MTLCommandBuffer, MTLCommandEncoder, MTLCommandQueue, MTLDevice, MTLDrawable,
    MTLLoadAction, MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLScissorRect, MTLStoreAction, MTLTexture, MTLViewport,
};
use objc2_quartz_core::{CAMetalDrawable, CAMetalLayer};
use termin_core::{Rect, Rgba, Size};
use termin_text::AtlasUpload;

use super::buffer::{create_unit_quad_buffer, srgb_to_linear_lut, InstanceRing, Uniforms};
use super::device::{configure_layer, create_device};
use super::pipeline::Pipelines;
use super::target::OffscreenTarget;
use super::texture::{execute_atlas_uploads, AtlasTexture, INITIAL_ATLAS_SIZE};
use super::RenderError;
use crate::{DrawList, RenderTarget, Renderer};

/// Native macOS Metal renderer for Termin.
pub struct MetalRenderer {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipelines: Pipelines,
    unit_quad: Retained<ProtocolObject<dyn objc2_metal::MTLBuffer>>,
    ring: InstanceRing,
    mask_atlas: AtlasTexture,
    color_atlas: Option<AtlasTexture>,
    staging_buffer: Vec<u8>,
    contrast_boost: f32,
    srgb_lut: [f32; 256],
    active_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    active_layer: Option<Retained<CAMetalLayer>>,
    configured_layer: usize,
}

impl MetalRenderer {
    /// Create a new Metal renderer selecting the default or low-power Metal device.
    pub fn new() -> Result<Self, RenderError> {
        let device = create_device()?;
        Self::with_device(device)
    }

    /// Create a Metal renderer with an explicit Metal device.
    pub fn with_device(
        device: Retained<ProtocolObject<dyn MTLDevice>>,
    ) -> Result<Self, RenderError> {
        let queue = device
            .newCommandQueue()
            .ok_or(RenderError::BufferAllocationFailed)?;

        let pipelines = Pipelines::new(&device)?;
        let unit_quad = create_unit_quad_buffer(&device)?;
        let ring = InstanceRing::new(&device)?;

        let mask_atlas = AtlasTexture::new(
            &device,
            INITIAL_ATLAS_SIZE,
            INITIAL_ATLAS_SIZE,
            objc2_metal::MTLPixelFormat::R8Unorm,
        )?;

        Ok(Self {
            device,
            queue,
            pipelines,
            unit_quad,
            ring,
            mask_atlas,
            color_atlas: None,
            staging_buffer: Vec::new(),
            contrast_boost: 0.0,
            srgb_lut: srgb_to_linear_lut(),
            active_texture: None,
            active_layer: None,
            configured_layer: 0,
        })
    }

    /// Access the underlying Metal device.
    pub fn device(&self) -> &ProtocolObject<dyn MTLDevice> {
        &self.device
    }

    /// Access the underlying Metal command queue.
    pub fn queue(&self) -> &ProtocolObject<dyn MTLCommandQueue> {
        &self.queue
    }

    /// Configure contrast/thicken boost for glyph rendering (default 0.0).
    pub fn set_contrast_boost(&mut self, boost: f32) {
        self.contrast_boost = boost;
    }

    /// Supply raw staging pixel data for pending atlas uploads.
    pub fn stage_atlas_data(&mut self, data: &[u8]) {
        self.staging_buffer.clear();
        self.staging_buffer.extend_from_slice(data);
    }

    /// Configure active target texture for subsequent `render()` calls.
    pub fn set_active_texture(
        &mut self,
        texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    ) {
        self.active_texture = texture;
    }

    /// Configure active CAMetalLayer for subsequent `render()` calls.
    pub fn set_active_layer(&mut self, layer: Option<Retained<CAMetalLayer>>) {
        if let Some(l) = &layer {
            configure_layer(l, &self.device);
        }
        self.active_layer = layer;
    }

    /// Render into a `CAMetalLayer` given as a raw pointer (as exposed by
    /// `termin_platform::RenderTarget::metal_layer`), so crates without objc2 can drive the renderer.
    /// The layer is configured (device, pixel format) once per distinct pointer.
    ///
    /// # Safety
    /// `layer` must point to a live `CAMetalLayer` for the duration of the call.
    pub unsafe fn render_to_raw_layer(
        &mut self,
        layer: *mut std::ffi::c_void,
        size: Size,
        scale_factor: f32,
        list: &DrawList,
        clear: Rgba,
    ) -> Result<(), RenderError> {
        if layer.is_null() {
            return Err(RenderError::DrawableUnavailable);
        }
        // SAFETY: caller guarantees `layer` is a valid CAMetalLayer pointer.
        let layer_ref: &CAMetalLayer = unsafe { &*(layer as *const CAMetalLayer) };
        if self.configured_layer != layer as usize {
            configure_layer(layer_ref, &self.device);
            self.configured_layer = layer as usize;
        }
        self.render_to_layer(layer_ref, size, scale_factor, list, clear)
    }

    /// Wait until all in-flight GPU frames have completed.
    pub fn wait_idle(&self) {
        self.ring.semaphore.wait_idle();
    }

    /// Render a DrawList directly to an OffscreenTarget synchronously (for tests).
    pub fn render_offscreen(
        &mut self,
        target: &OffscreenTarget,
        list: &DrawList,
        clear: Rgba,
    ) -> Result<(), RenderError> {
        self.render_to_texture_internal(
            &target.texture,
            None,
            target.size,
            target.scale_factor,
            list,
            clear,
            true,
            false,
        )
    }

    /// Render a DrawList directly into a CAMetalLayer.
    pub fn render_to_layer(
        &mut self,
        layer: &CAMetalLayer,
        size: Size,
        scale_factor: f32,
        list: &DrawList,
        clear: Rgba,
    ) -> Result<(), RenderError> {
        // SAFETY: Obtaining next drawable from CAMetalLayer.
        let drawable = {
            let _t = termin_core::trace::span(termin_core::trace::Phase::NextDrawable);
            unsafe { layer.nextDrawable() }.ok_or(RenderError::DrawableUnavailable)?
        };

        // SAFETY: Querying texture from valid CAMetalDrawable.
        let texture = unsafe { drawable.texture() };
        // SAFETY: Reading a BOOL property from a valid CAMetalLayer.
        let with_transaction = unsafe { layer.presentsWithTransaction() };
        self.render_to_texture_internal(
            &texture,
            Some(&drawable),
            size,
            scale_factor,
            list,
            clear,
            false,
            with_transaction,
        )
    }

    /// Internal encoder for rendering into an MTLTexture with optional drawable presentation.
    #[allow(clippy::too_many_arguments)]
    fn render_to_texture_internal(
        &mut self,
        texture: &ProtocolObject<dyn MTLTexture>,
        drawable: Option<&ProtocolObject<dyn CAMetalDrawable>>,
        size: Size,
        scale_factor: f32,
        list: &DrawList,
        clear: Rgba,
        wait: bool,
        present_with_transaction: bool,
    ) -> Result<(), RenderError> {
        let phys_w = (size.w * scale_factor).round() as u32;
        let phys_h = (size.h * scale_factor).round() as u32;

        if phys_w == 0 || phys_h == 0 {
            return Ok(());
        }

        // Wait on the 2-frame dispatch semaphore
        {
            let _t = termin_core::trace::span(termin_core::trace::Phase::SlotWait);
            self.ring.semaphore.wait_for_slot();
        }
        let encode_span = termin_core::trace::span(termin_core::trace::Phase::Encode);
        let slot_idx = self.ring.next_slot();

        // Count instance totals for the frame
        let total_quads: usize = list.layers.iter().map(|l| l.quads.len()).sum();
        let total_glyphs: usize = list
            .layers
            .iter()
            .map(|l| l.glyphs.len() + l.color_glyphs.len() + l.icons.len())
            .sum();

        // Ensure instance buffer capacities in ring slot
        self.ring.slots[slot_idx].ensure_quad_capacity(&self.device, total_quads)?;
        self.ring.slots[slot_idx].ensure_glyph_capacity(&self.device, total_glyphs)?;

        // Populate instance buffers sequentially
        let mut quad_cursor = 0;
        let mut glyph_cursor = 0;
        for layer in &list.layers {
            if !layer.quads.is_empty() {
                self.ring.slots[slot_idx].write_quads(quad_cursor, &layer.quads);
                quad_cursor += layer.quads.len();
            }
            if !layer.glyphs.is_empty() {
                self.ring.slots[slot_idx].write_glyphs(glyph_cursor, &layer.glyphs);
                glyph_cursor += layer.glyphs.len();
            }
            if !layer.color_glyphs.is_empty() {
                self.ring.slots[slot_idx].write_glyphs(glyph_cursor, &layer.color_glyphs);
                glyph_cursor += layer.color_glyphs.len();
            }
            if !layer.icons.is_empty() {
                self.ring.slots[slot_idx].write_glyphs(glyph_cursor, &layer.icons);
                glyph_cursor += layer.icons.len();
            }
        }

        // Populate uniforms buffer
        let uniforms = Uniforms {
            viewport_size: [phys_w as f32, phys_h as f32],
            scale_factor,
            contrast_boost: self.contrast_boost,
            srgb_to_linear: self.srgb_lut,
        };
        self.ring.slots[slot_idx].write_uniforms(&uniforms);

        // Create command buffer
        let cmd_buf = self
            .queue
            .commandBuffer()
            .ok_or(RenderError::BufferAllocationFailed)?;

        // Attach completion handler to signal semaphore on GPU frame completion
        let sem = self.ring.semaphore.clone();
        let block =
            block2::StackBlock::new(move |_cmd: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                sem.signal_completed();
            });
        // SAFETY: Registering completion handler callback on valid MTLCommandBuffer.
        unsafe {
            cmd_buf.addCompletedHandler(&*block as *const _ as *mut _);
        }

        // Configure render pass descriptor
        // SAFETY: Creating render pass descriptor.
        let pass_desc = unsafe { MTLRenderPassDescriptor::new() };
        // SAFETY: Color attachment index 0 is valid for a single render target pass.
        let color_att = unsafe { pass_desc.colorAttachments().objectAtIndexedSubscript(0) };
        color_att.setTexture(Some(texture));
        color_att.setLoadAction(MTLLoadAction::Clear);
        color_att.setStoreAction(MTLStoreAction::Store);
        // Metal MTLClearColor expects linear RGB values; hardware encodes to sRGB on write.
        color_att.setClearColor(MTLClearColor {
            red: self.srgb_lut[clear.r() as usize] as f64,
            green: self.srgb_lut[clear.g() as usize] as f64,
            blue: self.srgb_lut[clear.b() as usize] as f64,
            alpha: (clear.a() as f64) / 255.0,
        });

        // Create command encoder
        let encoder = cmd_buf
            .renderCommandEncoderWithDescriptor(&pass_desc)
            .ok_or(RenderError::BufferAllocationFailed)?;

        // Set viewport
        encoder.setViewport(MTLViewport {
            originX: 0.0,
            originY: 0.0,
            width: phys_w as f64,
            height: phys_h as f64,
            znear: 0.0,
            zfar: 1.0,
        });

        let slot = &self.ring.slots[slot_idx];

        // SAFETY: Binding unit-quad and uniforms buffers at configured buffer indices.
        unsafe {
            encoder.setVertexBuffer_offset_atIndex(Some(&self.unit_quad), 0, 0);
            encoder.setVertexBuffer_offset_atIndex(Some(&slot.uniforms_buffer), 0, 2);
            encoder.setFragmentBuffer_offset_atIndex(Some(&slot.uniforms_buffer), 0, 0);
        }

        // Render each layer in DrawList order
        let mut quad_offset = 0;
        let mut glyph_offset = 0;

        for layer in &list.layers {
            // Apply scissor rect
            let scissor = compute_scissor(layer.clip, scale_factor, phys_w, phys_h);
            if scissor.width == 0 || scissor.height == 0 {
                // Fully clipped layer; skip draw calls but advance offsets
                quad_offset += layer.quads.len();
                glyph_offset += layer.glyphs.len() + layer.color_glyphs.len() + layer.icons.len();
                continue;
            }
            encoder.setScissorRect(scissor);

            // 1. Quads
            if !layer.quads.is_empty() {
                encoder.setRenderPipelineState(&self.pipelines.quad);
                // SAFETY: Binding quad instance buffer and drawing instanced primitives.
                unsafe {
                    encoder.setVertexBuffer_offset_atIndex(Some(&slot.quad_buffer), 0, 1);
                    encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        layer.quads.len(),
                        quad_offset,
                    );
                }
                quad_offset += layer.quads.len();
            }

            // 2. Mask Glyphs
            if !layer.glyphs.is_empty() {
                encoder.setRenderPipelineState(&self.pipelines.glyph_mask);
                // SAFETY: Binding glyph instance buffer, mask atlas texture, and sampler state.
                unsafe {
                    encoder.setVertexBuffer_offset_atIndex(Some(&slot.glyph_buffer), 0, 1);
                    encoder.setFragmentTexture_atIndex(Some(&self.mask_atlas.texture), 0);
                    encoder.setFragmentSamplerState_atIndex(Some(&self.pipelines.sampler), 0);
                    encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        layer.glyphs.len(),
                        glyph_offset,
                    );
                }
                glyph_offset += layer.glyphs.len();
            }

            // 3. Color Glyphs
            if !layer.color_glyphs.is_empty() {
                encoder.setRenderPipelineState(&self.pipelines.glyph_color);
                // SAFETY: Binding glyph instance buffer, color atlas texture, and sampler state.
                unsafe {
                    encoder.setVertexBuffer_offset_atIndex(Some(&slot.glyph_buffer), 0, 1);
                    if let Some(atlas) = &self.color_atlas {
                        encoder.setFragmentTexture_atIndex(Some(&atlas.texture), 0);
                    }
                    encoder.setFragmentSamplerState_atIndex(Some(&self.pipelines.sampler), 0);
                    encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        layer.color_glyphs.len(),
                        glyph_offset,
                    );
                }
                glyph_offset += layer.color_glyphs.len();
            }

            // 4. Icons
            if !layer.icons.is_empty() {
                encoder.setRenderPipelineState(&self.pipelines.icon);
                // SAFETY: Binding icon glyph instance buffer, mask atlas texture, and sampler state.
                unsafe {
                    encoder.setVertexBuffer_offset_atIndex(Some(&slot.glyph_buffer), 0, 1);
                    encoder.setFragmentTexture_atIndex(Some(&self.mask_atlas.texture), 0);
                    encoder.setFragmentSamplerState_atIndex(Some(&self.pipelines.sampler), 0);
                    encoder.drawPrimitives_vertexStart_vertexCount_instanceCount_baseInstance(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        layer.icons.len(),
                        glyph_offset,
                    );
                }
                glyph_offset += layer.icons.len();
            }
        }

        encoder.endEncoding();

        drop(encode_span);
        if present_with_transaction {
            // CAMetalLayer.presentsWithTransaction requires: commit, wait until
            // scheduled, then present, so the frame lands in the current CA
            // transaction (in sync with the resizing window).
            {
                let _t = termin_core::trace::span(termin_core::trace::Phase::Submit);
                cmd_buf.commit();
                cmd_buf.waitUntilScheduled();
            }
            if let Some(d) = drawable {
                let _t = termin_core::trace::span(termin_core::trace::Phase::Present);
                let mtl_d: &ProtocolObject<dyn MTLDrawable> = ProtocolObject::from_ref(d);
                mtl_d.present();
            }
        } else {
            if let Some(d) = drawable {
                let _t = termin_core::trace::span(termin_core::trace::Phase::Present);
                let mtl_d: &ProtocolObject<dyn MTLDrawable> = ProtocolObject::from_ref(d);
                cmd_buf.presentDrawable(mtl_d);
            }
            let _t = termin_core::trace::span(termin_core::trace::Phase::Submit);
            cmd_buf.commit();
        }

        if wait {
            // SAFETY: Waiting synchronously for command buffer to complete on GPU.
            unsafe {
                cmd_buf.waitUntilCompleted();
            }
        }

        // Check buffer usage for shrinking after sustained idle
        let _ =
            self.ring
                .update_usage_and_shrink(&self.device, slot_idx, total_quads, total_glyphs);

        Ok(())
    }
}

impl Renderer for MetalRenderer {
    fn upload_glyphs(&mut self, uploads: &[AtlasUpload]) {
        let data = std::mem::take(&mut self.staging_buffer);
        let _ = execute_atlas_uploads(
            &self.device,
            &self.queue,
            &mut self.mask_atlas,
            &mut self.color_atlas,
            uploads,
            &data,
        );
        self.staging_buffer = data;
    }

    fn upload_glyphs_with_data(&mut self, uploads: &[AtlasUpload], data: &[u8]) {
        let _ = execute_atlas_uploads(
            &self.device,
            &self.queue,
            &mut self.mask_atlas,
            &mut self.color_atlas,
            uploads,
            data,
        );
    }

    fn render(&mut self, target: &mut dyn RenderTarget, list: &DrawList, clear: Rgba) {
        let size = target.size();
        let scale_factor = target.scale_factor();

        if let Some(texture) = self.active_texture.take() {
            let _ = self.render_to_texture_internal(
                &texture,
                None,
                size,
                scale_factor,
                list,
                clear,
                false,
                false,
            );
            self.active_texture = Some(texture);
        } else if let Some(layer) = self.active_layer.take() {
            let _ = self.render_to_layer(&layer, size, scale_factor, list, clear);
            self.active_layer = Some(layer);
        } else {
            log::warn!("MetalRenderer::render called with no active texture or CAMetalLayer set");
        }
    }

    fn release_idle_resources(&mut self) {
        self.staging_buffer.shrink_to_fit();
        let _ = self.ring.release_idle_resources(&self.device);
    }
}

/// Compute a physical pixel scissor rectangle clamped to the render viewport.
fn compute_scissor(
    clip: Option<Rect>,
    scale_factor: f32,
    viewport_width: u32,
    viewport_height: u32,
) -> MTLScissorRect {
    match clip {
        None => MTLScissorRect {
            x: 0,
            y: 0,
            width: viewport_width as usize,
            height: viewport_height as usize,
        },
        Some(rect) => {
            let x = (rect.x * scale_factor).max(0.0).round() as usize;
            let y = (rect.y * scale_factor).max(0.0).round() as usize;
            let w = (rect.w * scale_factor).max(0.0).round() as usize;
            let h = (rect.h * scale_factor).max(0.0).round() as usize;

            let clamped_x = x.min(viewport_width as usize);
            let clamped_y = y.min(viewport_height as usize);
            let clamped_w = w.min((viewport_width as usize).saturating_sub(clamped_x));
            let clamped_h = h.min((viewport_height as usize).saturating_sub(clamped_y));

            MTLScissorRect {
                x: clamped_x,
                y: clamped_y,
                width: clamped_w,
                height: clamped_h,
            }
        }
    }
}
