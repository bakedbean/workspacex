//! Project-manager pane: visibility, focus swapping, digest, filter.

use super::*;
use crate::data::store::Store;
use crate::test_support::EnvGuard;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use super::common::*;
use crossterm::event::{KeyEvent, KeyModifiers};

#[test]
fn app_initializes_pm_state_off() {
    let store = Store::open_in_memory().unwrap();
    let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(!app.pm_visible);
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
}

#[test]
fn dashboard_renders_full_area_when_pm_hidden() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(!app.pm_visible);
    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    let rendered = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!rendered.contains("Project Manager"), "{rendered}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tab_swaps_focus_when_pm_visible() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.pm_visible = true;
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn esc_returns_focus_to_dashboard() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.pm_visible = true;
    app.focus = crate::ui::PaneFocus::ProjectManager;
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tab_no_op_when_pm_hidden() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(!app.pm_visible);
    handle_key_dashboard(
        &mut app,
        KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p_toggles_digest_and_focus_without_spawning() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(!app.pm_visible);
    press_key(&mut app, KeyCode::Char('p')).await;
    assert!(app.pm_visible);
    assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
    press_key(&mut app, KeyCode::Char('p')).await;
    assert!(!app.pm_visible);
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn digest_jk_moves_selection_and_clamps() {
    let mut app = test_app_with_two_ready_workspaces();
    press_key(&mut app, KeyCode::Char('p')).await;
    assert_eq!(app.pm_digest_selected, 0);
    press_key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(app.pm_digest_selected, 1);
    press_key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(app.pm_digest_selected, 1, "clamped at last card");
    press_key(&mut app, KeyCode::Char('k')).await;
    assert_eq!(app.pm_digest_selected, 0);
    press_key(&mut app, KeyCode::Char('k')).await;
    assert_eq!(app.pm_digest_selected, 0, "clamped at first card");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn digest_q_and_esc_behavior() {
    let mut app = test_app_with_two_ready_workspaces();
    press_key(&mut app, KeyCode::Char('p')).await;
    press_key(&mut app, KeyCode::Esc).await;
    assert!(app.pm_visible, "Esc only unfocuses");
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    press_key(&mut app, KeyCode::Tab).await; // refocus digest
    press_key(&mut app, KeyCode::Char('q')).await;
    assert!(!app.pm_visible, "q closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn digest_r_clears_poll_throttles() {
    let mut app = test_app_with_two_ready_workspaces();
    app.pr_last_poll_ms
        .insert(crate::data::store::WorkspaceId(1), 123);
    app.diff_last_poll_ms
        .insert(crate::data::store::WorkspaceId(1), 123);
    press_key(&mut app, KeyCode::Char('p')).await;
    press_key(&mut app, KeyCode::Char('r')).await;
    assert!(app.pr_last_poll_ms.is_empty());
    assert!(app.diff_last_poll_ms.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slash_enters_filter_mode_and_chars_edit_buffer() {
    let mut app = test_app_with_two_ready_workspaces();
    press_key(&mut app, KeyCode::Char('p')).await;
    assert_eq!(app.pm_filter, None);
    press_key(&mut app, KeyCode::Char('/')).await;
    assert_eq!(app.pm_filter.as_deref(), Some(""));
    press_key(&mut app, KeyCode::Char('f')).await;
    press_key(&mut app, KeyCode::Char('i')).await;
    assert_eq!(app.pm_filter.as_deref(), Some("fi"));
    press_key(&mut app, KeyCode::Backspace).await;
    assert_eq!(app.pm_filter.as_deref(), Some("f"));
    // Bound letters become filter text while typing: q must NOT close.
    press_key(&mut app, KeyCode::Char('q')).await;
    assert_eq!(app.pm_filter.as_deref(), Some("fq"));
    assert!(app.pm_visible, "q while filtering edits the buffer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filter_esc_clears_then_second_esc_unfocuses() {
    let mut app = test_app_with_two_ready_workspaces();
    press_key(&mut app, KeyCode::Char('p')).await;
    press_key(&mut app, KeyCode::Char('/')).await;
    press_key(&mut app, KeyCode::Char('x')).await;
    press_key(&mut app, KeyCode::Esc).await;
    assert_eq!(app.pm_filter, None, "first Esc clears the filter");
    assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
    press_key(&mut app, KeyCode::Esc).await;
    assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    assert!(app.pm_visible, "second Esc only unfocuses");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn filter_edits_clamp_selection_to_filtered_count() {
    let mut app = test_app_with_two_ready_workspaces();
    press_key(&mut app, KeyCode::Char('p')).await;
    press_key(&mut app, KeyCode::Char('j')).await;
    assert_eq!(app.pm_digest_selected, 1);
    press_key(&mut app, KeyCode::Char('/')).await;
    // "first" matches only one card -> selection clamps to 0.
    for c in "first".chars() {
        press_key(&mut app, KeyCode::Char(c)).await;
    }
    assert_eq!(app.pm_digest_selected, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_the_pane_clears_the_filter() {
    let mut app = test_app_with_two_ready_workspaces();
    press_key(&mut app, KeyCode::Char('p')).await;
    press_key(&mut app, KeyCode::Char('/')).await;
    press_key(&mut app, KeyCode::Char('x')).await;
    // Tab away (filter persists while the pane stays open), then close
    // from dashboard focus.
    press_key(&mut app, KeyCode::Tab).await;
    assert_eq!(app.pm_filter.as_deref(), Some("x"));
    press_key(&mut app, KeyCode::Char('p')).await;
    assert!(!app.pm_visible);
    assert_eq!(app.pm_filter, None, "closing clears the filter");
}

/// Regression: the render path clamps `pm_digest_selected` to the
/// current card count before drawing, but Enter used to look the card
/// up with the raw (unclamped) index — so if the card list shrank
/// while a stale, out-of-range selection lingered, Enter would find
/// no card and silently no-op instead of attaching. `handle_key_dashboard`
/// now clamps the same way the renderer does before the `card_at`
/// lookup. Model the attach assertion on
/// `updates_panel_modal_enter_switches_view_and_clears_attention`:
/// a successful attach flips `app.view` to `View::Attached` targeting
/// the attached workspace.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn digest_enter_clamps_out_of_range_selection_to_last_card() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    let mut env = EnvGuard::new();
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let first_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "first",
            branch: "repo/first",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    let second_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "second",
            branch: "repo/second",
            worktree_path: std::path::Path::new(".."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(first_id, WorkspaceState::Ready)
        .unwrap();
    store
        .set_workspace_state(second_id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert_eq!(
        crate::ui::pm_pane::card_count(&app.build_pm_digest()),
        2,
        "fixture should yield exactly two digest cards"
    );

    press_key(&mut app, KeyCode::Char('p')).await; // open + focus digest
    app.pm_digest_selected = 5; // out of range for a 2-card digest
    press_key(&mut app, KeyCode::Enter).await;

    assert!(
        matches!(&app.view, crate::ui::View::Attached(s) if s.focused_target().map(|t| t.workspace_id) == Some(second_id)),
        "Enter with an out-of-range selection should clamp to and attach \
         the last card's workspace (second), not no-op; got {:?}",
        app.view
    );
}

#[tokio::test]
async fn tab_swap_clears_armed_z_leader() {
    // If the user arms `z` then Tabs over to PM, the leader must
    // clear — otherwise their next key after Tabbing back would
    // be unexpectedly eaten by the z-leader dispatcher.
    let (mut app, _) = make_app_with_n_repos(2);
    // Tab swap path requires PM visible.
    app.pm_visible = true;
    app.focus = crate::ui::PaneFocus::Dashboard;
    press(&mut app, 'z', KeyModifiers::NONE).await;
    assert!(app.z_leader_pending, "z should arm the leader");
    press_key(&mut app, KeyCode::Tab).await;
    assert!(
        !app.z_leader_pending,
        "Tab to PM should clear the armed leader"
    );
    assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
}
