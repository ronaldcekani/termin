//! Warp-style block mode runtime: per-tab editor state, key routing, history
//! and scroll position (`termin-app::block_mode`).
//!
//! See `docs/specs/app-model-and-ui.md §5.2–5.3` and `docs/specs/input-editor.md`.
//! The model stays frozen; this state lives beside it on the main thread, keyed
//! by tab, and is fed by the VT's OSC 133 block events.

use std::time::Instant;

use rustc_hash::FxHashMap;
use termin_core::{Key, KeyEvent, Mods, NamedKey, TabId};
use termin_editor::{HistoryEntry, HistoryIndex};
use termin_vt::{BlockEvent, Modes, Terminal};

use termin_ui::block_view::VisualLineMap;

use crate::controllers::editor::{EditorController, InputRoute, ShellMark};
use crate::model::types::{Tab, TabMode};

/// Block-mode state for one tab.
#[derive(Debug, Default)]
pub struct BlockTab {
    pub ctl: EditorController,
    /// Pixels scrolled up from the bottom of the block list (0 = follow output).
    pub scroll_from_bottom: f32,
    /// Prefix-history navigation cursor: (history index, text before navigation).
    history_nav: Option<(usize, String)>,
    /// Visual line map, rebuilt in place each frame.
    pub map: VisualLineMap,
}

impl BlockTab {
    /// The input editor is shown while the shell waits at a prompt.
    pub fn editor_visible(&self) -> bool {
        matches!(
            self.ctl.route(),
            InputRoute::Editor | InputRoute::AwaitingCommand
        )
    }
}

/// Outcome of offering a key to the block editor.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// Not handled: fall back to keybindings / PTY.
    Pass,
    /// Handled; redraw.
    Handled,
    /// Write these bytes to the PTY (submit, control keys), then redraw.
    Write(Vec<u8>),
}

/// Block mode state for all tabs.
#[derive(Debug)]
pub struct BlockMode {
    pub enabled: bool,
    pub tabs: FxHashMap<TabId, BlockTab>,
    history: HistoryIndex,
    history_loaded: bool,
}

impl BlockMode {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            tabs: FxHashMap::default(),
            history: HistoryIndex::new(),
            history_loaded: false,
        }
    }

    /// Feed a VT block event for `tab`.
    pub fn on_block_event(&mut self, tab: TabId, ev: BlockEvent) {
        let state = self.tabs.entry(tab).or_default();
        match ev {
            BlockEvent::PromptStarted(_) => {
                // The integration scripts emit B right after A inside PS1, and B has
                // no VT event of its own, so A opens the editor.
                state.ctl.on_mark(ShellMark::Prompt);
                state.ctl.on_mark(ShellMark::Input);
                state.scroll_from_bottom = 0.0;
            }
            BlockEvent::CommandStarted(_) => state.ctl.on_mark(ShellMark::Command),
            BlockEvent::CommandFinished(_, code) => state.ctl.on_mark(ShellMark::Finished(code)),
            _ => {}
        }
    }

    /// Whether `tab` renders as blocks right now.
    pub fn is_active(&self, tab: &Tab, term: &Terminal) -> bool {
        if !self.enabled || tab.mode == TabMode::Classic || term.is_alt_screen() {
            return false;
        }
        tab.mode == TabMode::Block
            || self
                .tabs
                .get(&tab.id)
                .is_some_and(|s| s.ctl.integration_seen())
    }

    /// Forget state for closed tabs.
    pub fn retain(&mut self, mut alive: impl FnMut(TabId) -> bool) {
        self.tabs.retain(|id, _| alive(*id));
    }

    fn ensure_history(&mut self) {
        if self.history_loaded {
            return;
        }
        self.history_loaded = true;
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = std::path::Path::new(&home).join(".zsh_history");
        if let Ok(bytes) = std::fs::read(path) {
            for entry in termin_editor::parse_zsh_history_bytes(&bytes, "") {
                self.history.push(entry);
            }
        }
    }

    /// Insert committed text into the editor of `tab` if it owns input.
    pub fn insert_text(&mut self, tab: TabId, cwd: &str, text: &str) -> bool {
        self.ensure_history();
        let Some(state) = self.tabs.get_mut(&tab) else {
            return false;
        };
        if !state.editor_visible() || !state.ctl.insert_text(text) {
            return false;
        }
        state.history_nav = None;
        refresh_suggestion(state, &self.history, cwd);
        true
    }

    /// Offer a key to the editor of `tab`. `term_modes` decides bracketed submit.
    pub fn handle_key(
        &mut self,
        tab: TabId,
        cwd: &str,
        ev: &KeyEvent,
        term_modes: Modes,
    ) -> KeyOutcome {
        self.ensure_history();
        let Some(state) = self.tabs.get_mut(&tab) else {
            return KeyOutcome::Pass;
        };
        if !ev.pressed || state.ctl.route() != InputRoute::Editor {
            return KeyOutcome::Pass;
        }
        let mods = ev.mods;
        let shift = mods.contains(Mods::SHIFT);
        let alt = mods.contains(Mods::ALT);
        let cmd = mods.contains(Mods::CMD);
        let ctrl = mods.contains(Mods::CTRL);
        // Typed characters arrive as key events carrying committed text.
        if !cmd && !ctrl {
            if let Some(text) = ev.text.as_deref() {
                if !text.is_empty() && !text.chars().any(char::is_control) {
                    if state.ctl.insert_text(text) {
                        state.history_nav = None;
                        refresh_suggestion(state, &self.history, cwd);
                    }
                    return KeyOutcome::Handled;
                }
            }
        }
        let ed = &mut state.ctl.editor;
        let mut edited = false;
        match ev.key {
            Key::Named(NamedKey::Enter) if shift || alt => {
                ed.insert_newline();
                edited = true;
            }
            Key::Named(NamedKey::Enter) => {
                if let Some(menu) = ed.menu.take() {
                    if let Some(c) = menu.selected_candidate() {
                        let insert = c.insert_text.clone();
                        ed.insert(&insert);
                    }
                    edited = true;
                } else {
                    let bracketed = term_modes.contains(Modes::BRACKETED_PASTE);
                    let Some(submit) = state.ctl.submit(Instant::now(), bracketed) else {
                        return KeyOutcome::Handled;
                    };
                    state.history_nav = None;
                    if !submit.command.is_empty() {
                        self.history.push(HistoryEntry::new(
                            unix_now(),
                            None,
                            None,
                            cwd,
                            "zsh",
                            submit.command.as_str(),
                        ));
                    }
                    return KeyOutcome::Write(submit.bytes);
                }
            }
            Key::Named(NamedKey::Backspace) if alt || ctrl => edited = ed.delete_word_backward(),
            Key::Named(NamedKey::Backspace) if cmd => edited = ed.delete_to_line_start(),
            Key::Named(NamedKey::Backspace) => edited = ed.backspace(),
            Key::Named(NamedKey::Delete) => edited = ed.delete_forward(),
            Key::Named(NamedKey::ArrowLeft) if cmd => ed.move_to_line_start(shift),
            Key::Named(NamedKey::ArrowLeft) if alt => ed.move_word_left(shift),
            Key::Named(NamedKey::ArrowLeft) => ed.move_left(shift),
            Key::Named(NamedKey::ArrowRight) if cmd => {
                if !ed.accept_suggestion() {
                    ed.move_to_line_end(shift);
                }
                edited = true;
            }
            Key::Named(NamedKey::ArrowRight) if alt => {
                if !ed.accept_suggestion_word() {
                    ed.move_word_right(shift);
                }
                edited = true;
            }
            Key::Named(NamedKey::ArrowRight) => {
                let at_end = ed.cursor >= ed.text.len();
                if at_end && ed.accept_suggestion() {
                    edited = true;
                } else {
                    ed.move_right(shift);
                }
            }
            Key::Named(NamedKey::Home) => ed.move_to_line_start(shift),
            Key::Named(NamedKey::End) => ed.move_to_line_end(shift),
            Key::Named(NamedKey::ArrowUp) | Key::Named(NamedKey::ArrowDown) => {
                let up = matches!(ev.key, Key::Named(NamedKey::ArrowUp));
                if let Some(menu) = ed.menu.as_mut() {
                    if up {
                        menu.select_prev();
                    } else {
                        menu.select_next();
                    }
                    return KeyOutcome::Handled;
                }
                let moved = if up {
                    ed.move_up(shift)
                } else {
                    ed.move_down(shift)
                };
                if !moved {
                    navigate_history(state, &self.history, cwd, up);
                }
                return KeyOutcome::Handled;
            }
            Key::Named(NamedKey::Escape) => {
                ed.handle_escape();
            }
            Key::Named(NamedKey::Tab) => {
                let ctx = termin_editor::CompletionContext {
                    cwd: cwd.into(),
                    env_path: std::env::var("PATH").unwrap_or_default(),
                    shell_kind: "zsh".into(),
                    ..Default::default()
                };
                state.ctl.refresh_completion(&ctx);
                let ed = &mut state.ctl.editor;
                // A single candidate completes inline, like a shell.
                if ed.menu.as_ref().is_some_and(|m| m.candidates.len() == 1) {
                    if let Some(menu) = ed.menu.take() {
                        if let Some(c) = menu.selected_candidate() {
                            let insert = c.insert_text.clone();
                            ed.insert(&insert);
                        }
                    }
                }
                return KeyOutcome::Handled;
            }
            Key::Char(c) if ctrl && !cmd => match c.to_ascii_lowercase() {
                'c' => {
                    if ed.text.is_empty() {
                        return KeyOutcome::Write(vec![0x03]);
                    }
                    ed.clear();
                    edited = true;
                }
                'd' if ed.text.is_empty() => return KeyOutcome::Write(vec![0x04]),
                'd' => edited = ed.delete_forward(),
                'a' => ed.move_to_line_start(false),
                'e' => ed.move_to_line_end(false),
                'u' => edited = ed.delete_to_line_start(),
                'k' => edited = ed.delete_to_line_end(),
                'w' => edited = ed.delete_word_backward(),
                'y' => {
                    ed.yank();
                    edited = true;
                }
                'l' => return KeyOutcome::Write(vec![0x0c]),
                _ => return KeyOutcome::Handled,
            },
            Key::Char('z') if cmd && !shift => edited = ed.undo(),
            Key::Char('z') if cmd && shift => edited = ed.redo(),
            Key::Char('a') if cmd => {
                ed.move_to_buffer_start(false);
                ed.move_to_buffer_end(true);
            }
            // Other ⌘ chords belong to keybindings.
            _ if cmd => return KeyOutcome::Pass,
            // Printable text arrives through IME commit; swallow the raw key.
            _ => return KeyOutcome::Handled,
        }
        if edited {
            state.ctl.invalidate_completions();
            state.history_nav = None;
            refresh_suggestion(state, &self.history, cwd);
        }
        KeyOutcome::Handled
    }

    /// Paste text into the editor of `tab` if it owns input.
    pub fn paste(&mut self, tab: TabId, cwd: &str, text: &str) -> bool {
        self.insert_text(tab, cwd, text)
    }

    /// Scroll the block list of `tab` by `dy` pixels (positive = towards older).
    pub fn scroll(&mut self, tab: TabId, dy: f32) -> bool {
        let Some(state) = self.tabs.get_mut(&tab) else {
            return false;
        };
        let next = (state.scroll_from_bottom + dy).max(0.0);
        let changed = next != state.scroll_from_bottom;
        state.scroll_from_bottom = next;
        changed
    }
}

fn refresh_suggestion(state: &mut BlockTab, history: &HistoryIndex, cwd: &str) {
    let ed = &mut state.ctl.editor;
    ed.retokenize();
    ed.suggestion = if ed.cursor >= ed.text.len() && !ed.text.contains('\n') {
        history.suggest(&ed.text, cwd)
    } else {
        None
    };
}

fn navigate_history(state: &mut BlockTab, history: &HistoryIndex, cwd: &str, up: bool) {
    let (current, prefix) = match state.history_nav.take() {
        Some((idx, prefix)) => (Some(idx), prefix),
        None if !up => return,
        None => (None, state.ctl.editor.text.clone()),
    };
    let ed = &mut state.ctl.editor;
    match history.navigate_prefix(&prefix, cwd, current, up) {
        Some((idx, command)) => {
            ed.clear();
            ed.insert(&command);
            ed.retokenize();
            ed.suggestion = None;
            state.history_nav = Some((idx, prefix));
        }
        None if !up => {
            // Past the newest entry: restore what was typed.
            ed.clear();
            ed.insert(&prefix);
            ed.retokenize();
        }
        None => state.history_nav = current.map(|idx| (idx, prefix)),
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::BlockId;

    fn key(key: Key, mods: Mods) -> KeyEvent {
        KeyEvent {
            key,
            physical: 0,
            mods,
            text: None,
            repeat: false,
            pressed: true,
        }
    }

    fn at_prompt() -> (BlockMode, TabId) {
        let mut bm = BlockMode::new(true);
        bm.history_loaded = true; // keep tests independent of ~/.zsh_history
        let tab = TabId(1);
        bm.on_block_event(tab, BlockEvent::PromptStarted(BlockId(0)));
        (bm, tab)
    }

    #[test]
    fn typing_and_enter_submit_to_pty() {
        let (mut bm, tab) = at_prompt();
        assert!(bm.insert_text(tab, "/tmp", "echo hi"));
        let out = bm.handle_key(
            tab,
            "/tmp",
            &key(Key::Named(NamedKey::Enter), Mods::empty()),
            Modes::BRACKETED_PASTE,
        );
        assert_eq!(
            out,
            KeyOutcome::Write(b"\x1b[200~echo hi\x1b[201~\r".to_vec())
        );
        assert!(
            bm.tabs[&tab].editor_visible(),
            "awaiting C keeps editor shown"
        );
        bm.on_block_event(tab, BlockEvent::CommandStarted(BlockId(0)));
        assert!(!bm.tabs[&tab].editor_visible());
        // While running, keys go to the PTY.
        let out = bm.handle_key(
            tab,
            "/tmp",
            &key(Key::Char('c'), Mods::CTRL),
            Modes::empty(),
        );
        assert_eq!(out, KeyOutcome::Pass);
    }

    #[test]
    fn history_drives_suggestion_and_up_arrow() {
        let (mut bm, tab) = at_prompt();
        bm.insert_text(tab, "/tmp", "cargo build");
        bm.handle_key(
            tab,
            "/tmp",
            &key(Key::Named(NamedKey::Enter), Mods::empty()),
            Modes::empty(),
        );
        bm.on_block_event(tab, BlockEvent::CommandStarted(BlockId(0)));
        bm.on_block_event(tab, BlockEvent::CommandFinished(BlockId(0), 0));
        bm.on_block_event(tab, BlockEvent::PromptStarted(BlockId(1)));

        bm.insert_text(tab, "/tmp", "car");
        let sugg = bm.tabs[&tab]
            .ctl
            .editor
            .suggestion
            .as_ref()
            .map(|s| s.text.clone());
        assert_eq!(sugg.as_deref(), Some("go build"));
        let right = key(Key::Named(NamedKey::ArrowRight), Mods::empty());
        bm.handle_key(tab, "/tmp", &right, Modes::empty());
        assert_eq!(bm.tabs[&tab].ctl.editor.text, "cargo build");

        bm.handle_key(
            tab,
            "/tmp",
            &key(Key::Char('c'), Mods::CTRL),
            Modes::empty(),
        );
        assert!(bm.tabs[&tab].ctl.editor.text.is_empty());
        bm.handle_key(
            tab,
            "/tmp",
            &key(Key::Named(NamedKey::ArrowUp), Mods::empty()),
            Modes::empty(),
        );
        assert_eq!(bm.tabs[&tab].ctl.editor.text, "cargo build");
    }

    #[test]
    fn key_events_with_text_insert_into_editor() {
        let (mut bm, tab) = at_prompt();
        let mut ev = key(Key::Char('l'), Mods::empty());
        ev.text = Some("l".into());
        assert_eq!(
            bm.handle_key(tab, "/", &ev, Modes::empty()),
            KeyOutcome::Handled
        );
        let mut ev = key(Key::Named(NamedKey::Space), Mods::empty());
        ev.text = Some(" ".into());
        bm.handle_key(tab, "/", &ev, Modes::empty());
        assert_eq!(bm.tabs[&tab].ctl.editor.text, "l ");
    }

    #[test]
    fn ctrl_c_on_empty_editor_interrupts_and_cmd_chords_pass() {
        let (mut bm, tab) = at_prompt();
        let out = bm.handle_key(tab, "/", &key(Key::Char('c'), Mods::CTRL), Modes::empty());
        assert_eq!(out, KeyOutcome::Write(vec![0x03]));
        let out = bm.handle_key(tab, "/", &key(Key::Char('t'), Mods::CMD), Modes::empty());
        assert_eq!(out, KeyOutcome::Pass);
    }

    #[test]
    fn scroll_clamps_at_bottom_and_prompt_resets_follow() {
        let (mut bm, tab) = at_prompt();
        assert!(!bm.scroll(tab, -10.0));
        assert!(bm.scroll(tab, 40.0));
        assert_eq!(bm.tabs[&tab].scroll_from_bottom, 40.0);
        bm.on_block_event(tab, BlockEvent::PromptStarted(BlockId(2)));
        assert_eq!(bm.tabs[&tab].scroll_from_bottom, 0.0);
    }
}
