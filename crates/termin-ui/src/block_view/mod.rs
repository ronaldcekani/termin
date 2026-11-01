//! Block mode visual line mapping and drawing primitives.
//!
//! The map is intentionally independent of app state so callers can cache it
//! per tab and rebuild it only when the VT block list or viewport changes.

pub mod render;
pub use render::BlockView;

use termin_core::{BlockId, Rect};
use termin_vt::{AbsLine, Block, BlockFlags, BlockList};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Header,
    Command,
    Rows { first: AbsLine, end: AbsLine },
    Gap,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub block: Option<BlockId>,
    pub kind: SegmentKind,
    pub y: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct VisualLineMap {
    pub segments: Vec<Segment>,
    pub height: f32,
    pub line_height: f32,
}

impl VisualLineMap {
    /// Build chrome and visible output rows. Prompt/input rows are represented
    /// by chrome and therefore omitted from the terminal row segments.
    pub fn build(
        blocks: &BlockList,
        line_height: f32,
        header_height: f32,
        command_height: f32,
        top: AbsLine,
        bottom: AbsLine,
    ) -> Self {
        let mut map = Self::default();
        map.rebuild(
            blocks,
            line_height,
            header_height,
            command_height,
            top,
            bottom,
        );
        map
    }

    /// Rebuild in place, reusing the segment buffer (no allocation in steady state).
    pub fn rebuild(
        &mut self,
        blocks: &BlockList,
        line_height: f32,
        header_height: f32,
        command_height: f32,
        top: AbsLine,
        bottom: AbsLine,
    ) {
        self.segments.clear();
        self.height = 0.0;
        self.line_height = line_height.max(1.0);
        let map = self;
        let mut previous_end: Option<AbsLine> = None;
        // Blocks that never reached OSC 133;C (the live prompt, empty Enter) have
        // no command to show; the input editor stands in for them.
        for block in blocks
            .iter()
            .filter(|b| !b.flags.contains(BlockFlags::CLEARED) && b.output_start.is_some())
        {
            let end = block.end.unwrap_or(bottom);
            if let Some(prev) = previous_end {
                if block.prompt_start.0 > prev.0.saturating_add(1) {
                    let gap_lines = block.prompt_start.0 - prev.0 - 1;
                    map.push(None, SegmentKind::Gap, gap_lines as f32 * map.line_height);
                }
            }
            map.push(Some(block.id), SegmentKind::Header, header_height.max(0.0));
            if block.command.is_some() {
                map.push(
                    Some(block.id),
                    SegmentKind::Command,
                    command_height.max(0.0),
                );
            }
            let row_start = block.output_start.unwrap_or(end);
            let first = AbsLine(row_start.0.max(top.0));
            let last = AbsLine(end.0.min(bottom.0));
            if last.0 > first.0 {
                map.push(
                    Some(block.id),
                    SegmentKind::Rows { first, end: last },
                    (last.0 - first.0) as f32 * map.line_height,
                );
            }
            previous_end = Some(end);
        }
    }

    fn push(&mut self, block: Option<BlockId>, kind: SegmentKind, h: f32) {
        if h <= 0.0 {
            return;
        }
        self.segments.push(Segment {
            block,
            kind,
            y: self.height,
            h,
        });
        self.height += h;
    }

    pub fn segment_at(&self, y: f32) -> Option<&Segment> {
        self.segments.iter().find(|s| y >= s.y && y < s.y + s.h)
    }

    /// Return the active block header to pin when its body remains visible.
    pub fn sticky_header(&self, scroll_y: f32, viewport: Rect) -> Option<(BlockId, f32)> {
        let header = self
            .segments
            .iter()
            .rev()
            .find(|s| s.kind == SegmentKind::Header && s.y <= scroll_y)?;
        let id = header.block?;
        let body_visible = self.segments.iter().any(|s| {
            s.block == Some(id)
                && matches!(s.kind, SegmentKind::Rows { .. })
                && s.y + s.h > scroll_y + header.h
                && s.y < scroll_y + viewport.h
        });
        body_visible.then_some((id, viewport.y))
    }
}

/// Visible absolute output range represented by a block.
pub fn output_range(block: &Block, viewport_bottom: AbsLine) -> Option<(AbsLine, AbsLine)> {
    let start = block.output_start?;
    let end = block.end.unwrap_or(viewport_bottom);
    (end.0 > start.0).then_some((start, end))
}

/// Result of filtering a single block's output lines (`components.md §4`, `§15`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BlockOutputFilterResult {
    /// Active case-insensitive filter query.
    pub query: String,
    /// Matching absolute line numbers within the block's output range, paired with
    /// `(byte_start, byte_end)` highlight ranges within each line's text.
    pub matching_lines: Vec<(AbsLine, Vec<(usize, usize)>)>,
    /// Total output lines examined in the block.
    pub total_lines: usize,
}

impl BlockOutputFilterResult {
    /// Filter `lines` (paired with their `AbsLine` index) by case-insensitive substring `query`.
    pub fn filter_lines<'a>(
        query: &str,
        lines: impl IntoIterator<Item = (AbsLine, &'a str)>,
    ) -> Self {
        let q_trim = query.trim();
        let q_lower = q_trim.to_ascii_lowercase();
        let mut matching_lines = Vec::new();
        let mut total_lines = 0;

        for (abs_line, text) in lines {
            total_lines += 1;
            if q_lower.is_empty() {
                matching_lines.push((abs_line, Vec::new()));
                continue;
            }
            let lower = text.to_ascii_lowercase();
            let mut spans = Vec::new();
            let mut search_from = 0;
            while let Some(pos) = lower[search_from..].find(&q_lower) {
                let start = search_from + pos;
                let end = start + q_lower.len();
                spans.push((start, end));
                search_from = end;
            }
            if !spans.is_empty() {
                matching_lines.push((abs_line, spans));
            }
        }

        Self {
            query: q_trim.to_string(),
            matching_lines,
            total_lines,
        }
    }

    /// Number of matching output lines.
    pub fn match_count(&self) -> usize {
        self.matching_lines.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termin_core::BlockId;
    use termin_vt::BlockList;

    #[test]
    fn maps_output_and_omits_cleared_blocks() {
        let mut blocks = BlockList::new();
        let id = match blocks.on_prompt_start(AbsLine(2)) {
            termin_vt::BlockEvent::PromptStarted(id) => id,
            _ => unreachable!(),
        };
        blocks.on_input_start(AbsLine(3));
        blocks.on_output_start(AbsLine(4), Some("echo hi"));
        blocks.on_command_finished(AbsLine(7), 0);
        let map = VisualLineMap::build(&blocks, 20.0, 18.0, 20.0, AbsLine(0), AbsLine(10));
        assert_eq!(
            map.segments.iter().filter(|s| s.block == Some(id)).count(),
            3
        );
        assert_eq!(
            map.segments[2].kind,
            SegmentKind::Rows {
                first: AbsLine(4),
                end: AbsLine(7)
            }
        );
        assert_eq!(map.segment_at(40.0).and_then(|s| s.block), Some(id));
        assert_eq!(BlockId(id.0), id);
    }

    #[test]
    fn omits_blocks_without_a_command() {
        let mut blocks = BlockList::new();
        blocks.on_prompt_start(AbsLine(0));
        blocks.on_input_start(AbsLine(0));
        // Empty Enter: next prompt starts without OSC 133;C.
        blocks.on_prompt_start(AbsLine(1));
        blocks.on_input_start(AbsLine(1));
        let map = VisualLineMap::build(&blocks, 20.0, 18.0, 20.0, AbsLine(0), AbsLine(2));
        assert!(map.segments.is_empty());
        assert_eq!(map.height, 0.0);
    }

    #[test]
    fn filters_block_output_lines_case_insensitively() {
        let lines = vec![
            (AbsLine(10), "Compiling termin-vt v0.1.0"),
            (AbsLine(11), "error[E0308]: mismatched types"),
            (AbsLine(12), "warning: unused variable"),
            (AbsLine(13), "ERROR: build failed"),
        ];
        let res = BlockOutputFilterResult::filter_lines("error", lines);
        assert_eq!(res.total_lines, 4);
        assert_eq!(res.match_count(), 2);
        assert_eq!(res.matching_lines[0].0, AbsLine(11));
        assert_eq!(res.matching_lines[0].1, vec![(0, 5)]);
        assert_eq!(res.matching_lines[1].0, AbsLine(13));
        assert_eq!(res.matching_lines[1].1, vec![(0, 5)]);
    }
}
