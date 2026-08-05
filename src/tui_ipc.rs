//! Unix-socket IPC listener for the running TUI. A per-process socket lets
//! external jumpers (the waybar workspace menu, the macOS menubar app) tell
//! a live TUI to select a workspace instead of spawning a new one.

use std::path::PathBuf;

use tokio::io::AsyncBufReadExt;

use crate::app::SharedApp;

pub fn socket_dir() -> PathBuf {
    if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(rt).join("wsx");
    }
    dirs::state_dir()
        .map(|d| d.join("wsx/run"))
        .unwrap_or_else(|| std::env::temp_dir().join("wsx-run"))
}

pub fn socket_path_for(pid: u32) -> PathBuf {
    socket_dir().join(format!("tui-{pid}.sock"))
}

pub fn live_socket_candidates() -> Vec<(PathBuf, u32)> {
    let mut found: Vec<(PathBuf, u32, std::time::SystemTime)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(socket_dir()) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(pid) = name
                .strip_prefix("tui-")
                .and_then(|s| s.strip_suffix(".sock"))
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            found.push((entry.path(), pid, mtime));
        }
    }
    found.sort_by_key(|(_, _, mtime)| std::cmp::Reverse(*mtime));
    found.into_iter().map(|(p, pid, _)| (p, pid)).collect()
}

/// Whether a `wsx` TUI is currently listening on one of its IPC sockets.
///
/// Messages queued by `wsx agent send` are only injected by a running TUI
/// (`App::drain_agent_messages`), so a queued handoff with no dashboard up
/// never reaches its target. A live listener accepts a connection; a stale
/// socket file left behind by a dead process refuses it.
pub fn any_live_tui() -> bool {
    live_socket_candidates()
        .into_iter()
        .any(|(path, _pid)| std::os::unix::net::UnixStream::connect(path).is_ok())
}

/// Wire protocol: `select <repo...> <slug>` — repo names may contain spaces,
/// slugs never do, so the last token is the slug.
pub fn parse_line(line: &str) -> Option<(String, String)> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "select" {
        return None;
    }
    let rest: Vec<&str> = tokens.collect();
    let (slug, repo_parts) = rest.split_last()?;
    if repo_parts.is_empty() {
        return None;
    }
    Some((repo_parts.join(" "), (*slug).to_string()))
}

pub async fn handle_line(app: &SharedApp, line: &str) -> bool {
    let Some((repo, slug)) = parse_line(line) else {
        return false;
    };
    let mut g = app.lock().await;
    // The workspace may have been created after the TUI last refreshed.
    let _ = g.refresh();
    g.open_workspace_by_name(&repo, &slug)
}

pub async fn listen(app: SharedApp, path: PathBuf) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&path);
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("waybar ipc: bind {} failed: {e}", path.display());
            return;
        }
    };
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("waybar ipc: accept failed: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }
        };
        let app = app.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let ok = handle_line(&app, &line).await;
                tracing::debug!("waybar ipc: {line:?} -> {ok}");
            }
        });
    }
}

#[cfg(test)]
mod ipc_tests {
    use super::*;

    #[test]
    fn parse_line_handles_spaces_in_repo_names() {
        assert_eq!(
            parse_line("select meals backend api-fix\n"),
            Some(("meals backend".into(), "api-fix".into()))
        );
        assert_eq!(
            parse_line("select alpha one"),
            Some(("alpha".into(), "one".into()))
        );
        assert_eq!(parse_line("select onlyslug"), None);
        assert_eq!(parse_line("nonsense alpha one"), None);
        assert_eq!(parse_line(""), None);
    }

    #[test]
    fn socket_path_shape() {
        let p = socket_path_for(4242);
        assert!(p.to_string_lossy().ends_with("tui-4242.sock"));
    }

    #[test]
    fn any_live_tui_detects_a_listener_and_ignores_a_stale_socket() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = crate::test_support::EnvGuard::new();
        guard.set("XDG_RUNTIME_DIR", dir.path());
        std::fs::create_dir_all(socket_dir()).unwrap();

        // Nothing at all.
        assert!(!any_live_tui(), "empty socket dir is not a live TUI");

        // A stale socket: bind then drop. Dropping a UnixListener does NOT
        // unlink the path, so the file survives with nobody listening —
        // exactly what a crashed TUI leaves behind.
        let stale = socket_path_for(999_999);
        {
            let _dead = std::os::unix::net::UnixListener::bind(&stale).unwrap();
        }
        assert!(stale.exists(), "precondition: stale socket file remains");
        assert!(
            !any_live_tui(),
            "a socket with no listener is not a live TUI"
        );
        std::fs::remove_file(&stale).unwrap();

        // A real listener.
        let live = socket_path_for(std::process::id());
        let _listener = std::os::unix::net::UnixListener::bind(&live).unwrap();
        assert!(any_live_tui(), "a bound listener is a live TUI");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_line_selects_workspace() {
        // Build a SharedApp exactly like src/app.rs:2703 app_with_one_workspace()
        // (in-memory store + App::new), wrapped in Arc<tokio::sync::Mutex<_>>.
        let app: crate::app::SharedApp = {
            use crate::data::store::{NewWorkspace, Store};
            let store = Store::open_in_memory().unwrap();
            let repo = store
                .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
                .unwrap();
            store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name: "a",
                    branch: "x/a",
                    worktree_path: std::path::Path::new("/tmp/r/a"),
                    yolo: false,
                    agent: crate::pty::session::AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            let app =
                crate::app::App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
            std::sync::Arc::new(tokio::sync::Mutex::new(app))
        };
        let (repo, slug) = {
            let g = app.lock().await;
            (g.repos[0].name.clone(), g.workspaces[0].1.name.clone())
        };
        assert!(handle_line(&app, &format!("select {repo} {slug}")).await);
        assert!(!handle_line(&app, "select nope nothing").await);
        assert!(!handle_line(&app, "garbage").await);
    }
}
