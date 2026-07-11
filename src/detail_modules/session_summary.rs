//! Session summary module. Shows the agent's current status, last
//! activity, and tool-use trace for the selected workspace.

use crate::activity::events::WorkspaceEvents;
use crate::detail_modules::{DetailContext, DetailModule};
use crate::ui::dashboard::column_content::{
    format_ago_short, format_state_line, format_tool_trace,
};

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

            // Model line: which model the session is running on. Shown
            // as soon as the model id is known, independent of token
            // data, so it appears even before the first usage block.
            if let Some(model_id) = evt.model_id.as_deref() {
                let model_text = format!("model: {}", short_model_label(model_id));
                out.push(Line::from(vec![
                    prefix.clone(),
                    Span::styled(
                        truncate_to_chars(&model_text, inner_width),
                        theme.dim_style(),
                    ),
                ]));
            }

            // Context-window fill: a live signal the row never shows.
            if let Some((ctx_text, warn)) = format_context_line(evt) {
                let style = if warn {
                    theme.warn_style()
                } else {
                    theme.dim_style()
                };
                out.push(Line::from(vec![
                    prefix.clone(),
                    Span::styled(truncate_to_chars(&ctx_text, inner_width), style),
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

/// Abbreviate a token count as `950` / `77k` / `1M` / `1.2M`. The `k` form
/// floors (77_999 → "77k"); exact precision is meaningless for a fill gauge.
fn abbreviate_tokens(n: u64) -> String {
    if n < 1_000 {
        n.to_string()
    } else if n < 1_000_000 {
        format!("{}k", n / 1_000)
    } else {
        let m = n as f64 / 1_000_000.0;
        if (m - m.round()).abs() < 0.05 {
            format!("{}M", m.round() as u64)
        } else {
            format!("{m:.1}M")
        }
    }
}

/// Resolve the context-window size for a model id. Known families default
/// to 200k; if the current fill already exceeds that, treat the session as
/// the 1M variant (the model id doesn't encode the variant). Unknown or
/// absent model → None (render raw tokens without a percentage).
///
/// The `>` is strict: exactly 200k stays on the 200k window (100%, warn), and
/// 200_001 flips to the 1M window (20%). That discontinuity is intended — a
/// session past 200k provably isn't on a 200k-window model.
fn resolve_window(context_tokens: u64, model_id: Option<&str>) -> Option<u64> {
    let base = model_id.and_then(|m| {
        if m.contains("opus") || m.contains("sonnet") || m.contains("haiku") {
            Some(200_000u64)
        } else {
            None
        }
    })?;
    Some(if context_tokens > base {
        1_000_000
    } else {
        base
    })
}

/// A short display label for a model id: the Claude family word plus its
/// version, e.g. `claude-opus-4-8[1m]` → `opus 4.8`, `claude-sonnet-5` →
/// `sonnet 5`. The version is the run of short (1-2 digit) numeric segments
/// right after the family, so a trailing date segment like `20251001` is
/// ignored. Unknown / non-Claude ids fall back to the id with any leading
/// `claude-` stripped, truncated to 12 chars.
pub(crate) fn short_model_label(model_id: &str) -> String {
    // Drop a trailing bracketed variant tag like "[1m]".
    let base = model_id.split('[').next().unwrap_or(model_id);
    let segments: Vec<&str> = base.split('-').collect();
    let family_pos = segments
        .iter()
        .position(|s| matches!(*s, "opus" | "sonnet" | "haiku"));
    match family_pos {
        Some(i) => {
            let family = segments[i];
            let is_short_numeric =
                |s: &str| !s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit());
            let version: Vec<&str> = segments[i + 1..]
                .iter()
                .copied()
                .take_while(|s| is_short_numeric(s))
                .collect();
            if version.is_empty() {
                family.to_string()
            } else {
                format!("{} {}", family, version.join("."))
            }
        }
        None => {
            let cleaned = base.strip_prefix("claude-").unwrap_or(base);
            cleaned.chars().take(12).collect()
        }
    }
}

/// Build the detail bar's context-fill line and whether it should render in
/// the warn color. None when there's no token data yet (omit the line).
fn format_context_line(evt: &WorkspaceEvents) -> Option<(String, bool)> {
    // Treat a 0 sum (no usage yet, or a malformed all-zero usage block) the
    // same as "no data" — a `context: 0` line is noise, not signal.
    let n = evt.context_tokens.filter(|&n| n > 0)?;
    match resolve_window(n, evt.model_id.as_deref()) {
        Some(w) => {
            let pct = (n.saturating_mul(100) / w).min(999);
            let text = format!(
                "context: {} / {} · {}%",
                abbreviate_tokens(n),
                abbreviate_tokens(w),
                pct
            );
            Some((text, pct >= 85))
        }
        None => Some((
            format!("context: {} tokens", abbreviate_tokens(n)),
            n >= 150_000,
        )),
    }
}

/// The chat view's compact model + token-usage chip: `{label} {used}/{window}`
/// when the window is resolvable (e.g. `opus 4.8 45k/200k`), else
/// `{label} {used}` (raw tokens). The model label is omitted when `model_id`
/// is absent. Returns `(text, warn)`; `warn` mirrors the detail bar
/// (`format_context_line`): fill ≥ 85% of a known window, or raw tokens
/// ≥ 150k when the window is unknown. `None` when there's no token data.
pub(crate) fn format_chip_model_tokens(evt: &WorkspaceEvents) -> Option<(String, bool)> {
    let n = evt.context_tokens.filter(|&n| n > 0)?;
    let label = evt.model_id.as_deref().map(short_model_label);
    let (tokens_text, warn) = match resolve_window(n, evt.model_id.as_deref()) {
        Some(w) => {
            let pct = (n.saturating_mul(100) / w).min(999);
            (
                format!("{}/{}", abbreviate_tokens(n), abbreviate_tokens(w)),
                pct >= 85,
            )
        }
        None => (abbreviate_tokens(n), n >= 150_000),
    };
    let text = match label {
        Some(l) => format!("{l} {tokens_text}"),
        None => tokens_text,
    };
    Some((text, warn))
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

    #[test]
    fn abbreviate_tokens_uses_k_and_m() {
        assert_eq!(abbreviate_tokens(950), "950");
        assert_eq!(abbreviate_tokens(77_081), "77k");
        assert_eq!(abbreviate_tokens(200_000), "200k");
        assert_eq!(abbreviate_tokens(1_000_000), "1M");
        assert_eq!(abbreviate_tokens(1_250_000), "1.2M");
    }

    #[test]
    fn resolve_window_maps_known_models_and_upgrades_past_default() {
        assert_eq!(
            resolve_window(50_000, Some("claude-opus-4-8")),
            Some(200_000)
        );
        // current fill above the 200k default → treat as the 1M variant
        assert_eq!(
            resolve_window(250_000, Some("claude-opus-4-8")),
            Some(1_000_000)
        );
        assert_eq!(resolve_window(50_000, Some("some-unknown-model")), None);
        assert_eq!(resolve_window(50_000, None), None);
    }

    #[test]
    fn format_context_line_known_window_shows_percent() {
        let evt = WorkspaceEvents {
            context_tokens: Some(100_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_context_line(&evt).unwrap();
        assert_eq!(text, "context: 100k / 200k · 50%");
        assert!(!warn);
    }

    #[test]
    fn format_context_line_warns_near_limit() {
        let evt = WorkspaceEvents {
            context_tokens: Some(190_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (_text, warn) = format_context_line(&evt).unwrap();
        assert!(warn, "expected warn at 95% fill");
    }

    #[test]
    fn format_context_line_unknown_window_shows_raw_tokens() {
        let evt = WorkspaceEvents {
            context_tokens: Some(77_000),
            model_id: None,
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_context_line(&evt).unwrap();
        assert_eq!(text, "context: 77k tokens");
        assert!(!warn);
    }

    #[test]
    fn format_context_line_none_when_no_tokens() {
        let evt = WorkspaceEvents::default();
        assert!(format_context_line(&evt).is_none());
    }

    #[test]
    fn format_context_line_none_when_zero_tokens() {
        // A present-but-zero usage sum is noise, not a real fill — omit it.
        let evt = WorkspaceEvents {
            context_tokens: Some(0),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        assert!(format_context_line(&evt).is_none());
    }

    #[test]
    fn render_shows_model_line() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 12);
        assert!(
            text.contains("model: opus 4.8"),
            "missing model line:\n{text}"
        );
    }

    #[test]
    fn render_omits_model_line_when_model_unknown() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents::default()));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 12);
        assert!(
            !text.contains("model:"),
            "expected no model line without a model id:\n{text}"
        );
    }

    #[test]
    fn render_shows_context_fill_line() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents {
            context_tokens: Some(100_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        }));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;
        ctx.status = Status::Thinking;

        let text = render_to_text(&ctx, 60, 12);
        assert!(text.contains("context:"), "missing context line:\n{text}");
        assert!(text.contains("100k"), "missing token count:\n{text}");
    }

    #[test]
    fn render_omits_context_line_when_no_tokens() {
        let evt: &'static WorkspaceEvents = Box::leak(Box::new(WorkspaceEvents::default()));
        let mut ctx = stub_context();
        ctx.events = Some(evt);
        ctx.events_scanned = true;

        let text = render_to_text(&ctx, 60, 12);
        assert!(
            !text.contains("context:"),
            "expected no context line without token data:\n{text}"
        );
    }

    #[test]
    fn short_model_label_parses_family_and_version() {
        assert_eq!(short_model_label("claude-opus-4-8"), "opus 4.8");
        assert_eq!(short_model_label("claude-sonnet-5"), "sonnet 5");
        assert_eq!(short_model_label("claude-haiku-4-5"), "haiku 4.5");
    }

    #[test]
    fn short_model_label_strips_bracketed_variant() {
        assert_eq!(short_model_label("claude-opus-4-8[1m]"), "opus 4.8");
    }

    #[test]
    fn short_model_label_ignores_trailing_date_segment() {
        // The date segment (>2 digits) is not part of the version.
        assert_eq!(short_model_label("claude-haiku-4-5-20251001"), "haiku 4.5");
    }

    #[test]
    fn short_model_label_falls_back_for_unknown_ids() {
        // No known family word: strip a leading "claude-" and truncate to 12.
        assert_eq!(short_model_label("gpt-5-codex"), "gpt-5-codex");
        assert_eq!(
            short_model_label("some-really-long-unknown-model-id"),
            "some-really-"
        );
    }

    #[test]
    fn short_model_label_family_without_version() {
        assert_eq!(short_model_label("claude-opus"), "opus");
    }

    #[test]
    fn format_chip_model_tokens_known_window() {
        let evt = WorkspaceEvents {
            context_tokens: Some(45_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "opus 4.8 45k/200k");
        assert!(!warn);
    }

    #[test]
    fn format_chip_model_tokens_warns_past_85_percent() {
        let evt = WorkspaceEvents {
            context_tokens: Some(190_000),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "opus 4.8 190k/200k");
        assert!(warn);
    }

    #[test]
    fn format_chip_model_tokens_unknown_window_shows_raw_tokens() {
        let evt = WorkspaceEvents {
            context_tokens: Some(77_000),
            model_id: Some("gpt-5-codex".to_string()),
            ..WorkspaceEvents::default()
        };
        let (text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "gpt-5-codex 77k");
        assert!(!warn);
    }

    #[test]
    fn format_chip_model_tokens_unknown_window_warns_past_150k() {
        let evt = WorkspaceEvents {
            context_tokens: Some(160_000),
            model_id: Some("gpt-5-codex".to_string()),
            ..WorkspaceEvents::default()
        };
        let (_text, warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert!(warn);
    }

    #[test]
    fn format_chip_model_tokens_none_when_no_tokens() {
        let evt = WorkspaceEvents {
            context_tokens: None,
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        assert!(format_chip_model_tokens(&evt).is_none());
        let zero = WorkspaceEvents {
            context_tokens: Some(0),
            model_id: Some("claude-opus-4-8".to_string()),
            ..WorkspaceEvents::default()
        };
        assert!(format_chip_model_tokens(&zero).is_none());
    }

    #[test]
    fn format_chip_model_tokens_tokens_only_when_no_model() {
        let evt = WorkspaceEvents {
            context_tokens: Some(45_000),
            model_id: None,
            ..WorkspaceEvents::default()
        };
        let (text, _warn) = format_chip_model_tokens(&evt).expect("has tokens");
        assert_eq!(text, "45k");
    }
}
