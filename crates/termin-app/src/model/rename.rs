//! Inline rename editing state for workspaces and tabs.
//!
//! While `AppModel::rename` is `Some`, the app routes keyboard/IME input to the
//! field instead of the PTY/editor (`docs/specs/app-model-and-ui.md §3`).

use super::AppModel;
use termin_core::{KeyEvent, TabId, WorkspaceId};
use termin_ui::widgets::text_field::{FieldOutcome, TextField};

/// What is being renamed.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Workspace(WorkspaceId),
    Tab(TabId),
}

/// An in-progress inline rename.
#[derive(Clone, Debug)]
pub struct RenameEdit {
    pub target: RenameTarget,
    pub field: TextField,
    /// Text the field started with (an unchanged commit is a no-op).
    pub initial: String,
}

impl AppModel {
    /// Start renaming `ws_id` inline. Returns `false` if it does not exist.
    pub fn begin_rename_workspace(&mut self, ws_id: WorkspaceId) -> bool {
        let Some(ws) = self.workspaces.get(ws_id) else {
            return false;
        };
        let initial = match ws.name.as_deref() {
            Some(n) => n.to_string(),
            None => ws
                .meta
                .paths
                .first()
                .map(|p| p.rsplit('/').next().unwrap_or(p).to_string())
                .unwrap_or_else(|| "Workspace".to_string()),
        };
        self.start_rename(RenameTarget::Workspace(ws_id), initial)
    }

    /// Start renaming `tab_id` inline. Returns `false` if it does not exist.
    pub fn begin_rename_tab(&mut self, tab_id: TabId) -> bool {
        let Some(tab) = self.tabs.get(tab_id) else {
            return false;
        };
        let initial = tab.title.display().to_string();
        self.start_rename(RenameTarget::Tab(tab_id), initial)
    }

    fn start_rename(&mut self, target: RenameTarget, initial: String) -> bool {
        // Starting a second rename commits the first.
        self.commit_rename();
        self.rename = Some(RenameEdit {
            target,
            field: TextField::new(&initial),
            initial,
        });
        true
    }

    /// Commit the active rename; empty text reverts to the automatic name.
    pub fn commit_rename(&mut self) -> bool {
        let Some(edit) = self.rename.take() else {
            return false;
        };
        let text = edit.field.text().trim().to_string();
        if text == edit.initial.trim() {
            return true;
        }
        let value = Some(text); // rename_* treat blank as "reset to auto"
        match edit.target {
            RenameTarget::Workspace(id) => self.rename_workspace(id, value),
            RenameTarget::Tab(id) => self.rename_tab(id, value),
        };
        true
    }

    /// Abandon the active rename without changing names.
    pub fn cancel_rename(&mut self) -> bool {
        self.rename.take().is_some()
    }

    /// Feed a key to the active rename field. `Some(redraw)` if a rename is
    /// active (the key is consumed), `None` otherwise.
    pub fn rename_key(&mut self, ev: &KeyEvent) -> Option<bool> {
        let edit = self.rename.as_mut()?;
        if !ev.pressed {
            return Some(false);
        }
        Some(match edit.field.handle_key(ev) {
            FieldOutcome::Ignored => false,
            FieldOutcome::Edited => true,
            FieldOutcome::Commit => self.commit_rename(),
            FieldOutcome::Cancel => self.cancel_rename(),
        })
    }

    /// Insert committed IME/text input into the active rename field.
    pub fn rename_text(&mut self, text: &str) -> Option<bool> {
        let edit = self.rename.as_mut()?;
        Some(edit.field.insert(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::{Key, Mods, NamedKey, Size};

    fn key(k: Key, text: Option<&str>) -> KeyEvent {
        KeyEvent {
            key: k,
            physical: 0,
            mods: Mods::empty(),
            text: text.map(Into::into),
            repeat: false,
            pressed: true,
        }
    }

    fn model() -> (AppModel, WorkspaceId) {
        let mut m = AppModel::new();
        m.new_window(None, Size::new(800.0, 600.0), None);
        let ws = m.workspaces.iter().next().map(|(id, _)| id).unwrap();
        (m, ws)
    }

    #[test]
    fn rename_workspace_commit_and_revert() {
        let (mut m, ws) = model();
        assert!(m.begin_rename_workspace(ws));
        for c in "Work".chars() {
            let s = c.to_string();
            assert_eq!(m.rename_key(&key(Key::Char(c), Some(&s))), Some(true));
        }
        assert_eq!(
            m.rename_key(&key(Key::Named(NamedKey::Enter), None)),
            Some(true)
        );
        assert!(m.rename.is_none());
        assert_eq!(m.workspaces.get(ws).unwrap().name.as_deref(), Some("Work"));

        // Empty commit reverts to auto name.
        m.begin_rename_workspace(ws);
        m.rename_key(&key(Key::Named(NamedKey::Backspace), None));
        m.commit_rename();
        assert_eq!(m.workspaces.get(ws).unwrap().name, None);
    }

    #[test]
    fn escape_cancels_and_unchanged_commit_keeps_auto_name() {
        let (mut m, ws) = model();
        m.begin_rename_workspace(ws);
        m.rename_text("zzz");
        m.rename_key(&key(Key::Named(NamedKey::Escape), None));
        assert!(m.rename.is_none());
        assert_eq!(m.workspaces.get(ws).unwrap().name, None);

        m.begin_rename_workspace(ws);
        m.commit_rename();
        assert_eq!(m.workspaces.get(ws).unwrap().name, None);
    }

    #[test]
    fn rename_tab_and_keys_not_consumed_when_idle() {
        let (mut m, _) = model();
        assert_eq!(m.rename_key(&key(Key::Char('a'), Some("a"))), None);
        let tab = m.tabs.iter().next().map(|(id, _)| id).unwrap();
        assert!(m.begin_rename_tab(tab));
        m.rename_text("build");
        m.commit_rename();
        assert_eq!(
            m.tabs.get(tab).unwrap().title.user.as_deref(),
            Some("build")
        );
    }

    #[test]
    fn user_named_default_survives_snapshot() {
        let (mut m, ws) = model();
        m.rename_workspace(ws, Some("Default".into()));
        let snap = m.to_snapshot();
        let ws_snap = &snap.windows[0].workspaces[0];
        assert_eq!(ws_snap.name, "Default");
        m.rename_workspace(ws, None);
        let snap = m.to_snapshot();
        assert_eq!(snap.windows[0].workspaces[0].name, "");
    }
}
