//! Display link abstraction supporting macOS 14+ CADisplayLink and macOS 13 CVDisplayLink.
//!
//! See `docs/specs/platform-macos.md §5`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use termin_core::WindowId;

use objc2::rc::Retained;
use objc2::{declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass};
use objc2_app_kit::NSView;
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSProcessInfo, NSRunLoop, NSRunLoopCommonModes,
};
use objc2_quartz_core::CADisplayLink;

/// Common trait for active display links.
pub trait DisplayLinkTrait: Send {
    /// Pause or unpause the display link.
    fn set_paused(&self, paused: bool);

    /// Update target display when window moves between screens.
    fn update_display(&self, display_id: u32);
}

// ----------------------------------------------------------------------------
// macOS 14+ CADisplayLink via NSView.displayLink(target:selector:)
// ----------------------------------------------------------------------------

declare_class!(
    /// Target object for CADisplayLink notifications.
    pub struct DisplayLinkTarget;

    unsafe impl ClassType for DisplayLinkTarget {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TerminDisplayLinkTarget";
    }

    impl DeclaredClass for DisplayLinkTarget {
        type Ivars = std::cell::Cell<WindowId>;
    }

    unsafe impl NSObjectProtocol for DisplayLinkTarget {}

    unsafe impl DisplayLinkTarget {
        #[method(onDisplayLink:)]
        unsafe fn on_display_link(&self, _link: &CADisplayLink) {
            let win_id = self.ivars().get();
            crate::macos::app::on_display_link_fire(win_id);
        }
    }
);

impl DisplayLinkTarget {
    pub fn new(win_id: WindowId, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(std::cell::Cell::new(win_id));
        // SAFETY: Super init on NSObject.
        unsafe { msg_send_id![super(this), init] }
    }
}

pub struct CaDisplayLinkWrapper {
    link: Retained<CADisplayLink>,
    _target: Retained<DisplayLinkTarget>,
}

// SAFETY: Controlled thread safety; manipulation occurs on the main run loop.
unsafe impl Send for CaDisplayLinkWrapper {}

impl DisplayLinkTrait for CaDisplayLinkWrapper {
    fn set_paused(&self, paused: bool) {
        // SAFETY: CADisplayLink setPaused on main thread.
        unsafe {
            self.link.setPaused(paused);
        }
    }

    fn update_display(&self, _display_id: u32) {
        // CADisplayLink created via NSView auto-tracks the display.
    }
}

impl Drop for CaDisplayLinkWrapper {
    fn drop(&mut self) {
        // SAFETY: Invalidate CADisplayLink on drop.
        unsafe {
            self.link.invalidate();
        }
    }
}

// ----------------------------------------------------------------------------
// macOS 13 CVDisplayLink fallback via CoreVideo
// ----------------------------------------------------------------------------

#[repr(C)]
pub struct CVTimeStamp {
    pub version: u32,
    pub video_time_scale: i32,
    pub video_time: i64,
    pub host_time: u64,
    pub rate_scalar: f64,
    pub video_refresh_period: i64,
    pub smpte_time: [u8; 16],
    pub flags: u64,
    pub reserved: u64,
}

pub type CVDisplayLinkOutputCallback = unsafe extern "C" fn(
    display_link: *mut std::ffi::c_void,
    in_now: *const CVTimeStamp,
    in_output_time: *const CVTimeStamp,
    flags_in: u64,
    flags_out: *mut u64,
    display_link_context: *mut std::ffi::c_void,
) -> i32;

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    fn CVDisplayLinkCreateWithActiveCGDisplays(display_link_out: *mut *mut std::ffi::c_void)
        -> i32;
    fn CVDisplayLinkSetCurrentCGDisplay(
        display_link: *mut std::ffi::c_void,
        display_id: u32,
    ) -> i32;
    fn CVDisplayLinkSetOutputCallback(
        display_link: *mut std::ffi::c_void,
        callback: CVDisplayLinkOutputCallback,
        user_info: *mut std::ffi::c_void,
    ) -> i32;
    fn CVDisplayLinkStart(display_link: *mut std::ffi::c_void) -> i32;
    fn CVDisplayLinkStop(display_link: *mut std::ffi::c_void) -> i32;
    fn CVDisplayLinkRelease(display_link: *mut std::ffi::c_void);
}

extern "C" {
    fn dispatch_async_f(
        queue: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: unsafe extern "C" fn(*mut std::ffi::c_void),
    );
    static _dispatch_main_q: std::ffi::c_void;
}

#[inline]
fn main_dispatch_queue() -> *mut std::ffi::c_void {
    std::ptr::addr_of!(_dispatch_main_q) as *mut std::ffi::c_void
}

unsafe extern "C" fn cv_main_dispatch_callback(context: *mut std::ffi::c_void) {
    let win_id = WindowId(context as usize as u32);
    crate::macos::app::on_display_link_fire(win_id);
}

unsafe extern "C" fn cv_display_link_callback(
    _display_link: *mut std::ffi::c_void,
    _in_now: *const CVTimeStamp,
    _in_output_time: *const CVTimeStamp,
    _flags_in: u64,
    _flags_out: *mut u64,
    context: *mut std::ffi::c_void,
) -> i32 {
    let win_id = WindowId(context as usize as u32);
    if crate::macos::app::should_cv_dispatch(win_id) {
        dispatch_async_f(main_dispatch_queue(), context, cv_main_dispatch_callback);
    }
    0
}

pub struct CvDisplayLinkWrapper {
    link: *mut std::ffi::c_void,
    running: Arc<AtomicBool>,
}

// SAFETY: CoreVideo link reference coordinates start/stop across threads safely.
unsafe impl Send for CvDisplayLinkWrapper {}

impl DisplayLinkTrait for CvDisplayLinkWrapper {
    fn set_paused(&self, paused: bool) {
        // SAFETY: CoreVideo start and stop calls.
        unsafe {
            if paused {
                if self.running.swap(false, Ordering::SeqCst) {
                    CVDisplayLinkStop(self.link);
                }
            } else if !self.running.swap(true, Ordering::SeqCst) {
                CVDisplayLinkStart(self.link);
            }
        }
    }

    fn update_display(&self, display_id: u32) {
        // SAFETY: Updating CGDirectDisplay ID on CVDisplayLink.
        unsafe {
            CVDisplayLinkSetCurrentCGDisplay(self.link, display_id);
        }
    }
}

impl Drop for CvDisplayLinkWrapper {
    fn drop(&mut self) {
        // SAFETY: Stop and release CoreVideo display link.
        unsafe {
            CVDisplayLinkStop(self.link);
            CVDisplayLinkRelease(self.link);
        }
    }
}

/// Factory function to create the best display link for the current OS version.
pub fn create_display_link(
    view: &NSView,
    win_id: WindowId,
    mtm: MainThreadMarker,
) -> Option<Box<dyn DisplayLinkTrait>> {
    // Check for macOS 14+ CADisplayLink via NSView
    let os_version = NSProcessInfo::processInfo().operatingSystemVersion();
    if os_version.majorVersion >= 14 {
        let sel = sel!(displayLinkWithTarget:selector:);
        let responds = view.respondsToSelector(sel);
        if responds {
            let target = DisplayLinkTarget::new(win_id, mtm);
            // SAFETY: Calling displayLinkWithTarget:selector: on NSView.
            let link: Option<Retained<CADisplayLink>> = unsafe {
                msg_send_id![view, displayLinkWithTarget: &*target, selector: sel!(onDisplayLink:)]
            };
            if let Some(link) = link {
                // SAFETY: Adding CADisplayLink to NSRunLoop in common modes and starting paused.
                unsafe {
                    link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes);
                    link.setPaused(true);
                }
                return Some(Box::new(CaDisplayLinkWrapper {
                    link,
                    _target: target,
                }));
            }
        }
    }

    // Fallback: macOS 13 CVDisplayLink
    let mut link: *mut std::ffi::c_void = std::ptr::null_mut();
    // SAFETY: Initializing CVDisplayLink with active displays.
    let status = unsafe { CVDisplayLinkCreateWithActiveCGDisplays(&mut link) };
    if status != 0 || link.is_null() {
        log::warn!("Failed to create CVDisplayLink: status {}", status);
        return None;
    }

    let ctx = win_id.0 as usize as *mut std::ffi::c_void;
    // SAFETY: Setting output callback for CVDisplayLink.
    let cb_status = unsafe { CVDisplayLinkSetOutputCallback(link, cv_display_link_callback, ctx) };
    if cb_status != 0 {
        log::warn!("Failed to set CVDisplayLink callback: status {}", cb_status);
        // SAFETY: Releasing allocated link on callback failure.
        unsafe { CVDisplayLinkRelease(link) };
        return None;
    }

    Some(Box::new(CvDisplayLinkWrapper {
        link,
        running: Arc::new(AtomicBool::new(false)),
    }))
}
