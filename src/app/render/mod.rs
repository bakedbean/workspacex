//! Drawing a frame.
//!
//! [`draw`] clears the per-frame hit-test state every input handler depends
//! on, then delegates: one module per view, plus [`overlay`] for whatever
//! sits on top of it.
//!
//!   [`dashboard`]  the workspace list and its detail bar
//!   [`attached`]   PTY panes, local split tree or ssh-attached remote
//!   [`overlay`]    the modal stack and the two anchored pickers

pub mod attached;
pub mod dashboard;
pub mod overlay;

pub(crate) use dashboard::resolve_dashboard_detail_cfg;
pub(crate) use overlay::translate_activity;

// render — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)

use crate::app::App;
use crate::data::store::Store;

/// Tell each session whether it is on screen, so backgrounded agents stop
/// signalling the render loop for output no frame can show.
///
/// Reuses `resize_sync::visible_instances` rather than deriving a second notion
/// of visibility — that helper already answers "which instances does the
/// current view display" and is tested.
pub(super) fn sync_session_visibility(app: &App) {
    let visible = crate::app::resize_sync::visible_instances(&app.view);
    app.sessions.sync_visibility(&visible);
}

#[doc(hidden)]
pub fn draw_for_test(f: &mut ratatui::Frame, app: &mut App) {
    draw(f, app);
}

pub(super) fn nerd_fonts_enabled(store: &Store) -> bool {
    match store.get_setting("nerd_fonts").ok().flatten().as_deref() {
        Some("false") | Some("0") | Some("off") | Some("no") => false,
        _ => true, // default ON
    }
}

pub(super) fn notifications_enabled(store: &Store) -> bool {
    match store.get_setting("notifications").ok().flatten().as_deref() {
        Some("off") | Some("false") | Some("0") | Some("no") => false,
        _ => true, // default ON
    }
}

pub(super) fn compute_attention_line(
    app: &App,
    attached_id: Option<crate::data::store::WorkspaceId>,
    max_width: usize,
) -> Option<crate::ui::updates_bar::AttentionLine> {
    let now_ms = crate::util::time::now_ms();
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

pub fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    // Clear chip state at the start of every frame; the attached view and the
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
    app.dashboard_pr_rects.clear();
    app.dashboard_repo_pr_rects.clear();
    app.procs_link_rect = None;
    app.usage_graph_rect = None;
    app.footer_hint_rects.clear();
    app.usage_window_option_rects.clear();
    app.name_color_swatch_rects.clear();
    sync_session_visibility(app);

    // Every arm binds `_`: each draw_* re-derives what it needs from
    // `app.view` itself, so the borrow taken here ends before they take a
    // mutable one.
    match &app.view {
        crate::ui::View::Dashboard => dashboard::draw_dashboard(f, app, area),
        crate::ui::View::Attached(_) => attached::draw_attached(f, app, area),
        crate::ui::View::AttachedRemote => attached::draw_attached_remote(f, app, area),
    }
    overlay::draw_modal(f, app, area);
    overlay::draw_anchored_pickers(f, app, area);
    attached::draw_attached_nav_overlay(f, area, app);
}
