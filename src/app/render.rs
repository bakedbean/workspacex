// render — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)

use crate::app::activity::classify_activity_with_events;
use crate::app::bell::{COLD_START_WINDOW, alert_decision};
use crate::app::{ActivityState, App, SelectionTarget};
use crate::config::detail_bar_config::DetailBarConfig;
use crate::data::store::Store;
use crate::ui::dashboard::row::ColumnWidths;
use ratatui::layout::{Constraint, Direction, Layout};

/// One attached pane's render inputs: session, label, rect, focus flag,
/// and the workspace's coding agent (`None` for the project-manager pane).
type PaneData = (
    std::sync::Arc<crate::pty::session::Session>,
    String,
    ratatui::layout::Rect,
    bool,
    Option<crate::pty::session::AgentKind>,
);

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
    app.agent_chip_rects.clear();
    app.pr_link_rect = None;
    app.usage_graph_rect = None;
    app.footer_hint_rects.clear();
    app.usage_window_option_rects.clear();

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
            let now_ms = crate::time::now_ms();
            let mut workspaces: Vec<dashboard::WorkspaceItem<'_>> = Vec::new();
            for repo in &app.repos {
                for (rid, ws) in &app.workspaces {
                    if *rid != repo.id {
                        continue;
                    }
                    let status = app.classify_status(ws);
                    let session = app
                        .primary_instance(ws.id)
                        .and_then(|i| app.sessions.get(i));
                    let secs = session.as_ref().map(|s| {
                        let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                        if last == 0 {
                            return 0;
                        }
                        let now = now_ms.max(0) as u64;
                        now.saturating_sub(last) / 1000
                    });
                    let setup_failed = ws.setup_status == crate::data::store::SetupStatus::Failed;
                    let row = crate::ui::dashboard::row::RowInputs {
                        agent: ws.agent,
                        status,
                        name: ws.name.clone(),
                        branch: ws.branch.clone(),
                        procs: app
                            .workspace_processes
                            .get(&ws.id)
                            .map(|v| v.len() as u32)
                            .unwrap_or(0),
                        diff: app.workspace_diff.get(&ws.id).copied(),
                        column: crate::ui::dashboard::column_content::row_column(
                            status,
                            app.workspace_events.get(&ws.id),
                            now_ms,
                            app.fresh_reported_status(ws.id)
                                .and_then(|r| r.message.as_deref()),
                        ),
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
                let now_ms = crate::time::now_ms();
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
            let now_secs = crate::time::now_secs();
            let now_hour = now_secs - (now_secs % 3600);
            // VecDeque is non-contiguous; collect into a slice-able Vec so
            // aggregate_buckets can take it as `&[(u64, u32)]`.
            let history: Vec<(u64, u32)> = app.activity_history.iter().copied().collect();
            let activity: Vec<u32> = crate::ui::dashboard::sparkline::aggregate_buckets(
                &history,
                now_hour,
                window.hours(),
                24,
            );
            let column_widths = read_column_widths(&app.store);
            let inputs = dashboard::DashboardInputs {
                repos: app.repos.iter().collect(),
                workspaces,
                activity: &activity,
                column_widths,
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
            let (selection, selected) = dashboard::reconcile_selection(
                prev_selection,
                prev_selected,
                &new_selectable,
                |t| app.selection_target_exists(t),
            );
            app.selectable = new_selectable;
            app.dashboard.selection = selection;
            app.dashboard.selected = selected;
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
                        let session = app
                            .primary_instance(ws.id)
                            .and_then(|i| app.sessions.get(i));
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
                        let now_ms = crate::time::now_ms();
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
                            pr_number: app.pr_number.get(&ws.id).copied(),
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
        crate::ui::View::Attached(state) => {
            // If any leaf's session has gone away (e.g. workspace was
            // archived from elsewhere), bounce back to dashboard. Matches
            // the previous single-pane fallback at handle_key_attached.
            if state
                .leaves()
                .iter()
                .any(|t| app.sessions.get(t.instance).is_none())
            {
                app.leader_pending = false;
                app.view = crate::ui::View::Dashboard;
                return;
            }
            let focused_target = match state.focused_target() {
                Some(t) => t,
                None => {
                    app.leader_pending = false;
                    app.view = crate::ui::View::Dashboard;
                    return;
                }
            };
            let focused_id = focused_target.workspace_id;
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
            let focused_agent = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == focused_id)
                .map(|(_, w)| w.agent);

            // The attention items follow the bottom line's label prefix, so
            // shrink their width budget by the prefix and offset their click
            // rects by it too — `info_line_prefix_width` is the single source
            // of truth shared with the renderer.
            let prefix_w = attached::info_line_prefix_width(&focused_label, focused_agent) as usize;
            let max_width = (area.width as usize).saturating_sub(3 + prefix_w);
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

            // PR chip for the focused pane's workspace, drawn right-justified on
            // the chip row. Same `(lifecycle, number)` source the dashboard
            // detail header uses, so the chip text and click behaviour match.
            let pr = app
                .pr_number
                .get(&focused_id)
                .copied()
                .and_then(|n| app.pr_lifecycle.get(&focused_id).copied().map(|lc| (lc, n)));

            // Diff stats for the focused pane, drawn just left of the PR chip.
            // Same `app.workspace_diff` cache the dashboard `+N −N` cell reads,
            // so the chip-row count matches the dashboard and refreshes on the
            // same 10s diff poll as the agent makes commits.
            let diff = app.workspace_diff.get(&focused_id).copied();

            // Running-process count for the focused workspace, drawn leftmost in
            // the chip row's flush-right block. Same `app.workspace_processes`
            // map the dashboard row/detail bar count, so the chip-row `● Np`
            // matches them and refreshes on the same process-rescan tick.
            let procs = app
                .workspace_processes
                .get(&focused_id)
                .map(|v| v.len() as u32)
                .unwrap_or(0);

            // Build agents list for the footer agents row. Only shown when
            // the focused workspace has more than its primary agent.
            let focused_agents_list: Vec<(
                crate::data::store::AgentInstanceId,
                crate::pty::session::AgentKind,
                String,
                Option<char>,
            )> = {
                let instances = app.store.workspace_agents(focused_id).unwrap_or_default();
                if instances.len() > 1 {
                    // Keys cap at 10 (see `agent_switch_keys`); agents past the
                    // pool get `None` so they still render and stay clickable
                    // rather than being silently dropped by a `zip`.
                    let keys = attached::agent_switch_keys(instances.len());
                    instances
                        .into_iter()
                        .enumerate()
                        .map(|(i, inst)| (inst.id, inst.agent, inst.label(), keys.get(i).copied()))
                        .collect()
                } else {
                    Vec::new()
                }
            };
            let agents_present = !focused_agents_list.is_empty();

            let (info_area, separator_area, pane_area, chip_area, agents_area) =
                attached::layout_chrome(area, agents_present);
            let attention_rects: Vec<(crate::data::store::WorkspaceId, ratatui::layout::Rect)> =
                attention
                    .as_ref()
                    .map(|a| {
                        a.segments
                            .iter()
                            .map(|s| {
                                (
                                    s.workspace_id,
                                    ratatui::layout::Rect {
                                        x: info_area
                                            .x
                                            .saturating_add(prefix_w as u16)
                                            .saturating_add(s.start_col),
                                        y: info_area.y,
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

            // The agent instance in the focused pane is the "active" one; the
            // footer agents row thickens its identity bar so it's clear which
            // attached agent you're currently driving.
            let active_agent = panes
                .iter()
                .find(|(_, path, _)| *path == state.focus)
                .map(|(target, _, _)| target.instance);

            // Resize each session's PTY to its pane area (minus title row when multi-pane).
            for (target, _path, rect) in &panes {
                if let Some(session) = app.sessions.get(target.instance) {
                    attached::resize_pane(&session, *rect, multi_pane);
                }
            }

            // Build PaneSpec list. Use owned sessions + labels to keep
            // them alive while rendering. The leaf carries the agent instance
            // directly; resolve the session from it and the label/agent kind
            // from the instance (falling back to the workspace name + agent).
            let pane_data: Vec<PaneData> = panes
                .into_iter()
                .filter_map(|(target, path, rect)| {
                    let session = app.session_for(target.instance)?;
                    let instance = app
                        .store
                        .workspace_agents_by_id(target.instance)
                        .ok()
                        .flatten();
                    let (label, agent) = match instance {
                        Some(inst) => (inst.label(), Some(inst.agent)),
                        None => app
                            .workspaces
                            .iter()
                            .find(|(_, w)| w.id == target.workspace_id)
                            .map(|(_, w)| (w.name.clone(), Some(w.agent)))
                            .unwrap_or_default(),
                    };
                    let focused = path == state.focus;
                    Some((session, label, rect, focused, agent))
                })
                .collect();
            let specs: Vec<crate::ui::attached::PaneSpec<'_>> = pane_data
                .iter()
                .map(|(s, l, r, f, a)| crate::ui::attached::PaneSpec {
                    session: s,
                    label: l.as_str(),
                    rect: *r,
                    focused: *f,
                    agent: *a,
                })
                .collect();

            let out = attached::render_panes(
                f,
                &specs,
                &dividers,
                info_area,
                separator_area,
                chip_area,
                agents_area,
                &focused_label,
                focused_agent,
                attention_line,
                &pinned,
                procs,
                diff,
                pr,
                &focused_agents_list,
                active_agent,
                &app.theme,
            );
            app.chip_rects = out.chip_rects;
            app.pr_link_rect = out.pr_link_rect.map(|r| (focused_id, r));
            app.attention_rects = attention_rects;
            app.attached_pane_rects = out.pane_rects;
            app.agent_chip_rects = out.agent_chip_rects;
            app.footer_hint_rects = out.footer_hint_rects;
            app.pinned_commands_cache = pinned;
        }
        crate::ui::View::AttachedPm => {
            if let Some(session) = app.pm.as_ref() {
                let prefix_w = attached::info_line_prefix_width("project-manager", None) as usize;
                let max_width = (area.width as usize).saturating_sub(3 + prefix_w);
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
                let (info_area, separator_area, pane_area, chip_area, agents_area) =
                    attached::layout_chrome(area, false);
                let attention_rects: Vec<(crate::data::store::WorkspaceId, ratatui::layout::Rect)> =
                    attention
                        .as_ref()
                        .map(|a| {
                            a.segments
                                .iter()
                                .map(|s| {
                                    (
                                        s.workspace_id,
                                        ratatui::layout::Rect {
                                            x: info_area
                                                .x
                                                .saturating_add(prefix_w as u16)
                                                .saturating_add(s.start_col),
                                            y: info_area.y,
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
                    agent: None,
                }];
                let out = attached::render_panes(
                    f,
                    &specs,
                    &[],
                    info_area,
                    separator_area,
                    chip_area,
                    agents_area,
                    "project-manager",
                    None,
                    attention_line,
                    pinned,
                    0,
                    None,
                    None,
                    &[],
                    None,
                    &app.theme,
                );
                app.attached_pane_rects = out.pane_rects;
                app.attention_rects = attention_rects;
                app.footer_hint_rects = out.footer_hint_rects;
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
                let now_ms = crate::time::now_ms();
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
                input,
                notice,
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
                    input.as_deref(),
                    notice.as_deref(),
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
            crate::ui::modal::Modal::AgentsPanel {
                workspace_id,
                selected,
            } => {
                let agents = app
                    .store
                    .workspace_agents(*workspace_id)
                    .unwrap_or_default();
                crate::ui::modal::render_agents_panel(f, area, &agents, *selected, &app.theme);
            }
            crate::ui::modal::Modal::UsageWindowPicker { .. } => {
                // Rendered separately below, anchored to the footer graph.
            }
            other => modal::render(f, area, other, app.tick, &app.theme),
        }
    }
    // The usage-window picker renders anchored over the footer graph rather
    // than centered, so it is handled outside the generic modal dispatch. We
    // copy `selected` out first so the immutable borrow on `app.modal` ends
    // before we assign the returned option rects back to `app`.
    let picker_selected = match &app.modal {
        Some(crate::ui::modal::Modal::UsageWindowPicker { selected }) => Some(*selected),
        _ => None,
    };
    if let Some(selected) = picker_selected {
        let current = crate::config::usage_window::resolve(&app.store);
        let graph_rect = app.usage_graph_rect;
        let rects = crate::ui::modal::render_usage_window_picker(
            f, area, selected, current, graph_rect, &app.theme,
        );
        app.usage_window_option_rects = rects;
    }
    draw_attached_nav_overlay(f, area, app);
}

/// Render the Ctrl-x navigation overlay when the leader is armed in an
/// attached view. Keyed off `leader_pending`, so letter accelerators and the
/// overlay share one state. Context (multi-pane vs PM) selects the item list.
fn draw_attached_nav_overlay(f: &mut ratatui::Frame, area: ratatui::layout::Rect, app: &App) {
    if !app.leader_pending {
        return;
    }
    let (items, pinned_hint) = match &app.view {
        crate::ui::View::Attached(state) => (
            crate::ui::attached::nav_menu_items(state.leaf_count() > 1),
            !app.pinned_commands_cache.is_empty(),
        ),
        crate::ui::View::AttachedPm => (crate::ui::attached::pm_nav_menu_items(), false),
        _ => return,
    };
    crate::ui::attached::render_nav_overlay(
        f,
        area,
        &items,
        app.leader_selected,
        pinned_hint,
        &app.theme,
    );
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
    let now_ms = crate::time::now_ms();
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
                lifecycle: app.pr_lifecycle.get(&w.id).copied(),
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
