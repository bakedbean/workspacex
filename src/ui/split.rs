//! Recursive split-pane tree for the attached view. Inspired by vim's
//! window splits: any leaf (a single workspace's PTY) can be split
//! vertically (`:vsplit`, children side-by-side) or horizontally
//! (`:split`, children stacked) into a parent node.

use crate::data::store::{AgentInstanceId, WorkspaceId};
use ratatui::layout::{Constraint, Direction as LayoutDirection, Layout, Rect};

/// What a single leaf pane points at: a specific agent instance within a
/// workspace. For a single-agent workspace the `instance` is the
/// workspace's primary instance, so behavior is unchanged from when the
/// leaf carried a bare `WorkspaceId`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AttachTarget {
    pub workspace_id: WorkspaceId,
    pub instance: AgentInstanceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SplitDirection {
    /// Children are stacked side-by-side with a vertical divider, like
    /// vim's `:vsplit`. New pane appears to the right of the focused one.
    Vertical,
    /// Children are stacked top-to-bottom with a horizontal divider, like
    /// vim's `:split`. New pane appears below the focused one.
    Horizontal,
}

/// A 1-cell-wide (or 1-cell-tall) strip between two adjacent panes,
/// produced by `SplitTree::layout`. The renderer fills these with a
/// subtle line glyph so the split boundary is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Divider {
    pub rect: Rect,
    /// Direction of the producing split — `Vertical` means children sit
    /// side-by-side so this divider is a vertical line; `Horizontal`
    /// means children are stacked so the divider is a horizontal line.
    pub direction: SplitDirection,
}

/// Result of laying out a `SplitTree`: one entry per leaf plus the
/// divider strips reserved between adjacent siblings at every internal
/// node.
#[derive(Debug, Clone)]
pub struct LayoutResult {
    pub panes: Vec<(AttachTarget, FocusPath, Rect)>,
    pub dividers: Vec<Divider>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SplitTree {
    Leaf(AttachTarget),
    Split {
        direction: SplitDirection,
        children: Vec<SplitTree>,
    },
}

/// Path from the root: a sequence of child indices that identifies a leaf.
/// An empty path means the root itself (which must be a `Leaf`).
pub type FocusPath = Vec<usize>;

/// What `close` produced.
pub enum CloseOutcome {
    /// Tree still has at least one leaf; this is the new focus.
    Focus(FocusPath),
    /// Tree is now empty — caller should leave the attached view.
    Empty,
}

/// What `prune` produced.
pub enum PruneOutcome {
    /// At least one leaf survived; the tree is still well-formed (no
    /// 1-child Splits).
    Kept,
    /// No leaf survived; caller should treat this tree as gone.
    Empty,
}

#[derive(Debug, Clone)]
pub struct AttachedState {
    pub tree: SplitTree,
    pub focus: FocusPath,
}

impl AttachedState {
    pub fn single(target: AttachTarget) -> Self {
        Self {
            tree: SplitTree::Leaf(target),
            focus: Vec::new(),
        }
    }

    /// Attach target of the focused leaf, if the focus path resolves.
    pub fn focused_target(&self) -> Option<AttachTarget> {
        self.tree.leaf_at(&self.focus)
    }

    /// All attach targets present in the tree (any order).
    pub fn leaves(&self) -> Vec<AttachTarget> {
        self.tree.leaves()
    }

    /// Number of leaves in the tree.
    pub fn leaf_count(&self) -> usize {
        self.tree.leaves().len()
    }

    /// Split the focused leaf in `dir`, inserting `new_id` as a new pane.
    /// New leaf becomes focused. Returns `false` if the focus path was
    /// invalid (which shouldn't happen in normal use).
    pub fn split(&mut self, dir: SplitDirection, new_target: AttachTarget) -> bool {
        match self.tree.split(&self.focus, dir, new_target) {
            Some(new_focus) => {
                self.focus = new_focus;
                true
            }
            None => false,
        }
    }

    /// Close the focused leaf. If the tree becomes empty, returns
    /// `CloseOutcome::Empty` and the caller should switch to the
    /// dashboard.
    pub fn close_focused(&mut self) -> CloseOutcome {
        match self.tree.close(&self.focus) {
            CloseOutcome::Focus(p) => {
                self.focus = p;
                CloseOutcome::Focus(self.focus.clone())
            }
            CloseOutcome::Empty => CloseOutcome::Empty,
        }
    }

    /// Move focus in the given direction. Tree-aware: walks up from the
    /// focused leaf until it finds an ancestor split whose direction matches
    /// the arrow, then moves to the adjacent sibling's first leaf. Returns
    /// `true` if focus moved.
    pub fn focus_direction(&mut self, arrow: Arrow) -> bool {
        match self.tree.focus_direction(&self.focus, arrow) {
            Some(p) => {
                self.focus = p;
                true
            }
            None => false,
        }
    }

    /// Cycle focus to the next leaf in tree-order (depth-first). Wraps.
    /// Useful as a fallback nav when arrows don't move (e.g. simple two-pane
    /// vertical split).
    #[allow(dead_code)]
    pub fn focus_next(&mut self) -> bool {
        let order = self.tree.leaf_paths();
        if order.len() <= 1 {
            return false;
        }
        let cur = order.iter().position(|p| p == &self.focus).unwrap_or(0);
        let next = (cur + 1) % order.len();
        self.focus = order[next].clone();
        true
    }

    /// Lay out the tree within `area`. Returns one entry per leaf with its
    /// focus path and computed rect, plus the divider strips between
    /// adjacent siblings. Leaves are returned in tree order; pane rects
    /// are sized so dividers don't overlap their content.
    pub fn layout(&self, area: Rect) -> LayoutResult {
        self.tree.layout(area)
    }
}

impl SplitTree {
    pub fn leaves(&self) -> Vec<AttachTarget> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    /// Path from the root to the first (leftmost, depth-first) leaf.
    /// For a `Leaf` root this returns an empty path.
    pub fn first_leaf_path(&self) -> FocusPath {
        let mut out = Vec::new();
        let mut node = self;
        loop {
            match node {
                SplitTree::Leaf(_) => return out,
                SplitTree::Split { children, .. } => {
                    if children.is_empty() {
                        return out;
                    }
                    out.push(0);
                    node = &children[0];
                }
            }
        }
    }

    fn collect_leaves(&self, out: &mut Vec<AttachTarget>) {
        match self {
            SplitTree::Leaf(target) => out.push(*target),
            SplitTree::Split { children, .. } => {
                for c in children {
                    c.collect_leaves(out);
                }
            }
        }
    }

    pub fn leaf_paths(&self) -> Vec<FocusPath> {
        let mut out = Vec::new();
        self.collect_leaf_paths(&mut Vec::new(), &mut out);
        out
    }

    fn collect_leaf_paths(&self, path: &mut Vec<usize>, out: &mut Vec<FocusPath>) {
        match self {
            SplitTree::Leaf(_) => out.push(path.clone()),
            SplitTree::Split { children, .. } => {
                for (i, c) in children.iter().enumerate() {
                    path.push(i);
                    c.collect_leaf_paths(path, out);
                    path.pop();
                }
            }
        }
    }

    pub fn leaf_at(&self, path: &[usize]) -> Option<AttachTarget> {
        let node = at(self, path)?;
        match node {
            SplitTree::Leaf(target) => Some(*target),
            SplitTree::Split { .. } => None,
        }
    }

    /// Replace the leaf at `path` with a 2-child split (original leaf,
    /// then `new_id`). If the leaf's parent is already a Split in the same
    /// direction, insert `new_id` as a sibling instead of nesting deeper —
    /// matches vim's behavior and keeps the tree shallow.
    pub fn split(
        &mut self,
        path: &[usize],
        dir: SplitDirection,
        new_target: AttachTarget,
    ) -> Option<FocusPath> {
        // Sibling-insert path: parent is a Split with matching direction.
        if let Some((&last_idx, parent_path)) = path.split_last() {
            let parent_dir = match at(self, parent_path)? {
                SplitTree::Split { direction, .. } => Some(*direction),
                SplitTree::Leaf(_) => None,
            };
            if parent_dir == Some(dir) {
                let parent = at_mut(self, parent_path)?;
                if let SplitTree::Split { children, .. } = parent
                    && last_idx <= children.len()
                {
                    children.insert(last_idx + 1, SplitTree::Leaf(new_target));
                    let mut new_focus = parent_path.to_vec();
                    new_focus.push(last_idx + 1);
                    return Some(new_focus);
                }
            }
        }
        // Nesting path: replace leaf with a new 2-child Split.
        let target = at_mut(self, path)?;
        let orig_target = match *target {
            SplitTree::Leaf(t) => t,
            SplitTree::Split { .. } => return None,
        };
        *target = SplitTree::Split {
            direction: dir,
            children: vec![SplitTree::Leaf(orig_target), SplitTree::Leaf(new_target)],
        };
        let mut new_focus = path.to_vec();
        new_focus.push(1);
        Some(new_focus)
    }

    /// Remove the leaf at `path`. If the parent split had two children,
    /// collapse the parent to its remaining child. If it had more, just
    /// drop the entry. Returns the new focus path, or Empty if the tree
    /// is now gone.
    pub fn close(&mut self, path: &[usize]) -> CloseOutcome {
        let Some((&last_idx, parent_path)) = path.split_last() else {
            // Closing the root leaf: tree is empty.
            return CloseOutcome::Empty;
        };
        let parent = match at_mut(self, parent_path) {
            Some(p) => p,
            None => return CloseOutcome::Empty,
        };
        let SplitTree::Split { children, .. } = parent else {
            return CloseOutcome::Empty;
        };
        if last_idx >= children.len() {
            return CloseOutcome::Empty;
        }
        children.remove(last_idx);
        if children.is_empty() {
            // Shouldn't happen (we always require >= 2 on split), but be
            // defensive.
            return CloseOutcome::Empty;
        }
        if children.len() == 1 {
            // Collapse the now-singleton split into its sole remaining child.
            let only = children.remove(0);
            *parent = only;
            // New focus = the first leaf inside whatever subtree now lives
            // at parent_path.
            CloseOutcome::Focus(first_leaf_path(self, parent_path))
        } else {
            let new_last = last_idx.min(children.len() - 1);
            let mut new_focus = parent_path.to_vec();
            new_focus.push(new_last);
            CloseOutcome::Focus(first_leaf_path(self, &new_focus))
        }
    }

    /// Drop every leaf whose `keep(id)` returns false. After pruning,
    /// any `Split` that ends up with a single child is collapsed into
    /// that child (matches the invariant maintained by `close`).
    pub fn prune<F: Fn(AttachTarget) -> bool>(&mut self, keep: &F) -> PruneOutcome {
        match self {
            SplitTree::Leaf(target) => {
                if keep(*target) {
                    PruneOutcome::Kept
                } else {
                    PruneOutcome::Empty
                }
            }
            SplitTree::Split { children, .. } => {
                let mut i = 0;
                while i < children.len() {
                    match children[i].prune(keep) {
                        PruneOutcome::Kept => i += 1,
                        PruneOutcome::Empty => {
                            children.remove(i);
                        }
                    }
                }
                if children.is_empty() {
                    PruneOutcome::Empty
                } else if children.len() == 1 {
                    let only = children.remove(0);
                    *self = only;
                    PruneOutcome::Kept
                } else {
                    PruneOutcome::Kept
                }
            }
        }
    }

    pub fn layout(&self, area: Rect) -> LayoutResult {
        let mut panes = Vec::new();
        let mut dividers = Vec::new();
        self.layout_inner(area, &mut Vec::new(), &mut panes, &mut dividers);
        LayoutResult { panes, dividers }
    }

    fn layout_inner(
        &self,
        area: Rect,
        path: &mut Vec<usize>,
        out: &mut Vec<(AttachTarget, FocusPath, Rect)>,
        dividers: &mut Vec<Divider>,
    ) {
        match self {
            SplitTree::Leaf(target) => out.push((*target, path.clone(), area)),
            SplitTree::Split {
                direction,
                children,
            } => {
                if children.is_empty() {
                    return;
                }
                let dir = match direction {
                    SplitDirection::Vertical => LayoutDirection::Horizontal,
                    SplitDirection::Horizontal => LayoutDirection::Vertical,
                };
                // Reserve one cell between each pair of children for a
                // divider. If the area is too small to afford the
                // dividers, fall back to the original even split (no
                // dividers) so degenerate tiny rects don't appear.
                let n = children.len() as u16;
                let total = match dir {
                    LayoutDirection::Horizontal => area.width,
                    LayoutDirection::Vertical => area.height,
                };
                let divider_count = n.saturating_sub(1);
                let constraints: Vec<Constraint> = if total > divider_count + n {
                    let content = total - divider_count;
                    let base = content / n;
                    let extra = content % n;
                    let mut cs = Vec::with_capacity((2 * n - 1) as usize);
                    for i in 0..n {
                        if i > 0 {
                            cs.push(Constraint::Length(1));
                        }
                        let size = if i < extra { base + 1 } else { base };
                        cs.push(Constraint::Length(size));
                    }
                    cs
                } else {
                    (0..n).map(|_| Constraint::Ratio(1, n as u32)).collect()
                };
                let chunks = Layout::default()
                    .direction(dir)
                    .constraints(constraints)
                    .split(area);
                // When dividers were reserved we emit 2n-1 chunks: even
                // indices are panes, odd indices are dividers.
                let with_dividers = chunks.len() == (2 * n - 1) as usize;
                for (i, child) in children.iter().enumerate() {
                    let pane_idx = if with_dividers { i * 2 } else { i };
                    let rect = chunks[pane_idx];
                    path.push(i);
                    child.layout_inner(rect, path, out, dividers);
                    path.pop();
                    if with_dividers && i + 1 < children.len() {
                        dividers.push(Divider {
                            rect: chunks[pane_idx + 1],
                            direction: *direction,
                        });
                    }
                }
            }
        }
    }

    pub fn focus_direction(&self, focus: &[usize], arrow: Arrow) -> Option<FocusPath> {
        let need_dir = match arrow {
            Arrow::Left | Arrow::Right => SplitDirection::Vertical,
            Arrow::Up | Arrow::Down => SplitDirection::Horizontal,
        };
        let delta: isize = match arrow {
            Arrow::Left | Arrow::Up => -1,
            Arrow::Right | Arrow::Down => 1,
        };
        for depth in (0..focus.len()).rev() {
            let parent_path = &focus[..depth];
            let child_idx = focus[depth] as isize;
            let parent = at(self, parent_path)?;
            if let SplitTree::Split {
                direction,
                children,
            } = parent
                && *direction == need_dir
            {
                let new_idx = child_idx + delta;
                if new_idx >= 0 && (new_idx as usize) < children.len() {
                    let mut new_focus = parent_path.to_vec();
                    new_focus.push(new_idx as usize);
                    return Some(first_leaf_path(self, &new_focus));
                }
            }
        }
        None
    }
}

fn at<'a>(tree: &'a SplitTree, path: &[usize]) -> Option<&'a SplitTree> {
    let mut node = tree;
    for &i in path {
        match node {
            SplitTree::Leaf(_) => return None,
            SplitTree::Split { children, .. } => node = children.get(i)?,
        }
    }
    Some(node)
}

fn at_mut<'a>(tree: &'a mut SplitTree, path: &[usize]) -> Option<&'a mut SplitTree> {
    let mut node = tree;
    for &i in path {
        match node {
            SplitTree::Leaf(_) => return None,
            SplitTree::Split { children, .. } => node = children.get_mut(i)?,
        }
    }
    Some(node)
}

fn first_leaf_path(root: &SplitTree, base: &[usize]) -> FocusPath {
    let mut path = base.to_vec();
    let mut node = match at(root, base) {
        Some(n) => n,
        None => return base.to_vec(),
    };
    loop {
        match node {
            SplitTree::Leaf(_) => return path,
            SplitTree::Split { children, .. } => {
                if children.is_empty() {
                    return path;
                }
                path.push(0);
                node = &children[0];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{AgentInstanceId, WorkspaceId};

    /// Test leaf payload: a workspace and its (same-numbered) primary
    /// instance. Tree-structure assertions only ever compare these, so
    /// using `n` for both keeps the tests readable.
    fn wid(n: i64) -> AttachTarget {
        AttachTarget {
            workspace_id: WorkspaceId(n),
            instance: AgentInstanceId(n),
        }
    }

    #[test]
    fn single_leaf_layout_returns_full_area() {
        let s = AttachedState::single(wid(1));
        let result = s.layout(Rect::new(0, 0, 80, 24));
        assert_eq!(result.panes.len(), 1);
        assert!(result.dividers.is_empty());
        assert_eq!(result.panes[0].0, wid(1));
        assert_eq!(result.panes[0].1, Vec::<usize>::new());
        assert_eq!(result.panes[0].2, Rect::new(0, 0, 80, 24));
    }

    #[test]
    fn vertical_split_lays_side_by_side() {
        let mut s = AttachedState::single(wid(1));
        assert!(s.split(SplitDirection::Vertical, wid(2)));
        let result = s.layout(Rect::new(0, 0, 80, 24));
        let leaves = &result.panes;
        assert_eq!(leaves.len(), 2);
        // Vertical split = children laid out horizontally with a 1-col
        // divider between them. 80 - 1 = 79 content cells; first pane
        // gets the extra cell (40), second gets 39, divider sits at x=40.
        assert_eq!(leaves[0].2.x, 0);
        assert_eq!(leaves[0].2.width, 40);
        assert_eq!(leaves[1].2.x, 41);
        assert_eq!(leaves[1].2.width, 39);
        assert_eq!(leaves[0].2.width + leaves[1].2.width, 79);
        // Both panes share full height.
        assert_eq!(leaves[0].2.height, 24);
        assert_eq!(leaves[1].2.height, 24);
        // One vertical divider between them.
        assert_eq!(result.dividers.len(), 1);
        assert_eq!(result.dividers[0].direction, SplitDirection::Vertical);
        assert_eq!(result.dividers[0].rect, Rect::new(40, 0, 1, 24));
        // Focus moved to the new (second) pane.
        assert_eq!(s.focused_target(), Some(wid(2)));
    }

    #[test]
    fn horizontal_split_stacks_top_bottom() {
        let mut s = AttachedState::single(wid(1));
        assert!(s.split(SplitDirection::Horizontal, wid(2)));
        let result = s.layout(Rect::new(0, 0, 80, 24));
        let leaves = &result.panes;
        assert_eq!(leaves.len(), 2);
        // 24 - 1 (divider row) = 23 content rows: 12 + 11.
        assert_eq!(leaves[0].2.y, 0);
        assert_eq!(leaves[0].2.height, 12);
        assert_eq!(leaves[1].2.y, 13);
        assert_eq!(leaves[1].2.height, 11);
        assert_eq!(leaves[0].2.height + leaves[1].2.height, 23);
        assert_eq!(result.dividers.len(), 1);
        assert_eq!(result.dividers[0].direction, SplitDirection::Horizontal);
        assert_eq!(result.dividers[0].rect, Rect::new(0, 12, 80, 1));
    }

    #[test]
    fn same_direction_split_inserts_sibling_not_nests() {
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2));
        // Focus is on wid(2). Another vertical split should produce 3
        // siblings, not a nested split.
        s.split(SplitDirection::Vertical, wid(3));
        let result = s.layout(Rect::new(0, 0, 90, 24));
        let leaves = &result.panes;
        assert_eq!(leaves.len(), 3);
        assert_eq!(leaves[0].0, wid(1));
        assert_eq!(leaves[1].0, wid(2));
        assert_eq!(leaves[2].0, wid(3));
        // Two dividers between three siblings.
        assert_eq!(result.dividers.len(), 2);
        // Focus is on the newly inserted wid(3).
        assert_eq!(s.focused_target(), Some(wid(3)));
    }

    #[test]
    fn different_direction_split_nests() {
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2));
        // Focus on wid(2). Now split horizontally — should nest, replacing
        // wid(2) with a 2-child horizontal split (wid(2), wid(3)).
        s.split(SplitDirection::Horizontal, wid(3));
        let result = s.layout(Rect::new(0, 0, 80, 24));
        let leaves = &result.panes;
        assert_eq!(leaves.len(), 3);
        // Layout order: wid(1) on left, then nested split: wid(2) top, wid(3) bottom.
        assert_eq!(leaves[0].0, wid(1));
        assert_eq!(leaves[1].0, wid(2));
        assert_eq!(leaves[2].0, wid(3));
        assert!(leaves[1].2.y < leaves[2].2.y);
        assert_eq!(leaves[1].2.x, leaves[2].2.x);
        // One outer vertical divider plus one inner horizontal divider.
        assert_eq!(result.dividers.len(), 2);
    }

    #[test]
    fn close_focused_collapses_two_child_split() {
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2)); // focus on wid(2)
        match s.close_focused() {
            CloseOutcome::Focus(_) => {}
            CloseOutcome::Empty => panic!("should not be empty"),
        }
        assert_eq!(s.leaves(), vec![wid(1)]);
        assert_eq!(s.focused_target(), Some(wid(1)));
        // Tree should be a single Leaf again, not a 1-child Split.
        assert!(matches!(s.tree, SplitTree::Leaf(_)));
    }

    #[test]
    fn close_with_three_siblings_shrinks_split() {
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2));
        s.split(SplitDirection::Vertical, wid(3)); // focus on wid(3)
        match s.close_focused() {
            CloseOutcome::Focus(_) => {}
            CloseOutcome::Empty => panic!("should not be empty"),
        }
        assert_eq!(s.leaves(), vec![wid(1), wid(2)]);
        // Focus shifts to the new last index in the same parent.
        assert_eq!(s.focused_target(), Some(wid(2)));
    }

    #[test]
    fn close_last_leaf_returns_empty() {
        let mut s = AttachedState::single(wid(1));
        assert!(matches!(s.close_focused(), CloseOutcome::Empty));
    }

    #[test]
    fn focus_right_moves_to_next_sibling_in_vertical_split() {
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2)); // focus on wid(2)
        // Move left → wid(1).
        assert!(s.focus_direction(Arrow::Left));
        assert_eq!(s.focused_target(), Some(wid(1)));
        // Move right → wid(2).
        assert!(s.focus_direction(Arrow::Right));
        assert_eq!(s.focused_target(), Some(wid(2)));
        // Right again — no further sibling.
        assert!(!s.focus_direction(Arrow::Right));
        assert_eq!(s.focused_target(), Some(wid(2)));
    }

    #[test]
    fn focus_arrow_wrong_axis_does_not_move() {
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2));
        // Up/Down don't match a Vertical split — no movement.
        assert!(!s.focus_direction(Arrow::Up));
        assert!(!s.focus_direction(Arrow::Down));
        assert_eq!(s.focused_target(), Some(wid(2)));
    }

    #[test]
    fn focus_navigates_across_nested_splits() {
        // Layout: [wid(1) | (wid(2) / wid(3))]  (Vertical split, right child
        // is a Horizontal split.)
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2)); // focus wid(2)
        s.split(SplitDirection::Horizontal, wid(3)); // focus wid(3), nested under wid(2)
        // From wid(3), Up should move to wid(2) within the nested split.
        assert!(s.focus_direction(Arrow::Up));
        assert_eq!(s.focused_target(), Some(wid(2)));
        // From wid(2), Left walks up past the Horizontal split (no match),
        // hits the Vertical split, moves to wid(1).
        assert!(s.focus_direction(Arrow::Left));
        assert_eq!(s.focused_target(), Some(wid(1)));
    }

    #[test]
    fn focus_next_cycles_leaves() {
        let mut s = AttachedState::single(wid(1));
        s.split(SplitDirection::Vertical, wid(2));
        s.split(SplitDirection::Vertical, wid(3)); // focus wid(3)
        assert!(s.focus_next());
        assert_eq!(s.focused_target(), Some(wid(1)));
        assert!(s.focus_next());
        assert_eq!(s.focused_target(), Some(wid(2)));
    }

    #[test]
    fn splittree_serde_round_trip_preserves_nested_structure() {
        let mut tree = SplitTree::Leaf(wid(1));
        assert!(tree.split(&[], SplitDirection::Vertical, wid(2)).is_some());
        assert!(
            tree.split(&[1], SplitDirection::Horizontal, wid(3))
                .is_some()
        );
        let json = serde_json::to_string(&tree).expect("serialize");
        let back: SplitTree = serde_json::from_str(&json).expect("deserialize");
        let a = tree.layout(Rect::new(0, 0, 80, 24));
        let b = back.layout(Rect::new(0, 0, 80, 24));
        assert_eq!(a.panes.len(), b.panes.len());
        for (x, y) in a.panes.iter().zip(b.panes.iter()) {
            assert_eq!(x.0, y.0, "leaf id");
            assert_eq!(x.1, y.1, "focus path");
            assert_eq!(x.2, y.2, "rect");
        }
    }

    #[test]
    fn serde_round_trip_two_agents_same_workspace() {
        // Two leaves with the SAME workspace_id but DIFFERENT instance ids
        // (the two-agents-one-workspace case) must round-trip exactly.
        let a = AttachTarget {
            workspace_id: WorkspaceId(7),
            instance: AgentInstanceId(100),
        };
        let b = AttachTarget {
            workspace_id: WorkspaceId(7),
            instance: AgentInstanceId(200),
        };
        let mut tree = SplitTree::Leaf(a);
        assert!(tree.split(&[], SplitDirection::Vertical, b).is_some());
        let json = serde_json::to_string(&tree).expect("serialize");
        let back: SplitTree = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.leaves(), vec![a, b]);
    }

    #[test]
    fn workspaceid_serializes_as_bare_integer() {
        let id = crate::data::store::WorkspaceId(42);
        assert_eq!(serde_json::to_string(&id).unwrap(), "42");
        let back: crate::data::store::WorkspaceId = serde_json::from_str("42").unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn prune_removes_dropped_leaves_and_collapses_singletons() {
        // (A | B | C), prune B → (A | C)
        let mut tree = SplitTree::Leaf(wid(1));
        tree.split(&[], SplitDirection::Vertical, wid(2));
        tree.split(&[1], SplitDirection::Vertical, wid(3));
        let outcome = tree.prune(&|id| id != wid(2));
        assert!(matches!(outcome, PruneOutcome::Kept));
        assert_eq!(tree.leaves(), vec![wid(1), wid(3)]);
    }

    #[test]
    fn prune_collapses_nested_singleton() {
        // (A | (B / C)) — prune C → (A | B). The nested split must collapse
        // (no 1-child Split allowed).
        let mut tree = SplitTree::Leaf(wid(1));
        tree.split(&[], SplitDirection::Vertical, wid(2));
        tree.split(&[1], SplitDirection::Horizontal, wid(3));
        let outcome = tree.prune(&|id| id != wid(3));
        assert!(matches!(outcome, PruneOutcome::Kept));
        assert_eq!(tree.leaves(), vec![wid(1), wid(2)]);
        fn no_singleton_splits(t: &SplitTree) {
            if let SplitTree::Split { children, .. } = t {
                assert!(children.len() >= 2, "found singleton split");
                for c in children {
                    no_singleton_splits(c);
                }
            }
        }
        no_singleton_splits(&tree);
    }

    #[test]
    fn prune_returns_empty_when_no_leaves_survive() {
        let mut tree = SplitTree::Leaf(wid(1));
        tree.split(&[], SplitDirection::Vertical, wid(2));
        let outcome = tree.prune(&|_| false);
        assert!(matches!(outcome, PruneOutcome::Empty));
    }

    #[test]
    fn prune_keeps_leaf_when_predicate_true() {
        let mut tree = SplitTree::Leaf(wid(1));
        let outcome = tree.prune(&|_| true);
        assert!(matches!(outcome, PruneOutcome::Kept));
        assert_eq!(tree.leaves(), vec![wid(1)]);
    }

    #[test]
    fn first_leaf_path_returns_empty_for_leaf_root() {
        let tree = SplitTree::Leaf(wid(1));
        assert_eq!(tree.first_leaf_path(), Vec::<usize>::new());
    }

    #[test]
    fn first_leaf_path_walks_to_leftmost_leaf_of_nested_splits() {
        // (A | (B / C)) — first leaf path is [0] (A is the leftmost leaf).
        let mut tree = SplitTree::Leaf(wid(1));
        tree.split(&[], SplitDirection::Vertical, wid(2));
        tree.split(&[1], SplitDirection::Horizontal, wid(3));
        assert_eq!(tree.first_leaf_path(), vec![0]);
    }
}
