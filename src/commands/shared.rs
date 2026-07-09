//! `wsx shared list` — the machine-readable inventory of tmux-shared
//! workspaces and their agent instances. This is the Phase 2 wire contract a
//! future remote-browsing phase will consume over ssh, so field names on
//! `SharedAgentRecord`/`SharedWorkspaceRecord` are additive-only: don't
//! rename or remove without a version bump.

use crate::data::store::Store;
use crate::error::Result;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SharedAgentRecord {
    pub label: String,
    pub agent: String,
    pub tmux_session: Option<String>,
    pub alive: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SharedWorkspaceRecord {
    pub repo: String,
    pub workspace: String,
    pub branch: String,
    pub worktree_path: String,
    pub agents: Vec<SharedAgentRecord>,
    /// The workspace branch's PR lifecycle, computed on the host that owns the
    /// worktree (see `enrich_with_pr_status`) so the remote picker can color
    /// rows the same way the dashboard does. `#[serde(default)]` keeps the
    /// wire contract additive: a host on an older wsx that never emits this
    /// field decodes as `None` (unknown → drawn dim, no lifecycle color).
    /// `shared_list_records` itself leaves it `None`; enrichment is a separate,
    /// best-effort step.
    #[serde(default)]
    pub lifecycle: Option<crate::git::forge::BranchLifecycle>,
    /// The workspace branch's PR number, populated alongside `lifecycle` by
    /// `enrich_with_pr_status`. `#[serde(default)]` keeps the wire contract
    /// additive (older hosts decode it as `None`). `None` when there is no PR,
    /// when `gh` couldn't answer, or on a legacy host — the picker just omits
    /// the `#<num>` prefix in those cases.
    #[serde(default)]
    pub pr_number: Option<u32>,
}

/// Build records for every shared workspace. `liveness` is injected so tests
/// don't need tmux; production passes `crate::pty::tmux::has_session`.
pub fn shared_list_records(
    store: &Store,
    liveness: impl Fn(&str) -> bool,
) -> Result<Vec<SharedWorkspaceRecord>> {
    let mut out = Vec::new();
    for r in crate::data::repo::list(store)? {
        for w in store.workspaces(r.id)? {
            if !w.shared {
                continue;
            }
            let mut agents = Vec::new();
            for inst in store.workspace_agents(w.id)? {
                let alive = inst.session_ref.as_deref().map(&liveness).unwrap_or(false);
                agents.push(SharedAgentRecord {
                    label: inst.label(),
                    agent: inst.agent.store_value().into(),
                    tmux_session: inst.session_ref,
                    alive,
                });
            }
            out.push(SharedWorkspaceRecord {
                repo: r.name.clone(),
                workspace: w.name.clone(),
                branch: w.branch.clone(),
                worktree_path: w.worktree_path.to_string_lossy().into_owned(),
                agents,
                // Pure DB pass leaves PR status unknown; `enrich_with_pr_status`
                // fills lifecycle + number in from `gh` before the records go
                // over the wire.
                lifecycle: None,
                pr_number: None,
            });
        }
    }
    Ok(out)
}

/// Max `gh pr view` invocations in flight at once during enrichment. Bounds the
/// process/network fan-out on a host sharing many workspaces instead of
/// spawning one `gh` per workspace simultaneously.
const PR_ENRICH_CONCURRENCY: usize = 8;

/// Per-record ceiling on the `gh pr view` call. `gh` has no timeout of its own,
/// so a single network-stalled invocation would otherwise hang the whole
/// `wsx shared list --json` — and with it the remote picker's loading modal —
/// indefinitely. On timeout the record simply stays `None`: best-effort, and
/// the list still renders.
const PR_ENRICH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Populate each record's `lifecycle` and `pr_number` by asking `gh` for the
/// branch's PR status, with bounded concurrency and a per-record timeout.
/// Best-effort and degrades gracefully rather than erroring or hanging:
/// - a branch with **no PR** resolves to `Some(BranchLifecycle::NoPr)` with
///   `pr_number = None` (dim, no `#<num>`);
/// - a `gh` failure, timeout, or missing/unauthenticated `gh` leaves both
///   `lifecycle` and `pr_number` as `None` (also dim, no `#<num>`).
///
/// So `None` lifecycle means "couldn't determine", distinct from a known
/// `NoPr`; both render the same in the picker. Runs on the host that owns the
/// worktrees — i.e. inside the remote's `wsx shared list --json` — since PR
/// status is a property of the branch on the shared forge.
pub async fn enrich_with_pr_status(records: &mut [SharedWorkspaceRecord]) {
    use futures::StreamExt;

    let fetches = records.iter().map(|rec| {
        let path = std::path::PathBuf::from(&rec.worktree_path);
        let branch = rec.branch.clone();
        async move {
            match tokio::time::timeout(
                PR_ENRICH_TIMEOUT,
                crate::git::forge::fetch_pr_status(&path, &branch),
            )
            .await
            {
                Ok(Ok(Some(s))) => Some(s),
                // gh error, no resolvable PR, or timed out → best-effort None.
                _ => None,
            }
        }
    });
    // `buffered` bounds concurrency to PR_ENRICH_CONCURRENCY and preserves input
    // order, so the collected statuses line up with `records` by index.
    let statuses: Vec<Option<crate::git::forge::PrStatus>> = futures::stream::iter(fetches)
        .buffered(PR_ENRICH_CONCURRENCY)
        .collect()
        .await;
    for (rec, status) in records.iter_mut().zip(statuses) {
        rec.lifecycle = status.map(|s| s.lifecycle);
        rec.pr_number = status.and_then(|s| s.number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{NewWorkspace, WorkspaceState};
    use crate::pty::session::AgentKind;

    /// Seeds one shared workspace (with a primary agent instance whose
    /// session_ref is `"wsx-r-w"`) and one direct (non-shared) workspace in
    /// the same repo. Returns the store plus the shared workspace's id.
    fn seed(store: &Store) {
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "")
            .unwrap();

        let shared_ws = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w",
                branch: "r/w",
                worktree_path: std::path::Path::new("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(shared_ws, WorkspaceState::Ready)
            .unwrap();
        let primary = store
            .add_primary_agent(shared_ws, AgentKind::Claude, 0)
            .unwrap();
        store
            .set_instance_session_ref(primary.id, "wsx-r-w")
            .unwrap();

        let direct_ws = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "direct",
                branch: "r/direct",
                worktree_path: std::path::Path::new("/tmp/r/direct"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(direct_ws, WorkspaceState::Ready)
            .unwrap();
        store
            .add_primary_agent(direct_ws, AgentKind::Claude, 0)
            .unwrap();
    }

    #[test]
    fn shared_list_records_includes_only_shared_workspaces() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);

        let records = shared_list_records(&store, |n| n == "wsx-r-w").unwrap();

        assert_eq!(
            records.len(),
            1,
            "expected only the shared workspace: {records:?}"
        );
        let rec = &records[0];
        assert_eq!(rec.repo, "r");
        assert_eq!(rec.workspace, "w");
        assert_eq!(rec.branch, "r/w");
        assert_eq!(rec.agents.len(), 1);
        let agent = &rec.agents[0];
        assert_eq!(agent.label, "claude");
        assert_eq!(agent.agent, "claude");
        assert_eq!(agent.tmux_session.as_deref(), Some("wsx-r-w"));
        assert!(agent.alive);
    }

    #[test]
    fn shared_list_records_marks_missing_session_as_dead() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);

        // liveness closure always returns false: nothing is actually alive.
        let records = shared_list_records(&store, |_| false).unwrap();

        assert_eq!(records.len(), 1);
        assert!(!records[0].agents[0].alive);
    }

    #[test]
    fn shared_list_records_none_session_ref_is_dead_with_null_session() {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r2"), "r2", "")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "w2",
                branch: "r2/w2",
                worktree_path: std::path::Path::new("/tmp/r2/w2"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .set_workspace_state(ws, WorkspaceState::Ready)
            .unwrap();
        // No session_ref set: instance never attached to tmux.
        store.add_primary_agent(ws, AgentKind::Claude, 0).unwrap();

        let records = shared_list_records(&store, |_| true).unwrap();
        assert_eq!(records.len(), 1);
        let agent = &records[0].agents[0];
        assert!(agent.tmux_session.is_none());
        assert!(!agent.alive);
    }

    #[test]
    fn json_shape_contains_tmux_session_field() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);

        let records = shared_list_records(&store, |n| n == "wsx-r-w").unwrap();
        let json = serde_json::to_string(&records).unwrap();
        assert!(
            json.contains("\"tmux_session\":\"wsx-r-w\""),
            "json was: {json}"
        );
    }

    #[test]
    fn records_roundtrip_serde_and_tolerate_unknown_fields() {
        let json = r#"[{
        "repo": "r", "workspace": "w", "branch": "wsx/w",
        "worktree_path": "/tmp/r/w",
        "future_field": "ignored",
        "agents": [{"label": "claude", "agent": "claude",
                    "tmux_session": "wsx-r-w", "alive": true,
                    "another_future_field": 7}]
    }]"#;
        let mut recs: Vec<SharedWorkspaceRecord> = serde_json::from_str(json).unwrap();
        assert_eq!(recs[0].workspace, "w");
        assert_eq!(recs[0].agents[0].tmux_session.as_deref(), Some("wsx-r-w"));
        assert!(recs[0].agents[0].alive);
        // A payload from an older host with no `lifecycle`/`pr_number` keys
        // decodes as `None` (unknown → uncolored, no #num), via `#[serde(default)]`.
        assert_eq!(recs[0].lifecycle, None);
        assert_eq!(recs[0].pr_number, None);

        // A populated lifecycle + PR number survive a serialize → deserialize
        // round-trip, so what the remote computes reaches the local picker intact.
        recs[0].lifecycle = Some(crate::git::forge::BranchLifecycle::PrOpen);
        recs[0].pr_number = Some(2087);
        let back: Vec<SharedWorkspaceRecord> =
            serde_json::from_str(&serde_json::to_string(&recs).unwrap()).unwrap();
        assert_eq!(back[0].agents[0].label, "claude");
        assert_eq!(
            back[0].lifecycle,
            Some(crate::git::forge::BranchLifecycle::PrOpen)
        );
        assert_eq!(back[0].pr_number, Some(2087));
    }

    #[tokio::test]
    async fn enrich_is_best_effort_on_non_git_paths() {
        // Worktree paths that aren't git repos make `gh` fail; enrichment must
        // leave those records `None` rather than error, so a picker still shows
        // the (uncolored) list.
        let tmp = tempfile::TempDir::new().unwrap();
        let mut records = vec![SharedWorkspaceRecord {
            repo: "r".into(),
            workspace: "w".into(),
            branch: "main".into(),
            worktree_path: tmp.path().to_string_lossy().into_owned(),
            agents: vec![],
            lifecycle: None,
            pr_number: None,
        }];
        enrich_with_pr_status(&mut records).await;
        assert_eq!(records[0].lifecycle, None);
        assert_eq!(records[0].pr_number, None);
    }
}
