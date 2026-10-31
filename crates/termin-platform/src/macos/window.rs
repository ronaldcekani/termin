//! Window management, styling, delegate, and render target.
//!
//! See `docs/specs/platform-macos.md §2`.

use std::cell::Cell;

use crate::macos::display_link::{create_display_link, DisplayLinkTrait};
use crate::macos::view::TerminView;
use crate::{PlatformEvent, RenderTarget, WindowOpts};
use termin_core::{Size, WindowId};

use objc2::rc::Retained;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBackingStoreType, NSColor,
    NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowCollectionBehavior, NSWindowDelegate, NSWindowOcclusionState, NSWindowStyleMask,
    NSWindowTitleVisibility,
};
use objc2_foundation::{
    CGFloat, MainThreadMarker, NSNotification, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use objc2_quartz_core::CAMetalLayer;

declare_class!(
    /// Delegate handling NSWindow lifecycle and focus events.
    pub struct WindowDelegate;

    unsafe impl ClassType for WindowDelegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TerminWindowDelegate";
    }

    impl DeclaredClass for WindowDelegate {
        type Ivars = Cell<WindowId>;
    }

    unsafe impl NSObjectProtocol for WindowDelegate {}

    unsafe impl NSWindowDelegate for WindowDelegate {
        #[method(windowShouldClose:)]
        unsafe fn window_should_close(&self, _sender: &NSWindow) -> bool {
            let win_id = self.ivars().get();
            crate::macos::app::dispatch_event(win_id, PlatformEvent::CloseRequested);
            // Window closure is controlled explicitly via PlatformCx::close_window
            false
        }

        #[method(windowDidBecomeKey:)]
        unsafe fn window_did_become_key(&self, _notification: &NSNotification) {
            let win_id = self.ivars().get();
            crate::macos::app::set_active_window(win_id);
            crate::macos::app::dispatch_event(win_id, PlatformEvent::Focus(true));
        }

        #[method(windowDidResignKey:)]
        unsafe fn window_did_resign_key(&self, _notification: &NSNotification) {
            let win_id = self.ivars().get();
            crate::macos::app::dispatch_event(win_id, PlatformEvent::Focus(false));
        }

        #[method(windowDidChangeOcclusionState:)]
        unsafe fn window_did_change_occlusion_state(&self, notification: &NSNotification) {
            let win_id = self.ivars().get();
            if let Some(obj) = notification.object() {
                let is_win: bool = objc2::msg_send![&*obj, isKindOfClass: NSWindow::class()];
                if is_win {
                    let win: &NSWindow = &*(&*obj as *const objc2::runtime::AnyObject as *const NSWindow);
                    let state = win.occlusionState();
                    let occluded = !state.contains(NSWindowOcclusionState::Visible);
                    crate::macos::app::set_window_occluded(win_id, occluded);
                    crate::macos::app::dispatch_event(win_id, PlatformEvent::Occluded(occluded));
                }
            }
        }
    }
);

impl WindowDelegate {
    pub fn new(win_id: WindowId, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(Cell::new(win_id));
        // SAFETY: Calling super init on NSObject.
        unsafe { msg_send_id![super(this), init] }
    }
}

/// Active state and handles associated with a single OS window.
pub struct WindowState {
    pub id: WindowId,
    pub window: Retained<NSWindow>,
    pub view: Retained<TerminView>,
    pub metal_layer: Retained<CAMetalLayer>,
    pub display_link: Option<Box<dyn DisplayLinkTrait>>,
    pub delegate: Retained<WindowDelegate>,
    pub frame_requested: bool,
    pub frame_pending: bool,
    pub idle_frames: u32,
    pub scale_factor: f32,
    pub size: Size,
    pub is_occluded: bool,
}

/// Render target implementation providing drawable size and CAMetalLayer access.
pub struct MetalRenderTarget {
    pub size: Size,
    pub scale_factor: f32,
    pub metal_layer: Retained<CAMetalLayer>,
}

impl RenderTarget for MetalRenderTarget {
    fn size(&self) -> Size {
        self.size
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    fn metal_layer(&self) -> *mut std::ffi::c_void {
        Retained::as_ptr(&self.metal_layer) as *mut std::ffi::c_void
    }
}

/// Create a new native macOS window styled per Termin specifications.
pub fn create_native_window(
    win_id: WindowId,
    opts: &WindowOpts,
    mtm: MainThreadMarker,
) -> WindowState {
    let (origin_x, origin_y, width, height) = if let Some(frame) = opts.frame {
        (
            frame.x as CGFloat,
            frame.y as CGFloat,
            frame.w as CGFloat,
            frame.h as CGFloat,
        )
    } else {
        (100.0, 100.0, 800.0, 600.0)
    };

    let content_rect = NSRect::new(
        NSPoint::new(origin_x, origin_y),
        NSSize::new(width, height),
    );

    let style_mask = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::FullSizeContentView;

    // SAFETY: Creating NSWindow on main thread with specified style mask.
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSWindow>(),
            content_rect,
            style_mask,
            NSBackingStoreType::NSBackingStoreBuffered,
            false,
        )
    };

    // Style window per docs/specs/platform-macos.md §2
    unsafe {
        window.setTitlebarAppearsTransparent(true);
        window.setTitleVisibility(NSWindowTitleVisibility::NSWindowTitleHidden);
        window.setAcceptsMouseMovedEvents(true);

        // Native full screen support, disallow native AppKit tabs
        window.setCollectionBehavior(NSWindowCollectionBehavior::FullScreenPrimary);

        let sel_disallowed = objc2::sel!(setTabbingMode:);
        if window.respondsToSelector(sel_disallowed) {
            let () = objc2::msg_send![&window, setTabbingMode: 2isize]; // NSWindowTabbingModeDisallowed = 2
        }

        if let Some(title) = &opts.title {
            window.setTitle(&NSString::from_str(title));
        }
    }

    // Initialize content view (TerminView)
    let view_frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
    let view = TerminView::new(view_frame, win_id, mtm);

    // Optional blur effect view behind the Metal layer
    let blur_requested = opts.blur.unwrap_or(false);
    if blur_requested {
        unsafe {
            let effect_view = NSVisualEffectView::new(mtm);
            effect_view.setMaterial(NSVisualEffectMaterial::UnderWindowBackground);
            effect_view.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            effect_view.setState(NSVisualEffectState::Active);
            effect_view.setAutoresizingMask(
                NSAutoresizingMaskOptions::NSViewWidthSizable
                    | NSAutoresizingMaskOptions::NSViewHeightSizable,
            );
            effect_view.setFrame(view_frame);
            window.contentView().unwrap().addSubview(&effect_view);
        }
    }

    // Opacity handling
    let opacity = opts.opacity.unwrap_or(1.0);
    unsafe {
        if opacity < 1.0 {
            window.setOpaque(false);
            window.setBackgroundColor(Some(&NSColor::clearColor()));
        } else {
            window.setOpaque(true);
        }
    }

    // Attach TerminView as content view
    window.setContentView(Some(&view));

    // Window delegate
    let delegate = WindowDelegate::new(win_id, mtm);
    window.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)));

    // Retrieve CAMetalLayer backing
    let metal_layer: Retained<CAMetalLayer> = unsafe {
        let layer = view.layer().expect("TerminView missing backing layer");
        // SAFETY: TerminView's makeBackingLayer returns a CAMetalLayer.
        Retained::cast(layer)
    };

    let scale = window.backingScaleFactor() as f32;
    let size = Size {
        w: width as f32,
        h: height as f32,
    };

    // Display link
    let display_link = create_display_link(&view, win_id, mtm);

    // Present window
    window.makeKeyAndOrderFront(None);

    WindowState {
        id: win_id,
        window,
        view,
        metal_layer,
        display_link,
        delegate,
        frame_requested: false,
        frame_pending: false,
        idle_frames: 0,
        scale_factor: scale,
        size,
        is_occluded: false,
    }
}
