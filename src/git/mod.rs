#![allow(clippy::collapsible_if)]
//! Version control wrappers and git-host (forge) integration.
//!
//! This module is the `git -C <cwd>` command wrapper; the `forge` submodule
//! handles GitHub PR-lifecycle detection via `gh`.

pub mod forge;

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
    Ok(())
}

pub async fn remove_worktree(repo: &Path, path: &Path) -> Result<()> {
    let path_s = path.to_string_lossy();
    run(repo, &["worktree", "remove", "--force", &path_s]).await?;
    Ok(())
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
