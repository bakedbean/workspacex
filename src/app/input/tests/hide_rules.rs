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
