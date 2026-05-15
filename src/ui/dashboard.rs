use crate::app::SelectionTarget;
use crate::store::{Repo, Workspace, WorkspaceState};
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

// Column widths for the workspace row. Names and branches are truncated
// or right-padded so the columns align vertically across rows.
const NAME_WIDTH: usize = 20;
const BRANCH_BLOCK_WIDTH: usize = 28;

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
        lifecycle: Option<crate::forge::BranchLifecycle>,
        /// Set when a tool_use has been pending ≥3s (almost always a
        /// permission prompt). Carries (tool name, first-seen epoch ms) so
        /// we can render the elapsed wait time in the sub-line.
        awaiting_tool: Option<(String, i64)>,
        /// True when the assistant's most recent `stop_reason` indicates
        /// the agent finished its turn and is awaiting human input
        /// (`end_turn`, `max_tokens`, `stop_sequence`) and the user has
        /// not yet replied. Distinct from `awaiting_tool` (permission
        /// prompts), which has higher priority in the activity column.
        stopped: bool,
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
    theme: &Theme,
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

    f.render_widget(Paragraph::new(top_summary_line(items, theme)), chunks[0]);

    // No outer border anymore — the list spans the full width of chunks[1].
    let inner_width = chunks[1].width as usize;

    // Count workspaces between each Item::Header. We can't simply count
    // by repo.id during the render loop because we need the count BEFORE
    // emitting the header line.
    let mut counts_by_repo_idx: Vec<usize> = Vec::new();
    {
        let mut current: Option<usize> = None;
        for item in items.iter() {
            match item {
                Item::Header { .. } => {
                    counts_by_repo_idx.push(0);
                    current = Some(counts_by_repo_idx.len() - 1);
                }
                Item::Workspace { .. } => {
                    if let Some(i) = current {
                        counts_by_repo_idx[i] += 1;
                    }
                }
                _ => {}
            }
        }
    }
    let mut repo_idx = 0usize;

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
                let count = counts_by_repo_idx.get(repo_idx).copied().unwrap_or(0);
                repo_idx += 1;
                let (header, rule) = repo_header_lines(repo, count, inner_width, theme);
                list_items.push(ListItem::new(header));
                list_items.push(ListItem::new(rule));
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
                lifecycle,
                awaiting_tool,
                stopped,
            } => {
                if let Some(SelectionTarget::Workspace(id)) = selected
                    && id == workspace.id
                {
                    selected_idx = Some(list_items.len());
                }
                let main = workspace_main_row(
                    workspace,
                    *session_running,
                    *seconds_since_activity,
                    *has_prior_session,
                    *status,
                    *needs_attention,
                    *lifecycle,
                    awaiting_tool,
                    *stopped,
                    nerd_fonts,
                    theme,
                    inner_width,
                );
                list_items.push(ListItem::new(main));
                // Sub-line: if awaiting, render the permission prompt;
                // otherwise fall back to latest event. Setup-failed glyph
                // lives in the main row's name column in Task 5.
                if let Some((tool_name, first_seen_ms)) = awaiting_tool {
                    let age = format_age(*first_seen_ms);
                    let sub = format!(
                        "      \u{2514} \u{26a0} awaiting permission: {} ({})",
                        tool_name, age
                    );
                    list_items.push(ListItem::new(sub).style(theme.dim_style()));
                } else if let Some(ev) = latest_event {
                    let age = format_age(ev.timestamp_ms);
                    let sub = format!("      \u{2514} {} ({})", ev.display, age);
                    list_items.push(ListItem::new(sub).style(theme.dim_style()));
                }
            }
            Item::EmptyHint => {
                list_items.push(
                    ListItem::new("  (no workspaces — press n to create one)")
                        .style(theme.dim_style()),
                );
            }
            Item::Spacer => list_items.push(ListItem::new("")),
        }
    }

    state.list_state.select(selected_idx);
    let list = List::new(list_items).highlight_style(theme.selected_style());
    f.render_stateful_widget(list, chunks[1], &mut state.list_state);

    let footer = Paragraph::new(
        "[enter] attach   [n] new   [e] edit   [t] terminal   [d] archive   [q] quit",
    )
    .style(theme.dim_style());
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

/// Build the top summary line: `wsx · N workspaces[ · K awaiting][ · M stopped]`.
/// State suffixes are omitted when their count is zero. `wsx` uses the header
/// style; ` · `, the numeric totals, and the labels use dim style — except
/// alertable counts (`awaiting`, `stopped`), whose numeric value uses warn.
fn top_summary_line(items: &[Item], theme: &Theme) -> Line<'static> {
    let mut total = 0usize;
    let mut awaiting = 0usize;
    let mut stopped_n = 0usize;
    for item in items {
        if let Item::Workspace {
            awaiting_tool,
            stopped,
            ..
        } = item
        {
            total += 1;
            // Priority matches `classify_activity_with_events`: awaiting wins
            // over stopped, so a workspace with both flags counts toward
            // `awaiting` only (it renders as `awaiting` in the activity column).
            if awaiting_tool.is_some() {
                awaiting += 1;
            } else if *stopped {
                stopped_n += 1;
            }
        }
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("wsx".to_string(), theme.header_style()));
    spans.push(Span::styled(
        format!(" · {total} workspace{}", if total == 1 { "" } else { "s" }),
        theme.dim_style(),
    ));
    if awaiting > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{awaiting}"), theme.warn_style()));
        spans.push(Span::styled(" awaiting".to_string(), theme.dim_style()));
    }
    if stopped_n > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{stopped_n}"), theme.warn_style()));
        spans.push(Span::styled(" stopped".to_string(), theme.dim_style()));
    }
    Line::from(spans)
}

/// Build the two-line block that introduces a repo group:
///   `<name> · <path> · <count>`
///   `─────────────────────────...`
fn repo_header_lines(
    repo: &Repo,
    count: usize,
    inner_width: usize,
    theme: &Theme,
) -> (Line<'static>, Line<'static>) {
    let header = Line::from(vec![
        Span::styled(repo.name.clone(), theme.header_style()),
        Span::styled(
            format!(" · {} · {}", repo.path.display(), count),
            theme.dim_style(),
        ),
    ]);
    let rule_text: String = "─".repeat(inner_width);
    let rule = Line::from(Span::styled(rule_text, theme.dim_style()));
    (header, rule)
}

/// Render the bracketed branch label as a `Line` whose glyph + name are
/// styled per PR lifecycle. Returning a `Line` (rather than `String`) lets
/// the row composer apply per-segment colors while still measuring the
/// displayed width for right-justified padding.
fn format_branch_label(
    branch: &str,
    nerd: bool,
    lifecycle: Option<crate::forge::BranchLifecycle>,
    theme: &Theme,
) -> Line<'static> {
    use crate::forge::BranchLifecycle::*;
    let text = if nerd {
        let (glyph, suffix) = match lifecycle {
            None | Some(NoPr) => ("\u{e0a0}", ""),
            Some(PrOpen) | Some(PrConflicted) => ("\u{f407}", ""),
            Some(PrDraft) => ("\u{f407}", " draft"),
            Some(PrMerged) => ("\u{f419}", ""),
            Some(PrClosed) => ("\u{f659}", ""),
        };
        format!("{glyph} {branch}{suffix}")
    } else {
        let suffix = match lifecycle {
            Some(PrOpen) => " (pr)",
            Some(PrDraft) => " (draft)",
            Some(PrConflicted) => " (conflict)",
            Some(PrMerged) => " (merged)",
            Some(PrClosed) => " (closed)",
            None | Some(NoPr) => "",
        };
        format!("{branch}{suffix}")
    };
    let style = match lifecycle {
        Some(PrOpen) => Some(theme.ok_style()),
        Some(PrConflicted) => Some(theme.warn_style()),
        Some(PrMerged) => Some(theme.merged_style()),
        Some(PrClosed) => Some(theme.err_style()),
        // Draft / NoPr / None render in the default foreground.
        _ => None,
    };
    let span = match style {
        Some(s) => Span::styled(text, s),
        None => Span::raw(text),
    };
    Line::from(span)
}

/// Right-pad `s` with spaces to `target` chars. If `s` is longer, truncate
/// to `target - 1` chars and append `…`. char-count based (handles
/// multi-byte chars correctly for the alignment math we care about).
fn truncate_pad(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len == target {
        s.to_string()
    } else if len < target {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(target - len));
        out
    } else {
        let mut out: String = s.chars().take(target.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Compact relative-time label for the right-side age column: `5s`, `12s`,
/// `5m`, `1h`. Returns `—` (em-dash) when timestamp is 0 (sentinel for "no
/// meaningful age").
fn format_age_compact(timestamp_ms: i64) -> String {
    if timestamp_ms <= 0 {
        return "—".to_string();
    }
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let secs = ((now_ms - timestamp_ms) / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

/// Map an activity word to a style (color) per the spec.
fn activity_style(label: &str, theme: &Theme) -> Style {
    match label {
        "awaiting" | "stopped" => theme.warn_style(),
        "active" => theme.ok_style(),
        "idle" => Style::default(),
        "waiting" | "resumable" | "off" => theme.dim_style(),
        _ => Style::default(),
    }
}

/// Compose a workspace's main row as a `Line` of spans with fixed columns.
/// Right-justifies the activity + age at the inner-width edge.
#[allow(clippy::too_many_arguments)]
fn workspace_main_row(
    workspace: &Workspace,
    session_running: bool,
    seconds_since_activity: Option<u64>,
    has_prior_session: bool,
    status: Option<crate::git::WorkspaceStatus>,
    needs_attention: bool,
    lifecycle: Option<crate::forge::BranchLifecycle>,
    awaiting_tool: &Option<(String, i64)>,
    stopped: bool,
    nerd: bool,
    theme: &Theme,
    inner_width: usize,
) -> Line<'static> {
    let dot = match (session_running, &workspace.state, has_prior_session) {
        (true, _, _) => "●",
        (false, WorkspaceState::Failed, _) => "✕",
        (false, _, true) => "↻",
        _ => "○",
    };
    let activity = if awaiting_tool.is_some() {
        "awaiting"
    } else if stopped {
        "stopped"
    } else {
        match (seconds_since_activity, has_prior_session) {
            (Some(s), _) if s < 2 => "active",
            (Some(s), _) if s < 30 => "idle",
            (Some(_), _) => "waiting",
            (None, true) => "resumable",
            (None, false) => "off",
        }
    };
    // Age source: the most recent of awaiting_tool.first_seen_ms and
    // (implicit) latest event isn't available here, so we use 0 as a
    // sentinel — the sub-line carries the latest event's age.
    let age_ms = match awaiting_tool {
        Some((_, ts)) => *ts,
        None => 0,
    };
    let name_padded = truncate_pad(&workspace.name, NAME_WIDTH);
    let branch_line = format_branch_label(&workspace.branch, nerd, lifecycle, theme);
    // Take the styled spans from branch_line; pad/truncate to BRANCH_BLOCK_WIDTH.
    let branch_concat: String = branch_line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    let branch_style = branch_line
        .spans
        .iter()
        .find_map(|s| s.style.fg)
        .map(|fg| Style::default().fg(fg));
    let branch_padded = truncate_pad(&branch_concat, BRANCH_BLOCK_WIDTH);
    let git_status = status.map(|s| format_status(&s, nerd)).unwrap_or_default();
    let age = format_age_compact(age_ms);

    let attn = if needs_attention { "!" } else { " " };

    // Left side: indent + attn + glyph + name + gutter + branch + gutter + git
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(attn.to_string(), theme.warn_style()));
    spans.push(Span::raw(format!(" {dot} ")));
    spans.push(Span::raw(name_padded));
    spans.push(Span::raw("   ".to_string()));
    match branch_style {
        Some(style) => spans.push(Span::styled(branch_padded, style)),
        None => spans.push(Span::raw(branch_padded)),
    }
    spans.push(Span::raw("   ".to_string()));
    if !git_status.is_empty() {
        spans.push(Span::styled(git_status, theme.dim_style()));
    }

    // Right side: activity + space + age
    let right_text_w = activity.chars().count() + 1 + age.chars().count();
    let left_w: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    let gap = inner_width.saturating_sub(left_w + right_text_w).max(1);
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(
        activity.to_string(),
        activity_style(activity, theme),
    ));
    spans.push(Span::raw(" ".to_string()));
    spans.push(Span::styled(age, theme.dim_style()));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{RepoId, SetupStatus, WorkspaceId};
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
            setup_script: None,
            archive_script: None,
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

    fn t() -> Theme {
        Theme::default_theme()
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
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
                &t(),
                &mut state,
            )
        })
        .unwrap();
        let text = dump(&term, 120, 8);
        assert!(
            text.contains("demo") && text.contains("/repos/demo"),
            "missing header: {text}"
        );
        assert!(text.contains("alpha"), "missing workspace name: {text}");
        assert!(text.contains("active"), "missing activity column: {text}");
    }

    #[test]
    fn renders_empty_repo_hint() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).unwrap();
        let r = repo(1, "empty");
        let items = vec![Item::Header { repo: &r }, Item::EmptyHint];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 80, 8);
        assert!(text.contains("empty") && text.contains("/repos/empty"));
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, true, &t(), &mut state))
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
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
                &t(),
                &mut state,
            )
        })
        .unwrap();
        // The second workspace becomes the 5th list item (index 4): header,
        // rule, ws1 row, ws1 sub-line, ws2 row.
        assert_eq!(state.list_state.selected(), Some(4));
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let line = text
            .lines()
            .find(|l| l.contains("alpha"))
            .expect("alpha row");
        let trimmed = strip_border_prefix(line);
        assert!(!trimmed.starts_with("!"));
    }

    #[test]
    fn activity_is_right_justified() {
        // 120-wide terminal so the long row doesn't overflow and get
        // truncated — the contract being tested is alignment of the
        // "active" column, not overflow behaviour.
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w_short = workspace(1, 1, "a", "p/a");
        let w_long = workspace(
            2,
            1,
            "very-long-workspace-name-here",
            "prefix/very-long-workspace-name-here",
        );
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w_short,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w_long,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);

        // The "active" word should end at roughly the same column for both rows.
        let lines: Vec<&str> = text.lines().collect();
        let row_a = lines
            .iter()
            .find(|l| l.contains(" a ") || l.contains("│ a"))
            .expect("row a");
        let row_long = lines
            .iter()
            .find(|l| l.contains("very-long-workspac"))
            .expect("row long");
        // Find the *char* column where "active" starts in each row. Using char
        // indices avoids false drift from multi-byte glyphs (e.g. `…`) that
        // appear in truncated names/branches.
        let char_pos = |row: &str, needle: &str| -> usize {
            let byte_pos = row
                .find(needle)
                .unwrap_or_else(|| panic!("{needle} not in: {row}"));
            row[..byte_pos].chars().count()
        };
        let col_a = char_pos(row_a, "active");
        let col_long = char_pos(row_long, "active");
        // Allow ±2 chars tolerance for unicode glyph cell width quirks.
        let diff = (col_a as isize - col_long as isize).abs();
        assert!(
            diff <= 2,
            "activity column drifted: a={col_a} long={col_long}, lines:\n{text}"
        );
    }

    #[test]
    fn top_summary_shows_total_and_alertable_counts() {
        let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
        let r = repo(1, "demo");
        let w_quiet = workspace(1, 1, "quiet", "wsx/quiet");
        let w_awaiting = workspace(2, 1, "blocked", "wsx/blocked");
        let w_stopped = workspace(3, 1, "thinking", "wsx/thinking");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w_quiet,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w_awaiting,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: true,
                lifecycle: None,
                awaiting_tool: Some(("Bash".into(), 0)),
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w_stopped,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: true,
                lifecycle: None,
                awaiting_tool: None,
                stopped: true,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 12);
        let top = text.lines().next().unwrap().trim();
        assert!(top.contains("wsx"), "missing 'wsx': {top}");
        assert!(top.contains("3 workspaces"), "missing total: {top}");
        assert!(top.contains("1 awaiting"), "missing awaiting count: {top}");
        assert!(top.contains("1 stopped"), "missing stopped count: {top}");
    }

    #[test]
    fn top_summary_omits_zero_alertable_counts() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let top = text.lines().next().unwrap().trim();
        assert!(top.contains("1 workspace"), "missing total: {top}");
        assert!(
            !top.contains("awaiting"),
            "unexpected awaiting in quiet top: {top}"
        );
        assert!(
            !top.contains("stopped"),
            "unexpected stopped in quiet top: {top}"
        );
    }

    #[test]
    fn top_summary_handles_zero_workspaces() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let items: Vec<Item> = vec![];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let top = text.lines().next().unwrap().trim();
        assert!(top.contains("wsx"), "missing wsx: {top}");
        assert!(top.contains("0 workspaces"), "expected zero count: {top}");
        assert!(!top.contains("awaiting"), "unexpected awaiting: {top}");
        assert!(!top.contains("stopped"), "unexpected stopped: {top}");
    }

    #[test]
    fn outer_border_is_absent() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let buf = term.backend().buffer();
        // No vertical-bar border glyphs at either edge.
        let max_x = buf.area.width - 1;
        for y in 0..8u16 {
            assert_ne!(
                buf[(0u16, y)].symbol(),
                "│",
                "expected no border at col 0, row {y}"
            );
            assert_ne!(
                buf[(max_x, y)].symbol(),
                "│",
                "expected no border at right edge col {max_x}, row {y}"
            );
        }
    }

    #[test]
    fn repo_header_renders_with_rule_below() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let lines: Vec<&str> = text.lines().collect();
        // Find the repo header line; the next non-empty line should be a rule.
        let hdr_idx = lines
            .iter()
            .position(|l| l.contains("demo") && l.contains("/repos/demo"))
            .expect("repo header line");
        let rule = lines[hdr_idx + 1];
        let rule_chars: Vec<char> = rule.chars().filter(|c| !c.is_whitespace()).collect();
        assert!(
            !rule_chars.is_empty() && rule_chars.iter().all(|c| *c == '─'),
            "expected horizontal rule under header, got: {rule:?}"
        );
    }

    #[test]
    fn repo_header_includes_workspace_count() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w1 = workspace(1, 1, "alpha", "wsx/alpha");
        let w2 = workspace(2, 1, "beta", "wsx/beta");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w1,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w2,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let hdr = text
            .lines()
            .find(|l| l.contains("demo") && l.contains("/repos/demo"))
            .expect("repo header line");
        assert!(
            hdr.contains("· 2"),
            "expected workspace count in header: {hdr}"
        );
    }

    #[test]
    fn renders_awaiting_overrides_activity_and_sub_line() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
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
                needs_attention: true,
                lifecycle: None,
                // 10s ago — well past the 3s threshold.
                awaiting_tool: Some(("Bash".into(), now_ms - 10_000)),
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        assert!(
            text.contains("awaiting"),
            "expected 'awaiting' label: {text}"
        );
        assert!(
            text.contains("awaiting permission: Bash"),
            "expected sub-line: {text}"
        );
        // Should NOT show 'active' even though seconds_since_activity is 0.
        let bad = text
            .lines()
            .any(|l| l.contains("alpha") && l.contains("active"));
        assert!(!bad, "should not show 'active' when awaiting: {text}");
    }

    #[test]
    fn workspace_row_name_padded_to_fixed_width() {
        let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
        let r = repo(1, "demo");
        let w_short = workspace(1, 1, "ab", "wsx/ab");
        let w_long = workspace(2, 1, "much-longer-name", "wsx/much-longer-name");
        let items = vec![
            Item::Header { repo: &r },
            Item::Workspace {
                repo: &r,
                workspace: &w_short,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
            Item::Workspace {
                repo: &r,
                workspace: &w_long,
                session_running: true,
                seconds_since_activity: Some(0),
                has_prior_session: false,
                status: None,
                latest_event: None,
                needs_attention: false,
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let buf = term.backend().buffer();
        // Find the y of each workspace row by scanning the buffer for the
        // name. Then check that the glyph after the name column starts at
        // the same x on both rows.
        let find_y = |needle: &str| -> u16 {
            for y in 0..12u16 {
                let row: String = (0..120u16)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect();
                if row.contains(needle) {
                    return y;
                }
            }
            panic!("not found: {needle}");
        };
        let y_short = find_y("ab ");
        let y_long = find_y("much-longer-name");
        // Branch column should start at the same x on both rows.
        // x = indent(2) + attn(1) + sep(1) + dot(1) + sep(1) + name + gutter(3)
        let probe_x: u16 = (2 + 1 + 1 + 1 + 1 + NAME_WIDTH + 3) as u16;
        // After truncation/padding, both rows' branch glyph should appear at
        // probe_x — the branch glyph differs but its starting x should match.
        let short_at = buf[(probe_x, y_short)].symbol();
        let long_at = buf[(probe_x, y_long)].symbol();
        // Both should be non-space (the branch glyph or first branch char).
        assert!(
            short_at != " " && long_at != " ",
            "branch column misaligned: short={short_at:?} long={long_at:?}"
        );
    }

    #[test]
    fn workspace_row_branch_truncated_with_ellipsis() {
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let very_long_branch = "feat/the-quick-brown-fox-jumps-over-the-lazy-dog";
        let w = workspace(1, 1, "alpha", very_long_branch);
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let text = dump(&term, 120, 8);
        let row = text
            .lines()
            .find(|l| l.contains("alpha"))
            .expect("alpha row");
        assert!(
            row.contains('…'),
            "expected branch ellipsis truncation: {row}"
        );
    }

    #[test]
    fn activity_word_uses_warn_color_for_stopped() {
        // Direct unit test of the style mapping.
        let theme = Theme::default_theme();
        let style_stopped = activity_style("stopped", &theme);
        let style_awaiting = activity_style("awaiting", &theme);
        assert_eq!(style_stopped.fg, Some(theme.warn));
        assert_eq!(style_awaiting.fg, Some(theme.warn));
    }

    #[test]
    fn activity_word_uses_ok_color_for_active() {
        let theme = Theme::default_theme();
        let style = activity_style("active", &theme);
        assert_eq!(style.fg, Some(theme.ok));
    }

    #[test]
    fn activity_word_uses_dim_for_off_and_resumable() {
        let theme = Theme::default_theme();
        assert_eq!(activity_style("off", &theme).fg, Some(theme.dim));
        assert_eq!(activity_style("resumable", &theme).fg, Some(theme.dim));
    }

    #[test]
    fn activity_word_uses_default_for_idle() {
        let theme = Theme::default_theme();
        assert_eq!(activity_style("idle", &theme).fg, None);
    }

    #[test]
    fn sub_line_indent_aligns_with_name_column() {
        use crate::events::{EventKind, EventSnapshot};
        let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
        let r = repo(1, "demo");
        let w = workspace(1, 1, "alpha", "wsx/alpha");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let ev = EventSnapshot {
            kind: EventKind::AssistantText,
            display: "hello".into(),
            timestamp_ms: now - 5_000,
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
                lifecycle: None,
                awaiting_tool: None,
                stopped: false,
            },
        ];
        let mut state = DashboardState::default();
        term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
            .unwrap();
        let buf = term.backend().buffer();
        // Find the sub-line row (contains "hello") and confirm the └ glyph
        // is at column 6.
        let mut sub_y = None;
        for y in 0..8u16 {
            let row: String = (0..120u16)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if row.contains("hello") && row.contains('└') {
                sub_y = Some(y);
                break;
            }
        }
        let y = sub_y.expect("sub-line not found");
        assert_eq!(buf[(6u16, y)].symbol(), "└");
    }
}

#[cfg(test)]
mod label_tests {
    use super::*;
    use crate::forge::BranchLifecycle;

    fn line_text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn line_fg(l: &Line) -> Option<ratatui::style::Color> {
        l.spans.iter().find_map(|s| s.style.fg)
    }

    #[test]
    fn nerd_no_lifecycle_uses_branch_glyph() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", true, None, &t);
        assert_eq!(line_text(&l), "\u{e0a0} feat/x");
        assert_eq!(line_fg(&l), None);
    }

    #[test]
    fn nerd_open_pr_uses_pr_glyph_and_ok_color() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrOpen), &t);
        assert_eq!(line_text(&l), "\u{f407} feat/x");
        assert_eq!(line_fg(&l), Some(t.ok));
    }

    #[test]
    fn nerd_draft_pr_annotates_and_stays_uncolored() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrDraft), &t);
        assert_eq!(line_text(&l), "\u{f407} feat/x draft");
        assert_eq!(line_fg(&l), None);
    }

    #[test]
    fn nerd_conflicted_pr_uses_pr_glyph_and_warn_color() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrConflicted), &t);
        assert_eq!(line_text(&l), "\u{f407} feat/x");
        assert_eq!(line_fg(&l), Some(t.warn));
    }

    #[test]
    fn nerd_merged_pr_uses_merge_glyph_and_merged_color() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrMerged), &t);
        assert_eq!(line_text(&l), "\u{f419} feat/x");
        assert_eq!(line_fg(&l), Some(t.merged));
    }

    #[test]
    fn nerd_closed_pr_uses_x_glyph_and_err_color() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrClosed), &t);
        assert_eq!(line_text(&l), "\u{f659} feat/x");
        assert_eq!(line_fg(&l), Some(t.err));
    }

    #[test]
    fn nerd_no_pr_uses_branch_glyph_uncolored() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", true, Some(BranchLifecycle::NoPr), &t);
        assert_eq!(line_text(&l), "\u{e0a0} feat/x");
        assert_eq!(line_fg(&l), None);
    }

    #[test]
    fn ascii_open_pr_appends_pr_suffix() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrOpen), &t);
        assert_eq!(line_text(&l), "feat/x (pr)");
        assert_eq!(line_fg(&l), Some(t.ok));
    }

    #[test]
    fn ascii_draft_pr_appends_draft_suffix_uncolored() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrDraft), &t);
        assert_eq!(line_text(&l), "feat/x (draft)");
        assert_eq!(line_fg(&l), None);
    }

    #[test]
    fn ascii_conflicted_pr_appends_conflict_suffix() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrConflicted), &t);
        assert_eq!(line_text(&l), "feat/x (conflict)");
        assert_eq!(line_fg(&l), Some(t.warn));
    }

    #[test]
    fn ascii_merged_pr_appends_merged_suffix() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrMerged), &t);
        assert_eq!(line_text(&l), "feat/x (merged)");
        assert_eq!(line_fg(&l), Some(t.merged));
    }

    #[test]
    fn ascii_closed_pr_appends_closed_suffix() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrClosed), &t);
        assert_eq!(line_text(&l), "feat/x (closed)");
        assert_eq!(line_fg(&l), Some(t.err));
    }

    #[test]
    fn ascii_no_pr_is_plain_uncolored() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", false, Some(BranchLifecycle::NoPr), &t);
        assert_eq!(line_text(&l), "feat/x");
        assert_eq!(line_fg(&l), None);
    }

    #[test]
    fn ascii_none_is_plain_uncolored() {
        let t = Theme::default_theme();
        let l = format_branch_label("feat/x", false, None, &t);
        assert_eq!(line_text(&l), "feat/x");
        assert_eq!(line_fg(&l), None);
    }
}
