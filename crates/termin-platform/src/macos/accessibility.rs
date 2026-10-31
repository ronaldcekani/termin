//! Accessibility support (NSAccessibilityRole::TextArea, visible text, throttled notifications).
//!
//! See `docs/specs/platform-macos.md §10`.

use std::time::{Duration, Instant};
use objc2_app_kit::{
    NSAccessibilityPostNotification, NSAccessibilityValueChangedNotification,
};
use objc2::runtime::AnyObject;

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
