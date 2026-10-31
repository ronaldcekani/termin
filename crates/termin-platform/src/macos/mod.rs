//! macOS platform implementation.
//!
//! See `docs/specs/platform-macos.md`.

pub mod accessibility;
pub mod app;
pub mod clipboard;
pub mod cursor;
pub mod display_link;
pub mod input;
pub mod menu;
pub mod notification;
pub mod view;
pub mod wake;
pub mod window;

use crate::{AppHandler, Appearance, Platform};
use app::{AppState, AppDelegate, STATE};
use input::OptionAsAlt;
use wake::WakeSource;

use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;
use rustc_hash::FxHashMap;

/// macOS native platform implementation.
pub struct MacPlatform;

impl Platform for MacPlatform {
    fn run<H: AppHandler>(handler: H) -> ! {
        let mtm = MainThreadMarker::new().expect("Platform::run must be called on the main thread");

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

        let delegate = AppDelegate::new(mtm);
        app.setDelegate(Some(objc2::runtime::ProtocolObject::from_ref(&*delegate)));

        let (wake_handle, wake_source) = WakeSource::new();

        STATE.with(|cell| {
            *cell.borrow_mut() = Some(AppState {
                handler: Box::new(handler),
                windows: FxHashMap::default(),
                window_flags: FxHashMap::default(),
                active_window: None,
                option_as_alt: OptionAsAlt::Left,
                menu_actions: FxHashMap::default(),
                context_menus: FxHashMap::default(),
                wake_source: Some(wake_source),
                wake_handle,
                appearance: Appearance::Dark,
                mtm,
            });
        });

        // SAFETY: Starting AppKit event loop; will not return until process termination.
        unsafe {
            app.run();
        }

        unreachable!("NSApp.run() terminated unexpectedly");
    }
}
