//! Fixtures shared across the input test modules.

use super::*;
use crate::data::store::Store;
use crate::test_support::EnvGuard;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use crossterm::event::{KeyEvent, KeyModifiers};

/// Test helper: an App with two Ready workspaces in a single repo, so
/// `build_pm_digest` yields a two-card digest. Modeled on the
/// `updates_panel_v_splits_attached_view_vertically`-style fixtures
/// elsewhere in this file: insert the repo/workspaces into the store,
/// mark them Ready, then construct the App (whose `refresh()` on
/// `App::new` picks them up).
pub(super) fn test_app_with_two_ready_workspaces() -> App {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    let store = Store::open_in_memory().unwrap();
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let first_id = store
        .insert_workspace(&NewWorkspace {
            repo_id,
            name: "first",
            branch: "repo/first",
            worktree_path: std::path::Path::new("/tmp/wsx-digest-1"),
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
            worktree_path: std::path::Path::new("/tmp/wsx-digest-2"),
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
    App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap()
}

/// `names` workspaces in one repo (named "repo"), all Ready. Returns
/// their ids in insertion order.
pub(super) fn seed_workspaces(
    store: &Store,
    names: &[&str],
) -> Vec<crate::data::store::WorkspaceId> {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    let repo_id = store
        .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
        .unwrap();
    let mut ids = Vec::new();
    for &name in names {
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name,
                branch: &format!("repo/{name}"),
                worktree_path: &std::path::PathBuf::from(format!("/tmp/wsx-test/{name}")),
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
    ids
}

/// Two workspaces in one repo, both Ready. Returns their ids in
/// insertion order (alpha, beta).
pub(super) fn seed_two_workspaces(store: &Store) -> Vec<crate::data::store::WorkspaceId> {
    seed_workspaces(store, &["alpha", "beta"])
}

pub(super) fn shared_app() -> SharedApp {
    Arc::new(Mutex::new(
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-test"),
        )
        .unwrap(),
    ))
}

pub(super) fn key(code: crossterm::event::KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

pub(super) fn mouse_event(kind: MouseEventKind) -> MouseEvent {
    MouseEvent {
        kind,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::NONE,
    }
}

pub(super) fn spawn_attached_workspace(app: &mut App) -> crate::data::store::WorkspaceId {
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

/// Test helper: create an App with N repos registered in the store
/// and loaded into app.repos. Uses a unique tmpdir per call so paths
/// don't collide.
pub(super) fn make_app_with_n_repos(n: usize) -> (App, Vec<crate::data::store::RepoId>) {
    let store = Store::open_in_memory().unwrap();
    let mut ids = Vec::new();
    for i in 0..n {
        let path = std::env::temp_dir().join(format!("wsx-fold-test-{}-{}", std::process::id(), i));
        let id = store.add_repo(&path, &format!("repo-{i}"), "").unwrap();
        ids.push(id);
    }
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-fold-test")).unwrap();
    app.refresh().unwrap();
    (app, ids)
}

pub(super) async fn press(app: &mut App, ch: char, mods: KeyModifiers) {
    handle_key_dashboard(app, KeyEvent::new(KeyCode::Char(ch), mods))
        .await
        .unwrap();
}

pub(super) async fn press_key(app: &mut App, code: KeyCode) {
    handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE))
        .await
        .unwrap();
}

/// One workspace with a live `claude` agent and a dead `codex#2` agent.
/// After attach-only filtering this flattens to a single row (the live
/// one), so it doubles as a fixture for "dead rows are hidden".
pub(super) fn mixed_liveness_remote_list() -> crate::app::RemoteList {
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
pub(super) fn all_dead_remote_list() -> crate::app::RemoteList {
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

pub(super) fn init_git_repo() -> tempfile::TempDir {
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
pub(super) async fn wait_until<F>(
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

pub(super) fn seed_app_with_workspace() -> App {
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
