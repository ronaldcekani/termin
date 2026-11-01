//! Grid view and row iteration.
//!
//! See `docs/architecture.md §3.2` and `docs/specs/terminal-core.md §1`.

use crate::row::{Row, RowFlags, RowView};

/// Scroll position for viewing terminal contents.
///
/// See `docs/specs/terminal-core.md §3`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ScrollPos {
    Bottom,
    Lines(u64),
}

/// Iterator over visible rows for a given scroll position.
///
/// In the common steady state (`ScrollPos::Bottom`), zero heap allocations are made.
pub enum GridView<'a> {
    Screen { rows: &'a [Row], idx: usize },
    Scrolled { views: Vec<RowView<'a>>, idx: usize },
}

impl<'a> Iterator for GridView<'a> {
    type Item = RowView<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            GridView::Screen { rows, idx } => {
                if *idx < rows.len() {
                    let r = &rows[*idx];
                    *idx += 1;
                    Some(RowView {
                        cells: &r.cells,
                        wrapped: r.flags.contains(RowFlags::WRAPPED),
                    })
                } else {
                    None
                }
            }
            GridView::Scrolled { views, idx } => {
                if *idx < views.len() {
                    let v = views[*idx];
                    *idx += 1;
                    Some(v)
                } else {
                    None
                }
            }
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = match self {
            GridView::Screen { rows, idx } => rows.len().saturating_sub(*idx),
            GridView::Scrolled { views, idx } => views.len().saturating_sub(*idx),
        };
        (remaining, Some(remaining))
    }
}

impl<'a> ExactSizeIterator for GridView<'a> {}
