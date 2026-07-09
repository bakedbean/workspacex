// Glob-import everything in the parent `input` module so each test
// submodule's `use super::*;` cascades to input's items (App,
// handle_key_*, crossterm types, etc.). Without this, the nested
// `mod pm_state_tests` blocks would `use super::*;` on this empty
// wrapper module.
use super::*;

#[cfg(test)]
mod pm_state_tests {
    use super::*;
    use crate::data::store::Store;
    use crate::test_support::{EnvGuard, cat_path};
    use std::path::PathBuf;

    #[test]
    fn app_initializes_pm_state_off() {
        let store = Store::open_in_memory().unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(app.pm.is_none());
        assert!(!app.pm_visible);
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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

    #[test]
    fn dashboard_renders_split_with_pm_title_when_visible_even_without_session() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true; // No session yet — the pane shows a placeholder.
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
            rendered.contains("Project Manager"),
            "expected pane title in rendered buffer:\n{rendered}"
        );
        assert!(
            rendered.contains("Tab to focus"),
            "expected unfocused hint:\n{rendered}"
        );
    }

    use crossterm::event::{KeyEvent, KeyModifiers};

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
    async fn dashboard_down_at_last_entry_wraps_to_first() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![
            SelectionTarget::Repo(crate::data::store::RepoId(1)),
            SelectionTarget::Repo(crate::data::store::RepoId(2)),
            SelectionTarget::Repo(crate::data::store::RepoId(3)),
        ];
        app.select_index(2);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(
            app.dashboard.selected, 0,
            "Down at last should wrap to first"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_up_at_first_entry_wraps_to_last() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![
            SelectionTarget::Repo(crate::data::store::RepoId(1)),
            SelectionTarget::Repo(crate::data::store::RepoId(2)),
            SelectionTarget::Repo(crate::data::store::RepoId(3)),
        ];
        app.select_index(0);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.selected, 2, "Up at first should wrap to last");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_down_in_middle_advances_normally() {
        // Sanity check that wrap-around didn't break the non-edge case.
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![
            SelectionTarget::Repo(crate::data::store::RepoId(1)),
            SelectionTarget::Repo(crate::data::store::RepoId(2)),
            SelectionTarget::Repo(crate::data::store::RepoId(3)),
        ];
        app.select_index(1);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.selected, 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_esc_closes() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
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
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
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
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
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
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
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
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
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
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
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
                Some(crate::ui::modal::Modal::UpdatesPanel { selected: 1 })
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
                Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 })
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
        env.set("WSX_CLAUDE_BIN", cat_path());
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
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_v_splits_attached_view_vertically() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
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
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        // The renderer's order is grouped/sorted; in this minimal setup both
        // workspaces are in `repo`. Find the index of `second_id` from the
        // module's ordering helper.
        let order = crate::ui::modal::ordered_workspaces_for_panel(
            &app.repos,
            &app.workspaces,
            &app.workspace_events,
            &std::collections::HashMap::new(),
            &std::collections::HashSet::new(),
        );
        let target_idx = order.iter().position(|id| *id == second_id).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
            selected: target_idx,
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
    async fn ctrl_x_d_closes_focused_pane_when_split() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let first_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "first",
                branch: "repo/first",
                worktree_path: std::path::Path::new("/tmp/wsx-close-1"),
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
                worktree_path: std::path::Path::new("/tmp/wsx-close-2"),
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
        for id in [first_id, second_id] {
            let mode = crate::pty::session::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let __inst_2 = test_primary_instance(&app, id);
            app.sessions
                .spawn(
                    __inst_2,
                    id,
                    std::path::Path::new("."),
                    80,
                    24,
                    mode,
                    crate::agent::remote_control::RemoteOpts::disabled(),
                    crate::pty::session::AgentKind::Claude,
                    None,
                )
                .unwrap();
        }
        // Start in a 2-pane split with `second` focused.
        let first_target = test_target(&app, first_id);
        let second_target = test_target(&app, second_id);
        let mut state = AttachedState::single(first_target);
        state.split(SplitDirection::Vertical, second_target);
        app.view = crate::ui::View::Attached(state);

        // Ctrl-x d closes JUST the focused pane; should leave `first` alone.
        handle_key_attached(
            &mut app,
            second_target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);
        handle_key_attached(
            &mut app,
            second_target,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.view {
            crate::ui::View::Attached(state) => {
                assert_eq!(state.leaf_count(), 1, "should drop down to 1 pane");
                assert_eq!(state.focused_target(), Some(first_target));
            }
            other => panic!("expected Attached view; got {other:?}"),
        }

        // Ctrl-x d on the last pane detaches fully.
        handle_key_attached(
            &mut app,
            first_target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            first_target,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.view, crate::ui::View::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_d_detach_schedules_refresh_for_attached_workspace() {
        // The detail bar shows the workspace's events/diff/procs from
        // app state, which is normally refreshed every 2s by the
        // background poll. When the user detaches back to the
        // dashboard, we want the bar to reflect work just done in the
        // attached session immediately — so detach handlers must clear
        // throttle stamps and queue the workspace for an out-of-band
        // events-tail refresh.
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/wsx-detach-refresh"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_3 = test_primary_instance(&app, id);
        app.sessions
            .spawn(
                __inst_3,
                id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
                None,
            )
            .unwrap();
        let target = test_target(&app, id);
        app.view = crate::ui::View::Attached(AttachedState::single(target));
        // Seed throttle stamps so we can prove the detach handler
        // clears them (forcing the next poll tick to re-fetch).
        app.diff_last_poll_ms.insert(id, 12_345);
        app.pr_last_poll_ms.insert(id, 12_345);
        app.last_proc_scan_ms = 12_345;

        // Ctrl-x d on the last pane fully detaches.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();

        assert!(matches!(app.view, crate::ui::View::Dashboard));
        assert!(
            app.pending_workspace_refresh.contains(&id),
            "detached workspace should be queued for events-tail refresh"
        );
        assert!(
            !app.diff_last_poll_ms.contains_key(&id),
            "diff throttle stamp should be cleared on detach"
        );
        assert!(
            !app.pr_last_poll_ms.contains_key(&id),
            "PR throttle stamp should be cleared on detach"
        );
        assert_eq!(
            app.last_proc_scan_ms, 0,
            "proc-scan throttle should be reset on detach"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_shift_d_detach_schedules_refresh_for_attached_workspace() {
        // Same as the d-path test above, for the Ctrl-X Shift-D save+detach.
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/wsx-esc-refresh"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_4 = test_primary_instance(&app, id);
        app.sessions
            .spawn(
                __inst_4,
                id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
                None,
            )
            .unwrap();
        let target = test_target(&app, id);
        app.view = crate::ui::View::Attached(AttachedState::single(target));

        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
        )
        .await
        .unwrap();

        assert!(matches!(app.view, crate::ui::View::Dashboard));
        assert!(
            app.pending_workspace_refresh.contains(&id),
            "Shift-D-detached workspace should be queued for refresh"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_arrow_moves_focus_in_split() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let mut ids = Vec::new();
        for name in ["a", "b"] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id,
                    name,
                    branch: &format!("repo/{name}"),
                    worktree_path: &std::path::PathBuf::from(format!("/tmp/wsx-arrow-{name}")),
                    yolo: false,
                    agent: crate::pty::session::AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
            ids.push(id);
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        for id in &ids {
            let mode = crate::pty::session::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let __inst_5 = test_primary_instance(&app, *id);
            app.sessions
                .spawn(
                    __inst_5,
                    *id,
                    std::path::Path::new("."),
                    80,
                    24,
                    mode,
                    crate::agent::remote_control::RemoteOpts::disabled(),
                    crate::pty::session::AgentKind::Claude,
                    None,
                )
                .unwrap();
        }
        let target0 = test_target(&app, ids[0]);
        let target1 = test_target(&app, ids[1]);
        let mut state = AttachedState::single(target0);
        state.split(SplitDirection::Vertical, target1);
        // Focus is on ids[1] post-split.
        app.view = crate::ui::View::Attached(state);

        handle_key_attached(
            &mut app,
            target1,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            target1,
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.view {
            crate::ui::View::Attached(state) => {
                assert_eq!(state.focused_target(), Some(target0));
            }
            other => panic!("expected Attached view; got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_down_enter_fires_highlighted_action() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "a",
                branch: "repo/a",
                worktree_path: &std::path::PathBuf::from("/tmp/wsx-nav-a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let inst = test_primary_instance(&app, id);
        app.sessions
            .spawn(
                inst,
                id,
                std::path::Path::new("."),
                80,
                24,
                crate::pty::session::SpawnMode::Fresh {
                    rename_ctx: None,
                    custom_instructions: None,
                    doctrine: None,
                    additional_dirs: vec![],
                    yolo: false,
                },
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
                None,
            )
            .unwrap();
        let target = test_target(&app, id);
        app.view = crate::ui::View::Attached(AttachedState::single(target));

        // Arm leader (selected=0 => "detach"), Down once => index 1 ("updates").
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);
        assert_eq!(app.leader_selected, 0);
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert_eq!(app.leader_selected, 1);
        assert!(app.leader_pending, "↑↓ keep the leader armed");

        // Enter fires "updates" — same effect as pressing 'u' after ^x.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);
        assert!(
            matches!(
                app.modal,
                Some(crate::ui::modal::Modal::UpdatesPanel { .. })
            ),
            "Enter on the updates row opens the updates panel"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_esc_dismisses_nav_overlay_without_detaching() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "a",
                branch: "repo/a",
                worktree_path: &std::path::PathBuf::from("/tmp/wsx-nav-esc-a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let inst = test_primary_instance(&app, id);
        app.sessions
            .spawn(
                inst,
                id,
                std::path::Path::new("."),
                80,
                24,
                crate::pty::session::SpawnMode::Fresh {
                    rename_ctx: None,
                    custom_instructions: None,
                    doctrine: None,
                    additional_dirs: vec![],
                    yolo: false,
                },
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
                None,
            )
            .unwrap();
        let target = test_target(&app, id);
        app.view = crate::ui::View::Attached(AttachedState::single(target));

        // Arm the nav overlay with Ctrl-x.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending, "Ctrl-x must arm the nav overlay");

        // Esc dismisses the overlay and leaves us in the attached chat view —
        // it must NOT detach to the dashboard (that's what 'd' is for).
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending, "Esc must dismiss the nav overlay");
        assert!(
            matches!(app.view, crate::ui::View::Attached(_)),
            "Esc on the nav overlay must stay attached, not return to the dashboard"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_swallows_other_keys() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
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
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });

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
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });

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
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });

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
        let order = crate::ui::modal::ordered_workspaces_for_panel(
            &app.repos,
            &app.workspaces,
            &app.workspace_events,
            &activity_translated,
            &app.workspace_needs_attention,
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
        env.set("WSX_CLAUDE_BIN", cat_path());
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
        env.set("WSX_CLAUDE_BIN", cat_path());
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_u_in_attached_pm_opens_updates_panel() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Manually spawn a PM session so handle_key_attached_pm has one.
        let cwd = PathBuf::from(".");
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let s = app
            .sessions
            .spawn_pm(
                &cwd,
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
            .unwrap();
        app.pm = Some(s);
        app.view = crate::ui::View::AttachedPm;

        // Send the leader (Ctrl-x) then 'u'.
        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);

        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);
        assert!(matches!(
            app.modal,
            Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 })
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_down_enter_fires_highlighted_action_pm() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Spawn a PM session so handle_key_attached_pm has one (mirrors the
        // setup used by leader_u_in_attached_pm_opens_updates_panel).
        let cwd = PathBuf::from(".");
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let s = app
            .sessions
            .spawn_pm(
                &cwd,
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
            .unwrap();
        app.pm = Some(s);
        app.view = crate::ui::View::AttachedPm;

        // Arm the leader with Ctrl-x; overlay highlight should be at index 0.
        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending, "leader should be armed after Ctrl-x");
        assert_eq!(app.leader_selected, 0, "highlight starts at index 0");

        // Down once moves to index 1 ("u" = updates) but keeps the leader armed.
        handle_key_attached_pm(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.leader_selected, 1, "Down moves highlight to index 1");
        assert!(app.leader_pending, "↑↓ keep the leader armed");

        // Enter fires the highlighted action (index 1 = updates panel).
        handle_key_attached_pm(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(!app.leader_pending, "Enter clears the leader");
        assert!(
            matches!(
                app.modal,
                Some(crate::ui::modal::Modal::UpdatesPanel { .. })
            ),
            "Enter on the updates row opens the updates panel"
        );
    }

    fn mouse_event(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn spawn_pm_for_test(app: &mut App) {
        // Use a wrapper that ignores args and cats stdin: Codex Fresh now
        // injects `-c notify=...` for status reporting, which bare `cat` rejects.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = PathBuf::from(".");
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let s = app
            .sessions
            .spawn_pm(
                &cwd,
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Codex,
            )
            .unwrap();
        app.pm = Some(s);
    }

    fn spawn_attached_workspace(app: &mut App) -> crate::data::store::WorkspaceId {
        use crate::data::store::NewWorkspace;
        // Use a wrapper that ignores args and cats stdin: Codex Fresh now
        // injects `-c notify=...` for status reporting, which bare `cat` rejects.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let repo_id = app
            .store
            .add_repo(std::path::Path::new("."), "scratch", "test")
            .unwrap();
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "scrollback-test",
                branch: "main",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: crate::pty::session::AgentKind::Codex,
                shared: false,
            })
            .unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_8 = test_primary_instance(app, ws_id);
        app.sessions
            .spawn(
                __inst_8,
                ws_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Codex,
                None,
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(test_target(app, ws_id)));
        ws_id
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_up_scrolls_attached_workspace() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp)).await;
        assert_eq!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            3,
            "one wheel notch = 3 rows"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_down_decreases_offset_saturating() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scroll_up(5);
        handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollDown)).await;
        assert_eq!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_targets_pm_when_pm_attached() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        spawn_pm_for_test(&mut app);
        app.view = crate::ui::View::AttachedPm;
        handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp)).await;
        assert_eq!(
            app.pm
                .as_ref()
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_targets_pm_in_dashboard_when_pm_focused() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        spawn_pm_for_test(&mut app);
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::ProjectManager;
        // view stays Dashboard.
        handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp)).await;
        assert_eq!(
            app.pm
                .as_ref()
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_noop_when_dashboard_focused_no_target() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // No PM, no attached workspace; view is Dashboard.
        // Just verify the call doesn't panic.
        handle_mouse(&mut app, mouse_event(MouseEventKind::ScrollUp)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn keystroke_to_pty_resets_scrollback() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        let target = test_target(&app, ws_id);
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scroll_up(20);
        assert!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .is_scrolled()
        );
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            !app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .is_scrolled(),
            "any keystroke flowing to PTY must snap to live"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_keystroke_does_not_reset_scrollback() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        let target = test_target(&app, ws_id);
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scroll_up(20);
        // Ctrl-x is the leader. It's consumed by wsx and never reaches the PTY.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);
        assert!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .is_scrolled(),
            "leader key consumed by wsx; offset should be preserved"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn arrow_key_resets_scrollback_and_forwards_to_pty() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        let target = test_target(&app, ws_id);
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scroll_up(20);
        // Up arrow flows to the PTY (Claude Code prompt history) — must
        // also snap scrollback back to live.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            !app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .is_scrolled()
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_digit_sends_pinned_command_to_pty() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        let target = test_target(&app, ws_id);

        // Populate the cache directly (Task 7's resolution path is tested
        // separately via the resolve() unit tests).
        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];

        // Ctrl-x leader.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);

        // '1' — fires chip 1, clears leader.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);

        // cat echoes input back. Verify the screen eventually contains
        // the command text.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "expected '/pull-request' on screen; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_digit_out_of_range_is_noop() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        let target = test_target(&app, ws_id);

        // Only one chip in the cache.
        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];

        // Ctrl-x leader.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();

        // '5' — index 4, out of range for a 1-element cache.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);

        // No bytes should have been written; cat hasn't echoed anything.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            !screen_text.contains("/pull-request"),
            "out-of-range digit must not fire any chip; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_in_chip_rect_fires_pinned_command() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        // Place a 7-wide chip at (5, 30): "[1] PR " = 7 cols.
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        // wait for PTY cat echo
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = active_session(&app).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "expected chip click to send /pull-request; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_outside_chip_rect_does_nothing() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 50, // outside chip
            row: 10,    // outside chip
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let session = active_session(&app).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            !screen_text.contains("/pull-request"),
            "click outside any chip must not fire; got: {screen_text:?}"
        );
    }

    /// Clicking the chip-row running-process count (`● Np`) opens the
    /// ProcessList modal for the focused workspace, mirroring `K` on it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_procs_count_opens_process_list() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        // The chip-row procs count reports a clickable rect during draw.
        app.procs_link_rect = Some((
            ws_id,
            ratatui::layout::Rect {
                x: 60,
                y: 30,
                width: 4,
                height: 1,
            },
        ));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 61,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        assert!(
            matches!(app.modal, Some(Modal::ProcessList { workspace_id, .. }) if workspace_id == ws_id),
            "clicking the procs count should open ProcessList for that workspace; got {:?}",
            app.modal
        );
    }

    /// A click that misses the procs-count rect must not open the modal.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_outside_procs_count_does_not_open_process_list() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.procs_link_rect = Some((
            ws_id,
            ratatui::layout::Rect {
                x: 60,
                y: 30,
                width: 4,
                height: 1,
            },
        ));

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10, // outside the procs rect
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        assert!(
            !matches!(app.modal, Some(Modal::ProcessList { .. })),
            "click off the procs count must not open ProcessList; got {:?}",
            app.modal
        );
    }

    /// Regression (#224): while any modal is open, a left click landing on an
    /// attention row must be swallowed, not attach to the workspace beneath
    /// the overlay.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_attention_row_while_modal_open_does_not_attach() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        app.view = crate::ui::View::Dashboard;

        app.attention_rects = vec![(
            ws_id,
            ratatui::layout::Rect {
                x: 5,
                y: 10,
                width: 20,
                height: 1,
            },
        )];
        app.modal = Some(Modal::WorkspaceActions);

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        assert!(
            matches!(app.view, crate::ui::View::Dashboard),
            "attention click under a modal must not attach; got {:?}",
            app.view
        );
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "the open modal must be untouched by the swallowed click; got {:?}",
            app.modal
        );
    }

    /// Regression (#224): the modal click gate covers every left-click
    /// target, not just attention rows — here the attached-view chip-row
    /// procs count (`procs_link_rect` is only ever set by the attached
    /// render): a click on it under a modal must not replace that modal
    /// with ProcessList.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_procs_count_while_modal_open_is_swallowed() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.procs_link_rect = Some((
            ws_id,
            ratatui::layout::Rect {
                x: 60,
                y: 30,
                width: 4,
                height: 1,
            },
        ));
        app.modal = Some(Modal::Error {
            message: "boom".into(),
        });

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 61,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        assert!(
            matches!(app.modal, Some(Modal::Error { .. })),
            "procs click under a modal must not open ProcessList; got {:?}",
            app.modal
        );
    }

    /// Clicking a dashboard footer hint fires the corresponding key, exactly
    /// as if it had been pressed. `/` enters filter mode.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_dashboard_footer_hint_fires_key() {
        use crate::ui::footer::FooterHintAction;
        use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.view = crate::ui::View::Dashboard;

        app.footer_hint_rects = vec![(
            ratatui::layout::Rect {
                x: 0,
                y: 40,
                width: 8,
                height: 1,
            },
            FooterHintAction::Key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE)),
        )];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 40,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        assert!(
            app.dashboard.filter.is_some(),
            "clicking the `/` footer hint must enter filter mode"
        );
    }

    /// Clicking the `^x` leader pill in the attached footer arms the leader,
    /// exactly like pressing Ctrl-x.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_attached_footer_leader_pill_arms_leader() {
        use crate::ui::footer::FooterHintAction;
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        app.footer_hint_rects = vec![(
            ratatui::layout::Rect {
                x: 10,
                y: 40,
                width: 4,
                height: 1,
            },
            FooterHintAction::ArmLeader,
        )];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 11,
            row: 40,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        assert!(
            app.leader_pending,
            "clicking the ^x pill must arm the attached-view leader"
        );
    }

    /// The `^x` pill routes a real `Ctrl-x` through the handlers rather than
    /// poking `leader_pending` directly, so a second click behaves like a
    /// second `Ctrl-x` keypress: it clears the leader (and sends a literal
    /// `^X`) instead of leaving the leader stuck armed.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn double_click_attached_leader_pill_does_not_stick_armed() {
        use crate::ui::footer::FooterHintAction;
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        app.footer_hint_rects = vec![(
            ratatui::layout::Rect {
                x: 10,
                y: 40,
                width: 4,
                height: 1,
            },
            FooterHintAction::ArmLeader,
        )];
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 11,
            row: 40,
            modifiers: KeyModifiers::NONE,
        };

        handle_mouse(&mut app, click).await;
        assert!(app.leader_pending, "first ^x click arms the leader");

        handle_mouse(&mut app, click).await;
        assert!(
            !app.leader_pending,
            "second ^x click must clear the leader, matching double Ctrl-x \
             (not leave it stuck armed)"
        );
    }

    /// Clicking an attached footer keybind hint arms the leader and dispatches
    /// the command in one click. `^x a` opens the agents panel.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_attached_footer_hint_dispatches_leader_command() {
        use crate::ui::footer::FooterHintAction;
        use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        app.footer_hint_rects = vec![(
            ratatui::layout::Rect {
                x: 20,
                y: 40,
                width: 9,
                height: 1,
            },
            FooterHintAction::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        )];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 22,
            row: 40,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        assert!(
            matches!(app.modal, Some(crate::ui::modal::Modal::AgentsPanel { .. })),
            "clicking the `a` footer hint must open the agents panel via the leader"
        );
        assert!(
            !app.leader_pending,
            "leader must clear once the click's follow-up key is consumed"
        );
    }

    /// Chip click from `View::Dashboard` dispatches the command to the selected
    /// workspace's session, not `active_session` (which returns `None` in the
    /// dashboard view).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_chip_in_dashboard_view_fires_pinned_command() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        // Switch to dashboard view — active_session() now returns None.
        app.view = crate::ui::View::Dashboard;
        // Point selectable at the workspace so selected_target() returns it.
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        // Wait for PTY cat echo.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "dashboard chip click must dispatch /pull-request to the workspace session; got: {screen_text:?}"
        );
    }

    /// Ctrl-X arms `leader_pending`; a subsequent digit fires the chip command
    /// to the selected workspace's session.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_ctrl_x_then_digit_fires_pinned_chip() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        // Ctrl-X — arms the leader.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending, "Ctrl-X must arm leader_pending");

        // '1' — fires chip 0, clears leader.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            !app.leader_pending,
            "leader must clear after digit follow-up"
        );

        // cat echoes input back; verify the command reached the PTY.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "dashboard Ctrl-X+1 must dispatch /pull-request to the workspace PTY; got: {screen_text:?}"
        );
    }

    /// Ctrl-X then a non-digit key clears the leader without firing any chip.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_ctrl_x_then_non_digit_clears_leader_no_fire() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        // Ctrl-X — arms the leader.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);

        // 'z' — a key with no leader binding; clears the leader without
        // firing. (Not 'a', which now opens the agents panel.)
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            !app.leader_pending,
            "leader must clear after non-digit follow-up"
        );

        // No chip command should have been dispatched.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            !screen_text.contains("/pull-request"),
            "non-digit follow-up must not fire any chip; got: {screen_text:?}"
        );
    }

    /// Ctrl-X + a digit whose index exceeds the number of visible chip_rects
    /// is a no-op (fire_chip guards on idx >= chip_rects.len()).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_ctrl_x_digit_beyond_visible_chips_is_noop() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);

        // Three commands in cache but only two chip_rects rendered.
        app.pinned_commands_cache = vec![
            crate::commands::pinned::PinnedCommand {
                label: "PR".into(),
                command: "/pull-request".into(),
            },
            crate::commands::pinned::PinnedCommand {
                label: "B".into(),
                command: "/build".into(),
            },
            crate::commands::pinned::PinnedCommand {
                label: "T".into(),
                command: "/test".into(),
            },
        ];
        app.chip_rects = vec![
            ratatui::layout::Rect {
                x: 5,
                y: 30,
                width: 7,
                height: 1,
            },
            ratatui::layout::Rect {
                x: 13,
                y: 30,
                width: 5,
                height: 1,
            },
        ];

        // Ctrl-X then '3' — index 2, beyond chip_rects.len() == 2.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            !screen_text.contains("/test"),
            "digit beyond visible chips must not dispatch any command; got: {screen_text:?}"
        );
    }

    /// A chip click from the dashboard echoes the dispatched command
    /// into the reply input as visual confirmation, and sets a
    /// wall-clock deadline (`reply_draft_clear_at_ms`) so the tick
    /// handler wipes it shortly afterward.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn chip_dispatch_echoes_command_into_reply_input() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);
        // No pre-existing draft.
        assert_eq!(app.dashboard.reply_draft, "");
        assert!(app.dashboard.reply_draft_clear_at_ms.is_none());

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let now_before_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        handle_mouse(
            &mut app,
            MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 6,
                row: 30,
                modifiers: KeyModifiers::NONE,
            },
        )
        .await;

        // Draft echoes the dispatched command.
        assert_eq!(
            app.dashboard.reply_draft, "/pull-request",
            "chip dispatch must echo the command into reply_draft"
        );
        // Deadline is set in the future (sanity bound: within 5 seconds).
        let deadline = app
            .dashboard
            .reply_draft_clear_at_ms
            .expect("deadline must be set");
        assert!(
            deadline > now_before_ms && deadline < now_before_ms + 5_000,
            "deadline {deadline} should be slightly after {now_before_ms}"
        );
    }

    /// Backspace and Char keystrokes in the reply input cancel any
    /// pending chip-echo auto-clear so the user's edits aren't wiped
    /// by the tick handler mid-typing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn user_typing_in_reply_cancels_chip_echo_deadline() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        // Simulate state right after a chip dispatch: draft echoes the
        // command, deadline is set.
        app.dashboard.reply_draft = "/pull-request".to_string();
        app.dashboard.reply_draft_clear_at_ms = Some(u64::MAX);

        // User types a char.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        )
        .await
        .unwrap();

        // Deadline is cleared so the tick handler won't wipe their edits.
        assert!(
            app.dashboard.reply_draft_clear_at_ms.is_none(),
            "Char keystroke must cancel the chip-echo auto-clear deadline"
        );

        // Reset and try Backspace.
        app.dashboard.reply_draft_clear_at_ms = Some(u64::MAX);
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            app.dashboard.reply_draft_clear_at_ms.is_none(),
            "Backspace keystroke must cancel the chip-echo auto-clear deadline"
        );
    }

    /// A chip click from the dashboard on a workspace with NO live
    /// session must auto-spawn one so the chip command isn't silently
    /// dropped. Mirrors the production fix for the inline-reply gap.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_chip_auto_spawns_session_when_missing() {
        use crate::data::store::NewWorkspace;
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        // Remote-control defaults ON, which appends `--remote-control` to the
        // spawned agent command. The real `claude` understands that flag; the
        // `cat` stand-in does not — it errors out and exits immediately. The
        // command then only lands on screen if the PTY's own echo wins the
        // race against `cat`'s teardown, which flakes under CI load. Disable
        // it so `cat` stays alive and deterministically echoes the dispatched
        // command, mirroring the other fake-binary tests' RemoteOpts::disabled().
        store.set_setting("remote_control", "off").unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let repo_id = app
            .store
            .add_repo(std::path::Path::new("."), "scratch", "test")
            .unwrap();
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "auto-spawn-test",
                branch: "main",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        app.refresh().unwrap();

        // Critical precondition: NO session spawned for this workspace.
        assert!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .is_none(),
            "precondition: workspace must not have a session yet"
        );

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        // The session must have been auto-spawned by fire_chip.
        assert!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .is_some(),
            "fire_chip must auto-spawn a session for the selected workspace"
        );

        // And the command must have reached the new session's PTY.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "chip command must dispatch to the auto-spawned session; got: {screen_text:?}"
        );
    }

    /// A second Ctrl-X while the leader is armed must clear it (cancel the
    /// chord), not silently re-arm. Matches the attached-view leader
    /// behavior where the follow-up key always clears the leader.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_double_ctrl_x_clears_leader() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);

        // First Ctrl-X: arms.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending, "first Ctrl-X must arm leader");

        // Second Ctrl-X: must cancel (clear) the leader, not stay armed.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(
            !app.leader_pending,
            "second Ctrl-X must cancel the chord, not re-arm"
        );
    }

    /// Ctrl-X then 'a' opens the AgentsPanel modal for the selected workspace
    /// on the dashboard.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_ctrl_x_then_a_opens_agents_panel() {
        use crate::ui::modal::Modal;
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);
        app.modal = None;

        // Ctrl-X — arms the leader.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending, "Ctrl-X must arm leader_pending");

        // 'a' — must open AgentsPanel for the selected workspace.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending, "leader must clear after 'a'");
        match &app.modal {
            Some(Modal::AgentsPanel {
                workspace_id,
                selected,
            }) => {
                assert_eq!(
                    *workspace_id, ws_id,
                    "AgentsPanel must reference the selected workspace"
                );
                assert_eq!(*selected, 0);
            }
            other => panic!("expected AgentsPanel modal; got {other:?}"),
        }
    }

    /// Ctrl-X then 'a' opens the AgentsPanel modal for the focused pane's
    /// workspace in the attached view.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_ctrl_x_then_a_opens_agents_panel() {
        use crate::ui::modal::Modal;
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        let target = test_target(&app, ws_id);
        app.modal = None;

        // Ctrl-X — arms the leader.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending, "Ctrl-X must arm leader_pending");

        // 'a' — must open AgentsPanel for the focused workspace.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending, "leader must clear after 'a'");
        match &app.modal {
            Some(Modal::AgentsPanel {
                workspace_id,
                selected,
            }) => {
                assert_eq!(
                    *workspace_id, ws_id,
                    "AgentsPanel must reference the focused workspace"
                );
                assert_eq!(*selected, 0);
            }
            other => panic!("expected AgentsPanel modal; got {other:?}"),
        }
    }

    /// A chip click in the attached view dispatches the command but must
    /// NOT clear the dashboard reply draft or overwrite the dashboard
    /// pane focus — those state slots aren't visible from the attached
    /// view and trampling them would leak across the view boundary.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_chip_click_preserves_dashboard_draft_and_focus() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        // We're in View::Attached (set by spawn_attached_workspace).
        assert!(matches!(app.view, crate::ui::View::Attached(_)));

        // Seed dashboard-scoped state the user can't see from here.
        app.dashboard.reply_draft = "hello agent".into();
        app.focus = crate::ui::PaneFocus::ProjectManager;

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&mut app, click).await;

        // Command still dispatched.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "attached-view chip click must still dispatch the command; got: {screen_text:?}"
        );
        drop(parser);

        // But dashboard state must be unchanged.
        assert_eq!(
            app.dashboard.reply_draft, "hello agent",
            "attached-view chip click must not clear the dashboard reply draft"
        );
        assert!(
            matches!(app.focus, crate::ui::PaneFocus::ProjectManager),
            "attached-view chip click must not overwrite the dashboard pane focus"
        );
    }

    #[test]
    fn wrap_paste_bytes_wraps_with_bracketed_markers() {
        let out = wrap_paste_bytes("hello world");
        assert_eq!(out, b"\x1b[200~hello world\x1b[201~");
    }

    #[test]
    fn wrap_paste_bytes_handles_empty_content() {
        // Edge case: a paste of empty string still emits the markers so the
        // far side sees a zero-length paste boundary rather than nothing.
        let out = wrap_paste_bytes("");
        assert_eq!(out, b"\x1b[200~\x1b[201~");
    }

    #[test]
    fn wrap_paste_bytes_preserves_multiline_and_special_chars() {
        let out = wrap_paste_bytes("line1\nline2\t  trailing");
        assert_eq!(out, b"\x1b[200~line1\nline2\t  trailing\x1b[201~");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paste_in_attached_view_sends_bracketed_payload_to_pty() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));

        handle_event(&mut app, &shared, CtEvent::Paste("hello paste".into()))
            .await
            .unwrap();

        // cat echoes input back. The bracketed-paste markers are unknown
        // CSI sequences to vt100 and get swallowed; the inner content
        // appears on the screen verbatim.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = active_session(&app).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("hello paste"),
            "paste content must reach the PTY; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paste_in_dashboard_with_pm_focused_sends_bracketed_to_pm() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        spawn_pm_for_test(&mut app);
        // Dashboard view + PM visible + PM focused — same condition that
        // routes keystrokes to the PM session.
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::ProjectManager;
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));

        handle_event(&mut app, &shared, CtEvent::Paste("hello pm".into()))
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let pm = app.pm.as_ref().unwrap();
        let parser = pm.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("hello pm"),
            "PM-focused paste must reach the PM PTY; got: {screen_text:?}"
        );
    }

    #[test]
    fn paste_char_to_key_translates_newline_to_enter() {
        let k = paste_char_to_key('\n');
        assert!(matches!(k.code, KeyCode::Enter));
    }

    #[test]
    fn paste_char_to_key_translates_cr_to_enter() {
        let k = paste_char_to_key('\r');
        assert!(matches!(k.code, KeyCode::Enter));
    }

    #[test]
    fn paste_char_to_key_translates_tab() {
        let k = paste_char_to_key('\t');
        assert!(matches!(k.code, KeyCode::Tab));
    }

    #[test]
    fn paste_char_to_key_passes_through_printable() {
        let k = paste_char_to_key('a');
        assert!(matches!(k.code, KeyCode::Char('a')));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_resolves_related_repos_to_additional_dirs() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let backend_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let _frontend_id = store
            .add_repo(std::path::Path::new("/work/frontend"), "frontend", "")
            .unwrap();
        store
            .set_repo_related_repos(backend_id, Some("frontend"))
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id: backend_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let info = build_spawn_info(&app, ws_id);
        assert!(info.is_some());
        let (_id, _path, mode, _repo_path, _agent) = info.unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh {
                additional_dirs,
                custom_instructions,
                ..
            } => {
                assert_eq!(
                    additional_dirs,
                    vec![std::path::PathBuf::from("/work/frontend")],
                    "additional_dirs should resolve to frontend's source path"
                );
                let prompt = custom_instructions.expect("read-only fragment must be folded in");
                assert!(
                    prompt.contains("/work/frontend"),
                    "system prompt missing related path: {prompt}"
                );
                assert!(
                    prompt.contains("MUST NOT edit"),
                    "system prompt missing read-only directive: {prompt}"
                );
            }
            other => panic!("expected Fresh mode; got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_populates_doctrine() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let (_id, _path, mode, _repo_path, _agent) = build_spawn_info(&app, ws_id).unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh { doctrine, .. } => {
                let d = doctrine.expect("doctrine must be populated");
                assert!(
                    d.contains("superpowers"),
                    "claude doctrine includes superpowers: {d}"
                );
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_doctrine_is_agent_tailored_for_hermes() {
        // Proves the agent-tailored doctrine flows through the call site for a
        // non-Claude agent: Hermes must get the doctrine but NOT the superpowers
        // clause (which is Claude/Pi-only), while keeping the wsx-skill clause.
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "hermes-ws",
                branch: "backend/hermes-ws",
                worktree_path: std::path::Path::new("/wt/hermes-ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Hermes,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let (_id, _path, mode, _repo_path, _agent) = build_spawn_info(&app, ws_id).unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh { doctrine, .. } => {
                let d = doctrine.expect("doctrine must be populated");
                assert!(
                    !d.contains("superpowers"),
                    "hermes doctrine must omit superpowers: {d}"
                );
                assert!(
                    d.contains("wsx skill"),
                    "hermes doctrine keeps wsx skill clause: {d}"
                );
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_filters_self_reference() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let backend_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        store
            .set_repo_related_repos(backend_id, Some("backend"))
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id: backend_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let (_id, _path, mode, _repo_path, _agent) = build_spawn_info(&app, ws_id).unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh {
                additional_dirs,
                custom_instructions,
                ..
            } => {
                assert!(
                    additional_dirs.is_empty(),
                    "self-reference must be filtered"
                );
                assert!(
                    custom_instructions.is_none(),
                    "no related dirs => no fragment"
                );
            }
            other => panic!("expected Fresh mode; got {other:?}"),
        }
    }

    /// Shared workspaces spawn their primary instance inside a real tmux
    /// server and persist the derived session name to `session_ref`, so
    /// later consumers (kill, archive, `wsx shared list`) can reuse it
    /// without re-deriving. Skips when tmux is absent; isolates via
    /// TMUX_TMPDIR so the user's own tmux server is untouched.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_workspace_attach_records_tmux_session_ref() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        if !crate::pty::tmux::is_available() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        let tmpdir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.set("TMUX_TMPDIR", tmpdir.path().to_str().unwrap());
        // WSX_CLAUDE_BIN must point at a real script: `/bin/sh` would receive
        // the claude CLI args and reject them. Write a wrapper that ignores
        // args and sleeps so the tmux window keeps a live child.
        let script = tmpdir.path().join("fake-agent.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_CLAUDE_BIN", script.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w",
                branch: "r/w",
                worktree_path: tmpdir.path(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        attach_workspace(&mut app, ws_id).unwrap();
        let inst = app.store.workspace_agents(ws_id).unwrap();
        assert_eq!(inst[0].session_ref.as_deref(), Some("wsx-r-w"));
        let s = app.sessions.get(inst[0].id).unwrap();
        assert_eq!(s.tmux_session.as_deref(), Some("wsx-r-w"));
        // cleanup: kill backend so the private server dies
        s.kill_backend();
    }

    /// C1 regression: `session_ref` is the source of truth. After a workspace
    /// is renamed, a fresh spawn must reuse the OLD stored tmux name rather
    /// than re-deriving from the current name — otherwise `-A` would create a
    /// second session and orphan the original agent. Mirrors the
    /// `shared_workspace_attach_records_tmux_session_ref` tmux isolation.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_spawn_reuses_stored_session_ref_after_rename() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        if !crate::pty::tmux::is_available() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        let tmpdir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.set("TMUX_TMPDIR", tmpdir.path().to_str().unwrap());
        let script = tmpdir.path().join("fake-agent.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_CLAUDE_BIN", script.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "new-name",
                branch: "r/new-name",
                worktree_path: tmpdir.path(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();
        // Seed the primary with a session_ref from an OLD name (pre-rename).
        let primary = store
            .add_primary_agent(ws_id, crate::pty::session::AgentKind::Claude, 0)
            .unwrap();
        store
            .set_instance_session_ref(primary.id, "wsx-r-old-name")
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        attach_workspace(&mut app, ws_id).unwrap();
        // The spawned session must use the OLD stored name, NOT "wsx-r-new-name".
        let s = app.sessions.get(primary.id).unwrap();
        assert_eq!(
            s.tmux_session.as_deref(),
            Some("wsx-r-old-name"),
            "spawn re-derived the name from the renamed workspace instead of \
             reusing the stored session_ref"
        );
        // The stored ref is unchanged.
        let reloaded = app.store.workspace_agents(ws_id).unwrap();
        assert_eq!(reloaded[0].session_ref.as_deref(), Some("wsx-r-old-name"));
        s.kill_backend();
    }

    /// I3: two shared workspaces whose names sanitize to the same tmux base
    /// name must not collide. When the second instance derives a name already
    /// claimed by the first's stored `session_ref`, `tmux_name_for` appends the
    /// workspace id. No tmux server needed — this is pure name derivation.
    #[test]
    fn tmux_name_for_disambiguates_sanitization_collision() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        // repo `a` + ws `b-c`  → wsx-a-b-c
        // repo `a-b` + ws `c`  → wsx-a-b-c  (collision)
        let repo1 = store
            .add_repo(std::path::Path::new("/tmp/a"), "a", "")
            .unwrap();
        let repo2 = store
            .add_repo(std::path::Path::new("/tmp/a-b"), "a-b", "")
            .unwrap();
        let ws1 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo1,
                name: "b-c",
                branch: "a/b-c",
                worktree_path: std::path::Path::new("/tmp/a/b-c"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(ws1, WorkspaceState::Ready)
            .unwrap();
        let p1 = store
            .add_primary_agent(ws1, crate::pty::session::AgentKind::Claude, 0)
            .unwrap();
        // ws1 already occupies the colliding base name.
        store.set_instance_session_ref(p1.id, "wsx-a-b-c").unwrap();

        let ws2 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo2,
                name: "c",
                branch: "a-b/c",
                worktree_path: std::path::Path::new("/tmp/a-b/c"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(ws2, WorkspaceState::Ready)
            .unwrap();
        let p2 = store
            .add_primary_agent(ws2, crate::pty::session::AgentKind::Claude, 0)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let inst2 = app.store.workspace_agents_by_id(p2.id).unwrap().unwrap();
        let name = crate::app::tmux_name_for(&app, ws2, &inst2).unwrap();
        assert_eq!(
            name,
            format!("wsx-a-b-c-{}", ws2.0),
            "collision with ws1's stored name should append the workspace id"
        );

        // ws1 (which owns the base name) still derives the bare name.
        let inst1 = app.store.workspace_agents_by_id(p1.id).unwrap().unwrap();
        assert_eq!(
            crate::app::tmux_name_for(&app, ws1, &inst1).unwrap(),
            "wsx-a-b-c",
            "the instance that owns the stored ref keeps the bare name"
        );
    }

    /// I1: unsharing a workspace via the TUI must not leave a detached tmux
    /// agent orphaned. A shared workspace with a non-running (detached-but-
    /// alive) instance holds a `session_ref`; toggling it to unshared must
    /// kill that tmux session directly and clear the ref, so a later archive
    /// has nothing left to leak. Uses a fake `WSX_TMUX_BIN` recorder so no
    /// real tmux server is needed; the instance is never running, so no agent
    /// respawn is triggered.
    /// I2: toggling a direct workspace to shared while tmux is unavailable
    /// must NOT flip the flag or kill any running agent. Instead it raises the
    /// AgentMissing modal and returns. Points WSX_TMUX_BIN at a nonexistent
    /// path so `is_available()` reports false without depending on the host.
    #[tokio::test]
    async fn toggle_to_shared_without_tmux_is_a_noop_with_modal() {
        use crate::data::store::NewWorkspace;
        use crate::ui::modal::Modal;

        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        env.set("WSX_TMUX_BIN", "/nonexistent/wsx-no-tmux-here");

        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let repo_id = app
            .store
            .add_repo(std::path::Path::new("."), "scratch", "test")
            .unwrap();
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "share-me",
                branch: "main",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: crate::pty::session::AgentKind::Codex,
                shared: false, // starts direct; toggle proposes -> shared
            })
            .unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let inst = test_primary_instance(&app, ws_id);
        app.sessions
            .spawn(
                inst,
                ws_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Codex,
                None,
            )
            .unwrap();
        app.refresh().unwrap();
        let old_session = app.sessions.get(inst).expect("session should be running");

        crate::app::toggle_workspace_shared(&mut app, ws_id).unwrap();

        // Flag NOT flipped.
        let ws = app.store.workspace_by_id(ws_id).unwrap().unwrap();
        assert!(!ws.shared, "flag must stay direct when tmux is missing");
        // Modal surfaced.
        match &app.modal {
            Some(Modal::AgentMissing { ws_id: mid, .. }) => assert_eq!(*mid, ws_id),
            other => panic!("expected AgentMissing modal, got {other:?}"),
        }
        // Running session untouched (same Arc).
        let now_session = app.sessions.get(inst).expect("session must survive");
        assert!(
            Arc::ptr_eq(&old_session, &now_session),
            "the running agent must not be killed when tmux is missing"
        );
    }

    #[tokio::test]
    async fn toggle_unshare_kills_detached_tmux_and_clears_ref() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};

        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("tmux-calls.log");
        let fake = dir.path().join("fake-tmux.sh");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let mut env = EnvGuard::new();
        env.set("WSX_TMUX_BIN", fake.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w",
                branch: "r/w",
                worktree_path: dir.path(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();
        let primary = store
            .add_primary_agent(ws_id, crate::pty::session::AgentKind::Claude, 0)
            .unwrap();
        store
            .set_instance_session_ref(primary.id, "wsx-r-w")
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        crate::app::toggle_workspace_shared(&mut app, ws_id).unwrap();

        // Flag flipped to unshared.
        let ws = app.store.workspace_by_id(ws_id).unwrap().unwrap();
        assert!(!ws.shared, "toggle should flip shared -> false");
        // The detached tmux session was killed.
        let calls = std::fs::read_to_string(&log).unwrap();
        assert!(
            calls.contains("kill-session -t =wsx-r-w"),
            "detached tmux session must be killed on unshare, got: {calls:?}"
        );
        // The stale ref is cleared.
        let reloaded = app.store.workspace_agents(ws_id).unwrap();
        assert_eq!(
            reloaded[0].session_ref, None,
            "session_ref must be cleared on unshare"
        );
    }

    /// Toggling a workspace to shared must eagerly spawn agents that are NOT
    /// currently running — not just restart running ones. A stopped agent
    /// previously got a flag flip only: no tmux session existed until the
    /// user happened to attach locally, so the workspace showed up
    /// shared-but-dead (red badge, hidden from the remote picker) and the
    /// share looked like it failed. Uses a fake `WSX_TMUX_BIN` recorder, so
    /// the "agent" is the recorder script itself — no real tmux needed.
    #[tokio::test]
    async fn toggle_to_shared_spawns_stopped_instances_into_tmux() {
        use crate::data::store::{NewWorkspace, WorkspaceState};

        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("tmux-calls.log");
        let fake = dir.path().join("fake-tmux.sh");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\necho \"$@\" >> {}\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let mut env = EnvGuard::new();
        env.set("WSX_TMUX_BIN", fake.to_str().unwrap());
        env.set("WSX_CLAUDE_BIN", crate::test_support::cat_path());

        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w",
                branch: "r/w",
                worktree_path: dir.path(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false, // starts direct; toggle flips -> shared
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();
        let primary = store
            .add_primary_agent(ws_id, crate::pty::session::AgentKind::Claude, 0)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(
            app.sessions.get(primary.id).is_none(),
            "precondition: the agent is not running"
        );

        crate::app::toggle_workspace_shared(&mut app, ws_id).unwrap();

        let ws = app.store.workspace_by_id(ws_id).unwrap().unwrap();
        assert!(ws.shared, "toggle should flip shared -> true");
        assert!(
            app.sessions.get(primary.id).is_some(),
            "a stopped agent must be spawned into tmux when sharing"
        );
        let reloaded = app.store.workspace_agents(ws_id).unwrap();
        assert_eq!(
            reloaded[0].session_ref.as_deref(),
            Some("wsx-r-w"),
            "the eager shared spawn must persist the tmux session_ref"
        );
    }

    /// After a wsx restart, a shared workspace's tmux session can outlive
    /// the wsx client that spawned it — no `Session` in `app.sessions`, but
    /// the server-side session is still alive. `classify_status` should
    /// surface that as `Status::Detached` rather than the classifier's
    /// default `Idle`, while a direct workspace (which never touches tmux)
    /// stays plain `Idle`. Uses a fake `WSX_TMUX_BIN` recorder that exits 0
    /// for every invocation, so `has-session` reads "alive" without a real
    /// tmux server.
    #[test]
    fn shared_workspace_with_dead_client_but_live_tmux_is_detached() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        use crate::ui::dashboard::status::Status;

        let tmpdir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        let script = tmpdir.path().join("fake-tmux.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_TMUX_BIN", script.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "")
            .unwrap();

        let shared_path = tmpdir.path().join("shared-w");
        std::fs::create_dir_all(&shared_path).unwrap();
        let shared_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "shared-w",
                branch: "r/shared-w",
                worktree_path: &shared_path,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(shared_id, WorkspaceState::Ready)
            .unwrap();
        let primary = store
            .add_primary_agent(
                shared_id,
                crate::pty::session::AgentKind::Claude,
                crate::data::store::now_ms(),
            )
            .unwrap();
        store
            .set_instance_session_ref(primary.id, "wsx-r-shared-w")
            .unwrap();

        let direct_path = tmpdir.path().join("direct-w");
        std::fs::create_dir_all(&direct_path).unwrap();
        let direct_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "direct-w",
                branch: "r/direct-w",
                worktree_path: &direct_path,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(direct_id, WorkspaceState::Ready)
            .unwrap();

        // `App::new` -> `refresh()` -> `refresh_shared_detached()` runs its
        // first sweep unthrottled (`shared_detached_polled_ms` starts at 0),
        // so the sweep has already populated `shared_detached` by the time
        // `App::new` returns.
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();

        let shared_ws = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == shared_id)
            .map(|(_, w)| w.clone())
            .unwrap();
        let direct_ws = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == direct_id)
            .map(|(_, w)| w.clone())
            .unwrap();

        assert_eq!(app.classify_status(&shared_ws), Status::Detached);
        assert_eq!(app.classify_status(&direct_ws), Status::Idle);
    }

    /// A workspace whose only live wsx client belongs to a NON-primary
    /// instance is not detached: someone is watching it. The sweep must
    /// consider every instance's session, not just the primary's — with a
    /// primary-only check, `has_client` reads false while the primary's
    /// `session_ref` reads alive, and the workspace is wrongly marked `◆`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_workspace_with_running_added_instance_is_not_detached() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        use crate::ui::dashboard::status::Status;

        let tmpdir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        let script = tmpdir.path().join("fake-tmux.sh");
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_TMUX_BIN", script.to_str().unwrap());
        env.set("WSX_CODEX_BIN", cat_path());

        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "")
            .unwrap();
        let ws_path = tmpdir.path().join("w");
        std::fs::create_dir_all(&ws_path).unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w",
                branch: "r/w",
                worktree_path: &ws_path,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();
        let primary = store
            .add_primary_agent(
                ws_id,
                crate::pty::session::AgentKind::Claude,
                crate::data::store::now_ms(),
            )
            .unwrap();
        store
            .set_instance_session_ref(primary.id, "wsx-r-w")
            .unwrap();
        let added = store
            .add_workspace_agent(ws_id, crate::pty::session::AgentKind::Codex)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Live client on the ADDED instance only; the primary has none.
        app.sessions
            .spawn(
                added.id,
                ws_id,
                &ws_path,
                80,
                24,
                crate::pty::session::SpawnMode::Fresh {
                    rename_ctx: None,
                    custom_instructions: None,
                    doctrine: None,
                    additional_dirs: vec![],
                    yolo: false,
                },
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Codex,
                None,
            )
            .unwrap();

        // Force a fresh sweep now that the session exists (App::new's first
        // sweep ran before the spawn).
        app.shared_detached_polled_ms = 0;
        app.refresh().unwrap();

        let ws = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == ws_id)
            .map(|(_, w)| w.clone())
            .unwrap();
        assert_ne!(
            app.classify_status(&ws),
            Status::Detached,
            "a live client on a non-primary instance means someone is attached; not detached"
        );
    }

    /// Test helper: create an App with N repos registered in the store
    /// and loaded into app.repos. Uses a unique tmpdir per call so paths
    /// don't collide.
    fn make_app_with_n_repos(n: usize) -> (App, Vec<crate::data::store::RepoId>) {
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..n {
            let path =
                std::env::temp_dir().join(format!("wsx-fold-test-{}-{}", std::process::id(), i));
            let id = store.add_repo(&path, &format!("repo-{i}"), "").unwrap();
            ids.push(id);
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-fold-test")).unwrap();
        app.refresh().unwrap();
        (app, ids)
    }

    async fn press(app: &mut App, ch: char, mods: KeyModifiers) {
        handle_key_dashboard(app, KeyEvent::new(KeyCode::Char(ch), mods))
            .await
            .unwrap();
    }

    async fn press_key(app: &mut App, code: KeyCode) {
        handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn z_alone_arms_leader_without_action() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        assert!(app.z_leader_pending, "z should arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "z alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn zz_toggles_focused_repo_fold() {
        let (mut app, ids) = make_app_with_n_repos(2);
        app.select_index(0);
        let rid = ids[0];
        let key = rid.0 as u64;
        let before = app.dashboard.folded.get(&key).copied();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'z', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zz");
        let after = app.dashboard.folded.get(&key).copied();
        assert_ne!(
            before, after,
            "zz should change the fold state for the focused repo"
        );
    }

    #[tokio::test]
    async fn za_expands_all_repos() {
        let (mut app, ids) = make_app_with_n_repos(3);
        // Pre-fold one repo explicitly so we can see the "expand all" override.
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'a', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after za");
        for id in &ids {
            let key = id.0 as u64;
            assert_eq!(
                app.dashboard.folded.get(&key).copied(),
                Some(false),
                "za should set repo {key} to expanded (false)"
            );
        }
    }

    #[tokio::test]
    async fn z_shift_m_folds_all_repos() {
        let (mut app, ids) = make_app_with_n_repos(3);
        // Pre-expand one repo explicitly so we can see the "fold all" override.
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'M', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "leader should clear after zM");
        for id in &ids {
            let key = id.0 as u64;
            assert_eq!(
                app.dashboard.folded.get(&key).copied(),
                Some(true),
                "zM should set repo {key} to folded (true)"
            );
        }
    }

    #[tokio::test]
    async fn z_then_unknown_clears_leader_without_action() {
        let (mut app, _) = make_app_with_n_repos(2);
        let selected_before = app.dashboard.selected;
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'x', KeyModifiers::NONE).await;
        assert!(
            !app.z_leader_pending,
            "leader should clear after unknown key"
        );
        assert_eq!(
            app.dashboard.folded, folded_before,
            "unknown follow-up should leave fold state unchanged"
        );
        assert_eq!(
            app.dashboard.selected, selected_before,
            "unknown follow-up should be eaten, not pass through to selection"
        );
    }

    #[tokio::test]
    async fn z_then_esc_clears_leader() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press_key(&mut app, KeyCode::Esc).await;
        assert!(!app.z_leader_pending, "Esc should clear the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "Esc should not change fold state"
        );
    }

    #[tokio::test]
    async fn j_alias_advances_selection_like_down() {
        let (mut app, _) = make_app_with_n_repos(3);
        app.select_index(0);
        press(&mut app, 'j', KeyModifiers::NONE).await;
        assert_eq!(app.dashboard.selected, 1, "j should advance like Down");
    }

    #[tokio::test]
    async fn k_alias_retreats_selection_like_up() {
        let (mut app, _) = make_app_with_n_repos(3);
        app.select_index(2);
        press(&mut app, 'k', KeyModifiers::NONE).await;
        assert_eq!(app.dashboard.selected, 1, "k should retreat like Up");
    }

    #[tokio::test]
    async fn k_does_not_open_process_list_anymore() {
        // `k` is now a nav alias for Up. Process list must be opened by `K`.
        let (mut app, _) = make_app_with_n_repos(1);
        app.select_index(0);
        press(&mut app, 'k', KeyModifiers::NONE).await;
        assert!(
            app.modal.is_none(),
            "k must not open ProcessList; it's now a nav alias"
        );
    }

    #[tokio::test]
    async fn shift_k_opens_process_list_on_workspace() {
        use crate::data::store::{NewWorkspace, WorkspaceState};
        let (mut app, ids) = make_app_with_n_repos(1);
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id: ids[0],
                name: "alpha",
                branch: "repo-0/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        app.store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();
        app.refresh().unwrap();
        // Find and select the workspace row.
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(id) if *id == ws_id))
            .expect("workspace should appear in selectable list");
        app.select_index(idx);
        press(&mut app, 'K', KeyModifiers::SHIFT).await;
        assert!(
            matches!(app.modal, Some(Modal::ProcessList { workspace_id, .. }) if workspace_id == ws_id),
            "K on a workspace row should open ProcessList"
        );
    }

    #[tokio::test]
    async fn shift_k_moves_selected_repo_up() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.select_index(1); // select repo-1 (Repo header)
        press(&mut app, 'K', KeyModifiers::SHIFT).await;

        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(
            order,
            vec![ids[1], ids[0], ids[2]],
            "repo-1 moved above repo-0"
        );
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Repo(ids[1])),
            "selection follows the moved repo"
        );
    }

    #[tokio::test]
    async fn shift_j_moves_selected_repo_down() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.select_index(1); // select repo-1
        press(&mut app, 'J', KeyModifiers::SHIFT).await;

        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(
            order,
            vec![ids[0], ids[2], ids[1]],
            "repo-1 moved below repo-2"
        );
        assert_eq!(app.selected_target(), Some(SelectionTarget::Repo(ids[1])));
    }

    #[tokio::test]
    async fn shift_k_at_top_is_noop() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.select_index(0); // top repo
        press(&mut app, 'K', KeyModifiers::SHIFT).await;
        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(
            order,
            vec![ids[0], ids[1], ids[2]],
            "no movement at the top"
        );
    }

    #[tokio::test]
    async fn shift_j_at_bottom_is_noop() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.select_index(2); // bottom repo
        press(&mut app, 'J', KeyModifiers::SHIFT).await;
        let order: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(
            order,
            vec![ids[0], ids[1], ids[2]],
            "no movement at the bottom"
        );
    }

    #[tokio::test]
    async fn shift_j_repeated_walks_repo_and_selection_follows() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.select_index(0); // select repo-0 (top)
        // Walk it down twice: [0,1,2] -> [1,0,2] -> [1,2,0].
        press(&mut app, 'J', KeyModifiers::SHIFT).await;
        assert_eq!(
            app.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![ids[1], ids[0], ids[2]],
            "after first J: repo-0 moved to middle"
        );
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Repo(ids[0])),
            "selection still on the moved repo after first J"
        );
        press(&mut app, 'J', KeyModifiers::SHIFT).await;
        assert_eq!(
            app.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![ids[1], ids[2], ids[0]],
            "after second J: repo-0 walked to the bottom"
        );
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Repo(ids[0])),
            "selection tracked the repo across both moves"
        );
        // A third J at the bottom is a no-op.
        press(&mut app, 'J', KeyModifiers::SHIFT).await;
        assert_eq!(
            app.repos.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![ids[1], ids[2], ids[0]],
            "third J at bottom does nothing"
        );
    }

    #[tokio::test]
    async fn shift_j_on_workspace_is_noop_for_order() {
        use crate::data::store::{NewWorkspace, WorkspaceState};
        let (mut app, ids) = make_app_with_n_repos(2);
        // Add a workspace to repo-0 so there is a Workspace entry in selectable.
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id: ids[0],
                name: "ws-alpha",
                branch: "repo-0/ws-alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/ws-alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        app.store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();
        app.refresh().unwrap();
        // Find the workspace row in selectable and select it.
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(id) if *id == ws_id))
            .expect("workspace should appear in selectable list");
        app.select_index(idx);
        // Capture repo order before pressing Shift+J.
        let order_before: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        press(&mut app, 'J', KeyModifiers::SHIFT).await;
        let order_after: Vec<_> = app.repos.iter().map(|r| r.id).collect();
        assert_eq!(
            order_before, order_after,
            "Shift+J on a workspace row must not reorder repos"
        );
    }

    #[tokio::test]
    async fn i_alias_opens_new_workspace_modal_like_enter_on_repo() {
        // On a repo header, Enter opens the New Workspace modal. `i` (vim
        // insert) should do the same — it's the "enter this thing" verb.
        let (mut app, _) = make_app_with_n_repos(1);
        app.select_index(0);
        assert!(matches!(
            app.selected_target(),
            Some(SelectionTarget::Repo(_))
        ));
        press(&mut app, 'i', KeyModifiers::NONE).await;
        assert!(
            matches!(app.modal, Some(Modal::NewWorkspace { .. })),
            "i on a repo row should open NewWorkspace like Enter; got {:?}",
            app.modal
        );
    }

    #[tokio::test]
    async fn capital_s_opens_new_workspace_modal_with_shared_true() {
        // Capital S opens the NewWorkspace modal pre-set for a tmux-shared
        // workspace, mirroring how capital N pre-sets yolo mode.
        let (mut app, _) = make_app_with_n_repos(1);
        app.select_index(0);
        press(&mut app, 'S', KeyModifiers::SHIFT).await;
        match app.modal {
            Some(Modal::NewWorkspace { shared, yolo, .. }) => {
                assert!(shared, "S should open the modal with shared: true");
                assert!(!yolo, "S should not also enable yolo");
            }
            other => panic!("expected NewWorkspace modal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn capital_h_opens_host_picker_with_configured_hosts() {
        // Capital H opens a picker over the configured shared hosts,
        // sorted by name (shared_hosts::list already sorts; the picker
        // just snapshots that order). No workspace selection required.
        let (mut app, _) = make_app_with_n_repos(0);
        app.store
            .set_setting("shared_hosts", "mini=eben@mini\nlab=eben@lab")
            .unwrap();
        press(&mut app, 'H', KeyModifiers::SHIFT).await;
        match &app.modal {
            Some(Modal::RemoteHostPicker { hosts, selected }) => {
                assert_eq!(hosts.len(), 2);
                assert_eq!(hosts[0].0, "lab", "expected sorted by name: {hosts:?}");
                assert_eq!(hosts[1].0, "mini");
                assert_eq!(*selected, 0);
            }
            other => panic!("expected host picker, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn capital_h_with_no_hosts_explains_config_edit() {
        let (mut app, _) = make_app_with_n_repos(0);
        press(&mut app, 'H', KeyModifiers::SHIFT).await;
        match &app.modal {
            Some(Modal::Error { message }) => {
                assert!(
                    message.contains("config edit shared_hosts"),
                    "expected hint to name the setting command: {message}"
                );
            }
            other => panic!("expected error modal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn enter_in_host_picker_fetches_and_populates_remote_list() {
        // Full round trip without real ssh: WSX_SSH_BIN points at a fake
        // script that emits a valid one-workspace shared-list JSON array
        // (mirrors shared_hosts::tests::fetch_shared_list_parses_fake_ssh_output_and_surfaces_stderr).
        let mut env = EnvGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake-ssh-ok.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\necho '[{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"agents\":[]}]'\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", script.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        store.set_setting("shared_hosts", "mini=eben@mini").unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            let h = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('H'),
                crossterm::event::KeyModifiers::SHIFT,
            );
            handle_event(&mut g, &app, CtEvent::Key(h)).await.unwrap();
            assert!(
                matches!(g.modal, Some(Modal::RemoteHostPicker { .. })),
                "expected host picker after H, got {:?}",
                g.modal
            );
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            // Immediately after Enter, modal should be RemoteListLoading and
            // a fetch generation should be pending.
            assert!(
                matches!(g.modal, Some(Modal::RemoteListLoading { .. })),
                "expected loading modal immediately after Enter; got {:?}",
                g.modal
            );
            assert!(g.pending_remote_gen.is_some());
        }
        // Wait for the spawned fetch + reconcile to finish.
        wait_until(&app, "remote fetch to finish (list populated)", |g| {
            g.remote_list.is_some() && g.pending_remote_gen.is_none()
        })
        .await;
        let g = app.lock().await;
        let list = g.remote_list.as_ref().expect("remote_list populated");
        assert_eq!(list.host_name, "mini");
        assert_eq!(list.records.len(), 1);
        assert_eq!(list.records[0].workspace, "w");
        assert!(
            matches!(g.modal, Some(Modal::RemoteWorkspaceList { .. })),
            "expected the fetch to open RemoteWorkspaceList; got {:?}",
            g.modal
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attach_remote_spawns_ssh_and_detach_severs_client_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        // Fake ssh: prove argv shape, then stream a heartbeat like a remote attach.
        let log = dir.path().join("ssh-args.log");
        let fake = dir.path().join("fake-ssh.sh");
        std::fs::write(
            &fake,
            format!(
                "#!/bin/sh\necho \"$@\" > {}\nfor i in $(seq 1 60); do echo remote-beat; sleep 1; done\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", fake.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();

        crate::app::attach_remote(
            &mut app,
            crate::app::RemoteTarget {
                host_name: "mini".into(),
                dest: "eben@mini".into(),
                tmux: "wsx-r-w".into(),
            },
            80,
            24,
        )
        .unwrap();
        assert!(matches!(app.view, crate::ui::View::AttachedRemote));
        let session = app.remote.clone().unwrap();
        // beats arrive through the PTY
        let mut seen = false;
        for _ in 0..50 {
            if session
                .parser
                .lock()
                .unwrap()
                .screen()
                .contents()
                .contains("remote-beat")
            {
                seen = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(seen, "expected remote heartbeat through the PTY");
        // argv shape: -t <dest> -- <ONE pre-quoted remote command>. The remote
        // command must be a single ssh argv element (it still contains
        // multiple shell words), routed through `sh -l` like the list fetch:
        // sshd hands the joined string to the user's default shell in
        // non-login mode (`zsh -c`), which reads only ~/.zshenv — on stock
        // macOS, homebrew's
        // tmux isn't on that PATH ("zsh:1: command not found: tmux" on a real
        // host). `sh -l` reads ~/.profile, the one documented PATH
        // requirement shared with the fetch. The tmux =target stays
        // single-quoted (zsh =word expansion; see #226). `-u` forces UTF-8:
        // the ssh/sh -l context has no locale (LC_CTYPE=C on real hosts), and
        // without it tmux downgrades Unicode line-drawing to ACS/ASCII —
        // rendering pane borders as rows of literal q's.
        let args = std::fs::read_to_string(&log).unwrap();
        assert!(
            args.contains("-t eben@mini -- sh -lc \"tmux -u attach -t '=wsx-r-w'\""),
            "remote command must run tmux -u via a login shell with the =target quoted: {args}"
        );
        assert!(
            !args.contains("-t =wsx-r-w"),
            "unquoted =target must not appear (zsh =-expansion hazard): {args}"
        );
        assert!(
            session.tmux_session.is_none(),
            "remote sessions must never own a local tmux backend"
        );

        crate::app::detach_remote(&mut app);
        assert!(app.remote.is_none() && matches!(app.view, crate::ui::View::Dashboard));
        assert!(app.remote_target.is_none());
    }

    /// `target.tmux` arrives from the remote host's JSON and is interpolated
    /// into a shell-parsed string; `attach_remote` must reject anything
    /// outside the sanitized `[A-Za-z0-9_-]` charset at the boundary instead
    /// of trusting the wire (a quote or whitespace would break the remote
    /// quoting and could inject).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attach_remote_rejects_unsanitized_tmux_names() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        for hostile in ["wsx'; touch /tmp/pwned; echo '", "a b", "", "name\n"] {
            let err = crate::app::attach_remote(
                &mut app,
                crate::app::RemoteTarget {
                    host_name: "mini".into(),
                    dest: "eben@mini".into(),
                    tmux: hostile.into(),
                },
                80,
                24,
            )
            .unwrap_err();
            assert!(
                err.to_string().contains("invalid remote tmux session name"),
                "hostile name {hostile:?} must be rejected, got: {err}"
            );
            assert!(app.remote.is_none(), "no session may spawn for {hostile:?}");
            assert!(
                matches!(app.view, crate::ui::View::Dashboard),
                "view must be unchanged for {hostile:?}"
            );
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn key_in_attached_remote_after_ssh_exit_bounces_to_dashboard_with_error() {
        // Fake ssh that exits immediately (e.g. stale tmux session name):
        // pressing any key in AttachedRemote must return to the dashboard
        // and raise an error modal naming the host/session.
        let dir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        let fake = dir.path().join("fake-ssh-exit.sh");
        std::fs::write(&fake, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", fake.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            crate::app::attach_remote(
                &mut g,
                crate::app::RemoteTarget {
                    host_name: "mini".into(),
                    dest: "eben@mini".into(),
                    tmux: "wsx-r-w".into(),
                },
                80,
                24,
            )
            .unwrap();
            assert!(matches!(g.view, crate::ui::View::AttachedRemote));
        }
        // Wait for the child to actually exit so the status flips to Exited.
        wait_until(&app, "ssh child to exit", |g| {
            g.remote.as_ref().is_some_and(|s| {
                matches!(
                    *s.status.read().unwrap(),
                    crate::pty::session::SessionStatus::Exited { .. }
                )
            })
        })
        .await;
        let mut g = app.lock().await;
        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(key)).await.unwrap();
        assert!(matches!(g.view, crate::ui::View::Dashboard));
        assert!(g.remote.is_none() && g.remote_target.is_none());
        match &g.modal {
            Some(Modal::Error { message }) => {
                assert!(
                    message.contains("mini/wsx-r-w"),
                    "error should name host/session: {message}"
                );
            }
            other => panic!("expected error modal, got {other:?}"),
        }
    }

    /// The remote/shared bottom bar must show the host's GLOBAL pinned commands
    /// and the workspace's PR chip (recovered from the retained `remote_list`
    /// record whose agent owns the attached tmux), not just the `^x menu` hint.
    /// Repo-scoped stats the host doesn't ship (procs/diff/model) stay off.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remote_bottom_bar_shows_global_pinned_and_pr_chip() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let dir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        // Fake ssh that streams a heartbeat so the remote session stays live.
        let fake = dir.path().join("fake-ssh.sh");
        std::fs::write(
            &fake,
            "#!/bin/sh\nfor i in $(seq 1 60); do echo beat; sleep 1; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", fake.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        // Global pinned commands are resolved locally and drive the remote agent.
        store
            .set_setting("pinned_commands", "Commit=/commit\nTest=/test")
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();

        // A retained remote list whose live agent owns the tmux we attach to,
        // carrying a PR (open, #142) so the chip is recoverable by tmux match.
        app.remote_list = Some({
            use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
            crate::app::RemoteList {
                host_name: "mini".into(),
                dest: "eben@mini".into(),
                records: vec![SharedWorkspaceRecord {
                    repo: "r".into(),
                    workspace: "w".into(),
                    branch: "b".into(),
                    worktree_path: "/x".into(),
                    agents: vec![SharedAgentRecord {
                        label: "claude".into(),
                        agent: "claude".into(),
                        tmux_session: Some("wsx-r-w".into()),
                        alive: true,
                    }],
                    lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
                    pr_number: Some(142),
                }],
            }
        });

        crate::app::attach_remote(
            &mut app,
            crate::app::RemoteTarget {
                host_name: "mini".into(),
                dest: "eben@mini".into(),
                tmux: "wsx-r-w".into(),
            },
            80,
            24,
        )
        .unwrap();
        assert!(matches!(app.view, crate::ui::View::AttachedRemote));

        let backend = TestBackend::new(120, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| crate::app::render::draw_for_test(f, &mut app))
            .unwrap();
        let text: String = term
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();

        assert!(text.contains("menu"), "the ^x menu hint must still render");
        assert!(
            text.contains("Commit") && text.contains("Test"),
            "global pinned chips must render in the remote bar: {text:?}"
        );
        assert!(
            text.contains("142"),
            "the PR chip (#142) must render, recovered via tmux->record match: {text:?}"
        );
        // The dispatch cache is populated so `^x <digit>` / clicks can fire.
        assert_eq!(app.pinned_commands_cache.len(), 2);
        // No local WorkspaceId backs a remote PR, so its click target stays off.
        assert!(app.pr_link_rect.is_none());

        crate::app::detach_remote(&mut app);
    }

    /// `^x <digit>` in the remote view fires the matching global pinned command
    /// into the ssh PTY, driving the remote agent. The fake ssh `cat`s its stdin
    /// back out, so the dispatched command text lands on the remote screen.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remote_leader_digit_fires_pinned_command_into_ssh() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        let fake = dir.path().join("fake-ssh-cat.sh");
        std::fs::write(&fake, "#!/bin/sh\ncat\n").unwrap();
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", fake.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            // The render pass populates the dispatch cache from settings; seed it
            // directly here since this test drives keys without a render tick.
            g.pinned_commands_cache = crate::commands::pinned::parse("Ship=/ship-it\nTest=/test");
            crate::app::attach_remote(
                &mut g,
                crate::app::RemoteTarget {
                    host_name: "mini".into(),
                    dest: "eben@mini".into(),
                    tmux: "wsx-r-w".into(),
                },
                80,
                24,
            )
            .unwrap();
        }

        // ^x then '1' -> first pinned command (/ship-it) echoes back via `cat`.
        for k in [
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('x'),
                crossterm::event::KeyModifiers::CONTROL,
            ),
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('1'),
                crossterm::event::KeyModifiers::empty(),
            ),
        ] {
            let mut g = app.lock().await;
            handle_event(&mut g, &app, CtEvent::Key(k)).await.unwrap();
        }

        wait_until(&app, "pinned command to echo back through ssh", |g| {
            g.remote.as_ref().is_some_and(|s| {
                s.parser
                    .lock()
                    .unwrap()
                    .screen()
                    .contents()
                    .contains("/ship-it")
            })
        })
        .await;

        let mut g = app.lock().await;
        crate::app::detach_remote(&mut g);
    }

    #[tokio::test]
    async fn esc_during_remote_list_loading_clears_pending_gen_and_stale_fetch_no_ops() {
        // Esc while RemoteListLoading is up must close the modal AND clear
        // pending_remote_gen so the in-flight fetch's reconcile becomes a
        // no-op via its gen guard, instead of reopening a modal (or an
        // error) the user has already backed out of.
        let mut env = EnvGuard::new();
        let dir = tempfile::tempdir().unwrap();
        // Deliberately slow so Esc can land before the fetch completes.
        let script = dir.path().join("fake-ssh-slow.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 2\necho '[]'\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", script.to_str().unwrap());

        let store = Store::open_in_memory().unwrap();
        store.set_setting("shared_hosts", "mini=eben@mini").unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            let h = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('H'),
                crossterm::event::KeyModifiers::SHIFT,
            );
            handle_event(&mut g, &app, CtEvent::Key(h)).await.unwrap();
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            assert!(matches!(g.modal, Some(Modal::RemoteListLoading { .. })));
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
            assert!(
                g.modal.is_none(),
                "Esc should close the loading modal immediately"
            );
            assert!(
                g.pending_remote_gen.is_none(),
                "Esc should clear pending_remote_gen so the late reconcile no-ops"
            );
        }
        // Let the slow fetch finish and reconcile run; it must not resurrect
        // any modal or repopulate remote_list.
        tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
        let g = app.lock().await;
        assert!(
            g.modal.is_none(),
            "stale reconcile must not reopen a modal; got {:?}",
            g.modal
        );
        assert!(
            g.remote_list.is_none(),
            "stale reconcile must not populate remote_list"
        );
    }

    /// One workspace with a live `claude` agent and a dead `codex#2` agent.
    /// After attach-only filtering this flattens to a single row (the live
    /// one), so it doubles as a fixture for "dead rows are hidden".
    fn mixed_liveness_remote_list() -> crate::app::RemoteList {
        use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
        crate::app::RemoteList {
            host_name: "mini".into(),
            dest: "eben@mini".into(),
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                agents: vec![
                    SharedAgentRecord {
                        label: "claude".into(),
                        agent: "claude".into(),
                        tmux_session: Some("wsx-r-w".into()),
                        alive: true,
                    },
                    SharedAgentRecord {
                        label: "codex#2".into(),
                        agent: "codex".into(),
                        tmux_session: None,
                        alive: false,
                    },
                ],
                lifecycle: None,
                pr_number: None,
            }],
        }
    }

    /// One workspace whose only agent has a dead session — nothing attachable.
    fn all_dead_remote_list() -> crate::app::RemoteList {
        use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
        crate::app::RemoteList {
            host_name: "mini".into(),
            dest: "eben@mini".into(),
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: None,
                    alive: false,
                }],
                lifecycle: None,
                pr_number: None,
            }],
        }
    }

    #[tokio::test]
    async fn remote_workspace_list_navigation_bounded_to_live_rows() {
        // The dead `codex#2` agent is filtered out, leaving a single live row,
        // so `j` cannot advance the selection past index 0.
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
        app.remote_list = Some(mixed_liveness_remote_list());
        app.modal = Some(Modal::RemoteWorkspaceList {
            selected: 0,
            notice: None,
        });
        let shared_app = Arc::new(Mutex::new(
            App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
        ));

        let j = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('j'),
            crossterm::event::KeyModifiers::NONE,
        );
        handle_key_modal(&mut app, &shared_app, j).await.unwrap();
        match &app.modal {
            Some(Modal::RemoteWorkspaceList { selected, .. }) => assert_eq!(
                *selected, 0,
                "only one live row exists, so j must not move past it"
            ),
            other => panic!("expected RemoteWorkspaceList, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn remote_workspace_list_enter_with_no_live_rows_notices() {
        // When every shared workspace on the host has a dead session there are
        // no rows at all; Enter can't resolve a target, so the modal stays open
        // with the "no live session" notice rather than attaching.
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
        app.remote_list = Some(all_dead_remote_list());
        app.modal = Some(Modal::RemoteWorkspaceList {
            selected: 0,
            notice: None,
        });
        let shared_app = Arc::new(Mutex::new(
            App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
        ));

        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_key_modal(&mut app, &shared_app, enter)
            .await
            .unwrap();
        match &app.modal {
            Some(Modal::RemoteWorkspaceList { notice, .. }) => {
                assert_eq!(
                    notice.as_deref(),
                    Some("no live session to attach to"),
                    "expected the no-live-session notice"
                );
            }
            other => panic!("expected RemoteWorkspaceList to stay open, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn remote_workspace_list_esc_closes_modal_and_clears_remote_list() {
        let store = Store::open_in_memory().unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
        app.remote_list = Some(mixed_liveness_remote_list());
        app.modal = Some(Modal::RemoteWorkspaceList {
            selected: 0,
            notice: None,
        });
        let shared_app = Arc::new(Mutex::new(
            App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
        ));

        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_key_modal(&mut app, &shared_app, esc).await.unwrap();
        assert!(app.modal.is_none(), "Esc should close the modal");
        assert!(
            app.remote_list.is_none(),
            "Esc should clear app.remote_list (ephemeral contract)"
        );
    }

    #[tokio::test]
    async fn ctrl_s_in_new_workspace_modal_toggles_shared() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
        app.modal = Some(Modal::NewWorkspace {
            repo_id,
            name_buffer: "alpha".to_string(),
            yolo: false,
            shared: false,
            agent: crate::pty::session::AgentKind::Claude,
        });
        let shared_app = Arc::new(Mutex::new(
            App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
        ));
        let ctrl_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
        handle_key_modal(&mut app, &shared_app, ctrl_s)
            .await
            .unwrap();
        match app.modal {
            Some(Modal::NewWorkspace { shared, .. }) => {
                assert!(shared, "Ctrl-s should toggle shared from false to true");
            }
            other => panic!("expected NewWorkspace modal, got {other:?}"),
        }
        // Toggling again flips it back — and plain chars (no Ctrl) still
        // fall through to the name buffer rather than toggling.
        handle_key_modal(&mut app, &shared_app, ctrl_s)
            .await
            .unwrap();
        match &app.modal {
            Some(Modal::NewWorkspace { shared, .. }) => {
                assert!(!shared, "second Ctrl-s should toggle shared back to false");
            }
            other => panic!("expected NewWorkspace modal, got {other:?}"),
        }
        let plain_s = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
        handle_key_modal(&mut app, &shared_app, plain_s)
            .await
            .unwrap();
        match app.modal {
            Some(Modal::NewWorkspace {
                shared,
                name_buffer,
                ..
            }) => {
                assert!(!shared, "plain 's' must not toggle shared");
                assert_eq!(
                    name_buffer, "alphas",
                    "plain 's' should append to the name buffer"
                );
            }
            other => panic!("expected NewWorkspace modal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn h_folds_focused_repo() {
        let (mut app, ids) = make_app_with_n_repos(2);
        app.select_index(0);
        // Start expanded so we can observe the fold.
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'h', KeyModifiers::NONE).await;
        assert_eq!(
            app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
            Some(true),
            "h should fold the focused repo"
        );
    }

    #[tokio::test]
    async fn l_unfolds_focused_repo() {
        let (mut app, ids) = make_app_with_n_repos(2);
        app.select_index(0);
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'l', KeyModifiers::NONE).await;
        assert_eq!(
            app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
            Some(false),
            "l should unfold the focused repo"
        );
    }

    #[tokio::test]
    async fn h_is_idempotent_on_already_folded_repo() {
        // Unlike `zz`, `h` should not toggle — pressing it twice keeps the
        // repo folded. This is the behavior that lets you mash `h` while
        // navigating without accidentally re-opening a row.
        let (mut app, ids) = make_app_with_n_repos(2);
        app.select_index(0);
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'h', KeyModifiers::NONE).await;
        press(&mut app, 'h', KeyModifiers::NONE).await;
        assert_eq!(
            app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
            Some(true),
            "h on an already-folded repo must stay folded"
        );
    }

    #[tokio::test]
    async fn a_alone_is_no_op_on_dashboard() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'a', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "a alone should not arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "a alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn shift_m_alone_is_no_op_on_dashboard() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'M', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "M alone should not arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "M alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn z_m_folds_all_repos_without_shift_modifier() {
        // Some terminals (or CapsLock) report `Char('M')` without
        // KeyModifiers::SHIFT. The chord should still fire — matches
        // the codebase convention for capital-letter binds like `G`.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'M', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zM");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(true),
                "zM (no SHIFT) should fold every repo"
            );
        }
    }

    #[tokio::test]
    async fn zm_folds_all_repos() {
        // Vim `zm` (lowercase m) should fold all repos, same as `zM`.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'm', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zm");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(true),
                "zm should set repo {id:?} to folded (true)"
            );
        }
    }

    #[tokio::test]
    async fn zr_expands_all_repos() {
        // Vim `zr` (lowercase r) should expand all repos, same as `za`.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'r', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zr");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(false),
                "zr should set repo {id:?} to expanded (false)"
            );
        }
    }

    #[tokio::test]
    async fn z_shift_r_expands_all_repos() {
        // Vim `zR` (uppercase R) should also expand all repos.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'R', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "leader should clear after zR");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(false),
                "zR should set repo {id:?} to expanded (false)"
            );
        }
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

    fn init_git_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        r(&["init", "-q", "-b", "main"]);
        r(&["config", "user.email", "t@e"]);
        r(&["config", "user.name", "t"]);
        r(&["commit", "--allow-empty", "-q", "-m", "init"]);
        dir
    }

    /// Poll the shared `app` until `predicate` holds, re-acquiring the lock
    /// on each tick and releasing it between ticks so the background
    /// setup/archive task can make progress.
    ///
    /// This replaces fixed `sleep(…)` waits that assumed an async task
    /// finishes within a hard-coded window. Those assumptions flake on
    /// loaded CI runners (e.g. a `sleep 1` setup script not completing
    /// inside a 1500ms budget), failing identically across unrelated
    /// changes. Polling returns as soon as the condition is met — fast in
    /// the common case — and only spends the full ~10s budget before
    /// declaring a real failure.
    async fn wait_until<F>(
        app: &std::sync::Arc<tokio::sync::Mutex<App>>,
        desc: &str,
        mut predicate: F,
    ) where
        F: FnMut(&App) -> bool,
    {
        for _ in 0..400 {
            if predicate(&app.lock().await as &App) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("timed out after ~10s waiting for: {desc}");
    }

    #[tokio::test]
    async fn enter_in_new_workspace_modal_transitions_to_setup_running_and_spawns_task() {
        use crate::ui::modal::Modal;
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                shared: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
        }
        // Send Enter.
        let evt = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        {
            let mut g = app.lock().await;
            handle_event(&mut g, &app, CtEvent::Key(evt)).await.unwrap();
            // Immediately after Enter, modal should be SetupRunning.
            assert!(
                matches!(g.modal, Some(Modal::SetupRunning { .. })),
                "modal should transition to SetupRunning immediately; got {:?}",
                g.modal
            );
            assert!(g.pending_create_gen.is_some());
        }
        // Wait for the spawned create task to finish: the modal clears, the
        // pending generation is reset, and the workspace materializes.
        wait_until(&app, "create to finish (modal cleared, 1 workspace)", |g| {
            g.modal.is_none() && g.pending_create_gen.is_none() && g.workspaces.len() == 1
        })
        .await;
        let g = app.lock().await;
        assert!(
            g.modal.is_none(),
            "modal should clear after create succeeds; got {:?}",
            g.modal
        );
        assert!(g.pending_create_gen.is_none());
        assert_eq!(g.workspaces.len(), 1);
        let _ = repo_id; // suppress unused warning if not referenced above
    }

    #[tokio::test]
    async fn esc_in_setup_running_cancels_and_closes_modal() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        store
            .set_repo_setup_script(repo_id, Some("sleep 5"))
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        // Open the modal and press Enter.
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                shared: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            assert!(matches!(g.modal, Some(Modal::SetupRunning { .. })));
        }
        // Brief yield so the spawned task gets to start the setup script.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Press Esc.
        {
            let mut g = app.lock().await;
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
            assert!(g.modal.is_none(), "modal should close immediately on Esc");
            assert!(g.pending_create_gen.is_none());
        }
        // Wait for the spawned task to wind down and record the cancellation.
        wait_until(&app, "setup task to record Cancelled status", |g| {
            g.workspaces.len() == 1
                && g.workspaces[0].1.setup_status == crate::data::store::SetupStatus::Cancelled
        })
        .await;
        let g = app.lock().await;
        assert_eq!(g.workspaces.len(), 1);
        assert_eq!(
            g.workspaces[0].1.setup_status,
            crate::data::store::SetupStatus::Cancelled
        );
        // Modal should still be None — the late reconcile must not pop an error.
        assert!(g.modal.is_none());
    }

    #[tokio::test]
    async fn y_in_confirm_archive_transitions_to_archive_running_and_spawns_task() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        // Create the workspace BEFORE wrapping the store in the App, since
        // App::new takes the store by value.
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let created = crate::data::workspace::create(
            &store,
            &repo,
            Some("doomed"),
            tmp.path(),
            false,
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let ws_id = created.workspace.id;
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        // Open the ConfirmArchive modal.
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::ConfirmArchive {
                workspace_id: ws_id,
                name: created.workspace.name.clone(),
            });
        }
        // Send 'y'.
        let evt = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('y'),
            crossterm::event::KeyModifiers::empty(),
        );
        {
            let mut g = app.lock().await;
            handle_event(&mut g, &app, CtEvent::Key(evt)).await.unwrap();
            // Immediately after 'y', modal should be ArchiveRunning.
            match &g.modal {
                Some(Modal::ArchiveRunning {
                    step,
                    script_present,
                }) => {
                    assert_eq!(
                        *step,
                        crate::ui::modal::ArchiveStep::Script,
                        "initial step should be Script"
                    );
                    // The fixture repo at this test site has no
                    // archive script configured, so script_present
                    // must be false.
                    assert!(
                        !*script_present,
                        "fixture has no archive script; script_present should be false"
                    );
                }
                other => {
                    panic!("modal should transition to ArchiveRunning immediately; got {other:?}")
                }
            }
            assert!(g.pending_archive_gen.is_some());
        }
        // Wait for the spawned archive task to complete: the modal clears,
        // the pending generation resets, and the workspace is removed.
        wait_until(
            &app,
            "archive to finish (modal cleared, workspace gone)",
            |g| {
                g.modal.is_none()
                    && g.pending_archive_gen.is_none()
                    && g.workspaces.iter().all(|(_, w)| w.id != ws_id)
            },
        )
        .await;
        let g = app.lock().await;
        assert!(
            g.modal.is_none(),
            "modal should clear after archive succeeds; got {:?}",
            g.modal
        );
        assert!(g.pending_archive_gen.is_none());
        assert!(
            g.workspaces.iter().all(|(_, w)| w.id != ws_id),
            "archived workspace should be removed from app.workspaces"
        );
    }

    #[tokio::test]
    async fn esc_in_archive_running_is_noop() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // Give the archive a slow archive-script so it's still running
        // when we press Esc.
        store
            .set_repo_archive_script(repo_id, Some("sleep 1"))
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        // Create the workspace before moving the store into the App.
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let created = crate::data::workspace::create(
            &store,
            &repo,
            Some("doomed"),
            tmp.path(),
            false,
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let ws_id = created.workspace.id;
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::ConfirmArchive {
                workspace_id: ws_id,
                name: created.workspace.name.clone(),
            });
        }
        // Press 'y' to start archiving.
        {
            let mut g = app.lock().await;
            let y = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('y'),
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(y)).await.unwrap();
            assert!(matches!(g.modal, Some(Modal::ArchiveRunning { .. })));
        }
        // Yield briefly so the archive script kicks off but is still
        // running (sleep 1 gives us a 1s window).
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Press Esc — should be a no-op.
        {
            let mut g = app.lock().await;
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
            assert!(
                matches!(g.modal, Some(Modal::ArchiveRunning { .. })),
                "Esc must not close ArchiveRunning; got {:?}",
                g.modal
            );
            assert!(g.pending_archive_gen.is_some());
        }
        // Wait for the archive to actually finish.
        wait_until(
            &app,
            "archive to finish (modal cleared, workspace gone)",
            |g| g.modal.is_none() && g.workspaces.iter().all(|(_, w)| w.id != ws_id),
        )
        .await;
        let g = app.lock().await;
        assert!(
            g.modal.is_none(),
            "modal should clear once archive finishes"
        );
        assert!(
            g.workspaces.iter().all(|(_, w)| w.id != ws_id),
            "workspace should be archived"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn capital_t_opens_confirm_share_and_y_flips_shared_and_restarts_session() {
        // T on a selected workspace opens ConfirmShare proposing the flip of
        // the current `shared` flag; `y` commits it via
        // `toggle_workspace_shared`, which restarts any running session so
        // it respawns per the new flag (resuming via --continue).
        //
        // This exercises the *unshare* direction (shared: true -> false):
        // the respawn after unsharing is a plain direct spawn (no tmux
        // binary required), unlike the share direction, whose tmux-backed
        // respawn is covered by the tmux-gated e2e in Task 10.
        use crate::data::store::NewWorkspace;
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;

        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());

        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let repo_id = app
            .store
            .add_repo(std::path::Path::new("."), "scratch", "test")
            .unwrap();
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "share-toggle-test",
                branch: "main",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: crate::pty::session::AgentKind::Codex,
                shared: true,
            })
            .unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let inst = test_primary_instance(&app, ws_id);
        app.sessions
            .spawn(
                inst,
                ws_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Codex,
                None,
            )
            .unwrap();
        app.refresh().unwrap();
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);

        let old_session = app.sessions.get(inst).expect("session should be running");

        let shared_app = Arc::new(Mutex::new(app));

        // Press Shift+T: should open ConfirmShare proposing to_shared: false
        // (workspace starts shared: true).
        {
            let mut g = shared_app.lock().await;
            let t = KeyEvent::new(KeyCode::Char('T'), KeyModifiers::SHIFT);
            handle_event(&mut g, &shared_app, CtEvent::Key(t))
                .await
                .unwrap();
            match &g.modal {
                Some(Modal::ConfirmShare {
                    workspace_id,
                    to_shared,
                    ..
                }) => {
                    assert_eq!(*workspace_id, ws_id);
                    assert!(
                        !*to_shared,
                        "workspace starts shared; T should propose to_shared: false"
                    );
                }
                other => panic!("expected ConfirmShare modal, got {other:?}"),
            }
        }

        // Press 'y': flips the store flag and restarts the running instance.
        {
            let mut g = shared_app.lock().await;
            let y = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
            handle_event(&mut g, &shared_app, CtEvent::Key(y))
                .await
                .unwrap();
        }

        let g = shared_app.lock().await;
        assert!(
            g.modal.is_none(),
            "y should dismiss ConfirmShare on success; got {:?}",
            g.modal
        );
        let ws = g.store.workspace_by_id(ws_id).unwrap().unwrap();
        assert!(!ws.shared, "y should flip store workspace.shared to false");
        let new_session = g
            .sessions
            .get(inst)
            .expect("instance should have a respawned session");
        assert!(
            !Arc::ptr_eq(&old_session, &new_session),
            "the old session must be gone from app.sessions, replaced by a respawned one"
        );
    }

    #[tokio::test]
    async fn enter_during_setup_running_is_a_noop() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        store
            .set_repo_setup_script(repo_id, Some("sleep 1"))
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                shared: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            // Press Enter again — should not spawn a second create.
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
        }
        // Wait for the (single) setup to finish. Repeated Enter presses while
        // a create is pending are rejected synchronously, so the count
        // settles at exactly one rather than racing toward duplicates.
        wait_until(&app, "exactly one workspace to be created", |g| {
            g.workspaces.len() == 1
        })
        .await;
        let g = app.lock().await;
        assert_eq!(
            g.workspaces.len(),
            1,
            "exactly one workspace should be created"
        );
    }

    #[tokio::test]
    async fn successful_create_after_esc_does_not_show_error_modal() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // No setup script — create is very fast.
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                shared: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            // Immediately Esc — race against the spawned create completing.
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let g = app.lock().await;
        // Regardless of which side won the race, modal must not be Error.
        assert!(
            !matches!(g.modal, Some(Modal::Error { .. })),
            "Esc race should never produce an error modal, got {:?}",
            g.modal
        );
    }

    #[tokio::test]
    async fn create_in_folded_repo_unfolds_and_keeps_new_workspace_selected() {
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        // Create the workspace up front, since App::new consumes the store.
        let created = crate::data::workspace::create(
            &store,
            &repo,
            Some("feature"),
            tmp.path(),
            false,
            false,
            crate::pty::session::AgentKind::Claude,
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        let new_id = created.workspace.id;
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
        // The owning repo is collapsed in the dashboard — the scenario where a
        // freshly-created workspace would otherwise land on a hidden row and
        // get parked (no highlight, cursor adrift).
        app.dashboard.folded.insert(repo_id.0 as u64, true);
        let my_gen = app.alloc_create_gen();
        let shared = Arc::new(Mutex::new(app));
        crate::app::reconcile_create_result(shared.clone(), my_gen, Ok(created)).await;

        let mut g = shared.lock().await;
        let app: &mut App = &mut g;
        assert_eq!(
            app.dashboard.folded.get(&(repo_id.0 as u64)).copied(),
            Some(false),
            "creating a workspace in a folded repo must unfold it"
        );
        assert_eq!(
            app.dashboard.selection,
            Some(SelectionTarget::Workspace(new_id)),
            "selection should move to the newly created workspace"
        );
        // Draw a frame: this rebuilds `selectable` via `visible_targets`,
        // which hides workspaces inside folded repos. With the repo unfolded,
        // the new workspace must be a visible target and remain the live
        // selection rather than parking on an invisible row.
        let backend = TestBackend::new(120, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, app)).unwrap();
        assert!(
            app.selectable.contains(&SelectionTarget::Workspace(new_id)),
            "new workspace should be a visible selection target after draw"
        );
        assert_eq!(
            app.dashboard.selection,
            Some(SelectionTarget::Workspace(new_id)),
            "selection should stay on the new workspace after a draw, not park"
        );
    }

    fn seed_app_with_workspace() -> App {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Idle repos fold by default; force-expand so the workspace row is
        // visible in `visible_targets` during draw.
        app.dashboard.folded.insert(repo_id.0 as u64, false);
        app
    }

    #[test]
    fn detail_bar_renders_when_workspace_is_selected() {
        let mut app = seed_app_with_workspace();
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(_)))
            .expect("workspace target present");
        app.select_index(idx);

        let backend = TestBackend::new(120, 30);
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
            rendered.contains("Reply to agent"),
            "bar visible: {rendered}"
        );
    }

    #[test]
    fn detail_bar_absent_when_repo_header_is_selected() {
        let mut app = seed_app_with_workspace();
        let repo_idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Repo(_)))
            .expect("repo target present");
        app.select_index(repo_idx);

        let backend = TestBackend::new(120, 30);
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
            !rendered.contains("Reply to agent"),
            "bar absent on repo header: {rendered}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ensure_workspace_session_sets_modal_when_binary_missing() {
        use crate::data::store::{NewWorkspace, WorkspaceState};
        use crate::pty::session::AgentKind;
        let mut env = EnvGuard::new();
        env.set("WSX_HERMES_BIN", "/nonexistent/wsx-test-hermes");
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/ws"),
                yolo: false,
                agent: AgentKind::Hermes,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let outcome = crate::app::ensure_workspace_session(&mut app, id).unwrap();
        assert!(matches!(outcome, crate::app::AttachReady::AgentMissing));
        match app.modal {
            Some(crate::ui::modal::Modal::AgentMissing {
                ws_id,
                agent,
                ref binary,
            }) => {
                assert_eq!(ws_id, id);
                assert_eq!(agent, AgentKind::Hermes);
                assert_eq!(binary, "/nonexistent/wsx-test-hermes");
            }
            ref other => panic!("expected AgentMissing modal, got {other:?}"),
        }
    }

    #[test]
    fn agent_missing_modal_renders_binary_name() {
        use crate::pty::session::AgentKind;
        use crate::ui::modal::Modal;
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(Modal::AgentMissing {
            ws_id: crate::data::store::WorkspaceId(1),
            agent: AgentKind::Hermes,
            binary: "/nonexistent/hermes".to_string(),
        });
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
            rendered.contains("Hermes is not installed")
                || rendered.contains("hermes is not installed"),
            "expected 'Hermes is not installed' line:\n{rendered}"
        );
        assert!(
            rendered.contains("/nonexistent/hermes"),
            "expected binary path in modal body:\n{rendered}"
        );
        assert!(
            rendered.contains('s') && rendered.contains("switch agent"),
            "expected switch-agent hint:\n{rendered}"
        );
    }

    #[test]
    fn agent_picker_modal_renders_four_agents_with_current_marker() {
        use crate::pty::session::AgentKind;
        use crate::ui::modal::Modal;
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(Modal::AgentPicker {
            ws_id: crate::data::store::WorkspaceId(1),
            selected: 0,
            current: AgentKind::Hermes,
        });
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
            rendered.contains("claude"),
            "expected claude row: {rendered}"
        );
        assert!(rendered.contains("pi"), "expected pi row: {rendered}");
        assert!(
            rendered.contains("hermes"),
            "expected hermes row: {rendered}"
        );
        assert!(rendered.contains("codex"), "expected codex row: {rendered}");
        assert!(
            rendered.contains("current"),
            "expected current marker: {rendered}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn agent_missing_modal_esc_dismisses() {
        use crate::pty::session::AgentKind;
        use crate::ui::modal::Modal;
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(Modal::AgentMissing {
            ws_id: crate::data::store::WorkspaceId(1),
            agent: AgentKind::Hermes,
            binary: "hermes".to_string(),
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
        assert!(app.modal.is_none(), "Esc should dismiss AgentMissing");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn agent_missing_modal_s_opens_picker_with_current_preselected() {
        use crate::pty::session::AgentKind;
        use crate::ui::modal::Modal;
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = crate::data::store::WorkspaceId(42);
        app.modal = Some(Modal::AgentMissing {
            ws_id,
            agent: AgentKind::Hermes,
            binary: "hermes".to_string(),
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
            KeyEvent::new(crossterm::event::KeyCode::Char('s'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(Modal::AgentPicker {
                ws_id: picker_ws,
                selected,
                current,
            }) => {
                assert_eq!(picker_ws, ws_id);
                assert_eq!(current, AgentKind::Hermes);
                assert_eq!(AgentKind::ALL[selected], AgentKind::Hermes);
            }
            ref other => panic!("expected AgentPicker, got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn agent_picker_down_advances_and_clamps() {
        use crate::pty::session::AgentKind;
        use crate::ui::modal::Modal;
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(Modal::AgentPicker {
            ws_id: crate::data::store::WorkspaceId(1),
            selected: 0,
            current: AgentKind::Claude,
        });
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));

        for expected in [1usize, 2, 3, 3 /* clamps at last index */] {
            handle_key_modal(
                &mut app,
                &shared,
                KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
            )
            .await
            .unwrap();
            match app.modal {
                Some(Modal::AgentPicker { selected, .. }) => {
                    assert_eq!(selected, expected, "Down step");
                }
                ref other => panic!("expected AgentPicker, got {other:?}"),
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn agent_picker_enter_persists_and_retries_attach() {
        use crate::data::store::{NewWorkspace, WorkspaceState};
        use crate::pty::session::AgentKind;
        use crate::test_support::{EnvGuard, cat_path};
        use crate::ui::modal::Modal;
        // Switch from broken Hermes (won't spawn) to Claude (substituted with `cat`,
        // which spawns fine), so the retry attach succeeds.
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: AgentKind::Hermes,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let claude_idx = AgentKind::ALL
            .iter()
            .position(|k| *k == AgentKind::Claude)
            .unwrap();
        app.modal = Some(Modal::AgentPicker {
            ws_id: id,
            selected: claude_idx,
            current: AgentKind::Hermes,
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

        // Store now reports Claude.
        let stored = app
            .store
            .workspaces(repo_id)
            .unwrap()
            .into_iter()
            .find(|w| w.id == id)
            .expect("workspace present");
        assert_eq!(stored.agent, AgentKind::Claude);
        // In-memory mirror also updated.
        let mem = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == id)
            .expect("workspace in memory")
            .1
            .clone();
        assert_eq!(mem.agent, AgentKind::Claude);
        // A session exists.
        assert!(
            app.sessions.get(test_primary_instance(&app, id)).is_some(),
            "session should be alive"
        );
        // Modal closed.
        assert!(app.modal.is_none(), "modal should be cleared on success");
    }
}

#[cfg(test)]
mod detail_scroll {
    use super::*;
    use crate::data::store::Store;
    use crate::ui::View;
    use crate::ui::split::AttachedState;
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    use std::path::PathBuf;

    fn mouse_at(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_over_container_scrolls_that_container() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.view = View::Dashboard;
        app.detail_container_rects = [
            Some(Rect {
                x: 0,
                y: 20,
                width: 20,
                height: 5,
            }),
            Some(Rect {
                x: 21,
                y: 20,
                width: 20,
                height: 5,
            }),
            None,
            None,
        ];
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 25, 22)).await;
        assert_eq!(app.detail_scroll_offsets[1], 3);
        assert_eq!(app.detail_scroll_offsets[0], 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_outside_containers_does_not_touch_detail_offsets() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.view = View::Dashboard;
        app.detail_container_rects = [
            Some(Rect {
                x: 0,
                y: 20,
                width: 20,
                height: 5,
            }),
            None,
            None,
            None,
        ];
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 50, 5)).await;
        assert_eq!(app.detail_scroll_offsets, [0; 4]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_in_attached_view_does_not_touch_detail_offsets() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Construct an Attached view with a synthetic workspace id. No live
        // session is needed — `scroll_active` no-ops when the focused id has
        // no session, which is the behavior we want to verify (detail
        // offsets stay untouched regardless).
        app.view = View::Attached(AttachedState::single(crate::ui::split::AttachTarget {
            workspace_id: crate::data::store::WorkspaceId(1),
            instance: crate::data::store::AgentInstanceId(1),
        }));
        app.detail_container_rects = [
            Some(Rect {
                x: 0,
                y: 20,
                width: 20,
                height: 5,
            }),
            None,
            None,
            None,
        ];
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollDown, 5, 22)).await;
        assert_eq!(app.detail_scroll_offsets, [0; 4]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_up_scrolls_back_with_saturating_sub() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.view = View::Dashboard;
        app.detail_container_rects = [
            Some(Rect {
                x: 0,
                y: 20,
                width: 20,
                height: 5,
            }),
            None,
            None,
            None,
        ];
        app.detail_scroll_offsets[0] = 2;
        handle_mouse(&mut app, mouse_at(MouseEventKind::ScrollUp, 5, 22)).await;
        assert_eq!(app.detail_scroll_offsets[0], 0);
    }
}

#[cfg(test)]
mod ctrl_x_shift_d_tests {
    use super::*;
    use crate::test_support::{EnvGuard, cat_path};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_shift_d_saves_layout_and_returns_to_dashboard() {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        use crate::ui::split::{AttachedState, SplitDirection};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let first_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "first",
                branch: "repo/first",
                worktree_path: std::path::Path::new("/tmp/wsx-esc-1"),
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
                worktree_path: std::path::Path::new("/tmp/wsx-esc-2"),
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
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_9 = test_primary_instance(&app, first_id);
        app.sessions
            .spawn(
                __inst_9,
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
        let __inst_10 = test_primary_instance(&app, second_id);
        app.sessions
            .spawn(
                __inst_10,
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
        let second_target = test_target(&app, second_id);
        let mut state = AttachedState::single(first_target);
        state.split(SplitDirection::Vertical, second_target);
        app.view = crate::ui::View::Attached(state);

        // Send Ctrl-x then Shift-D.
        handle_key_attached(
            &mut app,
            first_target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            first_target,
            KeyEvent::new(KeyCode::Char('D'), KeyModifiers::SHIFT),
        )
        .await
        .unwrap();

        assert!(
            matches!(app.view, crate::ui::View::Dashboard),
            "should return to dashboard"
        );
        let saved = app.store.get_workspace_layout(first_id).unwrap();
        assert!(saved.is_some(), "layout should be saved under first leaf");
        let (tree, _focus) = saved.unwrap();
        let leaf_ws: Vec<_> = tree.leaves().iter().map(|t| t.workspace_id).collect();
        assert_eq!(leaf_ws, vec![first_id, second_id]);
        assert!(
            app.workspaces_with_multi_pane_layouts.contains(&first_id),
            "cache should refresh to include the new layout's anchor"
        );
    }
}

#[cfg(test)]
mod restore_layout_tests {
    use super::*;
    use crate::test_support::{EnvGuard, cat_path};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn setup_two_workspaces_with_sessions(
        slug: &str,
    ) -> (
        App,
        crate::data::store::WorkspaceId,
        crate::data::store::WorkspaceId,
    ) {
        use crate::data::store::{NewWorkspace, Store, WorkspaceState};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let first_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "first",
                branch: "repo/first",
                worktree_path: std::path::Path::new(&format!("/tmp/wsx-{slug}-1")),
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
                worktree_path: std::path::Path::new(&format!("/tmp/wsx-{slug}-2")),
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
        for id in [first_id, second_id] {
            let mode = crate::pty::session::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let __inst_11 = test_primary_instance(&app, id);
            app.sessions
                .spawn(
                    __inst_11,
                    id,
                    std::path::Path::new("."),
                    80,
                    24,
                    mode,
                    crate::agent::remote_control::RemoteOpts::disabled(),
                    crate::pty::session::AgentKind::Claude,
                    None,
                )
                .unwrap();
        }
        (app, first_id, second_id)
    }

    fn select_workspace_in_app(app: &mut App, id: crate::data::store::WorkspaceId) {
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(w) if *w == id))
            .expect("workspace in selectable list");
        app.select_index(idx);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_enter_restores_saved_layout() {
        use crate::ui::split::{SplitDirection, SplitTree};
        let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("restore");
        let mut tree = SplitTree::Leaf(test_target(&app, first_id));
        tree.split(&[], SplitDirection::Vertical, test_target(&app, second_id));
        app.store
            .set_workspace_layout(first_id, &tree, &[1])
            .unwrap();
        app.refresh().unwrap();
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
                assert_eq!(leaf_ws, vec![first_id, second_id]);
                assert_eq!(s.focus, vec![1]);
            }
            _ => panic!("expected attached view with restored layout"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_enter_falls_back_to_single_pane_when_no_layout() {
        let (mut app, first_id, _second_id) = setup_two_workspaces_with_sessions("fallback");
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
                assert_eq!(leaf_ws, vec![first_id]);
            }
            _ => panic!("expected single-pane attached view"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn l_key_opens_workspace_like_enter() {
        let (mut app, first_id, _second_id) = setup_two_workspaces_with_sessions("l-key");
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
                assert_eq!(leaf_ws, vec![first_id]);
            }
            _ => panic!("expected single-pane attached view after 'l' on workspace"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restore_prunes_archived_side_panes() {
        use crate::ui::split::{SplitDirection, SplitTree};
        let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("prune");
        let mut tree = SplitTree::Leaf(test_target(&app, first_id));
        tree.split(&[], SplitDirection::Vertical, test_target(&app, second_id));
        app.store
            .set_workspace_layout(first_id, &tree, &[1])
            .unwrap();
        // Archive second_id directly and refresh so app.workspaces drops it.
        app.store.delete_workspace(second_id).unwrap();
        app.refresh().unwrap();
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                let leaf_ws: Vec<_> = s.leaves().iter().map(|t| t.workspace_id).collect();
                assert_eq!(leaf_ws, vec![first_id], "side pane pruned");
            }
            _ => panic!("expected attached view with pruned layout"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_d_does_not_modify_saved_layout() {
        use crate::ui::split::{AttachedState, SplitDirection, SplitTree};
        let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("ctrlxd");
        let first_target = test_target(&app, first_id);
        let second_target = test_target(&app, second_id);
        let mut tree = SplitTree::Leaf(first_target);
        tree.split(&[], SplitDirection::Vertical, second_target);
        app.store
            .set_workspace_layout(first_id, &tree, &[1])
            .unwrap();
        let mut state = AttachedState::single(first_target);
        state.split(SplitDirection::Vertical, second_target);
        app.view = crate::ui::View::Attached(state);
        // Close second pane with Ctrl-x d (focus is on second_id from the split).
        handle_key_attached(
            &mut app,
            second_target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            second_target,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        // Close last pane → dashboard.
        handle_key_attached(
            &mut app,
            first_target,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            first_target,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.view, crate::ui::View::Dashboard));
        // The stored layout is unchanged.
        let (saved, _) = app.store.get_workspace_layout(first_id).unwrap().unwrap();
        let leaf_ws: Vec<_> = saved.leaves().iter().map(|t| t.workspace_id).collect();
        assert_eq!(leaf_ws, vec![first_id, second_id]);
    }
}

#[cfg(test)]
mod detail_bar_focus_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn make_app_with_workspace_selected() -> App {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Force-expand the repo so the workspace stays in `selectable`
        // (idle repos default-fold).
        app.dashboard.folded.insert(repo_id.0 as u64, false);
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(_)))
            .unwrap();
        app.select_index(idx);
        app
    }

    #[tokio::test]
    async fn tab_on_workspace_moves_focus_to_detail_bar_reply() {
        let mut app = make_app_with_workspace_selected();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
    }

    #[tokio::test]
    async fn tab_in_detail_bar_returns_focus_to_dashboard() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test]
    async fn esc_in_detail_bar_clears_draft_and_returns_to_dashboard() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "half-typed message".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "");
    }

    #[tokio::test]
    async fn char_in_detail_bar_appends_to_draft() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert_eq!(app.dashboard.reply_draft, "hi");
        // Focus must NOT have changed (this is a regression guard
        // against accidentally letting dashboard hotkeys fire).
        assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
    }

    #[tokio::test]
    async fn backspace_in_detail_bar_pops_last_char() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "abc".to_string();
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert_eq!(app.dashboard.reply_draft, "ab");
    }

    #[tokio::test]
    async fn arrow_down_while_focused_returns_to_dashboard_and_clears_draft() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "draft".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "");
    }

    // Issue 2: Tab cycle should include PM when visible.
    #[tokio::test]
    async fn tab_in_detail_bar_routes_to_pm_when_visible() {
        let mut app = make_app_with_workspace_selected();
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
    }

    // Issue 3: Arrow navigation in Dashboard focus must clear the reply draft
    // so it cannot be sent to the wrong workspace.
    #[tokio::test]
    async fn arrow_down_in_dashboard_focus_clears_reply_draft() {
        let mut app = make_app_with_workspace_selected();
        // Compose a draft in DetailBarReply, then Tab back to Dashboard.
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "hi");

        // Now arrow-navigate. The draft should be discarded so it can't
        // be sent to the wrong workspace.
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(
            app.dashboard.reply_draft, "",
            "draft must clear on navigation"
        );
    }

    /// Ctrl-X + digit fires a pinned chip even when focus is on
    /// DetailBarReply. The draft must be preserved across Ctrl-X (the leader
    /// arm) and cleared once the chip fires.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_digit_works_while_reply_focused() {
        use crate::data::store::NewWorkspace;
        use crate::test_support::{EnvGuard, cat_path};

        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());

        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();

        // Spawn a workspace with a live PTY session (uses `cat` as the binary
        // so any bytes we write are echoed back to the screen).
        let repo_id = app
            .store
            .add_repo(std::path::Path::new("."), "scratch", "test")
            .unwrap();
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "reply-chord-test",
                branch: "main",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_12 = test_primary_instance(&app, ws_id);
        app.sessions
            .spawn(
                __inst_12,
                ws_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
                None,
            )
            .unwrap();

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.select_index(0);
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "half-typed message".to_string();

        app.pinned_commands_cache = vec![crate::commands::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        // Drive Ctrl-X through the real dispatcher (handle_key_dashboard),
        // which first gives handle_detail_bar_reply_key a crack at it
        // because focus == DetailBarReply.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();

        // Leader must be armed.
        assert!(
            app.leader_pending,
            "Ctrl-X while reply is focused must arm leader_pending"
        );
        // Draft must be intact — Ctrl-X must NOT insert '^X'.
        assert_eq!(
            app.dashboard.reply_draft, "half-typed message",
            "Ctrl-X must not mutate the reply draft"
        );

        // Drive '1' through the same dispatcher.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        )
        .await
        .unwrap();

        // After chip fires: draft echoes the dispatched command (cleared
        // by the tick handler when reply_draft_clear_at_ms expires);
        // focus back to Dashboard.
        assert_eq!(
            app.dashboard.reply_draft, "/pull-request",
            "firing a chip must echo the command into the reply draft"
        );
        assert!(
            app.dashboard.reply_draft_clear_at_ms.is_some(),
            "firing a chip must set the reply_draft auto-clear deadline"
        );
        assert!(
            matches!(app.focus, crate::ui::PaneFocus::Dashboard),
            "firing a chip must return focus to Dashboard"
        );

        // Wait for the cat PTY to echo the command back.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "Ctrl-X+1 while reply focused must dispatch /pull-request to PTY; got: {screen_text:?}"
        );
    }
}

#[cfg(test)]
mod leader_view_transition_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    /// Armed Ctrl-X leader must be cleared when the attached view bounces back
    /// to Dashboard because the session is gone.  Before the fix, leader_pending
    /// would survive the transition and fire against whatever workspace happened
    /// to be selected on the dashboard next.
    #[tokio::test]
    async fn leader_cleared_when_attached_bounces_to_dashboard_on_missing_session() {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();

        // Place the app in Attached view — but do NOT spawn a session, so
        // handle_key_attached will immediately bounce back to Dashboard.
        let target = test_target(&app, ws_id);
        app.view = crate::ui::View::Attached(crate::ui::split::AttachedState::single(target));
        // Arm the leader as if the user had pressed Ctrl-X while attached.
        app.leader_pending = true;

        // Drive any key through handle_key_attached.  With no live session
        // it will assign app.view = View::Dashboard and return.
        handle_key_attached(
            &mut app,
            target,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        )
        .await
        .unwrap();

        assert!(
            matches!(app.view, crate::ui::View::Dashboard),
            "view must transition to Dashboard"
        );
        assert!(
            !app.leader_pending,
            "leader_pending must be cleared on view transition (was still true after bounce)"
        );
    }

    /// Armed Ctrl-X leader must be cleared when the attached-PM view bounces
    /// back to Dashboard because the PM session is gone.
    #[tokio::test]
    async fn leader_cleared_when_attached_pm_bounces_to_dashboard_on_missing_session() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();

        // Place the app in AttachedPm — but do NOT set app.pm, so
        // handle_key_attached_pm will immediately bounce back to Dashboard.
        app.view = crate::ui::View::AttachedPm;
        app.pm = None;
        // Arm the leader as if the user had pressed Ctrl-X while in AttachedPm.
        app.leader_pending = true;

        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        )
        .await
        .unwrap();

        assert!(
            matches!(app.view, crate::ui::View::Dashboard),
            "view must transition to Dashboard"
        );
        assert!(
            !app.leader_pending,
            "leader_pending must be cleared on view transition (was still true after PM bounce)"
        );
    }
}

#[cfg(test)]
mod attached_wheel_forwarding {
    use super::*;
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::layout::Rect;
    use std::path::PathBuf;
    use std::sync::atomic::Ordering;

    fn mouse_at_mod(kind: MouseEventKind, col: u16, row: u16, mods: KeyModifiers) -> MouseEvent {
        MouseEvent {
            kind,
            column: col,
            row,
            modifiers: mods,
        }
    }

    fn spawn_attached_workspace(app: &mut App) -> crate::data::store::WorkspaceId {
        use crate::data::store::NewWorkspace;
        use crate::test_support::{EnvGuard, cat_path};
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let repo_id = app
            .store
            .add_repo(std::path::Path::new("."), "scratch", "test")
            .unwrap();
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "wheel-fwd-test",
                branch: "main",
                worktree_path: std::path::Path::new("."),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let __inst_13 = test_primary_instance(app, ws_id);
        app.sessions
            .spawn(
                __inst_13,
                ws_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
                None,
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(test_target(app, ws_id)));
        ws_id
    }

    // Enable SGR mouse reporting on the session's parser and register a
    // full-screen pane rect so the cursor at (10,10) is "over" the pane.
    fn arm_mouse_mode_and_pane(app: &mut App, ws_id: crate::data::store::WorkspaceId) {
        let session = app.sessions.get(test_primary_instance(app, ws_id)).unwrap();
        {
            let mut p = session.parser.lock().unwrap();
            p.process(b"\x1b[?1000h\x1b[?1006h");
        }
        app.attached_pane_rects = vec![(
            session,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
        )];
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_wheel_forwards_when_mouse_mode_on() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        arm_mouse_mode_and_pane(&mut app, ws_id);
        handle_mouse(
            &mut app,
            mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::NONE),
        )
        .await;
        // Forwarded to the agent -> wsx scrollback must NOT move.
        assert_eq!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .scrollback_offset
                .load(Ordering::Relaxed),
            0,
            "plain wheel over a mouse-aware pane is forwarded, not scrolled locally"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shift_wheel_is_escape_hatch_to_scrollback() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        arm_mouse_mode_and_pane(&mut app, ws_id);
        handle_mouse(
            &mut app,
            mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::SHIFT),
        )
        .await;
        // Shift bypasses the agent -> wsx scrollback moves.
        assert_eq!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .scrollback_offset
                .load(Ordering::Relaxed),
            3,
            "shift+wheel drives wsx scrollback even when the agent has mouse mode on"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_wheel_scrolls_when_mouse_mode_off() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        // Register the pane rect but do NOT enable mouse mode.
        let session = app
            .sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap();
        app.attached_pane_rects = vec![(
            session,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
        )];
        handle_mouse(
            &mut app,
            mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .scrollback_offset
                .load(Ordering::Relaxed),
            3,
            "without agent mouse mode, plain wheel falls through to wsx scrollback"
        );
    }

    fn spawn_pm_for_test_local(app: &mut App) {
        let mut env = crate::test_support::EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", crate::test_support::cat_path());
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let s = app
            .sessions
            .spawn_pm(
                &PathBuf::from("."),
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
            .unwrap();
        app.pm = Some(s);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_wheel_down_forwards_when_mouse_mode_on() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        arm_mouse_mode_and_pane(&mut app, ws_id);
        // Pre-scroll so a fall-through ScrollDown would drop 5 -> 2; forwarding
        // leaves it at 5.
        app.sessions
            .get(test_primary_instance(&app, ws_id))
            .unwrap()
            .scroll_up(5);
        handle_mouse(
            &mut app,
            mouse_at_mod(MouseEventKind::ScrollDown, 10, 10, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .scrollback_offset
                .load(Ordering::Relaxed),
            5,
            "scrolldown over a mouse-aware pane is forwarded, leaving wsx scrollback untouched"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_wheel_over_chrome_falls_through_to_scrollback() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        arm_mouse_mode_and_pane(&mut app, ws_id); // pane rect is height 24 (rows 0..23)
        // Row 30 is below the pane -> no pane under cursor -> scrollback even though
        // mouse mode is on.
        handle_mouse(
            &mut app,
            mouse_at_mod(MouseEventKind::ScrollUp, 10, 30, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(
            app.sessions
                .get(test_primary_instance(&app, ws_id))
                .unwrap()
                .scrollback_offset
                .load(Ordering::Relaxed),
            3,
            "wheel over chrome (no pane under cursor) drives wsx scrollback"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn plain_wheel_forwards_in_attached_pm() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        spawn_pm_for_test_local(&mut app);
        app.view = crate::ui::View::AttachedPm;
        let pm = app.pm.as_ref().unwrap().clone();
        {
            let mut p = pm.parser.lock().unwrap();
            p.process(b"\x1b[?1000h\x1b[?1006h");
        }
        app.attached_pane_rects = vec![(
            pm.clone(),
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 24,
            },
        )];
        handle_mouse(
            &mut app,
            mouse_at_mod(MouseEventKind::ScrollUp, 10, 10, KeyModifiers::NONE),
        )
        .await;
        assert_eq!(
            pm.scrollback_offset.load(Ordering::Relaxed),
            0,
            "wheel over the PM pane with mouse mode on is forwarded, not scrolled locally"
        );
    }
}

#[cfg(test)]
mod process_command_tests {
    use super::*;
    use crate::data::store::{Store, WorkspaceId};
    use crate::ui::modal::Modal;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn shared() -> SharedApp {
        Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ))
    }

    fn process_list(input: Option<String>) -> Modal {
        Modal::ProcessList {
            workspace_id: WorkspaceId(1),
            selected: 0,
            input,
            notice: None,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn r_enters_input_mode() {
        let mut app = App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap();
        app.modal = Some(process_list(None));
        let shared = shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), .. }) if b.is_empty()
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn typing_appends_and_backspace_pops() {
        let mut app = App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap();
        app.modal = Some(process_list(Some(String::new())));
        let shared = shared();
        for c in ['l', 's'] {
            handle_key_modal(
                &mut app,
                &shared,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            )
            .await
            .unwrap();
        }
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), .. }) if b == "ls"
        ));
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), .. }) if b == "l"
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn esc_in_input_mode_returns_to_list_mode() {
        let mut app = App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap();
        app.modal = Some(process_list(Some("npm".to_string())));
        let shared = shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: None, .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn enter_with_empty_command_is_a_noop() {
        let mut app = App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap();
        app.modal = Some(process_list(Some("   ".to_string())));
        let shared = shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), notice: None, .. }) if b == "   "
        ));
    }

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn footer_shows_run_hint_in_list_mode() {
        let theme = crate::ui::theme::Theme::default();
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            crate::ui::modal::render_process_list(f, f.area(), "demo", &[], 0, None, None, &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[r] run"), "{rendered}");
    }

    #[test]
    fn footer_shows_input_prompt_in_input_mode() {
        let theme = crate::ui::theme::Theme::default();
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            crate::ui::modal::render_process_list(
                f,
                f.area(),
                "demo",
                &[],
                0,
                Some("cargo run"),
                None,
                &theme,
            );
        })
        .unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("run: cargo run"), "{rendered}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn question_mark_ignored_without_workspace_selection() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let k = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);

        // 1. No selection (empty selectable, selection = None).
        handle_key_dashboard(&mut app, k).await.unwrap();
        assert!(
            app.modal.is_none(),
            "? with no selection should not open WorkspaceActions, got {:?}",
            app.modal
        );

        // 2. Repo selected — ? must be a no-op.
        app.selectable = vec![SelectionTarget::Repo(crate::data::store::RepoId(1))];
        app.select_index(0);
        handle_key_dashboard(&mut app, k).await.unwrap();
        assert!(
            app.modal.is_none(),
            "? with a repo selected should not open WorkspaceActions, got {:?}",
            app.modal
        );

        // 3. Workspace selected — positive control.
        app.selectable = vec![SelectionTarget::Workspace(WorkspaceId(1))];
        app.select_index(0);
        handle_key_dashboard(&mut app, k).await.unwrap();
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "? with workspace selected should open WorkspaceActions, got {:?}",
            app.modal
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn question_mark_opens_and_closes_workspace_actions_overlay() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![SelectionTarget::Workspace(WorkspaceId(1))];
        app.select_index(0);

        let open = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
        handle_key_dashboard(&mut app, open).await.unwrap();
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "expected WorkspaceActions modal open, got {:?}",
            app.modal
        );

        // Verify Esc closes it.
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        handle_key_modal(&mut app, &shared, esc).await.unwrap();
        assert!(app.modal.is_none(), "expected overlay dismissed on Esc");

        // Verify '?' also toggles the overlay closed while it is open.
        let open2 = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
        handle_key_dashboard(&mut app, open2).await.unwrap();
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "expected WorkspaceActions modal open on second open, got {:?}",
            app.modal
        );
        let close_q = KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE);
        handle_key_modal(&mut app, &shared, close_q).await.unwrap();
        assert!(app.modal.is_none(), "expected overlay dismissed on '?'");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn workspace_actions_overlay_navigates_and_dismisses() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let shared = shared();
        app.selectable = vec![SelectionTarget::Workspace(WorkspaceId(1))];
        app.select_index(0);

        // 1. Open the overlay with '?'.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "expected WorkspaceActions modal open, got {:?}",
            app.modal
        );

        // 2. Down via handle_key_modal keeps the card open.
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "Down should keep WorkspaceActions overlay open, got {:?}",
            app.modal
        );

        // 3. Action key 'c' closes the card (no workspace selected, so the
        //    action itself no-ops — the important thing is the overlay closes).
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            app.modal.is_none(),
            "action key 'c' should close the overlay"
        );

        // 4. Re-open with '?', then Enter closes the card.
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            matches!(app.modal, Some(Modal::WorkspaceActions)),
            "expected WorkspaceActions modal open again, got {:?}",
            app.modal
        );
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        // With no workspace selected, Enter is a no-op on the dashboard and sets
        // no new modal. Assert that the WorkspaceActions card at minimum is gone.
        assert!(
            !matches!(app.modal, Some(Modal::WorkspaceActions)),
            "Enter should close the WorkspaceActions overlay, got {:?}",
            app.modal
        );
    }
}

/// Ctrl-Z must never reach a child PTY: it raises SIGTSTP for the pane's
/// foreground job, and wsx captures every keystroke so there's no prompt
/// left to `fg` it back. Both PTY-forwarding encoders drop it.
#[cfg(test)]
mod ctrl_z_suppression_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn encode_key_swallows_ctrl_z() {
        let ev = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(
            encode_key(ev).is_empty(),
            "Ctrl-Z must not be forwarded to the PTY"
        );
        // Upper-case form (Shift+Ctrl-Z, or CapsLock) is the same byte.
        let ev_upper = KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::CONTROL);
        assert!(encode_key(ev_upper).is_empty());
    }

    #[test]
    fn encode_key_still_forwards_other_ctrl_keys() {
        // Sanity: a neighboring control key like Ctrl-C is untouched (0x03).
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(encode_key(ev), vec![0x03]);
    }

    #[test]
    fn encode_key_for_pty_swallows_ctrl_z() {
        let ev = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::CONTROL);
        assert!(
            encode_key_for_pty(&ev).is_none(),
            "Ctrl-Z must not be forwarded to the PM PTY"
        );
    }

    #[test]
    fn encode_key_for_pty_still_forwards_other_ctrl_keys() {
        let ev = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(encode_key_for_pty(&ev), Some(vec![0x03]));
    }
}
