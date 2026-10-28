//! Drawing a frame.
//!
//! [`draw`] clears the per-frame hit-test state every input handler depends
//! on, then delegates: one module per view, plus [`overlay`] for whatever
//! sits on top of it.
//!
//!   [`dashboard`]  the workspace list and its detail bar
//!   [`attached`]   PTY panes, local split tree or ssh-attached remote
//!   [`overlay`]    the modal stack and the two anchored pickers

pub(crate) mod attached;
pub(crate) mod dashboard;
pub(crate) mod overlay;

pub(crate) use dashboard::resolve_dashboard_detail_cfg;
pub(crate) use overlay::{panel_inputs, translate_activity};

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
    crate::config::settings::toggle_on(store, "nerd_fonts")
}

pub(super) fn notifications_enabled(store: &Store) -> bool {
    crate::config::settings::toggle_on(store, "notifications")
}

pub(super) fn compute_attention_items(
    app: &App,
    attached_id: Option<crate::data::store::WorkspaceId>,
    max_width: usize,
) -> Option<crate::ui::updates_bar::AttentionItems> {
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
                ago_secs: dashboard::workspace_age_secs(app, w.id, now_ms),
                status: app.classify_status(w),
                hidden: app.hidden_from_top_bar(w.id),
            }
        })
        .collect();
    let entries = crate::ui::updates_bar::collect_workspace_row(
        &candidates,
        attached_id,
        now_ms,
        app.dashboard.sort_mode,
        app.dashboard.blocked_pin_max_age_secs,
    );
    (!entries.is_empty()).then_some(crate::ui::updates_bar::AttentionItems {
        entries,
        now_ms,
        max_width,
    })
}

pub fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let area = f.area();
    app.frame_size = Some((area.width, area.height));
    // Clear chip state at the start of every frame; the attached view and the
    // dashboard detail branch overwrite these with live values when chips render.
    app.chip_rects.clear();
    app.tag_chip_rects.clear();
    app.tags_manager_rect = None;
    app.attention_rects.clear();
    app.attention_more_rect = None;
    app.pinned_commands_cache.clear();
    app.prompt_tags_cache.clear();
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
    app.host_chip_rects.clear();
    app.remote_hosts_cache.clear();
    app.usage_window_option_rects.clear();
    app.name_color_swatch_rects.clear();
    sync_session_visibility(app);
    // Activity / attention / bells are view-independent: keep them fresh
    // while attached too, or the top bar and bell only ever fire from the
    // dashboard.
    dashboard::refresh_activity(app);

    // Every arm binds `_`: each draw_* re-derives what it needs from
    // `app.view` itself, so the borrow taken here ends before they take a
    // mutable one.
    match &app.view {
        crate::ui::View::Dashboard => dashboard::draw_dashboard(f, app, area),
        crate::ui::View::Attached(_) => attached::draw_attached(f, app, area),
        crate::ui::View::AttachedRemote => attached::draw_attached_remote(f, app, area),
    }
    let owner = drawer_owner(app);
    let floor = match app.drawer_hold {
        Some((held, rows)) if Some(held) == owner => rows,
        _ => 0,
    };
    let ((), tallest) = crate::ui::drawer::hold_height(floor, || {
        overlay::draw_modal(f, app, area);
        overlay::draw_anchored_pickers(f, app, area);
        attached::draw_attached_nav_overlay(f, area, app);
    });
    app.drawer_hold = owner.map(|o| (o, tallest));
}

/// What has the bottom drawer open. A change of owner (or closing it) resets
/// the drawer's grow-only height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawerOwner {
    Modal(std::mem::Discriminant<crate::ui::modal::Modal>),
    LeaderMenu,
}

/// Drop the drawer's held height when what owns the drawer has changed since
/// it was last drawn. Called after every input event as well as each frame:
/// input coalesces between frames, so a modal closed and a same-kind one
/// reopened before the next draw would otherwise inherit the old height.
pub(crate) fn release_stale_drawer_hold(app: &mut App) {
    if app
        .drawer_hold
        .is_some_and(|(held, _)| Some(held) != drawer_owner(app))
    {
        app.drawer_hold = None;
    }
}

fn drawer_owner(app: &App) -> Option<DrawerOwner> {
    match &app.modal {
        Some(m) => Some(DrawerOwner::Modal(std::mem::discriminant(m))),
        None if app.leader_pending && matches!(app.view, crate::ui::View::Attached(_)) => {
            Some(DrawerOwner::LeaderMenu)
        }
        None => None,
    }
}

#[cfg(test)]
mod drawer_hold_tests {
    use super::*;
    use crate::ui::modal::Modal;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    /// Height of the drawer in a drawn frame: rows from its `──` title rule
    /// to the bottom edge.
    fn drawer_height(app: &mut App) -> u16 {
        let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        let top = (0..buf.area.height)
            .rev()
            .find(|&y| buf[(0, y)].symbol() == "─" && buf[(1, y)].symbol() == "─")
            .expect("a drawer is open");
        buf.area.height - top
    }

    fn error(lines: usize) -> Option<Modal> {
        Some(Modal::Error {
            message: vec!["line"; lines].join("\n"),
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn closing_and_reopening_between_frames_does_not_inherit_the_height() {
        use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = error(1);
        let short = drawer_height(&mut app);
        app.modal = error(6);
        let tall = drawer_height(&mut app);
        assert!(tall > short);

        // Esc closes the error; a same-kind modal opens before any redraw.
        let shared = std::sync::Arc::new(tokio::sync::Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        crate::app::input::handle_event(&mut app, &shared, Event::Key(esc))
            .await
            .unwrap();
        assert!(app.modal.is_none());
        app.modal = error(1);
        assert_eq!(drawer_height(&mut app), short, "sized to its own content");
    }

    #[test]
    fn an_open_drawer_grows_but_never_shrinks_and_resets_on_a_new_modal() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();

        app.modal = error(2);
        let short = drawer_height(&mut app);
        app.modal = error(6);
        let tall = drawer_height(&mut app);
        assert_eq!(tall, short + 4, "grows with its content");
        app.modal = error(1);
        assert_eq!(drawer_height(&mut app), tall, "never shrinks while open");

        app.modal = Some(Modal::ConfirmQuit {
            creates: 1,
            archives: 0,
        });
        assert!(
            drawer_height(&mut app) < tall,
            "a different modal sizes to its own content"
        );
        app.modal = None;
        drawer_height_closed(&mut app);
        app.modal = error(1);
        assert!(drawer_height(&mut app) < short, "closing resets the hold");
    }

    fn drawer_height_closed(app: &mut App) {
        let mut term = Terminal::new(TestBackend::new(80, 30)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        assert!(app.drawer_hold.is_none());
    }
}
