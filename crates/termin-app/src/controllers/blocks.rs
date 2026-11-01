//! Block view actions controller.

use crate::action::Action;
use crate::model::AppModel;
use rustc_hash::FxHashMap;
use termin_core::BlockId;
use termin_vt::{AbsLine, BlockEvent, BlockList};

/// Stateful UI-side block navigation and pixel scrolling, owned per tab by the
/// app. The VT remains the source of block lifecycle truth.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlockController {
    pub selected: Option<BlockId>,
    pub scroll_y: f32,
    pub viewport_height: f32,
    pub content_height: f32,
    pub new_output: bool,
    /// Per-block output filter queries (`components.md §4, §15`).
    pub filters: FxHashMap<BlockId, String>,
    /// Block currently editing its output filter query.
    pub active_filter_block: Option<BlockId>,
}

impl BlockController {
    /// Keep the selected id valid as bounded VT scrollback evicts old blocks.
    pub fn reconcile(&mut self, blocks: &BlockList) {
        if self.selected.is_some_and(|id| blocks.get(id).is_none()) {
            self.selected = None;
        }
        self.filters.retain(|id, _| blocks.get(*id).is_some());
        if self
            .active_filter_block
            .is_some_and(|id| blocks.get(id).is_none())
        {
            self.active_filter_block = None;
        }
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
    }

    /// Set or clear the output filter query for `block_id` (`components.md §4`).
    pub fn set_block_filter(&mut self, block_id: BlockId, query: impl Into<String>) {
        let q = query.into();
        if q.trim().is_empty() {
            self.filters.remove(&block_id);
        } else {
            self.filters.insert(block_id, q);
        }
    }

    /// Get the active filter query for `block_id`, if any.
    pub fn block_filter(&self, block_id: BlockId) -> Option<&str> {
        self.filters.get(&block_id).map(String::as_str)
    }

    /// Filter output lines of `block_id` against its active query (case-insensitive).
    pub fn filter_block_lines<'a>(
        &self,
        block_id: BlockId,
        lines: impl IntoIterator<Item = (AbsLine, &'a str)>,
    ) -> Vec<AbsLine> {
        let q = self
            .filters
            .get(&block_id)
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default();
        lines
            .into_iter()
            .filter_map(|(line_no, text)| {
                if q.is_empty() || text.to_ascii_lowercase().contains(&q) {
                    Some(line_no)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn max_scroll(&self) -> f32 {
        (self.content_height - self.viewport_height).max(0.0)
    }

    /// Update scroll geometry and follow output unless the user has scrolled up.
    pub fn set_geometry(
        &mut self,
        content_height: f32,
        viewport_height: f32,
        output_changed: bool,
    ) {
        let was_at_bottom = self.scroll_y >= self.max_scroll() - 1.0;
        self.content_height = content_height.max(0.0);
        self.viewport_height = viewport_height.max(0.0);
        if was_at_bottom {
            self.scroll_y = self.max_scroll();
            self.new_output = false;
        } else if output_changed {
            self.new_output = true;
        }
        self.scroll_y = self.scroll_y.clamp(0.0, self.max_scroll());
    }

    pub fn scroll_by(&mut self, delta_y: f32) {
        self.scroll_y = (self.scroll_y + delta_y).clamp(0.0, self.max_scroll());
        if self.scroll_y >= self.max_scroll() {
            self.new_output = false;
        }
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_y = self.max_scroll();
        self.new_output = false;
    }

    pub fn select(&mut self, id: Option<BlockId>, blocks: &BlockList) -> bool {
        let next = id.filter(|id| {
            blocks
                .get(*id)
                .is_some_and(|block| !block.flags.contains(termin_vt::BlockFlags::CLEARED))
        });
        let changed = self.selected != next;
        self.selected = next;
        changed
    }

    pub fn select_relative(&mut self, blocks: &BlockList, delta: isize) -> bool {
        let visible: Vec<_> = blocks
            .iter()
            .filter(|b| !b.flags.contains(termin_vt::BlockFlags::CLEARED))
            .map(|b| b.id)
            .collect();
        if visible.is_empty() {
            return self.select(None, blocks);
        }
        let current = self
            .selected
            .and_then(|id| visible.iter().position(|v| *v == id));
        let index = match current {
            Some(i) => (i as isize + delta).clamp(0, visible.len() as isize - 1) as usize,
            None if delta < 0 => visible.len() - 1,
            None => 0,
        };
        self.select(Some(visible[index]), blocks)
    }

    /// Apply a parser block event. In particular, prompt start selects the new
    /// current prompt and command finish leaves selection stable.
    pub fn on_block_event(&mut self, event: BlockEvent) {
        if let BlockEvent::PromptStarted(id) = event {
            self.selected = Some(id);
        }
    }

    /// Hide completed blocks through the current cursor line (⌘K behavior).
    pub fn clear_finished(&mut self, blocks: &mut BlockList, cursor: AbsLine) {
        blocks.clear_up_to(cursor);
        if self.selected.is_some_and(|id| {
            blocks
                .get(id)
                .is_some_and(|b| b.flags.contains(termin_vt::BlockFlags::CLEARED))
        }) {
            self.selected = None;
        }
    }
}

pub fn handle(_model: &mut AppModel, _action: &Action) -> bool {
    let model = _model;
    match _action {
        Action::ToggleBlockMode => {
            let Some(id) = model.focused_tab() else {
                return false;
            };
            let Some(tab) = model.tabs.get_mut(id) else {
                return false;
            };
            tab.mode = match tab.mode {
                crate::model::TabMode::Block => crate::model::TabMode::Classic,
                crate::model::TabMode::Auto | crate::model::TabMode::Classic => {
                    crate::model::TabMode::Block
                }
            };
            true
        }
        Action::ScrollToPrevBlock
        | Action::ScrollToNextBlock
        | Action::SelectPrevBlock
        | Action::SelectNextBlock
        | Action::RerunBlock
        | Action::CopyBlock
        | Action::CopyBlockOutput
        | Action::CopyBlockCommand => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_streaming_output_until_user_scrolls_up() {
        let mut ctl = BlockController::default();
        ctl.set_geometry(500.0, 100.0, false);
        assert_eq!(ctl.scroll_y, 400.0);
        ctl.scroll_by(-80.0);
        ctl.set_geometry(560.0, 100.0, true);
        assert!(ctl.new_output);
        assert_eq!(ctl.scroll_y, 320.0);
        ctl.scroll_to_bottom();
        assert!(!ctl.new_output);
        assert_eq!(ctl.scroll_y, 460.0);
    }

    #[test]
    fn selection_navigation_skips_cleared_blocks() {
        let mut blocks = BlockList::new();
        let one = match blocks.on_prompt_start(AbsLine(0)) {
            BlockEvent::PromptStarted(id) => id,
            _ => unreachable!(),
        };
        blocks.on_input_start(AbsLine(1));
        blocks.on_output_start(AbsLine(2), Some("one"));
        blocks.on_command_finished(AbsLine(3), 0);
        let two = match blocks.on_prompt_start(AbsLine(4)) {
            BlockEvent::PromptStarted(id) => id,
            _ => unreachable!(),
        };
        let mut ctl = BlockController::default();
        ctl.select(Some(one), &blocks);
        ctl.select_relative(&blocks, 1);
        assert_eq!(ctl.selected, Some(two));
        ctl.clear_finished(&mut blocks, AbsLine(4));
        assert!(blocks
            .get(one)
            .unwrap()
            .flags
            .contains(termin_vt::BlockFlags::CLEARED));
        ctl.select_relative(&blocks, -1);
        assert_eq!(ctl.selected, Some(two));
        assert!(ctl.select(Some(one), &blocks));
        assert_eq!(ctl.selected, None);
    }
}
