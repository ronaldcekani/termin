//! Run loop wake mechanism using CFRunLoopSource (version 0).
//!
//! See `docs/specs/platform-macos.md §6`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use termin_core::WakeHandle;

pub type CFTypeRef = *const std::ffi::c_void;
pub type CFRunLoopRef = *mut std::ffi::c_void;
pub type CFRunLoopSourceRef = *mut std::ffi::c_void;
pub type CFStringRef = *const std::ffi::c_void;
pub type CFAllocatorRef = *const std::ffi::c_void;

#[repr(C)]
#[allow(non_snake_case)]
pub struct CFRunLoopSourceContext {
    pub version: isize,
    pub info: *mut std::ffi::c_void,
    pub retain: Option<unsafe extern "C" fn(*const std::ffi::c_void) -> *const std::ffi::c_void>,
    pub release: Option<unsafe extern "C" fn(*const std::ffi::c_void)>,
    pub copyDescription: Option<unsafe extern "C" fn(*const std::ffi::c_void) -> CFStringRef>,
    pub equal: Option<unsafe extern "C" fn(*const std::ffi::c_void, *const std::ffi::c_void) -> u8>,
    pub hash: Option<unsafe extern "C" fn(*const std::ffi::c_void) -> usize>,
    pub schedule: Option<unsafe extern "C" fn(*mut std::ffi::c_void, CFRunLoopRef, CFStringRef)>,
    pub cancel: Option<unsafe extern "C" fn(*mut std::ffi::c_void, CFRunLoopRef, CFStringRef)>,
    pub perform: Option<unsafe extern "C" fn(*mut std::ffi::c_void)>,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub fn CFRunLoopGetMain() -> CFRunLoopRef;
    pub fn CFRunLoopSourceCreate(
        allocator: CFAllocatorRef,
        order: isize,
        context: *mut CFRunLoopSourceContext,
    ) -> CFRunLoopSourceRef;
    pub fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopSourceSignal(source: CFRunLoopSourceRef);
    pub fn CFRunLoopWakeUp(rl: CFRunLoopRef);
    pub fn CFRunLoopSourceInvalidate(source: CFRunLoopSourceRef);
    pub fn CFRelease(cf: CFTypeRef);
    pub fn CFRetain(cf: CFTypeRef) -> CFTypeRef;

    pub static kCFRunLoopCommonModes: CFStringRef;
}

/// Global callback fired on the main thread when the wake source is signaled.
unsafe extern "C" fn wake_source_perform(_info: *mut std::ffi::c_void) {
    crate::macos::app::on_wake_performed();
}

/// Owns the active CFRunLoopSourceRef attached to the main run loop.
pub struct WakeSource {
    source: CFRunLoopSourceRef,
    run_loop: CFRunLoopRef,
    invalidated: Arc<AtomicBool>,
}

// SAFETY: WakeSource coordinates between background threads and main CFRunLoop.
unsafe impl Send for WakeSource {}
unsafe impl Sync for WakeSource {}

impl WakeSource {
    /// Create and register a version 0 `CFRunLoopSource` on the main run loop in common modes.
    pub fn new() -> (WakeHandle, Self) {
        let mut context = CFRunLoopSourceContext {
            version: 0,
            info: std::ptr::null_mut(),
            retain: None,
            release: None,
            copyDescription: None,
            equal: None,
            hash: None,
            schedule: None,
            cancel: None,
            perform: Some(wake_source_perform),
        };

        // SAFETY: CoreFoundation APIs called on the main thread with properly initialized context.
        let (source, run_loop) = unsafe {
            let run_loop = CFRunLoopGetMain();
            let source = CFRunLoopSourceCreate(std::ptr::null(), 0, &mut context);
            assert!(!source.is_null(), "failed to create CFRunLoopSource");
            CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
            CFRetain(source as _);
            CFRetain(run_loop as _);
            (source, run_loop)
        };

        let invalidated = Arc::new(AtomicBool::new(false));
        let src = Self {
            source,
            run_loop,
            invalidated,
        };
        let handle = src.handle();
        (handle, src)
    }

    /// Obtain a new `WakeHandle` associated with this wake source.
    pub fn handle(&self) -> WakeHandle {
        let invalidated = Arc::clone(&self.invalidated);
        let s_addr = self.source as usize;
        let rl_addr = self.run_loop as usize;

        WakeHandle::new(move || {
            if !invalidated.load(Ordering::Relaxed) {
                // SAFETY: source and run_loop were retained for the lifetime of this source.
                unsafe {
                    let s = s_addr as CFRunLoopSourceRef;
                    let rl = rl_addr as CFRunLoopRef;
                    CFRunLoopSourceSignal(s);
                    CFRunLoopWakeUp(rl);
                }
            }
        })
    }
}

impl Drop for WakeSource {
    fn drop(&mut self) {
        self.invalidated.store(true, Ordering::SeqCst);
        // SAFETY: Releasing our retained references and invalidating the source on shutdown.
        unsafe {
            CFRunLoopSourceInvalidate(self.source);
            CFRelease(self.source as _);
            CFRelease(self.run_loop as _);
        }
    }
}
