//! macOS native menu generation from MenuSpec.
//!
//! See `docs/specs/platform-macos.md §11`.

use crate::macos::input::parse_shortcut;
use crate::{ActionId, MenuItemSpec, MenuSpec};
use objc2::rc::Retained;
use objc2::sel;
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSString};
use rustc_hash::FxHashMap;

/// Convert a `MenuSpec` tree into an AppKit `NSMenu` hierarchy.
///
/// Maps action IDs to numeric tags on `NSMenuItem`s.
pub fn build_native_menu(
    menu_spec: &MenuSpec,
    action_map: &mut FxHashMap<u32, ActionId>,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    // SAFETY: Creating and configuring native NSMenu on the main thread.
    unsafe {
        let title_ns = NSString::from_str(&menu_spec.title);
        let menu = NSMenu::new(mtm);
        menu.setTitle(&title_ns);
        menu.setAutoenablesItems(false);

        for item_spec in &menu_spec.items {
            let item = build_native_menu_item(item_spec, action_map, mtm);
            menu.addItem(&item);
        }

        menu
    }
}

/// Convert a single `MenuItemSpec` to an `NSMenuItem`.
pub fn build_native_menu_item(
    item_spec: &MenuItemSpec,
    action_map: &mut FxHashMap<u32, ActionId>,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    // SAFETY: Initializing NSMenuItem on the main thread.
    unsafe {
        let title_ns = NSString::from_str(&item_spec.title);
        let key_equiv = item_spec.key_equivalent.as_deref().unwrap_or("");
        let (key_str, modifier_flags) = parse_shortcut(key_equiv);
        let key_ns = NSString::from_str(&key_str);

        let item = NSMenuItem::new(mtm);
        item.setTitle(&title_ns);
        item.setKeyEquivalent(&key_ns);
        item.setKeyEquivalentModifierMask(modifier_flags);
        item.setEnabled(true);

        if let Some(action_id) = item_spec.action {
            let tag = action_id.0;
            item.setTag(tag as isize);
            action_map.insert(tag, action_id);
            item.setAction(Some(sel!(menuAction:)));
        }

        if let Some(submenu_spec) = &item_spec.submenu {
            let submenu = build_native_menu(submenu_spec, action_map, mtm);
            item.setSubmenu(Some(&submenu));
        }

        item
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_menu_spec_conversion() {
        if let Some(mtm) = MainThreadMarker::new() {
            let mut action_map = FxHashMap::default();
            let spec = MenuSpec {
                title: "Shell".into(),
                items: vec![
                    MenuItemSpec {
                        title: "New Window".into(),
                        action: Some(ActionId(1)),
                        key_equivalent: Some("⌘⇧N".into()),
                        submenu: None,
                    },
                    MenuItemSpec {
                        title: "New Tab".into(),
                        action: Some(ActionId(2)),
                        key_equivalent: Some("⌘T".into()),
                        submenu: None,
                    },
                    MenuItemSpec {
                        title: "Submenu".into(),
                        action: None,
                        key_equivalent: None,
                        submenu: Some(MenuSpec {
                            title: "Nested".into(),
                            items: vec![MenuItemSpec {
                                title: "Nested Item".into(),
                                action: Some(ActionId(3)),
                                key_equivalent: Some("Cmd+K".into()),
                                submenu: None,
                            }],
                        }),
                    },
                ],
            };

            let menu = build_native_menu(&spec, &mut action_map, mtm);
            assert_eq!(action_map.len(), 3);
            assert_eq!(action_map.get(&1), Some(&ActionId(1)));
            assert_eq!(action_map.get(&2), Some(&ActionId(2)));
            assert_eq!(action_map.get(&3), Some(&ActionId(3)));

            // SAFETY: Verifying item count on the created menu.
            unsafe {
                assert_eq!(menu.numberOfItems(), 3);
            }
        }
    }
}
