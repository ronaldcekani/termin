//! Attention and notifications controller (Phase 3D).

use crate::action::Action;
use crate::model::AppModel;
use crate::model::AttentionKind;
use std::time::{Duration, Instant};
use termin_core::{TabId, WindowId};

/// Minimum continuous focus time before pass-through focus clears attention.
pub const CLEAR_DWELL: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, Debug, Default)]
pub struct AttentionDwell {
    candidate: Option<(TabId, Instant)>,
}

impl AttentionDwell {
    /// Clear focused-tab attention only after the same tab remained focused.
    pub fn update(&mut self, model: &mut AppModel, now: Instant) -> bool {
        let Some(tab_id) = model.focused_tab() else {
            self.candidate = None;
            return false;
        };
        let Some(tab) = model.tabs.get(tab_id) else {
            self.candidate = None;
            return false;
        };
        if !tab.attention.unread {
            self.candidate = None;
            return false;
        }
        match self.candidate {
            Some((candidate, since))
                if candidate == tab_id && now.saturating_duration_since(since) >= CLEAR_DWELL =>
            {
                self.candidate = None;
                if let Some(tab) = model.tabs.get_mut(tab_id) {
                    tab.attention.unread = false;
                }
                true
            }
            Some((candidate, _)) if candidate == tab_id => false,
            _ => {
                self.candidate = Some((tab_id, now));
                false
            }
        }
    }
}

/// Raise attention and propagate its message to the owning workspace card.
pub fn raise(
    model: &mut AppModel,
    tab_id: TabId,
    kind: AttentionKind,
    message: Option<Box<str>>,
    now: Instant,
) -> Option<WindowId> {
    let pane_id = model.tabs.get(tab_id)?.pane;
    let workspace_id = model.panes.get(pane_id)?.workspace;
    let window_id = model.workspaces.get(workspace_id)?.window;
    let message = message.filter(|text| !text.is_empty());
    if let Some(tab) = model.tabs.get_mut(tab_id) {
        tab.attention.unread = true;
        tab.attention.kind = kind;
        tab.attention.message = message.clone();
        tab.attention.at = Some(now);
    }
    if let Some(text) = message {
        if let Some(workspace) = model.workspaces.get_mut(workspace_id) {
            workspace.meta.status = Some(text.into_string());
        }
    }
    Some(window_id)
}

/// Number of tabs represented by the Dock unread badge.
pub fn unread_count(model: &AppModel) -> u32 {
    model
        .tabs
        .iter()
        .filter(|(_, tab)| tab.attention.unread)
        .count()
        .min(u32::MAX as usize) as u32
}

/// Whether the tab is in the active, focused pane of a key window.
pub fn is_tab_visible(model: &AppModel, tab_id: TabId) -> bool {
    let Some(tab) = model.tabs.get(tab_id) else {
        return false;
    };
    let Some(pane) = model.panes.get(tab.pane) else {
        return false;
    };
    let Some(workspace) = model.workspaces.get(pane.workspace) else {
        return false;
    };
    let Some(window) = model.windows.get(workspace.window) else {
        return false;
    };
    model.focused_window == Some(window.id)
        && window.active_workspace == workspace.id
        && workspace.focused_pane == pane.id
        && pane.active_tab == tab_id
}

/// Platform notification policy from app-model-and-ui §8.
pub fn should_notify_system(model: &AppModel, tab_id: TabId) -> bool {
    !is_tab_visible(model, tab_id)
}

pub fn handle(model: &mut AppModel, action: &Action) -> bool {
    match action {
        Action::ClearAttention => {
            if let Some(tab_id) = model.focused_tab() {
                if let Some(tab) = model.tabs.get_mut(tab_id) {
                    tab.attention.unread = false;
                    tab.attention.message = None;
                    return true;
                }
            }
            false
        }
        Action::JumpToAttention => {
            // Instant is monotonic, so this picks the newest unread item without
            // depending on arena iteration order.
            let mut target = None;
            for (_, ws) in model.workspaces.iter() {
                for p in ws.tree.leaves() {
                    if let Some(pane) = model.panes.get(p) {
                        for &t in &pane.tabs {
                            if let Some(tab) = model.tabs.get(t) {
                                if tab.attention.unread {
                                    let at = tab.attention.at;
                                    if target.map_or(true, |(_, _, _, best_at)| at > best_at) {
                                        target = Some((ws.id, p, t, at));
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some((ws_id, p_id, t_id, _)) = target {
                model.select_workspace(ws_id);
                if let Some(workspace) = model.workspaces.get_mut(ws_id) {
                    workspace.focused_pane = p_id;
                }
                if let Some(target_pane) = model.panes.get_mut(p_id) {
                    target_pane.active_tab = t_id;
                }
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::Size;

    #[test]
    fn raise_propagates_message_and_dwell_clears_only_after_timeout() {
        let mut model = AppModel::new();
        let window = model.new_window(None, Size::new(800.0, 600.0), None);
        let tab = model.focused_tab().unwrap();
        let now = Instant::now();
        assert!(is_tab_visible(&model, tab));
        assert!(!should_notify_system(&model, tab));
        assert_eq!(
            raise(
                &mut model,
                tab,
                AttentionKind::Notify,
                Some("waiting".into()),
                now
            ),
            Some(window)
        );
        let workspace = model.focused_workspace().unwrap();
        assert_eq!(
            model
                .workspaces
                .get(workspace)
                .unwrap()
                .meta
                .status
                .as_deref(),
            Some("waiting")
        );

        let mut dwell = AttentionDwell::default();
        assert!(!dwell.update(&mut model, now));
        assert!(!dwell.update(&mut model, now + CLEAR_DWELL - Duration::from_millis(1)));
        assert!(dwell.update(&mut model, now + CLEAR_DWELL));
        assert!(!model.tabs.get(tab).unwrap().attention.unread);
        assert_eq!(unread_count(&model), 0);
        assert!(!should_notify_system(&model, tab));
    }
}
