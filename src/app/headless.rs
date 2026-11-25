//! Starting a workspace's agent from a process that isn't a dashboard.

use super::*;
use crate::pty::session::Session;

/// Share `ws` and start its agents in tmux, returning the primary's session.
/// Dropping that session ends only its tmux client; the agent lives on.
pub(crate) fn start(app: &mut App, ws: WorkspaceId) -> Result<Arc<Session>> {
    // Inside tmux a missing binary is not a spawn error: the pane just exits.
    for inst in app.store.workspace_agents(ws)? {
        if let Some(bin) = crate::pty::session::missing_agent_binary(inst.agent) {
            return Err(crate::error::Error::AgentBinaryMissing(bin));
        }
    }
    session::force_share_workspace(app, ws)?;
    app.store
        .primary_instance_id(ws)?
        .and_then(|id| app.session_for(id))
        .ok_or_else(|| crate::error::Error::UserInput("the agent did not start".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{NewWorkspace, SetupStatus, WorkspaceState};
    use crate::pty::session::AgentKind;
    use crate::pty::tmux;
    use crate::test_support::EnvGuard;
    use std::time::Duration;

    struct TmuxServer(PathBuf);
    impl Drop for TmuxServer {
        fn drop(&mut self) {
            let _ = std::process::Command::new(tmux::tmux_bin())
                .env("TMUX_TMPDIR", &self.0)
                .env_remove("TMUX")
                .arg("kill-server")
                .output();
        }
    }

    fn add(store: &Store, wt: &std::path::Path, state: WorkspaceState) -> WorkspaceId {
        let repo = match crate::data::repo::list(store).unwrap().pop() {
            Some(r) => r.id,
            None => store.add_repo(wt, "hl", "").unwrap(),
        };
        let name = format!("w{}", store.workspaces(repo).unwrap().len());
        let wt = wt.join(&name);
        if wt.parent().is_some_and(|p| p.is_dir()) {
            std::fs::create_dir_all(&wt).unwrap();
        }
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: &name,
                branch: &name,
                worktree_path: &wt,
                yolo: true,
                agent: AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store.add_primary_agent(id, AgentKind::Claude, 1).unwrap();
        store.set_workspace_state(id, state).unwrap();
        id
    }

    #[test]
    fn a_headless_app_leaves_other_processes_creates_alone() {
        for headless in [true, false] {
            let tmp = tempfile::tempdir().unwrap();
            let db = tmp.path().join("t.db");
            let store = Store::open(&db).unwrap();
            let pending = add(&store, tmp.path(), WorkspaceState::Pending);
            let running = add(&store, tmp.path(), WorkspaceState::Ready);
            store
                .conn()
                .execute("UPDATE workspaces SET created_at = 0", [])
                .unwrap();
            store
                .set_setup_status(running, SetupStatus::Running)
                .unwrap();
            let app = if headless {
                App::new_headless(store, tmp.path().into()).unwrap()
            } else {
                App::new(store, tmp.path().into()).unwrap()
            };
            let get = |id| app.store.workspace_by_id(id).unwrap().unwrap();
            assert_eq!(get(pending).state == WorkspaceState::Pending, headless);
            assert_eq!(get(running).setup_status == SetupStatus::Running, headless);
        }
    }

    async fn until(what: &str, f: impl Fn() -> bool) {
        for _ in 0..80 {
            if f() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("timed out waiting for {what}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn start_leaves_an_agent_a_second_app_reattaches_to() {
        if !tmux::is_available() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let _server = TmuxServer(tmp.path().into());
        let script = tmp.path().join("agent.sh");
        std::fs::write(&script, "#!/bin/sh\necho AGENT_HERE\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let mut env = EnvGuard::new();
        env.set("TMUX_TMPDIR", tmp.path());
        env.set("TERM", "xterm-256color");
        env.set("HOME", tmp.path());
        env.set("WSX_CLAUDE_BIN", &script);

        let db = tmp.path().join("t.db");
        let store = Store::open(&db).unwrap();
        store.set_setting("mcp_mirror", "off").unwrap();
        let ws = add(&store, tmp.path(), WorkspaceState::Ready);
        let mut app = App::new_headless(store, tmp.path().into()).unwrap();
        let session = start(&mut app, ws).unwrap();
        let inst = app.store.workspace_agents(ws).unwrap().remove(0);
        let name = inst.session_ref.expect("session name stored");
        until("session", || tmux::has_session(&name)).await;
        until("agent output", || {
            tmux::capture_pane(&name).is_some_and(|s| s.contains("AGENT_HERE"))
        })
        .await;
        assert_eq!(tmux::capture_pane("wsx-not-a-session"), None);

        drop(session);
        drop(app);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(tmux::has_session(&name), "agent must outlive the starter");

        let mut again = App::new_headless(Store::open(&db).unwrap(), tmp.path().into()).unwrap();
        let _session = start(&mut again, ws).unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        let panes = std::process::Command::new(tmux::tmux_bin())
            .env_remove("TMUX")
            .args(["list-panes", "-a"])
            .output()
            .unwrap();
        let panes = String::from_utf8_lossy(&panes.stdout).lines().count();
        assert_eq!(panes, 1, "reattached, not duplicated");
    }

    #[tokio::test]
    async fn a_missing_worktree_is_an_error_and_raises_no_modal() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", "/bin/sh");
        let store = Store::open_in_memory().unwrap();
        let ws = add(&store, &tmp.path().join("gone"), WorkspaceState::Ready);
        let mut app = App::new_headless(store, tmp.path().into()).unwrap();
        let e = start(&mut app, ws).err().expect("must fail").to_string();
        assert!(e.contains("worktree missing") || e.contains("tmux"), "{e}");
        assert!(app.modal.is_none());
    }

    #[tokio::test]
    async fn a_missing_agent_binary_is_named() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", "/nonexistent/wsx-no-claude");
        let store = Store::open_in_memory().unwrap();
        let ws = add(&store, tmp.path(), WorkspaceState::Ready);
        let mut app = App::new_headless(store, tmp.path().into()).unwrap();
        let e = start(&mut app, ws).err().expect("must fail").to_string();
        assert!(e.contains("wsx-no-claude"), "{e}");
        assert!(app.modal.is_none());
    }
}
