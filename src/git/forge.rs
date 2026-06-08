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
    #[serde(default)]
    number: Option<u32>,
}

/// A branch's PR status: its lifecycle plus the PR number (when known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrStatus {
    pub lifecycle: BranchLifecycle,
    pub number: Option<u32>,
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
            "state,isDraft,mergeable,number",
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
        }));
    }

    // Auth failure, non-GitHub remote, network blip — degrade.
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let result = fetch_pr_status(tmp.path(), "main").await;
        assert!(matches!(result, Ok(None)), "got {result:?}");
    }
}
