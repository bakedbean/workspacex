//! Renderer and ordering for `Modal::UpdatesPanel` — the `Ctrl-x u` panel
//! that lists every workspace from inside an attached session. A stripped
//! down dashboard: it walks the dashboard's own ordering
//! (`crate::ui::dashboard::ordered_sections`, under the dashboard's live
//! group and sort modes) and draws each row from the same `RowInputs` the
//! dashboard row is built from.

use super::*;
use crate::data::store::{Repo, Workspace, WorkspaceId};
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::sort::SortMode;
use crate::ui::dashboard::{OrderedSection, SectionKind, WorkspaceItem, ordered_sections};
use crate::ui::text::{FILTER_ECHO_MAX, truncate, truncate_pad};

/// Everything the panel reads, gathered once per render or keypress by
/// `crate::app::render::panel_inputs`. The renderer and the key handler
/// both derive row order from one of these, so the row drawn at an index
/// is always the row `Enter` acts on.
pub struct PanelInputs<'a> {
    pub repos: Vec<&'a Repo>,
    /// One item per workspace, built by the dashboard's own row builder —
    /// status, age, PR state and diff are exactly what the dashboard shows.
    pub items: Vec<WorkspaceItem<'a>>,
    /// The store rows, for the workspace name and lifecycle state.
    pub workspaces: &'a [(RepoId, Workspace)],
    pub events: &'a HashMap<WorkspaceId, crate::activity::events::WorkspaceEvents>,
    pub activity: HashMap<WorkspaceId, crate::ui::updates_bar::ActivityState>,
    pub needs_attention: &'a HashSet<WorkspaceId>,
    pub awaiting: HashMap<WorkspaceId, (String, i64)>,
    /// The dashboard's live grouping and ordering, so the panel lists
    /// workspaces in the order the user last saw them on the dashboard.
    pub group_mode: GroupMode,
    pub sort_mode: SortMode,
    pub blocked_pin_max_age_secs: u64,
    /// Width of the PR-chip cell — the dashboard's configured chip column,
    /// so a chip truncates identically in both views.
    pub pr_width: usize,
}

impl PanelInputs<'_> {
    fn workspace(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces
            .iter()
            .find(|(_, w)| w.id == id)
            .map(|(_, w)| w)
    }

    fn item(&self, id: WorkspaceId) -> Option<&WorkspaceItem<'_>> {
        self.items.iter().find(|i| i.workspace_id == id)
    }

    /// The status text a row would display for `w` — the same string
    /// `workspace_row` draws in the status column. Its one caller is the
    /// filter in [`panel_sections`], which matches needles against it.
    fn status_text(&self, w: &Workspace) -> String {
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

/// The per-frame view state for the panel: which row is selected and the
/// filter needle (if any). Bundled separately from `PanelInputs` — this is
/// state the modal owns and cycles every render, where `PanelInputs` is
/// derived from app caches.
pub struct PanelView<'a> {
    pub selected: usize,
    pub filter: Option<&'a str>,
}

/// The filter needle, if the user has typed anything past `/`. `Some("")`
/// (filter mode on, nothing typed yet) collapses to `None` — both mean
/// "show every row".
///
/// The single source for that rule. The row list and the empty-state
/// message both go through here: if they each encoded it and the two ever
/// drifted, the panel would tell the user "no matching workspaces" over a
/// list that is plainly not narrowed (or vice versa).
fn active_needle(filter: Option<&str>) -> Option<&str> {
    filter.filter(|f| !f.is_empty())
}

impl PanelView<'_> {
    /// This view's [`active_needle`].
    fn active_needle(&self) -> Option<&str> {
        active_needle(self.filter)
    }
}

/// Widest the panel gets. Wider than the other modals to pay for the PR
/// and diff cells on every row.
pub const PANEL_MAX_WIDTH: u16 = 100;

/// Cap on the workspace-name column so one very long name can't starve the
/// status column of the entire panel.
const NAME_COL_MAX: usize = 28;

/// Chars consumed left of the name column: 2-space indent + glyph + space.
const ROW_PREFIX_W: usize = 4;

/// Gap between adjacent columns (name→chip→diff→status, status→age).
const COL_GAP_W: usize = 2;

/// Width of the diff cell, the dashboard row's.
const DIFF_W: usize = crate::ui::dashboard::row::DIFF_WIDTH;

/// The least status text a row keeps before it starts shedding the diff
/// and PR cells: enough for the longest fixed label (`no session`) plus a
/// little of a live one.
const STATUS_MIN_W: usize = 12;

/// Width of the shared workspace-name column: as wide as the longest name,
/// capped at [`NAME_COL_MAX`] and clamped so prefix + name + gap always
/// leave at least one char of status text in the narrowest panel. Shared
/// across every section so status texts start at one fixed column for
/// the whole panel.
fn name_col_width<'a>(names: impl Iterator<Item = &'a str>, row_width: usize) -> usize {
    let cap = NAME_COL_MAX.min(row_width.saturating_sub(ROW_PREFIX_W + COL_GAP_W + 1));
    names.map(|n| n.chars().count()).max().unwrap_or(0).min(cap)
}

/// Case-insensitive substring match against the workspace name, the owning
/// repo's name, and the row's live status text. Mirrors the dashboard's
/// `matches_filter`, whose three fields are the same idea: what the row is
/// called, where it lives, and what it currently says.
fn matches_filter(w: &Workspace, repo_name: &str, status_text: &str, needle: &str) -> bool {
    let needle = needle.to_lowercase();
    w.name.to_lowercase().contains(&needle)
        || repo_name.to_lowercase().contains(&needle)
        || status_text.to_lowercase().contains(&needle)
}

/// The panel's sections, in display order: the dashboard's ordering
/// ([`ordered_sections`] under the dashboard's live group and sort modes)
/// applied to the rows that survive `filter`. Unlike the dashboard the
/// panel never folds a repo or collapses quiet ones — every workspace is
/// one keystroke from being switched to — and it omits repos left with no
/// rows, since a bare header is noise in a panel meant to be scanned.
///
/// Used by both the renderer (to draw rows) and, flattened through
/// [`ordered_workspaces_for_panel`], the key handler (to map the selected
/// index back to a workspace id).
pub fn panel_sections(inputs: &PanelInputs<'_>, filter: Option<&str>) -> Vec<OrderedSection> {
    let needle = active_needle(filter);
    let shown: Vec<WorkspaceItem<'_>> = inputs
        .items
        .iter()
        .filter(|item| {
            let Some(n) = needle else {
                return true;
            };
            inputs
                .workspace(item.workspace_id)
                .is_some_and(|w| matches_filter(w, &item.repo.name, &inputs.status_text(w), n))
        })
        .cloned()
        .collect();
    ordered_sections(
        &inputs.repos,
        &shown,
        inputs.group_mode,
        inputs.sort_mode,
        inputs.blocked_pin_max_age_secs,
    )
    .into_iter()
    .filter(|s| !s.workspace_ids.is_empty())
    .collect()
}

/// [`panel_sections`] flattened to the workspace ids in row order — the
/// list `PanelView::selected` indexes into.
pub fn ordered_workspaces_for_panel(
    inputs: &PanelInputs<'_>,
    filter: Option<&str>,
) -> Vec<WorkspaceId> {
    panel_sections(inputs, filter)
        .into_iter()
        .flat_map(|s| s.workspace_ids)
        .collect()
}

/// Footer hint line, sized to fit the widest panel (`PANEL_MAX_WIDTH` − 2
/// border). The `↑↓` / `↵` glyphs match the dashboard footer's. The sort
/// and group hints name the dashboard's modes, because that is what `o`
/// and `G` cycle here. While filtering, printable keys are filter text
/// rather than shortcuts, so only the hints that still work are listed.
fn footer_text(sort: SortMode, group: GroupMode, filter: Option<&str>) -> String {
    match filter {
        Some(needle) => format!(
            "/{}    [esc] clear  [\u{2191}\u{2193}] move  [\u{21b5}] switch",
            truncate(needle, FILTER_ECHO_MAX)
        ),
        None => format!(
            "[\u{2191}\u{2193}] move  [\u{21b5}] switch  [v/s] split  [o] sort:{}  [G] group:{}  [/] filter  [esc] close",
            sort.as_str(),
            group_label(group)
        ),
    }
}

/// The group mode's tab label on the dashboard's title row.
fn group_label(group: GroupMode) -> &'static str {
    match group {
        GroupMode::Repo => "repo",
        GroupMode::Attention => "attention",
    }
}

/// Render the floating workspace-updates panel. Reads live App state via
/// borrowed slices so the panel updates on every render tick.
pub fn render_updates_panel(
    f: &mut Frame,
    area: Rect,
    inputs: &PanelInputs<'_>,
    view: &PanelView<'_>,
    now_ms: i64,
    theme: &Theme,
) {
    // Sizing: up to PANEL_MAX_WIDTH cols wide, ~25 rows tall, but never
    // larger than the area.
    let w = area.width.clamp(20, PANEL_MAX_WIDTH);
    let h = area.height.clamp(8, 25);
    let inner = panel_frame(f, area, w, h, " Workspace updates ", theme);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let body_area = chunks[0];
    let footer_area = chunks[1];

    let sections = panel_sections(inputs, view.filter);
    // Row labels: the workspace name, prefixed by its repo when the repo
    // headers are gone (attention grouping) — the same `<repo>/<name>` the
    // dashboard's flat rows use to keep repo context.
    let labels: Vec<Vec<(WorkspaceId, String)>> = sections
        .iter()
        .map(|s| {
            s.workspace_ids
                .iter()
                .map(|id| {
                    let name = inputs.workspace(*id).map(|w| w.name.as_str()).unwrap_or("");
                    let label = match s.kind {
                        SectionKind::Repo(_) => name.to_string(),
                        SectionKind::Attention(_) => {
                            let repo = inputs.item(*id).map(|i| i.repo.name.as_str()).unwrap_or("");
                            format!("{repo}/{name}")
                        }
                    };
                    (*id, label)
                })
                .collect()
        })
        .collect();

    // One shared name column for the whole panel so every status text starts
    // at the same column regardless of which section a row is in.
    let row_width = body_area.width as usize;
    let name_col = name_col_width(
        labels.iter().flatten().map(|(_, label)| label.as_str()),
        row_width,
    );

    let mut lines: Vec<Line> = Vec::new();
    let mut selected_visual_line: Option<usize> = None;
    let mut flat_index = 0usize;
    for (section, rows) in sections.iter().zip(&labels) {
        let (title, title_style) = match section.kind {
            SectionKind::Repo(rid) => (
                inputs
                    .repos
                    .iter()
                    .find(|r| r.id == rid)
                    .map(|r| r.name.clone())
                    .unwrap_or_default(),
                theme.header_style(),
            ),
            SectionKind::Attention(a) => (a.label().trim_start().to_string(), theme.header_style()),
        };
        lines.push(Line::from(vec![
            Span::styled(title, title_style),
            Span::styled(format!("  ({})", rows.len()), theme.dim_style()),
        ]));
        for (id, label) in rows {
            let is_selected = flat_index == view.selected;
            flat_index += 1;
            if is_selected {
                selected_visual_line = Some(lines.len());
            }
            let Some(w) = inputs.workspace(*id) else {
                continue;
            };
            let item = inputs.item(*id);
            let row = RowData {
                label,
                failed: w.state == crate::data::store::WorkspaceState::Failed,
                events: inputs.events.get(id),
                activity: inputs.activity.get(id).copied(),
                needs_attention: inputs.needs_attention.contains(id),
                awaiting: inputs.awaiting.get(id),
                status: item.map(|i| i.status).unwrap_or(Status::Idle),
                lifecycle: item.and_then(|i| i.row.lifecycle),
                pr_number: item.and_then(|i| i.row.pr_number),
                review: item.and_then(|i| i.row.review),
                unresolved: item.and_then(|i| i.row.unresolved),
                diff: item.and_then(|i| i.row.diff),
            };
            lines.push(workspace_row(
                &row,
                is_selected,
                now_ms,
                name_col,
                inputs.pr_width,
                row_width,
                theme,
            ));
        }
        lines.push(Line::from(""));
    }
    // Nothing to show. Separate the two causes: an empty panel and a panel
    // whose rows the needle hid are very different situations for the user.
    if lines.is_empty() {
        let msg = if view.active_needle().is_some() {
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
        Paragraph::new(footer_text(
            inputs.sort_mode,
            inputs.group_mode,
            view.filter,
        ))
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

/// The status text a row displays, plus the timestamp its age column is
/// anchored to. Split out of `workspace_row` so the filter matches the same
/// string the row is built from: a row never displays status text the
/// filter fails to match. (Not the converse — `workspace_row` truncates
/// this text to the status column, so a needle that only matches the
/// truncated-away tail still keeps the row. Matching the full text is the
/// more useful direction: what the user typed was there, panel width just
/// hid it.)
fn row_status_text(
    w: &Workspace,
    events: Option<&crate::activity::events::WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&(String, i64)>,
) -> (String, Option<i64>) {
    status_text_for(
        w.state == crate::data::store::WorkspaceState::Failed,
        events,
        activity,
        needs_attention,
        awaiting,
    )
}

fn status_text_for(
    failed: bool,
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
    if failed {
        return ("failed".to_string(), None);
    }
    if events.and_then(|e| e.latest.as_ref()).is_some() {
        return ("resumable".to_string(), None);
    }
    ("no session".to_string(), None)
}

/// One row's worth of inputs for [`workspace_row`].
struct RowData<'a> {
    /// The name cell: the workspace name, or `<repo>/<name>` when the
    /// section headers don't carry the repo.
    label: &'a str,
    /// The workspace's own lifecycle failed (no worktree).
    failed: bool,
    events: Option<&'a crate::activity::events::WorkspaceEvents>,
    activity: Option<crate::ui::updates_bar::ActivityState>,
    needs_attention: bool,
    awaiting: Option<&'a (String, i64)>,
    status: Status,
    lifecycle: Option<BranchLifecycle>,
    pr_number: Option<u32>,
    review: Option<crate::git::forge::ReviewDecision>,
    unresolved: Option<u32>,
    diff: Option<crate::git::DiffStats>,
}

fn workspace_row<'a>(
    row: &RowData<'a>,
    is_selected: bool,
    now_ms: i64,
    name_col: usize,
    pr_width: usize,
    row_width: usize,
    theme: &Theme,
) -> Line<'a> {
    use crate::ui::updates_bar::{ActivityState, format_age, glyph_for_activity};
    let glyph = if row.failed {
        '✕'
    } else if row.needs_attention {
        row.activity.map(glyph_for_activity).unwrap_or('⚠')
    } else {
        match row.activity {
            Some(ActivityState::Active) | Some(ActivityState::Idle) => '●',
            Some(ActivityState::AwaitingAnswer) => '?',
            Some(ActivityState::Complete) => '\u{2713}',
            Some(ActivityState::Awaiting)
            | Some(ActivityState::Stalled)
            | Some(ActivityState::Waiting) => '⚠',
            Some(ActivityState::Off) | None => {
                if row.events.and_then(|e| e.latest.as_ref()).is_some() {
                    '↻'
                } else {
                    '○'
                }
            }
        }
    };
    let (status_text, age_anchor_ms) = status_text_for(
        row.failed,
        row.events,
        row.activity,
        row.needs_attention,
        row.awaiting,
    );
    let age = age_anchor_ms.map(|t| format_age(now_ms.saturating_sub(t)));

    // Failed overrides the canonical status hue with `err` — a failed
    // workspace is the same urgency signal regardless of its prior status.
    let status_fg = if row.failed {
        theme.err_style()
    } else {
        theme.status_style(row.status)
    };
    // Lifecycle wins on the name even when the workspace is failed — a
    // failed workspace can still have a merged PR. Bold so the name
    // still reads as a name. When there's no lifecycle hue, explicitly
    // reset fg so the surrounding Block's dim style can't leak through
    // ratatui's style inheritance and dim the workspace name.
    let name_style = theme
        .lifecycle_style(row.lifecycle)
        .unwrap_or_else(|| Style::default().fg(ratatui::style::Color::Reset))
        .add_modifier(Modifier::BOLD);

    // Column layout: indent+glyph | name | pr chip | diff | status |
    // right-aligned age. The chip and diff cells are the dashboard row's
    // own, at the dashboard's widths. In a panel too narrow to hold them
    // and still show status text, the diff sheds first, then the chip —
    // status is what the panel is for. The status text is truncated so it
    // can never collide with the age column, and the row is padded to
    // exactly `row_width` so the selection background spans the full row.
    let mut avail = row_width.saturating_sub(ROW_PREFIX_W + name_col + COL_GAP_W);
    let pr_cell = pr_width + COL_GAP_W;
    let diff_cell = DIFF_W + COL_GAP_W;
    let (show_pr, show_diff) = if avail >= pr_cell + diff_cell + STATUS_MIN_W {
        (true, true)
    } else if avail >= pr_cell + STATUS_MIN_W {
        (true, false)
    } else {
        (false, false)
    };
    if show_pr {
        avail -= pr_cell;
    }
    if show_diff {
        avail -= diff_cell;
    }
    // Drop the age column when it (plus its gap) wouldn't leave at least one
    // char of status text — a clipped age is worse than no age.
    let age = age.filter(|a| a.chars().count() + COL_GAP_W < avail);
    let age_w = age.as_ref().map(|a| a.chars().count()).unwrap_or(0);
    let age_reserved = if age_w > 0 { age_w + COL_GAP_W } else { 0 };
    let status_budget = avail.saturating_sub(age_reserved);
    let status_txt = truncate(&status_text, status_budget);
    let pad_w = status_budget.saturating_sub(status_txt.chars().count()) + age_reserved - age_w;

    let mut spans = vec![
        Span::raw("  "),
        Span::styled(format!("{glyph} "), status_fg),
        Span::styled(truncate_pad(row.label, name_col), name_style),
        Span::raw(" ".repeat(COL_GAP_W)),
    ];
    if show_pr {
        spans.extend(crate::ui::dashboard::row::pr_chip_spans(
            row.lifecycle,
            row.pr_number,
            row.review,
            row.unresolved,
            pr_width,
            theme,
        ));
        spans.push(Span::raw(" ".repeat(COL_GAP_W)));
    }
    if show_diff {
        spans.extend(crate::ui::dashboard::row::diff_spans(
            row.diff, DIFF_W, theme,
        ));
        spans.push(Span::raw(" ".repeat(COL_GAP_W)));
    }
    spans.push(Span::styled(status_txt, status_fg));
    spans.push(Span::raw(" ".repeat(pad_w)));
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

    /// Positional shim over `workspace_row` so the row tests read as a
    /// flat list of the signals that drive glyph, text and color.
    #[allow(clippy::too_many_arguments)]
    fn row_line<'a>(
        w: &'a Workspace,
        events: Option<&'a crate::activity::events::WorkspaceEvents>,
        activity: Option<ActivityState>,
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
        let row = RowData {
            label: &w.name,
            failed: w.state == WorkspaceState::Failed,
            events,
            activity,
            needs_attention,
            awaiting,
            status,
            lifecycle,
            pr_number: None,
            review: None,
            unresolved: None,
            diff: None,
        };
        workspace_row(
            &row,
            is_selected,
            now_ms,
            name_col,
            crate::ui::dashboard::row::DEFAULT_PR_WIDTH,
            row_width,
            theme,
        )
    }

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
            name_color: None,
        }
    }

    /// A `WorkspaceEvents` carrying one latest event, the shape the
    /// active-with-event and resumable branches of `row_status_text` read.
    fn events_with_latest(
        display: &str,
        timestamp_ms: i64,
    ) -> crate::activity::events::WorkspaceEvents {
        crate::activity::events::WorkspaceEvents {
            latest: Some(crate::activity::events::EventSnapshot {
                kind: crate::activity::events::EventKind::AssistantToolUse,
                display: display.to_string(),
                timestamp_ms,
            }),
            ..Default::default()
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
        let line = row_line(
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
            98,
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
        let line = row_line(
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
            98,
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
        let line = row_line(
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
            98,
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
    /// `workspace_row` draws it and the panel filter matches against it. If
    /// the extraction ever drifted from what the row renders, the filter
    /// would fail to match text the user can plainly see.
    ///
    /// Every branch of the chain is covered, including the two that read
    /// `events`: the active-with-event branch (the only one returning
    /// *dynamic* text — the latest event's `display`, and the text a user is
    /// most likely to filter on) and the resumable branch.
    #[test]
    #[allow(clippy::type_complexity)]
    fn row_status_text_matches_what_the_row_renders() {
        let theme = Theme::ansi();
        let mut failed = fixture_workspace("gamma");
        failed.state = WorkspaceState::Failed;
        let awaiting = ("Bash".to_string(), 5_000i64);
        let (alpha, beta, delta, epsilon, zeta) = (
            fixture_workspace("alpha"),
            fixture_workspace("beta"),
            fixture_workspace("delta"),
            fixture_workspace("epsilon"),
            fixture_workspace("zeta"),
        );
        let live = events_with_latest("Edit src/main.rs", 9_000);
        // (workspace, events, activity, needs_attention, awaiting, expected)
        let cases: [(
            &Workspace,
            Option<&crate::activity::events::WorkspaceEvents>,
            Option<ActivityState>,
            bool,
            Option<&(String, i64)>,
            &str,
        ); 6] = [
            (
                &alpha,
                None,
                Some(ActivityState::Awaiting),
                true,
                Some(&awaiting),
                "awaiting permission: Bash",
            ),
            (
                &beta,
                None,
                Some(ActivityState::Stalled),
                true,
                None,
                "stalled",
            ),
            (&failed, None, None, false, None, "failed"),
            (&delta, None, None, false, None, "no session"),
            // Active with a live event: the row shows the event's display.
            (
                &epsilon,
                Some(&live),
                Some(ActivityState::Active),
                false,
                None,
                "Edit src/main.rs",
            ),
            // An event but no live activity: the session can be resumed.
            (&zeta, Some(&live), None, false, None, "resumable"),
        ];
        for (w, events, activity, attention, awaiting, expected) in cases {
            let (text, _) = row_status_text(w, events, activity, attention, awaiting);
            assert_eq!(text, expected, "row_status_text for {}", w.name);
            let line = row_line(
                w,
                events,
                activity,
                attention,
                awaiting,
                false,
                Status::Idle,
                None,
                10_000,
                20,
                98,
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
            let line = row_line(
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
                98,
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
        let line = row_line(
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
            98,
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
            let line = row_line(
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
                98,
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
            let line = row_line(
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
                98,
                &theme,
            );
            line_text(&line)
        };
        let col_short = row(&short).find("no session").unwrap();
        let col_long = row(&long).find("no session").unwrap();
        assert_eq!(col_short, col_long, "status must start at a fixed column");
    }

    /// A row with PR and diff data, for the cell tests below.
    fn pr_row<'a>(w: &'a Workspace, lifecycle: Option<BranchLifecycle>) -> RowData<'a> {
        RowData {
            label: &w.name,
            failed: false,
            events: None,
            activity: None,
            needs_attention: false,
            awaiting: None,
            status: Status::Idle,
            lifecycle,
            pr_number: Some(42),
            review: Some(crate::git::forge::ReviewDecision::ChangesRequested),
            unresolved: Some(3),
            diff: Some(crate::git::DiffStats {
                added: 12,
                removed: 3,
            }),
        }
    }

    /// The PR chip and the diff sit between the name and the status text,
    /// drawn by the dashboard row's own cell builders: chip in the
    /// lifecycle color with the verdict mark in its own color, diff as ok
    /// `+N` / err `−N`.
    #[test]
    fn workspace_row_draws_the_pr_chip_and_diff_between_name_and_status() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let row = pr_row(&w, Some(BranchLifecycle::PrOpen));
        let line = workspace_row(
            &row,
            false,
            10_000,
            20,
            crate::ui::dashboard::row::DEFAULT_PR_WIDTH,
            98,
            &theme,
        );
        let body = line_text(&line);
        let name_at = body.find("alpha").unwrap();
        let chip_at = body.find("#42 open").expect("pr chip");
        let diff_at = body.find("+12 −3").expect("diff cell");
        let status_at = body.find("no session").expect("status text");
        assert!(
            name_at < chip_at && chip_at < diff_at && diff_at < status_at,
            "{body:?}"
        );
        assert_eq!(span_containing(&line, "#42").style.fg, Some(theme.ok));
        assert_eq!(
            span_containing(&line, "3").style.fg,
            Some(theme.err),
            "verdict mark"
        );
        assert_eq!(span_containing(&line, "+12").style.fg, Some(theme.ok));
        assert_eq!(span_containing(&line, "−3").style.fg, Some(theme.err));
    }

    /// Rows without a PR or a diff keep both cells blank, so the status
    /// column starts at the same x on every row of the panel.
    #[test]
    fn workspace_row_keeps_status_aligned_with_and_without_pr_and_diff() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let with = pr_row(&w, Some(BranchLifecycle::PrOpen));
        let mut without = pr_row(&w, None);
        without.pr_number = None;
        without.review = None;
        without.diff = None;
        let at = |row: &RowData<'_>| {
            let line = workspace_row(
                row,
                false,
                10_000,
                20,
                crate::ui::dashboard::row::DEFAULT_PR_WIDTH,
                98,
                &theme,
            );
            let body = line_text(&line);
            assert_eq!(
                body.chars().count(),
                98,
                "row padded to full width: {body:?}"
            );
            body[..body.find("no session").unwrap()].chars().count()
        };
        assert_eq!(at(&with), at(&without));
        let body = line_text(&workspace_row(
            &without,
            false,
            10_000,
            20,
            crate::ui::dashboard::row::DEFAULT_PR_WIDTH,
            98,
            &theme,
        ));
        assert!(!body.contains('#') && !body.contains('+'), "{body:?}");
    }

    /// When the panel is too narrow for everything, the diff drops first,
    /// then the PR chip, so the status text always keeps some room and the
    /// row never overflows.
    #[test]
    fn workspace_row_sheds_diff_then_chip_as_the_panel_narrows() {
        let theme = Theme::ansi();
        let w = fixture_workspace("alpha");
        let row = pr_row(&w, Some(BranchLifecycle::PrOpen));
        let render = |width: usize| {
            let line = workspace_row(
                &row,
                false,
                10_000,
                20,
                crate::ui::dashboard::row::DEFAULT_PR_WIDTH,
                width,
                &theme,
            );
            let body = line_text(&line);
            assert!(
                body.chars().count() <= width,
                "row must not overflow at {width}: {body:?}"
            );
            body
        };
        let wide = render(98);
        assert!(wide.contains("#42") && wide.contains("+12"), "{wide:?}");
        let mid = render(66);
        assert!(mid.contains("#42"), "chip survives first: {mid:?}");
        assert!(!mid.contains("+12"), "diff drops first: {mid:?}");
        assert!(mid.contains("no session"), "{mid:?}");
        let narrow = render(50);
        assert!(!narrow.contains("#42"), "chip drops next: {narrow:?}");
        assert!(narrow.contains("no session"), "{narrow:?}");
    }

    /// Names wider than the name column truncate with an ellipsis instead of
    /// pushing the status column out of alignment.
    #[test]
    fn workspace_row_truncates_overlong_name_keeping_column() {
        let theme = Theme::ansi();
        let w = fixture_workspace("this-name-is-way-past-the-column");
        let line = row_line(
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
            98,
            &theme,
        );
        let body = line_text(&line);
        assert!(body.contains('…'), "expected ellipsis in: {body}");
        // Column position in chars, not bytes — the glyph and ellipsis are
        // multi-byte.
        let status_col = body[..body.find("no session").unwrap()].chars().count();
        assert_eq!(
            status_col,
            4 + 20 + 2 + (crate::ui::dashboard::row::DEFAULT_PR_WIDTH + 2) + (DIFF_W + 2),
            "status must start right after prefix, name, pr and diff cells"
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
        let line = row_line(
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
            98,
            &theme,
        );
        let body = line_text(&line);
        assert_eq!(body.chars().count(), 98, "row must fill row_width");
        assert!(
            body.ends_with("5s"),
            "age must sit at the right edge: {body:?}"
        );
        let age_span = line.spans.last().unwrap();
        assert_eq!(age_span.style, theme.dim_style(), "age renders dim");

        // A row without an age still pads to the full width.
        let no_age = row_line(
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
            98,
            &theme,
        );
        assert_eq!(line_text(&no_age).chars().count(), 98);
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
        let line = row_line(
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
        let line = row_line(
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
        let line = row_line(
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
            98,
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
    use crate::ui::dashboard::sort::BLOCKED_PIN_MAX_AGE_DEFAULT_SECS;
    use crate::ui::modal::updates_panel::test_fixtures::*;

    /// Bundles the signal maps and per-row overrides the ordering reads, so
    /// each test only fills in what it exercises.
    #[derive(Default)]
    struct Maps {
        events: HashMap<WorkspaceId, crate::activity::events::WorkspaceEvents>,
        activity: HashMap<WorkspaceId, crate::ui::updates_bar::ActivityState>,
        attention: HashSet<WorkspaceId>,
        awaiting: HashMap<WorkspaceId, (String, i64)>,
        statuses: HashMap<WorkspaceId, Status>,
        ago: HashMap<WorkspaceId, u64>,
        group_mode: Option<GroupMode>,
        sort_mode: Option<SortMode>,
    }

    fn inputs<'a>(
        repos: &'a [Repo],
        ws: &'a [(RepoId, Workspace)],
        maps: &'a Maps,
    ) -> PanelInputs<'a> {
        PanelInputs {
            repos: repos.iter().collect(),
            items: ws
                .iter()
                .map(|(rid, w)| {
                    let repo = repos.iter().find(|r| r.id == *rid).expect("repo for ws");
                    item(
                        repo,
                        w,
                        maps.statuses.get(&w.id).copied().unwrap_or(Status::Idle),
                        maps.ago.get(&w.id).copied(),
                    )
                })
                .collect(),
            workspaces: ws,
            events: &maps.events,
            activity: maps.activity.clone(),
            needs_attention: &maps.attention,
            awaiting: maps.awaiting.clone(),
            group_mode: maps.group_mode.unwrap_or_default(),
            sort_mode: maps.sort_mode.unwrap_or_default(),
            blocked_pin_max_age_secs: BLOCKED_PIN_MAX_AGE_DEFAULT_SECS,
            pr_width: crate::ui::dashboard::row::DEFAULT_PR_WIDTH,
        }
    }

    fn order_filtered(
        repos: &[Repo],
        ws: &[(RepoId, Workspace)],
        maps: &Maps,
        filter: Option<&str>,
    ) -> Vec<WorkspaceId> {
        ordered_workspaces_for_panel(&inputs(repos, ws, maps), filter)
    }

    fn order(repos: &[Repo], ws: &[(RepoId, Workspace)], maps: &Maps) -> Vec<WorkspaceId> {
        order_filtered(repos, ws, maps, None)
    }

    /// The panel takes its within-repo order from the dashboard's sort
    /// mode: recency (name tiebreak among never-active rows) or status
    /// urgency — never a private order of its own.
    #[test]
    fn rows_within_a_repo_follow_the_dashboard_sort_mode() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "zeta"),
            fixture_ws(2, 1, "alpha"),
            fixture_ws(3, 1, "mid"),
        ];
        let mut maps = Maps::default();
        maps.statuses.insert(WorkspaceId(3), Status::Question);
        maps.ago.insert(WorkspaceId(3), 30);
        // Recency: mid is freshly blocked and pins to the top; the other two
        // never ran and tie, so their names decide.
        maps.sort_mode = Some(SortMode::Recency);
        assert_eq!(
            order(&repos, &ws, &maps),
            vec![WorkspaceId(3), WorkspaceId(2), WorkspaceId(1)]
        );
        // Status: question outranks idle; equal idle rows keep input order.
        maps.sort_mode = Some(SortMode::Status);
        assert_eq!(
            order(&repos, &ws, &maps),
            vec![WorkspaceId(3), WorkspaceId(1), WorkspaceId(2)]
        );
    }

    /// Repos list in their persisted `sort_order`, and rows never cross a
    /// repo boundary however urgent they are.
    #[test]
    fn repos_follow_their_persisted_order_and_rows_stay_inside_them() {
        let mut repos = vec![fixture_repo(1), fixture_repo(2)];
        repos[0].sort_order = 5;
        repos[1].sort_order = 1;
        let ws = vec![fixture_ws(1, 1, "r1-urgent"), fixture_ws(2, 2, "r2-idle")];
        let mut maps = Maps::default();
        maps.statuses.insert(WorkspaceId(1), Status::Stalled);
        maps.ago.insert(WorkspaceId(1), 5);
        assert_eq!(
            order(&repos, &ws, &maps),
            vec![WorkspaceId(2), WorkspaceId(1)],
            "repo 2 sorts first by sort_order regardless of repo 1's urgency"
        );
    }

    /// Attention grouping swaps repo sections for the dashboard's urgency
    /// sections. Unlike the dashboard, an all-idle repo is not collapsed:
    /// its rows stay reachable under IDLE.
    #[test]
    fn attention_grouping_lists_urgency_sections_and_keeps_idle_rows() {
        let repos = vec![fixture_repo(1), fixture_repo(2)];
        let ws = vec![
            fixture_ws(1, 1, "quiet-a"),
            fixture_ws(2, 2, "busy"),
            fixture_ws(3, 2, "asked"),
        ];
        let mut maps = Maps {
            group_mode: Some(GroupMode::Attention),
            ..Default::default()
        };
        maps.statuses.insert(WorkspaceId(2), Status::Thinking);
        maps.statuses.insert(WorkspaceId(3), Status::Question);
        let sections = panel_sections(&inputs(&repos, &ws, &maps), None);
        let kinds: Vec<SectionKind> = sections.iter().map(|s| s.kind.clone()).collect();
        assert_eq!(
            kinds,
            vec![
                SectionKind::Attention(crate::ui::dashboard::AttentionSection::NeedsAttention),
                SectionKind::Attention(crate::ui::dashboard::AttentionSection::Working),
                SectionKind::Attention(crate::ui::dashboard::AttentionSection::Idle),
            ]
        );
        assert_eq!(
            order(&repos, &ws, &maps),
            vec![WorkspaceId(3), WorkspaceId(2), WorkspaceId(1)]
        );
    }

    /// A repo with no rows draws no section — the dashboard shows its
    /// header, the panel doesn't.
    #[test]
    fn repos_without_rows_are_omitted() {
        let repos = vec![fixture_repo(1), fixture_repo(2)];
        let ws = vec![fixture_ws(1, 2, "only")];
        let sections = panel_sections(&inputs(&repos, &ws, &Maps::default()), None);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].kind, SectionKind::Repo(RepoId(2)));
    }

    /// The panel is capped at `PANEL_MAX_WIDTH` columns with a 1-col border
    /// each side. Check every sort × group combination — both mode names
    /// are inlined.
    #[test]
    fn footer_fits_the_panel_in_every_mode() {
        let limit = PANEL_MAX_WIDTH as usize - 2;
        for sort in [SortMode::Recency, SortMode::Status] {
            for group in [GroupMode::Repo, GroupMode::Attention] {
                let idle = footer_text(sort, group, None);
                assert!(
                    idle.chars().count() <= limit,
                    "idle footer for {sort:?}/{group:?} is {} chars: {idle}",
                    idle.chars().count()
                );
                let filtering = footer_text(sort, group, Some(&"x".repeat(60)));
                assert!(
                    filtering.chars().count() <= limit,
                    "filtering footer is {} chars: {filtering}",
                    filtering.chars().count()
                );
            }
        }
    }

    /// The footer names the dashboard modes the panel is following, next to
    /// the keys that change them.
    #[test]
    fn footer_shows_the_dashboard_sort_and_group_modes() {
        let f = footer_text(SortMode::Recency, GroupMode::Attention, None);
        assert!(f.contains("[o] sort:recency"), "{f}");
        assert!(f.contains("[G] group:attention"), "{f}");
        let f = footer_text(SortMode::Status, GroupMode::Repo, None);
        assert!(f.contains("[o] sort:status"), "{f}");
        assert!(f.contains("[G] group:repo"), "{f}");
    }

    /// Idle footer advertises the filter key; filtering footer echoes the
    /// needle and swaps `esc close` for `esc clear`, because that is what
    /// Esc does while a filter is up.
    #[test]
    fn footer_swaps_hints_when_filtering() {
        let idle = footer_text(SortMode::Recency, GroupMode::Repo, None);
        assert!(idle.contains("[/] filter"), "{idle}");
        assert!(idle.contains("[esc] close"), "{idle}");

        let filtering = footer_text(SortMode::Recency, GroupMode::Repo, Some("auth"));
        assert!(filtering.starts_with("/auth"), "{filtering}");
        assert!(filtering.contains("[esc] clear"), "{filtering}");
        assert!(!filtering.contains("[esc] close"), "{filtering}");
    }

    /// `/` with nothing typed still echoes, so the keypress has visible
    /// feedback before the first character.
    #[test]
    fn footer_echoes_empty_needle() {
        let filtering = footer_text(SortMode::Recency, GroupMode::Repo, Some(""));
        assert!(filtering.starts_with('/'), "{filtering}");
        assert!(filtering.contains("[esc] clear"), "{filtering}");
    }

    /// A long needle is truncated rather than pushing the key hints off
    /// the line.
    #[test]
    fn footer_truncates_a_long_needle() {
        let filtering = footer_text(SortMode::Recency, GroupMode::Repo, Some(&"x".repeat(60)));
        assert!(filtering.contains('…'), "{filtering}");
        assert!(filtering.contains("[↑↓] move"), "{filtering}");
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
            order_filtered(&repos, &ws, &maps, Some("AUTH")),
            vec![WorkspaceId(1)]
        );
    }

    /// A repo-name needle keeps every workspace in that repo — the same
    /// affordance the dashboard gives for "show me just this repo" — and
    /// matches case-insensitively, like the other two fields.
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
            order_filtered(&repos, &ws, &maps, Some("Repo1")),
            vec![WorkspaceId(1), WorkspaceId(2)]
        );
    }

    /// The needle also matches the live status text, so "permission" or
    /// "stalled" narrows to the rows that actually say that — again
    /// case-insensitively.
    #[test]
    fn filter_matches_status_text() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha"), fixture_ws(2, 1, "beta")];
        let mut maps = Maps::default();
        maps.awaiting
            .insert(WorkspaceId(1), ("Bash".to_string(), 1_000));
        assert_eq!(
            order_filtered(&repos, &ws, &maps, Some("PERMISSION")),
            vec![WorkspaceId(1)]
        );
        // beta has no session at all, so its status text is "no session".
        assert_eq!(
            order_filtered(&repos, &ws, &maps, Some("No Session")),
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
        assert_eq!(order_filtered(&repos, &ws, &maps, Some("")), all);
        assert_eq!(order_filtered(&repos, &ws, &maps, None), all);
    }

    /// A needle matching nothing yields an empty order — the renderer turns
    /// that into "(no matching workspaces)".
    #[test]
    fn filter_matching_nothing_yields_empty_order() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![fixture_ws(1, 1, "alpha")];
        let maps = Maps::default();
        assert!(order_filtered(&repos, &ws, &maps, Some("zzz")).is_empty());
    }

    /// Filtering narrows the list without reshuffling it: survivors keep
    /// their unfiltered relative order.
    #[test]
    fn filter_preserves_relative_order() {
        let repos = vec![fixture_repo(1)];
        let ws = vec![
            fixture_ws(1, 1, "keep-one"),
            fixture_ws(2, 1, "drop-me"),
            fixture_ws(3, 1, "keep-two"),
        ];
        let mut maps = Maps::default();
        // Unfiltered, the blocked pin pulls keep-two to the front.
        maps.statuses.insert(WorkspaceId(3), Status::Question);
        maps.ago.insert(WorkspaceId(3), 10);
        let unfiltered = order(&repos, &ws, &maps);
        assert_eq!(unfiltered[0], WorkspaceId(3), "sanity: pin applied");
        let expected: Vec<WorkspaceId> = unfiltered
            .into_iter()
            .filter(|id| *id != WorkspaceId(2))
            .collect();
        assert_eq!(order_filtered(&repos, &ws, &maps, Some("keep")), expected);
    }

    /// A failed workspace still lists (it can be archived from the panel's
    /// neighbour views) — its state is a row signal, not a filter.
    #[test]
    fn failed_workspaces_still_list() {
        let repos = vec![fixture_repo(1)];
        let mut ws = vec![fixture_ws(1, 1, "broken")];
        ws[0].1.state = WorkspaceState::Failed;
        assert_eq!(order(&repos, &ws, &Maps::default()), vec![WorkspaceId(1)]);
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::data::store::{Repo, RepoId, Workspace, WorkspaceId};
    use crate::ui::dashboard::sort::BLOCKED_PIN_MAX_AGE_DEFAULT_SECS;
    use crate::ui::modal::updates_panel::test_fixtures::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    /// Draw the panel and flatten the buffer to one string per row.
    fn draw(repos: &[Repo], ws: &[(RepoId, Workspace)], filter: Option<&str>) -> String {
        draw_with(repos, ws, filter, GroupMode::Repo, &HashMap::new())
    }

    fn draw_with(
        repos: &[Repo],
        ws: &[(RepoId, Workspace)],
        filter: Option<&str>,
        group_mode: GroupMode,
        statuses: &HashMap<WorkspaceId, Status>,
    ) -> String {
        let theme = Theme::ansi();
        let events = HashMap::new();
        let attention = HashSet::new();
        let inputs = PanelInputs {
            repos: repos.iter().collect(),
            items: ws
                .iter()
                .map(|(rid, w)| {
                    let repo = repos.iter().find(|r| r.id == *rid).expect("repo for ws");
                    item(
                        repo,
                        w,
                        statuses.get(&w.id).copied().unwrap_or(Status::Idle),
                        None,
                    )
                })
                .collect(),
            workspaces: ws,
            events: &events,
            activity: HashMap::new(),
            needs_attention: &attention,
            awaiting: HashMap::new(),
            group_mode,
            sort_mode: SortMode::Recency,
            blocked_pin_max_age_secs: BLOCKED_PIN_MAX_AGE_DEFAULT_SECS,
            pr_width: crate::ui::dashboard::row::DEFAULT_PR_WIDTH,
        };
        let view = PanelView {
            selected: 0,
            filter,
        };
        let mut term = Terminal::new(TestBackend::new(PANEL_MAX_WIDTH, 25)).unwrap();
        term.draw(|f| render_updates_panel(f, f.area(), &inputs, &view, 10_000, &theme))
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
        let repos = vec![
            fixture_repo_named(1, "alpha-repo"),
            fixture_repo_named(2, "beta-repo"),
        ];
        let ws = vec![fixture_ws(1, 1, "one"), fixture_ws(2, 2, "two")];
        let rendered = draw(&repos, &ws, Some("one"));
        assert!(rendered.contains("alpha-repo"), "{rendered}");
        assert!(!rendered.contains("beta-repo"), "{rendered}");
    }

    /// The two empty states are distinguishable: a filter that hit nothing
    /// must not read as "you have no workspaces".
    #[test]
    fn empty_states_distinguish_filter_from_no_workspaces() {
        let repos = vec![fixture_repo_named(1, "alpha-repo")];
        let ws = vec![fixture_ws(1, 1, "one")];
        assert!(draw(&repos, &ws, Some("zzz")).contains("(no matching workspaces)"));
        assert!(draw(&repos, &[], None).contains("(no workspaces)"));
    }

    /// Attention grouping draws the dashboard's section headers in place
    /// of repo headers, and every row carries its repo as a prefix — the
    /// only place the repo shows once its header is gone.
    #[test]
    fn attention_grouping_draws_section_headers_and_repo_prefixed_rows() {
        let repos = vec![
            fixture_repo_named(1, "alpha-repo"),
            fixture_repo_named(2, "beta-repo"),
        ];
        let ws = vec![fixture_ws(1, 1, "one"), fixture_ws(2, 2, "two")];
        let mut statuses = HashMap::new();
        statuses.insert(WorkspaceId(2), Status::Thinking);
        let rendered = draw_with(&repos, &ws, None, GroupMode::Attention, &statuses);
        assert!(rendered.contains("WORKING  (1)"), "{rendered}");
        assert!(rendered.contains("IDLE  (1)"), "{rendered}");
        assert!(rendered.contains("beta-repo/two"), "{rendered}");
        assert!(rendered.contains("alpha-repo/one"), "{rendered}");
        assert!(
            !rendered.contains("alpha-repo  ("),
            "no repo header in attention mode: {rendered}"
        );
        // WORKING lists above IDLE.
        assert!(
            rendered.find("WORKING").unwrap() < rendered.find("IDLE").unwrap(),
            "{rendered}"
        );
    }
}

/// Fixtures shared by the panel's test modules.
#[cfg(test)]
pub(super) mod test_fixtures {
    use super::*;
    use crate::data::store::{Repo, RepoId, Workspace, WorkspaceId, WorkspaceState};
    use crate::ui::dashboard::row::RowInputs;
    use std::path::PathBuf;

    pub(super) fn fixture_repo(id: i64) -> Repo {
        fixture_repo_named(id, &format!("repo{id}"))
    }

    pub(super) fn fixture_repo_named(id: i64, name: &str) -> Repo {
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

    pub(super) fn fixture_ws(id: i64, repo: i64, name: &str) -> (RepoId, Workspace) {
        (
            RepoId(repo),
            Workspace {
                id: WorkspaceId(id),
                repo_id: RepoId(repo),
                name: name.to_string(),
                branch: name.to_string(),
                worktree_path: PathBuf::from("/tmp/ws"),
                state: WorkspaceState::Ready,
                setup_status: crate::data::store::SetupStatus::Ok,
                created_at: 0,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
                name_color: None,
            },
        )
    }

    /// A dashboard item for `w` with the given status and age, everything
    /// else at rest — what `build_workspace_items` would produce for a
    /// workspace with no session, PR or diff.
    pub(super) fn item<'a>(
        repo: &'a Repo,
        w: &Workspace,
        status: Status,
        ago_secs: Option<u64>,
    ) -> WorkspaceItem<'a> {
        WorkspaceItem {
            repo,
            workspace_id: w.id,
            status,
            row: RowInputs {
                agent: w.agent,
                peers: Vec::new(),
                status,
                branch: w.branch.clone(),
                pr_number: None,
                procs: 0,
                diff: None,
                column: None,
                ago_secs,
                selected: false,
                yolo: false,
                badge: None,
                undelivered_mail: false,
                shared: false,
                shared_active: false,
                has_multi_pane_layout: false,
                lifecycle: None,
                review: None,
                unresolved: None,
                nerd_fonts: false,
                name_color: None,
                workspace_id: w.id,
            },
        }
    }
}
