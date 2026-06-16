//! Extracted from ui/modal.rs.

use super::*;

/// Compute the order in which workspaces appear in the updates panel.
/// Returns workspace IDs in the same order the renderer walks them —
/// grouped by repo (in App's repo order), sorted within each repo by
/// (attention, failed, activity_rank, recency).
///
/// Used by both the renderer (to draw rows) and the key handler (to map
/// the selected index back to a workspace id).
pub fn ordered_workspaces_for_panel(
    repos: &[crate::data::store::Repo],
    workspaces: &[(RepoId, crate::data::store::Workspace)],
    events: &HashMap<crate::data::store::WorkspaceId, crate::activity::events::WorkspaceEvents>,
    activity: &HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::data::store::WorkspaceId>,
) -> Vec<crate::data::store::WorkspaceId> {
    let mut out = Vec::new();
    for repo in repos {
        let mut ws_for_repo: Vec<&crate::data::store::Workspace> = workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .collect();
        ws_for_repo.sort_by_key(|w| sort_key(w, events, activity, needs_attention));
        out.extend(ws_for_repo.into_iter().map(|w| w.id));
    }
    out
}

fn sort_key(
    w: &crate::data::store::Workspace,
    events: &HashMap<crate::data::store::WorkspaceId, crate::activity::events::WorkspaceEvents>,
    activity: &HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::data::store::WorkspaceId>,
) -> (u8, u8, u8, i64) {
    let attention = if needs_attention.contains(&w.id) {
        0
    } else {
        1
    };
    let activity_rank = match activity.get(&w.id).copied() {
        Some(crate::ui::updates_bar::ActivityState::Awaiting)
        | Some(crate::ui::updates_bar::ActivityState::AwaitingAnswer)
        | Some(crate::ui::updates_bar::ActivityState::Complete)
        | Some(crate::ui::updates_bar::ActivityState::Stalled)
        | Some(crate::ui::updates_bar::ActivityState::Waiting) => 0,
        Some(crate::ui::updates_bar::ActivityState::Active)
        | Some(crate::ui::updates_bar::ActivityState::Idle) => 1,
        Some(crate::ui::updates_bar::ActivityState::Off) => 2,
        None => 3,
    };
    let failed = if w.state == crate::data::store::WorkspaceState::Failed {
        1
    } else {
        0
    };
    let recency = -events
        .get(&w.id)
        .and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms))
        .unwrap_or(0);
    (attention, failed, activity_rank, recency)
}

/// Render the floating workspace-updates panel. Reads live App state via
/// borrowed slices so the panel updates on every render tick.
#[allow(clippy::too_many_arguments)]
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    repos: &[crate::data::store::Repo],
    workspaces: &[(RepoId, crate::data::store::Workspace)],
    events: &HashMap<crate::data::store::WorkspaceId, crate::activity::events::WorkspaceEvents>,
    activity: &HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    needs_attention: &HashSet<crate::data::store::WorkspaceId>,
    awaiting: &HashMap<crate::data::store::WorkspaceId, (String, i64)>,
    statuses: &HashMap<crate::data::store::WorkspaceId, Status>,
    lifecycles: &HashMap<crate::data::store::WorkspaceId, BranchLifecycle>,
    selected: usize,
    now_ms: i64,
    theme: &Theme,
) {
    // Sizing: ~80 cols wide, ~25 rows tall, but never larger than the area.
    let w = area.width.clamp(20, 80);
    let h = area.height.clamp(8, 25);
    let inner = panel_frame(f, area, w, h, " Workspace updates ", theme);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    let order = ordered_workspaces_for_panel(repos, workspaces, events, activity, needs_attention);
    // workspace_id -> position in `order` so we can match against `selected`.
    let pos_of: HashMap<crate::data::store::WorkspaceId, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    for repo in repos {
        let ws_for_repo: Vec<&crate::data::store::Workspace> = workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .filter(|w| pos_of.contains_key(&w.id))
            .collect();
        // Omit repos with no workspaces entirely — header included. The panel
        // is only ever opened from an attached/agent view, where empty repos
        // are noise rather than the dashboard's full repo inventory.
        if ws_for_repo.is_empty() {
            continue;
        }
        lines.push(Line::from(Span::styled(
            repo.name.clone(),
            theme.header_style(),
        )));
        // Already pre-sorted in `order`; preserve that ordering here too.
        let mut ws_sorted = ws_for_repo;
        ws_sorted.sort_by_key(|w| pos_of.get(&w.id).copied().unwrap_or(usize::MAX));
        for w in ws_sorted {
            let is_selected = pos_of.get(&w.id).copied() == Some(selected);
            if is_selected {
                selected_visual_line = Some(lines.len());
            }
            let status = statuses.get(&w.id).copied().unwrap_or(Status::Idle);
            let lifecycle = lifecycles.get(&w.id).copied();
            lines.push(workspace_row(
                w,
                events.get(&w.id),
                activity.get(&w.id).copied(),
                needs_attention.contains(&w.id),
                awaiting.get(&w.id),
                is_selected,
                status,
                lifecycle,
                now_ms,
                theme,
            ));
        }
        lines.push(Line::from(""));
    }
    // Nothing to show when no repo has any workspace (or there are no repos).
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(no workspaces)".to_string(),
            theme.dim_style(),
        )));
    }

    // Stateless scroll: keep the selected workspace centered in the viewport
    // when the rendered lines overflow the body area. Clamped so we never
    // scroll past the last line.
    let scroll_y =
        scroll_offset_for_selected(selected_visual_line, lines.len(), body_area.height as usize);

    // No widget-level style: per-span styles drive the row colors, and
    // every dim element (empty-repo hint, "(no repos)") already self-styles.
    // A widget-level dim would leak into spans with fg=None — notably the
    // workspace name when lifecycle is unknown.
    f.render_widget(Paragraph::new(lines).scroll((scroll_y, 0)), body_area);
    f.render_widget(
        Paragraph::new(
            "[\u{2191}/\u{2193}] move   [enter] switch   [v] vsplit   [s] hsplit   [esc] close",
        )
        .style(theme.dim_style()),
        footer_area,
    );
}

/// Compute the vertical scroll offset for the updates panel so the selected
/// row stays visible. Stateless — called fresh each render. Strategy:
/// center the selected line in the viewport, then clamp to the valid scroll
/// range so we never scroll past the end. Returns 0 when content fits or
/// when there is no selection.
fn scroll_offset_for_selected(
    selected_visual_line: Option<usize>,
    total_lines: usize,
    viewport_height: usize,
) -> u16 {
    let Some(s) = selected_visual_line else {
        return 0;
    };
    if viewport_height == 0 || total_lines <= viewport_height {
        return 0;
    }
    let centered = s.saturating_sub(viewport_height / 2);
    let max_scroll = total_lines.saturating_sub(viewport_height);
    centered.min(max_scroll).min(u16::MAX as usize) as u16
}

#[allow(clippy::too_many_arguments)]
fn workspace_row<'a>(
    w: &'a crate::data::store::Workspace,
    events: Option<&'a crate::activity::events::WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&'a (String, i64)>,
    is_selected: bool,
    status: Status,
    lifecycle: Option<BranchLifecycle>,
    now_ms: i64,
    theme: &Theme,
) -> Line<'a> {
    use crate::ui::updates_bar::{ActivityState, format_age, glyph_for_activity};
    let failed = w.state == crate::data::store::WorkspaceState::Failed;
    let glyph = if failed {
        '✕'
    } else if needs_attention {
        activity.map(glyph_for_activity).unwrap_or('⚠')
    } else {
        match activity {
            Some(ActivityState::Active) | Some(ActivityState::Idle) => '●',
            Some(ActivityState::AwaitingAnswer) => '?',
            Some(ActivityState::Complete) => '\u{2713}',
            Some(ActivityState::Awaiting)
            | Some(ActivityState::Stalled)
            | Some(ActivityState::Waiting) => '⚠',
            Some(ActivityState::Off) | None => {
                if events.and_then(|e| e.latest.as_ref()).is_some() {
                    '↻'
                } else {
                    '○'
                }
            }
        }
    };
    let (status_text, age_anchor_ms) = if let Some((tool, ts)) = awaiting {
        (format!("awaiting permission: {tool}"), Some(*ts))
    } else if needs_attention {
        let label = match activity {
            Some(ActivityState::AwaitingAnswer) => "question",
            Some(ActivityState::Complete) => "complete",
            Some(ActivityState::Stalled) => "stalled",
            _ => "waiting",
        };
        (
            label.to_string(),
            events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms)),
        )
    } else if matches!(
        activity,
        Some(ActivityState::Active) | Some(ActivityState::Idle)
    ) {
        let text = events
            .and_then(|e| e.latest.as_ref().map(|s| s.display.clone()))
            .unwrap_or_else(|| "active".to_string());
        let ts = events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms));
        (text, ts)
    } else if failed {
        ("failed".to_string(), None)
    } else if events.and_then(|e| e.latest.as_ref()).is_some() {
        ("resumable".to_string(), None)
    } else {
        ("no session".to_string(), None)
    };
    let age = age_anchor_ms.map(|t| format_age(now_ms.saturating_sub(t)));
    let suffix = match age {
        Some(a) => format!(" ({a})"),
        None => String::new(),
    };

    // Failed overrides the canonical status hue with `err` — a failed
    // workspace is the same urgency signal regardless of its prior status.
    let status_fg = if failed {
        theme.err_style()
    } else {
        theme.status_style(status)
    };
    // Lifecycle wins on the name even when the workspace is failed — a
    // failed workspace can still have a merged PR. Bold so the name
    // still reads as a name. When there's no lifecycle hue, explicitly
    // reset fg so the surrounding Block's dim style can't leak through
    // ratatui's style inheritance and dim the workspace name.
    let name_style = theme
        .lifecycle_style(lifecycle)
        .unwrap_or_else(|| Style::default().fg(ratatui::style::Color::Reset))
        .add_modifier(Modifier::BOLD);

    let name_padded = format!("{:<20}", w.name);
    let spans = vec![
        Span::raw("  "),
        Span::styled(format!("{glyph} "), status_fg),
        Span::styled(name_padded, name_style),
        Span::styled(format!(" {status_text}{suffix}"), status_fg),
    ];

    let mut line = Line::from(spans);
    if is_selected {
        // bg-only so per-span fg colors survive; matches the dashboard's
        // List::highlight_style(theme.selected_bg_style()).
        line = line.style(theme.selected_bg_style());
    }
    line
}

#[cfg(test)]
mod scroll_offset_tests {
    use super::*;

    #[test]
    fn no_selection_yields_zero_offset() {
        assert_eq!(scroll_offset_for_selected(None, 100, 20), 0);
    }

    #[test]
    fn content_fits_in_viewport_yields_zero_offset() {
        // 10 lines, viewport 20, selected at line 5 — no scroll needed.
        assert_eq!(scroll_offset_for_selected(Some(5), 10, 20), 0);
    }

    #[test]
    fn zero_height_viewport_yields_zero_offset() {
        assert_eq!(scroll_offset_for_selected(Some(50), 100, 0), 0);
    }

    #[test]
    fn selection_in_first_half_does_not_scroll() {
        // Selected at line 4, viewport 20, total 100: centering would put
        // selected at top half, so offset stays 0.
        assert_eq!(scroll_offset_for_selected(Some(4), 100, 20), 0);
    }

    #[test]
    fn selection_centers_in_viewport_when_overflowing() {
        // Selected at line 50, viewport 20, total 100.
        // centered = 50 - 10 = 40. max_scroll = 80. result = 40.
        // Selected appears at viewport row 50 - 40 = 10 (middle).
        assert_eq!(scroll_offset_for_selected(Some(50), 100, 20), 40);
    }

    #[test]
    fn selection_near_end_clamps_to_max_scroll() {
        // Selected at last line (99), viewport 20, total 100.
        // centered = 99 - 10 = 89. max_scroll = 80. clamped to 80.
        // Selected appears at viewport row 99 - 80 = 19 (last row).
        assert_eq!(scroll_offset_for_selected(Some(99), 100, 20), 80);
    }

    #[test]
    fn last_line_selected_in_short_overflow() {
        // total = 22, viewport = 20 — barely overflows by 2.
        // Selected at line 21 (last). centered = 21 - 10 = 11.
        // max_scroll = 2. clamped to 2. selected appears at row 19.
        assert_eq!(scroll_offset_for_selected(Some(21), 22, 20), 2);
    }
}

#[cfg(test)]
mod workspace_row_tests {
    use super::*;
    use crate::data::store::{Workspace, WorkspaceId, WorkspaceState};
    use crate::ui::updates_bar::ActivityState;
    use std::path::PathBuf;

    fn fixture_workspace(name: &str) -> Workspace {
        Workspace {
            id: WorkspaceId(1),
            repo_id: crate::data::store::RepoId(1),
            name: name.to_string(),
            branch: "main".to_string(),
            worktree_path: PathBuf::from("/tmp/ws"),
            state: WorkspaceState::Ready,
            setup_status: crate::data::store::SetupStatus::Ok,
            created_at: 0,
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
        }
    }

    /// Concatenate every span's content into a single String so tests can
    /// match against the rendered text regardless of styling.
    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    /// Find the first span whose content contains `needle`. Tests use this
    /// to locate the glyph, name, or status-text span by a known substring.
    fn span_containing<'a>(line: &'a Line<'_>, needle: &str) -> &'a Span<'a> {
        line.spans
            .iter()
            .find(|s| s.content.as_ref().contains(needle))
            .unwrap_or_else(|| panic!("no span containing {needle:?}"))
    }

    #[test]
    fn workspace_row_uses_question_glyph_for_awaiting_answer() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::AwaitingAnswer),
            true,
            None,
            false,
            Status::Question,
            None,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains("? "), "expected '?' glyph in: {body}");
        assert!(
            body.contains("question"),
            "expected 'question' status text in: {body}"
        );
    }

    #[test]
    fn workspace_row_uses_check_glyph_for_complete() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Complete),
            true,
            None,
            false,
            Status::Complete,
            None,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('\u{2713}'), "expected '✓' glyph in: {body}");
        assert!(
            body.contains("complete"),
            "expected 'complete' status text in: {body}"
        );
    }

    #[test]
    fn workspace_row_shows_permission_tool_in_status_text() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let awaiting = ("Bash".to_string(), 5_000i64);
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Awaiting),
            true,
            Some(&awaiting),
            false,
            Status::Question,
            None,
            10_000,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('⚠'), "expected '⚠' glyph in: {body}");
        assert!(
            body.contains("awaiting permission: Bash"),
            "expected permission tool name in status text: {body}"
        );
    }

    /// For each of the six canonical Status variants, the glyph and status-
    /// text spans should be painted with theme.status_style(status).fg.
    /// Mirrors the dashboard's gutter/glyph coloring so a glance at the modal
    /// matches a glance at the dashboard.
    #[test]
    fn workspace_row_paints_glyph_and_text_with_status_color() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        // (status, activity option, needs_attention, label substring to find)
        let cases: [(Status, Option<ActivityState>, bool, &str); 6] = [
            (
                Status::Question,
                Some(ActivityState::AwaitingAnswer),
                true,
                "question",
            ),
            (
                Status::Complete,
                Some(ActivityState::Complete),
                true,
                "complete",
            ),
            (
                Status::Stalled,
                Some(ActivityState::Stalled),
                true,
                "stalled",
            ),
            (
                Status::Waiting,
                Some(ActivityState::Waiting),
                true,
                "waiting",
            ),
            (
                Status::Thinking,
                Some(ActivityState::Active),
                false,
                "active",
            ),
            (Status::Idle, None, false, "no session"),
        ];
        for (status, activity, needs_attention, label) in cases {
            let line = workspace_row(
                &w,
                None,
                activity,
                needs_attention,
                None,
                false,
                status,
                None,
                10_000,
                &theme,
            );
            let glyph_span = &line.spans[1];
            let text_span = span_containing(&line, label);
            let expected = theme.status_style(status).fg;
            assert_eq!(
                glyph_span.style.fg, expected,
                "glyph fg for {status:?} should match status_style"
            );
            assert_eq!(
                text_span.style.fg, expected,
                "status text fg for {status:?} should match status_style"
            );
        }
    }

    /// Failed workspaces ignore the canonical status hue and paint glyph +
    /// text with err — failure is the same urgency signal regardless of what
    /// the classifier said before the failure.
    #[test]
    fn workspace_row_failed_overrides_status_with_err() {
        let theme = Theme::ansi();
        let mut w = fixture_workspace("alpha");
        w.state = WorkspaceState::Failed;
        let line = workspace_row(
            &w,
            None,
            None,
            false,
            None,
            false,
            Status::Idle, // classifier might say anything; failed wins
            None,
            10_000,
            &theme,
        );
        let glyph_span = &line.spans[1];
        let text_span = span_containing(&line, "failed");
        assert_eq!(glyph_span.style.fg, Some(theme.err));
        assert_eq!(text_span.style.fg, Some(theme.err));
    }

    /// Lifecycle drives the workspace name's foreground color. Mirrors the
    /// dashboard branch column so the modal and dashboard tell the same story
    /// about PR state.
    #[test]
    fn workspace_row_paints_name_with_lifecycle_color() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        // Lifecycles without a hue (NoPr, PrDraft, None) fall back to
        // Color::Reset so the surrounding Block's dim style can't leak
        // through ratatui's style inheritance and dim the name.
        let reset = Some(ratatui::style::Color::Reset);
        let cases = [
            (Some(PrOpen), Some(theme.ok)),
            (Some(PrConflicted), Some(theme.warn)),
            (Some(PrMerged), Some(theme.merged)),
            (Some(PrClosed), Some(theme.err)),
            (Some(NoPr), reset),
            (Some(PrDraft), reset),
            (None, reset),
        ];
        for (lifecycle, expected_fg) in cases {
            let line = workspace_row(
                &w,
                None,
                None,
                false,
                None,
                false,
                Status::Idle,
                lifecycle,
                10_000,
                &theme,
            );
            let name_span = span_containing(&line, "alpha");
            assert_eq!(
                name_span.style.fg, expected_fg,
                "name fg for lifecycle {lifecycle:?}"
            );
            assert!(
                name_span.style.add_modifier.contains(Modifier::BOLD),
                "name should be bold for lifecycle {lifecycle:?}"
            );
        }
    }

    /// Selection should only set the row's background — per-span foregrounds
    /// (status hue, lifecycle hue) must survive so the user can still tell at
    /// a glance which workspace is in what state on the selected row.
    #[test]
    fn workspace_row_selection_keeps_span_foregrounds() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let line = workspace_row(
            &w,
            None,
            Some(ActivityState::Complete),
            true,
            None,
            true, // selected
            Status::Complete,
            Some(crate::git::forge::BranchLifecycle::PrOpen),
            10_000,
            &theme,
        );
        // Line-level style carries only the selected bg, not a foreground.
        assert_eq!(line.style.bg, Some(theme.selected_bg));
        assert_eq!(line.style.fg, None);
        // Per-span foregrounds still match status / lifecycle.
        let glyph_span = &line.spans[1];
        let name_span = span_containing(&line, "alpha");
        let text_span = span_containing(&line, "complete");
        assert_eq!(glyph_span.style.fg, Some(theme.complete));
        assert_eq!(name_span.style.fg, Some(theme.ok));
        assert_eq!(text_span.style.fg, Some(theme.complete));
    }
}
