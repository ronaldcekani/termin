//! Input editor state machine and app-facing editing hooks (Phase 3B).
//!
//! The model contract intentionally has no editor field yet. `EditorController`
//! is the per-tab state 2A can own beside the model and pass to these functions.
//! It keeps editor behavior testable without coupling the editor crate to AppKit.

use super::completion::{CompletionRequest, CompletionRequestId};
use crate::{action::Action, model::AppModel};
use std::time::{Duration, Instant};
use termin_editor::{Candidate, EditorState};

/// Model action hook. Editor state is owned by the 2A runtime beside
/// `AppModel`; this stub remains for the frozen dispatcher until that owner
/// passes per-tab controller state to the handler.
pub fn handle(_model: &mut AppModel, _action: &Action) -> bool {
    false
}

/// Shell integration mark used by the block/editor state machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellMark {
    /// OSC 133;A: shell is presenting a prompt.
    Prompt,
    /// OSC 133;B: prompt input region has begun.
    Input,
    /// OSC 133;C: command execution has begun.
    Command,
    /// OSC 133;D;status: command has finished.
    Finished(i32),
}

/// Where keyboard input should go for a tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputRoute {
    /// The editor owns key/text input.
    Editor,
    /// A submitted command is waiting for OSC 133;C. Input stays controlled
    /// until C arrives or the 2 s timeout enables raw fallback.
    AwaitingCommand,
    /// Encode the key normally and send it to the PTY.
    Pty,
    /// The integration handshake timed out; stay raw until the next prompt mark.
    RawFallback,
}

/// Result of submitting editor text to the PTY.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submit {
    /// Exact bytes to enqueue on the PTY writer.
    pub bytes: Vec<u8>,
    /// Command text before transport framing, for the block/history layer.
    pub command: String,
}

/// Per-tab editor and prompt lifecycle state, owned by the app's main thread.
#[derive(Clone, Debug)]
pub struct EditorController {
    pub editor: EditorState,
    integration_seen: bool,
    at_prompt: bool,
    running: bool,
    raw_fallback: bool,
    submitted_at: Option<Instant>,
    awaiting_command: bool,
    completion_request: CompletionRequestId,
    pub type_ahead: bool,
}

impl Default for EditorController {
    fn default() -> Self {
        Self::new(true)
    }
}

impl EditorController {
    pub fn new(type_ahead: bool) -> Self {
        Self {
            editor: EditorState::new(),
            integration_seen: false,
            at_prompt: false,
            running: false,
            raw_fallback: false,
            submitted_at: None,
            awaiting_command: false,
            completion_request: CompletionRequestId(0),
            type_ahead,
        }
    }

    pub fn route(&self) -> InputRoute {
        if self.raw_fallback {
            InputRoute::RawFallback
        } else if self.awaiting_command {
            InputRoute::AwaitingCommand
        } else if self.running {
            InputRoute::Pty
        } else if self.at_prompt || !self.integration_seen {
            // Before integration is detected the caller continues classic mode;
            // once a prompt is available the editor owns input.
            if self.at_prompt {
                InputRoute::Editor
            } else {
                InputRoute::Pty
            }
        } else {
            InputRoute::Pty
        }
    }

    /// Feed an OSC 133 mark. `A` establishes availability; `B` opens editor input.
    pub fn on_mark(&mut self, mark: ShellMark) {
        match mark {
            ShellMark::Prompt => {
                self.integration_seen = true;
                self.at_prompt = false;
                self.running = false;
                self.raw_fallback = false;
                self.submitted_at = None;
                self.awaiting_command = false;
            }
            ShellMark::Input => {
                self.integration_seen = true;
                self.at_prompt = true;
                self.running = false;
                self.raw_fallback = false;
                self.invalidate_completions();
                // A secondary B can describe a continuation prompt after
                // submit; keep the submit deadline active until C or timeout.
            }
            ShellMark::Command => {
                self.at_prompt = false;
                self.running = true;
                self.raw_fallback = false;
                self.submitted_at = None;
                self.awaiting_command = false;
            }
            ShellMark::Finished(_) => {
                self.running = false;
                self.at_prompt = false;
            }
        }
    }

    /// Submit current editor text. The caller supplies the current DEC 2004 mode.
    /// Empty submissions still send CR. Multi-line text is preserved verbatim for
    /// bash 3.2 compatibility when bracketed paste is unavailable.
    pub fn submit(&mut self, now: Instant, bracketed_paste: bool) -> Option<Submit> {
        if self.route() != InputRoute::Editor || self.editor.menu.is_some() {
            return None;
        }
        let command = self.editor.submit();
        self.invalidate_completions();
        let mut bytes = Vec::with_capacity(command.len() + 16);
        if bracketed_paste && !command.is_empty() {
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(command.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
        } else if !command.is_empty() {
            bytes.extend_from_slice(command.as_bytes());
        }
        bytes.push(b'\r');
        self.at_prompt = false;
        self.submitted_at = Some(now);
        self.awaiting_command = true;
        Some(Submit { bytes, command })
    }

    /// After 2 s without `C`, release controlled input into raw fallback.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some(started) = self.submitted_at else {
            return false;
        };
        if self.awaiting_command && now.saturating_duration_since(started) >= Duration::from_secs(2)
        {
            self.raw_fallback = true;
            self.awaiting_command = false;
            self.at_prompt = false;
            self.submitted_at = None;
            return true;
        }
        false
    }

    /// Insert text from keyboard/IME. During command execution this is optional
    /// type-ahead and will remain in the editor when the next prompt arrives.
    pub fn insert_text(&mut self, text: &str) -> bool {
        let inserted = match self.route() {
            InputRoute::Editor => self.editor.try_insert(text),
            InputRoute::AwaitingCommand if self.type_ahead => self.editor.try_insert(text),
            InputRoute::Pty if self.running && self.type_ahead => self.editor.try_insert(text),
            _ => false,
        };
        if inserted && !text.is_empty() {
            self.invalidate_completions();
        }
        inserted
    }

    /// Refresh synchronous completions. The caller can schedule `result.jobs` on
    /// the worker and later merge returned candidates into `editor.menu`.
    pub fn refresh_completion(
        &mut self,
        context: &termin_editor::CompletionContext,
    ) -> CompletionRequest {
        self.completion_request = CompletionRequestId(self.completion_request.0.wrapping_add(1));
        super::completion::refresh(&mut self.editor, self.completion_request, context)
    }

    /// Merge a worker completion only if it belongs to the latest editor query.
    pub fn merge_completion(
        &mut self,
        request_id: CompletionRequestId,
        candidates: Vec<Candidate>,
    ) -> bool {
        if request_id != self.completion_request {
            return false;
        }
        super::completion::merge_async(
            &mut self.editor,
            request_id,
            self.completion_request,
            candidates,
        )
    }

    /// Invalidate outstanding worker replies after cursor movement or an edit
    /// performed through the exposed `editor` state.
    pub fn invalidate_completions(&mut self) {
        self.completion_request = CompletionRequestId(self.completion_request.0.wrapping_add(1));
    }

    /// Open the `cwd` chip directory menu (`components.md §5, §15`).
    ///
    /// Populates the editor with `cd ` (if empty) and opens a completion menu
    /// containing recent directories so clicking a directory changes into it.
    pub fn open_cwd_directory_menu(&mut self, recent_dirs: &[String]) -> bool {
        if recent_dirs.is_empty() {
            return false;
        }
        if self.editor.text.trim().is_empty() {
            self.editor.clear();
            self.editor.insert("cd ");
        }
        let candidates: Vec<Candidate> = recent_dirs
            .iter()
            .enumerate()
            .map(|(idx, dir)| Candidate {
                label: dir.clone(),
                detail: Some("Recent directory".to_string()),
                insert_text: dir.clone(),
                score: (1000usize.saturating_sub(idx * 10)) as u32,
            })
            .collect();
        self.editor.menu = Some(termin_editor::CompletionMenu {
            candidates,
            selected: 0,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submits_bracketed_and_plain_multiline_commands() {
        let now = Instant::now();
        let mut state = EditorController::default();
        state.on_mark(ShellMark::Prompt);
        state.on_mark(ShellMark::Input);
        state.editor.insert("echo one\necho two\n  \n");
        let bracketed = state.submit(now, true).unwrap();
        assert_eq!(bracketed.command, "echo one\necho two");
        assert_eq!(bracketed.bytes, b"\x1b[200~echo one\necho two\x1b[201~\r");

        state.on_mark(ShellMark::Command);
        state.on_mark(ShellMark::Finished(0));
        state.on_mark(ShellMark::Prompt);
        state.on_mark(ShellMark::Input);
        state.editor.insert("echo one\necho two");
        let plain = state.submit(now, false).unwrap();
        assert_eq!(plain.bytes, b"echo one\necho two\r");
    }

    #[test]
    fn empty_submit_and_completion_guard() {
        let mut state = EditorController::default();
        state.on_mark(ShellMark::Input);
        state.editor.menu = Some(termin_editor::CompletionMenu::default());
        assert!(state.submit(Instant::now(), true).is_none());
        state.editor.menu = None;
        assert_eq!(state.submit(Instant::now(), true).unwrap().bytes, b"\r");
    }

    #[test]
    fn typeahead_survives_running_command_and_submit_waits_for_command_mark() {
        let now = Instant::now();
        let mut state = EditorController::default();
        state.on_mark(ShellMark::Input);
        state.editor.insert("sleep 1");
        state.submit(now, false).unwrap();
        assert_eq!(state.route(), InputRoute::AwaitingCommand);
        assert!(state.insert_text("next"));
        assert_eq!(state.route(), InputRoute::AwaitingCommand);
        state.on_mark(ShellMark::Command);
        assert_eq!(state.route(), InputRoute::Pty);
        assert_eq!(state.editor.text, "next");
        state.on_mark(ShellMark::Finished(0));
        state.on_mark(ShellMark::Prompt);
        state.on_mark(ShellMark::Input);
        assert_eq!(state.editor.text, "next");

        state.editor.clear();
        state.submit(now, false).unwrap();
        assert_eq!(state.route(), InputRoute::AwaitingCommand);
        // B may be emitted for a shell continuation; it does not cancel wait.
        state.on_mark(ShellMark::Input);
        assert_eq!(state.route(), InputRoute::AwaitingCommand);
        assert!(!state.tick(now + Duration::from_millis(1_999)));
        assert_eq!(state.route(), InputRoute::AwaitingCommand);
        assert!(state.tick(now + Duration::from_secs(2)));
        assert_eq!(state.route(), InputRoute::RawFallback);
        state.on_mark(ShellMark::Prompt);
        assert_eq!(state.route(), InputRoute::Pty);
    }

    #[test]
    fn text_edit_invalidates_an_outstanding_completion_reply() {
        let mut state = EditorController::default();
        state.on_mark(ShellMark::Input);
        state.insert_text("gi");
        let request = state.refresh_completion(&termin_editor::CompletionContext::default());
        assert!(state.insert_text("t"));
        assert!(!state.merge_completion(
            request.id,
            vec![Candidate {
                label: "stale".into(),
                detail: None,
                insert_text: "stale".into(),
                score: 0,
            }]
        ));
    }

    #[test]
    fn cwd_chip_opens_recent_directory_completion_menu() {
        let mut state = EditorController::default();
        state.on_mark(ShellMark::Input);
        let dirs = vec![
            "~/projects/termin".to_string(),
            "~/projects/docs".to_string(),
        ];
        assert!(state.open_cwd_directory_menu(&dirs));
        assert_eq!(state.editor.text, "cd ");
        let menu = state.editor.menu.as_ref().expect("menu should be open");
        assert_eq!(menu.candidates.len(), 2);
        assert_eq!(menu.candidates[0].insert_text, "~/projects/termin");
    }
}
