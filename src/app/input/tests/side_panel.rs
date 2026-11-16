//! The side panel outside the repo focus view: on the dashboard list and
//! in the attached view.
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
    let mut lists = std::sync::Arc::unwrap_or_clone(branch_lists("origin/main"));
    lists.commits = (0..80)
        .map(|i| crate::git::BranchCommit {
            hash: format!("{i:07}"),
            subject: format!("commit {i}"),
        })
        .collect();
    app.workspace_branch_lists.insert(ids[0], lists.into());
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

/// An attached workspace with a base branch set and lists cached for it.
fn attached_app() -> (App, crate::ui::split::AttachTarget) {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    app.refresh().unwrap();
    let repo = app.repos.iter().find(|r| r.name == "scratch").unwrap().id;
    app.store
        .set_repo_base_branch(repo, Some("origin/main"))
        .unwrap();
    app.refresh().unwrap();
    app.workspace_branch_lists
        .insert(ws_id, branch_lists("origin/main"));
    let target = test_target(&app, ws_id);
    (app, target)
}

async fn chord(app: &mut App, target: crate::ui::split::AttachTarget, c: char) {
    handle_key_attached(
        app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(
        app,
        target,
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_c_opens_the_panel_beside_the_agent_pane() {
    let (mut app, target) = attached_app();
    draw(&mut app);
    assert!(app.side_panel_rect.is_none(), "closed by default");
    let full = app.attached_pane_rects[0].1;

    chord(&mut app, target, 'c').await;
    assert!(!app.leader_pending);
    let screen = draw(&mut app);
    assert!(screen.contains("Commits 2"), "{screen}");
    assert!(screen.contains("UNIQUE-COMMIT-SUBJECT"), "{screen}");
    let panel = app.side_panel_rect.expect("panel drawn");
    let pane = app.attached_pane_rects[0].1;
    assert!(pane.x + pane.width <= panel.x, "pane left of the panel");
    assert!(pane.width < full.width, "the pane narrows for the panel");
    assert!(!app.dashboard.panel.open, "the dashboard's panel is apart");

    chord(&mut app, target, 'C').await;
    let screen = draw(&mut app);
    assert!(
        screen.contains("2 files  +43 −3"),
        "C switches tab:\n{screen}"
    );

    // A click on the Commits tab shows it in the attached panel.
    let (tab, r) = app.side_panel_tab_rects[0];
    assert_eq!(tab, crate::ui::dashboard::side_panel::PanelTab::Commits);
    handle_mouse(
        &mut app,
        &shared_app(),
        MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: r.x,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await;
    assert_eq!(app.attached_panel.tab, tab);
    assert!(draw(&mut app).contains("UNIQUE-COMMIT-SUBJECT"));

    chord(&mut app, target, 'c').await;
    draw(&mut app);
    assert!(app.side_panel_rect.is_none(), "c closes it");
    assert_eq!(app.attached_pane_rects[0].1, full, "the pane gets it back");
}

/// The wheel over the attached panel scrolls the panel, not the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_wheel_over_the_attached_panel_scrolls_it() {
    let (mut app, target) = attached_app();
    let mut lists = std::sync::Arc::unwrap_or_clone(branch_lists("origin/main"));
    lists.commits = (0..80)
        .map(|i| crate::git::BranchCommit {
            hash: format!("{i:07}"),
            subject: format!("commit {i}"),
        })
        .collect();
    app.workspace_branch_lists
        .insert(target.workspace_id, lists.into());
    chord(&mut app, target, 'c').await;
    draw(&mut app);
    let panel = app.side_panel_rect.unwrap();
    handle_mouse(
        &mut app,
        &shared_app(),
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: panel.x + 3,
            row: panel.y + 5,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await;
    assert_eq!(app.attached_panel.scroll, 3);
    assert_eq!(app.dashboard.panel.scroll, 0);
}

async fn key(app: &mut App, code: KeyCode) {
    dispatch_key(app, &shared_app(), KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
}

fn diff_modal(app: &App) -> Option<&crate::ui::modal::FileDiffModal> {
    match &app.modal {
        Some(crate::ui::modal::Modal::FileDiff(m)) => Some(m),
        _ => None,
    }
}

/// `→` focuses the Files list; `j` / `k` move the cursor instead of the
/// rows, and Enter opens the cursor's file in the diff viewer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn right_focuses_the_files_and_enter_opens_a_diff() {
    let (mut app, _, ids) = app();
    draw(&mut app);
    key(&mut app, KeyCode::Right).await;
    let screen = draw(&mut app);
    assert!(app.dashboard.panel.is_focused());
    assert!(screen.contains("2 files"), "opens on Files:\n{screen}");

    key(&mut app, KeyCode::Char('j')).await;
    draw(&mut app);
    assert_eq!(app.dashboard.panel.file_cursor, 1);
    assert_eq!(
        app.dashboard.selection,
        Some(SelectionTarget::Workspace(ids[0])),
        "the rows stay put"
    );
    key(&mut app, KeyCode::Char('j')).await;
    draw(&mut app);
    assert_eq!(app.dashboard.panel.file_cursor, 1, "clamped to the list");

    key(&mut app, KeyCode::Enter).await;
    let m = diff_modal(&app).expect("diff viewer open");
    assert_eq!((m.workspace_id, m.index), (ids[0], 1));
    assert_eq!(m.file().unwrap().path, "src/zeta.rs");
    assert_eq!(m.base, "origin/main");
}

/// Esc gives focus back to the rows and leaves the panel open; a key the
/// panel has no use for drops focus and does what it would without it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn focus_goes_back_on_esc_or_any_other_key() {
    let (mut app, _, ids) = app();
    draw(&mut app);
    key(&mut app, KeyCode::Right).await;
    draw(&mut app);
    key(&mut app, KeyCode::Esc).await;
    assert!(!app.dashboard.panel.is_focused());
    assert!(app.dashboard.panel.open, "Esc only unfocuses");

    key(&mut app, KeyCode::Right).await;
    draw(&mut app);
    key(&mut app, KeyCode::Char('1')).await;
    assert!(!app.dashboard.panel.is_focused());
    assert_eq!(
        app.dashboard.panel.tab,
        crate::ui::dashboard::side_panel::PanelTab::Commits,
        "the key still switched the tab"
    );
    key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(
        app.dashboard.selection,
        Some(SelectionTarget::Workspace(ids[1])),
        "j moves the rows again"
    );
}

/// A click on a file opens its diff; in the viewer `]` steps to the next
/// file and the panel's cursor follows, and Esc closes it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clicking_a_file_opens_it_and_the_viewer_steps_and_closes() {
    let (mut app, _, _) = app();
    key(&mut app, KeyCode::Char('2')).await;
    draw(&mut app);
    let &(index, r) = app
        .side_panel_file_rects
        .iter()
        .find(|(i, _)| *i == 0)
        .expect("file rows are click targets");
    handle_mouse(
        &mut app,
        &shared_app(),
        MouseEvent {
            kind: MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: r.x + 1,
            row: r.y,
            modifiers: KeyModifiers::NONE,
        },
    )
    .await;
    assert_eq!(diff_modal(&app).map(|m| m.index), Some(index));
    let screen = draw(&mut app);
    assert!(
        screen.contains("src/alpha.rs") && screen.contains("1/2"),
        "{screen}"
    );
    assert!(screen.contains("minimap"), "{screen}");

    key(&mut app, KeyCode::Char(']')).await;
    assert_eq!(diff_modal(&app).map(|m| m.index), Some(1));
    assert_eq!(app.dashboard.panel.file_cursor, 1);
    // Summon keys belong to the viewer while it is open.
    key(&mut app, KeyCode::Char('s')).await;
    assert!(diff_modal(&app).is_some());

    key(&mut app, KeyCode::Esc).await;
    assert!(app.modal.is_none());
    assert!(app.dashboard.panel.open);
}

/// `Ctrl-x f` opens the attached panel on Files with focus; bare keys then
/// move the cursor rather than reaching the agent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_x_f_focuses_the_attached_files() {
    let (mut app, target) = attached_app();
    app.view = crate::ui::View::Attached(crate::ui::split::AttachedState::single(target));
    draw(&mut app);
    chord(&mut app, target, 'f').await;
    draw(&mut app);
    assert!(app.attached_panel.is_focused());
    assert!(!app.dashboard.panel.open);
    key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(app.attached_panel.file_cursor, 1);
    key(&mut app, KeyCode::Enter).await;
    let m = diff_modal(&app).expect("diff viewer open");
    assert_eq!((m.workspace_id, m.index), (target.workspace_id, 1));
}

/// `o` on a file the worktree doesn't have says so: as an error card from
/// the focused list, and on the viewer's hint row while it stays open,
/// cleared by the next key.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn o_on_a_missing_file_explains_itself() {
    let (mut app, _, _) = app();
    draw(&mut app);
    key(&mut app, KeyCode::Right).await;
    draw(&mut app);
    key(&mut app, KeyCode::Char('o')).await;
    match &app.modal {
        Some(crate::ui::modal::Modal::Error { message }) => {
            assert!(message.contains("src/alpha.rs is deleted"), "{message}")
        }
        other => panic!("expected an error card, got {other:?}"),
    }
    key(&mut app, KeyCode::Esc).await;

    key(&mut app, KeyCode::Enter).await;
    key(&mut app, KeyCode::Char('o')).await;
    let m = diff_modal(&app).expect("the viewer stays open");
    assert!(m.notice.as_deref().is_some_and(|n| n.contains("deleted")));
    let screen = draw(&mut app);
    assert!(screen.contains("nothing to open"), "{screen}");
    key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(diff_modal(&app).unwrap().notice, None);
}

fn left(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// Dragging the panel's left edge resizes it, on both surfaces, and the
/// width is kept for the next run.
#[tokio::test]
async fn dragging_the_edge_resizes_the_panel() {
    use crossterm::event::MouseButton;
    let (mut app, _, _) = app();
    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    let before = app.side_panel_rect.unwrap();
    let right = before.x + before.width;
    let shared = shared_app();

    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), before.x, 10),
    )
    .await;
    assert!(app.dashboard.panel.resizing);
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Drag(MouseButton::Left), before.x - 20, 12),
    )
    .await;
    draw(&mut app);
    let after = app.side_panel_rect.unwrap();
    assert_eq!(after.width, before.width + 20);
    assert_eq!(after.x + after.width, right, "the right edge stays put");

    // Far past the rows' minimum: clamped, and the clamped width is kept.
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Drag(MouseButton::Left), 0, 12),
    )
    .await;
    draw(&mut app);
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Up(MouseButton::Left), 0, 12),
    )
    .await;
    let clamped = app.side_panel_rect.unwrap().width;
    assert_eq!(clamped, 140 - 41);
    assert!(!app.dashboard.panel.resizing);
    assert_eq!(app.attached_panel.width, Some(clamped), "both surfaces");
    assert_eq!(
        app.store.get_setting("side_panel_width").unwrap(),
        Some(clamped.to_string())
    );
}

/// A press inside the panel, off its edge, doesn't start a drag.
#[tokio::test]
async fn a_press_inside_the_panel_is_not_a_drag() {
    use crossterm::event::MouseButton;
    let (mut app, _, _) = app();
    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    let panel = app.side_panel_rect.unwrap();
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), panel.x + 5, 10),
    )
    .await;
    assert!(!app.dashboard.panel.resizing);
}

/// The run loop coalesces events between draws: a release right after a
/// drag keeps the drag's width, clamped, without a draw in between.
#[tokio::test]
async fn a_release_without_a_draw_keeps_the_dragged_width() {
    use crossterm::event::MouseButton;
    let (mut app, _, _) = app();
    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    let r = app.side_panel_rect.unwrap();
    let shared = shared_app();
    for (kind, col) in [
        (MouseEventKind::Down(MouseButton::Left), r.x),
        (MouseEventKind::Drag(MouseButton::Left), r.x - 10),
        (MouseEventKind::Up(MouseButton::Left), r.x - 20),
    ] {
        handle_mouse(&mut app, &shared, left(kind, col, 10)).await;
    }
    assert_eq!(app.dashboard.panel.width, Some(r.width + 20));
    assert_eq!(
        app.store.get_setting("side_panel_width").unwrap(),
        Some((r.width + 20).to_string())
    );
}

/// A modal opening mid-drag, or a press after a release that never
/// arrived, ends the drag rather than resizing the panel.
#[tokio::test]
async fn a_modal_or_a_fresh_press_cancels_the_drag() {
    use crossterm::event::MouseButton;
    let (mut app, _, _) = app();
    press(&mut app, KeyCode::Char('c')).await;
    draw(&mut app);
    let r = app.side_panel_rect.unwrap();
    let shared = shared_app();
    let down = |col| left(MouseEventKind::Down(MouseButton::Left), col, 10);
    let drag = |col| left(MouseEventKind::Drag(MouseButton::Left), col, 10);

    handle_mouse(&mut app, &shared, down(r.x)).await;
    app.modal = Some(crate::ui::modal::Modal::Error {
        message: "x".into(),
    });
    handle_mouse(&mut app, &shared, drag(r.x - 10)).await;
    assert!(!app.dashboard.panel.resizing);
    assert_eq!(app.dashboard.panel.width, None);
    app.modal = None;

    handle_mouse(&mut app, &shared, down(r.x)).await;
    handle_mouse(&mut app, &shared, down(r.x + 5)).await;
    assert!(!app.dashboard.panel.resizing, "the lost release is dropped");
    handle_mouse(&mut app, &shared, drag(r.x - 10)).await;
    assert_eq!(app.dashboard.panel.width, None);
}

/// Switching views mid-drag drops the drag on both surfaces.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_view_switch_cancels_the_drag() {
    use crossterm::event::MouseButton;
    let (mut app, target) = attached_app();
    chord(&mut app, target, 'c').await;
    draw(&mut app);
    let r = app.side_panel_rect.unwrap();
    let shared = shared_app();
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Down(MouseButton::Left), r.x, 10),
    )
    .await;
    assert!(app.attached_panel.resizing);

    app.view = crate::ui::View::Dashboard;
    app.dashboard.panel.open = true;
    draw(&mut app);
    handle_mouse(
        &mut app,
        &shared,
        left(MouseEventKind::Up(MouseButton::Left), r.x - 10, 10),
    )
    .await;
    assert!(!app.attached_panel.resizing && !app.dashboard.panel.resizing);
    assert_eq!(app.attached_panel.width, None);
}
