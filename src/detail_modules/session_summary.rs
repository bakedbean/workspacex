//! Session summary module. Shows the agent's current status, last
//! activity, and tool-use trace for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};
use crate::ui::dashboard::column_content::{format_ago_short, format_state_line, format_tool_trace};

pub struct SessionSummary;

impl DetailModule for SessionSummary {
    fn id(&self) -> &'static str {
        "session_summary"
    }
    fn title(&self) -> &'static str {
        "SESSION SUMMARY"
    }

    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
        build_lines(ctx, width)
    }
}

fn build_lines(ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};

    // Pull `Duration` once so `now_ms` and `now_secs` share the
    // same time base. The rest of the codebase uses
    // `as_millis() as i64` for epoch-ms (see `app.rs`,
    // `app/background.rs`); deriving `now_ms` from `as_secs() * 1000`
    // would truncate sub-second precision and skew the 3s threshold
    // in `pending_permission_tool`.
    let now_duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let now_ms = now_duration.as_millis() as i64;
    let now_secs = now_duration.as_secs();
    let created_at_secs = (ctx.workspace.created_at.max(0) / 1000) as u64;
    let created_secs = now_secs.saturating_sub(created_at_secs);

    let events = if ctx.events_scanned { ctx.events } else { None };
    let theme = ctx.theme;
    let status = ctx.status;
    let column_width = width as usize;
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
                Span::styled(
                    truncate_to_chars(&state_text, inner_width),
                    theme.dim_style(),
                ),
            ]));

            // Recent files: 1–3 basenames from the edited-files ring.
            // Omitted when the ring is empty so we don't reserve a row
            // for a meaningless dash.
            if let Some(files_text) = format_recent_files(&evt.recent_edited_files, inner_width) {
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

    out
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
    use crate::activity::events::{StopReason, WorkspaceEvents};
    use crate::detail_modules::tests_helpers::stub_context;
    use crate::ui::dashboard::status::Status;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_to_text(ctx: &DetailContext<'_>, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                use ratatui::widgets::Paragraph;
                let area = ratatui::layout::Rect::new(0, 0, w, h);
                let lines = SessionSummary.lines(ctx, w);
                f.render_widget(Paragraph::new(lines), area);
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

    // -- state line + recent files ----------------------------------

    #[test]
    fn render_shows_status_label() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents::default()));
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
        // render() derives `now_ms` from SystemTime::now(); we can't
        // easily inject "now" without a refactor, so seed the
        // pending_tool_uses timestamp at epoch 0 to guarantee
        // (now_ms - timestamp) far exceeds the 3s threshold.
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
        // Use the production helper so the ring's most-recent-first
        // ordering matches what real edits produce.
        let mut evt = WorkspaceEvents::default();
        for path in ["/abs/path/to/alpha.rs", "relative/beta.rs", "gamma.rs"] {
            evt.push_recent_edited_file(path.to_string());
        }
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(evt));
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
    fn render_files_line_caps_at_three_most_recent() {
        // Use the production helper so the ring is most-recent-first
        // (push_front). With 5 push-fronts in this order, the front
        // ends up: five, four, three, two, one. The cap takes the
        // front 3 → the 3 most-recently-edited files.
        let mut evt = WorkspaceEvents::default();
        for name in ["one.rs", "two.rs", "three.rs", "four.rs", "five.rs"] {
            evt.push_recent_edited_file(name.to_string());
        }
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(evt));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 10);
        assert!(
            text.contains("five.rs"),
            "missing most-recent file:\n{text}"
        );
        assert!(text.contains("four.rs"), "missing 2nd-most-recent:\n{text}");
        assert!(
            text.contains("three.rs"),
            "missing 3rd-most-recent:\n{text}"
        );
        assert!(
            !text.contains("two.rs"),
            "expected list capped at 3 most-recent; found older entry:\n{text}"
        );
        assert!(
            !text.contains("one.rs"),
            "expected list capped at 3 most-recent; found oldest entry:\n{text}"
        );
    }

    #[test]
    fn render_omits_files_line_when_ring_empty() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents::default()));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 10);
        assert!(
            !text.contains("files:"),
            "expected no files line when ring is empty:\n{text}"
        );
    }

    #[test]
    fn lines_loading_state_emits_loading_line() {
        let mut ctx = stub_context();
        ctx.events_scanned = false;
        let out = SessionSummary.lines(&ctx, 40);
        assert!(!out.is_empty());
        // First line in the loading branch is "  loading…" in dim style.
        let first_text: String = out[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            first_text.contains("loading"),
            "expected 'loading' line, got: {first_text:?}"
        );
    }
}
