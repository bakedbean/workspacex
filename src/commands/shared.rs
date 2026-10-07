//! `wsx shared list` — the machine-readable inventory of tmux-shared
//! workspaces (with `--all`, every ready workspace) and their agent instances. This is the Phase 2 wire contract a
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
    /// Whether the workspace is tmux-shared on its host. Only `--all` emits
    /// `false` records; a host on an older wsx lists shared workspaces alone
    /// and never sends the key, hence the `true` default.
    #[serde(default = "default_shared")]
    pub shared: bool,
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

fn default_shared() -> bool {
    true
}

/// Build records for every shared workspace, plus — with `include_unshared` —
/// every ready direct one, so a remote picker can offer to share it.
/// `liveness` is injected so tests don't need tmux; production passes
/// `crate::pty::tmux::has_session`.
pub fn shared_list_records(
    store: &Store,
    include_unshared: bool,
    liveness: impl Fn(&str) -> bool,
) -> Result<Vec<SharedWorkspaceRecord>> {
    let mut out = Vec::new();
    for r in crate::data::repo::list(store)? {
        for w in store.workspaces(r.id)? {
            let listed = w.shared
                || (include_unshared && w.state == crate::data::store::WorkspaceState::Ready);
            if !listed {
                continue;
            }
            let mut agents = Vec::new();
            for inst in store.workspace_agents(w.id)? {
                // A direct workspace's agents run outside tmux, so a
                // `session_ref` left over from an earlier share names nothing
                // a peer could attach to.
                let label = inst.label();
                let tmux_session = inst.session_ref.filter(|_| w.shared);
                let alive = tmux_session.as_deref().map(&liveness).unwrap_or(false);
                agents.push(SharedAgentRecord {
                    label,
                    agent: inst.agent.store_value().into(),
                    tmux_session,
                    alive,
                });
            }
            out.push(SharedWorkspaceRecord {
                repo: r.name.clone(),
                workspace: w.name.clone(),
                branch: w.branch.clone(),
                worktree_path: w.worktree_path.to_string_lossy().into_owned(),
                shared: w.shared,
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

/// How long `wsx workspace share --restart` waits at each stage. The dashboard
/// claims a request on its next tick, so `claim` only has to outlast a tick
/// plus a slow refresh; `finish` covers spawning the agents; `live` covers the
/// tmux client creating its session after the spawn returned.
#[derive(Debug, Clone, Copy)]
pub struct ShareWaits {
    pub claim: std::time::Duration,
    pub finish: std::time::Duration,
    pub live: std::time::Duration,
    pub poll: std::time::Duration,
}

impl Default for ShareWaits {
    fn default() -> Self {
        Self {
            claim: std::time::Duration::from_secs(5),
            finish: std::time::Duration::from_secs(30),
            live: std::time::Duration::from_secs(10),
            poll: std::time::Duration::from_millis(150),
        }
    }
}

/// The record for one workspace, as `shared list --all` would report it.
fn workspace_record(
    store: &Store,
    ws: &crate::data::store::Workspace,
    liveness: impl Fn(&str) -> bool,
) -> Result<SharedWorkspaceRecord> {
    shared_list_records(store, true, liveness)?
        .into_iter()
        .find(|r| r.worktree_path == ws.worktree_path.to_string_lossy())
        .ok_or_else(|| crate::error::Error::UserInput(format!("workspace {} not found", ws.name)))
}

/// Every agent has a session name and that session is alive.
fn all_live(rec: &SharedWorkspaceRecord) -> bool {
    rec.shared && !rec.agents.is_empty() && rec.agents.iter().all(|a| a.alive)
}

/// `wsx workspace share <repo> <slug> --restart`: have the dashboard running
/// on this host share `ws` and start its agents inside tmux, then wait until
/// every agent's tmux session is up so a peer can attach right away. Returns
/// the workspace's record with the live session names.
///
/// Only the dashboard can do this — a direct agent is its child process — so
/// with no dashboard running the request is withdrawn and this errors rather
/// than flipping the flag behind a dashboard that may start later. Whether a
/// dashboard is up is learned only from the claim, never from
/// `ipc::any_live_tui`: its socket directory follows `XDG_RUNTIME_DIR` or
/// `TMPDIR`, which an ssh login (the usual caller) often doesn't share with
/// the dashboard's session — on macOS especially — so it would turn a running
/// dashboard away.
pub async fn share_and_restart(
    store: &Store,
    ws: &crate::data::store::Workspace,
    waits: ShareWaits,
    liveness: impl Fn(&str) -> bool,
) -> Result<SharedWorkspaceRecord> {
    use crate::data::share_requests::ShareRequestState;
    use crate::error::Error;

    let rec = workspace_record(store, ws, &liveness)?;
    if all_live(&rec) {
        return Ok(rec);
    }
    let id = store.enqueue_share_request(ws.id)?;
    let start = std::time::Instant::now();
    loop {
        match store.share_request_state(id)? {
            ShareRequestState::Pending if start.elapsed() >= waits.claim => {
                if store.withdraw_share_request(id)? {
                    return Err(Error::UserInput(
                        "no wsx dashboard on this host picked up the request; \
                         start `wsx` here, since only the dashboard can restart its agents"
                            .into(),
                    ));
                }
            }
            ShareRequestState::Pending | ShareRequestState::Claimed => {
                if start.elapsed() >= waits.claim + waits.finish {
                    return Err(Error::UserInput(
                        "the dashboard took too long to restart the workspace's agents; \
                         it may still finish — check `wsx shared list`"
                            .into(),
                    ));
                }
            }
            ShareRequestState::Finished { error } => {
                store.delete_share_request(id)?;
                if let Some(e) = error {
                    return Err(Error::UserInput(format!("sharing failed: {e}")));
                }
                break;
            }
            ShareRequestState::Gone => {
                return Err(Error::UserInput(
                    "the share request disappeared before it finished".into(),
                ));
            }
        }
        tokio::time::sleep(waits.poll).await;
    }
    let live_start = std::time::Instant::now();
    loop {
        let rec = workspace_record(store, ws, &liveness)?;
        if all_live(&rec) {
            return Ok(rec);
        }
        if live_start.elapsed() >= waits.live {
            let down: Vec<&str> = rec
                .agents
                .iter()
                .filter(|a| !a.alive)
                .map(|a| a.label.as_str())
                .collect();
            return Err(Error::UserInput(format!(
                "restarted, but no live tmux session for: {}",
                down.join(", ")
            )));
        }
        tokio::time::sleep(waits.poll).await;
    }
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
            // The basic fetch: only lifecycle/pr_number are consumed here,
            // and the full fetch's follow-up probes (review gate, unresolved
            // threads) could burn the timeout and discard a status `gh pr
            // view` had already answered.
            match tokio::time::timeout(
                PR_ENRICH_TIMEOUT,
                crate::git::forge::fetch_pr_status_basic(&path, &branch),
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
        rec.lifecycle = status.as_ref().map(|s| s.lifecycle);
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

        let records = shared_list_records(&store, false, |n| n == "wsx-r-w").unwrap();

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
    fn include_unshared_lists_ready_direct_workspaces_as_unattachable() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);
        // A stale `session_ref` from an earlier share must not surface on a
        // direct workspace, even if a tmux session by that name is alive.
        let direct = store
            .workspace_agents(
                store
                    .workspaces(crate::data::repo::list(&store).unwrap()[0].id)
                    .unwrap()
                    .iter()
                    .find(|w| w.name == "direct")
                    .unwrap()
                    .id,
            )
            .unwrap();
        store
            .set_instance_session_ref(direct[0].id, "wsx-r-direct")
            .unwrap();

        let records = shared_list_records(&store, true, |_| true).unwrap();

        assert_eq!(records.len(), 2, "{records:?}");
        let d = records.iter().find(|r| r.workspace == "direct").unwrap();
        assert!(!d.shared);
        assert_eq!(d.agents.len(), 1);
        assert!(d.agents[0].tmux_session.is_none());
        assert!(!d.agents[0].alive);
        assert!(records.iter().find(|r| r.workspace == "w").unwrap().shared);
    }

    #[test]
    fn include_unshared_skips_direct_workspaces_that_are_not_ready() {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r3"), "r3", "")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "creating",
                branch: "r3/creating",
                worktree_path: std::path::Path::new("/tmp/r3/creating"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws, WorkspaceState::Pending)
            .unwrap();

        assert!(
            shared_list_records(&store, true, |_| false)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn shared_list_records_marks_missing_session_as_dead() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);

        // liveness closure always returns false: nothing is actually alive.
        let records = shared_list_records(&store, false, |_| false).unwrap();

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

        let records = shared_list_records(&store, false, |_| true).unwrap();
        assert_eq!(records.len(), 1);
        let agent = &records[0].agents[0];
        assert!(agent.tmux_session.is_none());
        assert!(!agent.alive);
    }

    #[test]
    fn json_shape_contains_tmux_session_field() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);

        let records = shared_list_records(&store, false, |n| n == "wsx-r-w").unwrap();
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
        // An older host lists shared workspaces only and omits `shared`.
        assert!(recs[0].shared);
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

    fn quick_waits() -> ShareWaits {
        ShareWaits {
            claim: std::time::Duration::from_millis(50),
            finish: std::time::Duration::from_millis(50),
            live: std::time::Duration::from_millis(50),
            poll: std::time::Duration::from_millis(5),
        }
    }

    fn direct_ws(store: &Store) -> crate::data::store::Workspace {
        let repo = crate::data::repo::list(store).unwrap().remove(0);
        store
            .workspaces(repo.id)
            .unwrap()
            .into_iter()
            .find(|w| w.name == "direct")
            .unwrap()
    }

    #[tokio::test]
    async fn share_and_restart_without_a_dashboard_withdraws_and_errors() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);
        let ws = direct_ws(&store);

        let err = share_and_restart(&store, &ws, quick_waits(), |_| false)
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("no wsx dashboard"), "{err}");
        assert!(
            store
                .claim_share_requests(|_| true)
                .unwrap()
                .claimed
                .is_empty(),
            "the request must be withdrawn, not left for a later dashboard"
        );
        assert!(!store.workspace_by_id(ws.id).unwrap().unwrap().shared);
    }

    #[tokio::test]
    async fn share_and_restart_reports_the_dashboards_error() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);
        let ws = direct_ws(&store);
        // Stand in for the dashboard: the request it will see is the next id.
        let fut = share_and_restart(&store, &ws, quick_waits(), |_| false);
        let finish = async {
            loop {
                if let Some(req) = store.claim_share_requests(|_| true).unwrap().claimed.pop() {
                    store
                        .finish_share_request(req.id, Some("tmux missing"))
                        .unwrap();
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let (res, ()) = tokio::join!(fut, finish);
        assert!(res.unwrap_err().to_string().contains("tmux missing"));
    }

    #[tokio::test]
    async fn share_and_restart_returns_live_sessions_once_the_dashboard_is_done() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);
        let ws = direct_ws(&store);
        let fut = share_and_restart(&store, &ws, quick_waits(), |n| n == "wsx-r-direct");
        let dashboard = async {
            loop {
                if let Some(req) = store.claim_share_requests(|_| true).unwrap().claimed.pop() {
                    store.set_workspace_shared(ws.id, true).unwrap();
                    let inst = &store.workspace_agents(ws.id).unwrap()[0];
                    store
                        .set_instance_session_ref(inst.id, "wsx-r-direct")
                        .unwrap();
                    store.finish_share_request(req.id, None).unwrap();
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let (res, ()) = tokio::join!(fut, dashboard);
        let rec = res.unwrap();
        assert!(rec.shared);
        assert_eq!(rec.agents[0].tmux_session.as_deref(), Some("wsx-r-direct"));
        assert!(rec.agents[0].alive);
    }

    #[tokio::test]
    async fn share_and_restart_skips_the_dashboard_when_already_live() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);
        let repo = crate::data::repo::list(&store).unwrap().remove(0);
        let ws = store
            .workspaces(repo.id)
            .unwrap()
            .into_iter()
            .find(|w| w.name == "w")
            .unwrap();
        // No dashboard at all, yet it succeeds: nothing needs restarting.
        let rec = share_and_restart(&store, &ws, quick_waits(), |n| n == "wsx-r-w")
            .await
            .unwrap();
        assert!(rec.agents[0].alive);
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
            shared: true,
            agents: vec![],
            lifecycle: None,
            pr_number: None,
        }];
        enrich_with_pr_status(&mut records).await;
        assert_eq!(records[0].lifecycle, None);
        assert_eq!(records[0].pr_number, None);
    }
}
