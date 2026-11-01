//! Metal buffer management: unit quad, uniforms, ring buffers, and 2-frame dispatch semaphore.
//!
//! See `docs/specs/text-and-rendering.md §3.1` and `§3.6`.

use std::ptr::NonNull;
use std::sync::{Arc, Condvar, Mutex};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLBuffer, MTLDevice, MTLResourceOptions};

use super::RenderError;
use crate::{GlyphInst, Pod, QuadInst};

pub const MIN_QUAD_CAPACITY: usize = 256;
pub const MIN_GLYPH_CAPACITY: usize = 1024;
pub const LOW_USAGE_THRESHOLD_FRAMES: usize = 300;

/// Uniforms buffer structure matching MSL `Uniforms`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct Uniforms {
    pub viewport_size: [f32; 2],
    pub scale_factor: f32,
    pub contrast_boost: f32,
    pub srgb_to_linear: [f32; 256],
}

// SAFETY: Uniforms is #[repr(C)], consists purely of primitive f32 arrays/floats with no invalid bit patterns.
unsafe impl Pod for Uniforms {}

/// Compute the exact 256-entry piecewise sRGB-to-linear conversion table.
///
/// See `docs/specs/text-and-rendering.md §3.6`.
pub fn srgb_to_linear_lut() -> [f32; 256] {
    let mut lut = [0.0f32; 256];
    for (i, entry) in lut.iter_mut().enumerate() {
        let c = (i as f32) / 255.0;
        *entry = if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        };
    }
    lut
}

/// Create the shared unit-quad vertex buffer (4 vertices for `MTLPrimitiveTypeTriangleStrip`).
pub fn create_unit_quad_buffer(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Result<Retained<ProtocolObject<dyn MTLBuffer>>, RenderError> {
    const VERTICES: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let len = std::mem::size_of_val(&VERTICES);
    // SAFETY: Creating buffer with valid non-null pointer to static vertex data and storageModeShared.
    let buffer = unsafe {
        device.newBufferWithBytes_length_options(
            NonNull::new(VERTICES.as_ptr() as *mut _).unwrap(),
            len,
            MTLResourceOptions::MTLResourceStorageModeShared,
        )
    };
    buffer.ok_or(RenderError::BufferAllocationFailed)
}

/// Counting semaphore for synchronizing 2 in-flight GPU frames.
///
/// See `docs/specs/text-and-rendering.md §3.1`.
#[derive(Debug)]
pub struct FrameSemaphore {
    in_flight: Mutex<usize>,
    cvar: Condvar,
    max_in_flight: usize,
}

impl FrameSemaphore {
    pub fn new(max_in_flight: usize) -> Self {
        Self {
            in_flight: Mutex::new(0),
            cvar: Condvar::new(),
            max_in_flight,
        }
    }

    /// Wait until a ring buffer slot is available (in-flight frames < max_in_flight).
    pub fn wait_for_slot(&self) {
        let mut count = self.in_flight.lock().unwrap();
        while *count >= self.max_in_flight {
            count = self.cvar.wait(count).unwrap();
        }
        *count += 1;
    }

    /// Signal that a command buffer has completed execution on the GPU.
    pub fn signal_completed(&self) {
        let mut count = self.in_flight.lock().unwrap();
        if *count > 0 {
            *count -= 1;
            self.cvar.notify_one();
        }
    }

    /// Wait until all in-flight frames have completed on the GPU (used in tests / cleanup).
    pub fn wait_idle(&self) {
        let mut count = self.in_flight.lock().unwrap();
        while *count > 0 {
            count = self.cvar.wait(count).unwrap();
        }
    }
}

/// A single frame slot in the instance ring buffer.
pub struct InstanceSlot {
    pub quad_buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub quad_capacity: usize,
    pub glyph_buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
    pub glyph_capacity: usize,
    pub uniforms_buffer: Retained<ProtocolObject<dyn MTLBuffer>>,
}

impl InstanceSlot {
    pub fn new(device: &ProtocolObject<dyn MTLDevice>) -> Result<Self, RenderError> {
        let quad_capacity = MIN_QUAD_CAPACITY;
        let quad_bytes = quad_capacity * std::mem::size_of::<QuadInst>();
        let quad_buffer = device
            .newBufferWithLength_options(
                quad_bytes,
                MTLResourceOptions::MTLResourceStorageModeShared,
            )
            .ok_or(RenderError::BufferAllocationFailed)?;

        let glyph_capacity = MIN_GLYPH_CAPACITY;
        let glyph_bytes = glyph_capacity * std::mem::size_of::<GlyphInst>();
        let glyph_buffer = device
            .newBufferWithLength_options(
                glyph_bytes,
                MTLResourceOptions::MTLResourceStorageModeShared,
            )
            .ok_or(RenderError::BufferAllocationFailed)?;

        let uniforms_bytes = std::mem::size_of::<Uniforms>();
        let uniforms_buffer = device
            .newBufferWithLength_options(
                uniforms_bytes,
                MTLResourceOptions::MTLResourceStorageModeShared,
            )
            .ok_or(RenderError::BufferAllocationFailed)?;

        Ok(Self {
            quad_buffer,
            quad_capacity,
            glyph_buffer,
            glyph_capacity,
            uniforms_buffer,
        })
    }

    /// Ensure quad capacity by doubling until it fits the required instance count.
    pub fn ensure_quad_capacity(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        needed: usize,
    ) -> Result<(), RenderError> {
        if needed > self.quad_capacity {
            let mut new_cap = self.quad_capacity.max(MIN_QUAD_CAPACITY);
            while new_cap < needed {
                new_cap = new_cap.saturating_mul(2);
            }
            let bytes = new_cap * std::mem::size_of::<QuadInst>();
            let new_buf = device
                .newBufferWithLength_options(
                    bytes,
                    MTLResourceOptions::MTLResourceStorageModeShared,
                )
                .ok_or(RenderError::BufferAllocationFailed)?;

            self.quad_buffer = new_buf;
            self.quad_capacity = new_cap;
        }
        Ok(())
    }

    /// Ensure glyph capacity by doubling until it fits the required instance count.
    pub fn ensure_glyph_capacity(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        needed: usize,
    ) -> Result<(), RenderError> {
        if needed > self.glyph_capacity {
            let mut new_cap = self.glyph_capacity.max(MIN_GLYPH_CAPACITY);
            while new_cap < needed {
                new_cap = new_cap.saturating_mul(2);
            }
            let bytes = new_cap * std::mem::size_of::<GlyphInst>();
            let new_buf = device
                .newBufferWithLength_options(
                    bytes,
                    MTLResourceOptions::MTLResourceStorageModeShared,
                )
                .ok_or(RenderError::BufferAllocationFailed)?;

            self.glyph_buffer = new_buf;
            self.glyph_capacity = new_cap;
        }
        Ok(())
    }

    /// Write quads into the shared buffer at the specified instance offset.
    pub fn write_quads(&self, offset: usize, quads: &[QuadInst]) {
        if quads.is_empty() {
            return;
        }
        assert!(offset + quads.len() <= self.quad_capacity);
        let ptr = self.quad_buffer.contents().as_ptr() as *mut QuadInst;
        // SAFETY: Target buffer is valid shared memory allocated with sufficient capacity.
        unsafe {
            core::ptr::copy_nonoverlapping(quads.as_ptr(), ptr.add(offset), quads.len());
        }
    }

    /// Write glyph instances into the shared buffer at the specified instance offset.
    pub fn write_glyphs(&self, offset: usize, glyphs: &[GlyphInst]) {
        if glyphs.is_empty() {
            return;
        }
        assert!(offset + glyphs.len() <= self.glyph_capacity);
        let ptr = self.glyph_buffer.contents().as_ptr() as *mut GlyphInst;
        // SAFETY: Target buffer is valid shared memory allocated with sufficient capacity.
        unsafe {
            core::ptr::copy_nonoverlapping(glyphs.as_ptr(), ptr.add(offset), glyphs.len());
        }
    }

    /// Write uniforms data into the shared uniforms buffer.
    pub fn write_uniforms(&self, uniforms: &Uniforms) {
        let ptr = self.uniforms_buffer.contents().as_ptr() as *mut Uniforms;
        // SAFETY: Target uniforms buffer is exactly sized to std::mem::size_of::<Uniforms>().
        unsafe {
            core::ptr::copy_nonoverlapping(uniforms as *const Uniforms, ptr, 1);
        }
    }

    /// Shrink buffers to minimum capacity.
    pub fn shrink_to_minimum(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
    ) -> Result<(), RenderError> {
        if self.quad_capacity > MIN_QUAD_CAPACITY {
            let bytes = MIN_QUAD_CAPACITY * std::mem::size_of::<QuadInst>();
            self.quad_buffer = device
                .newBufferWithLength_options(
                    bytes,
                    MTLResourceOptions::MTLResourceStorageModeShared,
                )
                .ok_or(RenderError::BufferAllocationFailed)?;
            self.quad_capacity = MIN_QUAD_CAPACITY;
        }
        if self.glyph_capacity > MIN_GLYPH_CAPACITY {
            let bytes = MIN_GLYPH_CAPACITY * std::mem::size_of::<GlyphInst>();
            self.glyph_buffer = device
                .newBufferWithLength_options(
                    bytes,
                    MTLResourceOptions::MTLResourceStorageModeShared,
                )
                .ok_or(RenderError::BufferAllocationFailed)?;
            self.glyph_capacity = MIN_GLYPH_CAPACITY;
        }
        Ok(())
    }
}

/// Ring of 2 instance buffers synchronized with a 2-frame dispatch semaphore.
///
/// See `docs/specs/text-and-rendering.md §3.1`.
pub struct InstanceRing {
    pub slots: [InstanceSlot; 2],
    pub current_index: usize,
    pub semaphore: Arc<FrameSemaphore>,
    pub low_usage_count: usize,
}

impl InstanceRing {
    pub fn new(device: &ProtocolObject<dyn MTLDevice>) -> Result<Self, RenderError> {
        let slot0 = InstanceSlot::new(device)?;
        let slot1 = InstanceSlot::new(device)?;
        Ok(Self {
            slots: [slot0, slot1],
            current_index: 0,
            semaphore: Arc::new(FrameSemaphore::new(2)),
            low_usage_count: 0,
        })
    }

    /// Advance to next slot in the ring (0 -> 1 -> 0).
    pub fn next_slot(&mut self) -> usize {
        let slot = self.current_index;
        self.current_index = (self.current_index + 1) % 2;
        slot
    }

    /// Check usage against 25% threshold for 300 frames, shrinking buffers when sustained.
    ///
    /// See `docs/specs/text-and-rendering.md §3.1`.
    pub fn update_usage_and_shrink(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        slot_idx: usize,
        quads_used: usize,
        glyphs_used: usize,
    ) -> Result<(), RenderError> {
        let slot = &self.slots[slot_idx];
        let low_quad = quads_used < (slot.quad_capacity / 4);
        let low_glyph = glyphs_used < (slot.glyph_capacity / 4);

        if low_quad && low_glyph {
            self.low_usage_count += 1;
            if self.low_usage_count >= LOW_USAGE_THRESHOLD_FRAMES {
                self.low_usage_count = 0;
                for s in &mut self.slots {
                    if s.quad_capacity > MIN_QUAD_CAPACITY {
                        let new_cap = (s.quad_capacity / 2).max(MIN_QUAD_CAPACITY);
                        let bytes = new_cap * std::mem::size_of::<QuadInst>();
                        s.quad_buffer = device
                            .newBufferWithLength_options(
                                bytes,
                                MTLResourceOptions::MTLResourceStorageModeShared,
                            )
                            .ok_or(RenderError::BufferAllocationFailed)?;
                        s.quad_capacity = new_cap;
                    }
                    if s.glyph_capacity > MIN_GLYPH_CAPACITY {
                        let new_cap = (s.glyph_capacity / 2).max(MIN_GLYPH_CAPACITY);
                        let bytes = new_cap * std::mem::size_of::<GlyphInst>();
                        s.glyph_buffer = device
                            .newBufferWithLength_options(
                                bytes,
                                MTLResourceOptions::MTLResourceStorageModeShared,
                            )
                            .ok_or(RenderError::BufferAllocationFailed)?;
                        s.glyph_capacity = new_cap;
                    }
                }
            }
        } else {
            self.low_usage_count = 0;
        }
        Ok(())
    }

    /// Shrink all ring buffer slots back to initial minimum capacity when idle.
    pub fn release_idle_resources(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
    ) -> Result<(), RenderError> {
        self.low_usage_count = 0;
        for s in &mut self.slots {
            s.shrink_to_minimum(device)?;
        }
        Ok(())
    }
}
