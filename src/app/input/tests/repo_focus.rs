//! The repo focus view: entering and leaving it, moving between repos,
//! moving through its rows, and clicking rows and repo names.

use super::common::*;
use super::*;
use crate::data::store::{RepoId, Store, WorkspaceId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

/// Five workspaces in "repo", plus an empty second repo "other".
fn app() -> (App, RepoId, RepoId, Vec<WorkspaceId>) {
    let store = Store::open_in_memory().unwrap();
    let ids = seed_workspaces(&store, &["a", "b", "c", "d", "e"]);
    let other = store
        .add_repo(std::path::Path::new("/tmp/other"), "other", "")
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = crate::ui::View::Dashboard;
    app.refresh().unwrap();
    let repo = app.repos.iter().find(|r| r.name == "repo").unwrap().id;
    (app, repo, other, ids)
}

fn draw(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(120, 60)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, app))
        .unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

async fn press(app: &mut App, code: KeyCode) {
    handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
}

fn click(column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

fn selected_ws(app: &App) -> Option<WorkspaceId> {
    match app.selected_target() {
        Some(SelectionTarget::Workspace(id)) => Some(id),
        _ => None,
    }
}

#[tokio::test]
async fn f_on_a_header_focuses_its_repo_and_starts_on_the_first_workspace() {
    let (mut app, repo, _, _) = app();
    draw(&mut app);
    app.dashboard.selection = Some(SelectionTarget::Repo(repo));
    press(&mut app, KeyCode::Char('f')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo));
    draw(&mut app);
    assert_eq!(
        app.selectable.len(),
        5,
        "only the repo's workspaces are selectable"
    );
    assert!(
        app.selectable
            .iter()
            .all(|t| matches!(t, SelectionTarget::Workspace(_)))
    );
    assert_eq!(app.selected_target(), app.selectable.first().cloned());
}

#[tokio::test]
async fn f_on_a_workspace_keeps_it_selected() {
    let (mut app, repo, _, ids) = app();
    draw(&mut app);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[3]));
    press(&mut app, KeyCode::Char('f')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo));
    draw(&mut app);
    assert_eq!(selected_ws(&app), Some(ids[3]));
}

#[tokio::test]
async fn f_and_esc_leave_the_focus_view() {
    let (mut app, repo, _, ids) = app();
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    press(&mut app, KeyCode::Char('f')).await;
    press(&mut app, KeyCode::Char('f')).await;
    assert_eq!(app.dashboard.focused_repo, None);
    assert_eq!(selected_ws(&app), Some(ids[0]), "selection survives");

    app.dashboard.focus_repo(repo);
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.dashboard.focused_repo, None);
}

/// Esc peels filters before it leaves the focus view.
#[tokio::test]
async fn esc_clears_the_filter_before_leaving() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    app.dashboard.filter = Some("a".into());
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.dashboard.filter, None);
    assert_eq!(app.dashboard.focused_repo, Some(repo));
}

#[tokio::test]
async fn brackets_cycle_the_focused_repo_with_wrap() {
    let (mut app, repo, other, _) = app();
    app.dashboard.focus_repo(repo);
    press(&mut app, KeyCode::Char(']')).await;
    assert_eq!(app.dashboard.focused_repo, Some(other));
    press(&mut app, KeyCode::Char(']')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo), "wraps to the first");
    press(&mut app, KeyCode::Char('[')).await;
    assert_eq!(app.dashboard.focused_repo, Some(other), "wraps to the last");
}

#[tokio::test]
async fn brackets_do_nothing_outside_the_focus_view() {
    let (mut app, _, _, _) = app();
    press(&mut app, KeyCode::Char(']')).await;
    assert_eq!(app.dashboard.focused_repo, None);
}

/// j/k walk the rows in order; h does not fold anything out from under
/// the view.
#[tokio::test]
async fn j_and_k_move_through_the_rows() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    draw(&mut app);
    let order = app.selectable.clone();
    let at = |app: &App| {
        order
            .iter()
            .position(|t| Some(t.clone()) == app.selected_target())
    };

    assert_eq!(at(&app), Some(0));
    press(&mut app, KeyCode::Char('j')).await;
    press(&mut app, KeyCode::Down).await;
    assert_eq!(at(&app), Some(2));
    press(&mut app, KeyCode::Char('k')).await;
    assert_eq!(at(&app), Some(1));
    press(&mut app, KeyCode::Char('h')).await;
    assert_eq!(at(&app), Some(1));
    assert_eq!(app.dashboard.focused_repo, Some(repo));
    assert!(app.dashboard.folded.is_empty(), "h folds nothing");
}

#[tokio::test]
async fn n_in_an_empty_focus_view_creates_in_the_focused_repo() {
    let (mut app, _, other, _) = app();
    app.dashboard.selection = Some(SelectionTarget::Repo(other));
    press(&mut app, KeyCode::Char('f')).await;
    draw(&mut app);
    assert_eq!(app.selected_target(), None, "empty view selects nothing");
    press(&mut app, KeyCode::Char('n')).await;
    assert!(
        matches!(
            app.modal,
            Some(crate::ui::modal::Modal::NewWorkspace { repo_id, .. }) if repo_id == other
        ),
        "new-workspace form targets the focused repo"
    );
}

#[tokio::test]
async fn g_leaves_the_focus_view() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    press(&mut app, KeyCode::Char('G')).await;
    assert_eq!(app.dashboard.focused_repo, None);
}

#[test]
fn the_view_draws_a_breadcrumb_and_one_tall_row_per_workspace() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    let screen = draw(&mut app);
    assert!(screen.contains("◂ all repos / repo"), "{screen}");
    for name in ["repo/a", "repo/b", "repo/c", "repo/d", "repo/e"] {
        assert!(screen.contains(name), "row for {name} missing:\n{screen}");
    }
    assert_eq!(app.dashboard_focus_row_rects.len(), 5);
    assert!(
        app.dashboard_focus_row_rects
            .windows(2)
            .all(|w| w[0].1.y + w[0].1.height < w[1].1.y),
        "rows stack top to bottom with a gap"
    );
    assert!(app.dashboard_unfocus_rect.is_some());
    assert!(
        app.dashboard_repo_name_rects.is_empty(),
        "no repo headers in the focus view"
    );
}

#[test]
fn the_list_records_a_click_rect_per_repo_name() {
    let (mut app, repo, other, _) = app();
    draw(&mut app);
    let ids: Vec<RepoId> = app
        .dashboard_repo_name_rects
        .iter()
        .map(|(id, _)| *id)
        .collect();
    assert!(ids.contains(&repo) && ids.contains(&other), "{ids:?}");
    assert!(app.dashboard_focus_row_rects.is_empty());
}

#[test]
fn clicking_a_repo_name_targets_that_repo() {
    let (mut app, repo, _, _) = app();
    draw(&mut app);
    let (_, r) = *app
        .dashboard_repo_name_rects
        .iter()
        .find(|(id, _)| *id == repo)
        .unwrap();
    assert_eq!(
        repo_name_click_target(&app, &click(r.x, r.y)),
        Some(repo),
        "first cell of the name"
    );
    assert_eq!(
        repo_name_click_target(&app, &click(r.x + r.width - 1, r.y)),
        Some(repo),
        "last cell of the name"
    );
    assert_eq!(
        repo_name_click_target(&app, &click(r.x + r.width, r.y)),
        None
    );
}

#[test]
fn clicking_a_row_targets_its_workspace() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    draw(&mut app);
    for (ws, r) in app.dashboard_focus_row_rects.clone() {
        // Any line of the row counts, not just the first.
        assert_eq!(focus_row_click_target(&app, &click(r.x, r.y)), Some(ws));
        assert_eq!(
            focus_row_click_target(&app, &click(r.x + r.width / 2, r.y + r.height - 1)),
            Some(ws)
        );
    }
}

/// A long goal wraps in full and the full branch name shows.
#[test]
fn tall_rows_show_long_recaps_and_branches_whole() {
    let (mut app, repo, _, ids) = app();
    let goal = "Single-repo dashboard view with richer per-workspace detail, wrapping every recap field across as many lines as it needs";
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            goal: Some(goal.into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    let screen = draw(&mut app);
    let flat = screen.split_whitespace().collect::<Vec<_>>().join(" ");
    for chunk in ["wrapping every recap field", "as it needs"] {
        assert!(flat.contains(chunk), "{chunk:?} missing:\n{screen}");
    }
    let r = app
        .dashboard_focus_row_rects
        .iter()
        .find(|(id, _)| *id == ids[0])
        .unwrap()
        .1;
    assert!(r.height >= 4, "head, wrapped goal, git line: {r:?}");
}
