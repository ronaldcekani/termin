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
}

/// Trait representing an active render target (e.g. `CAMetalLayer` drawable).
///
/// See `docs/architecture.md §3.4`.
pub trait RenderTarget {
    fn size(&self) -> Size;
    fn scale_factor(&self) -> f32;
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
    pub fn create_window(&mut self, _opts: &WindowOpts) -> WindowId {
        WindowId::next()
    }

    /// Close an existing OS window.
    pub fn close_window(&mut self, _w: WindowId) {}

    /// Request a new display frame for window.
    pub fn request_frame(&mut self, _w: WindowId) {}

    /// Set the mouse cursor icon for window.
    pub fn set_cursor(&mut self, _w: WindowId, _c: CursorIcon) {}

    /// Set IME input composition rectangle.
    pub fn set_ime_rect(&mut self, _w: WindowId, _r: Rect) {}

    /// Read string from system clipboard.
    pub fn clipboard_get(&self) -> Option<String> {
        None
    }

    /// Write string to system clipboard.
    pub fn clipboard_set(&mut self, _s: &str) {}

    /// Post a system desktop notification.
    pub fn notify(&mut self, _n: &SystemNotification) {}

    /// Set application dock badge number (0 clears).
    pub fn set_badge(&mut self, _n: u32) {}

    /// Open URL in default system handler.
    pub fn open_url(&self, _url: &str) {}

    /// Reveal path in system file manager (Finder).
    pub fn reveal_in_finder(&self, _p: &Path) {}

    /// Update native system application menu.
    pub fn set_menu(&mut self, _menu: &MenuSpec) {}

    /// Obtain a wake handle to signal this platform context from another thread.
    pub fn wake_handle(&self) -> WakeHandle {
        self.wake.clone()
    }

    /// Current system visual appearance (Light or Dark).
    pub fn appearance(&self) -> Appearance {
        Appearance::Dark
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
    fn test_platform_cx_window_creation() {
        let wake = WakeHandle::new(|| {});
        let mut cx = PlatformCx::new(wake);
        let win = cx.create_window(&WindowOpts::default());
        assert!(win.0 > 0);
    }
}
