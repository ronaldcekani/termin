//! Entity data types: windows, workspaces, panes, tabs, attention, metadata.
//!
//! See `docs/specs/app-model-and-ui.md §1, §3, §8`.

use std::sync::Arc;
use std::time::Instant;
use termin_core::{PaneId, Rect, Size, TabId, WindowId, WorkspaceId};
use termin_pty::Session;
use termin_vt::Terminal;

use super::tree::{Divider, SplitTree};

/// Why a tab wants attention (§8).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AttentionKind {
    /// Terminal bell.
    Bell,
    /// OSC 9/777/99 notification.
    Notify,
    /// Long command finished successfully while not visible.
    CommandDone,
    /// Long command failed while not visible.
    CommandFailed,
}

/// Per-tab attention state.
#[derive(Clone, Debug, PartialEq)]
pub struct Attention {
    /// Not yet seen by the user.
    pub unread: bool,
    /// Source of the attention.
    pub kind: AttentionKind,
    /// Optional message (e.g. notification body).
    pub message: Option<Box<str>>,
    /// When it was raised (`None` = never raised).
    pub at: Option<Instant>,
}

impl Default for Attention {
    fn default() -> Self {
        Self {
            unread: false,
            kind: AttentionKind::Bell,
            message: None,
            at: None,
        }
    }
}

/// Tab rendering mode preference (persisted as `auto|block|classic`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum TabMode {
    /// Block mode when shell integration is present, else classic.
    #[default]
    Auto,
    /// Force block mode.
    Block,
    /// Force classic mode.
    Classic,
}

impl TabMode {
    /// Persistence string.
    pub fn as_str(self) -> &'static str {
        match self {
            TabMode::Auto => "auto",
            TabMode::Block => "block",
            TabMode::Classic => "classic",
        }
    }

    /// Parse a persistence string; unknown → `Auto`.
    pub fn parse(s: &str) -> Self {
        match s {
            "block" => TabMode::Block,
            "classic" => TabMode::Classic,
            _ => TabMode::Auto,
        }
    }
}

/// Title sources; precedence user > OSC > foreground process.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabTitle {
    /// User override (rename).
    pub user: Option<String>,
    /// OSC 0/2 title.
    pub osc: Option<String>,
    /// Foreground process name.
    pub process: Option<String>,
}

impl TabTitle {
    /// Effective title (never empty).
    pub fn display(&self) -> &str {
        [&self.user, &self.osc, &self.process]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .find(|s| !s.is_empty())
            .unwrap_or("shell")
    }
}

/// A tab (= session).
pub struct Tab {
    /// Id.
    pub id: TabId,
    /// Owning pane.
    pub pane: PaneId,
    /// PTY + terminal; `None` until 2A spawns it in response to
    /// [`ModelEffect::SpawnTab`](super::ModelEffect::SpawnTab).
    pub session: Option<Arc<Session<Terminal>>>,
    /// Title sources.
    pub title: TabTitle,
    /// Last known cwd (OSC 7 / persistence).
    pub cwd: Option<String>,
    /// Mode preference.
    pub mode: TabMode,
    /// Attention state.
    pub attention: Attention,
}

/// A pane (split-tree leaf) holding ordered tabs.
#[derive(Clone, Debug)]
pub struct Pane {
    /// Id.
    pub id: PaneId,
    /// Owning workspace.
    pub workspace: WorkspaceId,
    /// Ordered tabs (never empty).
    pub tabs: Vec<TabId>,
    /// Active tab (always in `tabs`).
    pub active_tab: TabId,
}

/// Sidebar metadata (§3.1); populated by track 3D.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceMeta {
    /// Git branch of the focused tab's cwd.
    pub git_branch: Option<String>,
    /// Working tree dirty.
    pub git_dirty: bool,
    /// Git repo root basename (for auto naming).
    pub repo_name: Option<String>,
    /// Distinct cwds, focused first (max 3 shown).
    pub paths: Vec<String>,
    /// TCP LISTEN ports.
    pub ports: Vec<u16>,
    /// Status line (attention message / long-running command).
    pub status: Option<String>,
    /// Foreground process of the focused tab.
    pub fg_process: Option<String>,
}

/// Cached layout of a workspace (§2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceLayout {
    /// Area the layout was computed for.
    pub area: Rect,
    /// Visible panes (just the zoomed pane when zoomed).
    pub panes: Vec<(PaneId, Rect)>,
    /// Dividers (empty when zoomed).
    pub dividers: Vec<Divider>,
}

impl WorkspaceLayout {
    /// Rect of `pane` if visible.
    pub fn rect_of(&self, pane: PaneId) -> Option<Rect> {
        self.panes.iter().find(|(p, _)| *p == pane).map(|(_, r)| *r)
    }

    /// Pane under a point.
    pub fn pane_at(&self, p: termin_core::Point) -> Option<PaneId> {
        self.panes
            .iter()
            .find(|(_, r)| r.contains(p))
            .map(|(id, _)| *id)
    }

    /// Divider whose hit area contains the point.
    pub fn divider_at(&self, p: termin_core::Point) -> Option<&Divider> {
        self.dividers.iter().find(|d| d.hit.contains(p))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LayoutKey {
    pub area: Rect,
    pub min: Size,
    pub zoomed: Option<PaneId>,
    pub rev: u32,
}

/// A workspace: name, split tree, focus.
#[derive(Clone, Debug)]
pub struct Workspace {
    /// Id.
    pub id: WorkspaceId,
    /// Owning window.
    pub window: WindowId,
    /// User name; `None` → auto (§3.2).
    pub name: Option<String>,
    /// Split tree.
    pub tree: SplitTree,
    /// Focused pane (always a leaf of `tree`).
    pub focused_pane: PaneId,
    /// Zoomed pane (a leaf of `tree`) if any.
    pub zoomed_pane: Option<PaneId>,
    /// Focus history, most recent first (used as navigation tie-break).
    pub focus_history: Vec<PaneId>,
    /// Sidebar metadata.
    pub meta: WorkspaceMeta,
    pub(crate) layout: WorkspaceLayout,
    pub(crate) layout_key: Option<LayoutKey>,
}

/// Per-window state.
#[derive(Clone, Debug)]
pub struct WindowState {
    /// Platform window id.
    pub id: WindowId,
    /// Ordered sidebar list (never empty).
    pub workspaces: Vec<WorkspaceId>,
    /// Active workspace (always in `workspaces`).
    pub active_workspace: WorkspaceId,
    /// Sidebar width in points.
    pub sidebar_width: f32,
    /// Sidebar shown.
    pub sidebar_visible: bool,
    /// Window content size in points (set by the platform layer).
    pub size: Size,
}

/// Height of the window top bar (traffic lights, sidebar toggle, folder search).
/// Sidebar and workspace content start below it.
pub const TITLEBAR_HEIGHT: f32 = 38.0;

impl WindowState {
    /// Sidebar rect (zero width when hidden), below the top bar.
    pub fn sidebar_rect(&self) -> Rect {
        let w = if self.sidebar_visible {
            self.sidebar_width.min(self.size.w)
        } else {
            0.0
        };
        Rect::new(
            0.0,
            TITLEBAR_HEIGHT,
            w,
            (self.size.h - TITLEBAR_HEIGHT).max(0.0),
        )
    }

    /// Workspace content area (right of the sidebar, below the top bar).
    pub fn content_rect(&self) -> Rect {
        let s = self.sidebar_rect();
        Rect::new(
            s.w,
            TITLEBAR_HEIGHT,
            (self.size.w - s.w).max(0.0),
            (self.size.h - TITLEBAR_HEIGHT).max(0.0),
        )
    }
}
