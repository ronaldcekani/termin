//! Application lifecycle, NSApplication setup, AppDelegate, and event coordination.
//!
//! See `docs/specs/platform-macos.md §1`.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::macos::input::OptionAsAlt;
use crate::macos::menu::build_native_menu;
use crate::macos::wake::WakeSource;
use crate::macos::window::{create_native_window, MetalRenderTarget, WindowState};
use crate::{
    ActionId, AppHandler, Appearance, CursorIcon, MenuSpec, PlatformCx, PlatformEvent, WindowOpts,
};
use termin_core::{Rect, Size, WakeHandle, WindowId};

use objc2::rc::Retained;
use objc2::{declare_class, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSApplicationTerminateReply, NSMenu, NSMenuItem,
    NSWorkspace,
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
    pub handler: Option<Box<dyn AppHandler>>,
    pub pending_events: VecDeque<(WindowId, PlatformEvent)>,
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

/// Run a closure on `AppHandler` with a fresh `PlatformCx` without holding the `STATE` borrow.
/// Any re-entrant `PlatformEvent`s emitted by AppKit during the callback are queued and drained
/// before returning.
pub fn run_handler<R>(f: impl FnOnce(&mut dyn AppHandler, &mut PlatformCx) -> R) -> Option<R> {
    let (mut handler, mut cx) = STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let state = borrow.as_mut()?;
        let handler = state.handler.take()?;
        let cx = PlatformCx::new(state.wake_handle.clone());
        Some((handler, cx))
    })?;

    let res = f(handler.as_mut(), &mut cx);

    loop {
        let next_ev = STATE.with(|cell| {
            let mut borrow = cell.borrow_mut();
            borrow.as_mut().and_then(|s| s.pending_events.pop_front())
        });
        match next_ev {
            Some((win_id, ev)) => handler.event(&mut cx, win_id, ev),
            None => break,
        }
    }

    STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(state) = borrow.as_mut() {
            state.handler = Some(handler);
        }
    });

    Some(res)
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

pub fn set_window_size_and_scale(win_id: WindowId, size: Size, scale_factor: f32) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get_mut(&win_id) {
            win_state.size = size;
            win_state.scale_factor = scale_factor;
        }
    });
}

pub fn set_appearance(appearance: Appearance) {
    with_state(|state| {
        state.appearance = appearance;
    });
}

pub fn get_context_menu(win_id: WindowId) -> Option<Retained<NSMenu>> {
    with_state(|state| state.context_menus.get(&win_id).cloned())
}

pub fn dispatch_event(win_id: WindowId, ev: PlatformEvent) {
    let mut ev_opt = Some(ev);
    let is_reentrant = STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        if let Some(state) = borrow.as_mut() {
            if state.handler.is_none() {
                if let Some(e) = ev_opt.take() {
                    state.pending_events.push_back((win_id, e));
                }
                return true;
            }
        }
        false
    });
    if is_reentrant {
        return;
    }
    if let Some(e) = ev_opt {
        run_handler(|handler, cx| {
            handler.event(cx, win_id, e);
        });
    }
}

pub fn dispatch_sync_render(win_id: WindowId) {
    let target_opt = STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let state = borrow.as_mut()?;
        if state.handler.is_none() {
            if let Some(win_state) = state.windows.get_mut(&win_id) {
                if let Some((req_flag, _)) = state.window_flags.get(&win_id) {
                    req_flag.store(true, Ordering::Release);
                    win_state.idle_frames = 0;
                    if let Some(link) = &win_state.display_link {
                        link.set_paused(false);
                    }
                }
            }
            return None;
        }
        let win_state = state.windows.get(&win_id)?;
        Some(MetalRenderTarget {
            size: win_state.size,
            scale_factor: win_state.scale_factor,
            metal_layer: win_state.metal_layer.clone(),
        })
    });

    if let Some(mut target) = target_opt {
        // The resize event that preceded this call armed a display-link frame;
        // this synchronous render supersedes it, so drop the pending request
        // (a frame requested *during* the render below is kept).
        with_state(|state| {
            if let Some((req_flag, _)) = state.window_flags.get(&win_id) {
                req_flag.store(false, Ordering::Release);
            }
        });
        // SAFETY: Presents with transaction during live resize.
        unsafe {
            target.metal_layer.setPresentsWithTransaction(true);
        }
        run_handler(|handler, cx| {
            handler.frame(cx, win_id, &mut target);
        });
        // SAFETY: Reset presentsWithTransaction after frame.
        unsafe {
            target.metal_layer.setPresentsWithTransaction(false);
        }
    }
}

pub fn on_display_link_fire(win_id: WindowId) {
    let target_opt = STATE.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let state = borrow.as_mut()?;
        state.handler.as_ref()?;
        let win_state = state.windows.get_mut(&win_id)?;
        let (req_flag, pending_flag) = state.window_flags.get(&win_id)?;
        pending_flag.store(false, Ordering::Release);
        let requested = req_flag.swap(false, Ordering::AcqRel);

        if requested && !win_state.is_occluded {
            win_state.idle_frames = 0;
            Some(MetalRenderTarget {
                size: win_state.size,
                scale_factor: win_state.scale_factor,
                metal_layer: win_state.metal_layer.clone(),
            })
        } else {
            win_state.idle_frames += 1;
            if win_state.idle_frames >= 2 {
                if let Some(link) = &win_state.display_link {
                    link.set_paused(true);
                }
            }
            None
        }
    });

    if let Some(mut target) = target_opt {
        run_handler(|handler, cx| {
            handler.frame(cx, win_id, &mut target);
        });
    }
}

pub fn on_wake_performed() {
    run_handler(|handler, cx| {
        handler.wake(cx);
    });
}

pub fn on_menu_action(tag: u32) {
    let action_id = with_state(|state| state.menu_actions.get(&tag).copied());
    if let Some(action_id) = action_id {
        run_handler(|handler, cx| {
            handler.menu_action(cx, action_id);
        });
    }
}

pub fn on_application_did_finish_launching() {
    run_handler(|handler, cx| {
        handler.init(cx);
    });
}

pub fn on_application_should_terminate() -> bool {
    run_handler(|handler, cx| handler.should_terminate(cx)).unwrap_or(true)
}

pub fn on_application_reopen(has_visible_windows: bool) {
    if !has_visible_windows {
        let active_win = with_state(|state| state.active_window);
        if let Some(win_id) = active_win {
            dispatch_event(win_id, PlatformEvent::Focus(true));
        }
    }
}

pub fn on_application_open_files(paths: Vec<PathBuf>) {
    let target_win = with_state(|state| {
        state
            .active_window
            .or_else(|| state.windows.keys().next().copied())
    });
    if let Some(win_id) = target_win {
        dispatch_event(win_id, PlatformEvent::FileDrop(paths));
    }
}

/// Create a new window within active app state.
pub fn cx_create_window(opts: &WindowOpts) -> WindowId {
    let (win_id, mtm) = with_state(|state| {
        let win_id = WindowId::next();
        let req_flag = Arc::new(AtomicBool::new(true));
        let pending_flag = Arc::new(AtomicBool::new(false));

        register_window_flags(win_id, req_flag.clone(), pending_flag.clone());
        state.window_flags.insert(win_id, (req_flag, pending_flag));
        (win_id, state.mtm)
    });

    let win_state = create_native_window(win_id, opts, mtm);
    let window = win_state.window.clone();
    if let Some(link) = &win_state.display_link {
        link.set_paused(false);
    }

    with_state(|state| {
        state.windows.insert(win_id, win_state);
        state.active_window = Some(win_id);
    });

    // Present window after registration so synchronous AppKit delegate callbacks find it in `state.windows`.
    window.makeKeyAndOrderFront(None);
    let app = NSApplication::sharedApplication(mtm);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    win_id
}

/// Close an existing window within active app state.
pub fn cx_close_window(win_id: WindowId) {
    let removed = with_state(|state| {
        unregister_window_flags(win_id);
        state.window_flags.remove(&win_id);
        state.context_menus.remove(&win_id);
        let win_state = state.windows.remove(&win_id);
        if state.active_window == Some(win_id) {
            state.active_window = state.windows.keys().next().copied();
        }
        win_state
    });
    if let Some(win_state) = removed {
        // NSWindow holds its delegate unretained; detach it before our
        // `Retained<WindowDelegate>` drops so close animations can't call into freed memory.
        win_state.window.setDelegate(None);
        win_state.window.close();
    }
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

/// Set double-click passthrough regions for a window.
pub fn cx_set_dblclick_regions(win_id: WindowId, regions: Vec<Rect>) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            win_state.view.set_dblclick_regions(regions);
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
                    win_state
                        .window
                        .setBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor()));
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

/// Set accessibility tree snapshot for VoiceOver.
pub fn cx_set_accessibility_tree(
    win_id: WindowId,
    tree: crate::accessibility::AccessibilityTreeSnapshot,
) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            win_state.view.set_accessibility_tree(tree);
        }
    });
}

/// Post a VoiceOver announcement.
pub fn cx_announce(win_id: WindowId, message: &str) {
    with_state(|state| {
        if let Some(win_state) = state.windows.get(&win_id) {
            win_state.view.announce(message);
        }
    });
}

/// Toggle native full screen.
pub fn cx_toggle_fullscreen(win_id: WindowId) {
    let window = with_state(|state| state.windows.get(&win_id).map(|w| w.window.clone()));
    if let Some(window) = window {
        window.toggleFullScreen(None);
    }
}

/// Update main application menu from MenuSpec.
pub fn cx_set_menu(menu: &MenuSpec) {
    let (native_menu, mtm) = with_state(|state| {
        state.menu_actions.clear();
        let native_menu = build_native_menu(menu, &mut state.menu_actions, state.mtm);
        (native_menu, state.mtm)
    });
    let app = NSApplication::sharedApplication(mtm);
    app.setMainMenu(Some(&native_menu));
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

#[cfg(test)]
mod tests {
    use super::*;

    struct ReentrantTestHandler {
        events: Arc<Mutex<Vec<(WindowId, PlatformEvent)>>>,
    }

    impl AppHandler for ReentrantTestHandler {
        fn init(&mut self, cx: &mut PlatformCx) {
            // Calling cx methods that borrow STATE inside init must not panic
            cx.set_option_as_alt(OptionAsAlt::Both);
            assert_eq!(cx.appearance(), Appearance::Dark);
            // Simulating synchronous AppKit callback firing during init/window creation
            dispatch_event(WindowId(1), PlatformEvent::Focus(true));
        }
        fn event(&mut self, cx: &mut PlatformCx, win: WindowId, ev: PlatformEvent) {
            let _ = cx.appearance();
            self.events.lock().unwrap().push((win, ev));
        }
        fn frame(
            &mut self,
            _cx: &mut PlatformCx,
            _win: WindowId,
            _target: &mut dyn crate::RenderTarget,
        ) {
        }
        fn wake(&mut self, _cx: &mut PlatformCx) {}
        fn menu_action(&mut self, _cx: &mut PlatformCx, _action: ActionId) {}
        fn should_terminate(&mut self, _cx: &mut PlatformCx) -> bool {
            true
        }
    }

    #[test]
    fn test_run_handler_allows_state_access_and_reentrant_events() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        STATE.with(|cell| {
            *cell.borrow_mut() = Some(AppState {
                handler: Some(Box::new(ReentrantTestHandler {
                    events: Arc::clone(&events),
                })),
                pending_events: VecDeque::new(),
                windows: FxHashMap::default(),
                window_flags: FxHashMap::default(),
                active_window: None,
                option_as_alt: OptionAsAlt::Left,
                menu_actions: FxHashMap::default(),
                context_menus: FxHashMap::default(),
                wake_source: None,
                wake_handle: WakeHandle::new(|| {}),
                appearance: Appearance::Dark,
                mtm,
            });
        });

        on_application_did_finish_launching();

        assert_eq!(get_option_as_alt(), OptionAsAlt::Both);
        let recorded = events.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], (WindowId(1), PlatformEvent::Focus(true)));

        STATE.with(|cell| {
            *cell.borrow_mut() = None;
        });
    }
}
