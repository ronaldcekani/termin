//! Application data model (`termin-app::model`).
//!
//! Hierarchical entity storage, split trees, and pure geometric layout.
//! See `docs/architecture.md §5` and `docs/specs/app-model-and-ui.md §1, §2`.

pub mod arena;
pub mod rename;
pub mod tree;
pub mod types;

pub use arena::{Arena, ArenaId};
pub use rename::{RenameEdit, RenameTarget};
pub use tree::{
    neighbor, Dir, Direction, Divider, Node, NodeIdx, SplitTree, TreeLayout, DIVIDER_HIT,
    DIVIDER_THICKNESS, MAX_RATIO, MIN_RATIO,
};
pub use types::{
    Attention, AttentionKind, Pane, Tab, TabMode, TabTitle, WindowState, Workspace,
    WorkspaceLayout, WorkspaceMeta,
};

use std::sync::Arc;
use termin_core::{PaneId, Size, TabId, WindowId, WorkspaceId};
use termin_persist::{
    PaneSnapshot, SidebarSnapshot, Snapshot, TabSnapshot, TreeSnapshot, WindowSnapshot,
    WorkspaceSnapshot,
};
use termin_pty::Session;
use termin_vt::Terminal;

/// Side effects produced by model mutations that the runtime must execute.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelEffect {
    /// Request PTY spawning for a new tab.
    SpawnTab {
        tab: TabId,
        pane: PaneId,
        cwd: Option<String>,
    },
    /// Close an existing tab session.
    CloseTab { tab: TabId },
    /// Close an OS window.
    CloseWindow { window: WindowId },
}

/// Pure state model holding all windows, workspaces, panes, and tabs.
pub struct AppModel {
    pub windows: Arena<WindowId, WindowState>,
    pub workspaces: Arena<WorkspaceId, Workspace>,
    pub panes: Arena<PaneId, Pane>,
    pub tabs: Arena<TabId, Tab>,
    pub focused_window: Option<WindowId>,
    pub pending_effects: Vec<ModelEffect>,
    /// Active inline rename (workspace or tab), if any.
    pub rename: Option<RenameEdit>,
}

impl Default for AppModel {
    fn default() -> Self {
        Self::new()
    }
}

impl AppModel {
    pub fn new() -> Self {
        Self {
            windows: Arena::new(),
            workspaces: Arena::new(),
            panes: Arena::new(),
            tabs: Arena::new(),
            focused_window: None,
            pending_effects: Vec::new(),
            rename: None,
        }
    }

    /// Drain all pending model side effects.
    pub fn drain_effects(&mut self) -> Vec<ModelEffect> {
        std::mem::take(&mut self.pending_effects)
    }

    // --- Window Operations ---

    /// Create and insert a new window with a default workspace, pane, and tab.
    pub fn new_window(
        &mut self,
        window_id: Option<WindowId>,
        size: Size,
        cwd: Option<String>,
    ) -> WindowId {
        let win_id = match window_id {
            Some(w) => {
                self.windows.insert_at(
                    w,
                    WindowState {
                        id: w,
                        workspaces: Vec::new(),
                        active_workspace: WorkspaceId(0),
                        sidebar_width: 240.0,
                        sidebar_visible: true,
                        size,
                    },
                );
                w
            }
            None => self.windows.insert_with(|w| WindowState {
                id: w,
                workspaces: Vec::new(),
                active_workspace: WorkspaceId(0),
                sidebar_width: 240.0,
                sidebar_visible: true,
                size,
            }),
        };

        let ws_id = self.new_workspace(win_id, cwd);
        if let Some(win) = self.windows.get_mut(win_id) {
            win.active_workspace = ws_id;
        }

        self.focused_window = Some(win_id);
        win_id
    }

    /// Close a window and cascade close its workspaces, panes, and tabs.
    pub fn close_window(&mut self, window_id: WindowId) -> bool {
        let Some(win) = self.windows.remove(window_id) else {
            return false;
        };

        for ws_id in win.workspaces {
            self.remove_workspace_internal(ws_id);
        }

        self.pending_effects
            .push(ModelEffect::CloseWindow { window: window_id });

        if self.focused_window == Some(window_id) {
            self.focused_window = self.windows.ids().next();
        }

        true
    }

    // --- Workspace Operations ---

    /// Create a new workspace in `window_id` with an initial pane and tab.
    pub fn new_workspace(&mut self, window_id: WindowId, cwd: Option<String>) -> WorkspaceId {
        let tab_id = self.tabs.insert_with(|id| Tab {
            id,
            pane: PaneId(0),
            session: None,
            title: TabTitle::default(),
            cwd: cwd.clone(),
            mode: TabMode::Auto,
            attention: Attention::default(),
        });

        let pane_id = self.panes.insert_with(|id| Pane {
            id,
            workspace: WorkspaceId(0),
            tabs: vec![tab_id],
            active_tab: tab_id,
        });

        if let Some(tab) = self.tabs.get_mut(tab_id) {
            tab.pane = pane_id;
        }

        let ws_id = self.workspaces.insert_with(|id| Workspace {
            id,
            window: window_id,
            name: None,
            tree: SplitTree::new(pane_id),
            focused_pane: pane_id,
            zoomed_pane: None,
            focus_history: vec![pane_id],
            meta: WorkspaceMeta::default(),
            layout: WorkspaceLayout::default(),
            layout_key: None,
        });

        if let Some(pane) = self.panes.get_mut(pane_id) {
            pane.workspace = ws_id;
        }

        if let Some(win) = self.windows.get_mut(window_id) {
            win.workspaces.push(ws_id);
            win.active_workspace = ws_id;
        }

        self.pending_effects.push(ModelEffect::SpawnTab {
            tab: tab_id,
            pane: pane_id,
            cwd,
        });

        ws_id
    }

    fn remove_workspace_internal(&mut self, ws_id: WorkspaceId) {
        if let Some(ws) = self.workspaces.remove(ws_id) {
            for pane_id in ws.tree.leaves() {
                if let Some(pane) = self.panes.remove(pane_id) {
                    for tab_id in pane.tabs {
                        self.tabs.remove(tab_id);
                        self.pending_effects
                            .push(ModelEffect::CloseTab { tab: tab_id });
                    }
                }
            }
        }
    }

    /// Close a workspace; if it was the last workspace in its window, closes the window.
    pub fn close_workspace(&mut self, ws_id: WorkspaceId) -> bool {
        let win_id = match self.workspaces.get(ws_id) {
            Some(ws) => ws.window,
            None => return false,
        };

        self.remove_workspace_internal(ws_id);

        if let Some(win) = self.windows.get_mut(win_id) {
            win.workspaces.retain(|w| *w != ws_id);
            if win.workspaces.is_empty() {
                self.close_window(win_id);
            } else if win.active_workspace == ws_id {
                win.active_workspace = win.workspaces[0];
            }
        }

        true
    }

    /// Select a workspace by id.
    pub fn select_workspace(&mut self, ws_id: WorkspaceId) -> bool {
        let Some(ws) = self.workspaces.get(ws_id) else {
            return false;
        };
        let win_id = ws.window;
        if let Some(win) = self.windows.get_mut(win_id) {
            if win.workspaces.contains(&ws_id) {
                win.active_workspace = ws_id;
                self.focused_window = Some(win_id);
                return true;
            }
        }
        false
    }

    /// Next workspace in the current window (wraps).
    pub fn next_workspace(&mut self) -> bool {
        let Some(win_id) = self.focused_window else {
            return false;
        };
        let Some(win) = self.windows.get_mut(win_id) else {
            return false;
        };
        if win.workspaces.len() <= 1 {
            return false;
        }
        let pos = win
            .workspaces
            .iter()
            .position(|w| *w == win.active_workspace)
            .unwrap_or(0);
        let next_pos = (pos + 1) % win.workspaces.len();
        win.active_workspace = win.workspaces[next_pos];
        true
    }

    /// Previous workspace in the current window (wraps).
    pub fn prev_workspace(&mut self) -> bool {
        let Some(win_id) = self.focused_window else {
            return false;
        };
        let Some(win) = self.windows.get_mut(win_id) else {
            return false;
        };
        if win.workspaces.len() <= 1 {
            return false;
        }
        let pos = win
            .workspaces
            .iter()
            .position(|w| *w == win.active_workspace)
            .unwrap_or(0);
        let prev_pos = if pos == 0 {
            win.workspaces.len() - 1
        } else {
            pos - 1
        };
        win.active_workspace = win.workspaces[prev_pos];
        true
    }

    /// Move workspace to another window at `index`.
    pub fn move_workspace(
        &mut self,
        ws_id: WorkspaceId,
        to_window: WindowId,
        index: usize,
    ) -> bool {
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };
        let from_window = ws.window;
        if !self.windows.contains(to_window) {
            return false;
        }

        ws.window = to_window;

        if let Some(from_win) = self.windows.get_mut(from_window) {
            from_win.workspaces.retain(|w| *w != ws_id);
            if from_win.workspaces.is_empty() {
                self.close_window(from_window);
            } else if from_win.active_workspace == ws_id {
                from_win.active_workspace = from_win.workspaces[0];
            }
        }

        if let Some(to_win) = self.windows.get_mut(to_window) {
            let idx = index.min(to_win.workspaces.len());
            to_win.workspaces.insert(idx, ws_id);
            to_win.active_workspace = ws_id;
        }

        true
    }

    /// Rename a workspace (`None` or blank resets to auto; the inline editor is
    /// started with [`Self::begin_rename_workspace`]).
    pub fn rename_workspace(&mut self, ws_id: WorkspaceId, name: Option<String>) -> bool {
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };
        ws.name = match name {
            Some(s) if s.trim().is_empty() => None,
            other => other,
        };
        true
    }

    /// Toggle sidebar visibility for `window_id`.
    pub fn toggle_sidebar(&mut self, window_id: WindowId) {
        if let Some(win) = self.windows.get_mut(window_id) {
            win.sidebar_visible = !win.sidebar_visible;
        }
    }

    // --- Tab Operations ---

    /// Create a new tab in `pane_id`.
    pub fn new_tab(&mut self, pane_id: PaneId, inherit_cwd: bool) -> Option<TabId> {
        let pane = self.panes.get_mut(pane_id)?;
        let active_tab_id = pane.active_tab;
        let cwd = if inherit_cwd {
            self.tabs.get(active_tab_id).and_then(|t| t.cwd.clone())
        } else {
            None
        };

        let tab_id = self.tabs.insert_with(|id| Tab {
            id,
            pane: pane_id,
            session: None,
            title: TabTitle::default(),
            cwd: cwd.clone(),
            mode: TabMode::Auto,
            attention: Attention::default(),
        });

        let pane = self.panes.get_mut(pane_id).expect("pane exists");
        pane.tabs.push(tab_id);
        pane.active_tab = tab_id;

        self.pending_effects.push(ModelEffect::SpawnTab {
            tab: tab_id,
            pane: pane_id,
            cwd,
        });

        Some(tab_id)
    }

    /// Close tab `tab_id`. If it's the last tab in its pane, closes the pane.
    pub fn close_tab(&mut self, tab_id: TabId) -> bool {
        let Some(tab) = self.tabs.remove(tab_id) else {
            return false;
        };
        let pane_id = tab.pane;

        self.pending_effects
            .push(ModelEffect::CloseTab { tab: tab_id });

        let Some(pane) = self.panes.get_mut(pane_id) else {
            return true;
        };

        pane.tabs.retain(|t| *t != tab_id);
        if pane.tabs.is_empty() {
            self.close_pane(pane_id);
        } else if pane.active_tab == tab_id {
            pane.active_tab = pane.tabs[pane.tabs.len() - 1];
        }

        true
    }

    /// Select tab `n` (0-based) in `pane_id`.
    pub fn select_tab(&mut self, pane_id: PaneId, idx: usize) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id) else {
            return false;
        };
        if idx < pane.tabs.len() {
            pane.active_tab = pane.tabs[idx];
            true
        } else {
            false
        }
    }

    /// Next tab in `pane_id` (wraps).
    pub fn next_tab(&mut self, pane_id: PaneId) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id) else {
            return false;
        };
        if pane.tabs.len() <= 1 {
            return false;
        }
        let pos = pane
            .tabs
            .iter()
            .position(|t| *t == pane.active_tab)
            .unwrap_or(0);
        let next_pos = (pos + 1) % pane.tabs.len();
        pane.active_tab = pane.tabs[next_pos];
        true
    }

    /// Previous tab in `pane_id` (wraps).
    pub fn prev_tab(&mut self, pane_id: PaneId) -> bool {
        let Some(pane) = self.panes.get_mut(pane_id) else {
            return false;
        };
        if pane.tabs.len() <= 1 {
            return false;
        }
        let pos = pane
            .tabs
            .iter()
            .position(|t| *t == pane.active_tab)
            .unwrap_or(0);
        let prev_pos = if pos == 0 {
            pane.tabs.len() - 1
        } else {
            pos - 1
        };
        pane.active_tab = pane.tabs[prev_pos];
        true
    }

    /// Rename tab.
    pub fn rename_tab(&mut self, tab_id: TabId, name: Option<String>) -> bool {
        let Some(tab) = self.tabs.get_mut(tab_id) else {
            return false;
        };
        tab.title.user = match name {
            Some(s) if s.trim().is_empty() => None,
            other => other,
        };
        true
    }

    /// Move tab to another pane at `index`.
    pub fn move_tab(&mut self, tab_id: TabId, to_pane: PaneId, index: usize) -> bool {
        let Some(tab) = self.tabs.get_mut(tab_id) else {
            return false;
        };
        let from_pane = tab.pane;
        if !self.panes.contains(to_pane) {
            return false;
        }

        tab.pane = to_pane;

        if let Some(from_p) = self.panes.get_mut(from_pane) {
            from_p.tabs.retain(|t| *t != tab_id);
            if from_p.tabs.is_empty() {
                self.close_pane(from_pane);
            } else if from_p.active_tab == tab_id {
                from_p.active_tab = from_p.tabs[0];
            }
        }

        if let Some(to_p) = self.panes.get_mut(to_pane) {
            let idx = index.min(to_p.tabs.len());
            to_p.tabs.insert(idx, tab_id);
            to_p.active_tab = tab_id;
        }

        true
    }

    // --- Pane & Split Operations ---

    /// Split `target_pane` in `dir`, creating a new pane and tab.
    pub fn split_pane(
        &mut self,
        target_pane: PaneId,
        dir: Dir,
        inherit_cwd: bool,
    ) -> Option<(PaneId, TabId)> {
        let target = self.panes.get(target_pane)?;
        let ws_id = target.workspace;
        let active_tab_id = target.active_tab;
        let cwd = if inherit_cwd {
            self.tabs.get(active_tab_id).and_then(|t| t.cwd.clone())
        } else {
            None
        };

        let new_tab_id = self.tabs.insert_with(|id| Tab {
            id,
            pane: PaneId(0),
            session: None,
            title: TabTitle::default(),
            cwd: cwd.clone(),
            mode: TabMode::Auto,
            attention: Attention::default(),
        });

        let new_pane_id = self.panes.insert_with(|id| Pane {
            id,
            workspace: ws_id,
            tabs: vec![new_tab_id],
            active_tab: new_tab_id,
        });

        if let Some(tab) = self.tabs.get_mut(new_tab_id) {
            tab.pane = new_pane_id;
        }

        let ws = self.workspaces.get_mut(ws_id)?;
        if !ws.tree.split(target_pane, dir, new_pane_id) {
            self.panes.remove(new_pane_id);
            self.tabs.remove(new_tab_id);
            return None;
        }

        ws.focused_pane = new_pane_id;
        ws.focus_history.retain(|p| *p != new_pane_id);
        ws.focus_history.insert(0, new_pane_id);

        self.pending_effects.push(ModelEffect::SpawnTab {
            tab: new_tab_id,
            pane: new_pane_id,
            cwd,
        });

        Some((new_pane_id, new_tab_id))
    }

    /// Close a pane and its tabs; sibling takes its place in the split tree.
    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        let Some(pane) = self.panes.remove(pane_id) else {
            return false;
        };
        let ws_id = pane.workspace;

        for tab_id in pane.tabs {
            self.tabs.remove(tab_id);
            self.pending_effects
                .push(ModelEffect::CloseTab { tab: tab_id });
        }

        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return true;
        };

        if ws.tree.len() <= 1 {
            // Last pane in workspace -> close workspace
            self.close_workspace(ws_id);
            return true;
        }

        let next_focus = ws.tree.remove(pane_id);
        if let Some(next) = next_focus {
            if ws.focused_pane == pane_id {
                ws.focused_pane = next;
            }
        }

        ws.focus_history.retain(|p| *p != pane_id);
        if ws.zoomed_pane == Some(pane_id) {
            ws.zoomed_pane = None;
        }

        true
    }

    /// Move focus geometrically in `dir`.
    pub fn focus_pane(&mut self, dir: Direction) -> bool {
        let Some(ws_id) = self.focused_workspace() else {
            return false;
        };
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };

        let current = ws.focused_pane;
        let next = neighbor(&ws.layout.panes, current, dir, &ws.focus_history);
        if let Some(target) = next {
            ws.focused_pane = target;
            ws.focus_history.retain(|p| *p != target);
            ws.focus_history.insert(0, target);
            true
        } else {
            false
        }
    }

    /// Focus next pane in reading order.
    pub fn focus_next_pane(&mut self) -> bool {
        let Some(ws_id) = self.focused_workspace() else {
            return false;
        };
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };
        let leaves = ws.tree.leaves();
        if leaves.len() <= 1 {
            return false;
        }
        let pos = leaves
            .iter()
            .position(|p| *p == ws.focused_pane)
            .unwrap_or(0);
        let next_pos = (pos + 1) % leaves.len();
        let target = leaves[next_pos];
        ws.focused_pane = target;
        ws.focus_history.retain(|p| *p != target);
        ws.focus_history.insert(0, target);
        true
    }

    /// Focus previous pane in reading order.
    pub fn focus_prev_pane(&mut self) -> bool {
        let Some(ws_id) = self.focused_workspace() else {
            return false;
        };
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };
        let leaves = ws.tree.leaves();
        if leaves.len() <= 1 {
            return false;
        }
        let pos = leaves
            .iter()
            .position(|p| *p == ws.focused_pane)
            .unwrap_or(0);
        let prev_pos = if pos == 0 { leaves.len() - 1 } else { pos - 1 };
        let target = leaves[prev_pos];
        ws.focused_pane = target;
        ws.focus_history.retain(|p| *p != target);
        ws.focus_history.insert(0, target);
        true
    }

    /// Resize nearest ancestor split of `pane_id` in `dir` by `percent`.
    pub fn resize_split(&mut self, pane_id: PaneId, dir: Direction, percent: u8) -> bool {
        let Some(pane) = self.panes.get(pane_id) else {
            return false;
        };
        let Some(ws) = self.workspaces.get_mut(pane.workspace) else {
            return false;
        };
        let delta = percent as f32 / 100.0;
        ws.tree.resize(pane_id, dir, delta)
    }

    /// Equalize all splits in the active workspace.
    pub fn equalize_splits(&mut self) -> bool {
        let Some(ws_id) = self.focused_workspace() else {
            return false;
        };
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };
        ws.tree.equalize();
        true
    }

    /// Toggle zoom of focused pane.
    pub fn zoom_pane(&mut self) -> bool {
        let Some(ws_id) = self.focused_workspace() else {
            return false;
        };
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };
        ws.zoomed_pane = if ws.zoomed_pane.is_some() {
            None
        } else {
            Some(ws.focused_pane)
        };
        true
    }

    /// Swap leaves with neighbor.
    pub fn swap_pane(&mut self, dir: Direction) -> bool {
        let Some(ws_id) = self.focused_workspace() else {
            return false;
        };
        let Some(ws) = self.workspaces.get_mut(ws_id) else {
            return false;
        };
        let current = ws.focused_pane;
        let next = neighbor(&ws.layout.panes, current, dir, &ws.focus_history);
        if let Some(target) = next {
            ws.tree.swap_leaves(current, target)
        } else {
            false
        }
    }

    // --- Layout computation ---

    /// Recompute and cache layout for `win_id`.
    pub fn layout(&mut self, win_id: WindowId, min_size: Size) -> Option<&WorkspaceLayout> {
        let win = self.windows.get(win_id)?;
        let content_rect = win.content_rect();
        let ws_id = win.active_workspace;
        let ws = self.workspaces.get_mut(ws_id)?;

        let key = types::LayoutKey {
            area: content_rect,
            min: min_size,
            zoomed: ws.zoomed_pane,
            rev: ws.tree.rev(),
        };

        if ws.layout_key.as_ref() != Some(&key) {
            if let Some(zoomed) = ws.zoomed_pane {
                ws.layout = WorkspaceLayout {
                    area: content_rect,
                    panes: vec![(zoomed, content_rect)],
                    dividers: Vec::new(),
                };
            } else {
                let tree_layout = ws.tree.layout(content_rect, min_size);
                ws.layout = WorkspaceLayout {
                    area: content_rect,
                    panes: tree_layout.panes,
                    dividers: tree_layout.dividers,
                };
            }
            ws.layout_key = Some(key);
        }

        Some(&ws.layout)
    }

    // --- Focus / Context Accessors ---

    #[inline]
    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused_window.or_else(|| self.windows.ids().next())
    }

    #[inline]
    pub fn focused_workspace(&self) -> Option<WorkspaceId> {
        let win_id = self.focused_window()?;
        let win = self.windows.get(win_id)?;
        Some(win.active_workspace)
    }

    #[inline]
    pub fn focused_pane(&self) -> Option<PaneId> {
        let ws_id = self.focused_workspace()?;
        let ws = self.workspaces.get(ws_id)?;
        Some(ws.focused_pane)
    }

    #[inline]
    pub fn focused_tab(&self) -> Option<TabId> {
        let pane_id = self.focused_pane()?;
        let pane = self.panes.get(pane_id)?;
        Some(pane.active_tab)
    }

    #[inline]
    pub fn focused_session(&self) -> Option<Arc<Session<Terminal>>> {
        let tab_id = self.focused_tab()?;
        let tab = self.tabs.get(tab_id)?;
        tab.session.clone()
    }

    // --- Snapshot Persistence ---

    /// Convert model to a persistable `Snapshot`.
    pub fn to_snapshot(&self) -> Snapshot {
        let mut snap = Snapshot::fresh();
        snap.windows.clear();

        for (win_idx, (win_id, win)) in self.windows.iter().enumerate() {
            if self.focused_window == Some(win_id) {
                snap.focused_window = win_idx;
            }

            let mut win_snap = WindowSnapshot {
                frame: [0.0, 0.0, win.size.w, win.size.h],
                screen: "main".to_string(),
                fullscreen: false,
                sidebar: SidebarSnapshot {
                    visible: win.sidebar_visible,
                    width: win.sidebar_width,
                },
                active_workspace: win
                    .workspaces
                    .iter()
                    .position(|w| *w == win.active_workspace)
                    .unwrap_or(0),
                workspaces: Vec::new(),
            };

            for &ws_id in &win.workspaces {
                if let Some(ws) = self.workspaces.get(ws_id) {
                    let focused_path = ws
                        .tree
                        .tree_path(ws.focused_pane)
                        .unwrap_or_else(|| vec![0]);
                    let zoomed_path = ws.zoomed_pane.and_then(|p| ws.tree.tree_path(p));

                    let tree_snap = self.build_tree_snapshot(&ws.tree, ws.tree.root());

                    win_snap.workspaces.push(WorkspaceSnapshot {
                        name: ws.name.clone().unwrap_or_default(),
                        focused_pane: focused_path,
                        zoomed: zoomed_path,
                        tree: tree_snap,
                    });
                }
            }

            if !win_snap.workspaces.is_empty() {
                snap.windows.push(win_snap);
            }
        }

        if snap.windows.is_empty() {
            snap.windows.push(WindowSnapshot::default_window());
        }

        snap
    }

    fn build_tree_snapshot(&self, tree: &SplitTree, node_idx: NodeIdx) -> TreeSnapshot {
        match tree.node(node_idx) {
            Some(Node::Leaf(pane_id)) => {
                let pane_snap = if let Some(pane) = self.panes.get(*pane_id) {
                    let mut tabs = Vec::new();
                    for &t_id in &pane.tabs {
                        if let Some(tab) = self.tabs.get(t_id) {
                            tabs.push(TabSnapshot {
                                cwd: tab.cwd.clone().unwrap_or_else(|| "~".to_string()),
                                title: tab.title.user.clone(),
                                mode: tab.mode.as_str().to_string(),
                            });
                        }
                    }
                    let active = pane
                        .tabs
                        .iter()
                        .position(|t| *t == pane.active_tab)
                        .unwrap_or(0);
                    PaneSnapshot::new(tabs, active)
                } else {
                    PaneSnapshot::new(vec![TabSnapshot::new("~", None, "auto")], 0)
                };
                TreeSnapshot::pane(pane_snap)
            }
            Some(Node::Split { dir, ratio, a, b }) => {
                let s_dir = match dir {
                    Dir::Right => "right",
                    Dir::Down => "down",
                };
                TreeSnapshot::split(
                    s_dir,
                    *ratio,
                    self.build_tree_snapshot(tree, *a),
                    self.build_tree_snapshot(tree, *b),
                )
            }
            None => TreeSnapshot::pane(PaneSnapshot::new(
                vec![TabSnapshot::new("~", None, "auto")],
                0,
            )),
        }
    }

    /// Load and restore model state from a `Snapshot`.
    pub fn from_snapshot(snap: &Snapshot) -> Self {
        let mut model = Self::new();

        for win_snap in &snap.windows {
            let win_id = model.windows.insert_with(|w| WindowState {
                id: w,
                workspaces: Vec::new(),
                active_workspace: WorkspaceId(0),
                sidebar_width: win_snap.sidebar.width,
                sidebar_visible: win_snap.sidebar.visible,
                size: Size::new(win_snap.frame[2], win_snap.frame[3]),
            });

            for ws_snap in &win_snap.workspaces {
                let (tree, first_pane) = model.restore_tree_snapshot(&ws_snap.tree, win_id);
                let focused_pane = tree.pane_at_path(&ws_snap.focused_pane);
                let zoomed_pane = ws_snap.zoomed.as_ref().map(|path| tree.pane_at_path(path));

                let ws_id = model.workspaces.insert_with(|id| Workspace {
                    id,
                    window: win_id,
                    // Empty string is the snapshot encoding of "auto name".
                    name: if ws_snap.name.is_empty() {
                        None
                    } else {
                        Some(ws_snap.name.clone())
                    },
                    tree,
                    focused_pane: if focused_pane.0 == 0 {
                        first_pane
                    } else {
                        focused_pane
                    },
                    zoomed_pane,
                    focus_history: vec![first_pane],
                    meta: WorkspaceMeta::default(),
                    layout: WorkspaceLayout::default(),
                    layout_key: None,
                });

                // Update pane workspace pointers
                if let Some(ws) = model.workspaces.get(ws_id) {
                    for p in ws.tree.leaves() {
                        if let Some(pane) = model.panes.get_mut(p) {
                            pane.workspace = ws_id;
                        }
                    }
                }

                if let Some(win) = model.windows.get_mut(win_id) {
                    win.workspaces.push(ws_id);
                }
            }

            if let Some(win) = model.windows.get_mut(win_id) {
                let active_idx = win_snap
                    .active_workspace
                    .min(win.workspaces.len().saturating_sub(1));
                if let Some(&ws_id) = win.workspaces.get(active_idx) {
                    win.active_workspace = ws_id;
                }
            }
        }

        let focused_idx = snap
            .focused_window
            .min(model.windows.len().saturating_sub(1));
        model.focused_window = model.windows.ids().nth(focused_idx);

        model
    }

    fn restore_tree_snapshot(
        &mut self,
        tree_snap: &TreeSnapshot,
        _win_id: WindowId,
    ) -> (SplitTree, PaneId) {
        match tree_snap {
            TreeSnapshot::Pane { pane: pane_snap } => {
                let tab_ids: Vec<TabId> = pane_snap
                    .tabs
                    .iter()
                    .map(|ts| {
                        let id = self.tabs.insert_with(|id| Tab {
                            id,
                            pane: PaneId(0),
                            session: None,
                            title: TabTitle {
                                user: ts.title.clone(),
                                osc: None,
                                process: None,
                            },
                            cwd: Some(ts.cwd.clone()),
                            mode: TabMode::parse(&ts.mode),
                            attention: Attention::default(),
                        });
                        self.pending_effects.push(ModelEffect::SpawnTab {
                            tab: id,
                            pane: PaneId(0),
                            cwd: Some(ts.cwd.clone()),
                        });
                        id
                    })
                    .collect();

                let active_tab = if tab_ids.is_empty() {
                    let fallback_id = self.tabs.insert_with(|id| Tab {
                        id,
                        pane: PaneId(0),
                        session: None,
                        title: TabTitle::default(),
                        cwd: Some("~".to_string()),
                        mode: TabMode::Auto,
                        attention: Attention::default(),
                    });
                    self.pending_effects.push(ModelEffect::SpawnTab {
                        tab: fallback_id,
                        pane: PaneId(0),
                        cwd: Some("~".to_string()),
                    });
                    fallback_id
                } else {
                    let idx = pane_snap.active.min(tab_ids.len() - 1);
                    tab_ids[idx]
                };

                let tabs = if tab_ids.is_empty() {
                    vec![active_tab]
                } else {
                    tab_ids
                };

                let pane_id = self.panes.insert_with(|id| Pane {
                    id,
                    workspace: WorkspaceId(0),
                    tabs: tabs.clone(),
                    active_tab,
                });

                for &t in &tabs {
                    if let Some(tab) = self.tabs.get_mut(t) {
                        tab.pane = pane_id;
                    }
                }

                (SplitTree::new(pane_id), pane_id)
            }
            TreeSnapshot::Split { split, ratio, a, b } => {
                let dir = if split == "down" {
                    Dir::Down
                } else {
                    Dir::Right
                };
                let (mut tree_a, root_pane) = self.restore_tree_snapshot(a, _win_id);
                let (tree_b, leaf_b) = self.restore_tree_snapshot(b, _win_id);

                // Graft tree_b into tree_a by splitting root_pane
                tree_a.split(root_pane, dir, leaf_b);
                // Also merge additional leaves from tree_b if it had multiple leaves
                for extra_leaf in tree_b.leaves().into_iter().skip(1) {
                    tree_a.split(leaf_b, Dir::Right, extra_leaf);
                }

                tree_a.set_ratio(tree_a.root(), *ratio);
                (tree_a, root_pane)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_window_creates_hierarchy() {
        let mut model = AppModel::new();
        let win_id = model.new_window(None, Size::new(800.0, 600.0), Some("/tmp".into()));

        assert_eq!(model.windows.len(), 1);
        assert_eq!(model.workspaces.len(), 1);
        assert_eq!(model.panes.len(), 1);
        assert_eq!(model.tabs.len(), 1);

        assert_eq!(model.focused_window(), Some(win_id));
        assert!(model.focused_workspace().is_some());
        assert!(model.focused_pane().is_some());
        assert!(model.focused_tab().is_some());

        let effects = model.drain_effects();
        assert_eq!(effects.len(), 1);
        assert!(matches!(effects[0], ModelEffect::SpawnTab { .. }));
    }

    #[test]
    fn split_and_close_pane() {
        let mut model = AppModel::new();
        model.new_window(None, Size::new(800.0, 600.0), None);

        let initial_pane = model.focused_pane().unwrap();
        let (new_pane, _) = model.split_pane(initial_pane, Dir::Right, true).unwrap();

        assert_eq!(model.panes.len(), 2);
        assert_eq!(model.focused_pane(), Some(new_pane));

        model.close_pane(new_pane);
        assert_eq!(model.panes.len(), 1);
        assert_eq!(model.focused_pane(), Some(initial_pane));
    }

    #[test]
    fn snapshot_round_trip() {
        let mut model = AppModel::new();
        let win = model.new_window(None, Size::new(1000.0, 800.0), Some("/home".into()));
        let p1 = model.focused_pane().unwrap();
        model.split_pane(p1, Dir::Right, false);
        model.new_workspace(win, Some("/var".into()));

        let snap = model.to_snapshot();
        assert_eq!(snap.windows.len(), 1);
        assert_eq!(snap.windows[0].workspaces.len(), 2);

        let restored = AppModel::from_snapshot(&snap);
        assert_eq!(restored.windows.len(), 1);
        assert_eq!(restored.workspaces.len(), 2);
    }
}
