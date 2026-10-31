//! Application model, actions, and main thread controller (`termin-app`).
//!
//! See `docs/architecture.md §5` and `docs/specs/app-model-and-ui.md`.

use std::sync::Arc;
use termin_config::Config;
use termin_core::{PaneId, Slab, TabId, WindowId, WorkspaceId};
use termin_platform::{ActionId, AppHandler, PlatformCx, PlatformEvent, RenderTarget};
use termin_pty::Session;
use termin_vt::Terminal;

/// User or programmatic actions dispatched across the application.
///
/// See `docs/architecture.md §5` and `docs/specs/app-model-and-ui.md §6`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Action {
    NewWindow,
    NewWorkspace,
    NewTab,
    SplitRight,
    SplitDown,
    CloseTab,
    ClosePane,
    CloseWorkspace,
    FocusNextPane,
    FocusPrevPane,
    SelectTab(usize),
    SelectWorkspace(usize),
    SendText(String),
}

/// Tab state owning terminal session and editor.
///
/// See `docs/architecture.md §5`.
pub struct Tab {
    pub id: TabId,
    pub session: Arc<Session<Terminal>>,
    pub title: String,
}

/// Pane state holding tabs.
///
/// See `docs/architecture.md §5`.
pub struct Pane {
    pub id: PaneId,
    pub tabs: Vec<TabId>,
    pub active_tab: usize,
}

/// Workspace state holding a split tree of panes.
///
/// See `docs/architecture.md §5`.
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub panes: Vec<PaneId>,
    pub focused_pane: Option<PaneId>,
}

/// Window state holding workspaces.
///
/// See `docs/architecture.md §5`.
pub struct WindowState {
    pub id: WindowId,
    pub workspaces: Vec<WorkspaceId>,
    pub active_workspace: usize,
    pub sidebar_width: f32,
    pub sidebar_visible: bool,
}

/// Main application state owned on the main thread.
///
/// See `docs/architecture.md §5`.
pub struct App {
    pub windows: Slab<WindowState>,
    pub workspaces: Slab<Workspace>,
    pub panes: Slab<Pane>,
    pub tabs: Slab<Tab>,
    pub config: Arc<Config>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self {
            windows: Slab::new(),
            workspaces: Slab::new(),
            panes: Slab::new(),
            tabs: Slab::new(),
            config: Arc::new(Config::default()),
        }
    }
}

impl AppHandler for App {
    fn init(&mut self, cx: &mut PlatformCx) {
        let win = cx.create_window(&termin_platform::WindowOpts::default());
        self.windows.insert(WindowState {
            id: win,
            workspaces: Vec::new(),
            active_workspace: 0,
            sidebar_width: 240.0,
            sidebar_visible: true,
        });
    }

    fn event(&mut self, _cx: &mut PlatformCx, _win: WindowId, _ev: PlatformEvent) {}

    fn frame(&mut self, _cx: &mut PlatformCx, _win: WindowId, _target: &mut dyn RenderTarget) {}

    fn wake(&mut self, _cx: &mut PlatformCx) {}

    fn menu_action(&mut self, _cx: &mut PlatformCx, _action: ActionId) {}

    fn should_terminate(&mut self, _cx: &mut PlatformCx) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_creation() {
        let app = App::new();
        assert_eq!(app.windows.len(), 0);
    }
}
