//! Extracted from ui/modal.rs.

use super::*;
use crate::ui::text::{truncate, truncate_pad};

/// The borrowed caches the panel reads. Bundled so the renderer and the key
/// handler hand identical inputs to `ordered_workspaces_for_panel` without
/// two long, drift-prone argument lists. Mirrors `DashboardInputs` on the
/// dashboard side.
pub struct PanelInputs<'a> {
    pub repos: &'a [crate::data::store::Repo],
    pub workspaces: &'a [(RepoId, crate::data::store::Workspace)],
    pub events:
        &'a HashMap<crate::data::store::WorkspaceId, crate::activity::events::WorkspaceEvents>,
    pub activity:
        &'a HashMap<crate::data::store::WorkspaceId, crate::ui::updates_bar::ActivityState>,
    pub needs_attention: &'a HashSet<crate::data::store::WorkspaceId>,
    pub awaiting: &'a HashMap<crate::data::store::WorkspaceId, (String, i64)>,
    pub statuses: &'a HashMap<crate::data::store::WorkspaceId, Status>,
    pub lifecycles: &'a HashMap<crate::data::store::WorkspaceId, BranchLifecycle>,
}

impl PanelInputs<'_> {
    /// The status text a row would display for `w`. Used by the renderer's
    /// caller-side logic and, from Task 2, by the filter.
    fn status_text(&self, w: &crate::data::store::Workspace) -> String {
        row_status_text(
            w,
            self.events.get(&w.id),
            self.activity.get(&w.id).copied(),
            self.needs_attention.contains(&w.id),
            self.awaiting.get(&w.id),
        )
        .0
    }
}

/// Cap on the workspace-name column so one very long name can't starve the
/// status column of the entire panel.
const NAME_COL_MAX: usize = 28;

/// Chars consumed left of the name column: 2-space indent + glyph + space.
const ROW_PREFIX_W: usize = 4;

/// Gap between adjacent columns (name→status, status→age).
const COL_GAP_W: usize = 2;

/// Width of the shared workspace-name column: as wide as the longest name,
/// capped at [`NAME_COL_MAX`] and clamped so prefix + name + gap always
/// leave at least one char of status text in the narrowest panel. Shared
/// across every repo section so status texts start at one fixed column for
/// the whole panel.
fn name_col_width<'a>(names: impl Iterator<Item = &'a str>, row_width: usize) -> usize {
    let cap = NAME_COL_MAX.min(row_width.saturating_sub(ROW_PREFIX_W + COL_GAP_W + 1));
    names.map(|n| n.chars().count()).max().unwrap_or(0).min(cap)
}

/// User-cyclable sort mode for the updates panel. Carried in the modal
/// variant, so it resets to `Default` every time the panel opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdatesSort {
    /// Today's ordering: (attention, failed, activity_rank, recency).
    #[default]
    Default,
    /// Workspace-status urgency via `Status::priority()`; failed first.
    Status,
    /// PR lifecycle, actionable first: conflicted → open → draft →
    /// merged → closed → no PR → unknown.
    PrStatus,
}

impl UpdatesSort {
    /// Next mode in the `o`-key cycle.
    pub fn cycle(self) -> Self {
        match self {
            Self::Default => Self::Status,
            Self::Status => Self::PrStatus,
            Self::PrStatus => Self::Default,
        }
    }

    /// Short mode name shown in the footer hint.
    pub fn footer_label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Status => "status",
            Self::PrStatus => "pr",
        }
    }
}

/// Status-mode rank: lower sorts first. Failed outranks every status —
/// it's the loudest signal — then descending `Status::priority()`. The key
/// is `(failed_first, Reverse(urgency))` rather than a subtraction, so
/// there's no magic constant to fall out of sync if `Status::priority()`'s
/// range ever grows.
fn status_rank(
    w: &crate::data::store::Workspace,
    statuses: &HashMap<crate::data::store::WorkspaceId, Status>,
) -> (u8, std::cmp::Reverse<u8>) {
    let failed_first = if w.state == crate::data::store::WorkspaceState::Failed {
        0
    } else {
        1
    };
    let urgency = statuses
        .get(&w.id)
        .copied()
        .unwrap_or(Status::Idle)
        .priority();
    (failed_first, std::cmp::Reverse(urgency))
}

/// PrStatus-mode rank: actionable lifecycles first, unknown last.
fn lifecycle_rank(lifecycle: Option<BranchLifecycle>) -> u8 {
    match lifecycle {
        Some(BranchLifecycle::PrConflicted) => 0,
        Some(BranchLifecycle::PrOpen) => 1,
        Some(BranchLifecycle::PrDraft) => 2,
        Some(BranchLifecycle::PrMerged) => 3,
        Some(BranchLifecycle::PrClosed) => 4,
        Some(BranchLifecycle::NoPr) => 5,
        None => 6,
    }
}

/// Compute the order in which workspaces appear in the updates panel.
/// Returns workspace IDs in the same order the renderer walks them —
/// grouped by repo (in App's repo order), sorted within each repo by
/// the active `UpdatesSort` mode, tie-broken by (attention, failed,
/// activity_rank, recency).
///
/// Used by both the renderer (to draw rows) and the key handler (to map
/// the selected index back to a workspace id).
/// Case-insensitive substring match against the workspace name, the owning
/// repo's name, and the row's live status text. Mirrors the dashboard's
/// `matches_filter`, whose three fields are the same idea: what the row is
/// called, where it lives, and what it currently says.
fn matches_filter(
    w: &crate::data::store::Workspace,
    repo_name: &str,
    status_text: &str,
    needle: &str,
) -> bool {
    let needle = needle.to_lowercase();
    w.name.to_lowercase().contains(&needle)
        || repo_name.to_lowercase().contains(&needle)
        || status_text.to_lowercase().contains(&needle)
}

pub fn ordered_workspaces_for_panel(
    inputs: &PanelInputs<'_>,
    sort: UpdatesSort,
    filter: Option<&str>,
) -> Vec<crate::data::store::WorkspaceId> {
    // An empty buffer means "filter mode is on but nothing typed yet" —
    // every row still shows. Only a non-empty needle narrows the list.
    let needle = filter.filter(|f| !f.is_empty());
    let mut out = Vec::new();
    for repo in inputs.repos {
        let mut ws_for_repo: Vec<&crate::data::store::Workspace> = inputs
            .workspaces
            .iter()
            .filter(|(rid, _)| *rid == repo.id)
            .map(|(_, w)| w)
            .filter(|w| {
                needle
                    .map(|n| matches_filter(w, &repo.name, &inputs.status_text(w), n))
                    .unwrap_or(true)
            })
            .collect();
        ws_for_repo.sort_by_key(|w| {
            let default_key = sort_key(w, inputs.events, inputs.activity, inputs.needs_attention);
            let mode_rank = match sort {
                UpdatesSort::Default => (0, std::cmp::Reverse(0)),
                UpdatesSort::Status => status_rank(w, inputs.statuses),
                UpdatesSort::PrStatus => (
                    lifecycle_rank(inputs.lifecycles.get(&w.id).copied()),
                    std::cmp::Reverse(0),
                ),
            };
            (mode_rank, default_key)
        });
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

/// Footer hint line. `v`/`s` collapse into one `[v/s] split` chip so the
/// line still fits the widest panel (80 cols − 2 border = 78) with the
/// sort mode shown.
fn footer_text(sort: UpdatesSort) -> String {
    format!(
        "[\u{2191}/\u{2193}] move  [enter/l] switch  [v/s] split  [o] sort:{}  [esc] close",
        sort.footer_label()
    )
}

/// Render the floating workspace-updates panel. Reads live App state via
/// borrowed slices so the panel updates on every render tick.
// The brief for this task expected 7→8 args to stay under clippy's default
// too_many_arguments threshold (7); it does not — `filter` pushes this over.
// Restoring the allow here (only) rather than reshaping the interface, since
// the exact parameter list is a contract other tasks depend on.
#[allow(clippy::too_many_arguments)]
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    inputs: &PanelInputs<'_>,
    selected: usize,
    now_ms: i64,
    sort: UpdatesSort,
    filter: Option<&str>,
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

    let order = ordered_workspaces_for_panel(inputs, sort, filter);
    // workspace_id -> position in `order` so we can match against `selected`.
    let pos_of: HashMap<crate::data::store::WorkspaceId, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();

    // One shared name column for the whole panel so every status text starts
    // at the same column regardless of which repo section a row is in.
    let row_width = body_area.width as usize;
    let name_col = name_col_width(
        inputs
            .workspaces
            .iter()
            .filter(|(_, w)| pos_of.contains_key(&w.id))
            .map(|(_, w)| w.name.as_str()),
        row_width,
    );

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    for repo in inputs.repos {
        let ws_for_repo: Vec<&crate::data::store::Workspace> = inputs
            .workspaces
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
        lines.push(Line::from(vec![
            Span::styled(repo.name.clone(), theme.header_style()),
            Span::styled(format!("  ({})", ws_for_repo.len()), theme.dim_style()),
        ]));
        // Already pre-sorted in `order`; preserve that ordering here too.
        let mut ws_sorted = ws_for_repo;
        ws_sorted.sort_by_key(|w| pos_of.get(&w.id).copied().unwrap_or(usize::MAX));
        for w in ws_sorted {
            let is_selected = pos_of.get(&w.id).copied() == Some(selected);
            if is_selected {
                selected_visual_line = Some(lines.len());
            }
            let status = inputs.statuses.get(&w.id).copied().unwrap_or(Status::Idle);
            let lifecycle = inputs.lifecycles.get(&w.id).copied();
            lines.push(workspace_row(
                w,
                inputs.events.get(&w.id),
                inputs.activity.get(&w.id).copied(),
                inputs.needs_attention.contains(&w.id),
                inputs.awaiting.get(&w.id),
                is_selected,
                status,
                lifecycle,
                now_ms,
                name_col,
                row_width,
                theme,
            ));
        }
        lines.push(Line::from(""));
    }
    // Nothing to show. Separate the two causes: an empty panel and a panel
    // whose rows the needle hid are very different situations for the user.
    if lines.is_empty() {
        let msg = if filter.map(|f| !f.is_empty()).unwrap_or(false) {
            "(no matching workspaces)"
        } else {
            "(no workspaces)"
        };
        lines.push(Line::from(Span::styled(msg.to_string(), theme.dim_style())));
    }

    // Stateless scroll: keep the selected workspace centered in the viewport
    // when the rendered lines overflow the body area. Clamped so we never
    // scroll past the last line.
    let scroll_y =
        scroll_offset_for_selected(selected_visual_line, lines.len(), body_area.height as usize);

    // No widget-level style: per-span styles drive the row colors, and
    // the dim "(no workspaces)" fallback already self-styles. A widget-level
    // dim would leak into spans with fg=None — notably the workspace name
    // when lifecycle is unknown.
    f.render_widget(Paragraph::new(lines).scroll((scroll_y, 0)), body_area);
    f.render_widget(
        Paragraph::new(footer_text(sort)).style(theme.dim_style()),
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

/// The status text a row displays, plus the timestamp its age column is
/// anchored to. Split out of `workspace_row` so the filter can match on
/// exactly the text the row shows — a row must never display text the
/// filter fails to find.
fn row_status_text(
    w: &crate::data::store::Workspace,
    events: Option<&crate::activity::events::WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&(String, i64)>,
) -> (String, Option<i64>) {
    use crate::ui::updates_bar::ActivityState;
    if let Some((tool, ts)) = awaiting {
        return (format!("awaiting permission: {tool}"), Some(*ts));
    }
    if needs_attention {
        let label = match activity {
            Some(ActivityState::AwaitingAnswer) => "question",
            Some(ActivityState::Complete) => "complete",
            Some(ActivityState::Stalled) => "stalled",
            _ => "waiting",
        };
        return (
            label.to_string(),
            events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms)),
        );
    }
    if matches!(
        activity,
        Some(ActivityState::Active) | Some(ActivityState::Idle)
    ) {
        let text = events
            .and_then(|e| e.latest.as_ref().map(|s| s.display.clone()))
            .unwrap_or_else(|| "active".to_string());
        let ts = events.and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms));
        return (text, ts);
    }
    if w.state == crate::data::store::WorkspaceState::Failed {
        return ("failed".to_string(), None);
    }
    if events.and_then(|e| e.latest.as_ref()).is_some() {
        return ("resumable".to_string(), None);
    }
    ("no session".to_string(), None)
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
    name_col: usize,
    row_width: usize,
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
    let (status_text, age_anchor_ms) =
        row_status_text(w, events, activity, needs_attention, awaiting);
    let age = age_anchor_ms.map(|t| format_age(now_ms.saturating_sub(t)));

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

    // Column layout: indent+glyph | name | status | right-aligned age.
    // The status text is truncated so it can never collide with the age
    // column, and the row is padded to exactly `row_width` so the selection
    // background spans the full row.
    let avail = row_width.saturating_sub(ROW_PREFIX_W + name_col + COL_GAP_W);
    // Drop the age column when it (plus its gap) wouldn't leave at least one
    // char of status text — a clipped age is worse than no age.
    let age = age.filter(|a| a.chars().count() + COL_GAP_W < avail);
    let age_w = age.as_ref().map(|a| a.chars().count()).unwrap_or(0);
    let age_reserved = if age_w > 0 { age_w + COL_GAP_W } else { 0 };
    let status_budget = avail.saturating_sub(age_reserved);
    let status_txt = truncate(&status_text, status_budget);
    let pad_w = row_width
        .saturating_sub(ROW_PREFIX_W + name_col + COL_GAP_W + status_txt.chars().count() + age_w);

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(format!("{glyph} "), status_fg),
        Span::styled(truncate_pad(&w.name, name_col), name_style),
        Span::raw(" ".repeat(COL_GAP_W)),
        Span::styled(status_txt, status_fg),
        Span::raw(" ".repeat(pad_w)),
    ];
    if let Some(a) = age {
        spans.push(Span::styled(a, theme.dim_style()));
    }

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
            shared: false,
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
            20,
            78,
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
            20,
            78,
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
            20,
            78,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('⚠'), "expected '⚠' glyph in: {body}");
        assert!(
            body.contains("awaiting permission: Bash"),
            "expected permission tool name in status text: {body}"
        );
    }

    /// `row_status_text` is the single source for the row's status text —
    /// the renderer and (from Task 2) the filter both read it. If the
    /// extraction ever drifts from what `workspace_row` draws, the filter
    /// would fail to match text the user can plainly see.
    #[test]
    #[allow(clippy::type_complexity)]
    fn row_status_text_matches_what_the_row_renders() {
        let theme = Theme::ansi();
        let mut failed = fixture_workspace("gamma");
        failed.state = WorkspaceState::Failed;
        let awaiting = ("Bash".to_string(), 5_000i64);
        let (alpha, beta, delta) = (
            fixture_workspace("alpha"),
            fixture_workspace("beta"),
            fixture_workspace("delta"),
        );
        // (workspace, activity, needs_attention, awaiting, expected text)
        let cases: [(
            &Workspace,
            Option<ActivityState>,
            bool,
            Option<&(String, i64)>,
            &str,
        ); 4] = [
            (
                &alpha,
                Some(ActivityState::Awaiting),
                true,
                Some(&awaiting),
                "awaiting permission: Bash",
            ),
            (&beta, Some(ActivityState::Stalled), true, None, "stalled"),
            (&failed, None, false, None, "failed"),
            (&delta, None, false, None, "no session"),
        ];
        for (w, activity, attention, awaiting, expected) in cases {
            let (text, _) = row_status_text(w, None, activity, attention, awaiting);
            assert_eq!(text, expected, "row_status_text for {}", w.name);
            let line = workspace_row(
                w,
                None,
                activity,
                attention,
                awaiting,
                false,
                Status::Idle,
                None,
                10_000,
                20,
                78,
                &theme,
            );
            assert!(
                line_text(&line).contains(expected),
                "row for {} should render {expected:?}: {}",
                w.name,
                line_text(&line)
            );
        }
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
                20,
                78,
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
            20,
            78,
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
                20,
                78,
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

    /// Status texts must start at the same column regardless of name length —
    /// the whole point of the shared name column.
    #[test]
    fn workspace_row_aligns_status_column_across_name_lengths() {
        let theme = Theme::ansi();
        let short = fixture_workspace("a");
        let long = fixture_workspace("a-much-longer-name");
        let row = |w: &Workspace| {
            let line = workspace_row(
                w,
                None,
                None,
                false,
                None,
                false,
                Status::Idle,
                None,
                10_000,
                20,
                78,
                &theme,
            );
            line_text(&line)
        };
        let col_short = row(&short).find("no session").unwrap();
        let col_long = row(&long).find("no session").unwrap();
        assert_eq!(col_short, col_long, "status must start at a fixed column");
    }

    /// Names wider than the name column truncate with an ellipsis instead of
    /// pushing the status column out of alignment.
    #[test]
    fn workspace_row_truncates_overlong_name_keeping_column() {
        let theme = Theme::ansi();
        let w = fixture_workspace("this-name-is-way-past-the-column");
        let line = workspace_row(
            &w,
            None,
            None,
            false,
            None,
            false,
            Status::Idle,
            None,
            10_000,
            20,
            78,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('…'), "expected ellipsis in: {body}");
        // Column position in chars, not bytes — the glyph and ellipsis are
        // multi-byte.
        let status_col = body[..body.find("no session").unwrap()].chars().count();
        assert_eq!(
            status_col,
            4 + 20 + 2,
            "status must start right after prefix + name column + gap"
        );
    }

    /// The age lands right-aligned at the row edge as its own dim column, and
    /// every row pads to exactly `row_width` so the selection background can
    /// cover the full row.
    #[test]
    fn workspace_row_right_aligns_age_and_pads_to_row_width() {
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
            20,
            78,
            &theme,
        );
        let body = line_text(&line);
        assert_eq!(body.chars().count(), 78, "row must fill row_width");
        assert!(
            body.ends_with("5s"),
            "age must sit at the right edge: {body:?}"
        );
        let age_span = line.spans.last().unwrap();
        assert_eq!(age_span.style, theme.dim_style(), "age renders dim");

        // A row without an age still pads to the full width.
        let no_age = workspace_row(
            &w,
            None,
            None,
            false,
            None,
            false,
            Status::Idle,
            None,
            10_000,
            20,
            78,
            &theme,
        );
        assert_eq!(line_text(&no_age).chars().count(), 78);
    }

    /// A long status text is truncated so it can never collide with the
    /// right-aligned age column.
    #[test]
    fn workspace_row_truncates_status_before_age_column() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let awaiting = (
            "SomeVeryLongToolName".repeat(4), // way past any budget
            5_000i64,
        );
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
            20,
            60,
            &theme,
        );
        let body = line_text(&line);
        assert_eq!(body.chars().count(), 60, "row must not overflow row_width");
        assert!(body.ends_with("5s"), "age survives truncation: {body:?}");
        assert!(body.contains('…'), "status text truncates with ellipsis");
    }

    #[test]
    fn name_col_width_tracks_longest_name_capped() {
        assert_eq!(name_col_width(["ab", "abcd"].into_iter(), 78), 4);
        assert_eq!(name_col_width(std::iter::empty(), 78), 0);
        let long = "x".repeat(NAME_COL_MAX + 10);
        assert_eq!(
            name_col_width([long.as_str()].into_iter(), 78),
            NAME_COL_MAX,
            "column caps at NAME_COL_MAX"
        );
        // Narrow panel: the column also clamps so prefix + name + gap leave
        // at least one status char. Inner width 18 (narrowest panel) → 11.
        assert_eq!(
            name_col_width([long.as_str()].into_iter(), 18),
            18 - ROW_PREFIX_W - COL_GAP_W - 1,
            "column clamps to the row width in narrow panels"
        );
    }

    /// In the narrowest panel (inner width 18) a long name plus an age must
    /// not overflow the row: the name column clamps, the age drops when it
    /// can't fit alongside status text, and the row stays within row_width.
    #[test]
    fn workspace_row_never_overflows_narrow_panel() {
        let theme = Theme::ansi();
        let w = fixture_workspace("a-very-long-workspace-name");
        let row_width = 18;
        let name_col = name_col_width([w.name.as_str()].into_iter(), row_width);
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
            name_col,
            row_width,
            &theme,
        );
        let body = line_text(&line);
        assert!(
            body.chars().count() <= row_width,
            "row must not overflow: {body:?}"
        );
        assert!(
            !body.ends_with("5s"),
            "age must drop when there is no room for status text: {body:?}"
        );
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
            20,
            78,
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

#[cfg(test)]
mod ordering_tests {
    use super::*;
    use crate::data::store::{Repo, RepoId, Workspace, WorkspaceId, WorkspaceState};
    use crate::git::forge::BranchLifecycle;
    use std::path::PathBuf;

    fn fixture_repo(id: i64) -> Repo {
        Repo {
            id: RepoId(id),
            name: format!("repo{id}"),
            path: PathBuf::from("/tmp/r"),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
            sort_order: 0,
        }
    }

    fn fixture_ws(id: i64, repo: i64, name: &str) -> (RepoId, Workspace) {
        (
            RepoId(repo),
            Workspace {
                id: WorkspaceId(id),
                repo_id: RepoId(repo),
                name: name.to_string(),
                branch: "main".to_string(),
                worktree_path: PathBuf::from("/tmp/ws"),
                state: WorkspaceState::Ready,
                setup_status: crate::data::store::SetupStatus::Ok,
                created_at: 0,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            },
        )
    }

    /// Bundles the three signal maps the ordering function reads, so each
    /// test only fills in what it exercises.
    #[derive(Default)]
    struct Maps {
        events: HashMap<WorkspaceId, crate::activity::events::WorkspaceEvents>,
        activity: HashMap<WorkspaceId, crate::ui::updates_bar::ActivityState>,
        attention: HashSet<WorkspaceId>,
        awaiting: HashMap<WorkspaceId, (String, i64)>,
        statuses: HashMap<WorkspaceId, Status>,
        lifecycles: HashMap<WorkspaceId, BranchLifecycle>,
    }

    fn order_filtered(
        repos: &[Repo],
        ws: &[(RepoId, Workspace)],
        maps: &Maps,
        sort: UpdatesSort,
        filter: Option<&str>,
    ) -> Vec<WorkspaceId> {
        ordered_workspaces_for_panel(
            &PanelInputs {
                repos,
                workspaces: ws,
                events: &maps.events,
                activity: &maps.activity,
                needs_attention: &maps.attention,
                awaiting: &maps.awaiting,
                statuses: &maps.statuses,
                lifecycles: &maps.lifecycles,
            },
            sort,
            filter,
        )
    }

    fn order(
        repos: &[Repo],
        ws: &[(RepoId, Workspace)],
        maps: &Maps,
        sort: UpdatesSort,
    ) -> Vec<WorkspaceId> {
        order_filtered(repos, ws, maps, sort, None)
    }

    #[test]
    fn cycle_walks_default_status_pr_and_back() {
        assert_eq!(UpdatesSort::Default.cycle(), UpdatesSort::Status);
        assert_eq!(UpdatesSort::Status.cycle(), UpdatesSort::PrStatus);
        assert_eq!(UpdatesSort::PrStatus.cycle(), UpdatesSort::Default);
    }

    #[test]
    fn footer_labels_match_modes() {
        assert_eq!(UpdatesSort::Default.footer_label(), "default");
        assert_eq!(UpdatesSort::Status.footer_label(), "status");
        assert_eq!(UpdatesSort::PrStatus.footer_label(), "pr");
    }

    /// Status mode: failed workspaces outrank everything, then statuses by
    /// descending urgency (Status::priority), Idle last.
    #[test]
    fn status_sort_ranks_failed_then_urgency() {
        let repos = vec![fixture_repo(1)];
        let mut ws = vec![
            fixture_ws(1, 1, "idle"),
            fixture_ws(2, 1, "stalled"),
            fixture_ws(3, 1, "failed"),
            fixture_ws(4, 1, "question"),
        ];
        ws[2].1.state = WorkspaceState::Failed;
        let mut maps = Maps::default();
        maps.statuses.insert(WorkspaceId(1), Status::Idle);
        maps.statuses.insert(WorkspaceId(2), Status::Stalled);
        maps.statuses.insert(WorkspaceId(4), Status::Question);
        let got = order(&repos, &ws, &maps, UpdatesSort::Status);
        assert_eq!(
            got,
            vec![
                WorkspaceId(3),
                WorkspaceId(2),
                WorkspaceId(4),
                WorkspaceId(1)
            ],
            "failed → stalled → question → idle"
        );
    }

    /// A workspace missing from the statuses map ranks as Idle (last).
    #[test]
    fn status_sort_treats_missing_status_as_idle() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "unknown"), fixture_ws(2, 1, "complete")];
        let mut maps = Maps::default();
        maps.statuses.insert(WorkspaceId(2), Status::Complete);
        let got = order(&repos, &ws, &maps, UpdatesSort::Status);
        assert_eq!(got, vec![WorkspaceId(2), WorkspaceId(1)]);
    }

    /// PrStatus mode: actionable first — Conflicted → Open → Draft →
    /// Merged → Closed → NoPr → unknown (absent from the map).
    #[test]
    fn pr_sort_ranks_actionable_first() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "unknown"),
            fixture_ws(2, 1, "nopr"),
            fixture_ws(3, 1, "closed"),
            fixture_ws(4, 1, "merged"),
            fixture_ws(5, 1, "draft"),
            fixture_ws(6, 1, "open"),
            fixture_ws(7, 1, "conflicted"),
        ];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(2), NoPr);
        maps.lifecycles.insert(WorkspaceId(3), PrClosed);
        maps.lifecycles.insert(WorkspaceId(4), PrMerged);
        maps.lifecycles.insert(WorkspaceId(5), PrDraft);
        maps.lifecycles.insert(WorkspaceId(6), PrOpen);
        maps.lifecycles.insert(WorkspaceId(7), PrConflicted);
        let got = order(&repos, &ws, &maps, UpdatesSort::PrStatus);
        assert_eq!(
            got,
            vec![
                WorkspaceId(7),
                WorkspaceId(6),
                WorkspaceId(5),
                WorkspaceId(4),
                WorkspaceId(3),
                WorkspaceId(2),
                WorkspaceId(1),
            ]
        );
    }

    /// Ties within a mode fall back to the default key — here two PrOpen
    /// workspaces where one needs attention: attention wins the tie.
    #[test]
    fn mode_ties_fall_back_to_default_key() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "calm"), fixture_ws(2, 1, "alert")];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(1), PrOpen);
        maps.lifecycles.insert(WorkspaceId(2), PrOpen);
        maps.attention.insert(WorkspaceId(2));
        let got = order(&repos, &ws, &maps, UpdatesSort::PrStatus);
        assert_eq!(got, vec![WorkspaceId(2), WorkspaceId(1)]);
    }

    /// Default mode ignores the new maps entirely — a merged PR must not
    /// reorder anything when sort is Default.
    #[test]
    fn default_sort_ignores_status_and_lifecycle_maps() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "first"), fixture_ws(2, 1, "merged")];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(2), PrConflicted);
        maps.statuses.insert(WorkspaceId(2), Status::Stalled);
        let got = order(&repos, &ws, &maps, UpdatesSort::Default);
        assert_eq!(
            got,
            vec![WorkspaceId(1), WorkspaceId(2)],
            "default keys are equal; stable sort keeps input order"
        );
    }

    /// Sorting never crosses repo boundaries: a conflicted PR in repo 2
    /// stays under repo 2's header even though it outranks repo 1's rows.
    #[test]
    fn sorts_stay_within_repo_groups() {
        use BranchLifecycle::*;
        let repos = vec![fixture_repo(1), fixture_repo(2)];
        let ws = vec![
            fixture_ws(1, 1, "r1-open"),
            fixture_ws(2, 2, "r2-conflicted"),
        ];
        let mut maps = Maps::default();
        maps.lifecycles.insert(WorkspaceId(1), PrOpen);
        maps.lifecycles.insert(WorkspaceId(2), PrConflicted);
        let got = order(&repos, &ws, &maps, UpdatesSort::PrStatus);
        assert_eq!(
            got,
            vec![WorkspaceId(1), WorkspaceId(2)],
            "repo 1's workspaces list before repo 2's regardless of rank"
        );
    }

    #[test]
    fn footer_shows_active_sort_mode_and_fits_panel() {
        for (sort, label) in [
            (UpdatesSort::Default, "sort:default"),
            (UpdatesSort::Status, "sort:status"),
            (UpdatesSort::PrStatus, "sort:pr"),
        ] {
            let f = footer_text(sort);
            assert!(f.contains(label), "footer {f:?} must contain {label:?}");
            assert!(f.contains("[o]"), "footer must advertise the o key");
            assert!(
                f.chars().count() <= 78,
                "footer must fit the widest panel (80 - 2 border): {f:?}"
            );
        }
    }

    /// The needle matches the workspace name, case-insensitively.
    #[test]
    fn filter_matches_workspace_name() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "auth-refactor"),
            fixture_ws(2, 1, "billing-fix"),
        ];
        let maps = Maps::default();
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("AUTH")),
            vec![WorkspaceId(1)]
        );
    }

    /// A repo-name needle keeps every workspace in that repo — the same
    /// affordance the dashboard gives for "show me just this repo".
    #[test]
    fn filter_matches_repo_name_and_keeps_its_workspaces() {
        let repos = vec![fixture_repo(1), fixture_repo(2)];
        let ws = vec![
            fixture_ws(1, 1, "alpha"),
            fixture_ws(2, 1, "beta"),
            fixture_ws(3, 2, "gamma"),
        ];
        let maps = Maps::default();
        // fixture_repo(1) is named "repo1", fixture_repo(2) is "repo2".
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("repo1")),
            vec![WorkspaceId(1), WorkspaceId(2)]
        );
    }

    /// The needle also matches the live status text, so "permission" or
    /// "stalled" narrows to the rows that actually say that.
    #[test]
    fn filter_matches_status_text() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha"), fixture_ws(2, 1, "beta")];
        let mut maps = Maps::default();
        maps.awaiting
            .insert(WorkspaceId(1), ("Bash".to_string(), 1_000));
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("permission")),
            vec![WorkspaceId(1)]
        );
        // beta has no session at all, so its status text is "no session".
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("no session")),
            vec![WorkspaceId(2)]
        );
    }

    /// `Some("")` is the "user pressed / but hasn't typed" state: every row
    /// stays visible. Only a non-empty needle narrows anything.
    #[test]
    fn empty_needle_matches_everything() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha"), fixture_ws(2, 1, "beta")];
        let maps = Maps::default();
        let all = vec![WorkspaceId(1), WorkspaceId(2)];
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("")),
            all
        );
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, None),
            all
        );
    }

    /// A needle matching nothing yields an empty order — the renderer turns
    /// that into "(no matching workspaces)".
    #[test]
    fn filter_matching_nothing_yields_empty_order() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha")];
        let maps = Maps::default();
        assert!(order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("zzz")).is_empty());
    }

    /// Filtering narrows the list without reshuffling it: survivors keep
    /// their unfiltered relative order under every sort mode.
    #[test]
    fn filter_preserves_relative_order() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "keep-one"),
            fixture_ws(2, 1, "drop-me"),
            fixture_ws(3, 1, "keep-two"),
        ];
        let mut maps = Maps::default();
        maps.attention.insert(WorkspaceId(3));
        // Unfiltered, attention pulls keep-two to the front.
        let unfiltered = order(&repos, &ws, &maps, UpdatesSort::Default);
        let expected: Vec<WorkspaceId> = unfiltered
            .into_iter()
            .filter(|id| *id != WorkspaceId(2))
            .collect();
        assert_eq!(
            order_filtered(&repos, &ws, &maps, UpdatesSort::Default, Some("keep")),
            expected
        );
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::data::store::{Repo, RepoId, Workspace, WorkspaceId, WorkspaceState};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn fixture_repo(id: i64, name: &str) -> Repo {
        Repo {
            id: RepoId(id),
            name: name.to_string(),
            path: PathBuf::from("/tmp/r"),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
            sort_order: 0,
        }
    }

    fn fixture_ws(id: i64, repo: i64, name: &str) -> (RepoId, Workspace) {
        (
            RepoId(repo),
            Workspace {
                id: WorkspaceId(id),
                repo_id: RepoId(repo),
                name: name.to_string(),
                branch: "main".to_string(),
                worktree_path: PathBuf::from("/tmp/ws"),
                state: WorkspaceState::Ready,
                setup_status: crate::data::store::SetupStatus::Ok,
                created_at: 0,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            },
        )
    }

    /// Draw the panel and flatten the buffer to one string per row.
    fn draw(repos: &[Repo], ws: &[(RepoId, Workspace)], filter: Option<&str>) -> String {
        let theme = Theme::ansi();
        let (events, activity, attention, awaiting, statuses, lifecycles) = (
            HashMap::new(),
            HashMap::new(),
            HashSet::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
        );
        let inputs = PanelInputs {
            repos,
            workspaces: ws,
            events: &events,
            activity: &activity,
            needs_attention: &attention,
            awaiting: &awaiting,
            statuses: &statuses,
            lifecycles: &lifecycles,
        };
        let mut term = Terminal::new(TestBackend::new(80, 25)).unwrap();
        term.draw(|f| {
            render_updates_panel(
                f,
                f.area(),
                &inputs,
                0,
                10_000,
                UpdatesSort::Default,
                filter,
                &theme,
            )
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A repo whose workspaces all filter out loses its header too — an
    /// empty section header is pure noise in a panel meant to be scanned.
    #[test]
    fn filtered_out_repo_draws_no_header() {
        let repos = vec![fixture_repo(1, "alpha-repo"), fixture_repo(2, "beta-repo")];
        let ws = vec![fixture_ws(1, 1, "one"), fixture_ws(2, 2, "two")];
        let rendered = draw(&repos, &ws, Some("one"));
        assert!(rendered.contains("alpha-repo"), "{rendered}");
        assert!(!rendered.contains("beta-repo"), "{rendered}");
    }

    /// The two empty states are distinguishable: a filter that hit nothing
    /// must not read as "you have no workspaces".
    #[test]
    fn empty_states_distinguish_filter_from_no_workspaces() {
        let repos = vec![fixture_repo(1, "alpha-repo")];
        let ws = vec![fixture_ws(1, 1, "one")];
        assert!(draw(&repos, &ws, Some("zzz")).contains("(no matching workspaces)"));
        assert!(draw(&repos, &[], None).contains("(no workspaces)"));
    }
}
