//! The desktop-neutral workspace summary that panel widgets render: how many
//! workspaces there are, the most urgent reported state, a per-workspace
//! tooltip, and one row per workspace. `wsx desktop status` prints it as
//! JSON for the Plasma applet; the waybar module formats its own payload
//! from the same [`summarize`].

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::data::store::{ReportedState, Store};
use crate::desktop::rows::{attention_rank, sanitize, state_glyph};
use crate::error::Result;

/// One workspace, as the summary sees it.
pub(crate) struct SummaryWorkspace {
    pub name: String,
    pub state: Option<ReportedState>,
    pub message: Option<String>,
    pub pr_number: Option<u32>,
}

/// A registered repo and its workspaces, in the store's order.
pub(crate) struct SummaryRepo {
    pub name: String,
    pub workspaces: Vec<SummaryWorkspace>,
}

/// Every repo and workspace, and the most urgent state reported among them.
pub(crate) struct Summary {
    pub repos: Vec<SummaryRepo>,
    pub most_urgent: Option<ReportedState>,
}

impl Summary {
    /// Workspaces across every repo.
    pub(crate) fn count(&self) -> usize {
        self.repos.iter().map(|r| r.workspaces.len()).sum()
    }

    /// Each repo on its own line, then its workspaces indented beneath it
    /// with their state glyph and status message. `clean` makes each name
    /// and message safe for the surface showing it.
    pub(crate) fn tooltip(&self, clean: impl Fn(&str) -> String) -> String {
        let mut lines = Vec::new();
        for repo in &self.repos {
            lines.push(clean(&repo.name));
            if repo.workspaces.is_empty() {
                lines.push("  (no workspaces)".into());
            }
            for ws in &repo.workspaces {
                let mut line = format!("  {} {}", state_glyph(ws.state), clean(&ws.name));
                if let Some(msg) = &ws.message {
                    line.push_str(" \u{2014} ");
                    line.push_str(&clean(msg));
                }
                lines.push(line);
            }
        }
        lines.join("\n")
    }
}

/// Read the summary: repos in `repo::list` order, each one's workspaces in
/// the store's order, with their reported status and cached PR number.
pub(crate) fn summarize(store: &Store) -> Result<Summary> {
    let statuses = store.all_workspace_status()?;
    let pr_numbers: HashMap<_, _> = store
        .all_scm_cache()?
        .into_iter()
        .map(|(id, cache)| (id, cache.pr_number))
        .collect();
    let mut most_urgent: Option<ReportedState> = None;
    let mut repos = Vec::new();
    for repo in crate::data::repo::list(store)? {
        let mut workspaces = Vec::new();
        for ws in store.workspaces(repo.id)? {
            let status = statuses.get(&ws.id);
            if let Some(st) = status
                && most_urgent.is_none_or(|b| attention_rank(st.state) > attention_rank(b))
            {
                most_urgent = Some(st.state);
            }
            workspaces.push(SummaryWorkspace {
                state: status.map(|s| s.state),
                message: status.and_then(|s| s.message.clone()),
                pr_number: pr_numbers.get(&ws.id).copied().flatten(),
                name: ws.name,
            });
        }
        repos.push(SummaryRepo {
            name: repo.name,
            workspaces,
        });
    }
    Ok(Summary { repos, most_urgent })
}

/// The class a state shows as, which colors the indicator: `blocked`,
/// `done`, `waiting` or `working` (`busy` shows as working), and `idle` when
/// nothing reports a state.
pub(crate) fn state_class(state: Option<ReportedState>) -> &'static str {
    match state {
        Some(ReportedState::Blocked) => "blocked",
        Some(ReportedState::Done) => "done",
        Some(ReportedState::Waiting) => "waiting",
        Some(ReportedState::Working | ReportedState::Busy) => "working",
        None => "idle",
    }
}

/// `wsx desktop status`'s output.
#[derive(Serialize, Debug, PartialEq)]
pub struct DesktopStatus {
    /// Workspaces across every repo.
    pub count: usize,
    /// The [`state_class`] of the most urgent reported state.
    pub class: &'static str,
    /// Plain text, one line per repo and per workspace. Empty when no repo
    /// is registered.
    pub tooltip: String,
    /// One per workspace, grouped by repo in the tooltip's order.
    pub rows: Vec<DesktopRow>,
    /// Why the summary couldn't be read; everything else is then empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct DesktopRow {
    pub repo: String,
    pub slug: String,
    /// The reported state's `as_str()`, or null when none is reported.
    pub state: Option<&'static str>,
    /// The status message with control characters and line separators
    /// turned into spaces, so it stays on one line. Null when unset.
    pub message: Option<String>,
    pub pr_number: Option<u32>,
}

pub fn desktop_status(store: &Store) -> Result<DesktopStatus> {
    let summary = summarize(store)?;
    let tooltip = summary.tooltip(sanitize);
    let rows = summary
        .repos
        .iter()
        .flat_map(|repo| {
            repo.workspaces.iter().map(|ws| DesktopRow {
                repo: repo.name.clone(),
                slug: ws.name.clone(),
                state: ws.state.map(ReportedState::as_str),
                message: ws.message.as_deref().map(sanitize),
                pr_number: ws.pr_number,
            })
        })
        .collect();
    Ok(DesktopStatus {
        count: summary.count(),
        class: state_class(summary.most_urgent),
        tooltip,
        rows,
        error: None,
    })
}

/// The status, or an otherwise empty one carrying the error.
fn load(db_path: &Path) -> DesktopStatus {
    Store::open(db_path)
        .and_then(|store| desktop_status(&store))
        .unwrap_or_else(|e| DesktopStatus {
            count: 0,
            class: state_class(None),
            tooltip: String::new(),
            rows: Vec::new(),
            error: Some(e.to_string()),
        })
}

/// Never fails: a widget polls this, and shows `error` rather than a
/// command failure when the database can't be read.
pub fn print_status(db_path: &Path) {
    // Plain strings and numbers only, so serializing can't fail.
    println!(
        "{}",
        serde_json::to_string(&load(db_path)).expect("serialize desktop status")
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store, WorkspaceState};
    use crate::pty::session::AgentKind;

    fn seed() -> (Store, Vec<crate::data::store::WorkspaceId>) {
        let store = Store::open_in_memory().unwrap();
        let alpha = store
            .add_repo(std::path::Path::new("/tmp/alpha"), "alpha", "feat")
            .unwrap();
        store
            .add_repo(std::path::Path::new("/tmp/empty"), "empty", "feat")
            .unwrap();
        let mut ids = Vec::new();
        for name in ["one", "two"] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id: alpha,
                    name,
                    branch: &format!("feat/{name}"),
                    worktree_path: &std::path::PathBuf::from(format!("/tmp/wt-{name}")),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
            ids.push(id);
        }
        (store, ids)
    }

    #[test]
    fn desktop_status_counts_classes_and_lists_rows_in_tooltip_order() {
        let (store, ids) = seed();
        store
            .set_workspace_status(ids[0], ReportedState::Busy, Some("a <b> & c"), "hook")
            .unwrap();
        store
            .set_workspace_status(ids[1], ReportedState::Waiting, Some("line\nbreak"), "model")
            .unwrap();
        store
            .upsert_scm_pr(
                ids[1],
                &crate::git::forge::PrStatus {
                    lifecycle: crate::git::forge::BranchLifecycle::PrOpen,
                    number: Some(42),
                    url: None,
                    review: None,
                    unresolved: None,
                },
                1000,
            )
            .unwrap();

        let s = desktop_status(&store).unwrap();
        assert_eq!(s.count, 2);
        // Waiting outranks busy, which would show as working.
        assert_eq!(s.class, "waiting");
        // Plain text, not Pango: markup characters stay as they are, and a
        // message's line break can't add a tooltip line.
        assert_eq!(
            s.tooltip,
            "alpha\n  \u{21bb} one \u{2014} a <b> & c\n  \u{2026} two \u{2014} line break\n\
             empty\n  (no workspaces)"
        );
        assert_eq!(
            s.rows,
            vec![
                DesktopRow {
                    repo: "alpha".into(),
                    slug: "one".into(),
                    state: Some("busy"),
                    message: Some("a <b> & c".into()),
                    pr_number: None,
                },
                DesktopRow {
                    repo: "alpha".into(),
                    slug: "two".into(),
                    state: Some("waiting"),
                    message: Some("line break".into()),
                    pr_number: Some(42),
                },
            ]
        );
        let json = serde_json::to_value(&s).unwrap();
        assert!(json.get("error").is_none(), "{json}");
    }

    #[test]
    fn nothing_reported_is_idle_and_no_repos_means_an_empty_tooltip() {
        let (store, _) = seed();
        let s = desktop_status(&store).unwrap();
        assert_eq!(s.class, "idle");
        assert!(
            s.rows
                .iter()
                .all(|r| r.state.is_none() && r.message.is_none())
        );

        let empty = desktop_status(&Store::open_in_memory().unwrap()).unwrap();
        assert_eq!(empty.count, 0);
        assert_eq!(empty.tooltip, "");
        assert!(empty.rows.is_empty());
    }

    #[test]
    fn an_unreadable_database_reports_its_error_instead_of_failing() {
        // A directory can't be opened as a database.
        let dir = tempfile::tempdir().unwrap();
        let s = load(dir.path());
        assert!(s.error.is_some(), "{s:?}");
        assert_eq!((s.count, s.class, s.tooltip.as_str()), (0, "idle", ""));
        assert!(s.rows.is_empty());
    }
}
