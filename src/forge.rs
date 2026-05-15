use crate::error::Result;
use serde::Deserialize;
use std::path::Path;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

/// Parse the JSON returned by `gh pr view <branch> --json state,isDraft,mergeable`.
/// Returns the lifecycle variant for a known PR, or `None` if the JSON is
/// missing or unparseable (callers treat unknown as "no info").
///
/// Priority for open PRs: CONFLICTING wins over draft, because a conflict
/// requires action regardless of whether the PR is marked ready.
pub(crate) fn parse_gh_pr_view(stdout: &str) -> Option<BranchLifecycle> {
    let parsed: GhPrView = serde_json::from_str(stdout.trim()).ok()?;
    let conflicted = parsed.mergeable.as_deref() == Some("CONFLICTING");
    match parsed.state.as_str() {
        "OPEN" if conflicted => Some(BranchLifecycle::PrConflicted),
        "OPEN" if parsed.is_draft => Some(BranchLifecycle::PrDraft),
        "OPEN" => Some(BranchLifecycle::PrOpen),
        "MERGED" => Some(BranchLifecycle::PrMerged),
        "CLOSED" => Some(BranchLifecycle::PrClosed),
        _ => None,
    }
}

/// Heuristic: `gh pr view` exits 1 with a stderr line like
/// `no pull requests found for branch "foo"` when the branch has no PR.
/// This is distinct from auth errors, network errors, or "no remote".
pub(crate) fn stderr_means_no_pr(stderr: &str) -> bool {
    stderr.contains("no pull requests found")
}

pub async fn fetch_branch_lifecycle(
    worktree: &Path,
    branch: &str,
) -> Result<Option<BranchLifecycle>> {
    let out = Command::new("gh")
        .current_dir(worktree)
        .args(["pr", "view", branch, "--json", "state,isDraft,mergeable"])
        .output()
        .await;

    let out = match out {
        Ok(o) => o,
        // gh not installed, not on PATH, permission error, etc. — degrade.
        Err(_) => return Ok(None),
    };

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Ok(parse_gh_pr_view(&stdout));
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr_means_no_pr(&stderr) {
        return Ok(Some(BranchLifecycle::NoPr));
    }

    // Auth failure, non-GitHub remote, network blip — degrade.
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_pr() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE"}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrOpen));
    }

    #[test]
    fn parses_open_pr_when_mergeable_missing() {
        // Older gh versions or non-PR responses may omit `mergeable`.
        let json = r#"{"state":"OPEN","isDraft":false}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrOpen));
    }

    #[test]
    fn parses_draft_pr() {
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"MERGEABLE"}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrDraft));
    }

    #[test]
    fn parses_conflicted_pr() {
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"CONFLICTING"}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrConflicted));
    }

    #[test]
    fn conflict_overrides_draft() {
        // A draft PR that's also conflicted: surface the conflict (more
        // actionable than the draft status).
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"CONFLICTING"}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrConflicted));
    }

    #[test]
    fn parses_merged_pr() {
        let json = r#"{"state":"MERGED","isDraft":false,"mergeable":"UNKNOWN"}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrMerged));
    }

    #[test]
    fn parses_closed_pr() {
        let json = r#"{"state":"CLOSED","isDraft":false,"mergeable":"UNKNOWN"}"#;
        assert_eq!(parse_gh_pr_view(json), Some(BranchLifecycle::PrClosed));
    }

    #[test]
    fn parser_returns_none_for_garbage() {
        assert_eq!(parse_gh_pr_view("not json"), None);
        assert_eq!(parse_gh_pr_view(""), None);
        assert_eq!(parse_gh_pr_view(r#"{"state":"WAT"}"#), None);
    }

    #[test]
    fn stderr_no_pr_heuristic() {
        assert!(stderr_means_no_pr(
            r#"no pull requests found for branch "foo""#
        ));
        assert!(!stderr_means_no_pr("error: not authenticated"));
        assert!(!stderr_means_no_pr(""));
    }

    /// Sanity check that fetch handles a non-git path gracefully.
    /// Should not panic; should return Ok(None) (treated as "unknown").
    #[tokio::test]
    async fn fetch_returns_none_on_non_git_path() {
        let tmp = tempfile::TempDir::new().unwrap();
        let result = fetch_branch_lifecycle(tmp.path(), "main").await;
        assert!(matches!(result, Ok(None)), "got {result:?}");
    }
}
