// render — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)

use crate::app::activity::classify_activity_with_events;
use crate::app::bell::{COLD_START_WINDOW, alert_decision};
use crate::app::{ActivityState, App, SelectionTarget};
use crate::config::detail_bar_config::DetailBarConfig;
use crate::data::store::Store;
use crate::ui::dashboard::row::ColumnWidths;
use ratatui::layout::{Constraint, Direction, Layout};

pub fn draw(f: &mut ratatui::Frame, app: &mut App) {
    use crate::ui::{attached, dashboard, modal};
    let area = f.area();
    // Clear chip state at the start of every frame; View::Attached and the
    // dashboard detail branch overwrite these with live values when chips render.
    app.chip_rects.clear();
    app.attention_rects.clear();
    app.pinned_commands_cache.clear();
    // Clear detail-bar container rects each frame; the workspace-selected
    // branch overwrites this with live values when the detail bar renders.
    // Prevents stale rects from triggering wheel events on invisible containers.
    app.detail_container_rects = [None; 4];
    app.attached_pane_rects.clear();
    match &app.view {
        crate::ui::View::Dashboard => {
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
            let mut workspaces: Vec<dashboard::WorkspaceItem<'_>> = Vec::new();
            for repo in &app.repos {
                for (rid, ws) in &app.workspaces {
                    if *rid != repo.id {
                        continue;
                    }
                    let status = app.classify_status(ws);
                    let session = app.sessions.get(ws.id);
                    let secs = session.as_ref().map(|s| {
                        let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                        if last == 0 {
                            return 0;
                        }
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        now.saturating_sub(last) / 1000
                    });
                    let latest = app
                        .workspace_events
                        .get(&ws.id)
                        .and_then(|e| e.latest.clone());
                    let setup_failed = ws.setup_status == crate::data::store::SetupStatus::Failed;
                    let row = crate::ui::dashboard::row::RowInputs {
                        status,
                        name: ws.name.clone(),
                        branch: ws.branch.clone(),
                        procs: app
                            .workspace_processes
                            .get(&ws.id)
                            .map(|v| v.len() as u32)
                            .unwrap_or(0),
                        diff: app.workspace_diff.get(&ws.id).copied(),
                        last_message: latest.map(|ev| ev.display),
                        ago_secs: secs,
                        selected: matches!(app.selected_target(),
                            Some(SelectionTarget::Workspace(id)) if id == ws.id),
                        yolo: ws.yolo,
                        setup_failed,
                        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                        nerd_fonts,
                        workspace_id: ws.id,
                        has_multi_pane_layout: app
                            .workspaces_with_multi_pane_layouts
                            .contains(&ws.id),
                    };
                    workspaces.push(dashboard::WorkspaceItem {
                        repo,
                        workspace_id: ws.id,
                        status,
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
                let session = app.sessions.get(ws.id);
                let running = session.as_ref().is_some_and(|s| {
                    matches!(
                        *s.status.read().unwrap(),
                        crate::pty::session::SessionStatus::Running { .. }
                    )
                });
                let secs = session.as_ref().map(|s| {
                    let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                    if last == 0 {
                        return 0;
                    }
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    now.saturating_sub(last) / 1000
                });
                let awaiting = app.awaiting_permission(ws.id).is_some();
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
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

            let activity: Vec<u32> = app.activity_history.iter().map(|(_h, m)| *m).collect();
            let column_widths = read_column_widths(&app.store);
            let inputs = dashboard::DashboardInputs {
                repos: app.repos.iter().collect(),
                workspaces,
                activity: &activity,
                column_widths,
            };
            // Rebuild `selectable` in the V5 visible order (noise-sort
            // across repos, priority-sort within repo, hide folded
            // workspaces, apply filter). Nav keys index into this Vec,
            // so it must match what the renderer emits below or the
            // selection will appear to skip rows / jump back.
            let new_selectable = dashboard::visible_targets(&inputs, &app.dashboard);
            if new_selectable != app.selectable {
                // Preserve the user's *target* across reorderings, not
                // their *index* — keep arrow nav anchored to the same
                // workspace even if the visible order shifts (e.g.
                // status change moves it up/down).
                let prev_target = app.selectable.get(app.dashboard.selected).copied();
                app.selectable = new_selectable;
                if let Some(t) = prev_target {
                    if let Some(idx) = app.selectable.iter().position(|s| *s == t) {
                        app.dashboard.selected = idx;
                    } else if !app.selectable.is_empty() {
                        app.dashboard.selected =
                            app.dashboard.selected.min(app.selectable.len() - 1);
                    } else {
                        app.dashboard.selected = 0;
                    }
                } else if !app.selectable.is_empty() {
                    app.dashboard.selected = app.dashboard.selected.min(app.selectable.len() - 1);
                }
            }
            app.dashboard.selection = app.selected_target();
            dashboard::render_without_footer(
                f,
                dashboard_area,
                &inputs,
                &mut app.dashboard,
                app.tick,
                &app.theme,
            );
            if let Some(pm_area) = pm_area {
                if let Some(session) = app.pm.as_ref() {
                    crate::ui::pm_pane::resize_session(session, pm_area);
                }
                crate::ui::pm_pane::render(f, pm_area, app.pm.as_ref(), app.focus, &app.theme);
            }
            if let (Some(detail_area), Some(SelectionTarget::Workspace(ws_id))) =
                (detail_area, app.selected_target())
            {
                if let Some((rid, ws)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
                    if let Some(repo) = app.repos.iter().find(|r| r.id == *rid) {
                        let session = app.sessions.get(ws.id);
                        // Activity timestamp: prefer whichever signal is more
                        // recent. `session.activity_ms` only exists for
                        // workspaces wsx is currently attached to. The JSONL
                        // event's own `timestamp_ms` (parsed from the line's
                        // `timestamp` field) is the actual event time — this
                        // is what we want, NOT `last_log_activity_ms`, which
                        // is the wall-clock time when wsx observed the log
                        // growing (gets stamped to "now" on the first tail
                        // pass after startup, so all workspaces would
                        // otherwise show the same age starting from zero).
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        let session_last_ms = session
                            .as_ref()
                            .map(|s| {
                                s.activity_ms.load(std::sync::atomic::Ordering::Relaxed) as i64
                            })
                            .unwrap_or(0);
                        let event_last_ms = app
                            .workspace_events
                            .get(&ws.id)
                            .and_then(|e| e.latest.as_ref().map(|ev| ev.timestamp_ms))
                            .unwrap_or(0);
                        let last_ms = session_last_ms.max(event_last_ms);
                        let ago_secs = if last_ms == 0 {
                            None
                        } else {
                            Some(((now_ms - last_ms).max(0) / 1000) as u64)
                        };
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
                            procs,
                            diff: app.workspace_diff.get(&ws.id).copied(),
                            diff_per_file: app.workspace_diff_per_file.get(&ws.id),
                            lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                            pr_title: None,
                            pr_number: None,
                            status,
                            ago_secs,
                            reply_draft: &app.dashboard.reply_draft,
                            reply_focused: matches!(
                                app.focus,
                                crate::ui::PaneFocus::DetailBarReply
                            ),
                            events_scanned: app.workspace_events_scanned.contains(&ws.id),
                            config: &detail_cfg,
                            registry: &app.registry,
                            pinned: &pinned,
                            scroll_offsets: &mut app.detail_scroll_offsets,
                        };
                        let out = crate::ui::dashboard::detail::render(
                            f,
                            detail_area,
                            &mut inputs,
                            &app.theme,
                        );
                        app.detail_container_rects = out.container_rects;
                        if !out.chip_rects.is_empty() {
                            app.chip_rects = out.chip_rects;
                            app.pinned_commands_cache = pinned;
                        }
                    }
                }
            }
            // Render footer below detail/PM so the spec order
            // list / detail / pm / footer is respected.
            dashboard::render_footer(f, footer_area, &activity, &app.theme);
        }
        crate::ui::View::Attached(state) => {
            // If any leaf's session has gone away (e.g. workspace was
            // archived from elsewhere), bounce back to dashboard. Matches
            // the previous single-pane fallback at handle_key_attached.
            if state
                .leaves()
                .iter()
                .any(|id| app.sessions.get(*id).is_none())
            {
                app.leader_pending = false;
                app.view = crate::ui::View::Dashboard;
                return;
            }
            let focused_id = match state.focused_id() {
                Some(id) => id,
                None => {
                    app.leader_pending = false;
                    app.view = crate::ui::View::Dashboard;
                    return;
                }
            };
            let focused_label = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == focused_id)
                .map(|(_, w)| {
                    let repo_name = app
                        .repos
                        .iter()
                        .find(|r| r.id == w.repo_id)
                        .map(|r| r.name.as_str())
                        .unwrap_or("");
                    if repo_name.is_empty() {
                        w.name.clone()
                    } else {
                        format!("{}/{}", repo_name, w.name)
                    }
                })
                .unwrap_or_default();

            // Conservative right margin for the status row; `render_panes`
            // renders the attention line flush at `status_area.x` with no
            // prefix, so each segment's `start_col` maps directly to a screen
            // column (load-bearing for attention-entry click hit-testing).
            let max_width = (area.width as usize).saturating_sub(3);
            let attention = if matches!(
                app.modal,
                Some(crate::ui::modal::Modal::UpdatesPanel { .. })
            ) {
                None
            } else {
                compute_attention_line(app, Some(focused_id), max_width)
            };

            // Pinned commands resolve against the FOCUSED pane's workspace.
            let global_pinned = app.store.get_setting("pinned_commands").ok().flatten();
            let repo_pinned = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == focused_id)
                .and_then(|(_, w)| {
                    app.repos
                        .iter()
                        .find(|r| r.id == w.repo_id)
                        .and_then(|r| r.pinned_commands.clone())
                });
            let pinned =
                crate::commands::pinned::resolve(global_pinned.as_deref(), repo_pinned.as_deref());

            let (pane_area, chip_area, status_area, footer_area) =
                attached::layout_chrome(area, attention.is_some(), !pinned.is_empty());
            let attention_rects: Vec<(
                crate::data::store::WorkspaceId,
                ratatui::layout::Rect,
            )> = attention
                .as_ref()
                .map(|a| {
                    a.segments
                        .iter()
                        .map(|s| {
                            (
                                s.workspace_id,
                                ratatui::layout::Rect {
                                    x: status_area.x.saturating_add(s.start_col),
                                    y: status_area.y,
                                    width: s.width,
                                    height: 1,
                                },
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let attention_line = attention.map(|a| a.line);
            let crate::ui::split::LayoutResult { panes, dividers } = state.layout(pane_area);
            let multi_pane = panes.len() > 1;

            // Resize each session's PTY to its pane area (minus title row when multi-pane).
            for (ws_id, _path, rect) in &panes {
                if let Some(session) = app.sessions.get(*ws_id) {
                    attached::resize_pane(&session, *rect, multi_pane);
                }
            }

            // Build PaneSpec list. Use owned sessions + labels to keep
            // them alive while rendering.
            let pane_data: Vec<(
                std::sync::Arc<crate::pty::session::Session>,
                String,
                ratatui::layout::Rect,
                bool,
            )> = panes
                .into_iter()
                .filter_map(|(ws_id, path, rect)| {
                    let session = app.sessions.get(ws_id)?;
                    let label = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == ws_id)
                        .map(|(_, w)| w.name.clone())
                        .unwrap_or_default();
                    let focused = path == state.focus;
                    Some((session, label, rect, focused))
                })
                .collect();
            let specs: Vec<crate::ui::attached::PaneSpec<'_>> = pane_data
                .iter()
                .map(|(s, l, r, f)| crate::ui::attached::PaneSpec {
                    session: s,
                    label: l.as_str(),
                    rect: *r,
                    focused: *f,
                })
                .collect();

            let out = attached::render_panes(
                f,
                &specs,
                &dividers,
                chip_area,
                status_area,
                footer_area,
                &focused_label,
                multi_pane,
                attention_line,
                &pinned,
                &app.theme,
            );
            app.chip_rects = out.chip_rects;
            app.attention_rects = attention_rects;
            app.attached_pane_rects = out.pane_rects;
            app.pinned_commands_cache = pinned;
        }
        crate::ui::View::AttachedPm => {
            if let Some(session) = app.pm.as_ref() {
                let max_width = (area.width as usize).saturating_sub(3);
                let attention = if matches!(
                    app.modal,
                    Some(crate::ui::modal::Modal::UpdatesPanel { .. })
                ) {
                    None
                } else {
                    compute_attention_line(app, None, max_width)
                };
                // PM pane is out of scope for pinned commands per spec.
                let pinned: &[crate::commands::pinned::PinnedCommand] = &[];
                let (pane_area, chip_area, status_area, footer_area) =
                    attached::layout_chrome(area, attention.is_some(), false);
                let attention_rects: Vec<(
                    crate::data::store::WorkspaceId,
                    ratatui::layout::Rect,
                )> = attention
                    .as_ref()
                    .map(|a| {
                        a.segments
                            .iter()
                            .map(|s| {
                                (
                                    s.workspace_id,
                                    ratatui::layout::Rect {
                                        x: status_area.x.saturating_add(s.start_col),
                                        y: status_area.y,
                                        width: s.width,
                                        height: 1,
                                    },
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let attention_line = attention.map(|a| a.line);
                attached::resize_pane(session, pane_area, false);
                let specs = [crate::ui::attached::PaneSpec {
                    session,
                    label: "project-manager",
                    rect: pane_area,
                    focused: true,
                }];
                let out = attached::render_panes(
                    f,
                    &specs,
                    &[],
                    chip_area,
                    status_area,
                    footer_area,
                    "project-manager",
                    false,
                    attention_line,
                    pinned,
                    &app.theme,
                );
                app.attached_pane_rects = out.pane_rects;
                app.attention_rects = attention_rects;
            } else {
                // PM session went away; bounce to dashboard on next event.
                app.leader_pending = false;
                app.view = crate::ui::View::Dashboard;
            }
        }
    }
    if let Some(m) = &app.modal {
        match m {
            crate::ui::modal::Modal::UpdatesPanel { selected } => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let mut awaiting: std::collections::HashMap<
                    crate::data::store::WorkspaceId,
                    (String, i64),
                > = std::collections::HashMap::new();
                for (_rid, w) in &app.workspaces {
                    if let Some(a) = app.awaiting_permission(w.id) {
                        awaiting.insert(w.id, a);
                    }
                }
                let activity_translated: std::collections::HashMap<
                    crate::data::store::WorkspaceId,
                    crate::ui::updates_bar::ActivityState,
                > = app
                    .workspace_activity
                    .iter()
                    .map(|(k, v)| (*k, translate_activity(*v)))
                    .collect();
                let statuses: std::collections::HashMap<
                    crate::data::store::WorkspaceId,
                    crate::ui::dashboard::status::Status,
                > = app
                    .workspaces
                    .iter()
                    .map(|(_, w)| (w.id, app.classify_status(w)))
                    .collect();
                crate::ui::modal::render_updates_panel(
                    f,
                    area,
                    &app.repos,
                    &app.workspaces,
                    &app.workspace_events,
                    &activity_translated,
                    &app.workspace_needs_attention,
                    &awaiting,
                    &statuses,
                    &app.pr_lifecycle,
                    *selected,
                    now_ms,
                    &app.theme,
                );
            }
            crate::ui::modal::Modal::ProcessList {
                workspace_id,
                selected,
            } => {
                let workspace_name = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == *workspace_id)
                    .map(|(_, w)| w.name.clone())
                    .unwrap_or_default();
                let procs = app
                    .workspace_processes
                    .get(workspace_id)
                    .cloned()
                    .unwrap_or_default();
                crate::ui::modal::render_process_list(
                    f,
                    area,
                    &workspace_name,
                    &procs,
                    *selected,
                    &app.theme,
                );
            }
            crate::ui::modal::Modal::RepoSettings { repo_id, selected } => {
                if let Some(repo) = app.repos.iter().find(|r| r.id == *repo_id) {
                    let repo_name = repo.name.clone();
                    crate::ui::modal::render_repo_settings(
                        f, area, &repo_name, repo, *selected, &app.theme,
                    );
                }
            }
            other => modal::render(f, area, other, app.tick, &app.theme),
        }
    }
}

#[doc(hidden)]
pub fn draw_for_test(f: &mut ratatui::Frame, app: &mut App) {
    draw(f, app);
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
fn dashboard_regions(
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

fn nerd_fonts_enabled(store: &Store) -> bool {
    match store.get_setting("nerd_fonts").ok().flatten().as_deref() {
        Some("false") | Some("0") | Some("off") | Some("no") => false,
        _ => true, // default ON
    }
}

pub(crate) fn pm_enabled(store: &Store) -> bool {
    match store.get_setting("pm_enabled").ok().flatten() {
        None => true,
        Some(v) => !matches!(
            v.trim().to_lowercase().as_str(),
            "false" | "0" | "off" | "no"
        ),
    }
}

fn notifications_enabled(store: &Store) -> bool {
    match store.get_setting("notifications").ok().flatten().as_deref() {
        Some("off") | Some("false") | Some("0") | Some("no") => false,
        _ => true, // default ON
    }
}

/// Resolve the dashboard's user-tunable column widths from settings,
/// clamped to safe min/max. Unset or unparseable values fall back to the
/// V5 defaults (24 / 28).
fn read_column_widths(store: &Store) -> ColumnWidths {
    use crate::ui::dashboard::row::{ColumnWidths, DEFAULT_BRANCH_WIDTH, DEFAULT_NAME_WIDTH};
    let name = store
        .get_setting("dashboard_name_width")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_NAME_WIDTH);
    let branch = store
        .get_setting("dashboard_branch_width")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_BRANCH_WIDTH);
    ColumnWidths::clamped(name, branch)
}

fn compute_attention_line(
    app: &App,
    attached_id: Option<crate::data::store::WorkspaceId>,
    max_width: usize,
) -> Option<crate::ui::updates_bar::AttentionLine> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let candidates: Vec<crate::ui::updates_bar::WorkspaceUpdateInfo> = app
        .workspaces
        .iter()
        .map(|(rid, w)| {
            let activity = app
                .workspace_activity
                .get(&w.id)
                .copied()
                .map(translate_activity)
                .unwrap_or(crate::ui::updates_bar::ActivityState::Off);
            let repo_name = app
                .repos
                .iter()
                .find(|r| r.id == *rid)
                .map(|r| r.name.as_str())
                .unwrap_or("");
            crate::ui::updates_bar::WorkspaceUpdateInfo {
                id: w.id,
                name: w.name.as_str(),
                repo_name,
                events: app.workspace_events.get(&w.id),
                activity,
                needs_attention: app.workspace_needs_attention.contains(&w.id),
                awaiting_tool: app.awaiting_permission(w.id),
            }
        })
        .collect();
    let entries = crate::ui::updates_bar::collect_attention(&candidates, attached_id, now_ms);
    crate::ui::updates_bar::format_attention_line_styled(&entries, now_ms, max_width, &app.theme)
}

pub(crate) fn translate_activity(a: ActivityState) -> crate::ui::updates_bar::ActivityState {
    use crate::ui::updates_bar::ActivityState as U;
    match a {
        ActivityState::AwaitingAnswer => U::AwaitingAnswer,
        ActivityState::Complete => U::Complete,
        ActivityState::Awaiting => U::Awaiting,
        ActivityState::Active => U::Active,
        ActivityState::Idle => U::Idle,
        ActivityState::Stalled => U::Stalled,
        ActivityState::Waiting => U::Waiting,
        ActivityState::Off => U::Off,
    }
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
            })
            .unwrap();
        let mut pair = SplitTree::Leaf(a);
        pair.split(&[], SplitDirection::Vertical, a);
        store.set_workspace_layout(a, &pair, &[1]).unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(
            app.workspaces_with_multi_pane_layouts.contains(&a),
            "cache should contain anchor with multi-pane layout"
        );
        // Replace with a single-pane layout — should drop from the cache after refresh.
        app.store
            .set_workspace_layout(a, &SplitTree::Leaf(a), &[])
            .unwrap();
        app.refresh().unwrap();
        assert!(
            !app.workspaces_with_multi_pane_layouts.contains(&a),
            "single-pane layouts should not appear in the cache"
        );
    }
}
