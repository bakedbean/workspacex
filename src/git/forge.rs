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

impl BranchLifecycle {
    /// Whether a PR in this state can still be waiting on a reviewer.
    ///
    /// Conflicted counts: it's still open and can sit in someone's review
    /// queue. Drafts don't — a PR isn't eligible for approval until it's
    /// marked ready for review, so any verdict GitHub reports on one is
    /// noise. Merged and closed don't either — GitHub leaves
    /// `reviewDecision` populated after the fact, so without this the ✓
    /// would become permanent furniture on every merged PR.
    pub(crate) fn awaits_review(self) -> bool {
        matches!(self, Self::PrOpen | Self::PrConflicted)
    }
}

/// A PR's aggregate review verdict, as GitHub computes it from the repo's
/// branch-protection rules and the reviews submitted so far.
///
/// Deliberately has no "not gated" variant: repos that require no approval
/// map to `None` rather than to a variant, so they show no indicator at all.
/// `ReviewRequired` means GitHub is actively *waiting* on an approval — a
/// meaningfully different state from "this repo doesn't work that way".
///
/// Which of the two an empty `reviewDecision` means is not something GitHub
/// tells us directly — see [`parse_review_decision`] and
/// [`fetch_requires_approval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

/// Map GitHub's `reviewDecision` enum to [`ReviewDecision`]. `None` for the
/// empty string and for any value GitHub might add later — an unrecognised
/// verdict degrades to "no indicator" rather than failing the whole PR parse.
///
/// An empty string is *not* proof the repo has no approval gate. GitHub only
/// computes `REVIEW_REQUIRED` from classic branch protection; when the
/// requirement comes from a repository **ruleset** the field stays empty
/// until an approval actually lands, then flips straight to `APPROVED`.
/// [`fetch_requires_approval`] is what tells the two apart.
fn parse_review_decision(raw: &str) -> Option<ReviewDecision> {
    match raw {
        "APPROVED" => Some(ReviewDecision::Approved),
        "CHANGES_REQUESTED" => Some(ReviewDecision::ChangesRequested),
        "REVIEW_REQUIRED" => Some(ReviewDecision::ReviewRequired),
        _ => None,
    }
}

/// A rule the REST rules endpoint reports as applying to a branch. Only the
/// discriminant and the `pull_request` parameters are modelled; every other
/// rule type carries parameters we don't read, and serde drops them.
#[derive(Debug, Deserialize)]
struct GhBranchRule {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    parameters: Option<GhPullRequestRuleParams>,
}

/// The two independent ways a ruleset's `pull_request` rule can demand an
/// approval: a repo-wide count, and per-file-pattern reviewer teams that
/// each carry their own minimum. A ruleset can set either without the other,
/// so both are read.
#[derive(Debug, Deserialize)]
struct GhPullRequestRuleParams {
    #[serde(default)]
    required_approving_review_count: u32,
    #[serde(default)]
    required_reviewers: Vec<GhRequiredReviewer>,
}

#[derive(Debug, Deserialize)]
struct GhRequiredReviewer {
    #[serde(default)]
    minimum_approvals: u32,
}

/// Whether the branch rules in `stdout` gate merging on an approving review.
///
/// `None` — not `Some(false)` — when the body doesn't parse as a rule array:
/// the endpoint answers auth and rate-limit failures with a JSON *object*
/// (`{"message": ...}`), and reading one of those as "no gate" would silently
/// turn a transient error into a missing indicator.
pub(crate) fn parse_requires_approval(stdout: &str) -> Option<bool> {
    let rules: Vec<GhBranchRule> = serde_json::from_str(stdout.trim()).ok()?;
    Some(rules.iter().any(|r| {
        r.kind == "pull_request"
            && r.parameters.as_ref().is_some_and(|p| {
                p.required_approving_review_count >= 1
                    || p.required_reviewers
                        .iter()
                        .any(|rr| rr.minimum_approvals >= 1)
            })
    }))
}

/// The `owner/repo` slug from a PR's HTML URL, so the rules probe can name
/// the repo outright instead of relying on `gh`'s cwd-based `{owner}/{repo}`
/// placeholder. `gh pr view` already returns `url`, so this costs no extra
/// call. `None` for anything not shaped like `<host>/<owner>/<repo>/pull/N`.
pub(crate) fn repo_slug_from_pr_url(url: &str) -> Option<String> {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let mut segs = rest.split('/').skip(1); // skip the host
    let owner = segs.next().filter(|s| !s.is_empty())?;
    let repo = segs.next().filter(|s| !s.is_empty())?;
    // Guard against matching some other `/owner/thing/...` path shape.
    if segs.next() != Some("pull") {
        return None;
    }
    Some(format!("{owner}/{repo}"))
}

/// The PR's base branch, read from the same `gh pr view` payload the status
/// comes from. Kept off [`PrStatus`] deliberately: that type is what gets
/// persisted into `scm_cache`, and the base ref is needed only in-flight, to
/// address the rules probe.
pub(crate) fn parse_pr_base_ref(stdout: &str) -> Option<String> {
    let parsed: GhPrView = serde_json::from_str(stdout.trim()).ok()?;
    parsed.base_ref_name.filter(|b| !b.is_empty())
}

/// The argv (after `gh`) that lists the rules active on `base` in `slug`.
/// Split out to be unit-testable for the same reason as
/// [`pr_view_json_fields`]: a malformed path would answer 404, which the
/// caller can't tell from "this branch has no rules".
pub(crate) fn branch_rules_argv(slug: &str, base: &str) -> Vec<String> {
    vec!["api".into(), format!("repos/{slug}/rules/branches/{base}")]
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
    #[serde(rename = "reviewDecision", default)]
    review_decision: Option<String>,
    #[serde(rename = "baseRefName", default)]
    base_ref_name: Option<String>,
}

/// A branch's PR status: its lifecycle plus the PR number and URL (when known).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrStatus {
    pub lifecycle: BranchLifecycle,
    pub number: Option<u32>,
    pub url: Option<String>,
    /// The PR's aggregate review verdict, or `None` when the repo has no
    /// approval gate, when `gh` didn't report one, or when the value wasn't
    /// recognised. Populated for every lifecycle; consumers decide which
    /// lifecycles are worth showing it for (see `ui::theme::review_chip`).
    pub review: Option<ReviewDecision>,
    /// How many review threads on the PR are still unresolved, or `None`
    /// when it wasn't fetched (no verdict to hang it on, PR not open, or
    /// the GraphQL probe failed). `Some(0)` is a real answer — every
    /// conversation resolved — and renders as no number rather than a `0`.
    pub unresolved: Option<u32>,
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
        // A draft isn't eligible for approval until it's marked ready, so
        // any verdict GitHub reports on one is dropped here. Keyed on the
        // raw `isDraft` bit rather than the lifecycle because a conflicted
        // draft parses to `PrConflicted` — the lifecycle alone can't tell
        // it from a reviewable PR.
        review: if parsed.is_draft {
            None
        } else {
            parsed
                .review_decision
                .as_deref()
                .and_then(parse_review_decision)
        },
        // Comes from a separate GraphQL probe, not this payload — see
        // [`fetch_unresolved_threads`].
        unresolved: None,
    })
}

/// Whether the payload marks the PR a draft. Read separately from
/// [`parse_gh_pr_status`] because the lifecycle loses the draft bit when a
/// conflict takes priority, and the review-gate probe must still skip
/// drafts in that state. `false` for unparseable input — the caller has
/// already parsed the same payload successfully by the time this runs.
pub(crate) fn parse_pr_is_draft(stdout: &str) -> bool {
    serde_json::from_str::<GhPrView>(stdout.trim()).is_ok_and(|p| p.is_draft)
}

/// Heuristic: `gh pr view` exits 1 with a stderr line like
/// `no pull requests found for branch "foo"` when the branch has no PR.
/// This is distinct from auth errors, network errors, or "no remote".
pub(crate) fn stderr_means_no_pr(stderr: &str) -> bool {
    stderr.contains("no pull requests found")
}

/// The comma-separated `--json` field list `fetch_pr_status` asks `gh` for.
/// Split out so a test can assert the list stays in sync with what
/// [`parse_gh_pr_status`] reads — a field dropped here parses as absent
/// rather than erroring, which would silently blank an indicator.
pub(crate) fn pr_view_json_fields() -> &'static str {
    "state,isDraft,mergeable,number,url,reviewDecision,baseRefName"
}

/// Fill in the verdict `gh` couldn't compute. `gated` is what
/// [`fetch_requires_approval`] learned about the base branch: `None` when the
/// probe failed and we know nothing.
///
/// Only ever *adds* `ReviewRequired`, and only to a PR that has no verdict
/// and is still open — a verdict GitHub did report always wins, since it
/// reflects reviews actually submitted.
pub(crate) fn apply_review_gate(status: PrStatus, gated: Option<bool>) -> PrStatus {
    if status.review.is_some() || !status.lifecycle.awaits_review() || gated != Some(true) {
        return status;
    }
    PrStatus {
        review: Some(ReviewDecision::ReviewRequired),
        ..status
    }
}

/// The GraphQL query behind the unresolved-thread count. Thread resolution
/// is not in `gh pr view`'s `--json` field set at all — GitHub only exposes
/// it through GraphQL `reviewThreads` — hence a second probe instead of a
/// wider field list. Capped at the first 100 threads: past that the count
/// reads low, which is the cheap failure, and a PR with >100 review threads
/// has louder problems than an indicator.
const UNRESOLVED_THREADS_QUERY: &str = "query($owner:String!,$name:String!,$number:Int!){\
     repository(owner:$owner,name:$name){pullRequest(number:$number){\
     reviewThreads(first:100){nodes{isResolved}}}}}";

/// The argv (after `gh`) that fetches the PR's review threads. `None` when
/// `slug` isn't `owner/name` shaped — nothing sane to ask for. `-F` (not
/// `-f`) for the variables so `number` is typed as an Int, which the query
/// declares; `-f` would send a string and GitHub would reject the call.
pub(crate) fn unresolved_threads_argv(slug: &str, number: u32) -> Option<Vec<String>> {
    let (owner, name) = slug.split_once('/')?;
    Some(vec![
        "api".into(),
        "graphql".into(),
        "-f".into(),
        format!("query={UNRESOLVED_THREADS_QUERY}"),
        "-F".into(),
        format!("owner={owner}"),
        "-F".into(),
        format!("name={name}"),
        "-F".into(),
        format!("number={number}"),
    ])
}

#[derive(Debug, Deserialize)]
struct GhThreadsResponse {
    data: Option<GhThreadsData>,
}
#[derive(Debug, Deserialize)]
struct GhThreadsData {
    repository: Option<GhThreadsRepo>,
}
#[derive(Debug, Deserialize)]
struct GhThreadsRepo {
    #[serde(rename = "pullRequest")]
    pull_request: Option<GhThreadsPr>,
}
#[derive(Debug, Deserialize)]
struct GhThreadsPr {
    #[serde(rename = "reviewThreads")]
    review_threads: GhThreadNodes,
}
#[derive(Debug, Deserialize)]
struct GhThreadNodes {
    #[serde(default)]
    nodes: Vec<GhThreadNode>,
}
#[derive(Debug, Deserialize)]
struct GhThreadNode {
    #[serde(rename = "isResolved", default)]
    is_resolved: bool,
}

/// Count the unresolved threads in a GraphQL `reviewThreads` response.
///
/// `None` — not `Some(0)` — when the body doesn't reach the thread list:
/// GraphQL errors arrive as `{"errors":[...]}` with `data` null or the
/// repository/PR missing, and reading one of those as "all resolved" would
/// erase a real count on a transient failure.
pub(crate) fn parse_unresolved_threads(stdout: &str) -> Option<u32> {
    let parsed: GhThreadsResponse = serde_json::from_str(stdout.trim()).ok()?;
    let threads = parsed.data?.repository?.pull_request?.review_threads;
    Some(threads.nodes.iter().filter(|n| !n.is_resolved).count() as u32)
}

/// The unresolved-thread count for PR `number` on `slug`, or `None` when it
/// couldn't be learned. Not memoised: a thread resolves the moment someone
/// clicks "Resolve conversation", and this rides the same 30s poll cadence
/// as the PR status itself.
async fn fetch_unresolved_threads(worktree: &Path, slug: &str, number: u32) -> Option<u32> {
    let argv = unresolved_threads_argv(slug, number)?;
    let out = Command::new("gh")
        .current_dir(worktree)
        .args(argv)
        .output()
        .await
        .ok()?;
    parse_unresolved_threads(&String::from_utf8_lossy(&out.stdout))
}

/// How long a branch's approval gate is trusted before being re-probed.
///
/// Long, because the answer changes when someone edits a ruleset — a
/// once-a-quarter event — not when a PR moves. Doubles as the retention
/// bound: [`fetch_requires_approval`] drops entries this old when it writes. It matters most for the
/// short-lived refreshers (`wsx waybar refresh-prs`, `wsx menubar refresh`),
/// where the cache is only ever warm within a single sweep: there it
/// collapses one probe per workspace into one per repo.
const REVIEW_GATE_TTL_SECS: i64 = 900;

type ReviewGateEntries = std::collections::HashMap<(String, String), (bool, i64)>;
type ReviewGateCache = std::sync::Mutex<ReviewGateEntries>;

/// Record `gated` for `key`, dropping anything already past the TTL.
///
/// The sweep is here rather than inline so it can be tested: an inverted
/// predicate would evict every *live* entry instead of every stale one,
/// which no behavioural test would catch — the probe would simply stop
/// memoising and go back to one call per workspace, silently.
fn store_review_gate(cache: &mut ReviewGateEntries, key: (String, String), gated: bool, now: i64) {
    cache.retain(|_, (_, at)| now.saturating_sub(*at) < REVIEW_GATE_TTL_SECS);
    cache.insert(key, (gated, now));
}

fn review_gate_cache() -> &'static ReviewGateCache {
    static CACHE: std::sync::OnceLock<ReviewGateCache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// Whether merging into `base` on `slug` requires an approving review, per
/// the repo's rulesets. `None` when we couldn't find out — no `gh`, no auth,
/// rate-limited, network down — so callers leave the verdict untouched
/// rather than inventing one.
///
/// Memoised per `(slug, base)` for [`REVIEW_GATE_TTL_SECS`]. Only failures
/// are re-probed immediately; a known answer is reused.
async fn fetch_requires_approval(worktree: &Path, slug: &str, base: &str) -> Option<bool> {
    let key = (slug.to_string(), base.to_string());
    let now = crate::desktop::rows::unix_now();
    if let Ok(cache) = review_gate_cache().lock() {
        if let Some((gated, at)) = cache.get(&key) {
            if now.saturating_sub(*at) < REVIEW_GATE_TTL_SECS {
                return Some(*gated);
            }
        }
    }

    let out = Command::new("gh")
        .current_dir(worktree)
        .args(branch_rules_argv(slug, base))
        .output()
        .await
        .ok()?;
    // A non-zero exit is an error body, which `parse_requires_approval`
    // reads as `None` anyway; parse regardless so a 200 that somehow exits
    // non-zero still counts.
    let gated = parse_requires_approval(&String::from_utf8_lossy(&out.stdout))?;

    if let Ok(mut cache) = review_gate_cache().lock() {
        store_review_gate(&mut cache, key, gated, now);
    }
    Some(gated)
}

pub async fn fetch_pr_status(worktree: &Path, branch: &str) -> Result<Option<PrStatus>> {
    let out = Command::new("gh")
        .current_dir(worktree)
        .args(["pr", "view", branch, "--json", pr_view_json_fields()])
        .output()
        .await;

    let out = match out {
        Ok(o) => o,
        // gh not installed, not on PATH, permission error, etc. — degrade.
        Err(_) => return Ok(None),
    };

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout);
        let Some(status) = parse_gh_pr_status(&stdout) else {
            return Ok(None);
        };
        let slug = status.url.as_deref().and_then(repo_slug_from_pr_url);
        // Nothing to fill in unless GitHub left the verdict empty on a PR
        // that's still open. Checked before the probe so the common cases —
        // an already-answered verdict, a merged PR — cost no extra call.
        // The draft bit is checked on the raw payload: a conflicted draft's
        // lifecycle is `PrConflicted`, which awaits review, but the draft
        // itself is not eligible for approval and must not be gated.
        let status = if status.review.is_some()
            || !status.lifecycle.awaits_review()
            || parse_pr_is_draft(&stdout)
        {
            status
        } else {
            let gated = match (slug.as_deref(), parse_pr_base_ref(&stdout)) {
                (Some(slug), Some(base)) => fetch_requires_approval(worktree, slug, &base).await,
                // No URL or no base ref means no way to address the probe.
                _ => None,
            };
            apply_review_gate(status, gated)
        };
        // The unresolved-thread count only accompanies a verdict on a PR
        // that's still open — exactly when the review mark renders — so a
        // merged PR's lingering verdict or an ungated repo costs no extra
        // call and carries no number.
        let status = match (status.number, slug.as_deref()) {
            (Some(n), Some(slug))
                if status.review.is_some() && status.lifecycle.awaits_review() =>
            {
                PrStatus {
                    unresolved: fetch_unresolved_threads(worktree, slug, n).await,
                    ..status
                }
            }
            _ => status,
        };
        return Ok(Some(status));
    }

    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr_means_no_pr(&stderr) {
        return Ok(Some(PrStatus {
            lifecycle: BranchLifecycle::NoPr,
            number: None,
            url: None,
            review: None,
            unresolved: None,
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
    fn parses_each_review_decision() {
        for (raw, want) in [
            ("APPROVED", ReviewDecision::Approved),
            ("CHANGES_REQUESTED", ReviewDecision::ChangesRequested),
            ("REVIEW_REQUIRED", ReviewDecision::ReviewRequired),
        ] {
            let json = format!(
                r#"{{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":5,"reviewDecision":"{raw}"}}"#
            );
            assert_eq!(
                parse_gh_pr_status(&json).unwrap().review,
                Some(want),
                "reviewDecision {raw}"
            );
        }
    }

    #[test]
    fn parses_payloads_captured_from_real_gh() {
        // Verbatim `gh pr view --json state,isDraft,mergeable,number,url,\
        // reviewDecision` output, so a change in gh's field set or its
        // rendering of an absent verdict is caught here rather than by a
        // blank indicator on someone's dashboard.
        let approved = r#"{"isDraft":false,"mergeable":"UNKNOWN","number":14203,"reviewDecision":"APPROVED","state":"MERGED","url":"https://github.com/cli/cli/pull/14203"}"#;
        let s = parse_gh_pr_status(approved).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrMerged);
        assert_eq!(s.number, Some(14203));
        assert_eq!(s.review, Some(ReviewDecision::Approved));

        // A repo with no approval gate: gh renders the null verdict as "".
        let ungated = r#"{"isDraft":false,"mergeable":"UNKNOWN","number":286,"reviewDecision":"","state":"MERGED","url":"https://github.com/bakedbean/workspacex/pull/286"}"#;
        assert_eq!(parse_gh_pr_status(ungated).unwrap().review, None);
    }

    #[test]
    fn a_draft_verdict_is_dropped_at_parse() {
        // GitHub reports reviewDecision on drafts too, but a draft isn't
        // eligible for approval until it's marked ready — the verdict must
        // never reach the cache.
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"MERGEABLE","number":5,"reviewDecision":"REVIEW_REQUIRED"}"#;
        let s = parse_gh_pr_status(json).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrDraft);
        assert_eq!(s.review, None);
    }

    #[test]
    fn a_conflicted_draft_parses_without_a_verdict() {
        // Conflict wins the lifecycle (see `conflict_overrides_draft`), so
        // the lifecycle alone can't suppress the mark — the raw draft bit
        // has to. Without this, a conflicted draft renders as reviewable.
        let json = r#"{"state":"OPEN","isDraft":true,"mergeable":"CONFLICTING","number":5,"reviewDecision":"APPROVED"}"#;
        let s = parse_gh_pr_status(json).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrConflicted);
        assert_eq!(s.review, None);
    }

    #[test]
    fn draft_bit_is_read_independently_of_lifecycle() {
        // The review-gate probe skips drafts via this helper because the
        // conflicted-draft lifecycle claims to await review.
        let conflicted_draft =
            r#"{"state":"OPEN","isDraft":true,"mergeable":"CONFLICTING","number":5}"#;
        assert!(parse_pr_is_draft(conflicted_draft));
        let open = r#"{"state":"OPEN","isDraft":false,"number":5}"#;
        assert!(!parse_pr_is_draft(open));
        // Unparseable input degrades to "not a draft" — by the time the
        // fetch consults this, the same payload already parsed successfully.
        assert!(!parse_pr_is_draft("not json"));
    }

    #[test]
    fn review_decision_is_none_when_no_review_gate() {
        // gh renders GraphQL's null reviewDecision as "" — the repo requires
        // no approval and none was submitted. Distinct from REVIEW_REQUIRED.
        let json = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":5,"reviewDecision":""}"#;
        assert_eq!(parse_gh_pr_status(json).unwrap().review, None);
    }

    #[test]
    fn review_decision_is_none_when_field_absent_or_unknown() {
        let absent = r#"{"state":"OPEN","isDraft":false,"mergeable":"MERGEABLE","number":5}"#;
        assert_eq!(parse_gh_pr_status(absent).unwrap().review, None);
        // A value GitHub might add later must degrade, not fail the parse.
        let unknown = r#"{"state":"OPEN","isDraft":false,"number":5,"reviewDecision":"WAT"}"#;
        let s = parse_gh_pr_status(unknown).unwrap();
        assert_eq!(s.lifecycle, BranchLifecycle::PrOpen);
        assert_eq!(s.review, None);
    }

    #[test]
    fn fetch_argv_requests_review_decision() {
        assert!(
            pr_view_json_fields().contains("reviewDecision"),
            "gh --json field list must ask for reviewDecision"
        );
        assert!(
            pr_view_json_fields().contains("baseRefName"),
            "gh --json field list must ask for baseRefName — the rules probe \
             addresses the gate by base branch"
        );
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

    /// Verbatim from `gh api repos/{owner}/{repo}/rules/branches/main` on a
    /// repo whose approval gate lives in a ruleset — the exact case that
    /// leaves `reviewDecision` empty. Trimmed to the rules that matter plus
    /// one that doesn't, to prove the others are skipped rather than
    /// mistaken for a gate.
    const RULESET_WITH_APPROVAL: &str = r#"[
        {"type":"deletion","ruleset_id":1,"parameters":null},
        {"type":"pull_request","ruleset_id":1,"parameters":{
            "allowed_merge_methods":["squash"],
            "dismiss_stale_reviews_on_push":false,
            "require_code_owner_review":false,
            "require_last_push_approval":false,
            "required_approving_review_count":1,
            "required_review_thread_resolution":false,
            "required_reviewers":[{"file_patterns":["*"],"minimum_approvals":1,
                "reviewer":{"id":11453206,"type":"Team"}}]}},
        {"type":"non_fast_forward","ruleset_id":1,"parameters":null}
    ]"#;

    #[test]
    fn ruleset_requiring_an_approval_is_a_gate() {
        assert_eq!(parse_requires_approval(RULESET_WITH_APPROVAL), Some(true));
    }

    /// The control case: a repo with no rules on the branch at all answers
    /// with an empty array, and must read as ungated so nothing renders.
    #[test]
    fn no_rules_is_no_gate() {
        assert_eq!(parse_requires_approval("[]"), Some(false));
    }

    /// A ruleset can require a PR without requiring anyone to approve it.
    /// That's a gate on merging directly, not on review, so it must not
    /// light up the indicator.
    #[test]
    fn pull_request_rule_without_approvals_is_no_gate() {
        let json = r#"[{"type":"pull_request","parameters":{
            "required_approving_review_count":0,
            "required_reviewers":[]}}]"#;
        assert_eq!(parse_requires_approval(json), Some(false));
    }

    /// The count gates independently of `required_reviewers`, which is the
    /// commoner ruleset shape: "1 approval from anyone" names no team. The
    /// two arms are tested apart because the real-world fixture above sets
    /// both, so it can't tell which one is doing the work.
    #[test]
    fn approval_count_alone_is_a_gate() {
        let json = r#"[{"type":"pull_request","parameters":{
            "required_approving_review_count":1,
            "required_reviewers":[]}}]"#;
        assert_eq!(parse_requires_approval(json), Some(true));
    }

    /// `required_reviewers` gates independently of the repo-wide count: a
    /// ruleset naming a team with `minimum_approvals: 1` still waits on an
    /// approval even when the count field is 0.
    #[test]
    fn required_reviewers_alone_is_a_gate() {
        let json = r#"[{"type":"pull_request","parameters":{
            "required_approving_review_count":0,
            "required_reviewers":[{"file_patterns":["*"],"minimum_approvals":1}]}}]"#;
        assert_eq!(parse_requires_approval(json), Some(true));
    }

    /// Rules whose parameters we don't model must not derail the parse — a
    /// new rule type shipping in a ruleset can't be allowed to blank the
    /// verdict for every PR in the repo.
    #[test]
    fn unmodelled_rule_types_are_ignored_not_fatal() {
        let json = r#"[{"type":"some_future_rule","parameters":{"wat":[1,2,3]}}]"#;
        assert_eq!(parse_requires_approval(json), Some(false));
    }

    /// An error body is an object, not an array. It must read as "unknown"
    /// so the caller leaves the verdict alone rather than claiming the repo
    /// has no gate.
    #[test]
    fn an_error_body_is_unknown_not_ungated() {
        assert_eq!(parse_requires_approval(r#"{"message":"Not Found"}"#), None);
        assert_eq!(parse_requires_approval(""), None);
    }

    fn pr(lifecycle: BranchLifecycle, review: Option<ReviewDecision>) -> PrStatus {
        PrStatus {
            lifecycle,
            number: Some(1),
            url: Some("https://github.com/o/r/pull/1".into()),
            review,
            unresolved: None,
        }
    }

    /// The bug this whole path exists for: an open PR in a ruleset-gated
    /// repo, where GitHub reports no verdict at all.
    #[test]
    fn a_gated_open_pr_with_no_verdict_becomes_review_required() {
        let got = apply_review_gate(pr(BranchLifecycle::PrOpen, None), Some(true));
        assert_eq!(got.review, Some(ReviewDecision::ReviewRequired));
    }

    /// Conflicted PRs are still open and still reviewable, so they get the
    /// mark too — matching what `awaits_review` renders.
    #[test]
    fn gating_applies_to_every_still_open_lifecycle() {
        for lc in [BranchLifecycle::PrOpen, BranchLifecycle::PrConflicted] {
            assert_eq!(
                apply_review_gate(pr(lc, None), Some(true)).review,
                Some(ReviewDecision::ReviewRequired),
                "lifecycle {lc:?}"
            );
        }
    }

    /// A draft isn't eligible for approval until it's marked ready for
    /// review, so a gated repo's draft must not sprout a "needs review" mark.
    #[test]
    fn gating_skips_draft_prs() {
        assert_eq!(
            apply_review_gate(pr(BranchLifecycle::PrDraft, None), Some(true)).review,
            None
        );
    }

    /// A merged PR in a gated repo must not sprout a "needs review" mark —
    /// it's done, and the gate says nothing about it.
    #[test]
    fn gating_never_marks_a_finished_pr() {
        for lc in [
            BranchLifecycle::PrMerged,
            BranchLifecycle::PrClosed,
            BranchLifecycle::NoPr,
        ] {
            assert_eq!(
                apply_review_gate(pr(lc, None), Some(true)).review,
                None,
                "lifecycle {lc:?}"
            );
        }
    }

    /// A verdict GitHub did report reflects reviews actually submitted, so
    /// it always beats the gate — an approved PR stays approved even though
    /// its repo requires approval.
    #[test]
    fn a_reported_verdict_beats_the_gate() {
        for d in [
            ReviewDecision::Approved,
            ReviewDecision::ChangesRequested,
            ReviewDecision::ReviewRequired,
        ] {
            assert_eq!(
                apply_review_gate(pr(BranchLifecycle::PrOpen, Some(d)), Some(true)).review,
                Some(d),
                "verdict {d:?}"
            );
        }
    }

    /// An ungated repo keeps showing nothing, and a failed probe is treated
    /// the same way: never invent a mark we aren't sure of.
    #[test]
    fn no_gate_and_unknown_gate_both_leave_the_verdict_empty() {
        for gated in [Some(false), None] {
            assert_eq!(
                apply_review_gate(pr(BranchLifecycle::PrOpen, None), gated).review,
                None,
                "gated {gated:?}"
            );
        }
    }

    /// `apply_review_gate` and `lifecycle_shows_review` must agree: a mark
    /// added for a lifecycle the renderer hides is a wasted API call, and a
    /// lifecycle the renderer shows but the fetcher skips is the original bug
    /// coming back for that state.
    #[test]
    fn fetch_and_render_agree_on_which_lifecycles_await_review() {
        for lc in [
            BranchLifecycle::NoPr,
            BranchLifecycle::PrDraft,
            BranchLifecycle::PrOpen,
            BranchLifecycle::PrConflicted,
            BranchLifecycle::PrMerged,
            BranchLifecycle::PrClosed,
        ] {
            assert_eq!(
                lc.awaits_review(),
                crate::ui::theme::lifecycle_shows_review(lc),
                "lifecycle {lc:?}"
            );
        }
    }

    /// Verbatim from `gh api graphql` for the reviewThreads query — three
    /// threads, one still unresolved.
    #[test]
    fn counts_unresolved_threads_from_a_real_payload() {
        let json = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"isResolved":true},{"isResolved":false},{"isResolved":true}]}}}}}"#;
        assert_eq!(parse_unresolved_threads(json), Some(1));
    }

    /// All threads resolved is a real answer, distinct from "couldn't ask".
    #[test]
    fn zero_unresolved_threads_is_some_zero() {
        let json = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"isResolved":true}]}}}}}"#;
        assert_eq!(parse_unresolved_threads(json), Some(0));
        // So is a PR with no review threads at all.
        let empty = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[]}}}}}"#;
        assert_eq!(parse_unresolved_threads(empty), Some(0));
    }

    /// A GraphQL error body must read as "unknown", never as "all resolved" —
    /// a transient failure would otherwise erase a real count.
    #[test]
    fn thread_errors_and_garbage_are_unknown_not_zero() {
        for body in [
            r#"{"data":null,"errors":[{"message":"Could not resolve"}]}"#,
            r#"{"data":{"repository":null}}"#,
            r#"{"data":{"repository":{"pullRequest":null}}}"#,
            r#"{"errors":[{"message":"rate limited"}]}"#,
            "not json",
            "",
        ] {
            assert_eq!(parse_unresolved_threads(body), None, "body {body:?}");
        }
    }

    #[test]
    fn unresolved_threads_argv_types_the_number_as_int() {
        let argv = unresolved_threads_argv("o/r", 42).expect("argv");
        assert_eq!(argv[0], "api");
        assert_eq!(argv[1], "graphql");
        // `-F` (typed), not `-f` (string): the query declares $number as Int.
        assert!(argv.contains(&"-F".to_string()));
        assert!(argv.contains(&"owner=o".to_string()));
        assert!(argv.contains(&"name=r".to_string()));
        assert!(argv.contains(&"number=42".to_string()));
        let query = argv.iter().find(|a| a.starts_with("query=")).expect("query");
        assert!(query.contains("reviewThreads"));
        assert!(query.contains("isResolved"));
    }

    /// A slug that isn't `owner/name` shaped has nothing to query.
    #[test]
    fn unresolved_threads_argv_rejects_a_shapeless_slug() {
        assert_eq!(unresolved_threads_argv("noslash", 1), None);
    }

    fn key(base: &str) -> (String, String) {
        ("o/r".into(), base.into())
    }

    /// Nothing else ever removes an entry, so a base branch that stops
    /// existing — a stacked PR's parent, an archived repo — would otherwise
    /// outlive the process that cached it.
    #[test]
    fn writing_the_cache_drops_entries_past_the_ttl() {
        let now = 1_000_000;
        let mut cache = ReviewGateEntries::new();
        cache.insert(key("gone"), (true, now - REVIEW_GATE_TTL_SECS - 1));
        store_review_gate(&mut cache, key("main"), true, now);
        assert_eq!(
            cache.keys().collect::<Vec<_>>(),
            vec![&key("main")],
            "the stale entry should be gone and the fresh one kept"
        );
    }

    /// The half that a naive sweep gets backwards. Evicting live entries
    /// wouldn't fail anything visible — the probe would just stop memoising
    /// and go back to one `gh` call per workspace.
    #[test]
    fn writing_the_cache_keeps_entries_inside_the_ttl() {
        let now = 1_000_000;
        let mut cache = ReviewGateEntries::new();
        cache.insert(key("still-fresh"), (true, now - REVIEW_GATE_TTL_SECS + 1));
        store_review_gate(&mut cache, key("main"), false, now);
        assert_eq!(cache.len(), 2, "a live entry must survive the sweep");
        assert_eq!(cache.get(&key("main")), Some(&(false, now)));
    }

    #[test]
    fn base_ref_comes_from_the_same_payload() {
        let json = r#"{"state":"OPEN","number":5,"baseRefName":"main"}"#;
        assert_eq!(parse_pr_base_ref(json).as_deref(), Some("main"));
        // Absent or blank is "unknown", which suppresses the probe rather
        // than addressing it at a branch named "".
        assert_eq!(parse_pr_base_ref(r#"{"state":"OPEN"}"#), None);
        assert_eq!(
            parse_pr_base_ref(r#"{"state":"OPEN","baseRefName":""}"#),
            None
        );
    }

    #[test]
    fn slug_comes_from_a_pr_url() {
        assert_eq!(
            repo_slug_from_pr_url("https://github.com/bakedbean/workspacex/pull/288").as_deref(),
            Some("bakedbean/workspacex")
        );
    }

    /// GitHub Enterprise PR URLs carry the same path shape under a different
    /// host, and `gh api` resolves the host from the repo it's run in.
    #[test]
    fn slug_comes_from_an_enterprise_pr_url() {
        assert_eq!(
            repo_slug_from_pr_url("https://git.example.com/o/r/pull/1").as_deref(),
            Some("o/r")
        );
    }

    #[test]
    fn non_pr_urls_have_no_slug() {
        assert_eq!(repo_slug_from_pr_url("https://github.com/o/r"), None);
        assert_eq!(
            repo_slug_from_pr_url("https://github.com/o/r/issues/1"),
            None
        );
        assert_eq!(repo_slug_from_pr_url(""), None);
    }

    /// A base branch with a slash lands in the path as-is; the endpoint
    /// takes the full ref path after `branches/`.
    #[test]
    fn rules_argv_names_the_repo_and_branch() {
        assert_eq!(
            branch_rules_argv("o/r", "release/1.x"),
            vec!["api", "repos/o/r/rules/branches/release/1.x"]
        );
    }

    /// A path that isn't a git repo at all must degrade quietly, not panic —
    /// the dashboard probes every registered repo path on refresh.
    #[test]
    fn non_git_path_is_not_a_github_remote() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert!(!repo_has_github_remote(tmp.path()));
    }
}
