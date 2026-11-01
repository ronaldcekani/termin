//! Workspace actions controller.

use crate::action::Action;
use crate::model::AppModel;

pub fn handle(model: &mut AppModel, action: &Action) -> bool {
    match action {
        Action::NewWorkspace => {
            if let Some(win) = model.focused_window() {
                model.new_workspace(win, None);
                true
            } else {
                false
            }
        }
        Action::CloseWorkspace => {
            if let Some(ws) = model.focused_workspace() {
                model.close_workspace(ws);
                true
            } else {
                false
            }
        }
        Action::NextWorkspace => model.next_workspace(),
        Action::PrevWorkspace => model.prev_workspace(),
        Action::SelectWorkspace(idx) => {
            if let Some(win_id) = model.focused_window() {
                if let Some(win) = model.windows.get(win_id) {
                    if *idx >= 1 && *idx <= win.workspaces.len() {
                        let ws_id = win.workspaces[*idx - 1];
                        return model.select_workspace(ws_id);
                    }
                }
            }
            false
        }
        Action::MoveWorkspace { to_window, index } => {
            if let Some(ws) = model.focused_workspace() {
                model.move_workspace(ws, *to_window, *index)
            } else {
                false
            }
        }
        Action::RenameWorkspace(name) => {
            if let Some(ws) = model.focused_workspace() {
                match name {
                    // `None` opens the inline editor.
                    None => model.begin_rename_workspace(ws),
                    Some(_) => model.rename_workspace(ws, name.clone()),
                }
            } else {
                false
            }
        }
        Action::ToggleSidebar => {
            if let Some(win) = model.focused_window() {
                model.toggle_sidebar(win);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}
