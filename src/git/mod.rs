#![allow(clippy::collapsible_if)]
//! Version control wrappers and git-host (forge) integration.
//!
//! This module is the `git -C <cwd>` command wrapper; the `forge` submodule
//! handles GitHub PR-lifecycle detection via `gh`.

pub mod forge;
pub mod github_remotes;

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Run `git -C <cwd> <args...>` and return stdout on success, mapping
/// non-zero exit + stderr into `Error::Git`.
async fn run(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .await
        .map_err(|e| Error::Git(format!("spawn git: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(Error::Git(format!(
            "git {} failed: {stderr}",
            args.join(" ")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub async fn validate_repo(path: &Path) -> Result<()> {
    let out = run(path, &["rev-parse", "--is-inside-work-tree"]).await?;
    if out.trim() != "true" {
        return Err(Error::Git(format!(
            "{} is not a git work tree",
            path.display()
        )));
    }
    Ok(())
}

pub async fn current_branch(path: &Path) -> Result<String> {
    let out = run(path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    Ok(out.trim().to_string())
}

pub async fn head_commit(path: &Path) -> Result<String> {
    let out = run(path, &["rev-parse", "HEAD"]).await?;
    Ok(out.trim().to_string())
}

pub async fn preflight() -> Result<()> {
    let out = Command::new("git")
        .arg("--version")
        .output()
        .await
        .map_err(|e| Error::Git(format!("git not found on PATH: {e}")))?;
    if !out.status.success() {
        return Err(Error::Git("git --version failed".into()));
    }
    Ok(())
}

/// Resolve the repo's base branch reference for diff comparisons.
/// Returns the upstream tracking ref (e.g. `origin/main`) when
/// `git symbolic-ref --short refs/remotes/origin/HEAD` succeeds — using
/// the upstream tracking ref means a stale local `main` doesn't poison
/// the diff. Falls back to `main` on any error (no origin, origin/HEAD
/// not set, git not installed, etc.).
pub async fn resolve_base_branch(worktree: &Path) -> String {
    let output = tokio::process::Command::new("git")
        .current_dir(worktree)
        .args(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "main".to_string(),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WorkspaceStatus {
    pub modified: u32,  // tracked-file changes (index or worktree), excludes untracked
    pub untracked: u32, // files matching ?? in porcelain v1
    pub ahead: u32,     // commits ahead of upstream
    pub behind: u32,    // commits behind upstream
}

impl WorkspaceStatus {
    pub fn is_clean(&self) -> bool {
        self.modified == 0 && self.untracked == 0 && self.ahead == 0 && self.behind == 0
    }
}

pub async fn workspace_status(worktree: &Path) -> Result<WorkspaceStatus> {
    let out = run(worktree, &["status", "-b", "--porcelain=v1"]).await?;
    Ok(parse_porcelain(&out))
}

fn parse_porcelain(out: &str) -> WorkspaceStatus {
    let mut status = WorkspaceStatus::default();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // Parse `[ahead N, behind M]` if present.
            if let Some(brk) = rest.find('[') {
                if let Some(close_rel) = rest[brk..].find(']') {
                    let inside = &rest[brk + 1..brk + close_rel];
                    for part in inside.split(',') {
                        let part = part.trim();
                        if let Some(n) = part.strip_prefix("ahead ").and_then(|s| s.parse().ok()) {
                            status.ahead = n;
                        } else if let Some(n) =
                            part.strip_prefix("behind ").and_then(|s| s.parse().ok())
                        {
                            status.behind = n;
                        }
                    }
                }
            }
        } else if line.starts_with("??") {
            status.untracked += 1;
        } else if line.len() >= 2 {
            // Any other 2-char XY status → tracked-file change
            status.modified += 1;
        }
    }
    status
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::process::Command as StdCmd;
    use tempfile::TempDir;

    pub(super) fn init_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let run = |args: &[&str]| {
            let status = StdCmd::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["commit", "--allow-empty", "-q", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn validate_repo_accepts_real_repo() {
        let dir = init_repo();
        validate_repo(dir.path()).await.unwrap();
    }

    #[tokio::test]
    async fn validate_repo_rejects_non_repo() {
        let dir = TempDir::new().unwrap();
        assert!(validate_repo(dir.path()).await.is_err());
    }

    #[tokio::test]
    async fn current_branch_and_head() {
        let dir = init_repo();
        assert_eq!(current_branch(dir.path()).await.unwrap(), "main");
        let head = head_commit(dir.path()).await.unwrap();
        assert_eq!(head.len(), 40);
    }

    #[tokio::test]
    async fn preflight_succeeds_when_git_on_path() {
        preflight().await.unwrap();
    }

    #[tokio::test]
    async fn status_clean_repo() {
        let dir = init_repo();
        let s = workspace_status(dir.path()).await.unwrap();
        assert!(s.is_clean(), "fresh repo should be clean, got {s:?}");
    }

    #[tokio::test]
    async fn status_counts_modified_and_untracked() {
        let dir = init_repo();
        // Commit a file so we can modify it.
        std::fs::write(dir.path().join("tracked.txt"), "v1").unwrap();
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
        r(&["add", "tracked.txt"]);
        r(&["commit", "-q", "-m", "track it"]);
        // Modify the tracked file and add an untracked one.
        std::fs::write(dir.path().join("tracked.txt"), "v2").unwrap();
        std::fs::write(dir.path().join("untracked.txt"), "new").unwrap();
        let s = workspace_status(dir.path()).await.unwrap();
        assert_eq!(s.modified, 1, "{s:?}");
        assert_eq!(s.untracked, 1, "{s:?}");
    }

    #[tokio::test]
    async fn resolve_base_branch_uses_origin_head_when_set() {
        let dir = TempDir::new().unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success(),
                "git {args:?} failed"
            );
        };
        r(&["init", "-q", "-b", "trunk"]);
        r(&["config", "user.email", "t@e"]);
        r(&["config", "user.name", "t"]);
        r(&["commit", "--allow-empty", "-q", "-m", "init"]);
        // Fake an origin that points at this repo so symbolic-ref has something to read.
        r(&["remote", "add", "origin", dir.path().to_str().unwrap()]);
        r(&["fetch", "-q", "origin"]);
        r(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ]);

        let base = resolve_base_branch(dir.path()).await;
        assert_eq!(base, "origin/trunk");
    }

    #[tokio::test]
    async fn resolve_base_branch_falls_back_to_main_without_origin() {
        let dir = TempDir::new().unwrap();
        std::process::Command::new("git")
            .current_dir(dir.path())
            .args(["init", "-q"])
            .status()
            .unwrap();
        let base = resolve_base_branch(dir.path()).await;
        assert_eq!(base, "main");
    }

    #[test]
    fn parse_ahead_behind_block() {
        let out = "## main...origin/main [ahead 2, behind 3]\n M src/main.rs\n?? newfile\n";
        let s = parse_porcelain(out);
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 3);
        assert_eq!(s.modified, 1);
        assert_eq!(s.untracked, 1);
    }

    #[test]
    fn parse_handles_no_upstream() {
        let out = "## main\n";
        let s = parse_porcelain(out);
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }

    #[test]
    fn parse_handles_detached_head() {
        let out = "## HEAD (no branch)\n M foo\n";
        let s = parse_porcelain(out);
        assert_eq!(s.modified, 1);
        assert_eq!(s.ahead, 0);
    }

    #[test]
    fn parse_shortstat_both() {
        assert_eq!(
            parse_shortstat(" 5 files changed, 32 insertions(+), 12 deletions(-)\n"),
            Some(DiffStats {
                added: 32,
                removed: 12
            })
        );
    }

    #[test]
    fn parse_shortstat_only_insertions() {
        assert_eq!(
            parse_shortstat(" 1 file changed, 18 insertions(+)\n"),
            Some(DiffStats {
                added: 18,
                removed: 0
            })
        );
    }

    #[test]
    fn parse_shortstat_only_deletions() {
        assert_eq!(
            parse_shortstat(" 2 files changed, 4 deletions(-)\n"),
            Some(DiffStats {
                added: 0,
                removed: 4
            })
        );
    }

    #[test]
    fn parse_shortstat_empty_returns_zero() {
        assert_eq!(
            parse_shortstat(""),
            Some(DiffStats {
                added: 0,
                removed: 0
            })
        );
        assert_eq!(
            parse_shortstat("\n"),
            Some(DiffStats {
                added: 0,
                removed: 0
            })
        );
    }

    #[test]
    fn parse_shortstat_malformed_returns_none() {
        assert_eq!(parse_shortstat("garbage line"), None);
    }
}

#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub head: Option<String>,
}

/// List configured remote names for `repo`. Returns an empty Vec if
/// `git remote` fails (e.g. no remotes configured).
pub async fn remote_names(repo: &Path) -> Vec<String> {
    let out = match run(repo, &["remote"]).await {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    out.lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// Fetch the named branch from a remote IF `base` looks like a
/// remote-tracking ref. Heuristic: split on the first `/`; if the
/// prefix matches a configured remote name, run `git fetch <remote>
/// <branch>`. Otherwise no-op. `None`, empty values, and values with
/// no `/` are also no-ops.
///
/// Errors from the fetch itself propagate to the caller so workspace
/// creation can fail fast on bad refs or network issues.
pub async fn fetch_for_base(repo: &Path, base: Option<&str>) -> Result<()> {
    let Some(value) = base else { return Ok(()) };
    let value = value.trim();
    if value.is_empty() {
        return Ok(());
    }
    let Some((prefix, rest)) = value.split_once('/') else {
        return Ok(());
    };
    if rest.is_empty() {
        return Ok(());
    }
    let remotes = remote_names(repo).await;
    if !remotes.iter().any(|r| r == prefix) {
        return Ok(());
    }
    run(repo, &["fetch", prefix, rest]).await?;
    Ok(())
}

pub async fn create_worktree(
    repo: &Path,
    branch: &str,
    base: Option<&str>,
    path: &Path,
) -> Result<()> {
    let path_s = path.to_string_lossy();
    let mut args: Vec<&str> = vec!["worktree", "add", "-b", branch, &path_s];
    if let Some(b) = base {
        args.push(b);
    }
    run(repo, &args).await?;
    // Snapshot the agent sessions already sitting at this path while we are
    // the only ones who know the worktree is brand new. Those indexes are
    // keyed on path and outlive the worktree, so without the snapshot a
    // recycled slug makes the next occupant resume the previous one's
    // conversation. Every caller funnels through here, which is why the
    // snapshot lives at this choke point rather than in `data::workspace`'s
    // two create paths — and why `import_existing`, which never calls this,
    // correctly stays unsnapshotted.
    //
    // A failure is fatal to creation rather than best-effort: the gitdir was
    // just written by git, so a write that fails here means something is
    // genuinely wrong with the filesystem, and a silently unsnapshotted
    // worktree is one the session-bleed bug can still reach.
    crate::pty::session::write_worktree_sessions(path)?;
    Ok(())
}

pub async fn remove_worktree(repo: &Path, path: &Path) -> Result<()> {
    if is_registered_worktree(repo, path).await? {
        let path_s = path.to_string_lossy();
        run(repo, &["worktree", "remove", "--force", &path_s]).await?;
    } else if path.exists() {
        // The directory exists but git never registered it as a worktree (a
        // half-created or manually-deleted worktree). `git worktree remove`
        // would fail with "is not a working tree", which would otherwise
        // strand the workspace (archival aborts before deleting its row), so
        // remove the orphaned directory directly and let archival finish.
        tokio::fs::remove_dir_all(path).await.map_err(|e| {
            Error::Io(std::io::Error::new(
                e.kind(),
                format!("removing orphaned worktree dir {}: {e}", path.display()),
            ))
        })?;
    }
    Ok(())
}

/// Whether `path` is currently registered as a git worktree of `repo`.
/// Compares canonicalized paths because `git worktree list` reports the
/// canonical path (e.g. `/private/var/...` on macOS).
///
/// Propagates `list_worktrees` failures rather than guessing: a broken repo,
/// missing `git`, or transient error leaves registration *unknown*, and the
/// caller must not treat "unknown" as "orphaned" and blindly delete the dir.
async fn is_registered_worktree(repo: &Path, path: &Path) -> Result<bool> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let worktrees = list_worktrees(repo).await?;
    Ok(worktrees.iter().any(|w| {
        let wp = std::fs::canonicalize(&w.path).unwrap_or_else(|_| w.path.clone());
        wp == target
    }))
}

pub async fn list_worktrees(repo: &Path) -> Result<Vec<WorktreeInfo>> {
    let out = run(repo, &["worktree", "list", "--porcelain"]).await?;
    let mut result = Vec::new();
    let mut cur: Option<WorktreeInfo> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            if let Some(w) = cur.take() {
                result.push(w);
            }
            cur = Some(WorktreeInfo {
                path: PathBuf::from(p),
                branch: None,
                head: None,
            });
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            if let Some(c) = cur.as_mut() {
                c.head = Some(h.to_string());
            }
        } else if let Some(b) = line.strip_prefix("branch ") {
            if let Some(c) = cur.as_mut() {
                c.branch = Some(b.strip_prefix("refs/heads/").unwrap_or(b).to_string());
            }
        }
    }
    if let Some(w) = cur.take() {
        result.push(w);
    }
    Ok(result)
}

pub async fn branch_delete(repo: &Path, branch: &str, force: bool) -> Result<()> {
    let flag = if force { "-D" } else { "-d" };
    run(repo, &["branch", flag, branch]).await?;
    Ok(())
}

pub async fn rename_branch(repo: &Path, old: &str, new: &str) -> Result<()> {
    run(repo, &["branch", "-m", old, new]).await?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStats {
    pub added: u32,
    pub removed: u32,
}

/// Parse the trailing line of `git diff --shortstat`.
/// Accepts both `N insertions(+)` and `N deletions(-)` in either order
/// or alone. Returns `None` on a non-empty line that doesn't match.
pub fn parse_shortstat(s: &str) -> Option<DiffStats> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Some(DiffStats {
            added: 0,
            removed: 0,
        });
    }
    let mut added: u32 = 0;
    let mut removed: u32 = 0;
    let mut saw_known_marker = false;
    for part in trimmed.split(',') {
        let part = part.trim();
        if let Some(n) = part
            .strip_suffix(" insertion(+)")
            .or_else(|| part.strip_suffix(" insertions(+)"))
        {
            added = n.parse().ok()?;
            saw_known_marker = true;
        } else if let Some(n) = part
            .strip_suffix(" deletion(-)")
            .or_else(|| part.strip_suffix(" deletions(-)"))
        {
            removed = n.parse().ok()?;
            saw_known_marker = true;
        } else if part.ends_with(" file changed") || part.ends_with(" files changed") {
            // Acceptable file-count prefix; ignore.
        } else {
            // Unknown segment — bail.
            return None;
        }
    }
    if saw_known_marker || trimmed.contains("file") {
        Some(DiffStats { added, removed })
    } else {
        None
    }
}

/// Compute line-count diff stats for a worktree against `base`.
/// Returns `None` on any git failure (missing base ref, etc.).
pub async fn workspace_diff_stats(worktree: &std::path::Path, base: &str) -> Option<DiffStats> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .arg("diff")
        .arg("--shortstat")
        .arg(format!("{base}...HEAD"))
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_shortstat(&stdout)
}

/// Per-file line-count diff stats for a worktree against `base`. Keyed by
/// path *relative to the worktree root*, as `git diff --numstat` emits
/// them. Binary files (numstat output `-`) are silently omitted.
/// Returns `None` on any git failure.
pub async fn workspace_diff_per_file(
    worktree: &std::path::Path,
    base: &str,
) -> Option<std::collections::HashMap<String, DiffStats>> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(worktree)
        .arg("diff")
        .arg("--numstat")
        .arg(format!("{base}...HEAD"))
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut map = std::collections::HashMap::new();
    for line in stdout.lines() {
        // numstat format: "<added>\t<removed>\t<path>"; "-" for binary.
        let mut parts = line.splitn(3, '\t');
        let added = parts.next().and_then(|s| s.parse::<u32>().ok());
        let removed = parts.next().and_then(|s| s.parse::<u32>().ok());
        let path = parts.next();
        if let (Some(a), Some(r), Some(p)) = (added, removed, path) {
            map.insert(
                p.to_string(),
                DiffStats {
                    added: a,
                    removed: r,
                },
            );
        }
    }
    Some(map)
}

#[cfg(test)]
mod worktree_tests {
    use super::tests::init_repo;
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn create_and_list_worktree() {
        let repo = init_repo();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("feature");
        create_worktree(repo.path(), "feature", None, &wt)
            .await
            .unwrap();
        let listed = list_worktrees(repo.path()).await.unwrap();
        // macOS resolves $TMPDIR (`/var/folders/...`) to `/private/var/folders/...`,
        // and `git worktree list` always reports the canonical path. Compare
        // canonicalized paths so the assertion works on macOS and Linux.
        let wt_canon = std::fs::canonicalize(&wt).unwrap();
        assert!(listed.iter().any(|w| {
            std::fs::canonicalize(&w.path)
                .map(|p| p == wt_canon)
                .unwrap_or(false)
                && w.branch.as_deref() == Some("feature")
        }));
    }

    #[tokio::test]
    async fn create_worktree_snapshots_existing_sessions() {
        // Prior-session detection reads this snapshot to tell "my session"
        // from one left by a previous occupant of the same path. It has to
        // exist from the moment the worktree does, since the agent may spawn
        // immediately after creation.
        let home = TempDir::new().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());

        let repo = init_repo();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("snapshotted");
        create_worktree(repo.path(), "snapshotted", None, &wt)
            .await
            .unwrap();
        let marker = repo
            .path()
            .join(".git/worktrees/snapshotted/info/wsx-worktree-sessions");
        assert!(marker.exists(), "expected snapshot at {}", marker.display());
    }

    #[tokio::test]
    async fn recreated_worktree_gets_a_fresh_snapshot() {
        // The whole point: same repo, same branch name, same path, second
        // life. `git worktree remove` takes the gitdir (and the snapshot) with
        // it, so the new occupant must be snapshotted anew — this time naming
        // the session the first occupant left behind.
        let home = TempDir::new().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());

        let repo = init_repo();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("recycled");
        create_worktree(repo.path(), "recycled", None, &wt)
            .await
            .unwrap();
        let marker = repo
            .path()
            .join(".git/worktrees/recycled/info/wsx-worktree-sessions");
        assert_eq!(
            std::fs::read_to_string(&marker).unwrap().trim(),
            "",
            "a worktree on a clean path snapshots nothing"
        );

        // The first occupant holds a conversation.
        let abs = std::fs::canonicalize(&wt).unwrap();
        let sessions = home
            .path()
            .join(".claude/projects")
            .join(crate::activity::events::encode_cwd(&abs));
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(sessions.join("first-life.jsonl"), "{}").unwrap();

        remove_worktree(repo.path(), &wt).await.unwrap();
        assert!(
            !marker.exists(),
            "worktree removal should take the snapshot with it"
        );

        run(repo.path(), &["branch", "-D", "recycled"])
            .await
            .unwrap();
        create_worktree(repo.path(), "recycled", None, &wt)
            .await
            .unwrap();
        let second = std::fs::read_to_string(&marker).unwrap();
        assert!(
            second.contains("claude:first-life.jsonl"),
            "the recreated worktree must name its predecessor's session; got {second:?}"
        );
    }

    #[tokio::test]
    async fn remove_worktree_cleans_up() {
        let repo = init_repo();
        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("scratch");
        create_worktree(repo.path(), "scratch", None, &wt)
            .await
            .unwrap();
        remove_worktree(repo.path(), &wt).await.unwrap();
        let listed = list_worktrees(repo.path()).await.unwrap();
        assert!(!listed.iter().any(|w| w.path == wt));
    }

    #[tokio::test]
    async fn remove_worktree_handles_orphaned_dir() {
        // A directory that exists on disk but was never registered as a git
        // worktree (a half-created or manually-deleted worktree) must still be
        // removable, not error with "is not a working tree".
        let repo = init_repo();
        let wt_root = TempDir::new().unwrap();
        let orphan = wt_root.path().join("orphan");
        std::fs::create_dir_all(orphan.join("portal")).unwrap();
        remove_worktree(repo.path(), &orphan).await.unwrap();
        assert!(!orphan.exists());
    }

    #[tokio::test]
    async fn remove_worktree_propagates_list_failure_without_deleting() {
        // If `git worktree list` fails (here: the repo arg isn't a git repo),
        // worktree-registration is unknown. We must surface the error rather
        // than fall through and delete the directory — a transient git failure
        // on a registered worktree must not turn into a blind `rm -rf`.
        let not_a_repo = TempDir::new().unwrap();
        let wt_root = TempDir::new().unwrap();
        let dir = wt_root.path().join("present");
        std::fs::create_dir_all(&dir).unwrap();
        let res = remove_worktree(not_a_repo.path(), &dir).await;
        assert!(
            res.is_err(),
            "expected error when worktree status is unknown"
        );
        assert!(dir.exists(), "must not delete dir when status is unknown");
    }

    #[tokio::test]
    async fn create_worktree_with_explicit_base() {
        let repo = init_repo();
        // Add a second commit on main so HEAD advances.
        std::fs::write(repo.path().join("a.txt"), "v1").unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(repo.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        r(&["add", "a.txt"]);
        r(&["commit", "-q", "-m", "add a"]);
        // Capture the previous commit (init) and create `staging` pointing at it.
        let prev = std::process::Command::new("git")
            .current_dir(repo.path())
            .args(["rev-parse", "HEAD~1"])
            .output()
            .unwrap();
        let prev_sha = String::from_utf8_lossy(&prev.stdout).trim().to_string();
        r(&["branch", "staging", &prev_sha]);

        let wt_root = TempDir::new().unwrap();
        let wt = wt_root.path().join("from-staging");
        create_worktree(repo.path(), "feature", Some("staging"), &wt)
            .await
            .unwrap();

        let head = std::process::Command::new("git")
            .current_dir(&wt)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        let wt_head = String::from_utf8_lossy(&head.stdout).trim().to_string();
        assert_eq!(
            wt_head, prev_sha,
            "new worktree should be at staging's commit, not main HEAD"
        );
    }

    /// Test helper: clone `src` as a bare remote and add it as `origin`
    /// in a fresh local repo. Returns (local_repo, _remote_dir_guard).
    async fn local_with_origin() -> (TempDir, TempDir) {
        let remote = init_repo();
        // Make the remote bare so it can be pushed to / fetched from.
        let bare = TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["clone", "--bare", "--quiet"])
            .arg(remote.path())
            .arg(bare.path())
            .status()
            .unwrap();

        let local = init_repo();
        let bare_url = format!("file://{}", bare.path().display());
        std::process::Command::new("git")
            .current_dir(local.path())
            .args(["remote", "add", "origin", &bare_url])
            .status()
            .unwrap();
        // Push a new branch on the remote that doesn't exist locally.
        std::process::Command::new("git")
            .current_dir(remote.path())
            .args(["checkout", "-q", "-b", "feature-x"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(remote.path())
            .args(["commit", "--allow-empty", "-q", "-m", "x"])
            .status()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(remote.path())
            .args(["push", "--quiet", &bare_url, "feature-x"])
            .status()
            .unwrap();

        // Keep both alive by returning both TempDirs to the caller.
        (local, bare)
    }

    #[tokio::test]
    async fn fetch_for_base_no_op_when_unset() {
        let repo = init_repo();
        fetch_for_base(repo.path(), None).await.unwrap();
    }

    #[tokio::test]
    async fn fetch_for_base_no_op_when_no_slash() {
        let repo = init_repo();
        fetch_for_base(repo.path(), Some("main")).await.unwrap();
    }

    #[tokio::test]
    async fn fetch_for_base_no_op_when_prefix_does_not_match_remote() {
        let repo = init_repo();
        // No remote named "feature" — base "feature/foo" is a local branch.
        fetch_for_base(repo.path(), Some("feature/foo"))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn fetch_for_base_fetches_when_prefix_matches_remote() {
        let (local, _bare) = local_with_origin().await;
        // Sanity: before fetch, origin/feature-x doesn't exist locally.
        let pre = std::process::Command::new("git")
            .current_dir(local.path())
            .args(["rev-parse", "--verify", "refs/remotes/origin/feature-x"])
            .output()
            .unwrap();
        assert!(!pre.status.success(), "ref should not exist pre-fetch");

        fetch_for_base(local.path(), Some("origin/feature-x"))
            .await
            .unwrap();

        let post = std::process::Command::new("git")
            .current_dir(local.path())
            .args(["rev-parse", "--verify", "refs/remotes/origin/feature-x"])
            .output()
            .unwrap();
        assert!(post.status.success(), "ref should exist after fetch");
    }
}
