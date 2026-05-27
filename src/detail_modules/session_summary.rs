//! Session summary module. Shows the agent's current status, last
//! activity, and tool-use trace for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct SessionSummary;

impl DetailModule for SessionSummary {
    fn id(&self) -> &'static str {
        "session_summary"
    }
    fn title(&self) -> &'static str {
        "SESSION SUMMARY"
    }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        // Prompt (1) + trace (1) + state (1) + files (1) + footer (2).
        // Prompt and files may be absent or wrap, but Min(5) covers
        // the typical case while letting the layout collapse when
        // space is tight.
        Constraint::Min(5)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let now_ms = (now_secs as i64).saturating_mul(1000);
        let created_at_secs = (ctx.workspace.created_at.max(0) / 1000) as u64;
        let created_secs = now_secs.saturating_sub(created_at_secs);

        let events = if ctx.events_scanned { ctx.events } else { None };
        let theme = ctx.theme;
        let status = ctx.status;
        let column_width = area.width as usize;
        let inner_width = column_width.saturating_sub(2).max(1);
        let ago_secs = ctx.ago_secs;

        let mut out: Vec<Line<'static>> = Vec::new();

        // Bullet prefix takes the workspace's status color so the SESSION
        // SUMMARY column visually echoes the row's status gutter.
        let prefix = Span::styled("▸ ".to_string(), theme.status_style(status));
        // Continuation indent for wrapped/multi-line prompts: 2 cells, so
        // wrapped lines align with the first character of the prompt text.
        let continuation = Span::raw("  ".to_string());

        match events {
            None => {
                out.push(Line::from(Span::styled(
                    "  loading…".to_string(),
                    theme.dim_style(),
                )));
            }
            Some(evt) => {
                if let Some(prompt) = evt.first_user_text.as_deref() {
                    let trimmed = prompt.trim();
                    if !trimmed.is_empty() {
                        // Respect `\n` from the original prompt AND wrap long lines
                        // to the column width so the prompt is fully readable.
                        let wrapped = wrap_lines(trimmed, inner_width);
                        let italic = Style::default().add_modifier(Modifier::ITALIC);
                        for (i, line_text) in wrapped.iter().enumerate() {
                            let leader = if i == 0 {
                                prefix.clone()
                            } else {
                                continuation.clone()
                            };
                            out.push(Line::from(vec![
                                leader,
                                Span::styled(line_text.clone(), italic),
                            ]));
                        }
                    }
                }

                let trace = format_tool_trace(&evt.tool_use_counts);
                let trace_body = if trace.is_empty() {
                    Span::styled("—".to_string(), theme.dim_style())
                } else {
                    Span::raw(truncate_to_chars(&trace, inner_width))
                };
                out.push(Line::from(vec![prefix.clone(), trace_body]));

                // State line: canonical status label, optionally enriched
                // with a why-detail (pending tool, stall duration) that
                // RECENT CHAT can't surface.
                let state_text = format_state_line(status, evt, now_ms);
                out.push(Line::from(vec![
                    prefix.clone(),
                    Span::styled(truncate_to_chars(&state_text, inner_width), theme.dim_style()),
                ]));

                // Recent files: 1–3 basenames from the edited-files ring.
                // Omitted when the ring is empty so we don't reserve a row
                // for a meaningless dash.
                if let Some(files_text) = format_recent_files(&evt.recent_edited_files, inner_width)
                {
                    out.push(Line::from(vec![
                        prefix.clone(),
                        Span::styled(files_text, theme.dim_style()),
                    ]));
                }
            }
        }

        let created_text = format!("created {} ago", format_ago_short(Some(created_secs)));
        out.push(Line::from(vec![
            prefix.clone(),
            Span::styled(created_text, theme.dim_style()),
        ]));

        let active_text = match ago_secs {
            Some(s) => format!("active {} ago", format_ago_short(Some(s))),
            None => "active —".to_string(),
        };
        out.push(Line::from(vec![
            prefix.clone(),
            Span::styled(active_text, theme.dim_style()),
        ]));

        frame.render_widget(Paragraph::new(out), area);
    }
}

/// Canonical status label, optionally enriched with a why-detail. The
/// suffix is drawn from evt fields that explain the current state —
/// pending question/permission tool for `Question`, quiet duration for
/// `Stalled`. Other states use the bare label.
fn format_state_line(
    status: crate::ui::dashboard::status::Status,
    evt: &crate::events::WorkspaceEvents,
    now_ms: i64,
) -> String {
    use crate::ui::dashboard::status::Status;
    let base = status.label();
    let detail: Option<String> = match status {
        Status::Question => evt
            .pending_question_tool()
            .map(|n| n.to_string())
            .or_else(|| {
                evt.pending_permission_tool(now_ms, 3_000)
                    .map(|(name, _)| name)
            }),
        Status::Stalled => {
            if evt.last_log_activity_ms > 0 {
                let quiet_secs = now_ms
                    .saturating_sub(evt.last_log_activity_ms)
                    .max(0) as u64
                    / 1000;
                Some(format!("{} quiet", format_ago_short(Some(quiet_secs))))
            } else {
                None
            }
        }
        Status::Waiting | Status::Thinking | Status::Complete | Status::Idle => None,
    };
    match detail {
        Some(d) => format!("{base} · {d}"),
        None => base.to_string(),
    }
}

/// Render the most-recently-edited files as up to 3 basenames, joined
/// with commas under a "files:" label. Returns None when the ring is
/// empty so the caller can skip the line entirely.
fn format_recent_files(
    files: &std::collections::VecDeque<String>,
    max_width: usize,
) -> Option<String> {
    if files.is_empty() {
        return None;
    }
    let basenames: Vec<String> = files
        .iter()
        .take(3)
        .map(|p| {
            std::path::Path::new(p)
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| p.clone())
        })
        .collect();
    Some(truncate_to_chars(
        &format!("files: {}", basenames.join(", ")),
        max_width,
    ))
}

fn format_ago_short(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s"),
        Some(s) if s < 3600 => format!("{}m", s / 60),
        Some(s) => format!("{}h", s / 3600),
    }
}

fn format_tool_trace(counts: &crate::events::ToolUseCounts) -> String {
    let mut parts: Vec<String> = Vec::new();
    if counts.read > 0 {
        parts.push(format!(
            "read {} {}",
            counts.read,
            plural("file", counts.read)
        ));
    }
    if counts.edit > 0 {
        parts.push(format!(
            "edited {} {}",
            counts.edit,
            plural("file", counts.edit)
        ));
    }
    if counts.write > 0 {
        parts.push(format!(
            "wrote {} {}",
            counts.write,
            plural("file", counts.write)
        ));
    }
    if counts.bash > 0 {
        parts.push(format!(
            "ran {} {}",
            counts.bash,
            plural("command", counts.bash)
        ));
    }
    if counts.other > 0 {
        parts.push(format!("+{} other actions", counts.other));
    }
    parts.join(", ")
}

fn plural(noun: &str, n: u32) -> String {
    if n == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}

fn truncate_to_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// Greedy word-wrap. Splits long words at the column boundary.
fn wrap_lines(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            if word.chars().count() > width {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
                let mut buf: String = String::new();
                for ch in word.chars() {
                    if buf.chars().count() == width {
                        out.push(std::mem::take(&mut buf));
                    }
                    buf.push(ch);
                }
                if !buf.is_empty() {
                    current = buf;
                }
                continue;
            }
            let projected = if current.is_empty() {
                word.chars().count()
            } else {
                current.chars().count() + 1 + word.chars().count()
            };
            if projected > width {
                out.push(std::mem::take(&mut current));
                current.push_str(word);
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            out.push(current);
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;
    use crate::events::{StopReason, WorkspaceEvents};
    use crate::ui::dashboard::status::Status;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::collections::VecDeque;

    fn render_to_text(ctx: &DetailContext<'_>, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let area = ratatui::layout::Rect::new(0, 0, w, h);
                SessionSummary.render(area, ctx, f);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            for x in 0..w {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn id_is_session_summary() {
        assert_eq!(SessionSummary.id(), "session_summary");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(SessionSummary.title(), "SESSION SUMMARY");
    }

    #[test]
    fn height_hint_is_min_five() {
        // Prompt + trace + state + files + footer (bottom row) → 5.
        let ctx = stub_context();
        assert_eq!(SessionSummary.height_hint(&ctx), Constraint::Min(5));
    }

    // -- state line + recent files ----------------------------------

    #[test]
    fn render_shows_status_label() {
        let evt: &'static WorkspaceEvents =
            Box::leak(Box::new(WorkspaceEvents::default()));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;
        ctx.status = Status::Thinking;

        let text = render_to_text(&ctx, 60, 10);
        assert!(
            text.contains("thinking"),
            "expected status label in output:\n{text}"
        );
    }

    #[test]
    fn render_state_line_appends_pending_permission_tool_when_question() {
        // pending_permission_tool requires the tool_use to be ≥3s old.
        // last_log_activity_ms drives the time base used in render() via
        // SystemTime::now(); we can't easily inject "now" without a
        // refactor, so seed the pending_tool_uses timestamp far enough
        // in the past that any plausible `now_ms` returns it.
        let mut pending = std::collections::HashMap::new();
        pending.insert("tu_1".to_string(), ("Bash".to_string(), 0_i64));
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            pending_tool_uses: pending,
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;
        ctx.status = Status::Question;

        let text = render_to_text(&ctx, 60, 10);
        assert!(text.contains("question"), "missing label:\n{text}");
        assert!(text.contains("Bash"), "missing pending tool:\n{text}");
    }

    #[test]
    fn render_state_line_prefers_question_tool_over_permission_tool() {
        // Both an AskUserQuestion and a generic permission-prompt tool
        // are pending. The state line must surface the question tool
        // (it's the more specific signal).
        let mut pending = std::collections::HashMap::new();
        pending.insert("tu_q".to_string(), ("AskUserQuestion".to_string(), 0_i64));
        pending.insert("tu_b".to_string(), ("Bash".to_string(), 0_i64));
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            pending_tool_uses: pending,
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;
        ctx.status = Status::Question;

        let text = render_to_text(&ctx, 60, 10);
        assert!(
            text.contains("AskUserQuestion"),
            "expected question tool to win over permission tool:\n{text}"
        );
    }

    #[test]
    fn render_state_line_shows_stall_duration() {
        // For Stalled, the state line should append a "quiet" duration
        // derived from now_ms - last_log_activity_ms. We can't fix
        // wall-clock now here, so just assert that the line gets a
        // " · " suffix beyond the bare "stalled" label — proves the
        // duration branch ran.
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            // 1ms after epoch — far enough in the past that any plausible
            // `now_ms` produces a non-zero quiet duration.
            last_log_activity_ms: 1,
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;
        ctx.status = Status::Stalled;

        let text = render_to_text(&ctx, 60, 10);
        assert!(text.contains("stalled"), "missing stalled label:\n{text}");
        assert!(
            text.contains(" · "),
            "expected ' · <duration>' suffix on stalled line:\n{text}"
        );
    }

    #[test]
    fn render_lists_recent_files_as_basenames() {
        let mut ring = VecDeque::new();
        ring.push_back("/abs/path/to/alpha.rs".to_string());
        ring.push_back("relative/beta.rs".to_string());
        ring.push_back("gamma.rs".to_string());
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            recent_edited_files: ring,
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 10);
        assert!(text.contains("files:"), "missing files label:\n{text}");
        assert!(text.contains("alpha.rs"), "missing alpha basename:\n{text}");
        assert!(text.contains("beta.rs"), "missing beta basename:\n{text}");
        assert!(text.contains("gamma.rs"), "missing gamma basename:\n{text}");
        assert!(
            !text.contains("/abs/path/to/"),
            "expected basename only, found full path:\n{text}"
        );
    }

    #[test]
    fn render_files_line_caps_at_three_entries() {
        let mut ring = VecDeque::new();
        for name in ["one.rs", "two.rs", "three.rs", "four.rs", "five.rs"] {
            ring.push_back(name.to_string());
        }
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            recent_edited_files: ring,
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 10);
        assert!(text.contains("one.rs"), "missing 1st file:\n{text}");
        assert!(text.contains("three.rs"), "missing 3rd file:\n{text}");
        assert!(
            !text.contains("four.rs"),
            "expected files list capped at 3, found 4th:\n{text}"
        );
    }

    #[test]
    fn render_omits_files_line_when_ring_empty() {
        let evt: &'static WorkspaceEvents =
            Box::leak(Box::new(WorkspaceEvents::default()));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 10);
        assert!(
            !text.contains("files:"),
            "expected no files line when ring is empty:\n{text}"
        );
    }
}
