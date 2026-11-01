//! Split tree, layout computation and geometric focus navigation.
//!
//! See `docs/specs/app-model-and-ui.md §2`.

use termin_core::{PaneId, Point, Rect, Size};

/// Visual thickness of a divider in points.
pub const DIVIDER_THICKNESS: f32 = 1.0;
/// Hit-test thickness of a divider in points.
pub const DIVIDER_HIT: f32 = 6.0;
/// Smallest/largest split ratio.
pub const MIN_RATIO: f32 = 0.1;
/// Largest split ratio.
pub const MAX_RATIO: f32 = 0.9;

/// Split orientation. `Right` = children side by side (horizontal row),
/// `Down` = children stacked (vertical column).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    /// Side by side.
    Right,
    /// Stacked.
    Down,
}

/// Cardinal direction for focus/resize/swap.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// ←
    Left,
    /// →
    Right,
    /// ↑
    Up,
    /// ↓
    Down,
}

impl Direction {
    /// Lowercase name (`left|right|up|down`).
    pub fn name(self) -> &'static str {
        match self {
            Direction::Left => "left",
            Direction::Right => "right",
            Direction::Up => "up",
            Direction::Down => "down",
        }
    }

    /// Parse a lowercase name.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "left" => Direction::Left,
            "right" => Direction::Right,
            "up" => Direction::Up,
            "down" => Direction::Down,
            _ => return None,
        })
    }

    /// Split axis this direction moves along.
    pub fn axis(self) -> Dir {
        match self {
            Direction::Left | Direction::Right => Dir::Right,
            Direction::Up | Direction::Down => Dir::Down,
        }
    }
}

/// Index into the node arena.
pub type NodeIdx = usize;

/// Tree node.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Node {
    /// A pane.
    Leaf(PaneId),
    /// Inner split; `ratio` is the fraction given to `a`.
    Split {
        /// Orientation.
        dir: Dir,
        /// Fraction (0.1..0.9) of the available space for the first child.
        ratio: f32,
        /// First child (left/top).
        a: NodeIdx,
        /// Second child (right/bottom).
        b: NodeIdx,
    },
}

/// A draggable divider.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Divider {
    /// Split node this divider belongs to (valid until the tree mutates).
    pub node: NodeIdx,
    /// Orientation of the split (`Right` → vertical line).
    pub dir: Dir,
    /// 1 pt visual line.
    pub line: Rect,
    /// 6 pt hit area.
    pub hit: Rect,
    /// Rect of the whole split (for converting a drag position to a ratio).
    pub span: Rect,
}

impl Divider {
    /// Ratio that would put this divider under `p` (clamped).
    pub fn ratio_at(&self, p: Point) -> f32 {
        let (pos, start, len) = match self.dir {
            Dir::Right => (p.x, self.span.x, self.span.w),
            Dir::Down => (p.y, self.span.y, self.span.h),
        };
        let avail = (len - DIVIDER_THICKNESS).max(1.0);
        ((pos - start) / avail).clamp(MIN_RATIO, MAX_RATIO)
    }
}

/// Result of laying out a tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TreeLayout {
    /// Pane rectangles in leaf (reading) order.
    pub panes: Vec<(PaneId, Rect)>,
    /// Dividers in tree pre-order.
    pub dividers: Vec<Divider>,
}

/// Arena-backed binary split tree. Leaves are panes.
#[derive(Clone, Debug)]
pub struct SplitTree {
    nodes: Vec<Node>,
    root: NodeIdx,
    rev: u32,
}

impl SplitTree {
    /// Tree with a single pane.
    pub fn new(pane: PaneId) -> Self {
        Self {
            nodes: vec![Node::Leaf(pane)],
            root: 0,
            rev: 0,
        }
    }

    /// Mutation counter; bumps on every structural/ratio change.
    pub fn rev(&self) -> u32 {
        self.rev
    }

    /// Root node index.
    pub fn root(&self) -> NodeIdx {
        self.root
    }

    /// Node at `idx`.
    pub fn node(&self, idx: NodeIdx) -> Option<&Node> {
        self.nodes.get(idx)
    }

    /// Number of live leaves.
    pub fn len(&self) -> usize {
        self.leaves().len()
    }

    /// Never empty (kept for clippy symmetry).
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Leaves in reading order (a before b).
    pub fn leaves(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect_leaves(self.root, &mut out);
        out
    }

    fn collect_leaves(&self, idx: NodeIdx, out: &mut Vec<PaneId>) {
        match self.nodes[idx] {
            Node::Leaf(p) => out.push(p),
            Node::Split { a, b, .. } => {
                self.collect_leaves(a, out);
                self.collect_leaves(b, out);
            }
        }
    }

    /// First leaf (top-left) of the subtree at `idx`.
    fn first_leaf(&self, mut idx: NodeIdx) -> PaneId {
        loop {
            match self.nodes[idx] {
                Node::Leaf(p) => return p,
                Node::Split { a, .. } => idx = a,
            }
        }
    }

    /// Whether `pane` is a leaf.
    pub fn contains(&self, pane: PaneId) -> bool {
        self.path_to(pane).is_some()
    }

    /// Node indices from the root to the leaf holding `pane`.
    fn path_to(&self, pane: PaneId) -> Option<Vec<NodeIdx>> {
        fn go(t: &SplitTree, idx: NodeIdx, pane: PaneId, path: &mut Vec<NodeIdx>) -> bool {
            path.push(idx);
            match t.nodes[idx] {
                Node::Leaf(p) => {
                    if p == pane {
                        return true;
                    }
                }
                Node::Split { a, b, .. } => {
                    if go(t, a, pane, path) || go(t, b, pane, path) {
                        return true;
                    }
                }
            }
            path.pop();
            false
        }
        let mut path = Vec::new();
        go(self, self.root, pane, &mut path).then_some(path)
    }

    /// Replace leaf `target` with `Split{dir, 0.5, target, new}`.
    pub fn split(&mut self, target: PaneId, dir: Dir, new: PaneId) -> bool {
        let Some(path) = self.path_to(target) else {
            return false;
        };
        let idx = *path.last().expect("path non-empty");
        let a = self.nodes.len();
        self.nodes.push(Node::Leaf(target));
        self.nodes.push(Node::Leaf(new));
        self.nodes[idx] = Node::Split {
            dir,
            ratio: 0.5,
            a,
            b: a + 1,
        };
        self.rev += 1;
        true
    }

    /// Remove a leaf; its sibling takes the parent's place. Returns the
    /// sibling subtree's first leaf (the natural focus target), or `None` if
    /// `pane` is absent or is the only leaf.
    pub fn remove(&mut self, pane: PaneId) -> Option<PaneId> {
        let path = self.path_to(pane)?;
        if path.len() < 2 {
            return None;
        }
        let leaf = path[path.len() - 1];
        let parent = path[path.len() - 2];
        let Node::Split { a, b, .. } = self.nodes[parent] else {
            return None;
        };
        let sibling = if a == leaf { b } else { a };
        let focus = self.first_leaf(sibling);
        self.nodes[parent] = self.nodes[sibling];
        self.compact();
        self.rev += 1;
        Some(focus)
    }

    /// Drop unreachable nodes, renumbering in pre-order.
    fn compact(&mut self) {
        fn copy(old: &[Node], idx: NodeIdx, out: &mut Vec<Node>) -> NodeIdx {
            let new_idx = out.len();
            match old[idx] {
                Node::Leaf(p) => out.push(Node::Leaf(p)),
                Node::Split { dir, ratio, a, b } => {
                    out.push(Node::Leaf(PaneId(0)));
                    let na = copy(old, a, out);
                    let nb = copy(old, b, out);
                    out[new_idx] = Node::Split {
                        dir,
                        ratio,
                        a: na,
                        b: nb,
                    };
                }
            }
            new_idx
        }
        let mut out = Vec::with_capacity(self.nodes.len());
        self.root = copy(&self.nodes, self.root, &mut out);
        self.nodes = out;
    }

    /// Set a split's ratio (clamped). Returns `false` if `node` isn't a split.
    pub fn set_ratio(&mut self, node: NodeIdx, ratio: f32) -> bool {
        if let Some(Node::Split { ratio: r, .. }) = self.nodes.get_mut(node) {
            let v = if ratio.is_finite() { ratio } else { 0.5 };
            *r = v.clamp(MIN_RATIO, MAX_RATIO);
            self.rev += 1;
            true
        } else {
            false
        }
    }

    /// Ratio of a split node.
    pub fn ratio(&self, node: NodeIdx) -> Option<f32> {
        match self.nodes.get(node) {
            Some(Node::Split { ratio, .. }) => Some(*ratio),
            _ => None,
        }
    }

    /// Move the nearest ancestor divider of `pane` on `direction`'s axis by
    /// `delta` (a ratio fraction). `Right`/`Down` increase the ratio,
    /// `Left`/`Up` decrease it. Returns whether anything changed.
    pub fn resize(&mut self, pane: PaneId, direction: Direction, delta: f32) -> bool {
        let Some(path) = self.path_to(pane) else {
            return false;
        };
        let axis = direction.axis();
        for &idx in path.iter().rev().skip(1) {
            if let Node::Split { dir, ratio, .. } = self.nodes[idx] {
                if dir == axis {
                    let sign = match direction {
                        Direction::Right | Direction::Down => 1.0,
                        _ => -1.0,
                    };
                    let new = (ratio + sign * delta).clamp(MIN_RATIO, MAX_RATIO);
                    let changed = (new - ratio).abs() > f32::EPSILON;
                    if changed {
                        self.set_ratio(idx, new);
                    }
                    return changed;
                }
            }
        }
        false
    }

    /// Give every leaf equal space along each axis.
    pub fn equalize(&mut self) {
        fn count(nodes: &[Node], idx: NodeIdx, along: Dir) -> usize {
            match nodes[idx] {
                Node::Leaf(_) => 1,
                Node::Split { dir, a, b, .. } => {
                    let (ca, cb) = (count(nodes, a, along), count(nodes, b, along));
                    if dir == along {
                        ca + cb
                    } else {
                        ca.max(cb)
                    }
                }
            }
        }
        for i in 0..self.nodes.len() {
            if let Node::Split { dir, a, b, .. } = self.nodes[i] {
                let ca = count(&self.nodes, a, dir) as f32;
                let cb = count(&self.nodes, b, dir) as f32;
                let r = (ca / (ca + cb)).clamp(MIN_RATIO, MAX_RATIO);
                if let Node::Split { ratio, .. } = &mut self.nodes[i] {
                    *ratio = r;
                }
            }
        }
        self.rev += 1;
    }

    /// Exchange the positions of two leaves.
    pub fn swap_leaves(&mut self, x: PaneId, y: PaneId) -> bool {
        let (mut ix, mut iy) = (None, None);
        for (i, n) in self.nodes.iter().enumerate() {
            // Only reachable nodes matter; orphans are compacted away.
            if let Node::Leaf(p) = n {
                if *p == x {
                    ix = Some(i);
                } else if *p == y {
                    iy = Some(i);
                }
            }
        }
        match (ix, iy) {
            (Some(i), Some(j)) if i != j => {
                self.nodes[i] = Node::Leaf(y);
                self.nodes[j] = Node::Leaf(x);
                self.rev += 1;
                true
            }
            _ => false,
        }
    }

    /// Path of `0`(a)/`1`(b) choices from the root to `pane` (persistence).
    pub fn tree_path(&self, pane: PaneId) -> Option<Vec<usize>> {
        let path = self.path_to(pane)?;
        let mut out = Vec::with_capacity(path.len());
        for w in path.windows(2) {
            if let Node::Split { a, .. } = self.nodes[w[0]] {
                out.push(usize::from(w[1] != a));
            }
        }
        Some(out)
    }

    /// Resolve a path produced by [`SplitTree::tree_path`]; invalid steps
    /// stop descending (the first leaf of the reached subtree is returned).
    pub fn pane_at_path(&self, path: &[usize]) -> PaneId {
        let mut idx = self.root;
        for &step in path {
            match self.nodes[idx] {
                Node::Split { a, b, .. } => {
                    if step == 0 {
                        idx = a;
                    } else if step == 1 {
                        idx = b;
                    } else {
                        break;
                    }
                }
                Node::Leaf(_) => break,
            }
        }
        self.first_leaf(idx)
    }

    /// Compute pane rects and dividers inside `area`. `min` is the minimum
    /// pane size in points; ratios are clamped so panes keep it when space
    /// allows.
    pub fn layout(&self, area: Rect, min: Size) -> TreeLayout {
        let mut out = TreeLayout::default();
        self.layout_into(area, min, &mut out);
        out
    }

    /// Like [`SplitTree::layout`] but reuses `out`'s allocations.
    pub fn layout_into(&self, area: Rect, min: Size, out: &mut TreeLayout) {
        out.panes.clear();
        out.dividers.clear();
        self.lay(self.root, area, min, out);
    }

    fn lay(&self, idx: NodeIdx, rect: Rect, min: Size, out: &mut TreeLayout) {
        match self.nodes[idx] {
            Node::Leaf(p) => out.panes.push((p, rect)),
            Node::Split { dir, ratio, a, b } => {
                let (len, min_len) = match dir {
                    Dir::Right => (rect.w, min.w),
                    Dir::Down => (rect.h, min.h),
                };
                let avail = (len - DIVIDER_THICKNESS).max(0.0);
                let mut first = avail * ratio;
                if avail >= 2.0 * min_len {
                    first = first.clamp(min_len, avail - min_len);
                }
                let first = first.round().clamp(0.0, avail);
                let second = avail - first;
                let (ra, line, rb) = match dir {
                    Dir::Right => (
                        Rect::new(rect.x, rect.y, first, rect.h),
                        Rect::new(rect.x + first, rect.y, DIVIDER_THICKNESS, rect.h),
                        Rect::new(rect.x + first + DIVIDER_THICKNESS, rect.y, second, rect.h),
                    ),
                    Dir::Down => (
                        Rect::new(rect.x, rect.y, rect.w, first),
                        Rect::new(rect.x, rect.y + first, rect.w, DIVIDER_THICKNESS),
                        Rect::new(rect.x, rect.y + first + DIVIDER_THICKNESS, rect.w, second),
                    ),
                };
                let grow = (DIVIDER_HIT - DIVIDER_THICKNESS) / 2.0;
                let hit = match dir {
                    Dir::Right => line.inset(-grow, 0.0),
                    Dir::Down => line.inset(0.0, -grow),
                };
                out.dividers.push(Divider {
                    node: idx,
                    dir,
                    line,
                    hit,
                    span: rect,
                });
                self.lay(a, ra, min, out);
                self.lay(b, rb, min, out);
            }
        }
    }
}

/// Geometric focus navigation: the pane adjacent to `from` in `dir` with the
/// greatest perpendicular overlap; ties go to the most recently focused pane
/// (`mru`, front = most recent).
pub fn neighbor(
    panes: &[(PaneId, Rect)],
    from: PaneId,
    dir: Direction,
    mru: &[PaneId],
) -> Option<PaneId> {
    const EPS: f32 = 0.01;
    let src = panes.iter().find(|(p, _)| *p == from)?.1;
    // (pane, gap, overlap)
    let mut cands: Vec<(PaneId, f32, f32)> = Vec::new();
    for &(p, r) in panes {
        if p == from {
            continue;
        }
        let (gap, overlap) = match dir {
            Direction::Left => (
                src.left() - r.right(),
                overlap_1d(src.top(), src.bottom(), r.top(), r.bottom()),
            ),
            Direction::Right => (
                r.left() - src.right(),
                overlap_1d(src.top(), src.bottom(), r.top(), r.bottom()),
            ),
            Direction::Up => (
                src.top() - r.bottom(),
                overlap_1d(src.left(), src.right(), r.left(), r.right()),
            ),
            Direction::Down => (
                r.top() - src.bottom(),
                overlap_1d(src.left(), src.right(), r.left(), r.right()),
            ),
        };
        if gap >= -EPS && overlap > EPS {
            cands.push((p, gap, overlap));
        }
    }
    let min_gap = cands.iter().map(|c| c.1).fold(f32::INFINITY, f32::min);
    let rank = |p: PaneId| mru.iter().position(|m| *m == p).unwrap_or(usize::MAX);
    cands
        .into_iter()
        .filter(|c| c.1 <= min_gap + DIVIDER_THICKNESS + EPS)
        .max_by(|x, y| {
            x.2.partial_cmp(&y.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| rank(y.0).cmp(&rank(x.0)))
                .then_with(|| y.0.cmp(&x.0))
        })
        .map(|c| c.0)
}

fn overlap_1d(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Size = Size::new(10.0, 10.0);

    fn p(n: u32) -> PaneId {
        PaneId(n)
    }

    fn area() -> Rect {
        Rect::new(0.0, 0.0, 101.0, 101.0)
    }

    #[test]
    fn single_leaf_fills_area() {
        let t = SplitTree::new(p(1));
        let l = t.layout(area(), MIN);
        assert_eq!(l.panes, vec![(p(1), area())]);
        assert!(l.dividers.is_empty());
    }

    #[test]
    fn split_right_layout_snapshot() {
        let mut t = SplitTree::new(p(1));
        assert!(t.split(p(1), Dir::Right, p(2)));
        let l = t.layout(area(), MIN);
        // avail = 100, first = 50, divider at x=50, second starts at 51
        assert_eq!(l.panes[0], (p(1), Rect::new(0.0, 0.0, 50.0, 101.0)));
        assert_eq!(l.panes[1], (p(2), Rect::new(51.0, 0.0, 50.0, 101.0)));
        assert_eq!(l.dividers.len(), 1);
        assert_eq!(l.dividers[0].line, Rect::new(50.0, 0.0, 1.0, 101.0));
        assert_eq!(l.dividers[0].hit, Rect::new(47.5, 0.0, 6.0, 101.0));
    }

    #[test]
    fn nested_layout_snapshot() {
        // [1 | (2 / 3)]
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(2), Dir::Down, p(3));
        let l = t.layout(area(), MIN);
        assert_eq!(l.panes[0].1, Rect::new(0.0, 0.0, 50.0, 101.0));
        assert_eq!(l.panes[1].1, Rect::new(51.0, 0.0, 50.0, 50.0));
        assert_eq!(l.panes[2].1, Rect::new(51.0, 51.0, 50.0, 50.0));
        assert_eq!(l.dividers.len(), 2);
        assert_eq!(l.dividers[1].dir, Dir::Down);
    }

    #[test]
    fn panes_tile_without_overlap() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(1), Dir::Down, p(3));
        t.split(p(2), Dir::Down, p(4));
        let l = t.layout(Rect::new(0.0, 0.0, 333.0, 222.0), MIN);
        for (i, (_, a)) in l.panes.iter().enumerate() {
            for (_, b) in l.panes.iter().skip(i + 1) {
                assert!(a
                    .intersection(*b)
                    .map_or(true, |r| r.w <= 0.0 || r.h <= 0.0));
            }
        }
    }

    #[test]
    fn ratio_respects_min_size() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        let root = t.root();
        t.set_ratio(root, 0.1);
        let l = t.layout(Rect::new(0.0, 0.0, 101.0, 50.0), Size::new(30.0, 10.0));
        assert_eq!(l.panes[0].1.w, 30.0);
        t.set_ratio(root, 5.0);
        assert_eq!(t.ratio(root), Some(MAX_RATIO));
    }

    #[test]
    fn remove_promotes_sibling() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(2), Dir::Down, p(3));
        assert_eq!(t.remove(p(1)), Some(p(2)));
        assert_eq!(t.leaves(), vec![p(2), p(3)]);
        assert_eq!(t.remove(p(3)), Some(p(2)));
        assert_eq!(t.leaves(), vec![p(2)]);
        assert_eq!(t.remove(p(2)), None, "last leaf stays");
    }

    #[test]
    fn resize_moves_nearest_axis_ancestor() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(2), Dir::Down, p(3));
        // pane 3 has a Down parent and a Right grandparent
        assert!(t.resize(p(3), Direction::Right, 0.05));
        assert!((t.ratio(t.root()).unwrap() - 0.55).abs() < 1e-5);
        assert!(t.resize(p(3), Direction::Up, 0.05));
        let down = t.layout(area(), MIN).dividers[1].node;
        assert!((t.ratio(down).unwrap() - 0.45).abs() < 1e-5);
        assert!(
            !t.resize(p(1), Direction::Down, 0.05),
            "no vertical ancestor"
        );
    }

    #[test]
    fn equalize_three_in_a_row() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(2), Dir::Right, p(3));
        t.equalize();
        let l = t.layout(Rect::new(0.0, 0.0, 302.0, 10.0), Size::new(1.0, 1.0));
        let w: Vec<f32> = l.panes.iter().map(|x| x.1.w).collect();
        assert!(
            (w[0] - w[1]).abs() <= 2.0 && (w[1] - w[2]).abs() <= 2.0,
            "{w:?}"
        );
    }

    #[test]
    fn path_round_trip() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(2), Dir::Down, p(3));
        for leaf in t.leaves() {
            let path = t.tree_path(leaf).unwrap();
            assert_eq!(t.pane_at_path(&path), leaf);
        }
        assert_eq!(t.tree_path(p(3)), Some(vec![1, 1]));
        assert_eq!(
            t.pane_at_path(&[9, 9, 9]),
            p(1),
            "garbage path never panics"
        );
    }

    #[test]
    fn swap_leaves_exchanges_positions() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        assert!(t.swap_leaves(p(1), p(2)));
        assert_eq!(t.leaves(), vec![p(2), p(1)]);
    }

    /// Layout: [1 | (2 / 3)] — focus navigation.
    #[test]
    fn focus_navigation() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(2), Dir::Down, p(3));
        let l = t.layout(area(), MIN).panes;
        assert_eq!(neighbor(&l, p(1), Direction::Right, &[]), Some(p(2)));
        assert_eq!(
            neighbor(&l, p(1), Direction::Right, &[p(3)]),
            Some(p(3)),
            "mru tie-break"
        );
        assert_eq!(neighbor(&l, p(2), Direction::Down, &[]), Some(p(3)));
        assert_eq!(neighbor(&l, p(3), Direction::Up, &[]), Some(p(2)));
        assert_eq!(neighbor(&l, p(3), Direction::Left, &[]), Some(p(1)));
        assert_eq!(neighbor(&l, p(1), Direction::Left, &[]), None);
        assert_eq!(neighbor(&l, p(1), Direction::Up, &[]), None);
        assert_eq!(neighbor(&l, p(3), Direction::Down, &[]), None);
    }

    #[test]
    fn focus_prefers_greater_overlap() {
        // 1 on the left; right column is (2 small on top) / (3 large below)
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        t.split(p(2), Dir::Down, p(3));
        let down = t.layout(area(), MIN).dividers[1].node;
        t.set_ratio(down, 0.2);
        let l = t.layout(area(), MIN).panes;
        assert_eq!(neighbor(&l, p(1), Direction::Right, &[p(2)]), Some(p(3)));
    }

    #[test]
    fn divider_ratio_at() {
        let mut t = SplitTree::new(p(1));
        t.split(p(1), Dir::Right, p(2));
        let d = t.layout(area(), MIN).dividers[0];
        assert!((d.ratio_at(Point::new(50.0, 3.0)) - 0.5).abs() < 0.01);
        assert_eq!(d.ratio_at(Point::new(-40.0, 3.0)), MIN_RATIO);
    }
}
