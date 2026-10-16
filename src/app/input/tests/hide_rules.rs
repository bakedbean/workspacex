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
    select(&mut app, group.clone());
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

fn count_of(rows: &[(HideRule, usize)], rule: &HideRule) -> usize {
    rows.iter()
        .find(|(r, _)| r == rule)
        .map(|(_, n)| *n)
        .expect("row present")
}

#[tokio::test]
async fn f_opens_picker_for_selected_repo_with_counts() {
    let (mut app, repo, _) = app_with_merged_hidden();
    open_picker(&mut app).await;
    let Some(crate::ui::modal::Modal::HideRules {
        repo_id,
        rows,
        rules,
        ..
    }) = &app.modal
    else {
        panic!("expected HideRules modal, got {:?}", app.modal);
    };
    assert_eq!(*repo_id, repo);
    assert_eq!(count_of(rows, &MERGED), 1, "one merged workspace");
    assert!(rules.contains(&MERGED));
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
        !stored.hide_rules.contains(&MERGED),
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

const NO_PR: HideRule = HideRule::Pr(PrCriterion::NoPr);

/// One repo hiding no-PR workspaces; both start with no PR and no activity,
/// created long enough ago that creation no longer counts as recent.
fn app_with_no_pr_hidden() -> (
    App,
    crate::data::store::RepoId,
    Vec<crate::data::store::WorkspaceId>,
) {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let repo = app.repos[0].id;
    app.store
        .set_repo_hide_rules(repo, &HideRules::parse(Some("pr:none")))
        .unwrap();
    app.refresh().unwrap();
    for (_, w) in &mut app.workspaces {
        w.created_at = 0;
    }
    let ids: Vec<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    for id in &ids {
        app.pr_lifecycle.insert(*id, BranchLifecycle::NoPr);
    }
    (app, repo, ids)
}

fn report_working(app: &mut App, ws: crate::data::store::WorkspaceId) {
    app.pushed_status.insert(
        ws,
        crate::data::store::ReportedStatus {
            state: crate::data::store::ReportedState::Working,
            message: None,
            source: "test".into(),
            reported_at: crate::util::time::now_ms(),
        },
    );
}

#[tokio::test]
async fn no_pr_workspace_with_uncommitted_changes_stays_visible() {
    let (mut app, repo, ids) = app_with_no_pr_hidden();
    app.workspace_status.insert(
        ids[0],
        crate::git::WorkspaceStatus {
            modified: 1,
            ..Default::default()
        },
    );
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[0])));
    assert_eq!(app.hidden_group_of(ids[1]), Some((repo, NO_PR)));
}

#[tokio::test]
async fn newly_created_no_pr_workspace_stays_visible() {
    let (mut app, repo, ids) = app_with_no_pr_hidden();
    let now = crate::util::time::now_ms();
    for (_, w) in &mut app.workspaces {
        if w.id == ids[0] {
            w.created_at = now;
        }
    }
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[0])));
    assert_eq!(app.hidden_group_of(ids[1]), Some((repo, NO_PR)));
}

#[tokio::test]
async fn no_pr_workspace_with_commits_ahead_of_base_stays_visible() {
    let (mut app, _, ids) = app_with_no_pr_hidden();
    // Pushed and clean: nothing ahead of upstream, no line diff.
    app.workspace_status.insert(ids[0], Default::default());
    app.workspace_ahead_of_base.insert(ids[0], 1);
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[0])));
}

#[tokio::test]
async fn picker_counts_only_hideable_no_pr_workspaces() {
    let (mut app, _, ids) = app_with_no_pr_hidden();
    report_working(&mut app, ids[0]);
    open_picker(&mut app).await;
    let Some(crate::ui::modal::Modal::HideRules { rows, .. }) = &app.modal else {
        panic!("expected HideRules modal, got {:?}", app.modal);
    };
    assert_eq!(count_of(rows, &NO_PR), 1, "the working one isn't counted");
}

#[tokio::test]
async fn selection_follows_a_workspace_that_leaves_the_hidden_group() {
    let (mut app, repo, ids) = app_with_no_pr_hidden();
    app.dashboard.expanded_hidden.insert((repo, NO_PR));
    draw(&mut app);
    select(&mut app, SelectionTarget::Workspace(ids[0]));
    report_working(&mut app, ids[0]); // agent starts working
    draw(&mut app);
    assert_eq!(app.hidden_group_of(ids[0]), None);
    assert_eq!(
        app.selected_target(),
        Some(SelectionTarget::Workspace(ids[0]))
    );
    assert_eq!(
        app.selectable[app.dashboard.selected],
        SelectionTarget::Workspace(ids[0])
    );
}

#[tokio::test]
async fn tagged_workspace_hides_under_tag_rule() {
    let mut app = test_app_with_two_ready_workspaces();
    app.refresh().unwrap();
    let repo = app.repos[0].id;
    let ids: Vec<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    app.store.add_workspace_tag(ids[0], "parked").unwrap();
    app.store
        .set_repo_hide_rules(repo, &HideRules::parse(Some("tag:parked")))
        .unwrap();
    app.refresh().unwrap();
    draw(&mut app);
    assert!(!app.selectable.contains(&SelectionTarget::Workspace(ids[0])));
    assert_eq!(
        app.hidden_group_of(ids[0]),
        Some((repo, HideRule::tag("parked")))
    );
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[1])));
}

fn picker_rows(app: &App) -> Vec<(HideRule, usize)> {
    let Some(crate::ui::modal::Modal::HideRules { rows, .. }) = &app.modal else {
        panic!("expected HideRules modal, got {:?}", app.modal);
    };
    rows.clone()
}

#[tokio::test]
async fn picker_lists_global_tags_first_with_per_repo_counts() {
    let (mut app, _, ids) = app_with_merged_hidden();
    app.store.add_workspace_tag(ids[0], "parked").unwrap();
    // A tag used only in another repo is still offered.
    let other = app
        .store
        .add_repo(std::path::Path::new("/tmp/o"), "other", "")
        .unwrap();
    let ow = app
        .store
        .insert_workspace(&crate::data::store::NewWorkspace {
            repo_id: other,
            name: "o",
            branch: "o",
            worktree_path: std::path::Path::new("/tmp/wsx-o"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    app.store.add_workspace_tag(ow, "ui").unwrap();
    app.refresh().unwrap();
    open_picker(&mut app).await;
    let rows = picker_rows(&app);
    assert_eq!(rows[0], (HideRule::tag("parked"), 1));
    assert_eq!(
        rows[1],
        (HideRule::tag("ui"), 0),
        "counts are this repo only"
    );
    assert_eq!(rows[2].0, MERGED);
}

#[tokio::test]
async fn stale_tag_rule_is_listed_with_zero_count() {
    let (mut app, repo, _) = app_with_merged_hidden();
    app.store
        .set_repo_hide_rules(repo, &HideRules::parse(Some("tag:gone,pr:merged")))
        .unwrap();
    app.refresh().unwrap();
    open_picker(&mut app).await;
    assert_eq!(picker_rows(&app)[0], (HideRule::tag("gone"), 0));
}

#[tokio::test]
async fn space_on_tag_row_toggles_tag_rule() {
    let (mut app, repo, ids) = app_with_merged_hidden();
    app.store.add_workspace_tag(ids[1], "parked").unwrap();
    app.refresh().unwrap();
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
    assert!(stored.hide_rules.contains(&HideRule::tag("parked")));
}

#[tokio::test]
async fn picker_cursor_reaches_the_last_pr_row_past_tag_rows() {
    let (mut app, _, ids) = app_with_merged_hidden();
    app.store.add_workspace_tag(ids[1], "parked").unwrap();
    app.refresh().unwrap();
    open_picker(&mut app).await;
    let shared = shared_app();
    for _ in 0..20 {
        crate::app::input::modal::handle_key_modal(&mut app, &shared, key(KeyCode::Down))
            .await
            .unwrap();
    }
    let Some(crate::ui::modal::Modal::HideRules { selected, rows, .. }) = &app.modal else {
        panic!();
    };
    assert_eq!(*selected, rows.len() - 1);
    assert_eq!(rows.len(), 7, "one tag row plus six PR rows");
}
