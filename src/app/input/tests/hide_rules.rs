//! Hidden-group expand/collapse, selection fallback, and the `F` picker.

use super::common::*;
use super::*;
use crate::data::hide_rules::{HideRule, HideRules, PrCriterion};
use crate::git::forge::BranchLifecycle;
use crossterm::event::KeyModifiers;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

const MERGED: HideRule = HideRule::Pr(PrCriterion::Merged);

/// One repo hiding merged PRs; `first` is merged, `second` open.
fn app_with_merged_hidden() -> (
    App,
    crate::data::store::RepoId,
    Vec<crate::data::store::WorkspaceId>,
) {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let repo = app.repos[0].id;
    app.store
        .set_repo_hide_rules(repo, &HideRules::parse(Some("pr:merged")))
        .unwrap();
    app.refresh().unwrap();
    let ids: Vec<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    app.pr_lifecycle.insert(ids[0], BranchLifecycle::PrMerged);
    app.pr_lifecycle.insert(ids[1], BranchLifecycle::PrOpen);
    (app, repo, ids)
}

fn draw(app: &mut App) {
    let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, app))
        .unwrap();
}

fn select(app: &mut App, t: SelectionTarget) {
    let idx = app
        .selectable
        .iter()
        .position(|x| *x == t)
        .expect("target visible");
    app.select_index(idx);
}

#[tokio::test]
async fn enter_expands_and_h_collapses_a_hidden_group() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    draw(&mut app);
    let group = SelectionTarget::HiddenGroup { repo, rule: MERGED };
    select(&mut app, group);
    press_key(&mut app, KeyCode::Enter).await;
    assert!(app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[0])));

    select(&mut app, SelectionTarget::Workspace(ids[0]));
    press(&mut app, 'h', KeyModifiers::NONE).await;
    assert!(!app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
    assert_eq!(
        app.selected_target(),
        Some(group),
        "h lands on the group row"
    );
}

#[tokio::test]
async fn h_on_collapsed_group_row_folds_the_repo() {
    let (mut app, repo, _) = app_with_merged_hidden();
    draw(&mut app);
    select(
        &mut app,
        SelectionTarget::HiddenGroup { repo, rule: MERGED },
    );
    press(&mut app, 'h', KeyModifiers::NONE).await;
    assert_eq!(app.dashboard.folded.get(&(repo.0 as u64)), Some(&true));
}

#[tokio::test]
async fn l_expands_a_hidden_group() {
    let (mut app, repo, _) = app_with_merged_hidden();
    draw(&mut app);
    select(
        &mut app,
        SelectionTarget::HiddenGroup { repo, rule: MERGED },
    );
    press(&mut app, 'l', KeyModifiers::NONE).await;
    assert!(app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
}

#[tokio::test]
async fn selected_workspace_falls_back_to_its_group_row_when_hidden() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    app.pr_lifecycle.insert(ids[0], BranchLifecycle::PrOpen);
    draw(&mut app);
    select(&mut app, SelectionTarget::Workspace(ids[0]));
    app.pr_lifecycle.insert(ids[0], BranchLifecycle::PrMerged); // PR merges
    draw(&mut app);
    assert_eq!(
        app.selected_target(),
        Some(SelectionTarget::HiddenGroup { repo, rule: MERGED })
    );
}

#[tokio::test]
async fn select_by_name_reveals_hidden_group() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    draw(&mut app);
    assert!(app.select_workspace_by_name("repo", "first"));
    assert!(app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
    draw(&mut app);
    assert_eq!(
        app.selected_target(),
        Some(SelectionTarget::Workspace(ids[0]))
    );
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[0])));
}

#[tokio::test]
async fn select_by_name_refuses_hidden_workspace_the_filter_excludes() {
    let (mut app, _, ids) = app_with_merged_hidden();
    app.dashboard.filter = Some("second".into());
    draw(&mut app);
    assert!(!app.select_workspace_by_name("repo", "first"));
    draw(&mut app);
    assert_ne!(
        app.selected_target(),
        Some(SelectionTarget::Workspace(ids[0]))
    );
}

async fn open_picker(app: &mut App) {
    draw(app);
    let repo = app.repos[0].id;
    select(app, SelectionTarget::Repo(repo));
    press(app, 'F', KeyModifiers::NONE).await;
}

#[tokio::test]
async fn f_opens_picker_for_selected_repo_with_counts() {
    let (mut app, repo, _) = app_with_merged_hidden();
    open_picker(&mut app).await;
    let Some(crate::ui::modal::Modal::HideRules {
        repo_id,
        counts,
        rules,
        ..
    }) = &app.modal
    else {
        panic!("expected HideRules modal, got {:?}", app.modal);
    };
    assert_eq!(*repo_id, repo);
    assert_eq!(counts[0], 1, "one merged workspace"); // ALL[0] is Merged
    assert!(rules.contains(MERGED));
}

#[tokio::test]
async fn space_toggles_and_persists() {
    let (mut app, repo, _) = app_with_merged_hidden();
    open_picker(&mut app).await;
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Char(' ')))
        .await
        .unwrap();
    let stored = app
        .store
        .repos()
        .unwrap()
        .into_iter()
        .find(|r| r.id == repo)
        .unwrap();
    assert!(
        !stored.hide_rules.contains(MERGED),
        "space on row 0 toggles merged off"
    );
    assert!(
        matches!(app.modal, Some(crate::ui::modal::Modal::HideRules { .. })),
        "picker stays open"
    );
}

#[tokio::test]
async fn toggling_rule_off_forgets_expansion() {
    let (mut app, repo, _) = app_with_merged_hidden();
    app.dashboard.expanded_hidden.insert((repo, MERGED));
    open_picker(&mut app).await;
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Char(' ')))
        .await
        .unwrap();
    assert!(!app.dashboard.expanded_hidden.contains(&(repo, MERGED)));
}

#[tokio::test]
async fn esc_closes_picker() {
    let (mut app, _, _) = app_with_merged_hidden();
    open_picker(&mut app).await;
    let shared = shared_app();
    crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();
    assert!(app.modal.is_none());
}
