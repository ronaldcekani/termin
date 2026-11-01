//! Action dispatcher routing actions to subsystem controllers (`termin-app::dispatch`).
//!
//! See `docs/architecture.md §5` and `docs/specs/app-model-and-ui.md §7`.

use crate::action::{Action, Area};
use crate::controllers;
use crate::model::AppModel;

/// Dispatch an action to its responsible subsystem controller.
///
/// Returns `true` if the action caused state changes that may require a frame redraw.
pub fn dispatch(model: &mut AppModel, action: &Action) -> bool {
    match action.area() {
        Area::Windows => controllers::windows::handle(model, action),
        Area::Workspaces => controllers::workspaces::handle(model, action),
        Area::Panes => controllers::panes::handle(model, action),
        Area::Tabs => controllers::tabs::handle(model, action),
        Area::Terminal => controllers::terminal::handle(model, action),
        Area::Blocks => controllers::blocks::handle(model, action),
        Area::Editor => controllers::editor::handle(model, action),
        Area::Palette => false, // Stub for Phase 3C palette
        Area::Find => controllers::find::handle(model, action),
        Area::Attention => controllers::attention::handle(model, action),
        Area::Config => {
            if matches!(action, Action::InstallCli) {
                controllers::cli_install::handle(model, action)
            } else {
                controllers::config::handle(model, action)
            }
        }
        Area::Input => {
            if let Action::SendText(text) = action {
                if let Some(session) = model.focused_session() {
                    session.write(text.as_bytes());
                    return true;
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::Size;

    #[test]
    fn dispatch_routes_actions() {
        let mut model = AppModel::new();
        model.new_window(None, Size::new(800.0, 600.0), None);

        assert!(dispatch(&mut model, &Action::SplitRight));
        assert_eq!(model.panes.len(), 2);

        assert!(dispatch(&mut model, &Action::NewWorkspace));
        assert_eq!(model.workspaces.len(), 2);

        assert!(dispatch(&mut model, &Action::CloseWorkspace));
        assert_eq!(model.workspaces.len(), 1);
    }
}
