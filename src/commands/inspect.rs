//! Read-only views of a workspace for `wsx status show`, `wsx recap show`,
//! `wsx agent list` and `wsx workspace list`: one record type per view, built
//! from the store, rendered either as text or (with `--json`) serialized.
//!
//! The `--json` shapes are a contract agents parse, so fields are
//! additive-only: don't rename or remove one. Timestamps are Unix epoch
//! milliseconds, as stored.

use crate::data::agents::AgentInstance;
use crate::data::store::{
    AgentInstanceId, ReportedStatus, Store, Workspace, WorkspaceId, WorkspaceRecap,
};
use crate::error::Result;
use std::collections::HashMap;

/// One reported status: a workspace's derived row or a single agent's.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StatusRecord {
    pub state: String,
    pub message: Option<String>,
    pub source: String,
    pub reported_at: i64,
}

impl From<&ReportedStatus> for StatusRecord {
    fn from(s: &ReportedStatus) -> Self {
        StatusRecord {
            state: s.state.as_str().to_string(),
            message: s.message.clone(),
            source: s.source.clone(),
            reported_at: s.reported_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentRecord {
    pub id: i64,
    /// The address `wsx agent send` takes (`claude`, `claude#2`).
    pub label: String,
    pub kind: String,
    pub primary: bool,
    /// This agent's own last push; `None` when it never reported.
    pub status: Option<StatusRecord>,
}

/// `wsx status show`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StatusView {
    pub repo: String,
    pub slug: String,
    /// The derived workspace-level status the dashboard shows.
    pub status: Option<StatusRecord>,
    pub agents: Vec<AgentRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecapRecord {
    pub goal: Option<String>,
    pub state: Option<String>,
    pub next: Option<String>,
    pub goal_short: Option<String>,
    pub state_short: Option<String>,
    pub next_short: Option<String>,
    pub updated_at: i64,
}

impl From<&WorkspaceRecap> for RecapRecord {
    fn from(r: &WorkspaceRecap) -> Self {
        RecapRecord {
            goal: r.goal.clone(),
            state: r.state.clone(),
            next: r.next.clone(),
            goal_short: r.goal_short.clone(),
            state_short: r.state_short.clone(),
            next_short: r.next_short.clone(),
            updated_at: r.updated_at,
        }
    }
}

/// `wsx recap show`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecapView {
    pub repo: String,
    pub slug: String,
    pub recap: Option<RecapRecord>,
}

/// The dashboard row's recap: the short forms only.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RecapShortRecord {
    pub goal_short: Option<String>,
    pub state_short: Option<String>,
    pub next_short: Option<String>,
}

/// PR state as last cached by the dashboard's background poll or `wsx
/// waybar refresh-prs`. Never fetched live: `workspace list` makes no
/// network calls, so this can be stale or absent.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PrRecord {
    /// `no_pr`, `draft`, `open`, `conflicted`, `merged` or `closed`.
    pub state: String,
    pub number: Option<u32>,
    pub url: Option<String>,
    /// `approved`, `changes_requested`, `review_required`, or `None`.
    pub review: Option<String>,
    pub unresolved: Option<u32>,
    pub fetched_at: Option<i64>,
}

/// One `wsx workspace list --json` entry: everything the dashboard row shows.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WorkspaceRecord {
    pub repo: String,
    pub slug: String,
    pub branch: String,
    pub path: String,
    pub status: Option<StatusRecord>,
    pub recap: Option<RecapShortRecord>,
    pub agents: Vec<AgentRecord>,
    pub pr: Option<PrRecord>,
}

fn repo_name(store: &Store, ws: &Workspace) -> Result<String> {
    Ok(store
        .repos()?
        .into_iter()
        .find(|r| r.id == ws.repo_id)
        .map(|r| r.name)
        .unwrap_or_else(|| "?".to_string()))
}

fn agent_records(
    agents: &[AgentInstance],
    statuses: &HashMap<AgentInstanceId, ReportedStatus>,
) -> Vec<AgentRecord> {
    agents
        .iter()
        .map(|a| AgentRecord {
            id: a.id.0,
            label: a.label(),
            kind: a.agent.store_value().to_string(),
            primary: a.is_primary,
            status: statuses.get(&a.id).map(StatusRecord::from),
        })
        .collect()
}

/// The agents attached to `ws`, primary first, each with its own status.
pub fn agents(store: &Store, ws: WorkspaceId) -> Result<Vec<AgentRecord>> {
    Ok(agent_records(
        &store.workspace_agents(ws)?,
        &store.agent_statuses(ws)?,
    ))
}

pub fn status_view(store: &Store, ws: &Workspace) -> Result<StatusView> {
    Ok(StatusView {
        repo: repo_name(store, ws)?,
        slug: ws.name.clone(),
        status: store
            .workspace_status(ws.id)?
            .as_ref()
            .map(StatusRecord::from),
        agents: agents(store, ws.id)?,
    })
}

pub fn recap_view(store: &Store, ws: &Workspace) -> Result<RecapView> {
    Ok(RecapView {
        repo: repo_name(store, ws)?,
        slug: ws.name.clone(),
        recap: store
            .workspace_recap(ws.id)?
            .as_ref()
            .map(RecapRecord::from),
    })
}

/// Every workspace in `repos` order, from one pass over each table rather
/// than a query per workspace.
pub fn workspace_records(
    store: &Store,
    repos: &[crate::data::store::Repo],
) -> Result<Vec<WorkspaceRecord>> {
    let statuses = store.all_workspace_status()?;
    let agent_statuses = store.all_agent_statuses()?;
    let recaps = store.all_workspace_recaps()?;
    let agents = store.all_workspace_agents()?;
    let scm = store.all_scm_cache()?;
    let mut out = Vec::new();
    for r in repos {
        for w in store.workspaces(r.id)? {
            out.push(WorkspaceRecord {
                repo: r.name.clone(),
                slug: w.name.clone(),
                branch: w.branch.clone(),
                path: w.worktree_path.to_string_lossy().into_owned(),
                status: statuses.get(&w.id).map(StatusRecord::from),
                recap: recaps.get(&w.id).map(|r| RecapShortRecord {
                    goal_short: r.goal_short.clone(),
                    state_short: r.state_short.clone(),
                    next_short: r.next_short.clone(),
                }),
                agents: agent_records(
                    agents.get(&w.id).map(Vec::as_slice).unwrap_or_default(),
                    &agent_statuses,
                ),
                pr: scm.get(&w.id).and_then(|c| {
                    Some(PrRecord {
                        state: crate::data::scm_cache::lifecycle_to_str(c.pr_lifecycle?)
                            .to_string(),
                        number: c.pr_number,
                        url: c.pr_url.clone(),
                        review: c
                            .pr_review
                            .map(|d| crate::data::scm_cache::review_to_str(d).to_string()),
                        unresolved: c.pr_unresolved,
                        fetched_at: c.fetched_at,
                    })
                }),
            });
        }
    }
    Ok(out)
}

/// `working — "running tests" (model, 3m ago)`, or `-` when nothing was
/// reported. Shared by `status show`, `agent list` and the context digest.
pub fn format_status(s: Option<&StatusRecord>, now_ms: i64) -> String {
    let Some(s) = s else {
        return "-".to_string();
    };
    let age = crate::commands::context::format_age(now_ms, s.reported_at);
    let source = if s.source.trim().is_empty() {
        "-"
    } else {
        s.source.as_str()
    };
    match s.message.as_deref().filter(|m| !m.trim().is_empty()) {
        Some(m) => format!("{} — \"{}\" ({}, {})", s.state, m, source, age),
        None => format!("{} ({}, {})", s.state, source, age),
    }
}

fn agent_name(a: &AgentRecord) -> String {
    if a.primary {
        format!("{} (primary)", a.label)
    } else {
        a.label.clone()
    }
}

pub fn render_status(v: &StatusView, now_ms: i64) -> String {
    let mut out = format!("status: {}\n", format_status(v.status.as_ref(), now_ms));
    if !v.agents.is_empty() {
        out.push_str("agents:\n");
        for a in &v.agents {
            out.push_str(&format!(
                "  {}: {}\n",
                agent_name(a),
                format_status(a.status.as_ref(), now_ms)
            ));
        }
    }
    out
}

/// `wsx agent list`: `<id>  <label>[  (primary)]`, then the agent's own
/// status when it has reported one. Rows without a status keep the
/// pre-per-agent-status format byte for byte.
pub fn render_agents(agents: &[AgentRecord], now_ms: i64) -> String {
    let mut out = String::new();
    for a in agents {
        let tag = if a.primary { "  (primary)" } else { "" };
        out.push_str(&format!("{}  {}{}", a.id, a.label, tag));
        if a.status.is_some() {
            out.push_str(&format!("  {}", format_status(a.status.as_ref(), now_ms)));
        }
        out.push('\n');
    }
    out
}

pub fn render_recap(v: &RecapView) -> String {
    let Some(r) = &v.recap else {
        return "no recap set\n".to_string();
    };
    let f = |o: &Option<String>| o.as_deref().unwrap_or("-").to_string();
    format!(
        "goal:        {}\nstate:       {}\nnext:        {}\n\
         goal-short:  {}\nstate-short: {}\nnext-short:  {}\n",
        f(&r.goal),
        f(&r.state),
        f(&r.next),
        f(&r.goal_short),
        f(&r.state_short),
        f(&r.next_short),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{NewWorkspace, ReportedState};
    use crate::pty::session::AgentKind;

    fn seed() -> (Store, Workspace, AgentInstanceId, AgentInstanceId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "wsx/w",
                worktree_path: std::path::Path::new("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let primary = store
            .add_primary_agent(id, AgentKind::Claude, 1)
            .unwrap()
            .id;
        let peer = store.add_workspace_agent(id, AgentKind::Codex).unwrap().id;
        let ws = store.workspace_by_id(id).unwrap().unwrap();
        (store, ws, primary, peer)
    }

    #[test]
    fn status_view_shows_each_agents_own_state() {
        let (store, ws, primary, peer) = seed();
        store
            .set_agent_status(
                ws.id,
                Some(primary),
                ReportedState::Working,
                Some("impl"),
                "model",
            )
            .unwrap();
        store
            .set_agent_status(
                ws.id,
                Some(peer),
                ReportedState::Blocked,
                Some("need a call"),
                "hook",
            )
            .unwrap();
        let v = status_view(&store, &ws).unwrap();
        assert_eq!((v.repo.as_str(), v.slug.as_str()), ("r", "w"));
        assert_eq!(v.status.as_ref().unwrap().state, "blocked");
        assert_eq!(v.agents.len(), 2);
        assert_eq!(v.agents[0].label, "claude");
        assert!(v.agents[0].primary);
        assert_eq!(v.agents[0].status.as_ref().unwrap().state, "working");
        assert_eq!(v.agents[1].label, "codex");
        assert_eq!(v.agents[1].status.as_ref().unwrap().state, "blocked");

        let reported = v.agents[0].status.as_ref().unwrap().reported_at;
        let text = render_status(&v, reported + 180_000);
        assert_eq!(
            text,
            format!(
                "status: blocked — \"need a call\" (hook, {})\n\
                 agents:\n  \
                 claude (primary): working — \"impl\" (model, 3m ago)\n  \
                 codex: blocked — \"need a call\" (hook, {})\n",
                crate::commands::context::format_age(
                    reported + 180_000,
                    v.status.as_ref().unwrap().reported_at
                ),
                crate::commands::context::format_age(
                    reported + 180_000,
                    v.agents[1].status.as_ref().unwrap().reported_at
                ),
            )
        );
    }

    #[test]
    fn empty_status_renders_dashes() {
        let (store, ws, _, _) = seed();
        let v = status_view(&store, &ws).unwrap();
        assert_eq!(
            render_status(&v, 0),
            "status: -\nagents:\n  claude (primary): -\n  codex: -\n"
        );
    }

    #[test]
    fn agent_list_without_status_keeps_the_old_format() {
        let (store, ws, primary, peer) = seed();
        let text = render_agents(&agents(&store, ws.id).unwrap(), 0);
        assert_eq!(
            text,
            format!("{}  claude  (primary)\n{}  codex\n", primary.0, peer.0)
        );
    }

    #[test]
    fn agent_list_appends_a_reported_status() {
        let (store, ws, _, peer) = seed();
        store
            .set_agent_status(ws.id, Some(peer), ReportedState::Done, None, "model")
            .unwrap();
        let recs = agents(&store, ws.id).unwrap();
        let now = recs[1].status.as_ref().unwrap().reported_at;
        let text = render_agents(&recs, now);
        assert!(
            text.ends_with(&format!("{}  codex  done (model, 0s ago)\n", peer.0)),
            "{text}"
        );
    }

    #[test]
    fn recap_renders_like_before() {
        let (store, ws, _, _) = seed();
        assert_eq!(
            render_recap(&recap_view(&store, &ws).unwrap()),
            "no recap set\n"
        );
        store
            .set_workspace_recap(ws.id, Some("g"), None, None, Some("gs"), None, None)
            .unwrap();
        assert_eq!(
            render_recap(&recap_view(&store, &ws).unwrap()),
            "goal:        g\nstate:       -\nnext:        -\n\
             goal-short:  gs\nstate-short: -\nnext-short:  -\n"
        );
    }
}
