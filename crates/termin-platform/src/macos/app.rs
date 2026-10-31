//! Application lifecycle, NSApplication setup, AppDelegate, and event coordination.
//!
//! See `docs/specs/platform-macos.md §1`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::macos::input::OptionAsAlt;
use crate::macos::menu::build_native_menu;
use crate::macos::wake::WakeSource;
use crate::macos::window::{create_native_window, MetalRenderTarget, WindowState};
use crate::{
    ActionId, Appearance, AppHandler, CursorIcon, MenuSpec, PlatformCx,
    PlatformEvent, WindowOpts,
};
use termin_core::{Rect, WakeHandle, WindowId};

use objc2::rc::Retained;
use objc2::{
    declare_class, msg_send_id, mutability, ClassType, DeclaredClass,
};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate,
    NSApplicationTerminateReply, NSMenu, NSMenuItem, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSNotification, NSObject, NSObjectProtocol, NSString, NSURL,
};
use rustc_hash::FxHashMap;

declare_class!(
    /// Custom AppDelegate forwarding events to AppHandler.
    pub struct AppDelegate;

    unsafe impl ClassType for AppDelegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TerminAppDelegate";
    }

    impl DeclaredClass for AppDelegate {}

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[method(applicationDidFinishLaunching:)]
        unsafe fn application_did_finish_launching(&self, _notification: &NSNotification) {
            on_application_did_finish_launching();
        }

        #[method(applicationShouldTerminate:)]
        unsafe fn application_should_terminate(&self, _sender: &NSApplication) -> NSApplicationTerminateReply {
            if on_application_should_terminate() {
                NSApplicationTerminateReply::NSTerminateNow
            } else {
                NSApplicationTerminateReply::NSTerminateCancel
            }
        }

        #[method(applicationShouldHandleReopen:hasVisibleWindows:)]
        unsafe fn application_should_handle_reopen(&self, _sender: &NSApplication, has_visible_windows: bool) -> bool {
            on_application_reopen(has_visible_windows);
            true
        }

        #[method(application:openURLs:)]
        unsafe fn application_open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            let mut paths = Vec::new();
            for i in 0..urls.count() {
                let url = urls.objectAtIndex(i);
                if let Some(path) = url.path() {
                    paths.push(PathBuf::from(path.to_string()));
                }
            }
            if !paths.is_empty() {
                on_application_open_files(paths);
            }
        }

        #[method(application:openFiles:)]
        unsafe fn application_open_files(&self, _sender: &NSApplication, filenames: &NSArray<NSString>) {
            let mut paths = Vec::new();
            for i in 0..filenames.count() {
                let fname = filenames.objectAtIndex(i);
                paths.push(PathBuf::from(fname.to_string()));
            }
            if !paths.is_empty() {
                on_application_open_files(paths);
            }
        }
    }

    unsafe impl AppDelegate {
        #[method(menuAction:)]
        unsafe fn menu_action(&self, sender: &NSMenuItem) {
            let tag = sender.tag() as u32;
            on_menu_action(tag);
        }
    }
);

impl AppDelegate {
    pub fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(());
        // SAFETY: Super init on NSObject.
        unsafe { msg_send_id![super(this), init] }
    }
}

type WindowAtomicFlags = (Arc<AtomicBool>, Arc<AtomicBool>);
type WindowFlagsMap = FxHashMap<WindowId, WindowAtomicFlags>;

/// Global thread-safe flags for frame requests and dispatch coalescing.
static WINDOW_FLAGS: Mutex<Option<WindowFlagsMap>> = Mutex::new(None);

/// Register atomic flags for a window.
fn register_window_flags(win_id: WindowId, req: Arc<AtomicBool>, pending: Arc<AtomicBool>) {
    let mut lock = WINDOW_FLAGS.lock().unwrap();
    if lock.is_none() {
        *lock = Some(FxHashMap::default());
    }
    if let Some(map) = lock.as_mut() {
        map.insert(win_id, (req, pending));
    }
}

/// Remove atomic flags when window closes.
fn unregister_window_flags(win_id: WindowId) {
    let mut lock = WINDOW_FLAGS.lock().unwrap();
    if let Some(map) = lock.as_mut() {
        map.remove(&win_id);
    }
}

/// Check if CVDisplayLink background callback should dispatch to main.
pub fn should_cv_dispatch(win_id: WindowId) -> bool {
    let lock = WINDOW_FLAGS.lock().unwrap();
    if let Some(map) = lock.as_ref() {
        if let Some((req, pending)) = map.get(&win_id) {
            if req.load(Ordering::Acquire) && !pending.swap(true, Ordering::AcqRel) {
                return true;
            }
        }
    }
    false
}

/// Application state stored on the main thread run loop.
pub struct AppState {
    pub handler: Box<dyn AppHandler>,
    pub windows: FxHashMap<WindowId, WindowState>,
    pub window_flags: WindowFlagsMap,
    pub active_window: Option<WindowId>,
    pub option_as_alt: OptionAsAlt,
    pub menu_actions: FxHashMap<u32, ActionId>,
    pub context_menus: FxHashMap<WindowId, Retained<NSMenu>>,
    pub wake_source: Option<WakeSource>,
    pub wake_handle: WakeHandle,
    pub appearance: Appearance,
    pub mtm: MainThreadMarker,
}

thread_local! {
    pub static STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

/// Run a closure with mutable access to `AppState`.
pub fn with_state<R>(f: impl FnOnce(&mut AppState) -> R) -> R {
    STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let state = borrow.as_mut().expect("Termin AppState not initialized");
        f(state)
    })
}

/// Run a closure with mutable access to `AppState` and a fresh `PlatformCx`.
pub fn with_app<R>(f: impl FnOnce(&mut dyn AppHandler, &mut PlatformCx, &mut AppState) -> R) -> R {
    STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let state = borrow.as_mut().expect("Termin AppState not initialized");
        let wake = state.wake_handle.clone();
        let mut cx = PlatformCx::new(wake);
        let mut handler = std::mem::replace(&mut state.handler, Box::new(DummyHandler));
        let res = f(handler.as_mut(), &mut cx, state);
        state.handler = handler;
        res
    })
}

struct DummyHandler;
impl AppHandler for DummyHandler {
    fn init(&mut self, _cx: &mut PlatformCx) {}
    fn event(&mut self, _cx: &mut PlatformCx, _win: WindowId, _ev: PlatformEvent) {}
    fn frame(&mut self, _cx: &mut PlatformCx, _win: WindowId, _target: &mut dyn crate::RenderTarget) {}
    fn wake(&mut self, _cx: &mut PlatformCx) {}
    fn menu_action(&mut self, _cx: &mut PlatformCx, _action: ActionId) {}
    fn should_terminate(&mut self, _cx: &mut PlatformCx) -> bool {
        true
    }
}

pub fn get_option_as_alt() -> OptionAsAlt {
    STATE.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| s.option_as_alt)
            .unwrap_or(OptionAsAlt::Left)
    })
}

pub fn set_active_window(win_id: WindowId) {
    with_state(|state| {
        state.active_window = Some(win_id);
    });
}

pub fn set_window_occluded(win_id: WindowId, occluded: bool) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get_mut(&win_id) {
            win_state.is_occluded = occluded;
        }
    });
}

pub fn get_context_menu(win_id: WindowId) -> Option<Retained<NSMenu>> {
    with_state(|state| state.context_menus.get(&win_id).cloned())
}

pub fn dispatch_event(win_id: WindowId, ev: PlatformEvent) {
    with_app(|handler, cx, _state| {
        handler.event(cx, win_id, ev);
    });
}

pub fn dispatch_sync_render(win_id: WindowId) {
    with_app(|handler, cx, state| {
        if let Some(win_state) = state.windows.get_mut(&win_id) {
            // SAFETY: Presents with transaction during live resize.
            unsafe {
                win_state.metal_layer.setPresentsWithTransaction(true);
            }
            let mut target = MetalRenderTarget {
                size: win_state.size,
                scale_factor: win_state.scale_factor,
                metal_layer: win_state.metal_layer.clone(),
            };
            handler.frame(cx, win_id, &mut target);
            // SAFETY: Reset presentsWithTransaction after frame.
            unsafe {
                win_state.metal_layer.setPresentsWithTransaction(false);
            }
        }
    });
}

pub fn on_display_link_fire(win_id: WindowId) {
    with_app(|handler, cx, state| {
        if let Some(win_state) = state.windows.get_mut(&win_id) {
            if let Some((req_flag, pending_flag)) = state.window_flags.get(&win_id) {
                pending_flag.store(false, Ordering::Release);
                let requested = req_flag.swap(false, Ordering::AcqRel);

                if requested && !win_state.is_occluded {
                    win_state.idle_frames = 0;
                    let mut target = MetalRenderTarget {
                        size: win_state.size,
                        scale_factor: win_state.scale_factor,
                        metal_layer: win_state.metal_layer.clone(),
                    };
                    handler.frame(cx, win_id, &mut target);
                } else {
                    win_state.idle_frames += 1;
                    if win_state.idle_frames >= 2 {
                        if let Some(link) = &win_state.display_link {
                            link.set_paused(true);
                        }
                    }
                }
            }
        }
    });
}

pub fn on_wake_performed() {
    with_app(|handler, cx, _state| {
        handler.wake(cx);
    });
}

pub fn on_menu_action(tag: u32) {
    with_app(|handler, cx, state| {
        if let Some(action_id) = state.menu_actions.get(&tag).copied() {
            handler.menu_action(cx, action_id);
        }
    });
}

pub fn on_application_did_finish_launching() {
    with_app(|handler, cx, _state| {
        handler.init(cx);
    });
}

pub fn on_application_should_terminate() -> bool {
    with_app(|handler, cx, _state| {
        handler.should_terminate(cx)
    })
}

pub fn on_application_reopen(has_visible_windows: bool) {
    if !has_visible_windows {
        // App reopen when all windows were closed: handler can handle or open window
        with_app(|handler, cx, state| {
            if let Some(win_id) = state.active_window {
                handler.event(cx, win_id, PlatformEvent::Focus(true));
            }
        });
    }
}

pub fn on_application_open_files(paths: Vec<PathBuf>) {
    with_app(|handler, cx, state| {
        let target_win = state.active_window.or_else(|| state.windows.keys().next().copied());
        if let Some(win_id) = target_win {
            handler.event(cx, win_id, PlatformEvent::FileDrop(paths));
        }
    });
}

/// Create a new window within active app state.
pub fn cx_create_window(opts: &WindowOpts) -> WindowId {
    with_state(|state| {
        let win_id = WindowId::next();
        let req_flag = Arc::new(AtomicBool::new(false));
        let pending_flag = Arc::new(AtomicBool::new(false));

        register_window_flags(win_id, req_flag.clone(), pending_flag.clone());
        state.window_flags.insert(win_id, (req_flag, pending_flag));

        let win_state = create_native_window(win_id, opts, state.mtm);
        state.windows.insert(win_id, win_state);
        state.active_window = Some(win_id);
        win_id
    })
}

/// Close an existing window within active app state.
pub fn cx_close_window(win_id: WindowId) {
    with_state(|state| {
        unregister_window_flags(win_id);
        state.window_flags.remove(&win_id);
        state.context_menus.remove(&win_id);
        if let Some(win_state) = state.windows.remove(&win_id) {
            win_state.window.close();
        }
        if state.active_window == Some(win_id) {
            state.active_window = state.windows.keys().next().copied();
        }
    });
}

/// Request display frame for a window.
pub fn cx_request_frame(win_id: WindowId) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get_mut(&win_id) {
            if let Some((req_flag, _)) = state.window_flags.get(&win_id) {
                req_flag.store(true, Ordering::Release);
                win_state.idle_frames = 0;
                if let Some(link) = &win_state.display_link {
                    link.set_paused(false);
                }
            }
        }
    });
}

/// Set mouse cursor icon.
pub fn cx_set_cursor(_win_id: WindowId, cursor: CursorIcon) {
    crate::macos::cursor::set_cursor(cursor);
}

/// Set IME composition rectangle for a window.
pub fn cx_set_ime_rect(win_id: WindowId, r: Rect) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            win_state.view.set_ime_rect(r);
        }
    });
}

/// Set drag regions for a window.
pub fn cx_set_drag_regions(win_id: WindowId, regions: Vec<Rect>) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            win_state.view.set_drag_regions(regions);
        }
    });
}

/// Set window opacity.
pub fn cx_set_window_opacity(win_id: WindowId, opacity: f32) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            // SAFETY: Updating opacity properties on NSWindow.
            unsafe {
                if opacity < 1.0 {
                    win_state.window.setOpaque(false);
                    win_state.window.setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
                } else {
                    win_state.window.setOpaque(true);
                }
            }
        }
    });
}

/// Set accessible text for VoiceOver.
pub fn cx_set_accessible_text(win_id: WindowId, text: &str) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            win_state.view.set_accessible_text(text);
        }
    });
}

/// Toggle native full screen.
pub fn cx_toggle_fullscreen(win_id: WindowId) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            win_state.window.toggleFullScreen(None);
        }
    });
}

/// Update main application menu from MenuSpec.
pub fn cx_set_menu(menu: &MenuSpec) {
    with_state(|state| {
        state.menu_actions.clear();
        let native_menu = build_native_menu(menu, &mut state.menu_actions, state.mtm);
        let app = NSApplication::sharedApplication(state.mtm);
        app.setMainMenu(Some(&native_menu));
    });
}

/// Open URL in default browser.
pub fn cx_open_url(url: &str) {
    // SAFETY: Calling NSWorkspace openURL on main thread.
    unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        let ns_str = NSString::from_str(url);
        if let Some(ns_url) = NSURL::URLWithString(&ns_str) {
            ws.openURL(&ns_url);
        }
    }
}

/// Reveal path in Finder.
pub fn cx_reveal_in_finder(path: &Path) {
    // SAFETY: Calling activateFileViewerSelectingURLs on main thread.
    unsafe {
        let ws = NSWorkspace::sharedWorkspace();
        let path_str = path.to_string_lossy();
        let ns_str = NSString::from_str(&path_str);
        let ns_url = NSURL::fileURLWithPath(&ns_str);
        let urls = NSArray::from_id_slice(&[ns_url]);
        ws.activateFileViewerSelectingURLs(&urls);
    }
}
