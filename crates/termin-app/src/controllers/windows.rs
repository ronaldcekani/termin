//! Window actions controller.

use crate::action::Action;
use crate::model::AppModel;

pub fn handle(model: &mut AppModel, action: &Action) -> bool {
    match action {
        Action::NewWindow => {
            model.new_window(None, termin_core::Size::new(960.0, 640.0), None);
            true
        }
        Action::CloseWindow => {
            if let Some(win) = model.focused_window() {
                model.close_window(win);
                true
            } else {
                false
            }
        }
        Action::Quit => {
            // Signal termination by clearing windows
            let wins: Vec<_> = model.windows.ids().collect();
            for w in wins {
                model.close_window(w);
            }
            true
        }
        _ => false,
    }
}
