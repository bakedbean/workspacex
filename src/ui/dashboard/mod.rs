use crate::app::SelectionTarget;
use crate::store::{Repo, SetupStatus, Workspace, WorkspaceState};
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

// Column widths for the workspace row. Names and branches are truncated
// or right-padded so the columns align vertically across rows.
const NAME_WIDTH: usize = 32;
const BRANCH_BLOCK_WIDTH: usize = 48;

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
        /// Why the agent paused. `None` when no stop_reason or when
        /// the user has already replied.
        stopped_kind: Option<crate::app::StoppedKind>,
        /// True when Claude has stalled mid-tool-chain: the JSONL log
        /// hasn't been appended for >60s, no tool_use is pending, and
        /// at least one stop_reason has been observed. Catches sessions
        /// where Claude crashes/hangs after a tool_result without ever
        /// writing a terminal stop_reason.
        stalled: bool,
        /// Number of processes detected with `cwd` inside this
        /// workspace's worktree (sourced from `app.workspace_processes`).
        /// Rendered inline as `~N` in merged-style when nonzero; hidden
        /// when zero.
        proc_count: usize,
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
                stopped_kind,
                stalled,
                proc_count,
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
                    *stopped_kind,
                    *stalled,
                    *proc_count,
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
        "n: new, N: new (permissive), e: edit, t: terminal, v: diff, k: procs, s: settings, d: archive, q: quit",
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

/// Build the top summary line: `wsx · N workspaces[ · K permission][ · Q question][ · C complete][ · S stalled]`.
/// State suffixes are omitted when their count is zero. `wsx` uses the header
/// style; ` · `, the numeric totals, and the labels use dim style. Alertable
/// counts (`permission`, `question`, `stalled`) use warn for the numeric value;
/// `complete` uses ok.
fn top_summary_line(items: &[Item], theme: &Theme) -> Line<'static> {
    let mut total = 0usize;
    let mut awaiting = 0usize;
    let mut question = 0usize;
    let mut complete = 0usize;
    let mut stalled_n = 0usize;
    for item in items {
        if let Item::Workspace {
            awaiting_tool,
            stopped_kind,
            stalled,
            ..
        } = item
        {
            total += 1;
            // Priority matches `classify_activity_with_events`: awaiting >
            // stopped_kind > stalled. A workspace with multiple flags
            // counts only toward its highest-priority bucket.
            if awaiting_tool.is_some() {
                awaiting += 1;
            } else {
                match stopped_kind {
                    Some(crate::app::StoppedKind::AwaitingAnswer) => question += 1,
                    Some(crate::app::StoppedKind::Complete) => complete += 1,
                    None if *stalled => stalled_n += 1,
                    None => {}
                }
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
        spans.push(Span::styled(" permission".to_string(), theme.dim_style()));
    }
    if question > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{question}"), theme.warn_style()));
        spans.push(Span::styled(" question".to_string(), theme.dim_style()));
    }
    if complete > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{complete}"), theme.ok_style()));
        spans.push(Span::styled(" complete".to_string(), theme.dim_style()));
    }
    if stalled_n > 0 {
        spans.push(Span::styled(" · ".to_string(), theme.dim_style()));
        spans.push(Span::styled(format!("{stalled_n}"), theme.warn_style()));
        spans.push(Span::styled(" stalled".to_string(), theme.dim_style()));
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
            theme.path_style(),
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
        "awaiting" | "question" | "stalled" => theme.warn_style(),
        "complete" | "active" => theme.ok_style(),
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
    stopped_kind: Option<crate::app::StoppedKind>,
    stalled: bool,
    proc_count: usize,
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
    } else {
        match stopped_kind {
            Some(crate::app::StoppedKind::AwaitingAnswer) => "question",
            Some(crate::app::StoppedKind::Complete) => "complete",
            None if stalled => "stalled",
            None => match (seconds_since_activity, has_prior_session) {
                (Some(s), _) if s < 2 => "active",
                (Some(s), _) if s < 30 => "idle",
                (Some(_), _) => "waiting",
                (None, true) => "resumable",
                (None, false) => "off",
            },
        }
    };
    // Age source: the most recent of awaiting_tool.first_seen_ms and
    // (implicit) latest event isn't available here, so we use 0 as a
    // sentinel — the sub-line carries the latest event's age.
    let age_ms = match awaiting_tool {
        Some((_, ts)) => *ts,
        None => 0,
    };
    // When setup failed, reserve 3 chars (" ⚙!") at the end of the name
    // column and truncate the name to 17 chars so the total stays at 20.
    let setup_failed = workspace.setup_status == SetupStatus::Failed;
    let name_padded = if setup_failed {
        // No styled span here yet — we emit the badge as a separate styled
        // span below so it gets err coloring.
        truncate_pad(&workspace.name, NAME_WIDTH - 3)
    } else {
        truncate_pad(&workspace.name, NAME_WIDTH)
    };
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

    let attn = if needs_attention {
        match (awaiting_tool.is_some(), stopped_kind, stalled, nerd) {
            // Permission prompt — single character, both nerd + ascii.
            (true, _, _, _) => "!",
            // Question — nerd-font question circle vs ascii fallback.
            (false, Some(crate::app::StoppedKind::AwaitingAnswer), _, true) => "\u{f128}",
            (false, Some(crate::app::StoppedKind::AwaitingAnswer), _, false) => "?",
            // Complete — nerd-font check circle vs ascii fallback.
            (false, Some(crate::app::StoppedKind::Complete), _, true) => "\u{f058}",
            (false, Some(crate::app::StoppedKind::Complete), _, false) => "\u{2713}",
            // Stalled — keep `!`.
            (false, None, true, _) => "!",
            // Defensive default — needs_attention is true but no specific cause;
            // fall back to `!` so attention is always visible.
            (false, None, false, _) => "!",
        }
    } else {
        " "
    };
    let attn_style = match (awaiting_tool.is_some(), stopped_kind) {
        (false, Some(crate::app::StoppedKind::Complete)) => theme.ok_style(),
        _ => theme.warn_style(),
    };

    // Left side: indent + attn + glyph + name + gutter + branch + gutter + git
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::styled(attn.to_string(), attn_style));
    spans.push(Span::raw(format!(" {dot} ")));
    if workspace.yolo {
        // YOLO workspaces auto-approve every tool use; warn-style the name
        // so it's identifiable at a glance in the dashboard list.
        spans.push(Span::styled(name_padded, theme.warn_style()));
    } else {
        spans.push(Span::raw(name_padded));
    }
    if setup_failed {
        // NOTE: this err_style fg is suppressed when the row is selected
        // (ratatui's highlight_style patches the fg). The glyph still
        // appears on the selected row, just without the red coloring.
        spans.push(Span::styled(" ⚙!".to_string(), theme.err_style()));
    }
    spans.push(Span::raw("   ".to_string()));
    match branch_style {
        Some(style) => spans.push(Span::styled(branch_padded, style)),
        None => spans.push(Span::raw(branch_padded)),
    }
    spans.push(Span::raw("   ".to_string()));
    if !git_status.is_empty() {
        spans.push(Span::styled(git_status, theme.dim_style()));
    }

    if proc_count > 0 {
        spans.push(Span::styled(
            format!(" ~{proc_count}"),
            theme.merged_style(),
        ));
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
mod tests;

#[cfg(test)]
mod label_tests;
