//! Panes and split actions controller.

use crate::action::Action;
use crate::model::{AppModel, Dir};

pub fn handle(model: &mut AppModel, action: &Action) -> bool {
    match action {
        Action::SplitRight => {
            if let Some(pane) = model.focused_pane() {
                model.split_pane(pane, Dir::Right, true).is_some()
            } else {
                false
            }
        }
        Action::SplitDown => {
            if let Some(pane) = model.focused_pane() {
                model.split_pane(pane, Dir::Down, true).is_some()
            } else {
                false
            }
        }
        Action::ClosePane => {
            if let Some(pane) = model.focused_pane() {
                model.close_pane(pane)
            } else {
                false
            }
        }
        Action::FocusPane(dir) => model.focus_pane(*dir),
        Action::FocusNextPane => model.focus_next_pane(),
        Action::FocusPrevPane => model.focus_prev_pane(),
        Action::ResizeSplit { dir, percent } => {
            if let Some(pane) = model.focused_pane() {
                model.resize_split(pane, *dir, *percent)
            } else {
                false
            }
        }
        Action::EqualizeSplits => model.equalize_splits(),
        Action::ZoomPane => model.zoom_pane(),
        Action::SwapPane(dir) => model.swap_pane(*dir),
        _ => false,
    }
}
