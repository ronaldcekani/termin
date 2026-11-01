//! Pointer hover tracking for window chrome (title bar, tab strips, sidebar rows).

use termin_core::{PaneId, Point, Rect, Size, TabId, WindowId, WorkspaceId};
use termin_ui::widgets::{
    tabstrip::StripLayout,
    titlebar::{BarHit, TitlebarLayout},
};
use termin_ui::{close_rect, TAB_STRIP_HEIGHT};

use crate::input::workspace_at;
use crate::model::AppModel;

/// Chrome element under the pointer.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum HoverTarget {
    #[default]
    None,
    BarToggle,
    BarNewWorkspace,
    BarSearch,
    Tab(TabId),
    TabClose(TabId),
    /// Tab-strip action button `[new tab, split right, split down]` index.
    StripAction(PaneId, u8),
    Workspace(WorkspaceId),
}

impl HoverTarget {
    pub fn bar(self) -> Option<BarHit> {
        match self {
            Self::BarToggle => Some(BarHit::Toggle),
            Self::BarNewWorkspace => Some(BarHit::NewWorkspace),
            Self::BarSearch => Some(BarHit::Search),
            _ => None,
        }
    }
}

/// Hover target tagged with the window it belongs to.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct HoverState {
    win: Option<WindowId>,
    target: HoverTarget,
}

impl HoverState {
    /// Record the target under the pointer in `win`; `true` if anything changed.
    pub fn set(&mut self, win: WindowId, target: HoverTarget) -> bool {
        let next = Self {
            win: (target != HoverTarget::None).then_some(win),
            target,
        };
        let changed = *self != next;
        *self = next;
        changed
    }

    /// Forget any hover; returns the window that had one, if any.
    pub fn clear(&mut self) -> Option<WindowId> {
        let win = self.win;
        *self = Self::default();
        win
    }

    /// Window the hover belongs to.
    pub fn window(&self) -> Option<WindowId> {
        self.win
    }

    /// Target to draw in `win`.
    pub fn target_for(&self, win: WindowId) -> HoverTarget {
        if self.win == Some(win) {
            self.target
        } else {
            HoverTarget::None
        }
    }
}

/// What lies under `pos` in `win`, using the same geometry as click hit testing.
pub fn hover_at(
    model: &mut AppModel,
    win: WindowId,
    pos: Point,
    min_pane: Size,
    line_pitch: f32,
) -> HoverTarget {
    let Some(width) = model.windows.get(win).map(|w| w.size.w) else {
        return HoverTarget::None;
    };
    match TitlebarLayout::new(width).hit(pos) {
        Some(BarHit::Toggle) => return HoverTarget::BarToggle,
        Some(BarHit::NewWorkspace) => return HoverTarget::BarNewWorkspace,
        Some(BarHit::Search) => return HoverTarget::BarSearch,
        None => {}
    }
    if let Some(ws) = workspace_at(model, win, pos, line_pitch) {
        return HoverTarget::Workspace(ws);
    }
    let Some((pane_id, pane_rect)) = model.layout(win, min_pane).and_then(|l| {
        l.panes
            .iter()
            .find(|(_, r)| Rect::new(r.x, r.y, r.w, TAB_STRIP_HEIGHT).contains(pos))
            .copied()
    }) else {
        return HoverTarget::None;
    };
    let Some(pane) = model.panes.get(pane_id) else {
        return HoverTarget::None;
    };
    let strip = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);
    let active = pane
        .tabs
        .iter()
        .position(|t| *t == pane.active_tab)
        .unwrap_or(0);
    let layout = StripLayout::new(strip, pane.tabs.len(), active);
    for (i, &tab) in pane.tabs.iter().enumerate() {
        let Some(r) = layout.tab_rect(strip, i) else {
            continue;
        };
        if close_rect(r).contains(pos) {
            return HoverTarget::TabClose(tab);
        }
        if r.contains(pos) {
            return HoverTarget::Tab(tab);
        }
    }
    for (i, r) in layout.action_rects(strip).iter().enumerate() {
        if r.contains(pos) {
            return HoverTarget::StripAction(pane_id, i as u8);
        }
    }
    HoverTarget::None
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_ui::widgets::sidebar::line_pitch;

    fn setup() -> (AppModel, WindowId) {
        let mut m = AppModel::new();
        let win = m.new_window(None, Size::new(1000.0, 600.0), None);
        (m, win)
    }

    fn center(r: Rect) -> Point {
        Point::new(r.x + r.w * 0.5, r.y + r.h * 0.5)
    }

    const MIN: Size = Size { w: 80.0, h: 40.0 };

    #[test]
    fn titlebar_buttons_set_and_clear_without_spurious_changes() {
        let (mut m, win) = setup();
        let bar = TitlebarLayout::new(1000.0);
        let mut st = HoverState::default();

        let t = hover_at(&mut m, win, center(bar.toggle), MIN, line_pitch(14.0));
        assert_eq!(t, HoverTarget::BarToggle);
        assert!(st.set(win, t));
        assert_eq!(st.target_for(win), HoverTarget::BarToggle);
        // Same target again: no change, so no frame request.
        assert!(!st.set(win, t));

        let t = hover_at(&mut m, win, center(bar.new_workspace), MIN, 18.0);
        assert_eq!(t, HoverTarget::BarNewWorkspace);
        assert!(st.set(win, t));

        // Off-window point (mouseExited) clears.
        let t = hover_at(&mut m, win, Point::new(-1.0e6, -1.0e6), MIN, 18.0);
        assert_eq!(t, HoverTarget::None);
        assert!(st.set(win, t));
        assert_eq!(st.target_for(win), HoverTarget::None);
        assert!(!st.set(win, t));
    }

    #[test]
    fn tab_and_strip_action_hover() {
        let (mut m, win) = setup();
        let pane_id = m
            .workspaces
            .iter()
            .next()
            .map(|(_, w)| w.focused_pane)
            .unwrap();
        let pane_rect = m.layout(win, MIN).unwrap().rect_of(pane_id).unwrap();
        let strip = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);
        let tab = m.panes.get(pane_id).unwrap().active_tab;
        let layout = StripLayout::new(strip, 1, 0);
        let tr = layout.tab_rect(strip, 0).unwrap();
        assert_eq!(
            hover_at(&mut m, win, Point::new(tr.x + 40.0, tr.y + 10.0), MIN, 18.0),
            HoverTarget::Tab(tab)
        );
        assert_eq!(
            hover_at(&mut m, win, center(close_rect(tr)), MIN, 18.0),
            HoverTarget::TabClose(tab)
        );
        let [new_tab, ..] = layout.action_rects(strip);
        assert_eq!(
            hover_at(&mut m, win, center(new_tab), MIN, 18.0),
            HoverTarget::StripAction(pane_id, 0)
        );
    }

    #[test]
    fn hover_is_per_window_and_clearable() {
        let (_, win) = setup();
        let mut st = HoverState::default();
        st.set(win, HoverTarget::BarSearch);
        assert_eq!(st.clear(), Some(win));
        assert_eq!(st.clear(), None);
        assert_eq!(st.target_for(win), HoverTarget::None);
    }
}
