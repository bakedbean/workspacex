//! The repo focus view: entering and leaving it, moving between repos,
//! moving through its rows, and clicking rows and repo names.

use super::common::*;
use super::*;
use crate::data::store::{RepoId, Store, WorkspaceId};
use crate::git::forge::BranchLifecycle;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

/// Five workspaces in "repo", plus an empty second repo "other".
fn app() -> (App, RepoId, RepoId, Vec<WorkspaceId>) {
    app_named(&["a", "b", "c", "d", "e"])
}

/// `app()` with the first workspace's name (and so its branch) so long that
/// its row is taller than a small screen: branch names wrap uncapped.
fn app_with_a_tall_row() -> (App, RepoId, RepoId, Vec<WorkspaceId>) {
    let long = format!("a{}", "x".repeat(600));
    app_named(&[&long, "b", "c", "d", "e"])
}

fn app_named(names: &[&str]) -> (App, RepoId, RepoId, Vec<WorkspaceId>) {
    let store = Store::open_in_memory().unwrap();
    let ids = seed_workspaces(&store, names);
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

/// Adds a repo named `name` holding one Ready workspace, then refreshes.
fn add_repo_with_a_workspace(app: &mut App, name: &str) -> RepoId {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    let rid = app
        .store
        .add_repo(&PathBuf::from(format!("/tmp/{name}")), name, "")
        .unwrap();
    let id = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id: rid,
            name: &format!("{name}-ws"),
            branch: &format!("{name}/ws"),
            worktree_path: &PathBuf::from(format!("/tmp/wsx-test/{name}-ws")),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    app.store
        .set_workspace_state(id, WorkspaceState::Ready)
        .unwrap();
    app.refresh().unwrap();
    rid
}

#[tokio::test]
async fn brackets_cycle_the_focused_repo_with_wrap() {
    let (mut app, repo, _, _) = app();
    let third = add_repo_with_a_workspace(&mut app, "third");
    app.dashboard.focus_repo(repo);
    press(&mut app, KeyCode::Char(']')).await;
    assert_eq!(
        app.dashboard.focused_repo,
        Some(third),
        "skips empty \"other\""
    );
    press(&mut app, KeyCode::Char(']')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo), "wraps to the first");
    press(&mut app, KeyCode::Char('[')).await;
    assert_eq!(app.dashboard.focused_repo, Some(third), "wraps to the last");
    press(&mut app, KeyCode::Char('[')).await;
    assert_eq!(
        app.dashboard.focused_repo,
        Some(repo),
        "skips empty going back"
    );
}

/// Focused on an empty repo (say its last workspace was archived),
/// the brackets still move on to the repos that have workspaces.
#[tokio::test]
async fn brackets_leave_an_empty_focused_repo() {
    let (mut app, repo, other, _) = app();
    app.dashboard.focus_repo(other);
    press(&mut app, KeyCode::Char(']')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo));
}

/// With only one repo holding workspaces, the brackets stay put.
#[tokio::test]
async fn brackets_stay_when_no_other_repo_has_workspaces() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    press(&mut app, KeyCode::Char(']')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo));
    press(&mut app, KeyCode::Char('[')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo));
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
async fn g_toggles_pr_grouping_and_remembers_it() {
    use crate::ui::dashboard::FOCUS_GROUP_BY_PR_SETTING;
    let (mut app, repo, _, _) = app();
    let group_mode = app.dashboard.group_mode;
    app.dashboard.focus_repo(repo);
    assert!(app.dashboard.focus_group_by_pr, "grouped by default");
    press(&mut app, KeyCode::Char('G')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo), "stays focused");
    assert!(!app.dashboard.focus_group_by_pr);
    assert_eq!(
        app.dashboard.group_mode, group_mode,
        "list grouping untouched"
    );
    assert_eq!(
        app.store
            .get_setting(FOCUS_GROUP_BY_PR_SETTING)
            .unwrap()
            .as_deref(),
        Some("off")
    );
    press(&mut app, KeyCode::Char('G')).await;
    assert!(app.dashboard.focus_group_by_pr);
    assert_eq!(
        app.dashboard.focus_scroll_anchor, None,
        "the next frame scrolls the reordered selection into view"
    );
}

/// The focus view's side panel starts open on the Next tab, showing
/// the selected workspace's recap in full; `>` and `<` step through its
/// tabs, wrapping, and the tab is remembered. The rows show no prose.
#[tokio::test]
async fn the_focus_panel_starts_open_and_switches_tabs() {
    use crate::ui::dashboard::FOCUS_TAB_SETTING;
    use crate::ui::dashboard::side_panel::PanelTab;
    let (mut app, repo, _, ids) = app();
    let goal = format!("UNIQUE-GOAL {}GOAL-END", "word ".repeat(80));
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            goal: Some(goal),
            state: Some("UNIQUE-STATE".into()),
            next: Some("UNIQUE-NEXT".into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    assert_eq!(app.dashboard.focus_panel.tab, PanelTab::Next, "next first");
    let screen = draw(&mut app);
    assert!(app.side_panel_rect.is_some(), "open:\n{screen}");
    assert!(screen.contains("UNIQUE-NEXT"), "{screen}");
    assert!(
        screen.contains("UNIQUE-STATE"),
        "state under next:\n{screen}"
    );
    assert!(
        !screen.contains("UNIQUE-GOAL"),
        "one tab at a time:\n{screen}"
    );
    assert_eq!(
        screen.matches("UNIQUE-NEXT").count(),
        1,
        "not on the row too"
    );

    press(&mut app, KeyCode::Char('<')).await;
    assert_eq!(app.dashboard.focus_panel.tab, PanelTab::Goal);
    let screen = draw(&mut app);
    assert!(
        screen.contains("UNIQUE-GOAL") && screen.contains("GOAL-END"),
        "the goal is never cut short:\n{screen}"
    );
    assert_eq!(
        app.store.get_setting(FOCUS_TAB_SETTING).unwrap().as_deref(),
        Some("goal")
    );
    assert!(!app.dashboard.panel.open, "the list's panel is untouched");

    press(&mut app, KeyCode::Char('3')).await;
    assert_eq!(app.dashboard.focus_panel.tab, PanelTab::Next);
    press(&mut app, KeyCode::Char('>')).await;
    press(&mut app, KeyCode::Char('>')).await;
    press(&mut app, KeyCode::Char('>')).await;
    assert_eq!(app.dashboard.focus_panel.tab, PanelTab::Chat);
    press(&mut app, KeyCode::Char('>')).await;
    assert_eq!(app.dashboard.focus_panel.tab, PanelTab::Prompt, "wraps");
}

/// `6` shows the agent's latest message in full, even with a recap set,
/// and saves the tab.
#[tokio::test]
async fn six_shows_the_latest_message_beside_a_recap() {
    use crate::ui::dashboard::FOCUS_TAB_SETTING;
    use crate::ui::dashboard::side_panel::PanelTab;
    let (mut app, repo, _, ids) = app();
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            next: Some("UNIQUE-NEXT".into()),
            ..Default::default()
        },
    );
    app.workspace_events.insert(
        ids[0],
        crate::activity::events::WorkspaceEvents {
            last_assistant_text: Some("**UNIQUE-CHAT** done".into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    press(&mut app, KeyCode::Char('6')).await;
    assert_eq!(app.dashboard.focus_panel.tab, PanelTab::Chat);
    let screen = draw(&mut app);
    assert!(
        screen.contains("UNIQUE-CHAT done") && !screen.contains("**"),
        "{screen}"
    );
    assert!(!screen.contains("UNIQUE-NEXT"), "{screen}");
    assert_eq!(
        app.store.get_setting(FOCUS_TAB_SETTING).unwrap().as_deref(),
        Some("chat")
    );
}

/// `c` toggles the focus view's panel as on the list, and Esc closes it
/// before leaving the view. With it closed, the rows show their prose.
#[tokio::test]
async fn c_and_esc_close_the_focus_panel() {
    let (mut app, repo, _, ids) = app();
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            next: Some("UNIQUE-NEXT".into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let screen = draw(&mut app);
    assert!(app.side_panel_rect.is_some(), "open to start with");
    assert!(
        !screen.contains("next    UNIQUE-NEXT"),
        "no row prose:\n{screen}"
    );

    press(&mut app, KeyCode::Char('c')).await;
    let screen = draw(&mut app);
    assert!(app.side_panel_rect.is_none(), "c closes it");
    assert!(
        screen.contains("next    UNIQUE-NEXT"),
        "row prose:\n{screen}"
    );
    assert!(!app.dashboard.panel.open, "the list's panel is untouched");

    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    assert!(app.side_panel_rect.is_some(), "c reopens it");
    press(&mut app, KeyCode::Esc).await;
    assert!(!app.dashboard.focus_panel.open, "Esc closes it first");
    assert_eq!(app.dashboard.focused_repo, Some(repo));
    draw(&mut app);
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.dashboard.focused_repo, None, "then leaves the view");
}

/// With the panel focused (`→`), `j` scrolls a text tab's body instead of
/// moving the selection.
#[tokio::test]
async fn a_focused_text_tab_scrolls() {
    let (mut app, repo, _, ids) = app();
    let prompt = (0..120)
        .map(|i| format!("line-{i}"))
        .collect::<Vec<_>>()
        .join("\n");
    app.workspace_events.insert(
        ids[0],
        crate::activity::events::WorkspaceEvents {
            first_user_text: Some(prompt),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    press(&mut app, KeyCode::Char('1')).await;
    draw(&mut app);
    press(&mut app, KeyCode::Right).await;
    assert!(app.dashboard.focus_panel.is_focused());
    for _ in 0..5 {
        dispatch_key(
            &mut app,
            &shared_app(),
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
    }
    let screen = draw(&mut app);
    assert!(
        !screen.contains("line-0\n") && !screen.contains("line-0 "),
        "{screen}"
    );
    assert!(screen.contains("line-5"), "{screen}");
    assert_eq!(selected_ws(&app), Some(ids[0]), "selection unchanged");
}

/// `→` on a closed focus panel opens it on Files, and that tab is saved.
#[tokio::test]
async fn right_on_a_closed_focus_panel_saves_files() {
    use crate::ui::dashboard::FOCUS_TAB_SETTING;
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    press(&mut app, KeyCode::Char('c')).await;
    press(&mut app, KeyCode::Right).await;
    assert!(app.dashboard.focus_panel.open);
    assert_eq!(
        app.store.get_setting(FOCUS_TAB_SETTING).unwrap().as_deref(),
        Some("files")
    );
}

/// Closing the focus panel lasts while you move between repos, but the
/// next time you enter the focus view it starts open again.
#[tokio::test]
async fn entering_the_focus_view_reopens_its_panel() {
    let (mut app, repo, _, _) = app();
    app.dashboard.selection = Some(SelectionTarget::Repo(repo));
    press(&mut app, KeyCode::Char('f')).await;
    press(&mut app, KeyCode::Char('c')).await;
    press(&mut app, KeyCode::Char(']')).await;
    assert!(!app.dashboard.focus_panel.open, "[ / ] keep it closed");
    press(&mut app, KeyCode::Char('f')).await;
    assert_eq!(app.dashboard.focused_repo, None);
    app.dashboard.selection = Some(SelectionTarget::Repo(repo));
    press(&mut app, KeyCode::Char('f')).await;
    assert!(app.dashboard.focus_panel.open, "open again on entry");
}

/// A blank full recap field falls back to its short form in the panel.
#[test]
fn a_blank_full_field_falls_back_to_the_short_one() {
    let (mut app, repo, _, ids) = app();
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            goal: Some("   ".into()),
            goal_short: Some("SHORT-GOAL".into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.focus_panel.tab = crate::ui::dashboard::side_panel::PanelTab::Goal;
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let screen = draw(&mut app);
    assert!(screen.contains("SHORT-GOAL"), "{screen}");
    assert!(!screen.contains("No recap goal set"), "{screen}");
}

#[test]
fn the_tab_setting_loads_at_startup() {
    use crate::ui::dashboard::FOCUS_TAB_SETTING;
    let store = Store::open_in_memory().unwrap();
    store.set_setting(FOCUS_TAB_SETTING, "prompt\n").unwrap();
    let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert_eq!(
        app.dashboard.focus_panel.tab,
        crate::ui::dashboard::side_panel::PanelTab::Prompt
    );
}

#[tokio::test]
async fn g_outside_the_focus_view_still_flips_list_grouping() {
    let (mut app, _, _, _) = app();
    let before = app.dashboard.group_mode;
    press(&mut app, KeyCode::Char('G')).await;
    assert_ne!(app.dashboard.group_mode, before);
    assert!(app.dashboard.focus_group_by_pr);
}

#[test]
fn the_grouping_setting_loads_at_startup() {
    use crate::ui::dashboard::FOCUS_GROUP_BY_PR_SETTING;
    let store = Store::open_in_memory().unwrap();
    store
        .set_setting(FOCUS_GROUP_BY_PR_SETTING, "off\n")
        .unwrap();
    let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(!app.dashboard.focus_group_by_pr);
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
            .all(|w| w[0].1.y + w[0].1.height == w[1].1.y),
        "rows stack top to bottom, flush"
    );
    assert!(app.dashboard_unfocus_rect.is_some());
    assert!(
        app.dashboard_repo_name_rects.is_empty(),
        "no repo headers in the focus view"
    );
}

/// The breadcrumb sums up the repo by PR stage, and every section header
/// tallies the workspaces in it that need a look.
#[test]
fn the_breadcrumb_shows_the_pipeline_and_headers_tally_attention() {
    let (mut app, repo, _, ids) = app();
    app.pr_lifecycle.insert(ids[0], BranchLifecycle::PrOpen);
    app.pr_lifecycle
        .insert(ids[1], BranchLifecycle::PrConflicted);
    app.pr_lifecycle.insert(ids[2], BranchLifecycle::PrDraft);
    app.pr_lifecycle.insert(ids[3], BranchLifecycle::PrMerged);
    app.dashboard.focus_repo(repo);
    let screen = draw(&mut app);
    let crumb = screen.lines().find(|l| l.contains("◂ all repos")).unwrap();
    assert!(
        crumb.contains("No PR 1 › Draft 1 › Open 2 › Done 1"),
        "{crumb}"
    );
    assert!(screen.contains("━━ Open · 2 · ⏺ 1 conflict ━"), "{screen}");
    // The focus header's tally filters by status like the dashboard's.
    let crumb_y = screen
        .lines()
        .position(|l| l.contains("◂ all repos"))
        .unwrap() as u16;
    assert!(
        app.dashboard_status_rects
            .iter()
            .any(|(_, r)| r.y == crumb_y),
        "{:?}",
        app.dashboard_status_rects
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

/// Beside the open panel the rows show no prose: a long goal wraps in
/// the panel, in full, and nowhere else.
#[test]
fn a_long_goal_wraps_in_the_panel_not_the_row() {
    let (mut app, repo, _, ids) = app();
    let goal = "Single-repo dashboard view with richer per-workspace detail, wrapping the recap goal across the lines it needs";
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            goal: Some(goal.into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.focus_panel.tab = crate::ui::dashboard::side_panel::PanelTab::Goal;
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let screen = draw(&mut app);
    let flat = screen.split_whitespace().collect::<Vec<_>>().join(" ");
    for chunk in ["Single-repo dashboard", "lines it needs"] {
        assert!(flat.contains(chunk), "{chunk:?} missing:\n{screen}");
    }
    let panel = app.side_panel_rect.expect("open panel");
    let goal_lines: Vec<&str> = screen
        .lines()
        .filter(|l| goal.split(' ').any(|w| w.len() > 6 && l.contains(w)))
        .collect();
    assert!(goal_lines.len() >= 2, "goal wraps:\n{screen}");
    for l in goal_lines {
        let rows_part: String = l.chars().take(panel.x as usize).collect();
        assert!(
            !goal
                .split(' ')
                .any(|w| w.len() > 6 && rows_part.contains(w)),
            "goal on a row:\n{screen}"
        );
    }
}

/// The focus view keeps only the detail bar's reply row: no header or
/// containers, but Tab still reaches the reply input.
#[tokio::test]
async fn the_focus_view_keeps_only_the_reply_row() {
    let (mut app, repo, _, ids) = app();
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let list = draw(&mut app);
    assert!(
        list.contains("Reply to agent"),
        "bar shows on the list:\n{list}"
    );

    app.dashboard.focus_repo(repo);
    let focus = draw(&mut app);
    assert!(focus.contains("Reply to agent"), "reply row kept:\n{focus}");
    assert!(
        !focus.contains("SESSION SUMMARY"),
        "containers dropped:\n{focus}"
    );
    assert!(app.detail_container_rects.iter().all(Option::is_none));
    press(&mut app, KeyCode::Tab).await;
    assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
    // Still focused after a redraw: the reply row counts as a visible bar.
    draw(&mut app);
    assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
}

/// A row taller than the screen still draws, from its top, rather than
/// vanishing; and no click target covers lines nothing was drawn on.
#[test]
fn an_oversized_selected_row_still_draws() {
    let (mut app, repo, _, ids) = app_with_a_tall_row();
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let mut term = Terminal::new(TestBackend::new(100, 16)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, &mut app))
        .unwrap();
    let screen: String = {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert!(
        screen.contains("repo/a"),
        "selected row's head drawn:\n{screen}"
    );
    let buf = term.backend().buffer().clone();
    for (_, r) in &app.dashboard_focus_row_rects {
        for y in r.y..r.y + r.height {
            let line: String = (r.x..r.x + r.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            assert!(
                !line.trim().is_empty(),
                "click target over a blank line {y}:\n{screen}"
            );
        }
    }
}

/// Rows partly scrolled past the bottom are drawn clipped, and their click
/// target stops at the last drawn line.
#[tokio::test]
async fn click_targets_match_drawn_lines_after_scrolling() {
    let (mut app, repo, _, ids) = app();
    app.dashboard.focus_repo(repo);
    // Short screen: 5 three-line rows plus spacers don't fit.
    let mut term = Terminal::new(TestBackend::new(100, 14)).unwrap();
    let mut draw_small = |app: &mut App| {
        term.draw(|f| crate::app::render::draw_for_test(f, app))
            .unwrap();
        term.backend().buffer().clone()
    };
    draw_small(&mut app);
    for _ in 0..4 {
        press(&mut app, KeyCode::Char('j')).await;
    }
    let buf = draw_small(&mut app);
    assert_eq!(
        selected_ws(&app),
        app.selectable.last().and_then(|t| match t {
            SelectionTarget::Workspace(id) => Some(*id),
            _ => None,
        })
    );
    let last = app.selectable.len() - 1;
    let sel = match &app.selectable[last] {
        SelectionTarget::Workspace(id) => *id,
        _ => unreachable!(),
    };
    assert!(
        app.dashboard_focus_row_rects
            .iter()
            .any(|(id, _)| *id == sel),
        "scrolled to the selected row"
    );
    for (_, r) in &app.dashboard_focus_row_rects {
        for y in r.y..r.y + r.height {
            let line: String = (r.x..r.x + r.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            assert!(!line.trim().is_empty(), "target over blank line {y}");
        }
    }
    let _ = ids;
}

/// Unregistering the focused repo ends the focus instead of leaving stale
/// focus-only behavior over the list.
#[test]
fn unregistering_the_focused_repo_clears_focus() {
    let (mut app, _, other, _) = app();
    app.dashboard.focus_repo(other);
    app.store.remove_repo(other).unwrap();
    app.refresh().unwrap();
    draw(&mut app);
    assert_eq!(app.dashboard.focused_repo, None);
}

/// Leaving the focus view onto a folded repo selects its header so the
/// highlight stays visible.
#[tokio::test]
async fn leaving_into_a_folded_repo_selects_its_header() {
    let (mut app, repo, _, ids) = app();
    app.dashboard.folded.insert(repo.0 as u64, true);
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[2]));
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.selected_target(), Some(SelectionTarget::Repo(repo)));
}

/// The status filter narrows the focus view's rows and its nav order alike.
#[test]
fn the_status_filter_narrows_the_focus_view() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    app.dashboard.status_filter = Some(crate::ui::dashboard::status::Status::Question);
    let screen = draw(&mut app);
    assert!(app.selectable.is_empty());
    assert!(app.dashboard_focus_row_rects.is_empty());
    assert!(screen.contains("(no matching workspaces)"), "{screen}");
}

/// Unregistering the focused repo ends the focus on the next frame, so the
/// reply-only bar and focus-only keys don't linger over the list.
#[tokio::test]
async fn unregistering_the_focused_repo_ends_the_focus() {
    let (mut app, _, other, _) = app();
    app.dashboard.focus_repo(other);
    draw(&mut app);
    app.store.remove_repo(other).unwrap();
    app.refresh().unwrap();
    draw(&mut app);
    assert_eq!(app.dashboard.focused_repo, None);
    // `f` then focuses afresh instead of only clearing stale state.
    let repo = app.repos[0].id;
    app.dashboard.selection = Some(SelectionTarget::Repo(repo));
    press(&mut app, KeyCode::Char('f')).await;
    assert_eq!(app.dashboard.focused_repo, Some(repo));
}

/// `repo` with `d` and `e` tagged `parked` and a rule hiding that tag.
fn app_with_parked_group() -> (App, RepoId, Vec<WorkspaceId>) {
    let (mut app, repo, _, ids) = app();
    for id in [ids[3], ids[4]] {
        app.store.add_workspace_tag(id, "parked").unwrap();
    }
    app.store
        .set_repo_hide_rules(
            repo,
            &crate::data::hide_rules::HideRules::parse(Some("tag:parked")),
        )
        .unwrap();
    app.refresh().unwrap();
    app.dashboard.focus_repo(repo);
    (app, repo, ids)
}

fn parked(repo: RepoId) -> SelectionTarget {
    SelectionTarget::HiddenGroup {
        repo,
        rule: crate::data::hide_rules::HideRule::tag("parked"),
    }
}

/// A hidden tag group shows as a summary row after the repo's workspaces,
/// collapsed, exactly as under the repo's header on the list.
#[test]
fn hidden_groups_show_collapsed_in_the_focus_view() {
    let (mut app, repo, ids) = app_with_parked_group();
    let screen = draw(&mut app);
    assert!(screen.contains("#parked"), "group row drawn:\n{screen}");
    assert!(screen.contains("2 hidden"), "{screen}");
    assert!(!screen.contains("repo/d"), "members stay hidden:\n{screen}");
    assert_eq!(app.selectable.len(), 4, "three rows and the group");
    assert_eq!(app.selectable.last(), Some(&parked(repo)));
    assert!(!app.selectable.contains(&SelectionTarget::Workspace(ids[3])));
    assert_eq!(app.dashboard_focus_group_rects.len(), 1);
}

/// `l` / Enter expand the group in place and `h` collapses it — from the
/// group row or from one of its members, which hands the selection back to
/// the group row.
#[tokio::test]
async fn hidden_groups_expand_and_collapse_like_the_list() {
    let (mut app, repo, ids) = app_with_parked_group();
    draw(&mut app);
    app.dashboard.selection = Some(parked(repo));
    press(&mut app, KeyCode::Char('l')).await;
    let screen = draw(&mut app);
    assert!(
        screen.contains("repo/d") && screen.contains("repo/e"),
        "{screen}"
    );
    assert_eq!(
        &app.selectable[3..],
        &[
            parked(repo),
            SelectionTarget::Workspace(ids[3]),
            SelectionTarget::Workspace(ids[4]),
        ],
        "members follow their group row"
    );

    press(&mut app, KeyCode::Char('j')).await;
    assert_eq!(selected_ws(&app), Some(ids[3]));
    press(&mut app, KeyCode::Char('h')).await;
    assert_eq!(app.selected_target(), Some(parked(repo)));
    draw(&mut app);
    assert_eq!(app.selectable.len(), 4, "collapsed again");

    press(&mut app, KeyCode::Enter).await;
    draw(&mut app);
    assert_eq!(app.selectable.len(), 6, "Enter expands too");
    // `h` on an ordinary row in the focus view still does nothing.
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    press(&mut app, KeyCode::Char('h')).await;
    assert!(app.dashboard.folded.is_empty());
    assert_eq!(app.dashboard.focused_repo, Some(repo));
}

/// Clicking a group's row toggles it and selects it.
#[test]
fn clicking_a_hidden_group_toggles_it() {
    let (mut app, repo, _) = app_with_parked_group();
    draw(&mut app);
    let (group, r) = app.dashboard_focus_group_rects[0].clone();
    assert_eq!(
        focus_group_click_target(&app, &click(r.x, r.y)),
        Some(group.clone())
    );
    toggle_focus_group(&mut app, group.clone());
    assert!(app.dashboard.expanded_hidden.contains(&group));
    assert_eq!(app.selected_target(), Some(parked(repo)));
    toggle_focus_group(&mut app, group.clone());
    assert!(!app.dashboard.expanded_hidden.contains(&group));
}

/// The expansion is shared with the list: a group opened in the focus view
/// is open on the list too, as one piece of state.
#[tokio::test]
async fn group_expansion_carries_between_views() {
    let (mut app, repo, ids) = app_with_parked_group();
    draw(&mut app);
    app.dashboard.selection = Some(parked(repo));
    press(&mut app, KeyCode::Char('l')).await;
    press(&mut app, KeyCode::Esc).await;
    draw(&mut app);
    assert!(app.selectable.contains(&SelectionTarget::Workspace(ids[3])));
}

/// PgDn pages through a workspace taller than the screen to its last line
/// (on a terminal too narrow for the panel, so the row shows its prose),
/// and the selection holding still doesn't snap it back to the top.
#[tokio::test]
async fn pgdn_reaches_the_end_of_an_oversized_row() {
    let (mut app, repo, _, ids) = app_with_a_tall_row();
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            next: Some("STATE-AT-THE-END".into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let frame = |app: &mut App| -> String {
        let mut term = Terminal::new(TestBackend::new(99, 16)).unwrap();
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
    };
    let first = frame(&mut app);
    assert!(!first.contains("STATE-AT-THE-END"), "starts at the top");
    let mut seen_end = false;
    for _ in 0..20 {
        press(&mut app, KeyCode::PageDown).await;
        let screen = frame(&mut app);
        if screen.contains("STATE-AT-THE-END") {
            seen_end = true;
            break;
        }
    }
    assert!(seen_end, "the row's end is reachable");
    assert_eq!(selected_ws(&app), Some(ids[0]), "selection unchanged");
    // Paging back up returns to the row's head.
    for _ in 0..20 {
        press(&mut app, KeyCode::PageUp).await;
    }
    assert!(frame(&mut app).contains("repo/a"));
    assert_eq!(app.dashboard.focus_scroll, 0);
}

/// Moving the selection scrolls the new row into view even after a manual
/// scroll took the view elsewhere.
#[tokio::test]
async fn moving_the_selection_scrolls_it_back_into_view() {
    let (mut app, repo, _, ids) = app();
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let mut term = Terminal::new(TestBackend::new(120, 14)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, &mut app))
        .unwrap();
    press(&mut app, KeyCode::PageDown).await;
    press(&mut app, KeyCode::PageDown).await;
    term.draw(|f| crate::app::render::draw_for_test(f, &mut app))
        .unwrap();
    press(&mut app, KeyCode::Char('j')).await;
    term.draw(|f| crate::app::render::draw_for_test(f, &mut app))
        .unwrap();
    let sel = selected_ws(&app).unwrap();
    let r = app
        .dashboard_focus_row_rects
        .iter()
        .find(|(id, _)| *id == sel)
        .map(|(_, r)| *r);
    assert!(r.is_some_and(|r| r.y >= 4), "selected row on screen: {r:?}");
}

/// Two commits and two changed files (one binary-free), polled against
/// `base`.
/// Two commits and two changed files, polled against `base`.
pub(super) fn branch_lists(base: &str) -> std::sync::Arc<crate::git::BranchLists> {
    use crate::git::{BranchCommit, ChangedFile, DiffStats};
    std::sync::Arc::new(crate::git::BranchLists::new(
        base,
        vec![
            BranchCommit {
                hash: "a1b2c3d".into(),
                subject: "UNIQUE-COMMIT-SUBJECT".into(),
                stats: None,
            },
            BranchCommit {
                hash: "9f8e7d6".into(),
                subject: "earlier commit".into(),
                stats: None,
            },
        ],
        vec![
            ChangedFile {
                path: "src/zeta.rs".into(),
                old_path: None,
                stats: Some(DiffStats {
                    added: 3,
                    removed: 1,
                }),
            },
            ChangedFile {
                path: "src/alpha.rs".into(),
                old_path: None,
                stats: Some(DiffStats {
                    added: 40,
                    removed: 2,
                }),
            },
        ],
    ))
}

/// The focus view with `a` selected, a base branch set, and two commits and
/// two changed files cached for `a`.
fn app_with_branch_data() -> (App, RepoId, Vec<WorkspaceId>) {
    let (mut app, repo, _, ids) = app();
    app.store
        .set_repo_base_branch(repo, Some("origin/main"))
        .unwrap();
    app.refresh().unwrap();
    app.workspace_branch_lists
        .insert(ids[0], branch_lists("origin/main"));
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    (app, repo, ids)
}

#[tokio::test]
async fn the_focus_panel_shows_commits_on_4() {
    let (mut app, _, _) = app_with_branch_data();
    let screen = draw(&mut app);
    assert!(
        !screen.contains("UNIQUE-COMMIT-SUBJECT"),
        "on Next to start"
    );
    assert!(app.side_panel_rect.is_some(), "open to start with");

    press(&mut app, KeyCode::Char('4')).await;
    let screen = draw(&mut app);
    assert!(screen.contains("Commits 2"), "{screen}");
    assert!(screen.contains("Files 2"), "{screen}");
    assert!(screen.contains("repo/a vs main"), "{screen}");
    assert!(
        screen.contains("a1b2c3d  UNIQUE-COMMIT-SUBJECT"),
        "{screen}"
    );
    let panel = app.side_panel_rect.expect("panel drawn");
    for (_, r) in &app.dashboard_focus_row_rects {
        assert!(r.x + r.width <= panel.x, "rows stay left of the panel");
    }
}

#[tokio::test]
async fn number_keys_and_clicks_switch_tabs() {
    use crate::ui::dashboard::side_panel::PanelTab;
    let (mut app, _, _) = app_with_branch_data();
    press(&mut app, KeyCode::Char('5')).await;
    assert_eq!(app.dashboard.focus_panel.tab, PanelTab::Files);
    let screen = draw(&mut app);
    assert!(screen.contains("2 files  +43 −3"), "{screen}");
    let alpha = screen.find("src/alpha.rs").unwrap();
    let zeta = screen.find("src/zeta.rs").unwrap();
    assert!(alpha < zeta, "sorted by path:\n{screen}");

    let tabs: Vec<PanelTab> = app.side_panel_tab_rects.iter().map(|(t, _)| *t).collect();
    assert_eq!(tabs, PanelTab::ALL, "every tab is a target");
    let (tab, r) = app.side_panel_tab_rects[3];
    assert_eq!(tab, PanelTab::Commits);
    assert_eq!(panel_tab_click_target(&app, &click(r.x, r.y)), Some(tab));
    show_panel_tab(&mut app, tab);
    assert!(draw(&mut app).contains("UNIQUE-COMMIT-SUBJECT"));
    assert!(!app.dashboard.panel.open, "the list's panel is untouched");
}

/// The panel follows the selection, and a repo header or empty selection
/// gets an explanation instead of stale data.
#[tokio::test]
async fn the_panel_follows_the_selection() {
    let (mut app, _, ids) = app_with_branch_data();
    press(&mut app, KeyCode::Char('4')).await;
    draw(&mut app);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[1]));
    let screen = draw(&mut app);
    assert!(screen.contains("repo/b vs main"), "{screen}");
    assert!(!screen.contains("UNIQUE-COMMIT-SUBJECT"), "{screen}");
    assert!(
        screen.contains("loading"),
        "nothing polled for b yet:\n{screen}"
    );
}

/// Too narrow for rows and a panel: the panel stays hidden, and the rows
/// show one line of its tab's prose instead.
#[tokio::test]
async fn a_narrow_terminal_hides_the_panel() {
    let (mut app, _, ids) = app_with_branch_data();
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            goal: Some("UNIQUE-GOAL".into()),
            next: Some("UNIQUE-NEXT".into()),
            ..Default::default()
        },
    );
    let mut term = Terminal::new(TestBackend::new(90, 30)).unwrap();
    let mut screen = |app: &mut App| {
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
    };
    let s = screen(&mut app);
    assert!(app.side_panel_rect.is_none());
    assert!(s.contains("next    UNIQUE-NEXT"), "{s}");
    press(&mut app, KeyCode::Char('2')).await;
    let s = screen(&mut app);
    assert!(
        s.contains("goal    UNIQUE-GOAL") && !s.contains("UNIQUE-NEXT"),
        "{s}"
    );
}

/// Lists polled against a different base than the repo's current one are
/// never shown as its results: the panel reads as still loading.
#[tokio::test]
async fn lists_from_another_base_are_not_shown() {
    let (mut app, _, ids) = app_with_branch_data();
    app.workspace_branch_lists
        .insert(ids[0], branch_lists("origin/old-base"));
    press(&mut app, KeyCode::Char('4')).await;
    let screen = draw(&mut app);
    assert!(screen.contains("repo/a vs main"), "{screen}");
    assert!(!screen.contains("UNIQUE-COMMIT-SUBJECT"), "{screen}");
    assert!(screen.contains("loading"), "{screen}");
}

/// Changing the repo's Base branch drops everything measured against the
/// old one, so nothing stale shows while the next poll runs.
#[test]
fn changing_the_base_branch_drops_old_results() {
    let (mut app, repo, ids) = app_with_branch_data();
    app.workspace_ahead_of_base.insert(ids[0], 2);
    app.workspace_diff.insert(
        ids[0],
        crate::git::DiffStats {
            added: 1,
            removed: 1,
        },
    );
    app.diff_last_poll_ms.insert(ids[0], 1);
    crate::app::repo_setting::apply_repo_setting(
        &mut app,
        repo,
        crate::app::repo_setting::RepoSettingField::BaseBranch,
        "origin/develop",
    )
    .unwrap();
    assert!(!app.workspace_branch_lists.contains_key(&ids[0]));
    assert!(!app.workspace_ahead_of_base.contains_key(&ids[0]));
    assert!(!app.workspace_diff.contains_key(&ids[0]));
    assert!(
        !app.diff_last_poll_ms.contains_key(&ids[0]),
        "re-polled next tick"
    );
}

/// On a terminal too narrow to show the panel, Esc leaves the focus view.
#[tokio::test]
async fn esc_on_a_hidden_panel_leaves_focus() {
    let (mut app, _, _) = app_with_branch_data();
    let mut term = Terminal::new(TestBackend::new(90, 30)).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, &mut app))
        .unwrap();
    press(&mut app, KeyCode::Esc).await;
    assert_eq!(app.dashboard.focused_repo, None);
}

/// A binary file on the branch shows in Files rather than vanishing.
#[tokio::test]
async fn binary_files_show_in_the_files_tab() {
    let (mut app, _, ids) = app_with_branch_data();
    let mut lists = std::sync::Arc::unwrap_or_clone(branch_lists("origin/main"));
    lists.files = vec![crate::git::ChangedFile {
        path: "assets/logo.png".into(),
        old_path: None,
        stats: None,
    }];
    app.workspace_branch_lists.insert(ids[0], lists.into());
    press(&mut app, KeyCode::Char('5')).await;
    let screen = draw(&mut app);
    assert!(screen.contains("assets/logo.png"), "{screen}");
    assert!(screen.contains("1 binary"), "{screen}");
}

/// Index of the first drawn line containing `needle`.
fn line_of(screen: &str, needle: &str) -> usize {
    screen
        .lines()
        .position(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("`{needle}` not drawn:\n{screen}"))
}

fn set_lifecycles(app: &mut App, ids: &[WorkspaceId], lcs: &[Option<BranchLifecycle>]) {
    for (id, lc) in ids.iter().zip(lcs) {
        match lc {
            Some(lc) => app.pr_lifecycle.insert(*id, *lc),
            None => app.pr_lifecycle.remove(id),
        };
    }
}

#[tokio::test]
async fn sections_follow_pr_lifecycle() {
    use BranchLifecycle::*;
    let (mut app, repo, _, ids) = app();
    set_lifecycles(
        &mut app,
        &ids,
        &[
            Some(PrOpen),
            None,
            Some(PrDraft),
            Some(PrMerged),
            Some(PrConflicted),
        ],
    );
    app.dashboard.focus_repo(repo);
    let s = draw(&mut app);
    let (no_pr, draft, open, done) = (
        line_of(&s, "━━ No PR · 1"),
        line_of(&s, "━━ Draft · 1"),
        line_of(&s, "━━ Open · 2"),
        line_of(&s, "━━ Done · 1"),
    );
    assert!(no_pr < draft && draft < open && open < done, "{s}");
    let b = line_of(&s, "repo/b");
    assert!(no_pr < b && b < draft, "{s}");
    let e = line_of(&s, "repo/e");
    assert!(open < e && e < done, "{s}");
}

#[tokio::test]
async fn empty_sections_are_omitted() {
    let (mut app, repo, _, ids) = app();
    set_lifecycles(&mut app, &ids, &[Some(BranchLifecycle::PrOpen); 5]);
    app.dashboard.focus_repo(repo);
    let s = draw(&mut app);
    assert!(s.contains("━━ Open · 5"), "{s}");
    for other in ["No PR", "Draft", "Done"] {
        assert!(!s.contains(&format!("━━ {other}")), "{s}");
    }
}

#[tokio::test]
async fn unpolled_rows_sit_under_no_pr() {
    let (mut app, repo, _, _) = app();
    app.dashboard.focus_repo(repo);
    let s = draw(&mut app);
    assert!(s.contains("━━ No PR · 5"), "{s}");
    assert!(s.contains("no PR"), "{s}");
    assert!(app.dashboard_pr_rects.is_empty(), "no PR chip to click");
}

#[tokio::test]
async fn headers_are_not_selectable() {
    use BranchLifecycle::*;
    let (mut app, repo, _, ids) = app();
    set_lifecycles(
        &mut app,
        &ids,
        &[Some(PrOpen), None, Some(PrDraft), Some(PrMerged), None],
    );
    app.dashboard.focus_repo(repo);
    draw(&mut app);
    assert_eq!(app.selectable.len(), 5);
    assert!(
        app.selectable
            .iter()
            .all(|t| matches!(t, SelectionTarget::Workspace(_)))
    );
    // No PR holds b and e; j from the second lands on the Draft row.
    app.select_index(1);
    draw(&mut app);
    press(&mut app, KeyCode::Char('j')).await;
    draw(&mut app);
    assert_eq!(selected_ws(&app), Some(ids[2]));
    assert_eq!(app.focus_section_starts, vec![0, 2, 3, 4]);
}

#[tokio::test]
async fn focus_view_does_not_pin_blocked() {
    use crate::activity::events::{EventKind, EventSnapshot, StopReason, WorkspaceEvents};
    use crate::ui::dashboard::sort::SortMode;
    let (mut app, repo, _, ids) = app();
    set_lifecycles(&mut app, &ids, &[Some(BranchLifecycle::PrMerged); 5]);
    let now = crate::util::time::now_ms();
    let latest = |ago_ms: i64| {
        Some(EventSnapshot {
            kind: EventKind::AssistantText,
            display: "x".into(),
            timestamp_ms: now - ago_ms,
        })
    };
    // c is blocked but older than d; pinning would float it above d.
    app.workspace_events.insert(
        ids[2],
        WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            last_assistant_text: Some("Which approach?".into()),
            latest: latest(600_000),
            ..Default::default()
        },
    );
    app.workspace_events.insert(
        ids[3],
        WorkspaceEvents {
            latest: latest(5_000),
            ..Default::default()
        },
    );
    app.dashboard.sort_mode = SortMode::Recency;
    app.dashboard.blocked_pin_max_age_secs = 3600;
    app.dashboard.focus_repo(repo);
    let s = draw(&mut app);
    let (d, c) = (line_of(&s, "repo/d"), line_of(&s, "repo/c"));
    assert!(line_of(&s, "━━ Done") < d && d < c, "c is not pinned:\n{s}");
}

#[tokio::test]
async fn selection_follows_a_workspace_across_sections() {
    use BranchLifecycle::*;
    let (mut app, repo, _, ids) = app();
    set_lifecycles(&mut app, &ids, &[Some(PrOpen); 5]);
    set_lifecycles(&mut app, &ids[2..3], &[Some(PrDraft)]);
    app.dashboard.focus_repo(repo);
    draw(&mut app);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[2]));
    draw(&mut app);
    set_lifecycles(&mut app, &ids[2..3], &[Some(PrOpen)]);
    let s = draw(&mut app);
    assert_eq!(selected_ws(&app), Some(ids[2]));
    assert!(!s.contains("━━ Draft"), "{s}");
    assert!(line_of(&s, "━━ Open · 5") < line_of(&s, "repo/c"), "{s}");
}

#[tokio::test]
async fn hidden_groups_trail_the_sections() {
    use BranchLifecycle::*;
    let (mut app, repo, _, ids) = app();
    app.store
        .set_repo_hide_rules(
            repo,
            &crate::data::hide_rules::HideRules::parse(Some("pr:merged")),
        )
        .unwrap();
    app.refresh().unwrap();
    set_lifecycles(
        &mut app,
        &ids,
        &[
            Some(PrOpen),
            None,
            Some(PrDraft),
            Some(PrMerged),
            Some(PrOpen),
        ],
    );
    app.dashboard.focus_repo(repo);
    let s = draw(&mut app);
    assert!(line_of(&s, "hidden") > line_of(&s, "━━ Open"), "{s}");
    assert!(!s.contains("━━ Done"), "{s}");
    assert_eq!(app.focus_section_starts, vec![0, 1, 2, 4]);
}

/// Selection (by workspace id) after each key press, drawing in between.
async fn press_drawn(app: &mut App, code: KeyCode) -> Option<WorkspaceId> {
    press(app, code).await;
    draw(app);
    selected_ws(app)
}

/// Sections: No PR [b], Draft [c], Open [a, e], Done [d].
fn app_with_sections() -> (App, Vec<WorkspaceId>) {
    use BranchLifecycle::*;
    let (mut app, repo, _, ids) = app();
    set_lifecycles(
        &mut app,
        &ids,
        &[
            Some(PrOpen),
            None,
            Some(PrDraft),
            Some(PrMerged),
            Some(PrConflicted),
        ],
    );
    app.dashboard.focus_repo(repo);
    draw(&mut app);
    (app, ids)
}

#[tokio::test]
async fn close_brace_jumps_to_next_section() {
    let (mut app, ids) = app_with_sections();
    let (a, b, c, d) = (ids[0], ids[1], ids[2], ids[3]);
    app.select_index(
        app.selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(b))
            .unwrap(),
    );
    draw(&mut app);
    assert_eq!(press_drawn(&mut app, KeyCode::Char('}')).await, Some(c));
    assert_eq!(press_drawn(&mut app, KeyCode::Char('}')).await, Some(a));
    assert_eq!(press_drawn(&mut app, KeyCode::Char('}')).await, Some(d));
    assert_eq!(press_drawn(&mut app, KeyCode::Char('}')).await, Some(d));
}

#[tokio::test]
async fn open_brace_goes_to_section_top_then_previous() {
    let (mut app, ids) = app_with_sections();
    let (a, b, c, e) = (ids[0], ids[1], ids[2], ids[4]);
    app.select_index(
        app.selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(e))
            .unwrap(),
    );
    draw(&mut app);
    assert_eq!(press_drawn(&mut app, KeyCode::Char('{')).await, Some(a));
    assert_eq!(press_drawn(&mut app, KeyCode::Char('{')).await, Some(c));
    assert_eq!(press_drawn(&mut app, KeyCode::Char('{')).await, Some(b));
    assert_eq!(press_drawn(&mut app, KeyCode::Char('{')).await, Some(b));
}

#[tokio::test]
async fn section_keys_with_one_section_are_noops() {
    let (mut app, repo, _, ids) = app();
    set_lifecycles(&mut app, &ids, &[Some(BranchLifecycle::PrOpen); 5]);
    app.dashboard.focus_repo(repo);
    draw(&mut app);
    app.select_index(2);
    draw(&mut app);
    assert_eq!(
        press_drawn(&mut app, KeyCode::Char('}')).await,
        Some(ids[2])
    );
    assert_eq!(
        press_drawn(&mut app, KeyCode::Char('{')).await,
        Some(ids[0])
    );
    assert_eq!(
        press_drawn(&mut app, KeyCode::Char('{')).await,
        Some(ids[0])
    );

    // No workspaces at all: nothing to select, nothing to panic on.
    let store = Store::open_in_memory().unwrap();
    let rid = store
        .add_repo(std::path::Path::new("/tmp/e"), "e", "")
        .unwrap();
    let mut empty = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    empty.view = crate::ui::View::Dashboard;
    empty.refresh().unwrap();
    empty.dashboard.focus_repo(rid);
    draw(&mut empty);
    press(&mut empty, KeyCode::Char('}')).await;
    press(&mut empty, KeyCode::Char('{')).await;
    assert!(empty.selectable.is_empty());
}

#[tokio::test]
async fn section_starts_follow_the_filter() {
    let (mut app, ids) = app_with_sections();
    for id in [ids[0], ids[1], ids[3], ids[4]] {
        app.store.add_workspace_tag(id, "keep").unwrap();
    }
    app.refresh().unwrap();
    // The refresh above drops the seeded lifecycles' polling state.
    let (a, b, c, d, e) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
    app.pr_lifecycle.insert(a, BranchLifecycle::PrOpen);
    app.pr_lifecycle.insert(c, BranchLifecycle::PrDraft);
    app.pr_lifecycle.insert(d, BranchLifecycle::PrMerged);
    app.pr_lifecycle.insert(e, BranchLifecycle::PrConflicted);
    app.pr_lifecycle.remove(&b);
    app.dashboard.filter = Some("#keep".into());
    draw(&mut app);
    app.select_index(
        app.selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(ids[1]))
            .unwrap(),
    );
    draw(&mut app);
    // Draft is filtered out, so `}` from b lands on the Open section. A
    // live `/` buffer types the key instead, so call the jump directly.
    super::super::dashboard::jump_focus_section(&mut app, true);
    draw(&mut app);
    assert_eq!(selected_ws(&app), Some(ids[0]));
}

#[tokio::test]
async fn section_keys_do_nothing_outside_focus() {
    let (mut app, _, _, _) = app();
    draw(&mut app);
    let before = app.dashboard.selection.clone();
    press(&mut app, KeyCode::Char('}')).await;
    press(&mut app, KeyCode::Char('{')).await;
    assert_eq!(app.dashboard.selection, before);
}

#[tokio::test]
async fn jump_reveals_the_section_header() {
    use BranchLifecycle::*;
    let store = Store::open_in_memory().unwrap();
    let names: Vec<String> = (0..12).map(|i| format!("w{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let ids = seed_workspaces(&store, &refs);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = crate::ui::View::Dashboard;
    app.refresh().unwrap();
    let repo = app.repos[0].id;
    let lcs: Vec<_> = (0..12)
        .map(|i| if i < 4 { None } else { Some(PrMerged) })
        .collect();
    set_lifecycles(&mut app, &ids, &lcs);
    app.dashboard.focus_repo(repo);
    let draw20 = |app: &mut App| {
        let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
        term.draw(|f| crate::app::render::draw_for_test(f, app))
            .unwrap();
        let buf = term.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    // Last row, then `{` back to the Done section's top, which scrolls up
    // from below the fold; its header must come with it.
    app.select_index(app.selectable.len() - 1);
    draw20(&mut app);
    press(&mut app, KeyCode::Char('{')).await;
    draw20(&mut app);
    let s = draw20(&mut app);
    assert!(s.contains("━━ Done · 8"), "{s}");
    assert!(s.contains("repo/w04"), "{s}");
}

/// At 80 columns the focus view still shows its section headers and every
/// branch name whole, wrapped within the branch column.
#[tokio::test]
async fn narrow_focus_view_keeps_sections_and_whole_branches() {
    use BranchLifecycle::*;
    let store = Store::open_in_memory().unwrap();
    let names = [
        "short",
        "a-considerably-longer-workspace-name-that-has-to-wrap",
        "another-fairly-long-name-for-the-branch-column",
    ];
    let ids = seed_workspaces(&store, &names);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.view = crate::ui::View::Dashboard;
    app.refresh().unwrap();
    let repo = app.repos.iter().find(|r| r.name == "repo").unwrap().id;
    set_lifecycles(&mut app, &ids, &[None, Some(PrOpen), Some(PrMerged)]);
    app.dashboard.selection = Some(SelectionTarget::Repo(repo));
    let mut term = Terminal::new(TestBackend::new(80, 60)).unwrap();
    let mut draw80 = |app: &mut App| {
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
    };
    draw80(&mut app);
    press(&mut app, KeyCode::Char('f')).await;
    draw80(&mut app);
    let lines = draw80(&mut app);
    let screen = lines.join("\n");
    for header in ["━━ No PR · 1", "━━ Open · 1", "━━ Done · 1"] {
        assert!(screen.contains(header), "{header:?}:\n{screen}");
    }
    // The word starting at `x` on line `y`, if one starts there.
    let word_at = |y: usize, x: usize| -> Option<String> {
        let cells: Vec<char> = lines[y].chars().collect();
        (x < cells.len() && cells[x] != ' ' && (x == 0 || cells[x - 1] == ' '))
            .then(|| cells[x..].iter().take_while(|c| **c != ' ').collect())
    };
    for name in names {
        let branch = format!("repo/{name}");
        let head = format!("repo/{}", &name[..name.len().min(8)]);
        let y = line_of(&screen, &head);
        let x = lines[y].chars().collect::<String>()[..lines[y].find(&head).unwrap()]
            .chars()
            .count();
        let mut joined = word_at(y, x).unwrap();
        for next in y + 1..lines.len() {
            match word_at(next, x) {
                Some(w) if branch.starts_with(&format!("{joined}{w}")) => joined.push_str(&w),
                _ => break,
            }
        }
        assert_eq!(joined, branch, "\n{screen}");
    }
}

#[tokio::test]
async fn natural_order_drops_headers_and_keeps_list_order() {
    let (mut app, _) = app_with_sections();
    app.dashboard.focus_group_by_pr = false;
    let s = draw(&mut app);
    assert!(!s.contains("━━ No PR") && !s.contains("━━ Open"), "{s}");
    let lines: Vec<usize> = ["repo/a", "repo/b", "repo/c", "repo/d", "repo/e"]
        .iter()
        .map(|n| line_of(&s, n))
        .collect();
    assert!(lines.windows(2).all(|w| w[0] < w[1]), "list order:\n{s}");
    assert_eq!(app.focus_section_starts, vec![0]);
}

#[tokio::test]
async fn natural_order_pins_freshly_blocked_rows() {
    use crate::activity::events::{EventKind, EventSnapshot, StopReason, WorkspaceEvents};
    use crate::ui::dashboard::sort::SortMode;
    let (mut app, repo, _, ids) = app();
    let now = crate::util::time::now_ms();
    let latest = |ago_ms: i64| {
        Some(EventSnapshot {
            kind: EventKind::AssistantText,
            display: "x".into(),
            timestamp_ms: now - ago_ms,
        })
    };
    app.workspace_events.insert(
        ids[2],
        WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            last_assistant_text: Some("Which approach?".into()),
            latest: latest(600_000),
            ..Default::default()
        },
    );
    app.workspace_events.insert(
        ids[3],
        WorkspaceEvents {
            latest: latest(5_000),
            ..Default::default()
        },
    );
    app.dashboard.sort_mode = SortMode::Recency;
    app.dashboard.blocked_pin_max_age_secs = 3600;
    app.dashboard.focus_group_by_pr = false;
    app.dashboard.focus_repo(repo);
    let s = draw(&mut app);
    assert!(
        line_of(&s, "repo/c") < line_of(&s, "repo/d"),
        "c pinned:\n{s}"
    );
}

#[tokio::test]
async fn natural_order_braces_go_to_top_and_hidden_groups() {
    let (mut app, repo, _) = app_with_parked_group();
    app.dashboard.focus_group_by_pr = false;
    app.dashboard.focus_repo(repo);
    draw(&mut app);
    let first = app.selectable[0].clone();
    press(&mut app, KeyCode::Char('j')).await;
    draw(&mut app);
    press(&mut app, KeyCode::Char('{')).await;
    assert_eq!(app.selected_target(), Some(first.clone()), "{{ goes to top");
    draw(&mut app);
    press(&mut app, KeyCode::Char('}')).await;
    assert_eq!(
        app.selected_target(),
        Some(parked(repo)),
        "}} goes to hidden groups"
    );
}
