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
    for chunk in ["Single-repo dashboard view", "as it needs"] {
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
    let (mut app, repo, _, ids) = app();
    let goal = "word ".repeat(400);
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            goal: Some(goal),
            ..Default::default()
        },
    );
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
    assert!(screen.contains("goal    word word"), "{screen}");
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

/// PgDn pages through a workspace taller than the screen to its last line,
/// and the selection holding still doesn't snap it back to the top.
#[tokio::test]
async fn pgdn_reaches_the_end_of_an_oversized_row() {
    let (mut app, repo, _, ids) = app();
    app.recaps.insert(
        ids[0],
        crate::data::store::WorkspaceRecap {
            goal: Some(format!("{} END-OF-GOAL", "word ".repeat(400))),
            state: Some("STATE-AT-THE-END".into()),
            ..Default::default()
        },
    );
    app.dashboard.focus_repo(repo);
    app.dashboard.selection = Some(SelectionTarget::Workspace(ids[0]));
    let frame = |app: &mut App| -> String {
        let mut term = Terminal::new(TestBackend::new(100, 16)).unwrap();
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
        if screen.contains("END-OF-GOAL") && screen.contains("STATE-AT-THE-END") {
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
