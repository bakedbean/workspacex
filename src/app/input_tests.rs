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
        app.dashboard.selected = 2;
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
        app.dashboard.selected = 0;
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
        app.dashboard.selected = 1;
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
    async fn ctrl_x_esc_detach_schedules_refresh_for_attached_workspace() {
        // Same as the d-path test above, for the Ctrl-X Esc detach.
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
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();

        assert!(matches!(app.view, crate::ui::View::Dashboard));
        assert!(
            app.pending_workspace_refresh.contains(&id),
            "Esc-detached workspace should be queued for refresh"
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
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(test_target(&app, attached_id)));

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        // The bottom row is the footer with "Ctrl-x d detach". The second-
        // to-last row should NOT contain a status indicator glyph.
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

    fn mouse_event(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn spawn_pm_for_test(app: &mut App) {
        // Use AgentKind::Codex (WSX_CODEX_BIN=cat) because build_codex_command
        // injects no extra flags for a plain Fresh session, so cat stays alive.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", cat_path());
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
        // Use AgentKind::Codex (WSX_CODEX_BIN=cat) because build_codex_command
        // injects no extra flags for a plain Fresh session, so cat stays alive.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", cat_path());
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
        app.dashboard.selected = 0;

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
        app.dashboard.selected = 0;

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
        app.dashboard.selected = 0;

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
        app.dashboard.selected = 0;

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
        app.dashboard.selected = 0;
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
        app.dashboard.selected = 0;
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
        app.dashboard.selected = 0;

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
        app.dashboard.selected = 0;

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
        app.dashboard.selected = 0;
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
        app.dashboard.selected = 0;
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
        app.dashboard.selected = 0;
        press(&mut app, 'j', KeyModifiers::NONE).await;
        assert_eq!(app.dashboard.selected, 1, "j should advance like Down");
    }

    #[tokio::test]
    async fn k_alias_retreats_selection_like_up() {
        let (mut app, _) = make_app_with_n_repos(3);
        app.dashboard.selected = 2;
        press(&mut app, 'k', KeyModifiers::NONE).await;
        assert_eq!(app.dashboard.selected, 1, "k should retreat like Up");
    }

    #[tokio::test]
    async fn k_does_not_open_process_list_anymore() {
        // `k` is now a nav alias for Up. Process list must be opened by `K`.
        let (mut app, _) = make_app_with_n_repos(1);
        app.dashboard.selected = 0;
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
        app.dashboard.selected = idx;
        press(&mut app, 'K', KeyModifiers::SHIFT).await;
        assert!(
            matches!(app.modal, Some(Modal::ProcessList { workspace_id, .. }) if workspace_id == ws_id),
            "K on a workspace row should open ProcessList"
        );
    }

    #[tokio::test]
    async fn shift_k_moves_selected_repo_up() {
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.selected = 1; // select repo-1 (Repo header)
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
        app.dashboard.selected = 1; // select repo-1
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
        app.dashboard.selected = 0; // top repo
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
        app.dashboard.selected = 2; // bottom repo
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
        app.dashboard.selected = 0; // select repo-0 (top)
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
        app.dashboard.selected = idx;
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
        app.dashboard.selected = 0;
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
    async fn h_folds_focused_repo() {
        let (mut app, ids) = make_app_with_n_repos(2);
        app.dashboard.selected = 0;
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
        app.dashboard.selected = 0;
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
        app.dashboard.selected = 0;
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
        app.dashboard.selected = idx;

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
        app.dashboard.selected = repo_idx;

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
mod ctrl_x_esc_tests {
    use super::*;
    use crate::test_support::{EnvGuard, cat_path};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_esc_saves_layout_and_returns_to_dashboard() {
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
            )
            .unwrap();

        let first_target = test_target(&app, first_id);
        let second_target = test_target(&app, second_id);
        let mut state = AttachedState::single(first_target);
        state.split(SplitDirection::Vertical, second_target);
        app.view = crate::ui::View::Attached(state);

        // Send Ctrl-x then Esc.
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
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
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
        app.dashboard.selected = idx;
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
        app.dashboard.selected = idx;
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
            )
            .unwrap();

        app.view = crate::ui::View::Dashboard;
        app.selectable = vec![crate::app::SelectionTarget::Workspace(ws_id)];
        app.dashboard.selected = 0;
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
}
