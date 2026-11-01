//! Terminal actions controller (copy/paste/scroll/font/clear).

use crate::action::Action;
use crate::model::AppModel;

pub fn handle(_model: &mut AppModel, action: &Action) -> bool {
    // In Phase 2, terminal actions are dispatched directly in App / session_ctl.
    // This handler serves as the controller routing target.
    matches!(
        action,
        Action::Copy
            | Action::Paste
            | Action::SelectAll
            | Action::ClearBuffer
            | Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollToTop
            | Action::ScrollToBottom
            | Action::FontIncrease
            | Action::FontDecrease
            | Action::FontReset
    )
}
