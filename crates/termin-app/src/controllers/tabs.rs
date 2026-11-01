//! Tab actions controller.

use crate::action::Action;
use crate::model::AppModel;

pub fn handle(model: &mut AppModel, action: &Action) -> bool {
    match action {
        Action::NewTab => {
            if let Some(pane) = model.focused_pane() {
                model.new_tab(pane, true).is_some()
            } else {
                false
            }
        }
        Action::CloseTab => {
            if let Some(tab) = model.focused_tab() {
                model.close_tab(tab)
            } else {
                false
            }
        }
        Action::NextTab => {
            if let Some(pane) = model.focused_pane() {
                model.next_tab(pane)
            } else {
                false
            }
        }
        Action::PrevTab => {
            if let Some(pane) = model.focused_pane() {
                model.prev_tab(pane)
            } else {
                false
            }
        }
        Action::SelectTab(idx) => {
            if let Some(pane) = model.focused_pane() {
                if *idx >= 1 {
                    model.select_tab(pane, *idx - 1)
                } else {
                    false
                }
            } else {
                false
            }
        }
        Action::RenameTab(name) => {
            if let Some(tab) = model.focused_tab() {
                match name {
                    None => model.begin_rename_tab(tab),
                    Some(_) => model.rename_tab(tab, name.clone()),
                }
            } else {
                false
            }
        }
        Action::MoveTab { to_pane, index } => {
            if let Some(tab) = model.focused_tab() {
                model.move_tab(tab, *to_pane, *index)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Complete a drag-tab-to-split operation (`components.md §15`).
///
/// When `zone` is `Center`, moves `tab` into `target_pane`. When `zone` is one of
/// the directional edges (`Left`, `Right`, `Top`, `Bottom`), splits `target_pane`
/// along that axis and moves `tab` into the newly created split pane.
pub fn drop_tab_into_split_zone(
    model: &mut AppModel,
    tab: termin_core::TabId,
    target_pane: termin_core::PaneId,
    zone: termin_ui::widgets::pane::SplitDropZone,
) -> bool {
    use crate::model::{Dir, Direction};
    use termin_ui::widgets::pane::SplitDropZone;

    match zone {
        SplitDropZone::Center => model.move_tab(tab, target_pane, usize::MAX),
        SplitDropZone::Right | SplitDropZone::Left | SplitDropZone::Bottom | SplitDropZone::Top => {
            let dir = match zone {
                SplitDropZone::Left | SplitDropZone::Right => Dir::Right,
                SplitDropZone::Top | SplitDropZone::Bottom => Dir::Down,
                SplitDropZone::Center => unreachable!(),
            };
            let Some((new_pane, _)) = model.split_pane(target_pane, dir, false) else {
                return false;
            };
            if !model.move_tab(tab, new_pane, 0) {
                return false;
            }
            if matches!(zone, SplitDropZone::Left) {
                let _ = model.swap_pane(Direction::Left);
            } else if matches!(zone, SplitDropZone::Top) {
                let _ = model.swap_pane(Direction::Up);
            }
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::Size;
    use termin_ui::widgets::pane::SplitDropZone;

    #[test]
    fn test_drop_tab_into_split_zone_splits_and_moves_tab() {
        let mut model = AppModel::new();
        model.new_window(None, Size::new(800.0, 600.0), None);
        let pane1 = model.focused_pane().unwrap();
        let tab2 = model.new_tab(pane1, true).unwrap();
        assert_eq!(model.panes.get(pane1).unwrap().tabs.len(), 2);

        assert!(drop_tab_into_split_zone(
            &mut model,
            tab2,
            pane1,
            SplitDropZone::Right
        ));
        assert_eq!(model.panes.len(), 2);
        assert_eq!(model.panes.get(pane1).unwrap().tabs.len(), 1);
    }
}
