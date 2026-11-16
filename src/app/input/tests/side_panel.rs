//! The side panel outside the repo focus view: on the dashboard list.
//! (The focus view's own panel tests live in `repo_focus`.)

use super::common::*;
use super::repo_focus::branch_lists;
use super::*;
use crate::data::store::{RepoId, Store, WorkspaceId};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

/// The dashboard list, not focused, with `a` selected, a base branch set,
/// and two commits and two changed files cached for `a`.
fn app() -> (App, RepoId, Vec<WorkspaceId>) {
    let store = Store::open_in_memory().unwrap();
    let ids = seed_workspaces(&store, &["a", "b"]);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = crate::ui::View::Dashboard;
    app.refresh().unwrap();
    let repo = app.repos.iter().find(|r| r.name == "repo").unwrap().id;
    app.store
        .set_repo_base_branch(repo, Some("origin/main"))
        .unwrap();
    app.refresh().unwrap();
    app.workspace_branch_lists
        .insert(ids[0], branch_lists("origin/main"));
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    (app, repo, ids)
}

fn draw_at(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
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

fn draw(app: &mut App) -> String {
    draw_at(app, 140, 40)
}

async fn press(app: &mut App, code: KeyCode) {
    handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
}

#[tokio::test]
async fn c_on_the_list_opens_the_panel_beside_the_rows() {
    let (mut app, _, _) = app();
    assert!(!draw(&mut app).contains("UNIQUE-COMMIT-SUBJECT"));
    assert!(app.side_panel_rect.is_none(), "closed by default");

    press(&mut app, KeyCode::Char('c')).await;
    let screen = draw(&mut app);
    assert_eq!(app.dashboard.focused_repo, None, "still the list");
    assert!(screen.contains("Commits 2"), "{screen}");
    assert!(screen.contains("repo/a vs main"), "{screen}");
    assert!(
        screen.contains("a1b2c3d  UNIQUE-COMMIT-SUBJECT"),
        "{screen}"
    );
    let panel = app.side_panel_rect.expect("panel drawn");
    // The list's rows end left of the panel: the selected row's name
    // shows, and nothing of the list bleeds past the panel's edge.
    for line in screen
        .lines()
        .skip(panel.y as usize)
        .take(panel.height as usize)
    {
        let left: String = line.chars().take(panel.x as usize).collect();
        assert!(!left.contains("UNIQUE-COMMIT"), "{line:?}");
    }

    press(&mut app, KeyCode::Char('2')).await;
    let screen = draw(&mut app);
    assert!(screen.contains("2 files  +43 −3"), "{screen}");

    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    assert!(app.side_panel_rect.is_none(), "c closes it");
}

/// A repo header has no branch; the panel says to pick a workspace.
#[tokio::test]
async fn a_repo_header_selection_explains_itself() {
    let (mut app, repo, _) = app();
    press(&mut app, KeyCode::Char('c')).await;
    app.dashboard.selection = Some(SelectionTarget::Repo(repo));
    let screen = draw(&mut app);
    assert!(screen.contains("Select a workspace"), "{screen}");
}

/// Esc closes the panel; with nothing else to peel it does nothing more.
#[tokio::test]
async fn esc_closes_the_panel_on_the_list() {
    let (mut app, _, _) = app();
    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    press(&mut app, KeyCode::Esc).await;
    assert!(!app.dashboard.panel.open);
}

/// The wheel over the panel scrolls it rather than the list.
#[tokio::test]
async fn the_wheel_over_the_panel_scrolls_it() {
    let (mut app, _, ids) = app();
    let mut lists = branch_lists("origin/main");
    lists.commits = (0..80)
        .map(|i| crate::git::BranchCommit {
            hash: format!("{i:07}"),
            subject: format!("commit {i}"),
        })
        .collect();
    app.workspace_branch_lists.insert(ids[0], lists);
    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    let panel = app.side_panel_rect.unwrap();
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: panel.x + 3,
            row: panel.y + 5,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await;
    assert_eq!(app.dashboard.panel.scroll, 3);
    let screen = draw(&mut app);
    assert!(!screen.contains("0000000  commit 0"), "{screen}");
}
