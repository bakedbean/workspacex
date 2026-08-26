//! Drawing the dashboard view: the workspace list, its columns, and
//! the detail bar pinned under it.

use super::*;
use crate::app::activity::classify_activity_with_events;
use crate::app::bell::{COLD_START_WINDOW, alert_decision};
use crate::app::{App, SelectionTarget};
use crate::config::detail_bar_config::DetailBarConfig;
use crate::data::store::Store;
use crate::ui::dashboard::row::ColumnWidths;
use ratatui::layout::{Constraint, Direction, Layout};

/// The workspace list plus, when a workspace is selected, the detail bar.
pub(super) fn draw_dashboard(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    use crate::ui::dashboard;
    let selection_is_workspace =
        matches!(app.selected_target(), Some(SelectionTarget::Workspace(_)));
    let detail_cfg = resolve_dashboard_detail_cfg(app);
    let detail_visible = selection_is_workspace
        && detail_cfg.visible
        && area.height >= detail_cfg.minimum_height() + 10;
    // If the bar is hidden but focus is on the reply input,
    // bounce focus back to Dashboard and drop the draft.
    if !detail_visible && matches!(app.focus, crate::ui::PaneFocus::DetailBarReply) {
        app.focus = crate::ui::PaneFocus::Dashboard;
        app.dashboard.reply_draft.clear();
    }
    // Carve a 1-row footer off the bottom of the full area so the
    // spec order (list / detail / pm / footer) is respected. The
    // detail and PM regions are placed ABOVE the footer row.
    let inner_area = if area.height > 1 {
        ratatui::layout::Rect {
            height: area.height - 1,
            ..area
        }
    } else {
        area
    };
    let footer_area = ratatui::layout::Rect {
        y: area.y + area.height.saturating_sub(1),
        height: 1,
        ..area
    };
    let (dashboard_area, detail_area, pm_area) =
        dashboard_regions(inner_area, app.pm_visible, detail_visible, &detail_cfg);
    let notifications_on = notifications_enabled(&app.store);
    let nerd_fonts = nerd_fonts_enabled(&app.store);

    // Build per-workspace inputs in V5 shape.
    let now_ms = crate::util::time::now_ms();
    let mut workspaces: Vec<dashboard::WorkspaceItem<'_>> = Vec::new();
    for repo in &app.repos {
        for (rid, ws) in &app.workspaces {
            if *rid != repo.id {
                continue;
            }
            let row = build_row_inputs(app, ws, now_ms, nerd_fonts);
            workspaces.push(dashboard::WorkspaceItem {
                repo,
                workspace_id: ws.id,
                status: row.status,
                row,
            });
        }
    }

    // Commit the new activity states. Fires the bell on:
    //   - transition from any non-alertable state into
    //     AwaitingAnswer / Complete / Awaiting / Stalled,
    //   - transition between two different alertable states
    //     (e.g. Complete -> Awaiting when a permission prompt
    //     arrives while the user hasn't yet replied to the prior
    //     end_turn).
    // Activity is not recorded — and the bell is not considered —
    // until the tail loop has scanned the workspace's JSONL at
    // least once (see `workspace_events_scanned`). Without that
    // gate the classifier flickers from a provisional Active to
    // a real AwaitingAnswer/Complete the instant events arrive,
    // which would ring on cold start for every already-waiting
    // workspace. Once scanned, the first observation still skips
    // the bell (see `alert_decision`) so the visual marker can
    // surface alertable state without making noise. Does NOT
    // re-fire while an alertable state persists across polls.
    //
    // Keeps the legacy `ActivityState` vocabulary (via
    // `classify_activity_with_events`) for the bell pipeline —
    // the V5 `Status` enum is for display only and would lose the
    // `Active`/`Off`/`Awaiting` distinctions `alert_decision`
    // depends on.
    for (_rid, ws) in &app.workspaces {
        let session = app
            .primary_instance(ws.id)
            .and_then(|i| app.sessions.get(i));
        let running = session.as_ref().is_some_and(|s| {
            matches!(
                *s.status.read().unwrap(),
                crate::pty::session::SessionStatus::Running { .. }
            )
        });
        let secs = session.as_ref().map(|s| s.idle_secs().unwrap_or(0));
        let awaiting = app.awaiting_permission(ws.id).is_some();
        let now_ms = crate::util::time::now_ms();
        let stopped_kind = app
            .workspace_events
            .get(&ws.id)
            .and_then(crate::app::derive_stopped_kind);
        let stalled = app
            .workspace_events
            .get(&ws.id)
            .is_some_and(|e| e.is_stalled(now_ms, 60_000));
        let activity =
            classify_activity_with_events(secs, running, awaiting, stopped_kind, stalled);
        if app.workspace_events_scanned.contains(&ws.id) {
            let prev = app.workspace_activity.get(&ws.id).copied();
            let is_cold_start = app.started_at.elapsed() < COLD_START_WINDOW;
            let (mark_attention, fire_bell) =
                alert_decision(prev, activity, notifications_on, is_cold_start);
            if mark_attention {
                app.workspace_needs_attention.insert(ws.id);
            }
            if fire_bell {
                app.pending_bells.push(activity);
            }
            app.workspace_activity.insert(ws.id, activity);
        }
    }

    // Aggregate the retained hourly buckets into a fixed 24-bar,
    // time-aligned sparkline for the configured window.
    let window = crate::config::usage_window::resolve(&app.store);
    let now_secs = crate::util::time::now_secs();
    let now_hour = now_secs - (now_secs % 3600);
    // VecDeque is non-contiguous; collect into a slice-able Vec so
    // aggregate_buckets can take it as `&[(u64, u32)]`.
    let history: Vec<(u64, u32)> = app.activity_history.iter().copied().collect();
    let activity: Vec<u32> =
        crate::ui::dashboard::sparkline::aggregate_buckets(&history, now_hour, window.hours(), 24);
    let column_widths = read_column_widths(&app.store);
    let inputs = dashboard::DashboardInputs {
        repos: app.repos.iter().collect(),
        workspaces,
        activity: &activity,
        column_widths,
        github_remotes: &app.github_remotes,
        nerd_fonts,
    };
    // Rebuild `selectable` in the V5 visible order (repos ordered
    // by persisted `sort_order`, priority-sort within repo, hide
    // folded workspaces, apply filter). Nav keys index into this Vec,
    // so it must match what the renderer emits below or the
    // selection will appear to skip rows / jump back.
    let new_selectable = dashboard::visible_targets(&inputs, &app.dashboard);
    // Reconcile the durable selection against the rebuilt list every
    // frame. A temporarily-hidden target (folded repo / filter / quiet
    // repo) is PARKED on the same WorkspaceId rather than clamped onto a
    // neighbor, and restored when its row reappears. Running this
    // unconditionally (rather than only when `new_selectable` differs)
    // also drops a selection whose workspace was archived: `refresh()`
    // rebuilds `selectable` between draws, so the shape can be unchanged
    // here even though the target no longer exists.
    let prev_selection = app.dashboard.selection;
    let prev_selected = app.dashboard.selected;
    let (selection, selected) =
        dashboard::reconcile_selection(prev_selection, prev_selected, &new_selectable, |t| {
            app.selection_target_exists(t)
        });
    app.selectable = new_selectable;
    app.dashboard.selection = selection;
    app.dashboard.selected = selected;
    let click_targets = dashboard::render_without_footer(
        f,
        dashboard_area,
        &inputs,
        &mut app.dashboard,
        app.tick,
        &app.theme,
    );
    app.dashboard_pr_rects = click_targets.pr_chips;
    app.dashboard_repo_pr_rects = click_targets.repo_pr_links;
    if let Some(pm_area) = pm_area {
        let digest = app.build_pm_digest();
        let selected = app
            .pm_digest_selected
            .min(crate::ui::pm_pane::card_count(&digest).saturating_sub(1));
        crate::ui::pm_pane::render_digest(
            f,
            pm_area,
            &digest,
            selected,
            app.focus,
            app.pm_filter.as_deref(),
            crate::util::time::now_ms(),
            &app.theme,
        );
    }
    if let (Some(detail_area), Some(SelectionTarget::Workspace(ws_id))) =
        (detail_area, app.selected_target())
    {
        if let Some((rid, ws)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
            if let Some(repo) = app.repos.iter().find(|r| r.id == *rid) {
                let now_ms = crate::util::time::now_ms();
                let ago_secs = workspace_age_secs(app, ws.id, now_ms);
                let status = app.classify_status(ws);
                let procs: &[crate::activity::proc::ProcInfo] = app
                    .workspace_processes
                    .get(&ws.id)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]);
                let global_pinned = app.store.get_setting("pinned_commands").ok().flatten();
                let pinned = crate::commands::pinned::resolve(
                    global_pinned.as_deref(),
                    repo.pinned_commands.as_deref(),
                );
                crate::app::reset_detail_scroll_on_workspace_change(
                    &mut app.detail_scroll_offsets,
                    &mut app.detail_scroll_last_workspace,
                    Some(ws.id),
                );
                let mut inputs = crate::ui::dashboard::detail::DetailInputs {
                    repo,
                    workspace: ws,
                    events: app.workspace_events.get(&ws.id),
                    recap: app.recaps.get(&ws.id),
                    procs,
                    diff: app.workspace_diff.get(&ws.id).copied(),
                    diff_per_file: app.workspace_diff_per_file.get(&ws.id),
                    lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                    pr_title: None,
                    pr_number: app.pr_number.get(&ws.id).copied(),
                    review: app.pr_review.get(&ws.id).copied(),
                    status,
                    ago_secs,
                    reply_draft: &app.dashboard.reply_draft,
                    reply_focused: matches!(app.focus, crate::ui::PaneFocus::DetailBarReply),
                    events_scanned: app.workspace_events_scanned.contains(&ws.id),
                    config: &detail_cfg,
                    registry: &app.registry,
                    pinned: &pinned,
                    scroll_offsets: &mut app.detail_scroll_offsets,
                };
                let out =
                    crate::ui::dashboard::detail::render(f, detail_area, &mut inputs, &app.theme);
                app.detail_container_rects = out.container_rects;
                app.pr_link_rect = out.pr_link_rect.map(|r| (ws.id, r));
                if !out.chip_rects.is_empty() {
                    app.chip_rects = out.chip_rects;
                    app.pinned_commands_cache = pinned;
                }
            }
        }
    }
    // Render footer below detail/PM so the spec order
    // list / detail / pm / footer is respected.
    let (graph_rect, footer_hint_rects) = dashboard::render_footer(
        f,
        footer_area,
        &activity,
        &app.theme,
        window.label(),
        matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))),
    );
    app.usage_graph_rect = Some(graph_rect);
    app.footer_hint_rects = footer_hint_rects;
}

/// Derive a row's lifecycle badge. A live `in_flight` entry always wins:
/// it proves work is running right now, whereas the persisted statuses
/// describe how the last attempt ended.
pub(crate) fn lifecycle_badge_for(
    state: &crate::data::store::WorkspaceState,
    setup_status: &crate::data::store::SetupStatus,
    in_flight: Option<crate::data::in_flight::InFlightKind>,
) -> Option<crate::ui::dashboard::row::LifecycleBadge> {
    use crate::data::in_flight::InFlightKind;
    use crate::data::store::{SetupStatus, WorkspaceState};
    use crate::ui::dashboard::row::LifecycleBadge;

    match in_flight {
        Some(InFlightKind::Create) => return Some(LifecycleBadge::Provisioning),
        Some(InFlightKind::Archive) => return Some(LifecycleBadge::Archiving),
        None => {}
    }
    match (state, setup_status) {
        (WorkspaceState::Failed, _) => Some(LifecycleBadge::NoWorktree),
        (_, SetupStatus::Failed) => Some(LifecycleBadge::SetupFailed),
        (_, SetupStatus::Cancelled) => Some(LifecycleBadge::SetupCancelled),
        // A persisted `Running` with no registry entry is crash residue that
        // `sweep_stale_running` already resolved before the first draw.
        _ => None,
    }
}

/// Build one workspace's dashboard row inputs: status classification, live
/// peer agents, activity age, and the assorted per-row cache lookups the
/// renderer needs. Extracted from the per-workspace loop in `draw` so it's
/// callable directly from tests without spinning up a `TestBackend` frame.
/// Seconds since a workspace was last active, or `None` if nothing has ever
/// recorded activity for it.
///
/// Prefers whichever signal is more recent. `session.activity_ms` only exists
/// for workspaces wsx is currently attached to, so a detached workspace — and
/// every workspace right after a wsx restart — has only the event log to go
/// on. The JSONL event's own `timestamp_ms` (parsed from the line's
/// `timestamp` field) is the actual event time; deliberately NOT
/// `last_log_activity_ms`, which is the wall-clock time when wsx observed the
/// log growing and gets stamped to "now" on the first tail pass after startup,
/// which would make every workspace claim the same age from zero.
///
/// Shared by the dashboard rows and the detail bar so the two cannot report
/// different ages for the same workspace — and because the dashboard's recency
/// ordering keys off this, a row missing its age sorts as never-active.
pub(crate) fn workspace_age_secs(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
    now_ms: i64,
) -> Option<u64> {
    let session_last_ms = app
        .primary_instance(ws_id)
        .and_then(|i| app.sessions.get(i))
        .map(|s| s.activity_ms.load(std::sync::atomic::Ordering::Relaxed) as i64)
        .unwrap_or(0);
    let event_last_ms = app
        .workspace_events
        .get(&ws_id)
        .and_then(|e| e.latest.as_ref().map(|ev| ev.timestamp_ms))
        .unwrap_or(0);
    let last_ms = session_last_ms.max(event_last_ms);
    if last_ms == 0 {
        None
    } else {
        Some(((now_ms - last_ms).max(0) / 1000) as u64)
    }
}

pub(super) fn build_row_inputs(
    app: &App,
    ws: &crate::data::store::Workspace,
    now_ms: i64,
    nerd_fonts: bool,
) -> crate::ui::dashboard::row::RowInputs {
    let status = app.classify_status(ws);
    let session = app
        .primary_instance(ws.id)
        .and_then(|i| app.sessions.get(i));
    let secs = workspace_age_secs(app, ws.id, now_ms);
    let badge = lifecycle_badge_for(
        &ws.state,
        &ws.setup_status,
        app.in_flight.get(&ws.id).map(|f| f.kind),
    );
    let undelivered_mail = app.stuck_mail.contains(&ws.id);
    // Badge liveness: a running client in this wsx (the tmux
    // attach client) or a detached-but-alive server session
    // (from the throttled has-session sweep) both mean the
    // agent is alive in tmux right now.
    let session_running = session.as_ref().is_some_and(|s| {
        matches!(
            *s.status.read().unwrap(),
            crate::pty::session::SessionStatus::Running { .. }
        )
    });
    let shared_active = ws.shared && (session_running || app.shared_detached.contains(&ws.id));
    crate::ui::dashboard::row::RowInputs {
        agent: ws.agent,
        // Peers only, primary excluded — it renders unconditionally as
        // the rightmost bar. Order is roster order (creation time), so a
        // newly added peer lands next to the primary.
        peers: app
            .strip_instances(ws.id)
            .into_iter()
            .filter(|inst| !inst.is_primary)
            .map(|inst| inst.agent)
            .collect(),
        status,
        branch: ws.branch.clone(),
        pr_number: app.pr_number.get(&ws.id).copied(),
        procs: app
            .workspace_processes
            .get(&ws.id)
            .map(|v| v.len() as u32)
            .unwrap_or(0),
        diff: app.workspace_diff.get(&ws.id).copied(),
        column: Some(crate::ui::dashboard::column_content::row_column(
            status,
            app.workspace_events.get(&ws.id),
            now_ms,
            app.fresh_reported_status(ws.id),
            app.recaps.get(&ws.id),
        )),
        ago_secs: secs,
        selected: matches!(app.selected_target(),
            Some(SelectionTarget::Workspace(id)) if id == ws.id),
        yolo: ws.yolo,
        badge,
        undelivered_mail,
        shared: ws.shared,
        shared_active,
        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
        review: app.pr_review.get(&ws.id).copied(),
        nerd_fonts,
        name_color: ws.name_color.map(crate::config::name_color::color),
        workspace_id: ws.id,
        has_multi_pane_layout: app.workspaces_with_multi_pane_layouts.contains(&ws.id),
    }
}

/// Resolve the detail-bar config for the current selection. When a
/// workspace is selected, uses its repo's override; otherwise uses
/// global-only (no repo override applies when no repo is in focus).
pub(crate) fn resolve_dashboard_detail_cfg(app: &App) -> DetailBarConfig {
    if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
        if let Some((rid, _)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
            if let Some(repo) = app.repos.iter().find(|r| r.id == *rid) {
                return crate::config::detail_bar_config::resolve(repo, &app.store);
            }
        }
    }
    crate::config::detail_bar_config::resolve_global_only(&app.store)
}

/// Carve the dashboard area into list / detail / pm regions based on
/// whether PM is visible and whether a workspace is selected.
pub(super) fn dashboard_regions(
    area: ratatui::layout::Rect,
    pm_visible: bool,
    detail_visible: bool,
    detail_cfg: &DetailBarConfig,
) -> (
    ratatui::layout::Rect,
    Option<ratatui::layout::Rect>,
    Option<ratatui::layout::Rect>,
) {
    let detail_h = detail_cfg.preferred_height(area.height);
    match (pm_visible, detail_visible) {
        (false, false) => (area, None, None),
        (false, true) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(detail_h)])
                .split(area);
            (chunks[0], Some(chunks[1]), None)
        }
        (true, false) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(area);
            (chunks[0], None, Some(chunks[1]))
        }
        (true, true) => {
            let pm_h = ((u32::from(area.height) * 33 / 100) as u16).max(6);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(detail_h),
                    Constraint::Length(pm_h),
                ])
                .split(area);
            (chunks[0], Some(chunks[1]), Some(chunks[2]))
        }
    }
}

/// Resolve the dashboard's user-tunable column widths from settings,
/// clamped to safe min/max. Unset or unparseable values fall back to the
/// defaults (28 / 16).
pub(super) fn read_column_widths(store: &Store) -> ColumnWidths {
    use crate::ui::dashboard::row::{ColumnWidths, DEFAULT_BRANCH_WIDTH, DEFAULT_PR_WIDTH};
    let branch = store
        .get_setting("dashboard_branch_width")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_BRANCH_WIDTH);
    let pr = store
        .get_setting("dashboard_pr_width")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_PR_WIDTH);
    ColumnWidths::clamped(branch, pr)
}

#[cfg(test)]
mod layout_indicator_cache_tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn app_refresh_populates_layout_indicator_cache_from_store() {
        use crate::data::store::{NewWorkspace, Store};
        use crate::ui::split::{SplitDirection, SplitTree};
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let a = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/tmp/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let ta = crate::ui::split::AttachTarget {
            workspace_id: a,
            instance: crate::data::store::AgentInstanceId(a.0),
        };
        let mut pair = SplitTree::Leaf(ta);
        pair.split(&[], SplitDirection::Vertical, ta);
        store.set_workspace_layout(a, &pair, &[1]).unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(
            app.workspaces_with_multi_pane_layouts.contains(&a),
            "cache should contain anchor with multi-pane layout"
        );
        // Replace with a single-pane layout — should drop from the cache after refresh.
        app.store
            .set_workspace_layout(a, &SplitTree::Leaf(ta), &[])
            .unwrap();
        app.refresh().unwrap();
        assert!(
            !app.workspaces_with_multi_pane_layouts.contains(&a),
            "single-pane layouts should not appear in the cache"
        );
    }
}

#[cfg(test)]
mod selection_anchoring_tests {
    //! Integration tests driving the real `draw` → `reconcile_selection`
    //! wiring through ratatui's `TestBackend`. These exercise the fold →
    //! park → restore cycle and the archive fallback that the pure
    //! `reconcile_selection` unit tests cannot reach (the behavior lives in
    //! the per-frame render wiring, not the pure function).
    use super::*;
    use crate::data::store::{NewWorkspace, RepoId, Store, WorkspaceId};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::{Path, PathBuf};

    fn app_with_two_workspaces() -> (App, RepoId, WorkspaceId, WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/tmp/r"), "r", "x").unwrap();
        let mk = |name: &str, branch: &str, wt: &str| {
            store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name,
                    branch,
                    worktree_path: Path::new(wt),
                    yolo: false,
                    agent: crate::pty::session::AgentKind::Claude,
                    shared: false,
                })
                .unwrap()
        };
        let a = mk("a", "x/a", "/tmp/r/a");
        let b = mk("b", "x/b", "/tmp/r/b");
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        (app, repo, a, b)
    }

    fn draw_once(app: &mut App) {
        let backend = TestBackend::new(120, 40);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| super::draw_for_test(f, app)).unwrap();
    }

    /// Select B, fold its repo so B's row vanishes from the list. Selection
    /// must stay anchored to B (parked) across frames rather than jumping to a
    /// neighbor, then restore — with the highlight index pointing back at B —
    /// when the repo is expanded again. This is the #168 regression guard.
    #[test]
    fn selection_parks_on_folded_workspace_and_restores() {
        let (mut app, repo, _a, b) = app_with_two_workspaces();
        let repo_key = repo.0 as u64;

        // Force the repo expanded so both workspace rows are selectable, then
        // select B.
        app.dashboard.folded.insert(repo_key, false);
        draw_once(&mut app);
        let idx = app
            .selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(b))
            .expect("B selectable while expanded");
        app.select_index(idx);
        draw_once(&mut app);
        assert_eq!(app.selected_target(), Some(SelectionTarget::Workspace(b)));

        // Fold the repo: B's row disappears. Selection must PARK on B.
        app.dashboard.folded.insert(repo_key, true);
        draw_once(&mut app);
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(b)),
            "selection parked on B while its row is hidden"
        );
        // Steady state: another frame must not drift the parked selection.
        draw_once(&mut app);
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(b)),
            "selection stays parked across frames"
        );

        // Expand again: B reappears, selection restored AND the nav cursor
        // resolves back to B's row.
        app.dashboard.folded.insert(repo_key, false);
        draw_once(&mut app);
        assert_eq!(app.selected_target(), Some(SelectionTarget::Workspace(b)));
        assert_eq!(
            app.selectable.get(app.dashboard.selected).copied(),
            Some(SelectionTarget::Workspace(b)),
            "highlight index restored to B"
        );
    }

    /// When the selected workspace is deleted (archive flow calls
    /// `delete_workspace` then `refresh`), selection must fall back to a live
    /// target and never keep pointing at the gone workspace.
    #[test]
    fn selection_falls_back_when_selected_workspace_archived() {
        let (mut app, repo, _a, b) = app_with_two_workspaces();
        let repo_key = repo.0 as u64;

        app.dashboard.folded.insert(repo_key, false);
        draw_once(&mut app);
        let idx = app
            .selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(b))
            .expect("B selectable while expanded");
        app.select_index(idx);
        draw_once(&mut app);
        assert_eq!(app.selected_target(), Some(SelectionTarget::Workspace(b)));

        // Delete B and refresh, exactly as the archive flow does.
        app.store.delete_workspace(b).unwrap();
        app.refresh().unwrap();
        app.dashboard.folded.insert(repo_key, false);
        draw_once(&mut app);

        let sel = app.selected_target();
        assert!(sel.is_some(), "selection falls back to a live target");
        assert_ne!(
            sel,
            Some(SelectionTarget::Workspace(b)),
            "selection no longer points at the deleted workspace"
        );
        assert!(
            app.selection_target_exists(sel.unwrap()),
            "fallback target actually exists"
        );
    }
}

#[cfg(test)]
mod build_row_inputs_tests {
    //! Exercises `build_row_inputs` directly rather than through a
    //! `TestBackend` frame — it's the extracted per-workspace loop body from
    //! `draw`, so calling it needs only an `App` and a `Workspace`, not a
    //! full render pass. `test_workspace` / `test_spawn_session` are the
    //! `pub(crate)` fixture helpers from `app::strip_instances_tests`.
    use super::*;
    use crate::pty::session::{AgentKind, SessionStatus};

    fn test_app() -> App {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap()
    }

    /// `build_row_inputs` takes `&Workspace`, not a `WorkspaceId` — look the
    /// row up from `app.workspaces` the same way `draw`'s loop does.
    fn row_inputs(
        app: &App,
        ws: crate::data::store::WorkspaceId,
    ) -> crate::ui::dashboard::row::RowInputs {
        let (_, workspace) = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == ws)
            .expect("workspace present after refresh");
        build_row_inputs(app, workspace, crate::util::time::now_ms(), true)
    }

    /// A workspace with no live PTY — detached, or every workspace right
    /// after a wsx restart — still has a real last-activity time recorded in
    /// its event log. The row must report it: `ago_secs` is the dashboard's
    /// recency sort key, so sourcing it from the session alone would drop
    /// every cold workspace into the never-active bucket and leave a blocked
    /// one unpinnable.
    #[test]
    fn row_age_comes_from_the_event_log_when_no_session_is_running() {
        let mut app = test_app();
        let ws = app.test_workspace("cold");
        app.store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap();
        app.refresh().unwrap();
        let now = crate::util::time::now_ms();
        app.workspace_events.insert(
            ws,
            crate::activity::events::WorkspaceEvents {
                latest: Some(crate::activity::events::EventSnapshot {
                    kind: crate::activity::events::EventKind::AssistantText,
                    display: "asked you something".into(),
                    timestamp_ms: now - 90_000,
                }),
                ..Default::default()
            },
        );

        let ago = row_inputs(&app, ws).ago_secs;
        assert!(
            matches!(ago, Some(secs) if (85..=95).contains(&secs)),
            "expected ~90s from the event log, got {ago:?}"
        );
    }

    /// The live session is the fresher signal whenever it has one — the event
    /// log lags it.
    #[test]
    fn row_age_prefers_the_session_over_an_older_event() {
        let mut app = test_app();
        let ws = app.test_workspace("live");
        let primary = app
            .store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap();
        app.refresh().unwrap();
        app.test_spawn_session(primary.id, SessionStatus::Running { pid: 1 });
        let now = crate::util::time::now_ms();
        if let Some(inst) = app.primary_instance(ws)
            && let Some(session) = app.sessions.get(inst)
        {
            session
                .activity_ms
                .store((now - 5_000) as u64, std::sync::atomic::Ordering::Relaxed);
        }
        app.workspace_events.insert(
            ws,
            crate::activity::events::WorkspaceEvents {
                latest: Some(crate::activity::events::EventSnapshot {
                    kind: crate::activity::events::EventKind::AssistantText,
                    display: "older".into(),
                    timestamp_ms: now - 600_000,
                }),
                ..Default::default()
            },
        );

        let ago = row_inputs(&app, ws).ago_secs;
        assert!(
            matches!(ago, Some(secs) if secs <= 10),
            "expected the ~5s session time to win, got {ago:?}"
        );
    }

    #[test]
    fn row_peers_exclude_the_primary_and_dead_agents() {
        let mut app = test_app();
        let ws = app.test_workspace("multi");
        let primary = app
            .store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap();
        let live_peer = app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let dead_peer = app.store.add_workspace_agent(ws, AgentKind::Pi).unwrap();
        app.refresh().unwrap();
        app.test_spawn_session(primary.id, SessionStatus::Running { pid: 1 });
        app.test_spawn_session(live_peer.id, SessionStatus::Running { pid: 2 });
        app.test_spawn_session(dead_peer.id, SessionStatus::Exited { code: 0 });

        let peers = row_inputs(&app, ws).peers;
        assert_eq!(
            peers,
            vec![AgentKind::Codex],
            "primary and dead peer excluded"
        );
    }

    #[test]
    fn row_peers_include_registered_peers_with_no_session_after_a_restart() {
        let mut app = test_app();
        let ws = app.test_workspace("restarted");
        app.store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap();
        app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        app.refresh().unwrap();
        // No sessions at all: this is the state right after a wsx restart,
        // where the previous process killed every PTY on quit but the roster
        // rows survive in the DB.
        let peers = row_inputs(&app, ws).peers;
        assert_eq!(
            peers,
            vec![AgentKind::Codex],
            "a registered peer with no session this run still gets a bar"
        );
    }

    #[test]
    fn row_peers_are_empty_when_the_workspace_was_never_attached() {
        let mut app = test_app();
        let ws = app.test_workspace("cold");
        app.store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap();
        app.refresh().unwrap();
        // No sessions spawned at all.
        assert!(row_inputs(&app, ws).peers.is_empty());
    }
}
