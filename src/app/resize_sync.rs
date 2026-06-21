//! Keep backgrounded agent PTYs sized to the terminal so re-attaching after a
//! resize doesn't show a vt100 frame clipped to stale dimensions.
//!
//! ## Why this exists
//!
//! wsx renders each agent through an in-memory `vt100::Parser`. A session's PTY
//! size is reconciled only in the *attached* render path
//! (`ui::attached::resize_pane`, run every frame). While a workspace is
//! detached (Dashboard) or a different workspace is attached, a terminal resize
//! never reaches a backgrounded session's PTY. On re-attach, `resize_pane`
//! calls `vt100::set_size`, which truncates and de-wraps the existing screen to
//! the new width (it does *not* reflow) — garbling the visible frame, and
//! permanently garbling the scrollback that the live repaint never redraws.
//!
//! Fix: on a (debounced) terminal resize, resize every *backgrounded* running
//! session's PTY to the projected single-pane size. Each session's agent then
//! repaints at the right width while detached, so the on-attach `resize_pane`
//! is a no-op and nothing gets clipped.
//!
//! The project-manager (PM) session is out of scope here: it lives outside
//! `SessionManager::sessions` and is resized every frame by its own render
//! paths (the dashboard PM pane via `pm_pane::resize_session`, and the
//! full-screen `AttachedPm` view), so it never becomes a stale backgrounded
//! session. Including it in this sweep would fight those render paths.
//!
//! This module holds the pure, clock-injected pieces; the wiring into the event
//! loop lives in `app.rs` and the per-session resize in `SessionManager`.

use crate::data::store::AgentInstanceId;
use crate::ui::View;
use ratatui::layout::Rect;
use std::collections::HashSet;

/// Quiet period after the last resize event before applying. A window drag
/// emits a burst of events; coalescing to the final size avoids N
/// SIGWINCH-driven repaints per backgrounded session per gesture.
pub const DEBOUNCE_MS: u64 = 80;

/// Coalesces a burst of terminal-resize events into a single pending size,
/// applied once the resize settles. Clock is injected so it's unit-testable.
#[derive(Default)]
pub struct ResizeDebounce {
    pending: Option<Pending>,
}

struct Pending {
    cols: u16,
    rows: u16,
    due_ms: u64,
}

impl ResizeDebounce {
    /// Record a resize to `(cols, rows)` observed at `now_ms`. The latest
    /// dimensions win and the deadline is pushed out, so a continuous drag
    /// keeps deferring until it stops.
    pub fn note(&mut self, cols: u16, rows: u16, now_ms: u64) {
        self.pending = Some(Pending {
            cols,
            rows,
            due_ms: now_ms.saturating_add(DEBOUNCE_MS),
        });
    }

    /// If a resize is pending and its quiet period has elapsed by `now_ms`,
    /// return the final `(cols, rows)` and clear it. Otherwise `None`.
    pub fn take_due(&mut self, now_ms: u64) -> Option<(u16, u16)> {
        match &self.pending {
            Some(p) if now_ms >= p.due_ms => {
                let dims = (p.cols, p.rows);
                self.pending = None;
                Some(dims)
            }
            _ => None,
        }
    }
}

/// The pane size a single-pane attach gives a session on a terminal of
/// `cols × rows`. Mirrors `ui::attached::layout_chrome(.., false)` so the
/// on-attach `resize_pane` matches and stays a no-op in the common case.
///
/// `agents_present` is assumed `false`: while detached we don't know which
/// workspace will be attached. When the focused workspace shows the agents row,
/// the real pane is 1 row shorter, so the on-attach resize trims one row —
/// cosmetic and self-healing on the agent's next repaint. The width (the
/// dimension that drives the destructive clip) is always exact for a single
/// pane.
pub fn projected_pane_size(cols: u16, rows: u16) -> (u16, u16) {
    let (_, _, pane, _, _) = crate::ui::attached::layout_chrome(Rect::new(0, 0, cols, rows), false);
    (pane.width, pane.height)
}

/// Instances currently rendered (and thus already kept in sync by the attached
/// render path). The backgrounded resize skips these so it never resizes — and
/// thereby garbles — a pane the user is looking at.
pub fn visible_instances(view: &View) -> HashSet<AgentInstanceId> {
    match view {
        View::Attached(state) => state.leaves().into_iter().map(|t| t.instance).collect(),
        View::Dashboard | View::AttachedPm => HashSet::new(),
    }
}

/// Whether the backgrounded sweep should resize this session. It must be
/// running (resizing a dead PTY is pointless) and not currently visible: the
/// render path already keeps visible panes sized every frame, and resizing one
/// here would clip the very frame the user is looking at.
pub fn should_resize_backgrounded(
    instance: AgentInstanceId,
    is_running: bool,
    visible: &HashSet<AgentInstanceId>,
) -> bool {
    is_running && !visible.contains(&instance)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{AgentInstanceId, WorkspaceId};
    use crate::ui::split::{AttachTarget, AttachedState, SplitDirection};

    fn target(id: i64) -> AttachTarget {
        AttachTarget {
            workspace_id: WorkspaceId(id),
            instance: AgentInstanceId(id),
        }
    }

    #[test]
    fn debounce_withholds_until_quiet_period_elapses() {
        let mut d = ResizeDebounce::default();
        d.note(100, 30, 1_000);
        assert_eq!(d.take_due(1_000 + DEBOUNCE_MS - 1), None, "not yet due");
        assert_eq!(
            d.take_due(1_000 + DEBOUNCE_MS),
            Some((100, 30)),
            "due once the quiet period elapses"
        );
    }

    #[test]
    fn debounce_fires_once_then_clears() {
        let mut d = ResizeDebounce::default();
        d.note(80, 24, 0);
        assert_eq!(d.take_due(DEBOUNCE_MS), Some((80, 24)));
        assert_eq!(
            d.take_due(DEBOUNCE_MS + 1_000),
            None,
            "nothing pending after firing"
        );
    }

    #[test]
    fn debounce_coalesces_to_latest_dimensions_and_defers_deadline() {
        let mut d = ResizeDebounce::default();
        d.note(100, 30, 0);
        // A second event within the first window: latest dims win and the
        // deadline is pushed out to now + DEBOUNCE_MS.
        d.note(120, 40, 50);
        assert_eq!(d.take_due(80), None, "deadline deferred by the second note");
        assert_eq!(
            d.take_due(50 + DEBOUNCE_MS),
            Some((120, 40)),
            "latest dimensions applied"
        );
    }

    #[test]
    fn projected_pane_size_reserves_chrome_rows_and_keeps_full_width() {
        // info + separator + chip rows are reserved (agents row absent);
        // width is the full terminal width.
        assert_eq!(projected_pane_size(100, 30), (100, 27));
    }

    #[test]
    fn visible_instances_empty_when_not_attached() {
        assert!(visible_instances(&View::Dashboard).is_empty());
        assert!(visible_instances(&View::AttachedPm).is_empty());
    }

    #[test]
    fn should_resize_backgrounded_only_when_running_and_hidden() {
        let visible: HashSet<AgentInstanceId> = [AgentInstanceId(1)].into_iter().collect();
        // Running + hidden → resize it (the bug case: a detached session).
        assert!(should_resize_backgrounded(
            AgentInstanceId(2),
            true,
            &visible
        ));
        // Running but visible → skip (render path owns it; resizing clips it).
        assert!(!should_resize_backgrounded(
            AgentInstanceId(1),
            true,
            &visible
        ));
        // Not running → skip regardless of visibility.
        assert!(!should_resize_backgrounded(
            AgentInstanceId(2),
            false,
            &visible
        ));
    }

    #[test]
    fn visible_instances_collects_all_attached_leaves() {
        let mut state = AttachedState::single(target(1));
        state.split(SplitDirection::Vertical, target(2));
        let vis = visible_instances(&View::Attached(state));
        assert!(vis.contains(&AgentInstanceId(1)), "first leaf present");
        assert!(vis.contains(&AgentInstanceId(2)), "second leaf present");
        assert_eq!(vis.len(), 2, "exactly the two leaves");
    }
}
