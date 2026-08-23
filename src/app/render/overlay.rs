//! Drawing whatever sits on top of the current view: the modal stack,
//! and the two pickers anchored to a widget rather than centered.

use super::*;
use crate::app::{ActivityState, App};

/// The active modal, if any.
pub(super) fn draw_modal(f: &mut ratatui::Frame, app: &mut App, area: ratatui::layout::Rect) {
    use crate::ui::modal;
    let Some(m) = &app.modal else {
        return;
    };
    match m {
        crate::ui::modal::Modal::UpdatesPanel {
            selected,
            sort,
            filter,
        } => {
            let now_ms = crate::util::time::now_ms();
            let awaiting = app.awaiting_permission_map();
            let activity_translated: std::collections::HashMap<
                crate::data::store::WorkspaceId,
                crate::ui::updates_bar::ActivityState,
            > = app
                .workspace_activity
                .iter()
                .map(|(k, v)| (*k, translate_activity(*v)))
                .collect();
            let statuses = app.classified_statuses();
            let inputs = crate::ui::modal::PanelInputs {
                repos: &app.repos,
                workspaces: &app.workspaces,
                events: &app.workspace_events,
                activity: &activity_translated,
                needs_attention: &app.workspace_needs_attention,
                awaiting: &awaiting,
                statuses: &statuses,
                lifecycles: &app.pr_lifecycle,
            };
            let view = crate::ui::modal::PanelView {
                selected: *selected,
                sort: *sort,
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
        other => modal::render(f, area, other, &app.in_flight, app.tick, &app.theme),
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
