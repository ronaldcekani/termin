//! Input event handling, key/mouse routing, selection, and clipboard (`termin-app::input`).
//!
//! See `docs/architecture.md §4.2` and `docs/specs/app-model-and-ui.md §6`.

use termin_core::{KeyEvent, MouseButton, MouseEvent, MouseKind, Rect, ScrollEvent, WindowId};
use termin_platform::PlatformCx;
use termin_ui::{PaneWidget, TermFonts, TAB_STRIP_HEIGHT};
use termin_vt::{AbsPos, CellMouseEvent, Modes, Selection, SelectionKind};

use crate::action::Action;
use crate::dispatch::dispatch;
use crate::keymap::{Keymap, WhenContext};
use crate::model::AppModel;
use crate::model::Dir;
use crate::model::RenameTarget;

/// Current selection state for a pane.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionState {
    pub pane: termin_core::PaneId,
    pub start: AbsPos,
    pub end: AbsPos,
    pub dragging: bool,
}

/// Handle a keyboard event.
///
/// Returns `true` if state changed (requiring a frame redraw).
pub fn handle_key(
    model: &mut AppModel,
    keymap: &Keymap,
    ev: &KeyEvent,
    cx: &mut PlatformCx,
) -> bool {
    // 1. Resolve keybinding
    if let Some(action) = keymap.lookup(ev, WhenContext::Terminal) {
        match &action {
            Action::Copy => {
                copy_selection(model, cx);
                return true;
            }
            Action::Paste => {
                paste_clipboard(model, cx);
                return true;
            }
            Action::SelectAll => {
                select_all(model);
                return true;
            }
            _ => {
                return dispatch(model, &action);
            }
        }
    }

    // 2. Unbound key: send to focused terminal session
    if !ev.pressed {
        return false;
    }

    let Some(session) = model.focused_session() else {
        return false;
    };

    let Ok(term) = session.term.lock() else {
        return false;
    };

    let encoded = term.encode_key(ev);
    if !encoded.is_empty() {
        session.write(&encoded);
        true
    } else if let Some(ref s) = ev.text {
        session.write(s.as_bytes());
        true
    } else {
        false
    }
}

/// Handle text committed by IME.
pub fn handle_ime_commit(model: &mut AppModel, text: &str) -> bool {
    let Some(session) = model.focused_session() else {
        return false;
    };
    session.write(text.as_bytes());
    true
}

/// Copy current selection text to platform clipboard.
pub fn copy_selection(model: &mut AppModel, cx: &mut PlatformCx) {
    let Some(session) = model.focused_session() else {
        return;
    };

    let Ok(term) = session.term.lock() else {
        return;
    };

    if let Some(sel) = term.selection() {
        let text = term.selection_text(sel);
        if !text.is_empty() {
            cx.clipboard_set(&text);
        }
    }
}

/// Paste clipboard contents into focused session.
pub fn paste_clipboard(model: &mut AppModel, cx: &mut PlatformCx) {
    let Some(session) = model.focused_session() else {
        return;
    };
    let Some(text) = cx.clipboard_get() else {
        return;
    };

    let bracketed = if let Ok(term) = session.term.lock() {
        term.modes().contains(Modes::BRACKETED_PASTE)
    } else {
        false
    };

    if bracketed {
        session.write(b"\x1b[200~");
        session.write(text.as_bytes());
        session.write(b"\x1b[201~");
    } else {
        session.write(text.as_bytes());
    }
}

/// Select all text in focused pane.
pub fn select_all(model: &mut AppModel) {
    let Some(session) = model.focused_session() else {
        return;
    };
    if let Ok(mut term) = session.term.lock() {
        let rows = term.rows();
        let cols = term.cols();
        let scrollback = term.scrollback_len() as u64;
        let start = AbsPos::new(0, 0);
        let end = AbsPos::new(scrollback + rows as u64 - 1, cols.saturating_sub(1));
        term.set_selection(Some(Selection {
            kind: SelectionKind::Simple,
            anchor: start,
            head: end,
        }));
    };
}

/// Handle mouse clicks and drags inside the terminal view.
pub fn handle_mouse(
    model: &mut AppModel,
    ev: &MouseEvent,
    content_rect: Rect,
    fonts: &TermFonts,
) -> bool {
    let Some(session) = model.focused_session() else {
        return false;
    };

    // Calculate grid cell under mouse
    let cell_w = fonts.geo.cell_w;
    let cell_h = fonts.geo.cell_h;
    let rel_x = (ev.pos.x - content_rect.x).max(0.0);
    let rel_y = (ev.pos.y - content_rect.y).max(0.0);
    let col = (rel_x / cell_w).floor() as u16;
    let row = (rel_y / cell_h).floor() as u16;

    let Ok(mut term) = session.term.lock() else {
        return false;
    };

    let abs_line = visible_abs_line(term.scrollback_len(), term.display_scroll(), row);
    let abs_pos = AbsPos::new(abs_line, col);

    let modes = term.modes();

    // Check if application requested mouse reporting (and Alt is not held)
    let is_reporting = modes.intersects(
        Modes::MOUSE_X10
            | Modes::MOUSE_NORMAL
            | Modes::MOUSE_BUTTON
            | Modes::MOUSE_ANY
            | Modes::MOUSE_SGR,
    );
    if is_reporting && !ev.mods.contains(termin_core::Mods::ALT) {
        let cell_ev = CellMouseEvent {
            pos: abs_pos,
            event: *ev,
        };
        if let Some(encoded) = term.encode_mouse(&cell_ev) {
            session.write(&encoded);
            return true;
        }
    }

    // Native text selection handling
    match ev.kind {
        MouseKind::Down(_) => {
            if ev.click_count == 2 {
                term.select_word(abs_pos);
            } else if ev.click_count >= 3 {
                term.select_line(abs_line);
            } else {
                term.set_selection(Some(Selection {
                    kind: SelectionKind::Simple,
                    anchor: abs_pos,
                    head: abs_pos,
                }));
            }
            true
        }
        MouseKind::Drag(_) => {
            if let Some(sel) = term.selection() {
                let start_pos = sel.anchor;
                term.set_selection(Some(Selection {
                    kind: SelectionKind::Simple,
                    anchor: start_pos,
                    head: abs_pos,
                }));
                true
            } else {
                false
            }
        }
        MouseKind::Up(_) => true,
        MouseKind::Move => false,
    }
}

/// Route a window-coordinate click to its workspace, pane, or tab before
/// forwarding terminal-body input to the active session.
pub fn handle_window_mouse(
    model: &mut AppModel,
    win_id: WindowId,
    ev: &MouseEvent,
    fonts: &TermFonts,
    is_block_pane: &dyn Fn(termin_core::PaneId) -> bool,
) -> bool {
    let is_pointer_down = matches!(ev.kind, MouseKind::Down(_));
    let is_left_down = matches!(ev.kind, MouseKind::Down(MouseButton::Left));
    if is_pointer_down {
        model.focused_window = Some(win_id);
    }
    if is_left_down && model.rename.is_some() {
        // Clicking inside the field keeps editing; anywhere else commits.
        if rename_target_rect(model, win_id, ev.pos).is_some() {
            return true;
        }
        model.commit_rename();
    }
    if is_left_down {
        if let Some(ws_id) = workspace_at(model, win_id, ev.pos) {
            let selected = model.select_workspace(ws_id);
            if ev.click_count == 2 {
                return model.begin_rename_workspace(ws_id) || selected;
            }
            return selected;
        }
        if let Some(window) = model.windows.get(win_id) {
            if window.sidebar_visible {
                let sidebar = window.sidebar_rect();
                if ev.pos.x >= sidebar.x && ev.pos.x < sidebar.right() {
                    return true;
                }
            }
        }
    }

    if matches!(ev.kind, MouseKind::Move) {
        return false;
    }

    let min_size = termin_core::Size::new(fonts.geo.cell_w * 12.0, fonts.geo.cell_h * 3.0);
    let Some(layout) = model.layout(win_id, min_size).cloned() else {
        return false;
    };
    let pane_hit = if is_pointer_down {
        layout
            .panes
            .iter()
            .find(|(_, rect)| rect.contains(ev.pos))
            .copied()
    } else if model.focused_window == Some(win_id) {
        model
            .windows
            .get(win_id)
            .and_then(|window| model.workspaces.get(window.active_workspace))
            .and_then(|workspace| {
                let pane_id = workspace.focused_pane;
                layout.rect_of(pane_id).map(|rect| (pane_id, rect))
            })
    } else {
        None
    };
    let Some((pane_id, pane_rect)) = pane_hit else {
        return false;
    };

    let focused = if is_pointer_down {
        focus_pane(model, win_id, pane_id)
    } else {
        false
    };
    if is_left_down {
        let strip = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);
        if strip.contains(ev.pos) {
            return handle_tabstrip_click(model, pane_id, strip, ev.pos, ev.click_count) || focused;
        }
    }

    let content_rect = PaneWidget::content_rect(pane_rect);
    if is_block_pane(pane_id) {
        // Block mode draws blocks and the editor, not the raw grid: grid mouse
        // selection/reporting coordinates would not line up.
        focused
    } else if content_rect.contains(ev.pos) {
        handle_mouse(model, ev, content_rect, fonts) || focused
    } else {
        false
    }
}

/// Rect of the card/tab being renamed if `pos` lies inside it.
fn rename_target_rect(model: &AppModel, win_id: WindowId, pos: termin_core::Point) -> Option<Rect> {
    match model.rename.as_ref()?.target {
        RenameTarget::Workspace(ws) => (workspace_at(model, win_id, pos) == Some(ws))
            .then(|| Rect::new(pos.x, pos.y, 0.0, 0.0)),
        RenameTarget::Tab(tab) => {
            let pane_id = model.tabs.get(tab)?.pane;
            let rect = tab_rect_of(model, win_id, pane_id, tab)?;
            rect.contains(pos).then_some(rect)
        }
    }
}

/// Screen rect of `tab` in its pane's strip, if that pane is visible in `win_id`.
pub fn tab_rect_of(
    model: &AppModel,
    win_id: WindowId,
    pane_id: termin_core::PaneId,
    tab: termin_core::TabId,
) -> Option<Rect> {
    let win = model.windows.get(win_id)?;
    let pane_rect = model
        .workspaces
        .get(win.active_workspace)?
        .layout
        .rect_of(pane_id)?;
    let pane = model.panes.get(pane_id)?;
    let strip = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);
    let active = pane
        .tabs
        .iter()
        .position(|t| *t == pane.active_tab)
        .unwrap_or(0);
    let index = pane.tabs.iter().position(|t| *t == tab)?;
    termin_ui::widgets::tabstrip::StripLayout::new(strip, pane.tabs.len(), active)
        .tab_rect(strip, index)
}

fn workspace_at(
    model: &AppModel,
    win_id: WindowId,
    pos: termin_core::Point,
) -> Option<termin_core::WorkspaceId> {
    let window = model.windows.get(win_id)?;
    if !window.sidebar_visible || !window.sidebar_rect().contains(pos) {
        return None;
    }
    let mut y = window.sidebar_rect().y + termin_ui::widgets::sidebar::CARDS_TOP;
    for &ws_id in &window.workspaces {
        let ws = model.workspaces.get(ws_id)?;
        let lines = 1
            + usize::from(ws.meta.status.is_some())
            + usize::from(ws.meta.git_branch.is_some())
            + ws.meta.paths.iter().take(3).count()
            + usize::from(!ws.meta.ports.is_empty());
        let height = (18.0 + lines as f32 * 18.0).max(46.0);
        let card = Rect::new(
            window.sidebar_rect().x + 7.0,
            y,
            window.sidebar_rect().w - 14.0,
            height,
        );
        if card.contains(pos) {
            return Some(ws_id);
        }
        y += height + 6.0;
    }
    None
}

fn focus_pane(model: &mut AppModel, win_id: WindowId, pane_id: termin_core::PaneId) -> bool {
    let Some(pane) = model.panes.get(pane_id) else {
        return false;
    };
    let ws_id = pane.workspace;
    let Some(ws) = model.workspaces.get_mut(ws_id) else {
        return false;
    };
    if ws.window != win_id {
        return false;
    }
    model.focused_window = Some(win_id);
    ws.focused_pane = pane_id;
    ws.focus_history.retain(|id| *id != pane_id);
    ws.focus_history.insert(0, pane_id);
    true
}

fn handle_tabstrip_click(
    model: &mut AppModel,
    pane_id: termin_core::PaneId,
    rect: Rect,
    pos: termin_core::Point,
    click_count: u8,
) -> bool {
    let Some(pane) = model.panes.get(pane_id) else {
        return false;
    };
    let tab_ids = pane.tabs.clone();
    let active = tab_ids
        .iter()
        .position(|id| *id == pane.active_tab)
        .unwrap_or(0);
    let layout = termin_ui::widgets::tabstrip::StripLayout::new(rect, tab_ids.len(), active);
    for (index, tab_id) in tab_ids.iter().enumerate() {
        let Some(tab_rect) = layout.tab_rect(rect, index) else {
            continue;
        };
        let close_rect = Rect::new(
            tab_rect.right() - 23.0,
            tab_rect.y + (tab_rect.h - 18.0) * 0.5,
            18.0,
            18.0,
        );
        // Any tab's × closes it; closing a pane's last tab collapses the split.
        if close_rect.contains(pos) {
            return model.close_tab(*tab_id);
        }
        if tab_rect.contains(pos) {
            let selected = model.select_tab(pane_id, index);
            if click_count == 2 {
                return model.begin_rename_tab(*tab_id) || selected;
            }
            return selected;
        }
    }
    // Double-click on empty strip space opens a new tab (Safari/Warp style).
    if click_count == 2 && pos.x < layout.actions_x {
        return model.new_tab(pane_id, true).is_some();
    }
    let [new_tab, split_right, split_down] = layout.action_rects(rect);
    if new_tab.contains(pos) {
        return model.new_tab(pane_id, true).is_some();
    }
    if split_right.contains(pos) {
        return model.split_pane(pane_id, Dir::Right, true).is_some();
    }
    if split_down.contains(pos) {
        return model.split_pane(pane_id, Dir::Down, true).is_some();
    }
    false
}

/// Handle mouse wheel and trackpad scroll events.
pub fn handle_scroll(
    model: &mut AppModel,
    win_id: WindowId,
    ev: &ScrollEvent,
    fonts: &TermFonts,
    remainders: &mut rustc_hash::FxHashMap<termin_core::PaneId, f32>,
) -> bool {
    let Some(pane_id) = scroll_target(model, win_id, ev.pos, fonts) else {
        return false;
    };
    let Some(pane) = model.panes.get(pane_id) else {
        return false;
    };
    let Some(session) = model
        .tabs
        .get(pane.active_tab)
        .and_then(|tab| tab.session.clone())
    else {
        return false;
    };

    let Ok(mut term) = session.term.lock() else {
        return false;
    };

    let lines = scroll_lines(
        ev.dy,
        ev.precise,
        fonts.geo.cell_h,
        remainders.entry(pane_id).or_default(),
    );

    if lines != 0 {
        term.scroll_display(lines);
        true
    } else {
        false
    }
}

fn scroll_target(
    model: &mut AppModel,
    win_id: WindowId,
    pos: termin_core::Point,
    fonts: &TermFonts,
) -> Option<termin_core::PaneId> {
    let min_size = termin_core::Size::new(fonts.geo.cell_w * 12.0, fonts.geo.cell_h * 3.0);
    let layout = model.layout(win_id, min_size)?;
    let (pane_id, pane_rect) = layout
        .panes
        .iter()
        .find(|(_, rect)| rect.contains(pos))
        .copied()?;
    let content = PaneWidget::content_rect(pane_rect);
    content.contains(pos).then_some(pane_id)
}

fn scroll_lines(dy: f32, precise: bool, cell_h: f32, remainder: &mut f32) -> i32 {
    if precise {
        let accumulated = *remainder + dy / cell_h.max(1.0);
        let lines = accumulated.trunc() as i32;
        *remainder = accumulated - lines as f32;
        lines
    } else {
        dy.round() as i32
    }
}

fn visible_abs_line(scrollback_len: usize, display_scroll: usize, row: u16) -> u64 {
    scrollback_len.saturating_sub(display_scroll) as u64 + row as u64
}

#[cfg(test)]
mod routing_tests {
    use super::*;
    use termin_core::{Point, Size};
    use termin_text::FontId;
    use termin_ui::draw::CellGeometry;

    fn test_fonts() -> TermFonts {
        TermFonts {
            font: FontId(0),
            geo: CellGeometry {
                cell_w: 8.0,
                cell_h: 16.0,
                baseline: 12.0,
                scale: 2.0,
            },
            underline_y: 13.0,
            underline_h: 1.0,
            ui: termin_ui::draw::UiFont {
                font: FontId(0),
                geo: CellGeometry {
                    cell_w: 8.0,
                    cell_h: 16.0,
                    baseline: 12.0,
                    scale: 2.0,
                },
            },
        }
    }

    fn click(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            kind: MouseKind::Down(MouseButton::Left),
            pos: Point::new(x, y),
            mods: termin_core::Mods::empty(),
            click_count: 1,
        }
    }

    #[test]
    fn sidebar_click_activates_workspace_in_its_own_window() {
        let mut model = AppModel::new();
        let first_window = model.new_window(None, Size::new(800.0, 600.0), None);
        let second_window = model.new_window(None, Size::new(800.0, 600.0), None);
        let second_workspace = model
            .windows
            .get(second_window)
            .expect("window")
            .active_workspace;

        assert!(handle_window_mouse(
            &mut model,
            second_window,
            &click(120.0, 65.0),
            &test_fonts(),
            &|_| false,
        ));
        assert_eq!(model.focused_window, Some(second_window));
        assert_eq!(model.focused_workspace(), Some(second_workspace));
        assert_ne!(model.focused_window, Some(first_window));
    }

    #[test]
    fn tabstrip_click_selects_tab_in_the_clicked_pane() {
        let mut model = AppModel::new();
        let window = model.new_window(None, Size::new(900.0, 600.0), None);
        let pane = model.focused_pane().expect("pane");
        let first_tab = model.focused_tab().expect("first tab");
        let second_tab = model.new_tab(pane, true).expect("new tab");
        assert!(model.select_tab(pane, 0));

        let layout = model
            .layout(window, Size::new(96.0, 48.0))
            .expect("layout")
            .clone();
        let pane_rect = layout.rect_of(pane).expect("pane rect");
        let strip = Rect::new(pane_rect.x, pane_rect.y, pane_rect.w, TAB_STRIP_HEIGHT);
        let actions_x = strip.right() - 26.0 * 3.0 - 4.0;
        let available = actions_x - (strip.x + 6.0);
        let tab_w = (available / 2.0).clamp(72.0, 220.0).min(available);
        let second_tab_x = strip.x + 6.0 + tab_w + 2.0 + tab_w * 0.5;
        assert!(handle_window_mouse(
            &mut model,
            window,
            &click(second_tab_x, strip.y + 12.0),
            &test_fonts(),
            &|_| false,
        ));
        assert_eq!(model.panes.get(pane).unwrap().active_tab, second_tab);
        assert_ne!(model.panes.get(pane).unwrap().active_tab, first_tab);
    }

    fn dbl(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            click_count: 2,
            ..click(x, y)
        }
    }

    #[test]
    fn double_click_empty_strip_opens_new_tab() {
        let mut model = AppModel::new();
        let pane = model.new_window(None, Size::new(900.0, 600.0), None);
        let pane_id = model.focused_pane().unwrap();
        let rect = model
            .layout(pane, Size::new(96.0, 48.0))
            .unwrap()
            .rect_of(pane_id)
            .unwrap();
        // Far right of the single tab, left of the action buttons.
        let x = rect.x + rect.w * 0.6;
        assert!(handle_window_mouse(
            &mut model,
            pane,
            &dbl(x, rect.y + 12.0),
            &test_fonts(),
            &|_| false
        ));
        assert_eq!(model.panes.get(pane_id).unwrap().tabs.len(), 2);
    }

    #[test]
    fn double_click_workspace_row_starts_rename_and_click_elsewhere_commits() {
        let mut model = AppModel::new();
        let win = model.new_window(None, Size::new(900.0, 600.0), None);
        let ws = model.focused_workspace().unwrap();
        assert!(handle_window_mouse(
            &mut model,
            win,
            &dbl(120.0, 65.0),
            &test_fonts(),
            &|_| false
        ));
        assert!(model.rename.is_some());
        // Click inside the row keeps editing.
        handle_window_mouse(&mut model, win, &click(120.0, 65.0), &test_fonts(), &|_| {
            false
        });
        assert!(model.rename.is_some());
        model.rename_text("Mine");
        // Click in the terminal area commits.
        handle_window_mouse(
            &mut model,
            win,
            &click(600.0, 300.0),
            &test_fonts(),
            &|_| false,
        );
        assert!(model.rename.is_none());
        assert_eq!(
            model.workspaces.get(ws).unwrap().name.as_deref(),
            Some("Mine")
        );
    }

    #[test]
    fn hovered_inactive_close_button_closes_that_tab() {
        let mut model = AppModel::new();
        model.new_window(None, Size::new(900.0, 600.0), None);
        let pane = model.focused_pane().unwrap();
        let first = model.focused_tab().unwrap();
        let second = model.new_tab(pane, true).unwrap();
        model.select_tab(pane, 0);
        let rect = Rect::new(0.0, 0.0, 600.0, TAB_STRIP_HEIGHT);
        let layout = termin_ui::widgets::tabstrip::StripLayout::new(rect, 2, 0);
        let tab = layout.tab_rect(rect, 1).unwrap();
        let pos = Point::new(tab.right() - 14.0, tab.y + tab.h * 0.5);
        assert!(handle_tabstrip_click(&mut model, pane, rect, pos, 1));
        assert!(model.tabs.contains(first));
        assert!(!model.tabs.contains(second));
    }

    #[test]
    fn closing_single_tab_of_split_pane_collapses_split() {
        let mut model = AppModel::new();
        model.new_window(None, Size::new(1000.0, 700.0), None);
        let first_pane = model.focused_pane().unwrap();
        let (second_pane, second_tab) = model
            .split_pane(first_pane, Dir::Right, true)
            .expect("split pane");
        let rect = Rect::new(0.0, 0.0, 400.0, TAB_STRIP_HEIGHT);
        let layout = termin_ui::widgets::tabstrip::StripLayout::new(rect, 1, 0);
        let tab = layout.tab_rect(rect, 0).unwrap();
        let pos = Point::new(tab.right() - 14.0, tab.y + tab.h * 0.5);
        assert!(handle_tabstrip_click(&mut model, second_pane, rect, pos, 1));
        assert!(!model.tabs.contains(second_tab));
        assert!(!model.panes.contains(second_pane));
        assert!(model.panes.contains(first_pane));
    }

    #[test]
    fn pane_body_click_focuses_the_pane_under_the_pointer() {
        let mut model = AppModel::new();
        let window = model.new_window(None, Size::new(1000.0, 700.0), None);
        let first_pane = model.focused_pane().expect("initial pane");
        let (second_pane, _) = model
            .split_pane(first_pane, Dir::Right, true)
            .expect("split pane");
        let workspace = model.focused_workspace().expect("workspace");
        model.workspaces.get_mut(workspace).unwrap().focused_pane = first_pane;

        let layout = model
            .layout(window, Size::new(96.0, 48.0))
            .expect("layout")
            .clone();
        let second_rect = layout.rect_of(second_pane).expect("second pane rect");
        let hover = MouseEvent {
            kind: MouseKind::Move,
            pos: Point::new(
                second_rect.x + 20.0,
                second_rect.y + TAB_STRIP_HEIGHT + 20.0,
            ),
            mods: termin_core::Mods::empty(),
            click_count: 0,
        };
        assert!(!handle_window_mouse(
            &mut model,
            window,
            &hover,
            &test_fonts(),
            &|_| false,
        ));
        assert_eq!(
            model.workspaces.get(workspace).unwrap().focused_pane,
            first_pane
        );
        assert!(handle_window_mouse(
            &mut model,
            window,
            &click(
                second_rect.x + 20.0,
                second_rect.y + TAB_STRIP_HEIGHT + 20.0
            ),
            &test_fonts(),
            &|_| false,
        ));
        assert_eq!(
            model.workspaces.get(workspace).unwrap().focused_pane,
            second_pane
        );
    }

    #[test]
    fn precise_scroll_accumulates_subcell_deltas_without_losing_them() {
        let mut remainder = 0.0;
        assert_eq!(scroll_lines(5.0, true, 16.0, &mut remainder), 0);
        assert!((remainder - 0.3125).abs() < f32::EPSILON);
        assert_eq!(scroll_lines(6.0, true, 16.0, &mut remainder), 0);
        assert_eq!(scroll_lines(6.0, true, 16.0, &mut remainder), 1);
    }

    #[test]
    fn selection_line_accounts_for_scrolled_display_offset() {
        assert_eq!(visible_abs_line(100, 7, 0), 93);
        assert_eq!(visible_abs_line(3, 8, 2), 2);
    }

    #[test]
    fn scroll_target_uses_event_window_and_pointer_pane() {
        let mut model = AppModel::new();
        let first_window = model.new_window(None, Size::new(1000.0, 700.0), None);
        let (first_pane, _) = model
            .split_pane(model.focused_pane().unwrap(), Dir::Right, true)
            .unwrap();
        let second_window = model.new_window(None, Size::new(1000.0, 700.0), None);
        let second_pane = model.focused_pane().unwrap();
        let first_layout = model
            .layout(first_window, Size::new(96.0, 48.0))
            .unwrap()
            .clone();
        let second_layout = model
            .layout(second_window, Size::new(96.0, 48.0))
            .unwrap()
            .clone();
        let first_pane_rect = first_layout.rect_of(first_pane).unwrap();
        let second_pane_rect = second_layout.rect_of(second_pane).unwrap();

        assert_eq!(
            scroll_target(
                &mut model,
                first_window,
                Point::new(
                    first_pane_rect.x + 20.0,
                    first_pane_rect.y + TAB_STRIP_HEIGHT + 20.0,
                ),
                &test_fonts(),
            ),
            Some(first_pane)
        );
        assert_eq!(
            scroll_target(
                &mut model,
                second_window,
                Point::new(
                    second_pane_rect.x + 20.0,
                    second_pane_rect.y + TAB_STRIP_HEIGHT + 20.0,
                ),
                &test_fonts(),
            ),
            Some(second_pane)
        );
    }
}
