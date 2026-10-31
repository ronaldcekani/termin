//! Platform abstraction layer (`termin-platform`).
//!
//! Provides the `Platform`, `AppHandler`, and `PlatformCx` contracts for macOS (and future platforms).
//! See `docs/architecture.md §3.4` and `docs/specs/platform-macos.md`.

use std::ops::Range;
use std::path::{Path, PathBuf};
use termin_core::{
    KeyEvent, MouseEvent, PaneId, Rect, ScrollEvent, Size, SmallStr, TabId, WakeHandle, WindowId,
    WorkspaceId,
};

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub use macos::input::OptionAsAlt;

#[cfg(not(target_os = "macos"))]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum OptionAsAlt {
    #[default]
    Left,
    Right,
    Both,
    None,
}

#[cfg(target_os = "macos")]
pub use macos::MacPlatform;
#[cfg(target_os = "macos")]
pub use macos::MacPlatform as DefaultPlatform;

/// Numeric identifier for application menu and keyboard actions.
///
/// See `docs/architecture.md §3.4`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ActionId(pub u32);

/// Mouse cursor appearance.
///
/// See `docs/specs/platform-macos.md §3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CursorIcon {
    Arrow,
    IBeam,
    PointingHand,
    ResizeLeftRight,
    ResizeUpDown,
}

/// System color appearance mode (Light or Dark).
///
/// See `docs/specs/platform-macos.md §9`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Appearance {
    Light,
    Dark,
}

/// Options for configuring a new window.
///
/// See `docs/specs/platform-macos.md §2`.
#[derive(Clone, Debug, Default)]
pub struct WindowOpts {
    pub frame: Option<Rect>,
    pub restore_id: Option<u64>,
    pub title: Option<String>,
    pub opacity: Option<f32>,
    pub blur: Option<bool>,
}

/// Trait representing an active render target (e.g. `CAMetalLayer` drawable).
///
/// See `docs/architecture.md §3.4`.
pub trait RenderTarget {
    fn size(&self) -> Size;
    fn scale_factor(&self) -> f32;

    /// Direct pointer to underlying CAMetalLayer (macOS only).
    fn metal_layer(&self) -> *mut std::ffi::c_void {
        std::ptr::null_mut()
    }
}

/// Specification for a native system menu item.
///
/// See `docs/specs/platform-macos.md §11`.
#[derive(Clone, Debug)]
pub struct MenuItemSpec {
    pub title: String,
    pub action: Option<ActionId>,
    pub key_equivalent: Option<String>,
    pub submenu: Option<MenuSpec>,
}

/// Specification for a native system menu or submenu.
///
/// See `docs/specs/platform-macos.md §11`.
#[derive(Clone, Debug)]
pub struct MenuSpec {
    pub title: String,
    pub items: Vec<MenuItemSpec>,
}

/// Specification for a system notification.
///
/// See `docs/specs/platform-macos.md §8`.
#[derive(Clone, Debug)]
pub struct SystemNotification {
    pub title: String,
    pub body: String,
    pub window: WindowId,
    pub workspace: WorkspaceId,
    pub pane: PaneId,
    pub tab: TabId,
}

/// Window and input events dispatched to `AppHandler`.
///
/// See `docs/architecture.md §3.4` and `docs/specs/platform-macos.md §3, §4`.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum PlatformEvent {
    Key(KeyEvent),
    ImeCompose {
        text: SmallStr,
        cursor: Option<Range<usize>>,
    },
    ImeCommit(String),
    Mouse(MouseEvent),
    Scroll(ScrollEvent),
    Resized {
        size: Size,
        scale: f32,
    },
    Focus(bool),
    Occluded(bool),
    CloseRequested,
    FileDrop(Vec<PathBuf>),
    Appearance(Appearance),
}

/// Application context on the main thread for controlling windows, clipboard, cursor.
///
/// See `docs/architecture.md §3.4`.
pub struct PlatformCx {
    wake: WakeHandle,
}

impl PlatformCx {
    /// Create a new platform context with a main-thread wake handle.
    pub fn new(wake: WakeHandle) -> Self {
        Self { wake }
    }

    /// Create a new OS window with options.
    pub fn create_window(&mut self, opts: &WindowOpts) -> WindowId {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_create_window(opts)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = opts;
            WindowId::next()
        }
    }

    /// Close an existing OS window.
    pub fn close_window(&mut self, w: WindowId) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_close_window(w);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = w;
        }
    }

    /// Request a new display frame for window.
    pub fn request_frame(&mut self, w: WindowId) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_request_frame(w);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = w;
        }
    }

    /// Set the mouse cursor icon for window.
    pub fn set_cursor(&mut self, w: WindowId, c: CursorIcon) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_set_cursor(w, c);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (w, c);
        }
    }

    /// Set IME input composition rectangle.
    pub fn set_ime_rect(&mut self, w: WindowId, r: Rect) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_set_ime_rect(w, r);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (w, r);
        }
    }

    /// Set window drag regions in view coordinates.
    pub fn set_drag_regions(&mut self, w: WindowId, regions: Vec<Rect>) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_set_drag_regions(w, regions);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (w, regions);
        }
    }

    /// Set window opacity (alpha level from 0.0 to 1.0).
    pub fn set_window_opacity(&mut self, w: WindowId, opacity: f32) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_set_window_opacity(w, opacity);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (w, opacity);
        }
    }

    /// Set accessible text for VoiceOver.
    pub fn set_accessible_text(&mut self, w: WindowId, text: &str) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_set_accessible_text(w, text);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (w, text);
        }
    }

    /// Toggle native window fullscreen.
    pub fn toggle_fullscreen(&mut self, w: WindowId) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_toggle_fullscreen(w);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = w;
        }
    }

    /// Set option-as-alt mode.
    pub fn set_option_as_alt(&mut self, mode: OptionAsAlt) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::with_state(|s| s.option_as_alt = mode);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = mode;
        }
    }

    /// Read string from system clipboard.
    pub fn clipboard_get(&self) -> Option<String> {
        #[cfg(target_os = "macos")]
        {
            crate::macos::clipboard::clipboard_get()
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    /// Write string to system clipboard.
    pub fn clipboard_set(&mut self, s: &str) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::clipboard::clipboard_set(s);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = s;
        }
    }

    /// Post a system desktop notification.
    pub fn notify(&mut self, n: &SystemNotification) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::notification::notify(n);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = n;
        }
    }

    /// Set application dock badge number (0 clears).
    pub fn set_badge(&mut self, n: u32) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::notification::set_badge(n);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = n;
        }
    }

    /// Open URL in default system handler.
    pub fn open_url(&self, url: &str) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_open_url(url);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = url;
        }
    }

    /// Reveal path in system file manager (Finder).
    pub fn reveal_in_finder(&self, p: &Path) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_reveal_in_finder(p);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = p;
        }
    }

    /// Update native system application menu.
    pub fn set_menu(&mut self, menu: &MenuSpec) {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::cx_set_menu(menu);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = menu;
        }
    }

    /// Obtain a wake handle to signal this platform context from another thread.
    pub fn wake_handle(&self) -> WakeHandle {
        self.wake.clone()
    }

    /// Current system visual appearance (Light or Dark).
    pub fn appearance(&self) -> Appearance {
        #[cfg(target_os = "macos")]
        {
            crate::macos::app::with_state(|s| s.appearance)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Appearance::Dark
        }
    }
}

/// Application callback handler running on the main run loop.
///
/// See `docs/architecture.md §3.4`.
pub trait AppHandler: 'static {
    /// Initialize application state after run loop starts.
    fn init(&mut self, cx: &mut PlatformCx);

    /// Handle platform input or window event.
    fn event(&mut self, cx: &mut PlatformCx, win: WindowId, ev: PlatformEvent);

    /// Render frame into window render target.
    fn frame(&mut self, cx: &mut PlatformCx, win: WindowId, target: &mut dyn RenderTarget);

    /// Woken by background thread (IO reactor or worker).
    fn wake(&mut self, cx: &mut PlatformCx);

    /// Native menu item selected.
    fn menu_action(&mut self, cx: &mut PlatformCx, action: ActionId);

    /// Check if application should terminate (handles close confirmation).
    fn should_terminate(&mut self, cx: &mut PlatformCx) -> bool;
}

/// Platform entry point contract.
///
/// See `docs/architecture.md §3.4`.
pub trait Platform: Sized {
    /// Run the application event loop until termination.
    fn run<H: AppHandler>(handler: H) -> !;
}

#[cfg(not(target_os = "macos"))]
pub struct StubPlatform;

#[cfg(not(target_os = "macos"))]
impl Platform for StubPlatform {
    fn run<H: AppHandler>(_handler: H) -> ! {
        panic!("Termin platform only implemented on macOS");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    struct DummyHandler;
    impl AppHandler for DummyHandler {
        fn init(&mut self, _cx: &mut PlatformCx) {}
        fn event(&mut self, _cx: &mut PlatformCx, _win: WindowId, _ev: PlatformEvent) {}
        fn frame(&mut self, _cx: &mut PlatformCx, _win: WindowId, _target: &mut dyn RenderTarget) {}
        fn wake(&mut self, _cx: &mut PlatformCx) {}
        fn menu_action(&mut self, _cx: &mut PlatformCx, _action: ActionId) {}
        fn should_terminate(&mut self, _cx: &mut PlatformCx) -> bool {
            true
        }
    }

    #[test]
    fn test_platform_cx_basics() {
        let wake = WakeHandle::new(|| {});
        let mut cx = PlatformCx::new(wake);
        let test_str = "termin_test_clipboard_value_42";
        cx.clipboard_set(test_str);
        assert_eq!(cx.clipboard_get().as_deref(), Some(test_str));
    }
}
