//! Project Manager pane: dossier file + PM Claude Code session orchestration.

use crate::error::{Error, Result};
use crate::store::{Store, WorkspaceState};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct WorkspacesDossier {
    generated_at_epoch_seconds: i64,
    repos: Vec<RepoEntry>,
}

#[derive(Debug, Serialize)]
struct RepoEntry {
    name: String,
    path: PathBuf,
    workspaces: Vec<WorkspaceEntry>,
}

#[derive(Debug, Serialize)]
struct WorkspaceEntry {
    name: String,
    branch: String,
    worktree_path: PathBuf,
    session_log_dir: PathBuf,
    git: GitCounts,
}

#[derive(Debug, Serialize, Default)]
struct GitCounts {
    modified: usize,
    untracked: usize,
    ahead: usize,
    behind: usize,
}

/// Write the workspaces dossier file atomically (tempfile + rename).
///
/// Only `WorkspaceState::Ready` workspaces are included. Repos with no
/// Ready workspaces appear with an empty `workspaces` array.
pub fn write_workspaces_json(store: &Store, target: &Path) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut repos = Vec::new();
    for repo in store.repos()? {
        let mut workspaces = Vec::new();
        for ws in store.workspaces(repo.id)? {
            if ws.state != WorkspaceState::Ready {
                continue;
            }
            let session_log_dir = compute_session_log_dir(&ws.worktree_path);
            workspaces.push(WorkspaceEntry {
                name: ws.name,
                branch: ws.branch,
                worktree_path: ws.worktree_path,
                session_log_dir,
                git: GitCounts::default(),
            });
        }
        repos.push(RepoEntry {
            name: repo.name,
            path: repo.path,
            workspaces,
        });
    }

    let dossier = WorkspacesDossier {
        generated_at_epoch_seconds: now,
        repos,
    };

    let payload = serde_json::to_string_pretty(&dossier)
        .map_err(|e| Error::Io(std::io::Error::other(format!("workspaces.json serialize: {e}"))))?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, payload)?;
    std::fs::rename(&tmp, target)?;
    Ok(())
}

fn compute_session_log_dir(worktree: &Path) -> PathBuf {
    let abs = std::fs::canonicalize(worktree).unwrap_or_else(|_| worktree.to_path_buf());
    let encoded = crate::events::encode_cwd(&abs);
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    home.join(".claude/projects").join(encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewWorkspace, Store, WorkspaceState};
    use tempfile::TempDir;

    #[test]
    fn workspaces_json_includes_only_ready_filters_failed_and_pending() {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(Path::new("/tmp/fake-repo"), "demo", "")
            .unwrap();
        let ws_ready = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ready-one",
                branch: "demo/ready-one",
                worktree_path: Path::new("/tmp/wsx-wt/ready-one"),
            })
            .unwrap();
        store
            .set_workspace_state(ws_ready, WorkspaceState::Ready)
            .unwrap();
        let ws_failed = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "broken",
                branch: "demo/broken",
                worktree_path: Path::new("/tmp/wsx-wt/broken"),
            })
            .unwrap();
        store
            .set_workspace_state(ws_failed, WorkspaceState::Failed)
            .unwrap();

        let dir = TempDir::new().unwrap();
        let target = dir.path().join("workspaces.json");
        write_workspaces_json(&store, &target).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.contains("\"name\": \"ready-one\""), "{text}");
        assert!(!text.contains("\"name\": \"broken\""), "{text}");
        assert!(text.contains("\"generated_at_epoch_seconds\""), "{text}");
        assert!(text.contains("\"workspaces\": ["), "{text}");
    }

    #[test]
    fn workspaces_json_empty_repo_shows_empty_array() {
        let store = Store::open_in_memory().unwrap();
        store
            .add_repo(Path::new("/tmp/empty-repo"), "empty", "")
            .unwrap();
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("workspaces.json");
        write_workspaces_json(&store, &target).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.contains("\"name\": \"empty\""), "{text}");
        assert!(text.contains("\"workspaces\": []"), "{text}");
    }
}
