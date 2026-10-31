//! macOS clipboard interaction via NSPasteboard.
//!
//! See `docs/specs/platform-macos.md §7`.

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// Retrieve the current string value from the system pasteboard.
pub fn clipboard_get() -> Option<String> {
    // SAFETY: Calling standard NSPasteboard API on the main thread.
    unsafe {
        let pboard = NSPasteboard::generalPasteboard();
        let ns_str = pboard.stringForType(NSPasteboardTypeString);
        ns_str.map(|s| s.to_string())
    }
}

/// Write a string value to the system pasteboard.
pub fn clipboard_set(text: &str) {
    // SAFETY: Clearing and updating NSPasteboard string on the main thread.
    unsafe {
        let pboard = NSPasteboard::generalPasteboard();
        pboard.clearContents();
        let ns_str = NSString::from_str(text);
        pboard.setString_forType(&ns_str, NSPasteboardTypeString);
    }
}
