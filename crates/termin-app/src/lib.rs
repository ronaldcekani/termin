//! Application model, actions, and main thread controller (`termin-app`).
//!
//! See `docs/architecture.md §5` and `docs/specs/app-model-and-ui.md`.

pub mod action;
pub mod app;
pub mod block_mode;
pub mod controllers;
pub mod dispatch;
pub mod frame;
pub mod input;
pub mod keymap;
pub mod model;
pub mod run;
pub mod session_ctl;

pub use action::{Action, ActionInfo, ActionParseError, Area};
pub use app::App;
pub use dispatch::dispatch;
pub use keymap::{Binding, Keymap, WhenContext};
pub use model::{
    AppModel, ModelEffect, Pane, SplitTree, Tab, TabMode, TabTitle, WindowState, Workspace,
    WorkspaceLayout, WorkspaceMeta,
};
pub use run::run;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let app = App::default();
        assert_eq!(app.model.windows.len(), 0);
    }
}
