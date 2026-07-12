//! Project Manager digest: native per-workspace recap cards.

use crate::data::store::{
    Repo, RepoId, ReportedState, ReportedStatus, Workspace, WorkspaceId, WorkspaceRecap,
    WorkspaceState,
};
use crate::git::forge::BranchLifecycle;
use crate::ui::PaneFocus;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::collections::HashMap;

pub struct DigestInputs<'a> {
    pub repos: &'a [Repo],
    pub workspaces: &'a [(RepoId, Workspace)],
    pub recaps: &'a HashMap<WorkspaceId, WorkspaceRecap>,
    pub pushed_status: &'a HashMap<WorkspaceId, ReportedStatus>,
    pub git: &'a HashMap<WorkspaceId, crate::git::WorkspaceStatus>,
    pub pr_lifecycle: &'a HashMap<WorkspaceId, BranchLifecycle>,
    pub pr_number: &'a HashMap<WorkspaceId, u32>,
    pub last_activity_ms: &'a HashMap<WorkspaceId, i64>,
    /// Live filter needle: cards whose workspace name doesn't contain it
    /// (case-insensitive) are dropped. `None` or `""` matches everything.
    pub filter: Option<&'a str>,
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

/// Session activity within this window after a recap update does not mark
/// the recap stale — the `wsx recap set` tool call itself (and the rest of
/// that turn) lands in the session log right after `updated_at`.
pub const RECAP_STALE_SLACK_MS: i64 = 300_000;

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
    let needle = inputs
        .filter
        .filter(|f| !f.is_empty())
        .map(|f| f.to_lowercase());
    let mut out = Vec::new();
    for repo in inputs.repos {
        let mut cards: Vec<DigestCard> = inputs
            .workspaces
            .iter()
            .filter(|(rid, w)| {
                *rid == repo.id
                    && w.state == WorkspaceState::Ready
                    && needle
                        .as_ref()
                        .map(|n| w.name.to_lowercase().contains(n))
                        .unwrap_or(true)
            })
            .map(|(_, w)| {
                let recap = inputs.recaps.get(&w.id).cloned();
                let last = inputs.last_activity_ms.get(&w.id).copied();
                let recap_stale = match (&recap, last) {
                    (Some(r), Some(act)) => act > r.updated_at + RECAP_STALE_SLACK_MS,
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

/// Render the "PM digest" pane: per-repo grouped workspace cards with
/// status, recap, and fact lines, plus j/k selection and scrolling.
#[allow(clippy::too_many_arguments)]
pub fn render_digest(
    f: &mut Frame,
    area: Rect,
    digest: &[RepoDigest],
    selected: usize,
    focus: PaneFocus,
    filter: Option<&str>,
    now_ms: i64,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(area);
    render_title(f, chunks[0], focus, filter, theme);
    let body = chunks[1];

    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut sel_range = (0usize, 0usize);
    let mut idx = 0usize;
    for repo in digest {
        lines.push(repo_header_line(&repo.repo_name, body.width, theme));
        for card in &repo.cards {
            let is_sel = idx == selected;
            let start = lines.len();
            push_card_lines(&mut lines, card, is_sel, now_ms, theme);
            if is_sel {
                sel_range = (start, lines.len().saturating_sub(1));
            }
            lines.push(Line::raw(""));
            idx += 1;
        }
    }
    if lines.is_empty() {
        let msg = if filter.map(|n| !n.is_empty()).unwrap_or(false) {
            "no matching workspaces"
        } else {
            "no active workspaces"
        };
        f.render_widget(Paragraph::new(msg).style(theme.dim_style()), body);
        return;
    }
    let offset = scroll_offset(sel_range.0, sel_range.1, body.height as usize, lines.len());
    f.render_widget(Paragraph::new(lines).scroll((offset as u16, 0)), body);
}

/// Section title: a full-width `bg_soft` strip with the dim label on top.
/// The filled row is what separates the PM pane from the detail bar above —
/// a `─` rule here would blur into the repo-header and detail-bar rules,
/// which use the same glyph for sub-section separators.
fn render_title(f: &mut Frame, area: Rect, focus: PaneFocus, filter: Option<&str>, theme: &Theme) {
    let label = match (focus, filter) {
        // Filter mode: echo the live needle even while it's still empty,
        // so the `/` press has visible feedback before any typing.
        (PaneFocus::ProjectManager, Some(needle)) => {
            format!("Project Manager [/{needle} · Esc clear · Enter attach]")
        }
        (PaneFocus::ProjectManager, None) => {
            "Project Manager [j/k select · / filter · Enter attach · Esc/Tab back]".to_string()
        }
        (PaneFocus::Dashboard | PaneFocus::DetailBarReply, _) => {
            "Project Manager [Tab to focus · r refresh · p close]".to_string()
        }
    };
    let width = area.width as usize;
    let used = label.chars().count();
    let strip = theme.chip_bg_style();
    let mut spans: Vec<Span<'static>> = vec![Span::styled(label, theme.dim_style().patch(strip))];
    let pad = width.saturating_sub(used);
    if pad > 0 {
        spans.push(Span::styled(" ".repeat(pad), strip));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Repo group header: leading space, repo name, then a dim `─` rule padded
/// out to the pane width — mirrors the title's rule style at a smaller
/// scale.
fn repo_header_line(name: &str, width: u16, theme: &Theme) -> Line<'static> {
    let width = width as usize;
    let label = format!(" {name}");
    let used = label.chars().count();
    let gap = 1;
    let rule_len = width.saturating_sub(used + gap);
    let mut spans: Vec<Span<'static>> = vec![Span::raw(label)];
    if rule_len > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
        spans.push(Span::styled("─".repeat(rule_len), theme.dim_style()));
    }
    Line::from(spans)
}

/// Push one card's lines (header, recap, facts) onto `lines`.
fn push_card_lines(
    lines: &mut Vec<Line<'static>>,
    card: &DigestCard,
    is_sel: bool,
    now_ms: i64,
    theme: &Theme,
) {
    // Line 1: selection marker, name, branch + agent, status bracket.
    let marker = if is_sel { "▸" } else { " " };
    let mut name_style = Style::default();
    if is_sel {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    let mut header_spans: Vec<Span<'static>> = vec![
        Span::raw(format!("{marker} ")),
        Span::styled(card.name.clone(), name_style),
        Span::raw("  "),
        Span::styled(
            format!("{}  {}", card.branch, card.agent.display_name()),
            theme.dim_style(),
        ),
    ];
    if let Some(status) = &card.status {
        let age = crate::ui::updates_bar::format_age(now_ms - status.reported_at);
        let mut bracket = format!("  [{} {}]", status.state.as_str(), age);
        if let Some(msg) = &status.message {
            bracket.push(' ');
            bracket.push_str(msg);
        }
        header_spans.push(Span::raw(bracket));
    }
    lines.push(Line::from(header_spans));

    // Recap lines: goal/state/next, only for fields present.
    match &card.recap {
        Some(recap) => {
            if let Some(goal) = &recap.goal {
                lines.push(Line::raw(format!("     goal:  {goal}")));
            }
            if let Some(state) = &recap.state {
                lines.push(Line::raw(format!("     state: {state}")));
            }
            if let Some(next) = &recap.next {
                lines.push(Line::raw(format!("     next:  {next}")));
            }
        }
        None => {
            lines.push(Line::styled(
                "     no recap yet — agent hasn't run since this feature landed",
                theme.dim_style(),
            ));
        }
    }

    // Facts line: git counts, PR + lifecycle, last-activity age, stale flag.
    let mut segs: Vec<Span<'static>> = Vec::new();
    if let Some(git) = &card.git {
        segs.push(Span::styled(
            format!(
                "↑{} ↓{} ~{} ?{}",
                git.ahead, git.behind, git.modified, git.untracked
            ),
            theme.dim_style(),
        ));
    }
    // `NoPr` renders no chip at all, matching the detail bar and attached
    // chip row — "PR no pr" is noise, not information.
    if let Some((lifecycle, number)) = &card.pr {
        if !matches!(lifecycle, BranchLifecycle::NoPr) {
            let label = lifecycle_label(*lifecycle);
            let text = match number {
                Some(n) => format!("PR #{n} {label}"),
                None => format!("PR {label}"),
            };
            let style = theme
                .lifecycle_style(Some(*lifecycle))
                .unwrap_or_else(|| theme.dim_style());
            segs.push(Span::styled(text, style));
        }
    }
    if let Some(ms) = card.last_activity_ms {
        if ms != 0 {
            let age = crate::ui::updates_bar::format_age(now_ms - ms);
            segs.push(Span::styled(format!("active {age} ago"), theme.dim_style()));
        }
    }
    if card.recap_stale {
        segs.push(Span::styled("recap stale", theme.dim_style()));
    }
    if !segs.is_empty() {
        let mut fact_spans: Vec<Span<'static>> = vec![Span::raw("     ")];
        for (i, seg) in segs.into_iter().enumerate() {
            if i > 0 {
                fact_spans.push(Span::styled(" · ", theme.dim_style()));
            }
            fact_spans.push(seg);
        }
        lines.push(Line::from(fact_spans));
    }
}

/// Short label for a `BranchLifecycle`, used in the facts line.
fn lifecycle_label(lc: BranchLifecycle) -> &'static str {
    match lc {
        BranchLifecycle::NoPr => "no pr",
        BranchLifecycle::PrDraft => "draft",
        BranchLifecycle::PrOpen => "open",
        BranchLifecycle::PrConflicted => "conflicts",
        BranchLifecycle::PrMerged => "merged",
        BranchLifecycle::PrClosed => "closed",
    }
}

/// Smallest scroll offset (in lines) that keeps `[sel_start, sel_end]`
/// visible within a `viewport`-line window over `total` lines, clamped so
/// the window never scrolls past the content's end.
fn scroll_offset(_sel_start: usize, sel_end: usize, viewport: usize, total: usize) -> usize {
    if total <= viewport {
        return 0;
    }
    let max = total - viewport;
    if sel_end < viewport {
        return 0;
    }
    (sel_end + 1 - viewport).min(max)
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::ui::PaneFocus;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn buffer_text(terminal: &Terminal<TestBackend>) -> String {
        let buf = terminal.backend().buffer();
        let area = buf.area();
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn draw(digest: &[RepoDigest], selected: usize, focus: PaneFocus) -> String {
        draw_filtered(digest, selected, focus, None)
    }

    fn draw_filtered(
        digest: &[RepoDigest],
        selected: usize,
        focus: PaneFocus,
        filter: Option<&str>,
    ) -> String {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = crate::ui::theme::Theme::default();
        terminal
            .draw(|f| render_digest(f, f.area(), digest, selected, focus, filter, 10_000, &theme))
            .unwrap();
        buffer_text(&terminal)
    }

    // Reuse digest_tests' fixture builders via a tiny local card factory:
    fn card(name: &str) -> DigestCard {
        DigestCard {
            workspace_id: crate::data::store::WorkspaceId(1),
            name: name.into(),
            branch: format!("b/{name}"),
            agent: crate::pty::session::AgentKind::Claude,
            status: None,
            recap: None,
            recap_stale: false,
            git: None,
            pr: None,
            last_activity_ms: None,
        }
    }

    #[test]
    fn title_row_is_a_full_width_bg_strip_without_rule() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("w")],
        }];
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = crate::ui::theme::Theme::default();
        terminal
            .draw(|f| {
                render_digest(
                    f,
                    f.area(),
                    &digest,
                    0,
                    PaneFocus::Dashboard,
                    None,
                    10_000,
                    &theme,
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        for x in 0..buf.area().width {
            let cell = &buf[(x, 0)];
            assert_eq!(
                cell.bg, theme.bg_soft,
                "title row col {x} must carry the bg_soft strip"
            );
            assert_ne!(
                cell.symbol(),
                "─",
                "title row must not draw a ─ rule (col {x})"
            );
        }
        // The repo group headers below keep their ─ rules — only the
        // section title trades the rule for the strip.
        let text = buffer_text(&terminal);
        let repo_line = text.lines().nth(1).unwrap();
        assert!(repo_line.contains("alpha"), "{text}");
        assert!(repo_line.contains("─"), "{text}");
    }

    #[test]
    fn focused_title_advertises_filter_key() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("w")],
        }];
        assert!(draw(&digest, 0, PaneFocus::ProjectManager).contains("/ filter"));
    }

    #[test]
    fn active_filter_echoes_needle_in_title() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("auth-refactor")],
        }];
        let text = draw_filtered(&digest, 0, PaneFocus::ProjectManager, Some("auth"));
        assert!(text.contains("/auth"), "{text}");
        assert!(text.contains("Esc clear"), "{text}");
    }

    #[test]
    fn zero_match_placeholder_differs_from_empty_placeholder() {
        let with_filter = draw_filtered(&[], 0, PaneFocus::ProjectManager, Some("zzz"));
        assert!(
            with_filter.contains("no matching workspaces"),
            "{with_filter}"
        );
        let no_filter = draw(&[], 0, PaneFocus::Dashboard);
        assert!(no_filter.contains("no active workspaces"), "{no_filter}");
    }

    #[test]
    fn renders_repo_header_card_and_no_recap_placeholder() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("my-ws")],
        }];
        let text = draw(&digest, 0, PaneFocus::Dashboard);
        assert!(text.contains("alpha"), "{text}");
        assert!(text.contains("my-ws"), "{text}");
        assert!(text.contains("no recap yet"), "{text}");
    }

    #[test]
    fn renders_recap_lines_status_facts_and_stale_marker() {
        let mut c = card("busy-ws");
        c.recap = Some(crate::data::store::WorkspaceRecap {
            goal: Some("fix auth".into()),
            state: Some("tests failing".into()),
            next: Some("debug regex".into()),
            updated_at: 1_000,
        });
        c.recap_stale = true;
        c.status = Some(crate::data::store::ReportedStatus {
            state: crate::data::store::ReportedState::Blocked,
            message: Some("need a decision".into()),
            source: "model".into(),
            reported_at: 4_000,
        });
        c.git = Some(crate::git::WorkspaceStatus {
            modified: 3,
            untracked: 1,
            ahead: 2,
            behind: 0,
        });
        c.pr = Some((crate::git::forge::BranchLifecycle::PrOpen, Some(241)));
        c.last_activity_ms = Some(3_000);
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![c],
        }];
        let text = draw(&digest, 0, PaneFocus::ProjectManager);
        assert!(text.contains("goal:"), "{text}");
        assert!(text.contains("fix auth"), "{text}");
        assert!(text.contains("next:"), "{text}");
        assert!(text.contains("blocked"), "{text}");
        assert!(text.contains("need a decision"), "{text}");
        assert!(text.contains("↑2"), "{text}");
        assert!(text.contains("~3"), "{text}");
        assert!(text.contains("#241"), "{text}");
        assert!(text.contains("recap stale"), "{text}");
    }

    #[test]
    fn no_pr_lifecycle_renders_no_pr_segment() {
        let mut c = card("prless");
        c.pr = Some((crate::git::forge::BranchLifecycle::NoPr, None));
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![c],
        }];
        let text = draw(&digest, 0, PaneFocus::Dashboard);
        assert!(!text.contains("PR"), "NoPr must render no chip: {text}");
        assert!(!text.contains("no pr"), "{text}");
    }

    #[test]
    fn selection_marker_follows_selected_index() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("first"), card("second")],
        }];
        let text = draw(&digest, 1, PaneFocus::ProjectManager);
        let sel_line = text.lines().find(|l| l.contains("second")).unwrap();
        assert!(sel_line.contains("▸"), "{text}");
        let other = text.lines().find(|l| l.contains("first")).unwrap();
        assert!(!other.contains("▸"), "{text}");
    }

    #[test]
    fn empty_digest_shows_placeholder() {
        let text = draw(&[], 0, PaneFocus::Dashboard);
        assert!(text.contains("no active workspaces"), "{text}");
    }

    #[test]
    fn title_hints_differ_by_focus() {
        let digest = vec![RepoDigest {
            repo_name: "alpha".into(),
            cards: vec![card("w")],
        }];
        assert!(draw(&digest, 0, PaneFocus::ProjectManager).contains("Enter attach"));
        assert!(draw(&digest, 0, PaneFocus::Dashboard).contains("Tab to focus"));
    }

    #[test]
    fn scroll_offset_keeps_selection_visible() {
        assert_eq!(scroll_offset(0, 5, 10, 30), 0, "fits at top");
        assert_eq!(scroll_offset(20, 25, 10, 30), 16, "scrolls to show sel_end");
        assert_eq!(
            scroll_offset(28, 33, 10, 30),
            20,
            "clamped to total - viewport"
        );
    }
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
            filter: None,
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
            filter: None,
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
            ws(4, 1, "boundary", WorkspaceState::Ready),
        ];
        let mut recaps = HashMap::new();
        let recap = |t| WorkspaceRecap {
            goal: Some("g".into()),
            state: Some("s".into()),
            next: Some("n".into()),
            updated_at: t,
        };
        recaps.insert(WorkspaceId(1), recap(1_000));
        recaps.insert(WorkspaceId(2), recap(1_000));
        recaps.insert(WorkspaceId(4), recap(1_000));
        let mut last_activity = HashMap::new();
        // Well past the slack window -> stale.
        last_activity.insert(WorkspaceId(1), 400_000);
        // Within the slack window (the recap-set tool call itself) -> fresh.
        last_activity.insert(WorkspaceId(2), 5_000);
        // No recap at all -> shown as missing, not stale.
        last_activity.insert(WorkspaceId(3), 5_000);
        // Exactly at the slack boundary -> NOT stale (strict `>`).
        last_activity.insert(WorkspaceId(4), 1_000 + RECAP_STALE_SLACK_MS);
        let inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &recaps,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &last_activity,
            filter: None,
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
        assert!(
            !by_name("boundary").recap_stale,
            "activity exactly at updated_at + slack must not be stale"
        );
    }

    #[test]
    fn no_recap_with_activity_present_is_not_stale() {
        let repos = vec![repo(1, "alpha")];
        let workspaces = vec![ws(1, 1, "norecap-active", WorkspaceState::Ready)];
        let recaps = HashMap::new();
        let mut last_activity = HashMap::new();
        last_activity.insert(WorkspaceId(1), 5_000);
        let inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &recaps,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &last_activity,
            filter: None,
        };
        let digest = build_digest(&inputs);
        let card = &digest[0].cards[0];
        assert!(card.recap.is_none());
        assert!(!card.recap_stale);
    }

    #[test]
    fn filter_matches_names_case_insensitively_and_omits_empty_repos() {
        let repos = vec![repo(1, "alpha"), repo(2, "beta")];
        let workspaces = vec![
            ws(1, 1, "auth-refactor", WorkspaceState::Ready),
            ws(2, 1, "docs-pass", WorkspaceState::Ready),
            // repo 2 has no matching workspaces -> omitted entirely
            ws(3, 2, "site-copy", WorkspaceState::Ready),
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
            filter: Some("AUTH"),
        };
        let digest = build_digest(&inputs);
        assert_eq!(digest.len(), 1);
        assert_eq!(digest[0].repo_name, "alpha");
        let names: Vec<_> = digest[0].cards.iter().map(|c| c.name.clone()).collect();
        assert_eq!(names, ["auth-refactor"]);
    }

    #[test]
    fn empty_or_absent_filter_is_a_noop() {
        let repos = vec![repo(1, "alpha")];
        let workspaces = vec![
            ws(1, 1, "one", WorkspaceState::Ready),
            ws(2, 1, "two", WorkspaceState::Ready),
        ];
        let empty = HashMap::new();
        let mut inputs = DigestInputs {
            repos: &repos,
            workspaces: &workspaces,
            recaps: &empty,
            pushed_status: &HashMap::new(),
            git: &HashMap::new(),
            pr_lifecycle: &HashMap::new(),
            pr_number: &HashMap::new(),
            last_activity_ms: &HashMap::new(),
            filter: None,
        };
        assert_eq!(card_count(&build_digest(&inputs)), 2);
        inputs.filter = Some("");
        assert_eq!(card_count(&build_digest(&inputs)), 2);
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
            filter: None,
        };
        let digest = build_digest(&inputs);
        assert_eq!(card_count(&digest), 2);
        assert_eq!(card_at(&digest, 0).unwrap().name, "a1");
        assert_eq!(card_at(&digest, 1).unwrap().name, "b1");
        assert!(card_at(&digest, 2).is_none());
    }
}
