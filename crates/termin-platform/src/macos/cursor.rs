//! Mouse cursor management via NSCursor.
//!
//! See `docs/specs/platform-macos.md §3`.

use crate::CursorIcon;
use objc2_app_kit::NSCursor;

/// Set the current mouse cursor appearance.
pub fn set_cursor(cursor: CursorIcon) {
    // SAFETY: Accessing NSCursor class methods and calling set() on the main thread.
    unsafe {
        let ns_cursor = match cursor {
            CursorIcon::Arrow => NSCursor::arrowCursor(),
            CursorIcon::IBeam => NSCursor::IBeamCursor(),
            CursorIcon::PointingHand => NSCursor::pointingHandCursor(),
            CursorIcon::ResizeLeftRight => NSCursor::resizeLeftRightCursor(),
            CursorIcon::ResizeUpDown => NSCursor::resizeUpDownCursor(),
        };
        ns_cursor.set();
    }
}
