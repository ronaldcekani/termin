//! Command blocks model (OSC 133 / FinalTerm shell integration).
//!
//! See `docs/specs/terminal-core.md §6`.

use bitflags::bitflags;
use std::collections::VecDeque;
use std::time::Instant;
use termin_core::BlockId;

use crate::cell::AbsLine;

bitflags! {
    /// Command execution block flags.
    ///
    /// See `docs/specs/terminal-core.md §6.2`.
    #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
    pub struct BlockFlags: u8 {
        /// Alt-screen was entered while this block was running.
        const ALT_SCREEN_USED = 1 << 0;
        /// Block cleared by ED 2 / ED 3 / `clear`.
        const CLEARED         = 1 << 1;
        /// Block start row was evicted from scrollback.
        const TRUNCATED       = 1 << 2;
        /// Block was interrupted (new prompt arrived before completion).
        const INTERRUPTED     = 1 << 3;
    }
}

/// FinalTerm / OSC 133 command block events.
///
/// See `docs/specs/terminal-core.md §6`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockEvent {
    PromptStarted(BlockId),
    CommandStarted(BlockId),
    CommandFinished(BlockId, i32),
}

/// A command execution block.
///
/// See `docs/specs/terminal-core.md §6.2`.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: BlockId,
    pub prompt_start: AbsLine,
    pub input_start: Option<AbsLine>,
    pub output_start: Option<AbsLine>,
    pub end: Option<AbsLine>,
    pub command: Option<Box<str>>,
    pub exit: Option<i32>,
    pub started_at: Option<Instant>,
    pub finished_at: Option<Instant>,
    pub cwd: Option<Box<str>>,
    pub flags: BlockFlags,
}

/// List of command execution blocks.
///
/// See `docs/specs/terminal-core.md §6.2`.
#[derive(Clone, Debug)]
pub struct BlockList {
    blocks: VecDeque<Block>,
    pub active: Option<BlockId>,
    next_id: u32,
    last_cwd: Option<Box<str>>,
}

impl Default for BlockList {
    fn default() -> Self {
        Self::new()
    }
}

impl BlockList {
    pub const MAX_BLOCKS: usize = 10_000;

    pub fn new() -> Self {
        Self {
            blocks: VecDeque::new(),
            active: None,
            next_id: 1,
            last_cwd: None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Block> {
        self.blocks.iter()
    }

    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    pub fn get(&self, id: BlockId) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    /// Record CWD update from OSC 7 / OSC 1337.
    pub fn set_cwd(&mut self, cwd: &str) {
        self.last_cwd = Some(cwd.into());
    }

    /// 133;A: Prompt start.
    pub fn on_prompt_start(&mut self, current_line: AbsLine) -> BlockEvent {
        // If a block is currently active without finish (D), mark it interrupted
        if let Some(active_id) = self.active.take() {
            if let Some(b) = self.get_mut(active_id) {
                b.end = Some(current_line);
                b.flags.insert(BlockFlags::INTERRUPTED);
            }
        }

        let id = BlockId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        if self.next_id == 0 {
            self.next_id = 1;
        }

        let block = Block {
            id,
            prompt_start: current_line,
            input_start: None,
            output_start: None,
            end: None,
            command: None,
            exit: None,
            started_at: None,
            finished_at: None,
            cwd: self.last_cwd.clone(),
            flags: BlockFlags::empty(),
        };

        if self.blocks.len() >= Self::MAX_BLOCKS {
            self.blocks.pop_front();
        }

        self.blocks.push_back(block);
        self.active = Some(id);
        BlockEvent::PromptStarted(id)
    }

    /// 133;B: Command input start.
    pub fn on_input_start(&mut self, current_line: AbsLine) {
        if let Some(id) = self.active {
            if let Some(b) = self.get_mut(id) {
                b.input_start = Some(current_line);
            }
        }
    }

    /// 133;C: Command output start.
    pub fn on_output_start(
        &mut self,
        current_line: AbsLine,
        cmdline_url: Option<&str>,
    ) -> Option<BlockEvent> {
        if let Some(id) = self.active {
            if let Some(b) = self.get_mut(id) {
                b.output_start = Some(current_line);
                b.started_at = Some(Instant::now());
                if let Some(cmd) = cmdline_url {
                    b.command = Some(cmd.into());
                }
                return Some(BlockEvent::CommandStarted(id));
            }
        }
        None
    }

    /// 133;D: Command finished.
    pub fn on_command_finished(
        &mut self,
        current_line: AbsLine,
        exit_code: i32,
    ) -> Option<BlockEvent> {
        if let Some(id) = self.active.take() {
            if let Some(b) = self.get_mut(id) {
                b.end = Some(current_line);
                b.exit = Some(exit_code);
                b.finished_at = Some(Instant::now());
                return Some(BlockEvent::CommandFinished(id, exit_code));
            }
        }
        None
    }

    /// Mark all completed blocks up to `cursor_line` as CLEARED.
    pub fn clear_up_to(&mut self, cursor_line: AbsLine) {
        for b in &mut self.blocks {
            if let Some(end) = b.end {
                if end <= cursor_line {
                    b.flags.insert(BlockFlags::CLEARED);
                }
            }
        }
    }

    /// Handle line eviction in scrollback.
    pub fn on_lines_evicted(&mut self, oldest_retained: AbsLine) {
        self.blocks.retain_mut(|b| {
            if let Some(end) = b.end {
                if end <= oldest_retained {
                    return false; // fully evicted
                }
            }
            if b.prompt_start < oldest_retained {
                b.flags.insert(BlockFlags::TRUNCATED);
            }
            true
        });
    }

    /// Remap block line indices during reflow.
    pub fn remap(&mut self, remap_fn: impl Fn(AbsLine) -> AbsLine) {
        for b in &mut self.blocks {
            b.prompt_start = remap_fn(b.prompt_start);
            if let Some(input) = b.input_start {
                b.input_start = Some(remap_fn(input));
            }
            if let Some(output) = b.output_start {
                b.output_start = Some(remap_fn(output));
            }
            if let Some(end) = b.end {
                b.end = Some(remap_fn(end));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_lifecycle() {
        let mut list = BlockList::new();
        list.set_cwd("/test/dir");

        let ev1 = list.on_prompt_start(AbsLine(10));
        let id = match ev1 {
            BlockEvent::PromptStarted(id) => id,
            _ => panic!("unexpected event"),
        };

        list.on_input_start(AbsLine(11));
        let ev2 = list.on_output_start(AbsLine(12), Some("cargo build"));
        assert_eq!(ev2, Some(BlockEvent::CommandStarted(id)));

        let ev3 = list.on_command_finished(AbsLine(15), 0);
        assert_eq!(ev3, Some(BlockEvent::CommandFinished(id, 0)));

        let block = list.get(id).unwrap();
        assert_eq!(block.prompt_start, AbsLine(10));
        assert_eq!(block.input_start, Some(AbsLine(11)));
        assert_eq!(block.output_start, Some(AbsLine(12)));
        assert_eq!(block.end, Some(AbsLine(15)));
        assert_eq!(block.exit, Some(0));
        assert_eq!(block.command.as_deref(), Some("cargo build"));
        assert_eq!(block.cwd.as_deref(), Some("/test/dir"));
    }
}
