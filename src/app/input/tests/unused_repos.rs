//! `u` hides and shows repos with no workspaces.

use super::common::*;
use super::*;
use crate::data::store::RepoId;
use crate::ui::dashboard::HIDE_UNUSED_REPOS_SETTING;
use crossterm::event::KeyModifiers;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// Two ready workspaces in `repo`, plus an `empty` repo with none.
fn app_with_an_empty_repo() -> (App, RepoId, RepoId) {
    let mut app = test_app_with_two_ready_workspaces();
    let empty = app
        .store
        .add_repo(std::path::Path::new("/tmp/empty"), "empty", "")
        .unwrap();
    app.refresh().unwrap();
    let used = app.repos.iter().find(|r| r.name == "repo").unwrap().id;
    (app, used, empty)
}

fn draw(app: &mut App) -> String {
    let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, app))
        .unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn u_hides_repos_without_workspaces_and_remembers_it() {
    let (mut app, used, empty) = app_with_an_empty_repo();
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Repo(empty)));

    press(&mut app, 'u', KeyModifiers::NONE).await;
    assert!(app.dashboard.hide_unused_repos);
    assert_eq!(
        app.store
            .get_setting(HIDE_UNUSED_REPOS_SETTING)
            .unwrap()
            .as_deref(),
        Some("on")
    );
    let screen = draw(&mut app);
    assert!(!app.selectable.contains(&SelectionTarget::Repo(empty)));
    assert!(app.selectable.contains(&SelectionTarget::Repo(used)));
    assert!(!screen.contains("empty"), "{screen}");

    press(&mut app, 'u', KeyModifiers::NONE).await;
    assert!(!app.dashboard.hide_unused_repos);
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Repo(empty)));
}

#[test]
fn the_setting_is_restored_at_startup() {
    let store = crate::data::store::Store::open_in_memory().unwrap();
    store.set_setting(HIDE_UNUSED_REPOS_SETTING, "on").unwrap();
    let app = App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(app.dashboard.hide_unused_repos);
}

#[tokio::test]
async fn the_focused_repo_stays_even_when_empty() {
    let (mut app, _, empty) = app_with_an_empty_repo();
    app.dashboard.hide_unused_repos = true;
    app.dashboard.focus_repo(empty);
    let screen = draw(&mut app);
    assert!(screen.contains("n to create one"), "{screen}");
}

#[tokio::test]
async fn an_all_empty_list_says_how_to_show_them() {
    let (mut app, _) = make_app_with_n_repos(2);
    app.dashboard.hide_unused_repos = true;
    let screen = draw(&mut app);
    assert!(app.selectable.is_empty());
    assert!(screen.contains("u shows unused repos"), "{screen}");
}
