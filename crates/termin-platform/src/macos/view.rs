//! TerminView — custom NSView hosting CAMetalLayer, NSTextInputClient, and input translation.
//!
//! See `docs/specs/platform-macos.md §3, §4`.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::time::Instant;

use crate::macos::accessibility::post_value_changed_notification;
use crate::macos::input::device_modifiers::{NX_DEVICELALTKEYMASK, NX_DEVICERALTKEYMASK};
use crate::macos::input::{extract_mods, resolve_key, selector_to_named_key, translate_keycode};
use crate::{Appearance, PlatformEvent};
use termin_core::{
    Key, KeyEvent, MouseButton, MouseEvent, MouseKind, Point, Rect, ScrollEvent, ScrollPhase, Size,
    SmallStr, WindowId,
};

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_app_kit::{
    NSAppearanceCustomization, NSApplication, NSDragOperation, NSDraggingDestination,
    NSDraggingInfo, NSEvent, NSEventModifierFlags, NSEventPhase, NSFilenamesPboardType, NSMenu,
    NSPasteboardTypeFileURL, NSResponder, NSTextInputClient, NSTrackingArea, NSTrackingAreaOptions,
    NSView, NSViewLayerContentsRedrawPolicy,
};
use objc2_foundation::{
    CGFloat, MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey, NSNotFound,
    NSObject, NSObjectProtocol, NSPoint, NSRange, NSRangePointer, NSRect, NSSize, NSString,
    NSUserDefaults, NSValue, NSURL,
};
use objc2_metal::MTLPixelFormat;
use objc2_quartz_core::{CALayer, CAMetalLayer};

/// Instance variables for `TerminView`.
pub struct TerminViewIvars {
    pub window_id: Cell<WindowId>,
    pub tracking_area: RefCell<Option<Retained<NSTrackingArea>>>,
    pub drag_regions: RefCell<Vec<Rect>>,
    /// Sub-areas of drag regions whose double-clicks go to the app instead of zooming.
    pub dblclick_regions: RefCell<Vec<Rect>>,
    pub marked_text: RefCell<String>,
    pub marked_selection: Cell<NSRange>,
    pub ime_cursor_rect: Cell<NSRect>,
    pub in_key_down: RefCell<Option<Retained<NSEvent>>>,
    pub key_down_handled: Cell<bool>,
    pub accessible_text: RefCell<String>,
    pub accessibility_tree: RefCell<crate::accessibility::AccessibilityTreeSnapshot>,
    pub last_accessibility_notify: RefCell<Option<Instant>>,
}

declare_class!(
    /// Native view for Termin hosting Metal rendering and input processing.
    pub struct TerminView;

    unsafe impl ClassType for TerminView {
        #[inherits(NSResponder, NSObject)]
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TerminView";
    }

    impl DeclaredClass for TerminView {
        type Ivars = TerminViewIvars;
    }

    unsafe impl NSObjectProtocol for TerminView {}

    unsafe impl TerminView {
        #[method(isFlipped)]
        fn is_flipped(&self) -> bool {
            // Termin uses top-left origin coordinates (y increases downward).
            true
        }

        #[method(acceptsFirstResponder)]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[method(canBecomeKeyView)]
        fn can_become_key_view(&self) -> bool {
            true
        }

        #[method(wantsUpdateLayer)]
        fn wants_update_layer(&self) -> bool {
            true
        }

        #[method_id(makeBackingLayer)]
        unsafe fn make_backing_layer(&self) -> Retained<CALayer> {
            let layer = CAMetalLayer::new();
            let dev_ptr = objc2_metal::MTLCreateSystemDefaultDevice();
            if !dev_ptr.is_null() {
                // SAFETY: MTLCreateSystemDefaultDevice returns a valid pointer to default MTLDevice.
                let dev: &ProtocolObject<dyn objc2_metal::MTLDevice> = &*dev_ptr;
                layer.setDevice(Some(dev));
            }
            layer.setPixelFormat(MTLPixelFormat::BGRA8Unorm_sRGB);
            layer.setFramebufferOnly(true);
            layer.setPresentsWithTransaction(false);
            layer.setMaximumDrawableCount(2);
            // Keep existing contents pinned top-left while resizing instead of stretching.
            layer.setContentsGravity(objc2_foundation::ns_string!("topLeft"));

            let scale = self
                .window()
                .map(|w| w.backingScaleFactor())
                .unwrap_or(2.0);
            layer.setContentsScale(scale);

            let bounds = self.bounds();
            layer.setDrawableSize(NSSize::new(
                bounds.size.width * scale,
                bounds.size.height * scale,
            ));

            Retained::into_super(layer)
        }

        #[method(mouseDownCanMoveWindow)]
        fn mouse_down_can_move_window(&self) -> bool {
            false
        }

        #[method(updateTrackingAreas)]
        unsafe fn update_tracking_areas(&self) {
            let mut area_slot = self.ivars().tracking_area.borrow_mut();
            if let Some(old_area) = area_slot.take() {
                self.removeTrackingArea(&old_area);
            }

            let options = NSTrackingAreaOptions::NSTrackingMouseMoved
                | NSTrackingAreaOptions::NSTrackingMouseEnteredAndExited
                | NSTrackingAreaOptions::NSTrackingActiveAlways
                | NSTrackingAreaOptions::NSTrackingInVisibleRect;

            let new_area = NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                self.bounds(),
                options,
                Some(self),
                None,
            );

            self.addTrackingArea(&new_area);
            *area_slot = Some(new_area);

            let () = msg_send![super(self), updateTrackingAreas];
        }

        #[method(keyDown:)]
        unsafe fn key_down(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let option_as_alt = crate::macos::app::get_option_as_alt();

            // Check if Option key should act as Alt
            let flags = event.modifierFlags();
            let raw_flags = flags.bits();
            let left_alt = (raw_flags & NX_DEVICELALTKEYMASK) != 0;
            let right_alt = (raw_flags & NX_DEVICERALTKEYMASK) != 0;
            let treat_as_alt = option_as_alt.should_treat_as_alt(left_alt, right_alt);

            if treat_as_alt && flags.contains(NSEventModifierFlags::NSEventModifierFlagOption) {
                // Bypass IME, emit Key with ALT modifier
                let code = event.keyCode();
                let chars = event
                    .charactersIgnoringModifiers()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let key = resolve_key(code, &chars);
                let mods = extract_mods(flags);
                let repeat = event.isARepeat();

                crate::macos::app::dispatch_event(
                    win_id,
                    PlatformEvent::Key(KeyEvent {
                        key,
                        physical: code,
                        mods,
                        text: None,
                        repeat,
                        pressed: true,
                    }),
                );
                return;
            }

            // Command shortcuts bypass IME
            if flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand) {
                let code = event.keyCode();
                let chars = event
                    .charactersIgnoringModifiers()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let key = resolve_key(code, &chars);
                let mods = extract_mods(flags);
                let repeat = event.isARepeat();

                crate::macos::app::dispatch_event(
                    win_id,
                    PlatformEvent::Key(KeyEvent {
                        key,
                        physical: code,
                        mods,
                        text: None,
                        repeat,
                        pressed: true,
                    }),
                );
                return;
            }

            // Normal text / IME composition via interpretKeyEvents
            *self.ivars().in_key_down.borrow_mut() = Some(event.retain());
            self.ivars().key_down_handled.set(false);

            let events = NSArray::from_slice(&[event]);
            let () = msg_send![self, interpretKeyEvents: &*events];

            let handled = self.ivars().key_down_handled.get();
            self.ivars().in_key_down.borrow_mut().take();

            if !handled {
                // Fallback for special keys not consumed by NSTextInputClient
                let code = event.keyCode();
                let chars = event
                    .charactersIgnoringModifiers()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let key = resolve_key(code, &chars);
                let mods = extract_mods(flags);
                let repeat = event.isARepeat();

                crate::macos::app::dispatch_event(
                    win_id,
                    PlatformEvent::Key(KeyEvent {
                        key,
                        physical: code,
                        mods,
                        text: None,
                        repeat,
                        pressed: true,
                    }),
                );
            }
        }

        #[method(keyUp:)]
        unsafe fn key_up(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let code = event.keyCode();
            let chars = event
                .charactersIgnoringModifiers()
                .map(|s| s.to_string())
                .unwrap_or_default();
            let key = resolve_key(code, &chars);
            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Key(KeyEvent {
                    key,
                    physical: code,
                    mods,
                    text: None,
                    repeat: false,
                    pressed: false,
                }),
            );
        }

        #[method(flagsChanged:)]
        unsafe fn flags_changed(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let code = event.keyCode();
            let mods = extract_mods(event.modifierFlags());

            let key = if let Some(named) = translate_keycode(code) {
                Key::Named(named)
            } else {
                Key::Named(termin_core::NamedKey::Space)
            };

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Key(KeyEvent {
                    key,
                    physical: code,
                    mods,
                    text: None,
                    repeat: false,
                    pressed: true,
                }),
            );
        }

        #[method(performKeyEquivalent:)]
        unsafe fn perform_key_equivalent(&self, event: &NSEvent) -> objc2::runtime::Bool {
            if event.modifierFlags().contains(NSEventModifierFlags::NSEventModifierFlagCommand) {
                if let Some(mtm) = MainThreadMarker::new() {
                    let app = NSApplication::sharedApplication(mtm);
                    if let Some(menu) = app.mainMenu() {
                        if menu.performKeyEquivalent(event) {
                            return true.into();
                        }
                    }
                }
            }
            false.into()
        }

        #[method(mouseDown:)]
        unsafe fn mouse_down(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };

            // Check if click is in a window drag region
            let app_dblclick = event.clickCount() == 2 && self.is_in_dblclick_region(pt);
            let in_drag = self.is_in_drag_region(pt) && !app_dblclick;
            if in_drag {
                if event.clickCount() == 2 {
                    // Respect AppleActionOnDoubleClick user default
                    let defaults = NSUserDefaults::standardUserDefaults();
                    let action = defaults.stringForKey(&NSString::from_str("AppleActionOnDoubleClick"));
                    let action_str = action.as_ref().map(|s| s.to_string()).unwrap_or_default();
                    if let Some(win) = self.window() {
                        if action_str == "Minimize" {
                            win.miniaturize(None);
                        } else if action_str != "None" {
                            win.zoom(None);
                        }
                    }
                    return;
                }

                // Native window drag
                if let Some(win) = self.window() {
                    win.performWindowDragWithEvent(event);
                    return;
                }
            }

            let mods = extract_mods(event.modifierFlags());
            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Down(MouseButton::Left),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(mouseUp:)]
        unsafe fn mouse_up(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Up(MouseButton::Left),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(mouseDragged:)]
        unsafe fn mouse_dragged(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Drag(MouseButton::Left),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(rightMouseDown:)]
        unsafe fn right_mouse_down(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Down(MouseButton::Right),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(rightMouseUp:)]
        unsafe fn right_mouse_up(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Up(MouseButton::Right),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(rightMouseDragged:)]
        unsafe fn right_mouse_dragged(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Drag(MouseButton::Right),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(otherMouseDown:)]
        unsafe fn other_mouse_down(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());
            let btn = if event.buttonNumber() == 2 {
                MouseButton::Middle
            } else {
                MouseButton::Other(event.buttonNumber() as u8)
            };

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Down(btn),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(otherMouseUp:)]
        unsafe fn other_mouse_up(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());
            let btn = if event.buttonNumber() == 2 {
                MouseButton::Middle
            } else {
                MouseButton::Other(event.buttonNumber() as u8)
            };

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Up(btn),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(otherMouseDragged:)]
        unsafe fn other_mouse_dragged(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());
            let btn = if event.buttonNumber() == 2 {
                MouseButton::Middle
            } else {
                MouseButton::Other(event.buttonNumber() as u8)
            };

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Drag(btn),
                    pos: pt,
                    mods,
                    click_count: event.clickCount() as u8,
                }),
            );
        }

        #[method(mouseMoved:)]
        unsafe fn mouse_moved(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pt = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };
            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Move,
                    pos: pt,
                    mods,
                    click_count: 0,
                }),
            );
        }

        /// Pointer left the view: report a move far outside the window so hover state clears.
        #[method(mouseExited:)]
        unsafe fn mouse_exited(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let mods = extract_mods(event.modifierFlags());
            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Mouse(MouseEvent {
                    kind: MouseKind::Move,
                    pos: Point {
                        x: -1.0e6,
                        y: -1.0e6,
                    },
                    mods,
                    click_count: 0,
                }),
            );
        }

        #[method(scrollWheel:)]
        unsafe fn scroll_wheel(&self, event: &NSEvent) {
            let win_id = self.ivars().window_id.get();
            let loc = self.convertPoint_fromView(event.locationInWindow(), None);
            let pos = Point {
                x: loc.x as f32,
                y: loc.y as f32,
            };

            let precise = event.hasPreciseScrollingDeltas();
            let (dx, dy) = if precise {
                (
                    event.scrollingDeltaX() as f32,
                    event.scrollingDeltaY() as f32,
                )
            } else {
                (
                    (event.deltaX() * 10.0) as f32,
                    (event.deltaY() * 10.0) as f32,
                )
            };

            let p = event.phase();
            let mp = event.momentumPhase();
            let phase = if p.contains(NSEventPhase::Began) {
                ScrollPhase::Began
            } else if p.contains(NSEventPhase::Changed) {
                ScrollPhase::Changed
            } else if p.contains(NSEventPhase::Ended) {
                ScrollPhase::Ended
            } else if p.contains(NSEventPhase::Cancelled) {
                ScrollPhase::Cancelled
            } else if p.contains(NSEventPhase::MayBegin) {
                ScrollPhase::MayBegin
            } else if mp.contains(NSEventPhase::Began) || mp.contains(NSEventPhase::Changed) {
                ScrollPhase::Changed
            } else if mp.contains(NSEventPhase::Ended) || mp.contains(NSEventPhase::Cancelled) {
                ScrollPhase::Ended
            } else {
                ScrollPhase::None
            };

            let mods = extract_mods(event.modifierFlags());

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Scroll(ScrollEvent {
                    pos,
                    dx,
                    dy,
                    precise,
                    phase,
                    mods,
                }),
            );
        }

        #[method(viewDidChangeBackingProperties)]
        unsafe fn view_did_change_backing_properties(&self) {
            let win_id = self.ivars().window_id.get();
            let scale = self
                .window()
                .map(|w| w.backingScaleFactor() as f32)
                .unwrap_or(2.0);
            let bounds = self.bounds();
            let size = Size {
                w: bounds.size.width as f32,
                h: bounds.size.height as f32,
            };

            if let Some(layer) = self.layer() {
                layer.setContentsScale(scale as CGFloat);
                let metal_layer: &CAMetalLayer = &*(Retained::as_ptr(&layer) as *const CAMetalLayer);
                metal_layer.setDrawableSize(NSSize::new(
                    bounds.size.width * (scale as CGFloat),
                    bounds.size.height * (scale as CGFloat),
                ));
            }

            crate::macos::app::set_window_size_and_scale(win_id, size, scale);
            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Resized { size, scale },
            );
        }

        #[method(setFrameSize:)]
        unsafe fn set_frame_size(&self, new_size: NSSize) {
            let () = msg_send![super(self), setFrameSize: new_size];

            let win_id = self.ivars().window_id.get();
            let scale = self
                .window()
                .map(|w| w.backingScaleFactor() as f32)
                .unwrap_or(2.0);
            let size = Size {
                w: new_size.width as f32,
                h: new_size.height as f32,
            };

            if let Some(layer) = self.layer() {
                let metal_layer: &CAMetalLayer = &*(Retained::as_ptr(&layer) as *const CAMetalLayer);
                let want = NSSize::new(
                    new_size.width * (scale as CGFloat),
                    new_size.height * (scale as CGFloat),
                );
                // Skip the (costly, drawable-reallocating) call when unchanged.
                let cur = metal_layer.drawableSize();
                if cur.width != want.width || cur.height != want.height {
                    metal_layer.setDrawableSize(want);
                }
            }

            crate::macos::app::set_window_size_and_scale(win_id, size, scale);
            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Resized { size, scale },
            );

            // Synchronous live render during window resizing
            crate::macos::app::dispatch_sync_render(win_id);
        }

        #[method(viewDidEndLiveResize)]
        unsafe fn view_did_end_live_resize(&self) {
            let () = msg_send![super(self), viewDidEndLiveResize];
            // Final frame so any throttled terminal reflow settles promptly.
            let win_id = self.ivars().window_id.get();
            let bounds = self.bounds();
            let scale = self
                .window()
                .map(|w| w.backingScaleFactor() as f32)
                .unwrap_or(2.0);
            let size = Size {
                w: bounds.size.width as f32,
                h: bounds.size.height as f32,
            };
            crate::macos::app::dispatch_event(win_id, PlatformEvent::Resized { size, scale });
        }

        #[method(viewDidChangeEffectiveAppearance)]
        unsafe fn view_did_change_effective_appearance(&self) {
            let win_id = self.ivars().window_id.get();
            let app_name = self.effectiveAppearance().name();
            let is_dark = app_name.to_string().contains("Dark");
            let appearance = if is_dark {
                Appearance::Dark
            } else {
                Appearance::Light
            };

            crate::macos::app::set_appearance(appearance);
            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::Appearance(appearance),
            );
        }

        #[method_id(menuForEvent:)]
        unsafe fn menu_for_event(&self, _event: &NSEvent) -> Option<Retained<NSMenu>> {
            let win_id = self.ivars().window_id.get();
            crate::macos::app::get_context_menu(win_id)
        }

        // Accessibility methods
        #[method(isAccessibilityElement)]
        fn is_accessibility_element(&self) -> bool {
            true
        }

        #[method_id(accessibilityRole)]
        fn accessibility_role(&self) -> Retained<NSString> {
            let tree = self.ivars().accessibility_tree.borrow();
            if tree.root.is_some() {
                NSString::from_str("AXGroup")
            } else {
                NSString::from_str("AXTextArea")
            }
        }

        #[method_id(accessibilityChildren)]
        fn accessibility_children(&self) -> Option<Retained<NSArray<AnyObject>>> {
            let tree = self.ivars().accessibility_tree.borrow();
            if let Some(root) = tree.root() {
                let mtm = MainThreadMarker::from(self);
                let mut elements: Vec<Retained<AnyObject>> = Vec::new();
                for child in &root.children {
                    elements.push(crate::macos::accessibility::build_native_element(
                        child,
                        self.as_ref(),
                        mtm,
                    ));
                }
                if elements.is_empty() {
                    None
                } else {
                    Some(NSArray::from_id_slice(&elements))
                }
            } else {
                None
            }
        }

        #[method_id(accessibilityValue)]
        fn accessibility_value(&self) -> Retained<NSString> {
            NSString::from_str(&self.ivars().accessible_text.borrow())
        }

        #[method_id(accessibilitySelectedTextRange)]
        fn accessibility_selected_text_range(&self) -> Option<Retained<NSValue>> {
            let range = self.ivars().marked_selection.get();
            if range.location != NSNotFound as usize {
                // SAFETY: Instantiating NSValue with valid range.
                Some(unsafe { NSValue::valueWithRange(range) })
            } else {
                None
            }
        }
    }

    unsafe impl NSTextInputClient for TerminView {
        #[method(insertText:replacementRange:)]
        unsafe fn insert_text_replacement_range(
            &self,
            string: &AnyObject,
            _replacement_range: NSRange,
        ) {
            let win_id = self.ivars().window_id.get();

            let is_nsstring: bool = msg_send![string, isKindOfClass: NSString::class()];
            let is_attributed: bool = msg_send![string, isKindOfClass: NSAttributedString::class()];

            let text_str = if is_nsstring {
                let ns_str: &NSString = &*(string as *const AnyObject as *const NSString);
                ns_str.to_string()
            } else if is_attributed {
                let attr_str: &NSAttributedString =
                    &*(string as *const AnyObject as *const NSAttributedString);
                attr_str.string().to_string()
            } else {
                String::new()
            };

            self.ivars().marked_text.borrow_mut().clear();
            self.ivars().marked_selection.set(NSRange::new(NSNotFound as usize, 0));

            if let Some(event) = self.ivars().in_key_down.borrow().as_ref() {
                let code = event.keyCode();
                let chars = event
                    .charactersIgnoringModifiers()
                    .map(|s| s.to_string())
                    .unwrap_or_default();
                let key = resolve_key(code, &chars);
                let mods = extract_mods(event.modifierFlags());
                let repeat = event.isARepeat();

                crate::macos::app::dispatch_event(
                    win_id,
                    PlatformEvent::Key(KeyEvent {
                        key,
                        physical: code,
                        mods,
                        text: Some(SmallStr::from(text_str.as_str())),
                        repeat,
                        pressed: true,
                    }),
                );
                self.ivars().key_down_handled.set(true);
            } else {
                crate::macos::app::dispatch_event(
                    win_id,
                    PlatformEvent::ImeCommit(text_str),
                );
            }
        }

        #[method(doCommandBySelector:)]
        unsafe fn do_command_by_selector(&self, selector: objc2::runtime::Sel) {
            let sel_name = selector.name();
            if let Some(named_key) = selector_to_named_key(sel_name) {
                let win_id = self.ivars().window_id.get();
                if let Some(event) = self.ivars().in_key_down.borrow().as_ref() {
                    let code = event.keyCode();
                    let mods = extract_mods(event.modifierFlags());
                    let repeat = event.isARepeat();

                    crate::macos::app::dispatch_event(
                        win_id,
                        PlatformEvent::Key(KeyEvent {
                            key: Key::Named(named_key),
                            physical: code,
                            mods,
                            text: None,
                            repeat,
                            pressed: true,
                        }),
                    );
                    self.ivars().key_down_handled.set(true);
                }
            }
        }

        #[method(setMarkedText:selectedRange:replacementRange:)]
        unsafe fn set_marked_text_selected_range_replacement_range(
            &self,
            string: &AnyObject,
            selected_range: NSRange,
            _replacement_range: NSRange,
        ) {
            let win_id = self.ivars().window_id.get();

            let is_nsstring: bool = msg_send![string, isKindOfClass: NSString::class()];
            let is_attributed: bool = msg_send![string, isKindOfClass: NSAttributedString::class()];

            let text_str = if is_nsstring {
                let ns_str: &NSString = &*(string as *const AnyObject as *const NSString);
                ns_str.to_string()
            } else if is_attributed {
                let attr_str: &NSAttributedString =
                    &*(string as *const AnyObject as *const NSAttributedString);
                attr_str.string().to_string()
            } else {
                String::new()
            };

            *self.ivars().marked_text.borrow_mut() = text_str.clone();
            self.ivars().marked_selection.set(selected_range);

            let cursor = if selected_range.location != NSNotFound as usize {
                Some(selected_range.location..(selected_range.location + selected_range.length))
            } else {
                None
            };

            crate::macos::app::dispatch_event(
                win_id,
                PlatformEvent::ImeCompose {
                    text: SmallStr::from(text_str.as_str()),
                    cursor,
                },
            );
            self.ivars().key_down_handled.set(true);
        }

        #[method(unmarkText)]
        unsafe fn unmark_text(&self) {
            self.ivars().marked_text.borrow_mut().clear();
            self.ivars().marked_selection.set(NSRange::new(NSNotFound as usize, 0));
        }

        #[method(selectedRange)]
        unsafe fn selected_range(&self) -> NSRange {
            self.ivars().marked_selection.get()
        }

        #[method(markedRange)]
        unsafe fn marked_range(&self) -> NSRange {
            let len = self.ivars().marked_text.borrow().encode_utf16().count();
            if len > 0 {
                NSRange::new(0, len)
            } else {
                NSRange::new(NSNotFound as usize, 0)
            }
        }

        #[method(hasMarkedText)]
        unsafe fn has_marked_text(&self) -> bool {
            !self.ivars().marked_text.borrow().is_empty()
        }

        #[method_id(attributedSubstringForProposedRange:actualRange:)]
        unsafe fn attributed_substring_for_proposed_range_actual_range(
            &self,
            range: NSRange,
            actual_range: NSRangePointer,
        ) -> Option<Retained<NSAttributedString>> {
            if !actual_range.is_null() {
                // SAFETY: Pointer was checked non-null before writing.
                unsafe { *actual_range = range };
            }
            None
        }

        #[method_id(validAttributesForMarkedText)]
        unsafe fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            NSArray::new()
        }

        #[method(firstRectForCharacterRange:actualRange:)]
        unsafe fn first_rect_for_character_range_actual_range(
            &self,
            range: NSRange,
            actual_range: NSRangePointer,
        ) -> NSRect {
            if !actual_range.is_null() {
                // SAFETY: Pointer was checked non-null before writing.
                unsafe { *actual_range = range };
            }
            self.ivars().ime_cursor_rect.get()
        }

        #[method(characterIndexForPoint:)]
        unsafe fn character_index_for_point(&self, _point: NSPoint) -> usize {
            0
        }
    }

    unsafe impl NSDraggingDestination for TerminView {
        #[method(draggingEntered:)]
        unsafe fn dragging_entered(
            &self,
            sender: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> NSDragOperation {
            let pboard = sender.draggingPasteboard();
            let types: Retained<NSArray<NSString>> = unsafe {
                // SAFETY: NSPasteboardTypeFileURL is a valid global NSString constant.
                msg_send_id![objc2::class!(NSArray), arrayWithObject: NSPasteboardTypeFileURL]
            };
            if pboard.availableTypeFromArray(&types).is_some() {
                NSDragOperation::Copy
            } else {
                NSDragOperation::empty()
            }
        }

        #[method(performDragOperation:)]
        unsafe fn perform_drag_operation(
            &self,
            sender: &ProtocolObject<dyn NSDraggingInfo>,
        ) -> bool {
            let pboard = sender.draggingPasteboard();
            let win_id = self.ivars().window_id.get();

            let mut paths = Vec::new();
            if let Some(plist) = pboard.propertyListForType(NSFilenamesPboardType) {
                let is_array: bool = msg_send![&*plist, isKindOfClass: NSArray::<NSString>::class()];
                if is_array {
                    let array: &NSArray<NSString> =
                        &*(&*plist as *const AnyObject as *const NSArray<NSString>);
                    for i in 0..array.count() {
                        let item = array.objectAtIndex(i);
                        paths.push(PathBuf::from(item.to_string()));
                    }
                }
            } else if let Some(url_str) = pboard.stringForType(NSPasteboardTypeFileURL) {
                if let Some(url) = NSURL::URLWithString(&url_str) {
                    if let Some(path) = url.path() {
                        paths.push(PathBuf::from(path.to_string()));
                    }
                }
            }

            if !paths.is_empty() {
                crate::macos::app::dispatch_event(
                    win_id,
                    PlatformEvent::FileDrop(paths),
                );
                true
            } else {
                false
            }
        }
    }
);

impl TerminView {
    /// Create a new `TerminView` initialized with the given frame.
    pub fn new(frame: NSRect, win_id: WindowId, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(TerminViewIvars {
            window_id: Cell::new(win_id),
            tracking_area: RefCell::new(None),
            drag_regions: RefCell::new(Vec::new()),
            dblclick_regions: RefCell::new(Vec::new()),
            marked_text: RefCell::new(String::new()),
            marked_selection: Cell::new(NSRange::new(NSNotFound as usize, 0)),
            ime_cursor_rect: Cell::new(NSRect::ZERO),
            in_key_down: RefCell::new(None),
            key_down_handled: Cell::new(false),
            accessible_text: RefCell::new(String::new()),
            accessibility_tree: RefCell::new(
                crate::accessibility::AccessibilityTreeSnapshot::empty(),
            ),
            last_accessibility_notify: RefCell::new(None),
        });

        // SAFETY: Super initWithFrame: on NSView.
        let view: Retained<Self> = unsafe { msg_send_id![super(this), initWithFrame: frame] };

        // SAFETY: Setting view properties on main thread.
        unsafe {
            view.setWantsLayer(true);
            view.setLayerContentsRedrawPolicy(
                NSViewLayerContentsRedrawPolicy::NSViewLayerContentsRedrawDuringViewResize,
            );
            let types: Retained<NSArray<NSString>> =
                msg_send_id![objc2::class!(NSArray), arrayWithObject: NSPasteboardTypeFileURL];
            view.registerForDraggedTypes(&types);
        }

        view
    }

    /// Set the IME composition rectangle in view coordinates.
    pub fn set_ime_rect(&self, r: Rect) {
        let view_rect = NSRect::new(
            NSPoint::new(r.x as CGFloat, r.y as CGFloat),
            NSSize::new(r.w as CGFloat, r.h as CGFloat),
        );
        let win_rect = self.convertRect_toView(view_rect, None);
        if let Some(win) = self.window() {
            let screen_rect = win.convertRectToScreen(win_rect);
            self.ivars().ime_cursor_rect.set(screen_rect);
        }
    }

    /// Set window drag regions in view coordinates.
    pub fn set_drag_regions(&self, regions: Vec<Rect>) {
        *self.ivars().drag_regions.borrow_mut() = regions;
    }

    /// Set regions (inside drag regions) where a double-click is forwarded to the app.
    pub fn set_dblclick_regions(&self, regions: Vec<Rect>) {
        *self.ivars().dblclick_regions.borrow_mut() = regions;
    }

    /// Whether `pt` lies in an app-handled double-click region.
    pub fn is_in_dblclick_region(&self, pt: Point) -> bool {
        self.ivars()
            .dblclick_regions
            .borrow()
            .iter()
            .any(|r| pt.x >= r.x && pt.x <= (r.x + r.w) && pt.y >= r.y && pt.y <= (r.y + r.h))
    }

    /// Check if a point is within any configured window drag regions.
    pub fn is_in_drag_region(&self, pt: Point) -> bool {
        let regions = self.ivars().drag_regions.borrow();
        if regions.is_empty() {
            // Default 38 pt titlebar drag region
            let bounds = self.bounds();
            return pt.y >= 0.0
                && pt.y <= 38.0
                && pt.x >= 0.0
                && pt.x <= (bounds.size.width as f32);
        }
        for r in regions.iter() {
            if pt.x >= r.x && pt.x <= (r.x + r.w) && pt.y >= r.y && pt.y <= (r.y + r.h) {
                return true;
            }
        }
        false
    }

    /// Update exposed accessibility text and post throttled notification.
    pub fn set_accessible_text(&self, text: &str) {
        *self.ivars().accessible_text.borrow_mut() = text.to_string();
        let mut last_notify = self.ivars().last_accessibility_notify.borrow_mut();
        post_value_changed_notification(self.as_ref(), &mut last_notify);
    }

    /// Update exposed accessibility tree and sync terminal text.
    pub fn set_accessibility_tree(&self, tree: crate::accessibility::AccessibilityTreeSnapshot) {
        if let Some(text) = tree.terminal_output_text() {
            *self.ivars().accessible_text.borrow_mut() = text.to_string();
            let mut last_notify = self.ivars().last_accessibility_notify.borrow_mut();
            post_value_changed_notification(self.as_ref(), &mut last_notify);
        }
        *self.ivars().accessibility_tree.borrow_mut() = tree;
    }

    /// Post a VoiceOver announcement.
    pub fn announce(&self, message: &str) {
        crate::macos::accessibility::post_accessibility_announcement(self.as_ref(), message);
    }
}
