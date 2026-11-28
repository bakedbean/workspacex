//! Pure edits to a detail-bar container layout (`containers` in
//! [`DetailBarConfig`]), for the dashboard's layout editor. Each edit
//! takes the layout and a cursor and returns where the cursor lands, or
//! `None` when the edit doesn't apply (moving past an edge, adding a
//! fifth container…), leaving the layout untouched.
//!
//! A module appears at most once: [`place`] moves a module that is
//! already in the layout instead of duplicating it.
//!
//! [`DetailBarConfig`]: super::detail_bar_config::DetailBarConfig

/// Containers the bar draws at most; `DetailBarConfig::sanitize`
/// truncates past this.
pub const MAX_CONTAINERS: usize = 4;

pub type Containers = Vec<Vec<String>>;

/// A cursor: container index, then slot within it.
pub type Pos = (usize, usize);

/// Where module `id` sits, if it is placed.
pub fn find(containers: &Containers, id: &str) -> Option<Pos> {
    containers
        .iter()
        .enumerate()
        .find_map(|(c, mods)| mods.iter().position(|m| m == id).map(|r| (c, r)))
}

/// Append `id` to container `col`, taking it out of wherever it already
/// was. `None` when `col` doesn't exist.
pub fn place(containers: &mut Containers, id: &str, col: usize) -> Option<Pos> {
    if col >= containers.len() {
        return None;
    }
    if let Some((c, r)) = find(containers, id) {
        containers[c].remove(r);
    }
    containers[col].push(id.to_string());
    Some((col, containers[col].len() - 1))
}

/// Take the module at `pos` out of the layout. Returns the cursor's new
/// slot: the module that slid into its place, else the one above.
pub fn remove(containers: &mut Containers, (col, row): Pos) -> Option<Pos> {
    let mods = containers.get_mut(col)?;
    if row >= mods.len() {
        return None;
    }
    mods.remove(row);
    Some((col, row.min(mods.len().saturating_sub(1))))
}

/// Move the module at `pos` into the neighbouring container (`left` or
/// right), keeping its slot where the target has room.
pub fn shift(containers: &mut Containers, (col, row): Pos, left: bool) -> Option<Pos> {
    let target = neighbour(col, containers.len(), left)?;
    if row >= containers[col].len() {
        return None;
    }
    let id = containers[col].remove(row);
    let slot = row.min(containers[target].len());
    containers[target].insert(slot, id);
    Some((target, slot))
}

/// Swap the module at `pos` with the one above (`up`) or below it.
pub fn reorder(containers: &mut Containers, (col, row): Pos, up: bool) -> Option<Pos> {
    let mods = containers.get_mut(col)?;
    let other = neighbour(row, mods.len(), up)?;
    mods.swap(row, other);
    Some((col, other))
}

/// Swap container `col` with its neighbour (`left` or right).
pub fn swap_container(containers: &mut Containers, col: usize, left: bool) -> Option<usize> {
    let other = neighbour(col, containers.len(), left)?;
    containers.swap(col, other);
    Some(other)
}

/// Insert an empty container to the right of `col`. `None` at
/// [`MAX_CONTAINERS`].
pub fn add_container(containers: &mut Containers, col: usize) -> Option<usize> {
    if containers.len() >= MAX_CONTAINERS {
        return None;
    }
    let at = (col + 1).min(containers.len());
    containers.insert(at, Vec::new());
    Some(at)
}

/// Delete container `col` and the modules in it. `None` for the last
/// container: an empty layout means "use the default", not "no body".
pub fn remove_container(containers: &mut Containers, col: usize) -> Option<usize> {
    if containers.len() <= 1 || col >= containers.len() {
        return None;
    }
    containers.remove(col);
    Some(col.min(containers.len() - 1))
}

/// Clamp a cursor into the layout: a real container, and a real slot in
/// it (slot 0 for an empty container).
pub fn clamp(containers: &Containers, (col, row): Pos) -> Pos {
    let col = col.min(containers.len().saturating_sub(1));
    let len = containers.get(col).map_or(0, Vec::len);
    (col, row.min(len.saturating_sub(1)))
}

fn neighbour(i: usize, len: usize, before: bool) -> Option<usize> {
    if i >= len {
        return None;
    }
    if before {
        i.checked_sub(1)
    } else {
        (i + 1 < len).then_some(i + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(spec: &[&[&str]]) -> Containers {
        spec.iter()
            .map(|c| c.iter().map(|s| s.to_string()).collect())
            .collect()
    }

    #[test]
    fn place_appends_new_module() {
        let mut l = layout(&[&["a"], &[]]);
        assert_eq!(place(&mut l, "b", 1), Some((1, 0)));
        assert_eq!(l, layout(&[&["a"], &["b"]]));
    }

    #[test]
    fn place_moves_a_placed_module_instead_of_duplicating() {
        let mut l = layout(&[&["a", "b"], &["c"]]);
        assert_eq!(place(&mut l, "a", 1), Some((1, 1)));
        assert_eq!(l, layout(&[&["b"], &["c", "a"]]));
    }

    #[test]
    fn place_into_missing_container_is_refused() {
        let mut l = layout(&[&["a"]]);
        assert_eq!(place(&mut l, "b", 3), None);
        assert_eq!(l, layout(&[&["a"]]));
    }

    #[test]
    fn remove_lands_on_next_then_previous_slot() {
        let mut l = layout(&[&["a", "b", "c"]]);
        assert_eq!(remove(&mut l, (0, 1)), Some((0, 1)));
        assert_eq!(l, layout(&[&["a", "c"]]));
        assert_eq!(remove(&mut l, (0, 1)), Some((0, 0)));
        assert_eq!(remove(&mut l, (0, 0)), Some((0, 0)));
        assert_eq!(l, layout(&[&[]]));
        assert_eq!(remove(&mut l, (0, 0)), None);
    }

    #[test]
    fn shift_keeps_slot_where_target_has_room() {
        let mut l = layout(&[&["a", "b"], &["c", "d", "e"]]);
        assert_eq!(shift(&mut l, (0, 1), false), Some((1, 1)));
        assert_eq!(l, layout(&[&["a"], &["c", "b", "d", "e"]]));
        let mut l = layout(&[&["a", "b", "c"], &[]]);
        assert_eq!(shift(&mut l, (0, 2), false), Some((1, 0)));
        assert_eq!(l, layout(&[&["a", "b"], &["c"]]));
    }

    #[test]
    fn shift_past_an_edge_is_refused() {
        let mut l = layout(&[&["a"], &["b"]]);
        assert_eq!(shift(&mut l, (0, 0), true), None);
        assert_eq!(shift(&mut l, (1, 0), false), None);
        assert_eq!(l, layout(&[&["a"], &["b"]]));
    }

    #[test]
    fn reorder_swaps_within_container() {
        let mut l = layout(&[&["a", "b", "c"]]);
        assert_eq!(reorder(&mut l, (0, 2), true), Some((0, 1)));
        assert_eq!(l, layout(&[&["a", "c", "b"]]));
        assert_eq!(reorder(&mut l, (0, 0), true), None);
        assert_eq!(reorder(&mut l, (0, 2), false), None);
    }

    #[test]
    fn swap_container_moves_whole_column() {
        let mut l = layout(&[&["a"], &["b", "c"]]);
        assert_eq!(swap_container(&mut l, 1, true), Some(0));
        assert_eq!(l, layout(&[&["b", "c"], &["a"]]));
        assert_eq!(swap_container(&mut l, 0, true), None);
    }

    #[test]
    fn add_container_inserts_right_and_caps_at_max() {
        let mut l = layout(&[&["a"], &["b"]]);
        assert_eq!(add_container(&mut l, 0), Some(1));
        assert_eq!(l, layout(&[&["a"], &[], &["b"]]));
        assert_eq!(add_container(&mut l, 2), Some(3));
        assert_eq!(l.len(), MAX_CONTAINERS);
        assert_eq!(add_container(&mut l, 0), None);
    }

    #[test]
    fn remove_container_keeps_the_last_one() {
        let mut l = layout(&[&["a"], &["b"]]);
        assert_eq!(remove_container(&mut l, 1), Some(0));
        assert_eq!(l, layout(&[&["a"]]));
        assert_eq!(remove_container(&mut l, 0), None);
    }

    #[test]
    fn clamp_pulls_cursor_into_layout() {
        let l = layout(&[&["a", "b"], &[]]);
        assert_eq!(clamp(&l, (5, 5)), (1, 0));
        assert_eq!(clamp(&l, (0, 9)), (0, 1));
    }

    #[test]
    fn find_locates_module() {
        let l = layout(&[&["a"], &["b", "c"]]);
        assert_eq!(find(&l, "c"), Some((1, 1)));
        assert_eq!(find(&l, "z"), None);
    }
}
