//! What gets launched: spawn info, doctrine, and tmux-shared sessions.

use super::*;
use crate::data::store::Store;
use crate::test_support::{EnvGuard, cat_path};
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

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
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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

/// Regression test: a primary `workspace_agents` row seeded strictly
/// *after* the app's last `refresh()` — mirroring `resolve_primary_instance`
/// backfilling a missing primary during attach, a path with no `refresh()`
/// on it — must still be recognized as running when the workspace is
/// unshared. Before the fix, `toggle_workspace_shared` derived `running`
/// from the cached `agent_roster`, which would still be missing this
/// instance; the cleanup loop would then treat its `session_ref` as
/// orphaned/detached and tmux-kill the live session instead of
/// respawning it as a direct child.
#[tokio::test]
async fn toggle_unshare_respawns_primary_seeded_after_last_refresh() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};

    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("tmux-calls.log");
    let fake = dir.path().join("fake-tmux.sh");
    std::fs::write(
        &fake,
        format!("#!/bin/sh\necho \"$@\" >> {}\nexit 0\n", log.display()),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut env = EnvGuard::new();
    env.set("WSX_TMUX_BIN", fake.to_str().unwrap());
    env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());

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
            agent: crate::pty::session::AgentKind::Codex,
            shared: true,
        })
        .unwrap();
    store
        .set_workspace_state(ws_id, WorkspaceState::Ready)
        .unwrap();
    // Deliberately no primary row yet — the exact anomaly
    // `resolve_primary_instance` exists to repair.

    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(
        app.agent_roster.get(&ws_id).is_none_or(|v| v.is_empty()),
        "sanity: no primary row existed as of the last refresh"
    );

    // Mimics the attach path: a primary row is seeded and a session
    // spawned for it, with no `refresh()` anywhere on that path — so
    // `agent_roster` never learns about this instance.
    let primary_id = test_primary_instance(&app, ws_id);
    app.store
        .set_instance_session_ref(primary_id, "wsx-r-w")
        .unwrap();
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: None,
        doctrine: None,
        additional_dirs: vec![],
        yolo: false,
    };
    app.sessions
        .spawn(
            primary_id,
            ws_id,
            dir.path(),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            crate::pty::session::AgentKind::Codex,
            None,
        )
        .unwrap();
    assert!(
        app.agent_roster.get(&ws_id).is_none_or(|v| v.is_empty()),
        "sanity: the cache still doesn't know about the backfilled primary"
    );

    crate::app::toggle_workspace_shared(&mut app, ws_id).unwrap();

    // The live instance must be respawned direct, not tmux-killed as if
    // it were an orphaned, detached session.
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    assert!(
        !calls.contains("kill-session"),
        "a live instance must be respawned, not tmux-killed as if detached; tmux calls: {calls:?}"
    );
    assert!(
        app.sessions.get(primary_id).is_some(),
        "the primary must come back up as a direct child after unsharing, not be left dead"
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
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut env = EnvGuard::new();
    env.set("WSX_TMUX_BIN", fake.to_str().unwrap());
    env.set(
        "WSX_CLAUDE_BIN",
        crate::test_support::cat_ignore_args_path(),
    );

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
/// the server-side session is still alive. The `shared_detached` sweep
/// must record that (it keeps the shared badge green), while the status
/// classifier reads plain `Idle` — detachment is badge liveness, not a
/// top-level status. A direct workspace (which never touches tmux) gets
/// neither. Uses a fake `WSX_TMUX_BIN` recorder that exits 0 for every
/// invocation, so `has-session` reads "alive" without a real tmux
/// server.
#[test]
fn shared_workspace_with_dead_client_but_live_tmux_is_marked_detached() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use crate::ui::dashboard::status::Status;

    let tmpdir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    let script = tmpdir.path().join("fake-tmux.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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

    assert!(
        app.shared_detached.contains(&shared_id),
        "the sweep must record the detached-but-alive session for badge liveness"
    );
    assert!(!app.shared_detached.contains(&direct_id));
    assert_eq!(app.classify_status(&shared_ws), Status::Idle);
    assert_eq!(app.classify_status(&direct_ws), Status::Idle);
}

/// A workspace whose only live wsx client belongs to a NON-primary
/// instance is not detached: someone is watching it. The sweep must
/// consider every instance's session, not just the primary's — with a
/// primary-only check, `has_client` reads false while the primary's
/// `session_ref` reads alive, and the workspace is wrongly swept into
/// `shared_detached`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_workspace_with_running_added_instance_is_not_detached() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};

    let tmpdir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    let script = tmpdir.path().join("fake-tmux.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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

    assert!(
        !app.shared_detached.contains(&ws_id),
        "a live client on a non-primary instance means someone is attached; not detached"
    );
}

/// Regression test for a stale-cache bug: an instance added (and given a
/// running session) strictly *after* the app's last `agent_roster` fill
/// must still be seen as live by the very next `refresh()` — unlike
/// `shared_workspace_with_running_added_instance_is_not_detached` above,
/// where the added instance already exists in the store before
/// `App::new`'s first refresh, so it's vacuously present in
/// `agent_roster` from the start. Here the instance is added, and its
/// session spawned, only after `App::new` returns — exercising the
/// window where a `refresh()` must pick up an instance that didn't
/// exist as of the previous roster fill.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_workspace_with_instance_added_after_last_refresh_is_not_detached() {
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};

    let tmpdir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    let script = tmpdir.path().join("fake-tmux.sh");
    std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
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

    // `App::new`'s first (unthrottled) sweep runs with only the primary
    // in the roster — no live client anywhere yet, so it marks `ws_id`
    // detached (the tmux script always reports alive).
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    assert!(
        app.shared_detached.contains(&ws_id),
        "sanity: no client yet, so the workspace starts out detached"
    );

    // Add a second instance to the store, then spawn it, strictly after
    // `App::new`'s roster fill — `agent_roster` does not know about it
    // yet.
    let added = app
        .store
        .add_workspace_agent(ws_id, crate::pty::session::AgentKind::Codex)
        .unwrap();
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

    // Force a fresh sweep on the very next refresh.
    app.shared_detached_polled_ms = 0;
    app.refresh().unwrap();

    assert!(
        !app.shared_detached.contains(&ws_id),
        "the just-added instance's live session must be visible to this refresh's sweep, \
         not lag a cycle behind"
    );
}
