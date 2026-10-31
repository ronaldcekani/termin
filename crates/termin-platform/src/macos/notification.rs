//! Desktop notifications via UNUserNotificationCenter and dock badge updates.
//!
//! See `docs/specs/platform-macos.md §8`.

use crate::SystemNotification;
use block2::RcBlock;
use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSRequestUserAttentionType};
use objc2_foundation::{
    MainThreadMarker, NSBundle, NSDictionary, NSNumber, NSString,
};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationSound, UNUserNotificationCenter,
};
use std::sync::atomic::{AtomicBool, Ordering};

static AUTHORIZATION_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Request notification center authorization lazily on first notification.
fn ensure_authorized(center: &UNUserNotificationCenter) {
    if !AUTHORIZATION_REQUESTED.swap(true, Ordering::SeqCst) {
        let options = UNAuthorizationOptions::UNAuthorizationOptionAlert
            | UNAuthorizationOptions::UNAuthorizationOptionSound
            | UNAuthorizationOptions::UNAuthorizationOptionBadge;

        let block = RcBlock::new(|_granted: objc2::runtime::Bool, _error: *mut objc2_foundation::NSError| {
            // Lazy authorization result handled silently
        });

        // SAFETY: Calling requestAuthorizationWithOptions with valid block.
        unsafe {
            center.requestAuthorizationWithOptions_completionHandler(options, &block);
        }
    }
}

/// Post a desktop notification.
pub fn notify(n: &SystemNotification) {
    // Check if running as an app bundle
    // SAFETY: Checking bundle identifier on the main thread.
    let is_bundled = unsafe {
        NSBundle::mainBundle()
            .bundleIdentifier()
            .is_some()
    };

    if !is_bundled {
        log::info!("Notification [unbundled]: {} — {}", n.title, n.body);
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            app.requestUserAttention(NSRequestUserAttentionType::NSInformationalRequest);
        }
        return;
    }

    // SAFETY: Accessing currentNotificationCenter on macOS.
    unsafe {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        ensure_authorized(&center);

        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&n.title));
        content.setBody(&NSString::from_str(&n.body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));

        let k1 = NSString::from_str("window");
        let k2 = NSString::from_str("workspace");
        let k3 = NSString::from_str("pane");
        let k4 = NSString::from_str("tab");
        let key_refs = [&*k1, &*k2, &*k3, &*k4];

        let objects = vec![
            NSNumber::new_u32(n.window.0),
            NSNumber::new_u32(n.workspace.0),
            NSNumber::new_u32(n.pane.0),
            NSNumber::new_u32(n.tab.0),
        ];

        let user_info = NSDictionary::from_vec(&key_refs, objects);
        let dict_ref: &NSDictionary = &*(Retained::as_ptr(&user_info) as *const NSDictionary);
        content.setUserInfo(dict_ref);

        let request_id = NSString::from_str(&format!(
            "termin-{}-{}-{}",
            n.window.0,
            n.tab.0,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        ));

        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &request_id,
            &content,
            None,
        );

        center.addNotificationRequest_withCompletionHandler(
            &request,
            None,
        );
    }
}

/// Set the dock tile badge label count (0 clears badge).
pub fn set_badge(count: u32) {
    if let Some(mtm) = MainThreadMarker::new() {
        // SAFETY: Accessing NSApplication and dockTile on main thread.
        unsafe {
            let app = NSApplication::sharedApplication(mtm);
            let tile = app.dockTile();
            if count > 0 {
                let label = NSString::from_str(&count.to_string());
                tile.setBadgeLabel(Some(&label));
            } else {
                tile.setBadgeLabel(None);
            }
        }
    }
}
