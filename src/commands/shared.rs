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
            });
        }
    }
    Ok(out)
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
        let recs: Vec<SharedWorkspaceRecord> = serde_json::from_str(json).unwrap();
        assert_eq!(recs[0].workspace, "w");
        assert_eq!(recs[0].agents[0].tmux_session.as_deref(), Some("wsx-r-w"));
        assert!(recs[0].agents[0].alive);
        // and what we serialize, we can deserialize
        let back: Vec<SharedWorkspaceRecord> =
            serde_json::from_str(&serde_json::to_string(&recs).unwrap()).unwrap();
        assert_eq!(back[0].agents[0].label, "claude");
    }
}
