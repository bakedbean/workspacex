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
    g.select_workspace_by_name(&repo, &slug)
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
        let Ok((stream, _)) = listener.accept().await else {
            continue;
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
