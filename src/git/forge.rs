use crate::error::Result;
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BranchLifecycle {
    NoPr,
    PrDraft,
    PrOpen,
    PrConflicted,
    PrMerged,
    PrClosed,
}

#[derive(Debug, Deserialize)]
struct GhPrView {
    state: String,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
    #[serde(default)]
    mergeable: Option<String>,
    #[serde(default)]
    number: Option<u32>,
    #[serde(default)]
    url: Option<String>,
}

/// A branch's PR status: its lifecycle plus the PR number and URL (when known).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStatus {
    pub lifecycle: BranchLifecycle,
    pub number: Option<u32>,
    pub url: Option<String>,
}

/// Parse the JSON returned by
/// `gh pr view <branch> --json state,isDraft,mergeable,number`.
/// Returns the PR status for a known PR, or `None` if the JSON is missing
/// or unparseable (callers treat unknown as "no info").
///
/// Priority for open PRs: CONFLICTING wins over draft, because a conflict
/// requires action regardless of whether the PR is marked ready.
pub(crate) fn parse_gh_pr_status(stdout: &str) -> Option<PrStatus> {
    let parsed: GhPrView = serde_json::from_str(stdout.trim()).ok()?;
    let conflicted = parsed.mergeable.as_deref() == Some("CONFLICTING");
    let lifecycle = match parsed.state.as_str() {
        "OPEN" if conflicted => BranchLifecycle::PrConflicted,
        "OPEN" if parsed.is_draft => BranchLifecycle::PrDraft,
        "OPEN" => BranchLifecycle::PrOpen,
        "MERGED" => BranchLifecycle::PrMerged,
        "CLOSED" => BranchLifecycle::PrClosed,
        _ => return None,
    };
    Some(PrStatus {
        lifecycle,
        number: parsed.number,
        url: parsed.url,
    })
}

/// Heuristic: `gh pr view` exits 1 with a stderr line like
/// `no pull requests found for branch "foo"` when the branch has no PR.
/// This is distinct from auth errors, network errors, or "no remote".
pub(crate) fn stderr_means_no_pr(stderr: &str) -> bool {
    stderr.contains("no pull requests found")
}

pub async fn fetch_pr_status(worktree: &Path, branch: &str) -> Result<Option<PrStatus>> {
    let out = Command::new("gh")
        .current_dir(worktree)
        .args([
            "pr",
            "view",
            branch,
            "--json",
            "state,isDraft,mergeable,number,url",
        ])
        .output()
        .await;

    let out = match out {
        Ok(o) => o,
        // gh not installed, not on PATH, permission error, etc. — degrade.
        Err(_) => return Ok(None),
    };

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Ok(parse_gh_pr_status(&stdout));
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr_means_no_pr(&stderr) {
        return Ok(Some(PrStatus {
            lifecycle: BranchLifecycle::NoPr,
            number: None,
            url: None,
        }));
    }

    // Auth failure, non-GitHub remote, network blip — degrade.
    Ok(None)
}

/// The argv (after the `gh` program name) that opens `branch`'s PR in the
/// browser. Split out as a pure function so it can be unit-tested. Borrows
/// `branch` to match the `&[&str]` argv style used by `fetch_pr_status`.
pub(crate) fn pr_web_argv(branch: &str) -> Vec<&str> {
    vec!["pr", "view", branch, "--web"]
}

/// Open the PR for `branch` in the default browser via `gh pr view --web`.
/// Fire-and-forget: spawns detached and only logs spawn failures (gh itself
/// handles "no PR" / auth errors and we don't surface them on a click).
pub(crate) fn open_pr_in_browser(worktree: &Path, branch: &str) {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(pr_web_argv(branch))
        .current_dir(worktree)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Err(e) = cmd.spawn() {
        tracing::warn!(error = %e, branch, "failed to open PR in browser");
    }
}

/// The argv (after the `gh` program name) that opens the signed-in user's
/// open PRs for the repo in the browser. `gh` expands this to
/// `https://github.com/<owner>/<repo>/pulls?q=is:pr+is:open+author:@me`,
/// so wsx never has to learn the owner/repo slug or the user's login.
pub(crate) fn author_prs_web_argv() -> Vec<&'static str> {
    vec!["pr", "list", "--web", "--author", "@me"]
}

/// Open the signed-in user's open PRs for `repo` in the default browser.
/// Fire-and-forget on the same contract as [`open_pr_in_browser`]: gh
/// resolves the repo from `current_dir` and reports its own auth errors,
/// so only spawn failures are worth logging.
pub(crate) fn open_author_prs_in_browser(repo: &Path) {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(author_prs_web_argv())
        .current_dir(repo)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Err(e) = cmd.spawn() {
        tracing::warn!(error = %e, repo = %repo.display(), "failed to open author PRs in browser");
    }
}

/// The host component of a git remote URL, for the two forms git accepts:
/// `scheme://[user@]host[:port]/path` and scp-like `[user@]host:path`.
/// `None` for anything that names a local path rather than a host.
///
/// Both forms reduce to the same slice — everything before the first `/` —
/// because a scp-like URL's first `/` can only appear inside its path.
fn remote_host(url: &str) -> Option<&str> {
    let rest = match url.split_once("://") {
        // `file://` URLs are local paths wearing a scheme.
        Some((scheme, _)) if scheme.eq_ignore_ascii_case("file") => return None,
        Some((_, rest)) => rest,
        // No scheme: git reads the value as scp-like only when a colon
        // precedes any slash. Everything else — `/abs/path`, `./rel`, and
        // bare relative paths like `github.com/o/r.git` — is a local
        // directory that happens to be spelled like a host.
        None => {
            let colon = url.find(':')?;
            if url[..colon].contains('/') {
                return None;
            }
            url
        }
    };
    let authority = rest.split('/').next()?;
    let after_userinfo = authority.rsplit('@').next()?;
    // Trailing `:port` (URL form) or `:path` (scp-like form).
    after_userinfo.split(':').next()
}

/// Whether a git remote URL points at github.com. Self-hosted GitHub
/// Enterprise hosts deliberately don't count: `gh` may well handle them,
/// but we only claim what we can recognise.
fn url_is_github(url: &str) -> bool {
    remote_host(url).is_some_and(|h| {
        h.eq_ignore_ascii_case("github.com") || h.eq_ignore_ascii_case("www.github.com")
    })
}

/// Whether `repo`'s `origin` remote points at github.com. Blocking (it runs
/// one `git remote get-url`), so callers must memoise it rather than probe
/// per frame. Any failure — no origin, not a git repo, no `git` — reads as
/// "not GitHub", which hides the affordance rather than offering a dead one.
pub fn repo_has_github_remote(repo: &Path) -> bool {
    let out = std::process::Command::new("git")
        .current_dir(repo)
        .args(["remote", "get-url", "origin"])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => url_is_github(String::from_utf8_lossy(&o.stdout).trim()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pr_web_argv_builds_expected() {
        assert_eq!(
            pr_web_argv("feature/foo"),
            vec!["pr", "view", "feature/foo", "--web"]
        );
    }

    #[test]
    fn parses_open_pr() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":7}"#;
        let s = parse_gh_pr_status(json).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrOpen);
        assert_eq!(s.number, Some(7));
    }

    #[test]
    fn parses_open_pr_when_mergeable_missing() {
        let json = r#"{"state":"OPEN","isDraft":false,"number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrOpen)
        );
    }

    #[test]
    fn parses_draft_pr() {
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"MERGEABLE","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrDraft)
        );
    }

    #[test]
    fn parses_conflicted_pr() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"CONFLICTING","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrConflicted)
        );
    }

    #[test]
    fn conflict_overrides_draft() {
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"CONFLICTING","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrConflicted)
        );
    }

    #[test]
    fn parses_merged_pr() {
        let json = r#"{"state":"MERGED","isDraft":false,"mergeable":"UNKNOWN","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrMerged)
        );
    }

    #[test]
    fn parses_closed_pr() {
        let json = r#"{"state":"CLOSED","isDraft":false,"mergeable":"UNKNOWN","number":7}"#;
        assert_eq!(
            parse_gh_pr_status(json).map(|s| s.lifecycle),
            Some(BranchLifecycle::PrClosed)
        );
    }

    #[test]
    fn parser_returns_none_for_garbage() {
        assert!(parse_gh_pr_status("not json").is_none());
        assert!(parse_gh_pr_status("").is_none());
        assert!(parse_gh_pr_status(r#"{"state":"WAT"}"#).is_none());
    }

    #[test]
    fn parses_pr_number() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":152}"#;
        assert_eq!(parse_gh_pr_status(json).unwrap().number, Some(152));
    }

    #[test]
    fn tolerates_missing_number() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE"}"#;
        let s = parse_gh_pr_status(json).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrOpen);
        assert_eq!(s.number, None);
    }

    #[test]
    fn parse_carries_pr_url() {
        let s = parse_gh_pr_status(
            r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":5,"url":"https://github.com/o/r/pull/5"}"#,
        )
        .unwrap();
        assert_eq!(s.url.as_deref(), Some("https://github.com/o/r/pull/5"));
        // Absent url stays None.
        let s = parse_gh_pr_status(r#"{"state":"MERGED","number":9}"#).unwrap();
        assert_eq!(s.url, None);
    }

    #[test]
    fn stderr_no_pr_heuristic() {
        assert!(stderr_means_no_pr(
            r#"no pull requests found for branch "foo""#
        ));
        assert!(!stderr_means_no_pr("error: not authenticated"));
        assert!(!stderr_means_no_pr(""));
    }

    #[test]
    fn lifecycle_serde_round_trips_every_variant() {
        // The shared-workspace wire contract (SharedWorkspaceRecord) carries
        // this over ssh, so every variant must survive JSON serialize →
        // deserialize unchanged.
        for lc in [
            BranchLifecycle::NoPr,
            BranchLifecycle::PrDraft,
            BranchLifecycle::PrOpen,
            BranchLifecycle::PrConflicted,
            BranchLifecycle::PrMerged,
            BranchLifecycle::PrClosed,
        ] {
            let json = serde_json::to_string(&lc).unwrap();
            let back: BranchLifecycle = serde_json::from_str(&json).unwrap();
            assert_eq!(lc, back, "round-trip failed for {lc:?} (json {json})");
        }
    }

    /// Sanity check that fetch handles a non-git path gracefully.
    /// Should not panic; should return Ok(None) (treated as "unknown").
    #[tokio::test]
    async fn fetch_returns_none_on_non_git_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = fetch_pr_status(tmp.path(), "main").await;
        assert!(matches!(result, Ok(None)), "got {result:?}");
    }

    #[test]
    fn author_prs_web_argv_builds_expected() {
        assert_eq!(
            author_prs_web_argv(),
            vec!["pr", "list", "--web", "--author", "@me"]
        );
    }

    #[test]
    fn url_is_github_accepts_every_remote_form() {
        for url in [
            "https://github.com/o/r.git",
            "https://github.com/o/r",
            "http://github.com/o/r",
            "https://eben@github.com/o/r.git",
            "git@github.com:o/r.git",
            // scp-like without a user is still scp-like.
            "github.com:o/r.git",
            "ssh://git@github.com/o/r.git",
            "ssh://git@github.com:22/o/r.git",
            "git://github.com/o/r.git",
            // Host comparison is case-insensitive.
            "https://GitHub.com/o/r.git",
            "https://www.github.com/o/r.git",
        ] {
            assert!(url_is_github(url), "should be GitHub: {url}");
        }
    }

    #[test]
    fn url_is_github_rejects_lookalike_and_other_hosts() {
        for url in [
            "https://gitlab.com/o/r.git",
            // Self-hosted GHE: `gh` may well handle it, but we only claim
            // github.com. A naive `contains("github.com")` passes these.
            "git@github.example.com:o/r.git",
            "https://github.example.com/o/r.git",
            "https://github.com.evil.example/o/r.git",
            // Local paths and file:// remotes have no GitHub host at all.
            "/home/eben/mirrors/r.git",
            "file:///home/eben/mirrors/r.git",
            "",
            // A bare relative path. Git only reads a no-scheme value as
            // scp-like when a colon precedes any slash, so this names a
            // local directory, not github.com — and `gh` can't resolve it.
            "github.com/o/r.git",
            "./github.com/o/r.git",
            // Colon present, but after a slash: still a local path.
            "mirrors/github.com:o/r.git",
        ] {
            assert!(!url_is_github(url), "should not be GitHub: {url}");
        }
    }

    /// Test helper: a git repo whose `origin` is `url` (or no origin at all
    /// when `url` is `None`). The remote is never contacted, so a bogus URL
    /// is fine.
    fn repo_with_origin(url: Option<&str>) -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
        };
        git(&["init", "-q"]);
        if let Some(url) = url {
            git(&["remote", "add", "origin", url]);
        }
        dir
    }

    #[test]
    fn detects_a_github_origin() {
        let repo = repo_with_origin(Some("git@github.com:bakedbean/workspacex.git"));
        assert!(repo_has_github_remote(repo.path()));
    }

    #[test]
    fn non_github_origin_is_not_a_github_remote() {
        let repo = repo_with_origin(Some("https://gitlab.com/o/r.git"));
        assert!(!repo_has_github_remote(repo.path()));
    }

    #[test]
    fn repo_without_origin_is_not_a_github_remote() {
        let repo = repo_with_origin(None);
        assert!(!repo_has_github_remote(repo.path()));
    }

    /// A path that isn't a git repo at all must degrade quietly, not panic —
    /// the dashboard probes every registered repo path on refresh.
    #[test]
    fn non_git_path_is_not_a_github_remote() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(!repo_has_github_remote(tmp.path()));
    }
}
