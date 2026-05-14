use crate::app::SelectionTarget;
use crate::store::{Repo, SetupStatus, Workspace, WorkspaceState};
use crate::ui::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

#[derive(Debug, Clone)]
pub enum Item<'a> {
    Header {
        repo: &'a Repo,
    },
    Workspace {
        repo: &'a Repo,
        workspace: &'a Workspace,
        session_running: bool,
        seconds_since_activity: Option<u64>,
        has_prior_session: bool,
        status: Option<crate::git::WorkspaceStatus>,
        latest_event: Option<crate::events::EventSnapshot>,
        needs_attention: bool,
    },
    EmptyHint,
    Spacer,
}

#[derive(Default)]
pub struct DashboardState {
    pub selected: usize,
    pub list_state: ListState,
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    items: &[Item],
    selected: Option<SelectionTarget>,
    nerd_fonts: bool,
    state: &mut DashboardState,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    let header_text = Paragraph::new("wsx — Workspaces").style(theme::header());
    f.render_widget(header_text, chunks[0]);

    let mut selected_idx: Option<usize> = None;
    let mut list_items: Vec<ListItem> = Vec::with_capacity(items.len());
    for item in items.iter() {
        match item {
            Item::Header { repo } => {
                if let Some(SelectionTarget::Repo(id)) = selected
                    && id == repo.id
                {
                    selected_idx = Some(list_items.len());
                }
                let line = format!("▌ {}    {}", repo.name, repo.path.display());
                list_items.push(ListItem::new(line).style(theme::header()));
            }
            Item::Workspace {
                repo: _,
                workspace,
                session_running,
                seconds_since_activity,
                has_prior_session,
                status,
                latest_event,
                needs_attention,
            } => {
                if let Some(SelectionTarget::Workspace(id)) = selected
                    && id == workspace.id
                {
                    // The main workspace row is selectable; the sub-line
                    // below it is not.
                    selected_idx = Some(list_items.len());
                }
                let dot = match (*session_running, &workspace.state, *has_prior_session) {
                    (true, _, _) => "●",
                    (false, WorkspaceState::Failed, _) => "✕",
                    (false, _, true) => "↻",
                    _ => "○",
                };
                let setup_badge = match workspace.setup_status {
                    SetupStatus::Ok | SetupStatus::Skipped | SetupStatus::NotRun => "",
                    SetupStatus::Failed => " [setup-failed]",
                };
                let activity = match (*seconds_since_activity, *has_prior_session) {
                    (Some(s), _) if s < 2 => "active",
                    (Some(s), _) if s < 30 => "idle",
                    (Some(_), _) => "waiting",
                    (None, true) => "resumable",
                    (None, false) => "off",
                };
                let branch_label = format_branch_label(&workspace.branch, nerd_fonts);
                let status_str = status
                    .map(|s| format_status(&s, nerd_fonts))
                    .unwrap_or_default();
                let attn = if *needs_attention { "!" } else { " " };
                let line = format!(
                    "{attn} {dot} {name}  [{branch_label}]  {status_str:<14} {activity}{setup_badge}",
                    name = workspace.name,
                );
                list_items.push(ListItem::new(line));
                if let Some(ev) = latest_event {
                    let age = format_age(ev.timestamp_ms);
                    let sub = format!("    \u{2514} {} ({})", ev.display, age);
                    list_items.push(ListItem::new(sub).style(theme::dim()));
                }
            }
            Item::EmptyHint => {
                list_items.push(
                    ListItem::new("  (no workspaces — press n to create one)").style(theme::dim()),
                );
            }
            Item::Spacer => list_items.push(ListItem::new("")),
        }
    }

    state.list_state.select(selected_idx);
    let list = List::new(list_items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_style(theme::selected());
    f.render_stateful_widget(list, chunks[1], &mut state.list_state);

    let footer = Paragraph::new(
        "[enter] attach   [n] new   [e] edit   [t] terminal   [d] archive   [q] quit",
    )
    .style(theme::dim());
    f.render_widget(footer, chunks[2]);
}

fn format_status(status: &crate::git::WorkspaceStatus, nerd: bool) -> String {
    if status.is_clean() {
        return String::new();
    }
    let mut parts: Vec<String> = Vec::new();
    if status.modified > 0 {
        parts.push(if nerd {
            format!("\u{f459} {}", status.modified)
        } else {
            format!("~{}", status.modified)
        });
    }
    if status.untracked > 0 {
        parts.push(if nerd {
            format!("\u{f128} {}", status.untracked)
        } else {
            format!("?{}", status.untracked)
        });
    }
    if status.ahead > 0 {
        parts.push(if nerd {
            format!("\u{f062}{}", status.ahead)
        } else {
            format!("\u{2191}{}", status.ahead)
        });
    }
    if status.behind > 0 {
        parts.push(if nerd {
            format!("\u{f063}{}", status.behind)
        } else {
            format!("\u{2193}{}", status.behind)
        });
    }
    parts.join(" ")
}

/// Relative time label for an event timestamp ("3s ago", "2m ago", "1h ago").
fn format_age(timestamp_ms: i64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let secs = ((now_ms - timestamp_ms) / 1000).max(0);
    if secs < 60 {
        format!("{secs}s ago")
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else {
        format!("{}h ago", secs / 3600)
    }
}

fn format_branch_label(branch: &str, nerd: bool) -> String {
    if nerd {
        format!("\u{e0a0} {branch}")
    } else {
        branch.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{RepoId, WorkspaceId};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn repo(id: i64, name: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: name.into(),
            path: PathBuf::from(format!("/repos/{name}")),
            branch_prefix: "".into(),
            custom_instructions: None,
            created_at: 0,
        }
    }

    fn workspace(id: i64, repo_id: i64, name: &str, branch: &str) -> Workspace {
        Workspace {
            id: WorkspaceId(id),
            repo_id: RepoId(repo_id),
            name: name.into(),
            branch: branch.into(),
            worktree_path: PathBuf::from(format!("/w/{name}")),
            state: WorkspaceState::Ready,
            setup_status: SetupStatus::Ok,
            created_at: 0,
        }
    }

    fn dump(term: &Terminal<TestBackend>, w: u16, h: u16) -> String {
        let buf = term.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            let line: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
            s.push_str(line.trim_end());
            s.push('\n');
        }
        s
    }

    #[test]
    fn renders_repo_header_with_indented_workspace() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| {
            render(
                f,
                f.area(),
                &items,
                Some(SelectionTarget::Workspace(WorkspaceId(1))),
                false,
                &mut state,
            )
        })
        .unwrap();
        let text = dump(&term, 120, 8);
        assert!(text.contains("▌ demo"), "missing header: {text}");
        assert!(text.contains("alpha"), "missing workspace name: {text}");
        assert!(text.contains("active"), "missing activity column: {text}");
    }

    #[test]
    fn renders_empty_repo_hint() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).unwrap();
        let r = repo(1, "empty");
        let items = vec![Item::Header { repo: &r }, Item::EmptyHint];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &mut state))
            .unwrap();
        let text = dump(&term, 80, 8);
        assert!(text.contains("▌ empty"));
        assert!(text.contains("press n to create"));
    }

    #[test]
    fn renders_multiple_repos_grouped() {
        let mut term = Terminal::new(TestBackend::new(120, 15)).unwrap();
        let r1 = repo(1, "first");
        let r2 = repo(2, "second");
        let w1 = workspace(1, 1, "alpha", "alpha");
        let w2 = workspace(2, 2, "beta", "beta");
        let items = vec![
            Item::Header { repo: &r1 },
            Item::Workspace {
                repo: &r1,
                workspace: &w1,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
            },
            Item::Spacer,
            Item::Header { repo: &r2 },
            Item::Workspace {
                repo: &r2,
                workspace: &w2,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &mut state))
            .unwrap();
        let text = dump(&term, 120, 15);
        let first_pos = text.find("first").expect("first repo header");
        let alpha_pos = text.find("alpha").expect("alpha workspace");
        let second_pos = text.find("second").expect("second repo header");
        let beta_pos = text.find("beta").expect("beta workspace");
        assert!(
            first_pos < alpha_pos && alpha_pos < second_pos && second_pos < beta_pos,
            "ordering wrong:\n{text}"
        );
    }

    #[test]
    fn renders_status_counts_plain() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let st = crate::git::WorkspaceStatus {
            modified: 3,
            untracked: 1,
            ahead: 2,
            behind: 0,
        };
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: Some(st),
                latest_event: None,
                needs_attention: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        assert!(text.contains("~3"), "missing modified count: {text}");
        assert!(text.contains("?1"), "missing untracked count: {text}");
        assert!(text.contains("\u{2191}2"), "missing ahead count: {text}");
        assert!(
            !text.contains("\u{2193}"),
            "should not show zero behind: {text}"
        );
    }

    #[test]
    fn renders_status_counts_nerd() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let st = crate::git::WorkspaceStatus {
            modified: 2,
            untracked: 0,
            ahead: 0,
            behind: 1,
        };
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: Some(st),
                latest_event: None,
                needs_attention: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, true, &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        assert!(text.contains("\u{e0a0}"), "missing branch glyph: {text}");
        assert!(text.contains("\u{f459}"), "missing modified glyph: {text}");
        assert!(text.contains("\u{f063}"), "missing behind glyph: {text}");
    }

    #[test]
    fn renders_event_subline_when_event_present() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        // Timestamp ~5s ago to exercise the seconds branch of format_age.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let ev = crate::events::EventSnapshot {
            kind: crate::events::EventKind::AssistantToolUse,
            display: "ran `cargo test`".into(),
            timestamp_ms: now_ms - 5_000,
        };
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: Some(ev),
                needs_attention: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        assert!(text.contains("\u{2514}"), "missing └ glyph: {text}");
        assert!(
            text.contains("ran `cargo test`"),
            "missing event body: {text}"
        );
        assert!(text.contains("s ago"), "missing relative time: {text}");
    }

    #[test]
    fn selection_skips_event_subline() {
        // When a workspace has a sub-line, the second workspace's main row
        // should still get the correct selection highlight index — i.e.
        // selecting workspace 2 highlights row 3 (header=0, ws1=1, sub=2, ws2=3),
        // not row 2 (the sub-line).
        let r = repo(1, "demo");
        let w1 = workspace(1, 1, "alpha", "wsx/alpha");
        let w2 = workspace(2, 1, "beta", "wsx/beta");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let ev = crate::events::EventSnapshot {
            kind: crate::events::EventKind::AssistantText,
            display: "thinking…".into(),
            timestamp_ms: now_ms - 1_000,
        };
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w1,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: None,
                latest_event: Some(ev),
                needs_attention: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w2,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
            },
        ];
        let mut term = Terminal::new(TestBackend::new(120, 10)).unwrap();
        let mut state = DashboardState::default();
        term.draw(|f| {
            render(
                f,
                f.area(),
                &items,
                Some(SelectionTarget::Workspace(WorkspaceId(2))),
                false,
                &mut state,
            )
        })
        .unwrap();
        // The second workspace becomes the 4th list item (index 3): header,
        // ws1 row, ws1 sub-line, ws2 row.
        assert_eq!(state.list_state.selected(), Some(3));
    }

    #[test]
    fn renders_clean_workspace_with_no_status() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let st = crate::git::WorkspaceStatus::default();
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: Some(st),
                latest_event: None,
                needs_attention: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        assert!(text.contains("alpha"));
        // Clean workspace should not show any count markers.
        assert!(!text.contains("~"));
        assert!(!text.contains("?"));
    }

    /// Strip leading list/border decoration so tests can assert on the
    /// rendered row's own first character.
    fn strip_border_prefix(line: &str) -> &str {
        // Skip the left border glyph (│) and any whitespace immediately after it.
        line.trim_start_matches('\u{2502}').trim_start_matches(' ')
    }

    #[test]
    fn renders_attention_mark_when_needs_attention() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: true,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        // Look for the row that has the alpha workspace; assert ! is in the leading column.
        let line = text
            .lines()
            .find(|l| l.contains("alpha"))
            .expect("alpha row");
        let trimmed = strip_border_prefix(line);
        assert!(
            trimmed.starts_with("!"),
            "expected leading ! in: {trimmed:?}"
        );
    }

    #[test]
    fn no_attention_mark_by_default() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w,
                session_running: false,
                seconds_since_activity: None,
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let line = text
            .lines()
            .find(|l| l.contains("alpha"))
            .expect("alpha row");
        let trimmed = strip_border_prefix(line);
        assert!(!trimmed.starts_with("!"));
    }
}
