//! Project Manager pane: renders PM PTY into a sub-rect with focus-aware title.

use crate::data::store::{
    Repo, RepoId, ReportedState, ReportedStatus, Workspace, WorkspaceId, WorkspaceRecap,
    WorkspaceState,
};
use crate::git::forge::BranchLifecycle;
use crate::pty::render::render_screen;
use crate::pty::session::Session;
use crate::ui::PaneFocus;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::collections::HashMap;
use std::sync::Arc;

pub struct DigestInputs<'a> {
    pub repos: &'a [Repo],
    pub workspaces: &'a [(RepoId, Workspace)],
    pub recaps: &'a HashMap<WorkspaceId, WorkspaceRecap>,
    pub pushed_status: &'a HashMap<WorkspaceId, ReportedStatus>,
    pub git: &'a HashMap<WorkspaceId, crate::git::WorkspaceStatus>,
    pub pr_lifecycle: &'a HashMap<WorkspaceId, BranchLifecycle>,
    pub pr_number: &'a HashMap<WorkspaceId, u32>,
    pub last_activity_ms: &'a HashMap<WorkspaceId, i64>,
}

#[derive(Debug, Clone)]
pub struct DigestCard {
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub branch: String,
    pub agent: crate::pty::session::AgentKind,
    pub status: Option<ReportedStatus>,
    pub recap: Option<WorkspaceRecap>,
    /// Session log has newer activity than the recap's `updated_at` — the
    /// recap predates the latest work.
    pub recap_stale: bool,
    pub git: Option<crate::git::WorkspaceStatus>,
    pub pr: Option<(BranchLifecycle, Option<u32>)>,
    pub last_activity_ms: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RepoDigest {
    pub repo_name: String,
    pub cards: Vec<DigestCard>,
}

/// Needs-attention rank: blocked (0) before waiting (1) before the rest (2).
fn attention_rank(status: Option<&ReportedStatus>) -> u8 {
    match status.map(|s| s.state) {
        Some(ReportedState::Blocked) => 0,
        Some(ReportedState::Waiting) => 1,
        _ => 2,
    }
}

/// Assemble the digest: Ready workspaces grouped by repo (repos in `repos`
/// order, repos with no Ready workspaces omitted), each repo's cards sorted
/// blocked → waiting → stalest-first (oldest activity first).
pub fn build_digest(inputs: &DigestInputs) -> Vec<RepoDigest> {
    let mut out = Vec::new();
    for repo in inputs.repos {
        let mut cards: Vec<DigestCard> = inputs
            .workspaces
            .iter()
            .filter(|(rid, w)| *rid == repo.id && w.state == WorkspaceState::Ready)
            .map(|(_, w)| {
                let recap = inputs.recaps.get(&w.id).cloned();
                let last = inputs.last_activity_ms.get(&w.id).copied();
                let recap_stale = match (&recap, last) {
                    (Some(r), Some(act)) => act > r.updated_at,
                    _ => false,
                };
                DigestCard {
                    workspace_id: w.id,
                    name: w.name.clone(),
                    branch: w.branch.clone(),
                    agent: w.agent,
                    status: inputs.pushed_status.get(&w.id).cloned(),
                    recap,
                    recap_stale,
                    git: inputs.git.get(&w.id).copied(),
                    pr: inputs
                        .pr_lifecycle
                        .get(&w.id)
                        .map(|lc| (*lc, inputs.pr_number.get(&w.id).copied())),
                    last_activity_ms: last,
                }
            })
            .collect();
        if cards.is_empty() {
            continue;
        }
        cards.sort_by_key(|c| {
            (
                attention_rank(c.status.as_ref()),
                c.last_activity_ms.unwrap_or(0),
            )
        });
        out.push(RepoDigest {
            repo_name: repo.name.clone(),
            cards,
        });
    }
    out
}

pub fn card_count(digest: &[RepoDigest]) -> usize {
    digest.iter().map(|r| r.cards.len()).sum()
}

pub fn card_at(digest: &[RepoDigest], index: usize) -> Option<&DigestCard> {
    digest.iter().flat_map(|r| &r.cards).nth(index)
}

/// Render the PM pane into `area`. When `session` is `None` (pane was
/// just opened and spawn is in flight), a single placeholder line is
/// shown.
pub fn render(
    f: &mut Frame,
    area: Rect,
    session: Option<&Arc<Session>>,
    focus: PaneFocus,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    let title_area = chunks[0];
    let term_area = chunks[1];

    let label = match focus {
        PaneFocus::ProjectManager => "Project Manager [Tab/Esc back]",
        PaneFocus::Dashboard | PaneFocus::DetailBarReply => {
            "Project Manager [Tab to focus · r refresh]"
        }
    };
    let width = title_area.width as usize;
    let used = label.chars().count();
    let gap = 2;
    let rule_len = width.saturating_sub(used + gap);
    let mut spans: Vec<Span<'static>> = vec![Span::styled(label.to_string(), theme.dim_style())];
    if rule_len > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), title_area);

    match session {
        Some(s) => {
            let offset = s
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed);
            let mut parser = s.parser.lock().unwrap();
            parser.set_scrollback(offset);
            let screen = parser.screen();
            render_screen(screen, f.buffer_mut(), term_area);
            if matches!(focus, PaneFocus::ProjectManager) && !screen.hide_cursor() && offset == 0 {
                let (cy, cx) = screen.cursor_position();
                f.set_cursor_position((term_area.x + cx, term_area.y + cy));
            }
        }
        None => {
            f.render_widget(
                Paragraph::new("starting project manager…").style(theme.dim_style()),
                term_area,
            );
        }
    }
}

/// Recompute PTY dimensions after a terminal resize.
pub fn resize_session(session: &Arc<Session>, area: Rect) {
    // Subtract 1 row for the title bar.
    let _ = session.resize(area.width, area.height.saturating_sub(1));
}

#[cfg(test)]
mod digest_tests {
    use super::*;
    use crate::data::store::{
        Repo, RepoId, ReportedState, ReportedStatus, Workspace, WorkspaceId, WorkspaceRecap,
        WorkspaceState,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn repo(id: i64, name: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: name.into(),
            path: PathBuf::from("/tmp/repo"),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
            sort_order: id,
        }
    }

    fn ws(id: i64, repo: i64, name: &str, state: WorkspaceState) -> (RepoId, Workspace) {
        (
            RepoId(repo),
            Workspace {
                id: WorkspaceId(id),
                repo_id: RepoId(repo),
                name: name.into(),
                branch: format!("b/{name}"),
                worktree_path: PathBuf::from(format!("/tmp/{name}")),
                state,
                setup_status: crate::data::store::SetupStatus::Ok,
                created_at: 0,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            },
        )
    }

    fn pushed(state: ReportedState) -> ReportedStatus {
        ReportedStatus {
            state,
            message: Some("msg".into()),
            source: "model".into(),
            reported_at: 1_000,
        }
    }

    #[test]
    fn filters_non_ready_and_empty_repos() {
        let repos = vec![repo(1, "alpha"), repo(2, "beta")];
        let workspaces = vec![
            ws(10, 1, "ready", WorkspaceState::Ready),
            ws(11, 1, "broken", WorkspaceState::Failed),
            // repo 2 has no ready workspaces -> omitted entirely
            ws(12, 2, "pending", WorkspaceState::Failed),
        ];
        let empty = HashMap::new();
        let inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &empty,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &HashMap::new(),
        };
        let digest = build_digest(&inputs);
        assert_eq!(digest.len(), 1);
        assert_eq!(digest[0].repo_name, "alpha");
        assert_eq!(digest[0].cards.len(), 1);
        assert_eq!(digest[0].cards[0].name, "ready");
        assert_eq!(card_count(&digest), 1);
    }

    #[test]
    fn orders_blocked_then_waiting_then_stalest_first() {
        let repos = vec![repo(1, "alpha")];
        let workspaces = vec![
            ws(1, 1, "fresh-working", WorkspaceState::Ready),
            ws(2, 1, "stale-working", WorkspaceState::Ready),
            ws(3, 1, "waiting", WorkspaceState::Ready),
            ws(4, 1, "blocked", WorkspaceState::Ready),
        ];
        let mut pushed_status = HashMap::new();
        pushed_status.insert(WorkspaceId(1), pushed(ReportedState::Working));
        pushed_status.insert(WorkspaceId(2), pushed(ReportedState::Working));
        pushed_status.insert(WorkspaceId(3), pushed(ReportedState::Waiting));
        pushed_status.insert(WorkspaceId(4), pushed(ReportedState::Blocked));
        let mut last_activity = HashMap::new();
        last_activity.insert(WorkspaceId(1), 9_000);
        last_activity.insert(WorkspaceId(2), 1_000); // stalest of the rank-2 pair
        let empty = HashMap::new();
        let inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &empty,
            pushed_status: &pushed_status,
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &last_activity,
        };
        let names: Vec<_> = build_digest(&inputs)[0]
            .cards
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(
            names,
            ["blocked", "waiting", "stale-working", "fresh-working"]
        );
    }

    #[test]
    fn recap_stale_when_activity_newer_than_recap() {
        let repos = vec![repo(1, "alpha")];
        let workspaces = vec![
            ws(1, 1, "stale", WorkspaceState::Ready),
            ws(2, 1, "fresh", WorkspaceState::Ready),
            ws(3, 1, "norecap", WorkspaceState::Ready),
        ];
        let mut recaps = HashMap::new();
        let recap = |t| WorkspaceRecap {
            goal: Some("g".into()),
            state: Some("s".into()),
            next: Some("n".into()),
            updated_at: t,
        };
        recaps.insert(WorkspaceId(1), recap(1_000));
        recaps.insert(WorkspaceId(2), recap(9_000));
        let mut last_activity = HashMap::new();
        last_activity.insert(WorkspaceId(1), 5_000);
        last_activity.insert(WorkspaceId(2), 5_000);
        let inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &recaps,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &last_activity,
        };
        let digest = build_digest(&inputs);
        let by_name = |n: &str| {
            digest[0]
                .cards
                .iter()
                .find(|c| c.name == n)
                .unwrap()
                .clone()
        };
        assert!(by_name("stale").recap_stale);
        assert!(!by_name("fresh").recap_stale);
        assert!(
            !by_name("norecap").recap_stale,
            "no recap -> shown as missing, not stale"
        );
        assert!(by_name("norecap").recap.is_none());
    }

    #[test]
    fn card_at_indexes_across_repos() {
        let repos = vec![repo(1, "alpha"), repo(2, "beta")];
        let workspaces = vec![
            ws(1, 1, "a1", WorkspaceState::Ready),
            ws(2, 2, "b1", WorkspaceState::Ready),
        ];
        let empty = HashMap::new();
        let inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &empty,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &HashMap::new(),
        };
        let digest = build_digest(&inputs);
        assert_eq!(card_count(&digest), 2);
        assert_eq!(card_at(&digest, 0).unwrap().name, "a1");
        assert_eq!(card_at(&digest, 1).unwrap().name, "b1");
        assert!(card_at(&digest, 2).is_none());
    }
}
