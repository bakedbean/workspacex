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
    fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint {
        // When a recap is pinned, request enough rows for prompt (1) +
        // trace (1) + a 3-line recap minimum + footer (2). Without a
        // recap the original 3-row minimum is plenty.
        let has_recap = ctx
            .events
            .and_then(|e| e.last_completed_turn_text.as_deref())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if has_recap {
            Constraint::Min(7)
        } else {
            Constraint::Min(3)
        }
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::style::{Modifier, Style};
        use ratatui::text::{Line, Span};
        use ratatui::widgets::Paragraph;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let created_at_secs = (ctx.workspace.created_at.max(0) / 1000) as u64;
        let created_secs = now_secs.saturating_sub(created_at_secs);

        let events = if ctx.events_scanned { ctx.events } else { None };
        let theme = ctx.theme;
        let status = ctx.status;
        let column_width = area.width as usize;
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
                        let inner_width = column_width.saturating_sub(2).max(1);
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
                    Span::raw(truncate_to_chars(&trace, column_width.saturating_sub(2)))
                };
                out.push(Line::from(vec![prefix.clone(), trace_body]));

                // Recap: pinned at the previous turn's end; cleaned and
                // budgeted to whatever vertical room remains after the
                // prompt and trace, reserving 2 lines for the
                // created/active footer.
                if let Some(recap) = evt.last_completed_turn_text.as_deref() {
                    let trimmed = recap.trim();
                    if !trimmed.is_empty() {
                        let inner_width = column_width.saturating_sub(2).max(1);
                        let wrapped = wrap_lines(trimmed, inner_width);
                        let max_lines = (area.height as usize)
                            .saturating_sub(out.len())
                            .saturating_sub(2)
                            .max(1);
                        let truncated = wrapped.len() > max_lines;
                        for (i, line_text) in wrapped.iter().take(max_lines).enumerate() {
                            let leader = if i == 0 {
                                prefix.clone()
                            } else {
                                continuation.clone()
                            };
                            let body_text = if truncated && i == max_lines - 1 {
                                let mut s =
                                    truncate_to_chars(line_text, inner_width.saturating_sub(1));
                                s.push('…');
                                s
                            } else {
                                line_text.clone()
                            };
                            out.push(Line::from(vec![
                                leader,
                                Span::styled(body_text, theme.dim_style()),
                            ]));
                        }
                    }
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
    use crate::events::WorkspaceEvents;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

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
    fn height_hint_is_min_three() {
        let ctx = stub_context();
        assert_eq!(SessionSummary.height_hint(&ctx), Constraint::Min(3));
    }

    #[test]
    fn height_hint_grows_when_recap_is_present() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            last_completed_turn_text: Some("a recap".into()),
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        // The recap module signals it wants more vertical room so the
        // host layout reserves enough rows for prompt + trace + recap +
        // footer.
        assert_eq!(SessionSummary.height_hint(&ctx), Constraint::Min(7));
    }

    #[test]
    fn render_includes_recap_text_when_set() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            first_user_text: Some("prompt".into()),
            last_completed_turn_text: Some("RECAPMARKER rendered out.".into()),
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 10);
        assert!(
            text.contains("RECAPMARKER"),
            "expected recap text in rendered output, got:\n{text}"
        );
    }

    #[test]
    fn render_recap_uses_available_height_beyond_three_lines() {
        // Recap that wraps to many lines; area has plenty of vertical
        // room. The render should expand the recap budget to use
        // available height rather than always trimming to 3 lines.
        //
        // With width 20 / inner_width 18, the wrap groups four 3-char
        // tokens per line: line1=A-D, line2=E-H, line3=I-L, line4=M-P,
        // line5=Q-T, line6=U-X, line7=Y-Z. "MMM" lands on line 4 and
        // is excluded by any 3-line cap.
        let long_recap = "AAA BBB CCC DDD EEE FFF GGG HHH III JJJ KKK LLL MMM NNN OOO PPP \
                          QQQ RRR SSS TTT UUU VVV WWW XXX YYY ZZZ";
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            first_user_text: Some("prompt".into()),
            last_completed_turn_text: Some(long_recap.into()),
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 20, 20);
        assert!(
            text.contains("MMM"),
            "expected wrapped recap to extend past line 3 (look for 'MMM'); got:\n{text}"
        );
    }

    #[test]
    fn render_recap_truncates_when_no_room_for_more_lines() {
        // Recap that wraps to many lines but area is small enough that
        // we can't fit everything plus the created/active footer.
        // The recap should clip and the footer should still appear.
        let long_recap = "AAA BBB CCC DDD EEE FFF GGG HHH III JJJ KKK LLL MMM NNN OOO";
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            first_user_text: Some("prompt".into()),
            last_completed_turn_text: Some(long_recap.into()),
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 20, 7);
        // Footer survives: "created" is the prefix on the created line.
        assert!(text.contains("created"), "footer missing:\n{text}");
    }

    #[test]
    fn render_omits_recap_block_when_unset() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            first_user_text: Some("prompt".into()),
            // last_completed_turn_text intentionally None.
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 10);
        assert!(text.contains("prompt"), "prompt missing:\n{text}");
        assert!(
            !text.contains("RECAPMARKER"),
            "unexpected recap text:\n{text}"
        );
    }
}
