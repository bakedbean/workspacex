//! Drawing whatever sits on top of the current view: the modal stack,
//! and the two pickers anchored to a widget rather than centered.

use super::*;
use crate::app::{ActivityState, App};

/// The active modal, if any.
pub(super) fn draw_modal(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    use crate::ui::modal;
    // The setup-log viewer is handled before the borrow of `app.modal` below,
    // because its renderer clamps `scroll` against the body height it just
    // laid out and we store that clamp back. Without it, holding Up past the
    // top of a short log would run the counter away and the first several
    // Downs would look like no-ops.
    if matches!(app.modal, Some(modal::Modal::SetupLog { .. })) {
        draw_setup_log(f, app, area);
        return;
    }
    let Some(m) = &app.modal else {
        return;
    };
    match m {
        crate::ui::modal::Modal::UpdatesPanel { selected, filter } => {
            let now_ms = crate::util::time::now_ms();
            let inputs = panel_inputs(app, now_ms);
            let view = crate::ui::modal::PanelView {
                selected: *selected,
                filter: filter.as_deref(),
            };
            crate::ui::modal::render_updates_panel(f, area, &inputs, &view, now_ms, &app.theme);
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
        crate::ui::modal::Modal::RemoteWorkspaceList { selected, notice } => {
            if let Some(list) = &app.remote_list {
                crate::ui::modal::render_remote_workspace_list(
                    f,
                    area,
                    list,
                    *selected,
                    notice.as_deref(),
                    &app.theme,
                    nerd_fonts_enabled(&app.store),
                );
            }
        }
        crate::ui::modal::Modal::RepoSettings { repo_id, selected } => {
            if let Some(repo) = app.repos.iter().find(|r| r.id == *repo_id) {
                crate::ui::modal::render_repo_settings(
                    f, area, &app.store, repo, *selected, &app.theme,
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
        crate::ui::modal::Modal::PromptTag(modal) => {
            let tags = crate::commands::tags::load(&app.store).unwrap_or_default();
            crate::ui::modal::render_prompt_tag(f, area, modal, &tags, &app.theme);
        }
        other => modal::render(f, area, other, &app.theme),
    }
}

/// Draw `Modal::SetupLog` and write the renderer's clamped scroll back onto
/// the modal. Split out so the immutable borrow of `app` ends before that
/// write.
fn draw_setup_log(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    let clamped = {
        let Some(crate::ui::modal::Modal::SetupLog {
            workspace_id,
            stored,
            scroll,
        }) = &app.modal
        else {
            return;
        };
        let label = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == *workspace_id)
            .map(|(repo_id, w)| {
                let repo = app
                    .repos
                    .iter()
                    .find(|r| r.id == *repo_id)
                    .map(|r| r.name.as_str())
                    .unwrap_or("?");
                format!("{repo}/{}", w.name)
            })
            .unwrap_or_default();
        let view = crate::ui::modal::SetupLogView {
            label: &label,
            live: app.in_flight.get(workspace_id),
            stored: stored.as_deref(),
            scroll: *scroll,
            tick: app.tick,
        };
        crate::ui::modal::render_setup_log(f, area, &view, &app.theme)
    };
    if let Some(crate::ui::modal::Modal::SetupLog { scroll, .. }) = &mut app.modal {
        *scroll = clamped;
    }
}

/// The usage-window and name-color pickers.
///
/// Both anchor to a widget rather than centering, and both return click
/// hit-test rects, so they are drawn here instead of through `draw_modal`.
pub(super) fn draw_anchored_pickers(
    f: &mut ratatui::Frame,
    app: &mut App,
    area: ratatui::layout::Rect,
) {
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
    // Same reason as the usage picker: the name-color picker returns its swatch
    // rects for click hit-testing, so it is drawn here rather than through the
    // generic modal dispatch. State is copied out first to end the borrow.
    let picker = match &app.modal {
        Some(crate::ui::modal::Modal::NameColorPicker {
            current,
            selected,
            filter,
            ..
        }) => Some((*current, *selected, filter.clone())),
        _ => None,
    };
    if let Some((current, selected, filter)) = picker {
        app.name_color_swatch_rects = crate::ui::modal::render_name_color_picker(
            f, area, &filter, selected, current, &app.theme,
        );
    }
}

/// Gather the workspace-updates panel's inputs from live app state. The
/// renderer and the panel's key handler both go through here, so the
/// ordering the rows are drawn in is the ordering `selected` indexes.
pub(crate) fn panel_inputs(app: &App, now_ms: i64) -> crate::ui::modal::PanelInputs<'_> {
    let nerd_fonts = nerd_fonts_enabled(&app.store);
    crate::ui::modal::PanelInputs {
        repos: app.repos.iter().collect(),
        items: super::dashboard::build_workspace_items(app, &app.repos, now_ms, nerd_fonts),
        workspaces: &app.workspaces,
        events: &app.workspace_events,
        activity: app
            .workspace_activity
            .iter()
            .map(|(k, v)| (*k, translate_activity(*v)))
            .collect(),
        needs_attention: &app.workspace_needs_attention,
        awaiting: app.awaiting_permission_map(),
        group_mode: app.dashboard.group_mode,
        sort_mode: app.dashboard.sort_mode,
        blocked_pin_max_age_secs: app.dashboard.blocked_pin_max_age_secs,
        pr_width: super::dashboard::read_column_widths(&app.store).pr,
    }
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
