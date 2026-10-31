//! Metal device and queue lifecycle management.
//!
//! See `docs/specs/text-and-rendering.md §3.1`.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLCreateSystemDefaultDevice, MTLDevice, MTLPixelFormat};
use objc2_quartz_core::CAMetalLayer;

use super::RenderError;

/// Create a Metal device, preferring a low-power GPU if available (e.g. integrated GPU on dual-GPU Macs).
///
/// See `docs/specs/text-and-rendering.md §3.1`.
pub fn create_device() -> Result<Retained<ProtocolObject<dyn MTLDevice>>, RenderError> {
    // Prefer low-power device if available (dual-GPU Macs: terminal doesn't need dGPU)
    // SAFETY: MTLCopyAllDevices returns a retained array of MTLDevice pointers according to Apple docs.
    let all_raw = unsafe { objc2_metal::MTLCopyAllDevices() };
    if let Some(devices) = unsafe { Retained::from_raw(all_raw.as_ptr()) } {
        for dev in devices.iter() {
            if dev.isLowPower() {
                // SAFETY: Retaining valid device reference from MTLCopyAllDevices array.
                if let Some(retained_dev) = unsafe { Retained::retain(dev as *const _ as *mut _) } {
                    return Ok(retained_dev);
                }
            }
        }
    }

    // Fall back to system default device
    // SAFETY: MTLCreateSystemDefaultDevice creates the default Metal device with +1 retain count.
    let dev_ptr = unsafe { MTLCreateSystemDefaultDevice() };
    if let Some(dev) = unsafe { Retained::from_raw(dev_ptr) } {
        Ok(dev)
    } else {
        Err(RenderError::NoMetalDevice)
    }
}

/// Configure CAMetalLayer properties for optimal terminal rendering.
///
/// See `docs/specs/text-and-rendering.md §3.1`.
pub fn configure_layer(layer: &CAMetalLayer, device: &ProtocolObject<dyn MTLDevice>) {
    // SAFETY: Setting standard CAMetalLayer properties on a valid layer instance.
    unsafe {
        layer.setDevice(Some(device));
        layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm_sRGB);
        layer.setFramebufferOnly(true);
        layer.setMaximumDrawableCount(2);
        layer.setDisplaySyncEnabled(true);
        layer.setPresentsWithTransaction(false);
    }
}
