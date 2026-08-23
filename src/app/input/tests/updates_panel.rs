//! The updates panel modal: navigation, sort, filter, rendering.

use super::*;
use crate::data::store::Store;
use crate::test_support::EnvGuard;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use super::common::*;
use crossterm::event::{KeyEvent, KeyModifiers};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_modal_esc_closes() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "Esc should close UpdatesPanel");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_modal_down_advances_selection() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    // Two workspaces so Down has somewhere to go.
    for (name, branch, path) in [
        ("alpha", "repo/alpha", "/tmp/wsx-test/alpha"),
        ("beta", "repo/beta", "/tmp/wsx-test/beta"),
    ] {
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name,
                branch,
                worktree_path: std::path::Path::new(path),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
    }
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match app.modal {
        Some(crate::ui::modal::Modal::UpdatesPanel { selected, .. }) => {
            assert_eq!(selected, 1, "Down should advance to index 1");
        }
        other => panic!("unexpected modal state: {other:?}"),
    }
    // Down again clamps at the last index.
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match app.modal {
        Some(crate::ui::modal::Modal::UpdatesPanel { selected, .. }) => {
            assert_eq!(selected, 1, "Down past last clamps at max");
        }
        other => panic!("unexpected modal state: {other:?}"),
    }
    // Up returns to 0.
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Up, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match app.modal {
        Some(crate::ui::modal::Modal::UpdatesPanel { selected, .. }) => {
            assert_eq!(selected, 0, "Up should retreat to 0");
        }
        other => panic!("unexpected modal state: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_modal_j_k_aliases_down_up() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    for (name, branch, path) in [
        ("alpha", "repo/alpha", "/tmp/wsx-test/alpha"),
        ("beta", "repo/beta", "/tmp/wsx-test/beta"),
    ] {
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name,
                branch,
                worktree_path: std::path::Path::new(path),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
    }
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        matches!(
            app.modal,
            Some(crate::ui::modal::Modal::UpdatesPanel { selected: 1, .. })
        ),
        "j should advance like Down; got {:?}",
        app.modal
    );
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        matches!(
            app.modal,
            Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0, .. })
        ),
        "k should retreat like Up; got {:?}",
        app.modal
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repo_settings_modal_j_k_aliases_down_up() {
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::RepoSettings {
        repo_id,
        selected: 0,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        matches!(
            app.modal,
            Some(crate::ui::modal::Modal::RepoSettings { selected: 1, .. })
        ),
        "j should advance in RepoSettings; got {:?}",
        app.modal
    );
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(
        matches!(
            app.modal,
            Some(crate::ui::modal::Modal::RepoSettings { selected: 0, .. })
        ),
        "k should retreat in RepoSettings; got {:?}",
        app.modal
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_modal_enter_switches_view_and_clears_attention() {
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
    let ws_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "blocked",
            branch: "repo/blocked",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(ws_id, WorkspaceState::Ready)
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.workspace_needs_attention.insert(ws_id);
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "Enter should close the modal");
    assert!(
        matches!(&app.view, crate::ui::View::Attached(s) if s.focused_target().map(|t| t.workspace_id) == Some(ws_id)),
        "Enter should switch view to the selected workspace; got {:?}",
        app.view
    );
    assert!(
        !app.workspace_needs_attention.contains(&ws_id),
        "attention flag should clear on Enter"
    );
}

/// 'l' is the vim-style alias for Enter — same attach flow as the
/// dashboard's 'l' binding.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_modal_l_switches_view_like_enter() {
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
    let ws_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "blocked",
            branch: "repo/blocked",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(ws_id, WorkspaceState::Ready)
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.workspace_needs_attention.insert(ws_id);
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "'l' should close the modal");
    assert!(
        matches!(&app.view, crate::ui::View::Attached(s) if s.focused_target().map(|t| t.workspace_id) == Some(ws_id)),
        "'l' should switch view to the selected workspace; got {:?}",
        app.view
    );
    assert!(
        !app.workspace_needs_attention.contains(&ws_id),
        "attention flag should clear on 'l'"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_v_splits_attached_view_vertically() {
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
            worktree_path: std::path::Path::new("/tmp/wsx-split-1"),
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
            worktree_path: std::path::Path::new("/tmp/wsx-split-2"),
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
    // Pre-spawn the "first" workspace and attach to it. Use `.` for the
    // spawn cwd so the PTY actually starts; the store-level
    // worktree_path is just a unique key for the row.
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_0 = test_primary_instance(&app, first_id);
    app.sessions
        .spawn(
            __inst_0,
            first_id,
            std::path::Path::new("."),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();
    let second_mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_1 = test_primary_instance(&app, second_id);
    app.sessions
        .spawn(
            __inst_1,
            second_id,
            std::path::Path::new("."),
            80,
            24,
            second_mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();
    let first_target = test_target(&app, first_id);
    app.view = crate::ui::View::Attached(AttachedState::single(first_target));

    // Open Updates panel, point at the second workspace, press 'v'.
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    // The renderer's order is grouped/sorted; in this minimal setup both
    // workspaces are in `repo`. Find the index of `second_id` from the
    // module's ordering helper.
    let order = crate::ui::modal::ordered_workspaces_for_panel(
        &crate::ui::modal::PanelInputs {
            repos: &app.repos,
            workspaces: &app.workspaces,
            events: &app.workspace_events,
            activity: &std::collections::HashMap::new(),
            needs_attention: &std::collections::HashSet::new(),
            awaiting: &std::collections::HashMap::new(),
            statuses: &std::collections::HashMap::new(),
            lifecycles: &std::collections::HashMap::new(),
        },
        crate::ui::modal::UpdatesSort::Default,
        None,
    );
    let target_idx = order.iter().position(|id| *id == second_id).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: target_idx,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "v should close the modal");
    match &app.view {
        crate::ui::View::Attached(state) => {
            assert_eq!(state.leaf_count(), 2, "v should produce a 2-pane split");
            let ws_ids: Vec<_> = state.leaves().iter().map(|t| t.workspace_id).collect();
            assert!(ws_ids.contains(&first_id));
            assert!(ws_ids.contains(&second_id));
            // Focus should be on the newly added pane.
            assert_eq!(
                state.focused_target().map(|t| t.workspace_id),
                Some(second_id)
            );
        }
        other => panic!("expected Attached view; got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_modal_swallows_other_keys() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Char('q'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_some(), "q should not dismiss UpdatesPanel");
    assert!(!app.quit, "q should not propagate to App::quit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_o_cycles_sort_and_follows_selection() {
    use crate::git::forge::BranchLifecycle;
    use crate::ui::modal::UpdatesSort;
    let store = Store::open_in_memory().unwrap();
    let ids = seed_two_workspaces(&store);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // beta has an open PR, alpha none — under PrStatus beta sorts first,
    // flipping the two rows relative to Default/Status order.
    app.pr_lifecycle.insert(ids[1], BranchLifecycle::PrOpen);
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0, // alpha
        sort: UpdatesSort::Default,
        filter: None,
    });
    let shared = shared_app();
    let press_o = key(crossterm::event::KeyCode::Char('o'));

    // Default → Status: both workspaces are Idle, order unchanged,
    // selection stays on alpha at index 0.
    handle_key_modal(&mut app, &shared, press_o).await.unwrap();
    match app.modal {
        Some(crate::ui::modal::Modal::UpdatesPanel { selected, sort, .. }) => {
            assert_eq!(sort, UpdatesSort::Status);
            assert_eq!(selected, 0, "selection stays on alpha");
        }
        ref other => panic!("unexpected modal state: {other:?}"),
    }

    // Status → PrStatus: beta (open PR) jumps to index 0; the cursor
    // must follow alpha to index 1 rather than staying on row 0.
    handle_key_modal(&mut app, &shared, press_o).await.unwrap();
    match app.modal {
        Some(crate::ui::modal::Modal::UpdatesPanel { selected, sort, .. }) => {
            assert_eq!(sort, UpdatesSort::PrStatus);
            assert_eq!(selected, 1, "cursor follows alpha to its new row");
        }
        ref other => panic!("unexpected modal state: {other:?}"),
    }

    // PrStatus → Default: back to the original order and back to row 0.
    handle_key_modal(&mut app, &shared, press_o).await.unwrap();
    match app.modal {
        Some(crate::ui::modal::Modal::UpdatesPanel { selected, sort, .. }) => {
            assert_eq!(sort, UpdatesSort::Default);
            assert_eq!(selected, 0, "cursor follows alpha back to row 0");
        }
        ref other => panic!("unexpected modal state: {other:?}"),
    }
}

/// `/` arms filter mode with an empty buffer — distinct from `None`, so
/// the footer can echo the bare `/` before any typing. Subsequent
/// printable keys are filter text, not the j/k/o/l/v/s shortcuts.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_slash_arms_filter_and_captures_typing() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::KeyCode;
    let store = Store::open_in_memory().unwrap();
    seed_two_workspaces(&store);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::UpdatesPanel {
        selected: 0,
        sort: UpdatesSort::Default,
        filter: None,
    });
    let shared = shared_app();

    handle_key_modal(&mut app, &shared, key(KeyCode::Char('/')))
        .await
        .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel { ref filter, .. }) => {
            assert_eq!(filter.as_deref(), Some(""), "/ arms an empty buffer");
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }

    // 'j' would move the selection outside filter mode; here it types.
    for c in ['j', 'b'] {
        handle_key_modal(&mut app, &shared, key(KeyCode::Char(c)))
            .await
            .unwrap();
    }
    match app.modal {
        Some(Modal::UpdatesPanel { ref filter, .. }) => {
            assert_eq!(filter.as_deref(), Some("jb"));
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }

    handle_key_modal(&mut app, &shared, key(KeyCode::Backspace))
        .await
        .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel { ref filter, .. }) => {
            assert_eq!(filter.as_deref(), Some("j"));
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }

    // Backspace past the start is inert, not a panel close.
    for _ in 0..3 {
        handle_key_modal(&mut app, &shared, key(KeyCode::Backspace))
            .await
            .unwrap();
    }
    match app.modal {
        Some(Modal::UpdatesPanel { ref filter, .. }) => {
            assert_eq!(filter.as_deref(), Some(""));
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }
}

/// Esc is two-stage: it clears an active filter first and only closes
/// the panel once there is no filter to clear.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_esc_clears_filter_before_closing() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::KeyCode;
    let store = Store::open_in_memory().unwrap();
    seed_two_workspaces(&store);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::UpdatesPanel {
        selected: 0,
        sort: UpdatesSort::Default,
        filter: Some("alp".to_string()),
    });
    let shared = shared_app();

    handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel { ref filter, .. }) => {
            assert_eq!(filter.as_deref(), None, "first Esc clears the filter");
        }
        ref other => panic!("panel should stay open; got {other:?}"),
    }

    handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();
    assert!(app.modal.is_none(), "second Esc closes the panel");
}

/// Arrows keep navigating while filter mode is on — they are the escape
/// hatch for j/k being filter text.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_arrows_navigate_while_filtering() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::KeyCode;
    let store = Store::open_in_memory().unwrap();
    seed_two_workspaces(&store);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::UpdatesPanel {
        selected: 0,
        sort: UpdatesSort::Default,
        filter: Some(String::new()),
    });
    let shared = shared_app();

    handle_key_modal(&mut app, &shared, key(KeyCode::Down))
        .await
        .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel {
            selected,
            ref filter,
            ..
        }) => {
            assert_eq!(selected, 1, "Down still moves while filtering");
            assert_eq!(filter.as_deref(), Some(""), "and does not edit the buffer");
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }
}

/// A char carrying CONTROL or ALT is not filter text — the intercept's
/// guard excludes it, so it falls through to the outer handler, which
/// doesn't inspect modifiers. `Ctrl-j` therefore still moves the
/// selection, exactly like a bare `j` would outside filter mode.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_control_modified_char_falls_through_while_filtering() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::KeyCode;
    let store = Store::open_in_memory().unwrap();
    seed_two_workspaces(&store);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(Modal::UpdatesPanel {
        selected: 0,
        sort: UpdatesSort::Default,
        filter: Some(String::new()),
    });
    let shared = shared_app();

    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel {
            selected,
            ref filter,
            ..
        }) => {
            assert_eq!(selected, 1, "Ctrl-j falls through to the Down/'j' arm");
            assert_eq!(filter.as_deref(), Some(""), "and does not edit the buffer");
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }
}

/// Enter is the other escape hatch (besides arrows) from j/k being
/// filter text — it still attaches to the selected, filtered-to
/// workspace while a filter is active.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_enter_attaches_while_filtering() {
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
    let ws_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "blocked",
            branch: "repo/blocked",
            worktree_path: std::path::Path::new("."),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(ws_id, WorkspaceState::Ready)
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: Some("block".to_string()),
    });
    let shared = shared_app();
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(crossterm::event::KeyCode::Enter, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "Enter should close the modal");
    assert!(
        matches!(&app.view, crate::ui::View::Attached(s) if s.focused_target().map(|t| t.workspace_id) == Some(ws_id)),
        "Enter should attach to the filtered-to workspace; got {:?}",
        app.view
    );
}

/// The cursor tracks its workspace across a filter edit rather than its
/// index. Typing "a" matches both alpha and beta, so the order is
/// unchanged (`[alpha, beta]`) — a real position lookup must still find
/// beta at index 1. This is deliberately NOT a narrowing edit: reading
/// the pre-edit selection from the post-edit order, or a bare
/// `.unwrap_or(0)`, would also land on 0 for a narrowing edit that hides
/// the selected row, so only a needle that keeps the row in place can
/// tell a genuine lookup apart from every 0-shaped fallback.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_selection_follows_workspace_across_filter_edits() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::KeyCode;
    let store = Store::open_in_memory().unwrap();
    seed_two_workspaces(&store);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // Start on beta (index 1), filter mode armed.
    app.modal = Some(Modal::UpdatesPanel {
        selected: 1,
        sort: UpdatesSort::Default,
        filter: Some(String::new()),
    });
    let shared = shared_app();

    handle_key_modal(&mut app, &shared, key(KeyCode::Char('a')))
        .await
        .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel { selected, .. }) => {
            assert_eq!(
                selected, 1,
                "cursor stays on beta, still at index 1 in the unchanged order"
            );
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }
}

/// The narrowing case, and the one that actually separates a workspace
/// lookup from index-clamping. Filtering only ever removes rows, so it
/// takes three: select the middle row, then type a needle that hides
/// the row *above* it. The selected workspace slides up to index 0,
/// while clamping the old index would leave the cursor at 1 — a
/// different workspace, still in range.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_selection_moves_up_when_a_filter_hides_the_row_above() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::KeyCode;
    let store = Store::open_in_memory().unwrap();
    // Only the last two carry an `x`; the repo ("repo") and the rows'
    // status text ("no session") have none, so `x` hides alpha alone.
    let ids = seed_workspaces(&store, &["alpha", "beta-x", "gamma-x"]);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // Start on beta-x (index 1 of [alpha, beta-x, gamma-x]).
    app.modal = Some(Modal::UpdatesPanel {
        selected: 1,
        sort: UpdatesSort::Default,
        filter: Some(String::new()),
    });
    assert_eq!(
        panel_order(&app, UpdatesSort::Default, Some("")),
        ids,
        "unfiltered order is insertion order"
    );
    let shared = shared_app();

    handle_key_modal(&mut app, &shared, key(KeyCode::Char('x')))
        .await
        .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel {
            selected,
            sort,
            ref filter,
        }) => {
            assert_eq!(filter.as_deref(), Some("x"));
            let order = panel_order(&app, sort, filter.as_deref());
            assert_eq!(order, vec![ids[1], ids[2]], "`x` hides alpha only");
            assert_eq!(
                selected, 0,
                "cursor follows beta-x to index 0; clamping would leave it at 1"
            );
            assert_eq!(
                order.get(selected).copied(),
                Some(ids[1]),
                "the selected row is still beta-x"
            );
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }
}

/// When a filter edit hides the selected workspace entirely, the
/// cursor clamps into range rather than pointing past the end of the
/// (possibly empty) new order.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_selection_clamps_when_filter_hides_everything() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::KeyCode;
    let store = Store::open_in_memory().unwrap();
    seed_two_workspaces(&store);
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    // Start on beta (index 1), filter mode armed.
    app.modal = Some(Modal::UpdatesPanel {
        selected: 1,
        sort: UpdatesSort::Default,
        filter: Some(String::new()),
    });
    let shared = shared_app();

    // Typing until nothing matches: the index clamps rather than
    // pointing past the end of an empty list.
    for c in ['z', 'z'] {
        handle_key_modal(&mut app, &shared, key(KeyCode::Char(c)))
            .await
            .unwrap();
    }
    match app.modal {
        Some(Modal::UpdatesPanel { selected, .. }) => {
            assert_eq!(selected, 0, "empty result clamps to 0");
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }
}

/// Filter state lives in the modal variant, so reopening starts clean —
/// a stale needle would silently hide rows on the next open.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_reopens_without_a_filter() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);
    app.modal = Some(Modal::UpdatesPanel {
        selected: 0,
        sort: UpdatesSort::Default,
        filter: Some("stale".to_string()),
    });
    let shared = shared_app();
    // Esc is two-stage: the first clears the active filter, the second
    // closes the panel (see `updates_panel_esc_clears_filter_before_closing`).
    handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();
    handle_key_modal(&mut app, &shared, key(KeyCode::Esc))
        .await
        .unwrap();
    assert!(app.modal.is_none());

    // Reopen via the real leader path: Ctrl-X then 'u'.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    handle_key_attached(&mut app, target, key(KeyCode::Char('u')))
        .await
        .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel { ref filter, .. }) => {
            assert_eq!(filter.as_deref(), None, "reopen starts unfiltered");
        }
        ref other => panic!("expected UpdatesPanel; got {other:?}"),
    }
}

/// The sort mode lives only in the modal variant, so closing the panel
/// (Esc) and reopening it via the real leader-`u` path must land back on
/// `UpdatesSort::Default` — not whatever mode was active when it closed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn updates_panel_reopens_in_default_after_close() {
    use crate::ui::modal::{Modal, UpdatesSort};
    use crossterm::event::{KeyCode, KeyEvent};
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let ws_id = spawn_attached_workspace(&mut app);
    let target = test_target(&app, ws_id);

    // Open in a non-default sort, then close with Esc — mirrors
    // `updates_panel_modal_esc_closes`.
    app.modal = Some(Modal::UpdatesPanel {
        selected: 0,
        sort: UpdatesSort::PrStatus,
        filter: None,
    });
    let shared = Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ));
    handle_key_modal(
        &mut app,
        &shared,
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
    )
    .await
    .unwrap();
    assert!(app.modal.is_none(), "Esc should close UpdatesPanel");

    // Re-trigger the real open path: Ctrl-X arms the leader, then 'u'
    // fires the accelerator that constructs a fresh UpdatesPanel modal.
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
    )
    .await
    .unwrap();
    assert!(app.leader_pending, "Ctrl-X must arm leader_pending");
    handle_key_attached(
        &mut app,
        target,
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
    )
    .await
    .unwrap();
    match app.modal {
        Some(Modal::UpdatesPanel { selected, sort, .. }) => {
            assert_eq!(selected, 0);
            assert_eq!(
                sort,
                UpdatesSort::Default,
                "modal must reopen in Default regardless of the sort it was closed in"
            );
        }
        ref other => panic!("expected UpdatesPanel modal; got {other:?}"),
    }
}

#[test]
fn updates_panel_render_shows_grouped_workspaces() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let store = Store::open_in_memory().unwrap();
    let repo1 = store
        .add_repo(std::path::Path::new("/tmp/r1"), "repo-alpha", "")
        .unwrap();
    let ws1 = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo1,
            name: "alpha-ws",
            branch: "repo-alpha/alpha-ws",
            worktree_path: std::path::Path::new("/tmp/wsx-test/alpha-ws"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(ws1, WorkspaceState::Ready)
        .unwrap();
    let repo2 = store
        .add_repo(std::path::Path::new("/tmp/r2"), "repo-beta", "")
        .unwrap();
    let ws2 = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo2,
            name: "beta-ws",
            branch: "repo-beta/beta-ws",
            worktree_path: std::path::Path::new("/tmp/wsx-test/beta-ws"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(ws2, WorkspaceState::Ready)
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });

    let backend = TestBackend::new(100, 30);
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
    assert!(
        rendered.contains("Workspace updates"),
        "missing panel title:\n{rendered}"
    );
    assert!(
        rendered.contains("repo-alpha"),
        "missing repo header:\n{rendered}"
    );
    assert!(
        rendered.contains("alpha-ws"),
        "missing workspace row:\n{rendered}"
    );
    assert!(
        rendered.contains("repo-beta"),
        "missing repo header:\n{rendered}"
    );
    assert!(
        rendered.contains("beta-ws"),
        "missing workspace row:\n{rendered}"
    );
}

#[test]
fn updates_panel_render_omits_repos_without_workspaces() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let store = Store::open_in_memory().unwrap();
    // repo-alpha has a workspace; repo-beta is empty.
    let repo1 = store
        .add_repo(std::path::Path::new("/tmp/r1"), "repo-alpha", "")
        .unwrap();
    let ws1 = store
        .insert_workspace(&NewWorkspace {
            repo_id: repo1,
            name: "alpha-ws",
            branch: "repo-alpha/alpha-ws",
            worktree_path: std::path::Path::new("/tmp/wsx-test/alpha-ws"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(ws1, WorkspaceState::Ready)
        .unwrap();
    store
        .add_repo(std::path::Path::new("/tmp/r2"), "repo-beta", "")
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });

    let backend = TestBackend::new(100, 30);
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
    assert!(
        rendered.contains("repo-alpha") && rendered.contains("alpha-ws"),
        "populated repo should still render:\n{rendered}"
    );
    assert!(
        !rendered.contains("repo-beta"),
        "empty repo header should be omitted:\n{rendered}"
    );
    assert!(
        !rendered.contains("(no workspaces)"),
        "empty-repo placeholder should no longer appear:\n{rendered}"
    );
}

#[test]
fn updates_panel_render_shows_global_empty_state_when_all_repos_empty() {
    use crate::data::store::Store;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let store = Store::open_in_memory().unwrap();
    // Repos exist but none have workspaces — exercises the global
    // empty-state path (no headers, single "(no workspaces)" line).
    store
        .add_repo(std::path::Path::new("/tmp/r1"), "repo-alpha", "")
        .unwrap();
    store
        .add_repo(std::path::Path::new("/tmp/r2"), "repo-beta", "")
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: 0,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });

    let backend = TestBackend::new(100, 30);
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
    assert!(
        rendered.contains("(no workspaces)"),
        "global empty-state line should render:\n{rendered}"
    );
    assert!(
        !rendered.contains("repo-alpha") && !rendered.contains("repo-beta"),
        "no repo headers should render when all repos are empty:\n{rendered}"
    );
}

#[test]
fn updates_panel_render_scrolls_to_keep_selected_visible() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let store = Store::open_in_memory().unwrap();
    // 5 repos × 8 workspaces = 40 ws rows + 5 headers + 5 blank
    // separators = 50 visual lines. The panel clamps height to ≤25,
    // so without scrolling the last workspaces are invisible.
    for r in 0..5 {
        let repo_path = format!("/tmp/scroll-test/r{r}");
        let repo_name = format!("repo-{r}");
        let repo_id = store
            .add_repo(std::path::Path::new(&repo_path), &repo_name, "")
            .unwrap();
        for w in 0..8 {
            let ws_name = format!("ws-{r}-{w}");
            let branch = format!("{repo_name}/{ws_name}");
            let worktree = format!("/tmp/scroll-test/{ws_name}");
            let ws_id = store
                .insert_workspace(&NewWorkspace {
                    repo_id,
                    name: &ws_name,
                    branch: &branch,
                    worktree_path: std::path::Path::new(&worktree),
                    yolo: false,
                    agent: crate::pty::session::AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            store
                .set_workspace_state(ws_id, WorkspaceState::Ready)
                .unwrap();
        }
    }

    let mut app = App::new(store, PathBuf::from("/tmp/scroll-test")).unwrap();

    // Build the same order the renderer uses, so we can select the
    // very last workspace — the one that would be clipped without
    // scroll support.
    let activity_translated: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::ui::updates_bar::ActivityState,
    > = app
        .workspace_activity
        .iter()
        .map(|(k, v)| (*k, crate::app::render::translate_activity(*v)))
        .collect();
    let statuses: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::ui::dashboard::status::Status,
    > = app
        .workspaces
        .iter()
        .map(|(_, w)| (w.id, app.classify_status(w)))
        .collect();
    let order = crate::ui::modal::ordered_workspaces_for_panel(
        &crate::ui::modal::PanelInputs {
            repos: &app.repos,
            workspaces: &app.workspaces,
            events: &app.workspace_events,
            activity: &activity_translated,
            needs_attention: &app.workspace_needs_attention,
            awaiting: &std::collections::HashMap::new(),
            statuses: &statuses,
            lifecycles: &app.pr_lifecycle,
        },
        crate::ui::modal::UpdatesSort::Default,
        None,
    );
    assert!(
        order.len() >= 40,
        "expected ≥40 workspaces, got {}",
        order.len()
    );
    let last_selected = order.len() - 1;
    let last_ws_id = order[last_selected];
    let last_ws_name = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == last_ws_id)
        .expect("last workspace present")
        .1
        .name
        .clone();

    app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
        selected: last_selected,
        sort: crate::ui::modal::UpdatesSort::Default,
        filter: None,
    });

    let backend = TestBackend::new(100, 30);
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
    assert!(
        rendered.contains(&last_ws_name),
        "selected workspace '{last_ws_name}' should be scrolled into \
         view but is not present in rendered modal:\n{rendered}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attached_view_shows_status_row_for_other_workspace_needing_attention() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut env = EnvGuard::new();
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let attached_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "attached-here",
            branch: "repo/attached-here",
            worktree_path: std::path::Path::new("/tmp/wsx-test/attached"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(attached_id, WorkspaceState::Ready)
        .unwrap();
    let other_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "the-other",
            branch: "repo/the-other",
            worktree_path: std::path::Path::new("/tmp/wsx-test/other"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(other_id, WorkspaceState::Ready)
        .unwrap();

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_6 = test_primary_instance(&app, attached_id);
    app.sessions
        .spawn(
            __inst_6,
            attached_id,
            std::path::Path::new("."),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();
    app.view = crate::ui::View::Attached(AttachedState::single(test_target(&app, attached_id)));
    // The new status row exclusively surfaces workspaces with
    // `needs_attention` set — recent activity alone no longer qualifies.
    // In production both flags are set together when `alert_decision`
    // fires; mirror that here so the V5 status glyph (`!` for stalled)
    // is what the styled line renders.
    app.workspace_needs_attention.insert(other_id);
    app.workspace_activity
        .insert(other_id, crate::app::ActivityState::Stalled);

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
    assert!(
        rendered.contains("the-other"),
        "expected status row mention of the other workspace:\n{rendered}"
    );
    assert!(
        rendered.contains("! repo/the-other"),
        "expected V5 stalled glyph next to workspace name on status row:\n{rendered}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attached_view_no_status_row_when_no_other_activity() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let mut env = EnvGuard::new();
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let attached_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "only-one",
            branch: "repo/only-one",
            worktree_path: std::path::Path::new("/tmp/wsx-test/only"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
        .set_workspace_state(attached_id, WorkspaceState::Ready)
        .unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    let __inst_7 = test_primary_instance(&app, attached_id);
    app.sessions
        .spawn(
            __inst_7,
            attached_id,
            std::path::Path::new("."),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Claude,
            None,
        )
        .unwrap();
    app.view = crate::ui::View::Attached(AttachedState::single(test_target(&app, attached_id)));

    let backend = TestBackend::new(80, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| draw_for_test(f, &mut app)).unwrap();
    let buf = term.backend().buffer();
    // The bottom line shows the workspace label + attention items (no footer).
    // The second-to-last row should NOT contain a status indicator glyph.
    let h = buf.area.height;
    let second_to_last: String = (0..buf.area.width)
        .map(|x| buf[(x, h - 2)].symbol())
        .collect();
    assert!(
        !second_to_last.contains('⚠'),
        "unexpected attention glyph in row {}: {second_to_last:?}",
        h - 2
    );
    assert!(
        !second_to_last.contains('●'),
        "unexpected activity glyph in row {}: {second_to_last:?}",
        h - 2
    );
}
