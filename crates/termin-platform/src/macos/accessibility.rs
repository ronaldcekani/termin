//! Accessibility support (NSAccessibilityRole::TextArea, visible text, throttled notifications, VoiceOver announcements).
//!
//! See `docs/specs/platform-macos.md §10` and `docs/design/components.md §15`.

use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAccessibilityAnnouncementKey, NSAccessibilityAnnouncementRequestedNotification,
    NSAccessibilityElement, NSAccessibilityPostNotification,
    NSAccessibilityPostNotificationWithUserInfo, NSAccessibilityValueChangedNotification,
};
use objc2_foundation::{MainThreadMarker, NSDictionary, NSPoint, NSRect, NSSize, NSString};

use crate::accessibility::AccessibilityNode;

/// Minimum interval between accessibility value change notifications (throttled to 2/s).
pub const ACCESSIBILITY_THROTTLE_INTERVAL: Duration = Duration::from_millis(500);

/// Check if an accessibility notification can be dispatched based on rate throttling.
pub fn should_post_notification(last_posted: Option<Instant>) -> bool {
    match last_posted {
        None => true,
        Some(last) => Instant::now().duration_since(last) >= ACCESSIBILITY_THROTTLE_INTERVAL,
    }
}

/// Post an accessibility value changed notification if throttling allows.
pub fn post_value_changed_notification(element: &AnyObject, last_posted: &mut Option<Instant>) {
    if should_post_notification(*last_posted) {
        *last_posted = Some(Instant::now());
        // SAFETY: Posting accessibility notification to AppKit on the main thread.
        unsafe {
            NSAccessibilityPostNotification(element, NSAccessibilityValueChangedNotification);
        }
    }
}

/// Post an accessibility announcement for VoiceOver (e.g. command completion or status change).
pub fn post_accessibility_announcement(element: &AnyObject, message: &str) {
    let msg_ns = NSString::from_str(message);
    let key = unsafe { NSAccessibilityAnnouncementKey };
    // SAFETY: NSDictionary creation and posting accessibility announcement to AppKit on the main thread.
    unsafe {
        let dict: Retained<NSDictionary<AnyObject, AnyObject>> =
            objc2::msg_send_id![objc2::class!(NSDictionary), dictionaryWithObject: &*msg_ns, forKey: key];
        let dict_ref: &NSDictionary<AnyObject, AnyObject> = &dict;
        let cast_dict: &AnyObject = &*(dict_ref as *const _ as *const AnyObject);
        NSAccessibilityPostNotificationWithUserInfo(
            element,
            NSAccessibilityAnnouncementRequestedNotification,
            Some(&*(cast_dict as *const AnyObject as *const _)),
        );
    }
}

/// Create native `NSAccessibilityElement` objects representing an `AccessibilityNode` hierarchy.
pub fn build_native_element(
    node: &AccessibilityNode,
    parent: &AnyObject,
    _mtm: MainThreadMarker,
) -> Retained<AnyObject> {
    let frame = NSRect::new(
        NSPoint::new(node.bounds.x as f64, node.bounds.y as f64),
        NSSize::new(node.bounds.w as f64, node.bounds.h as f64),
    );
    let label_ns = NSString::from_str(&node.label);
    let role_ns = NSString::from_str(node.role.macos_role_name());

    // SAFETY: Allocating and configuring NSAccessibilityElement on main thread.
    unsafe {
        let element: Retained<AnyObject> =
            NSAccessibilityElement::accessibilityElementWithRole_frame_label_parent(
                &role_ns,
                frame,
                Some(&label_ns),
                Some(parent),
            );

        if let Some(val) = &node.value {
            let val_ns = NSString::from_str(val);
            let _: () = objc2::msg_send![&*element, setAccessibilityValue: &*val_ns];
        }

        if let Some(desc) = &node.description {
            let desc_ns = NSString::from_str(desc);
            let _: () = objc2::msg_send![&*element, setAccessibilityHelp: &*desc_ns];
        }

        for child in &node.children {
            let child_elem = build_native_element(child, &element, _mtm);
            let _: () = objc2::msg_send![&*element, accessibilityAddChildElement: &*child_elem];
        }

        element
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_accessibility_throttling() {
        assert!(should_post_notification(None));
        let now = Instant::now();
        assert!(!should_post_notification(Some(now)));
        let past = now - Duration::from_millis(600);
        assert!(should_post_notification(Some(past)));
    }
}
