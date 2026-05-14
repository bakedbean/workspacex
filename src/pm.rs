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

/// Initialize the PM working directory. Creates `dir` if needed and
/// runs `git init` inside it so Claude Code is happy. Idempotent — if
/// the directory already contains a git repo, this is a no-op.
pub fn init_pm_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    if dir.join(".git").is_dir() {
        return Ok(());
    }
    let status = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir)
        .status()
        .map_err(|e| Error::Io(std::io::Error::other(format!("git init pm dir: {e}"))))?;
    if !status.success() {
        return Err(Error::Git(format!(
            "git init failed in {} (exit {:?})",
            dir.display(),
            status.code()
        )));
    }
    Ok(())
}

const PM_SYSTEM_PROMPT: &str = "\
You are a project manager for a developer running multiple parallel coding \
workspaces under wsx. Each workspace is a git worktree with its own Claude \
Code session. Your job: when asked, inspect their active workspaces and \
report (1) what each was created for, (2) where it left off, (3) what's \
next to close it out.\n\
\n\
Where to find information:\n\
  - ./workspaces.json lists all active workspaces with: name, branch,\n\
    worktree_path, session_log_dir, git counts.\n\
  - For the original prompt: read the FIRST user message in the earliest\n\
    *.jsonl under session_log_dir.\n\
  - For recent activity: read the LAST several entries in the most recent\n\
    *.jsonl under session_log_dir.\n\
  - For code state: cd to worktree_path; use git status / log / diff.\n\
\n\
Constraints:\n\
  - Read-only. You cannot modify workspaces.\n\
  - Be concise — the developer is glancing at a small pane. Default to a\n\
    per-workspace block:\n\
        <name>: <one-line status>\n\
          - Created for: <one-line>\n\
          - Last activity: <one-line>\n\
          - Next: <one-line>\n\
  - If you're uncertain about \"next\", say so; don't fabricate.\n\
  - workspaces.json refreshes when the developer asks. Trust its contents\n\
    over stale memory.";

/// Build the PM system prompt, optionally appending custom instructions.
pub fn pm_system_prompt(custom: Option<&str>) -> String {
    match custom.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => PM_SYSTEM_PROMPT.to_string(),
        Some(extra) => format!("{PM_SYSTEM_PROMPT}\n\n{extra}"),
    }
}

/// Comma-separated read-only tool allowlist for the PM session.
pub fn pm_allowed_tools() -> &'static str {
    "Read,Bash(git status:*),Bash(git log:*),Bash(git diff:*),Bash(git branch:*),Bash(cat:*),Bash(ls:*)"
}

/// The initial user message wsx sends to PM after a Fresh spawn.
pub const PM_AUTO_SUMMARY_MESSAGE: &str =
    "Give me a status summary of all active workspaces per your instructions.";

/// The user message wsx sends to PM on `r` refresh.
pub const PM_REFRESH_MESSAGE: &str =
    "Refresh: workspaces.json has been updated. Re-summarize the current state of all workspaces.";

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
    fn init_pm_dir_creates_dir_and_git_init() {
        let dir = TempDir::new().unwrap();
        let pm_root = dir.path().join("pm");
        init_pm_dir(&pm_root).unwrap();
        assert!(pm_root.is_dir());
        assert!(pm_root.join(".git").is_dir(), "expected git repo init");
        // Idempotent: second call should not error.
        init_pm_dir(&pm_root).unwrap();
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

    #[test]
    fn system_prompt_contains_distinctive_phrases() {
        let p = pm_system_prompt(None);
        assert!(p.contains("project manager"), "{p}");
        assert!(p.contains("./workspaces.json"), "{p}");
        assert!(p.contains("session_log_dir"), "{p}");
    }

    #[test]
    fn system_prompt_appends_custom_instructions() {
        let p = pm_system_prompt(Some("Be extra terse."));
        assert!(p.contains("project manager"), "{p}");
        assert!(p.ends_with("Be extra terse."), "{p}");
        assert!(p.contains("\n\nBe extra terse."), "{p}");
    }

    #[test]
    fn allowed_tools_is_read_only_set() {
        let tools = pm_allowed_tools();
        assert!(tools.contains("Read"));
        assert!(tools.contains("Bash(git status:*)"));
        assert!(tools.contains("Bash(git log:*)"));
        assert!(tools.contains("Bash(git diff:*)"));
        assert!(tools.contains("Bash(cat:*)"));
        assert!(tools.contains("Bash(ls:*)"));
        assert!(!tools.contains("Write"));
        assert!(!tools.contains("Edit"));
        // Catch any inadvertent broad bash variant.
        assert!(!tools.split(',').any(|t| t.trim() == "Bash(*)"), "{tools}");
    }
}
