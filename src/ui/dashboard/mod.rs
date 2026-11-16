//! Top-level dashboard render entry point. Owns `DashboardState` and
//! the public `DashboardInputs` type that the caller assembles in `app.rs`.

pub mod by_attention;
pub mod by_repo;
pub mod column_content;
pub mod detail;
#[cfg(test)]
pub(crate) mod fixture;
pub mod hidden;
pub mod layout;
pub mod row;
pub mod side_panel;
pub mod sort;
pub mod sparkline;
pub mod spinner;
pub mod status;
pub mod tall;

use crate::app::SelectionTarget;
use crate::data::hide_rules::{HideRule, HideSubject};
use crate::data::store::{Repo, RepoId};
use crate::git::forge::BranchLifecycle;
use crate::ui::dashboard::by_attention::{FlatRow, QuietRepo};
use crate::ui::dashboard::by_repo::RepoView;
use crate::ui::dashboard::hidden::HiddenGroupView;
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::dashboard::sort::{
    BLOCKED_PIN_MAX_AGE_DEFAULT_SECS, SortMode, StatusCounts, default_fold, order_workspaces,
};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListState, Paragraph};
use std::collections::HashMap;

/// Per-workspace inputs the caller has already classified.
#[derive(Debug, Clone)]
pub struct WorkspaceItem<'a> {
    pub repo: &'a Repo,
    pub workspace_id: crate::data::store::WorkspaceId,
    pub status: Status,
    pub row: RowInputs,
    /// What hide rules test; built by `App::hide_subject`.
    pub hide: HideSubject,
    /// The extra detail the repo focus view's tall rows show.
    pub detail: tall::WorkspaceDetail,
}

/// What `app.rs` passes to `render()`. Replaces the old `Item` enum.
#[derive(Debug, Clone)]
pub struct DashboardInputs<'a> {
    pub repos: Vec<&'a Repo>,
    pub workspaces: Vec<WorkspaceItem<'a>>,
    pub activity: &'a [u32],
    pub column_widths: row::ColumnWidths,
    /// Which repos live on github.com — gates the per-repo PR link on the
    /// by-repo headers.
    pub github_remotes: &'a crate::git::github_remotes::GithubRemotes,
    /// The global nerd-fonts setting, for glyphs chosen outside `RowInputs`.
    pub nerd_fonts: bool,
    /// Fleet variables for theme modules, built once per frame.
    pub fleet: &'a crate::ui::bar::segment::SegmentMap,
    /// The side panel's data for the selected workspace; `None` when the
    /// panel is closed or no workspace is selected.
    pub panel: Option<side_panel::PanelData>,
}

#[derive(Debug)]
pub struct DashboardState {
    pub list_state: ListState,
    pub group_mode: GroupMode,
    /// How workspaces are ordered within a repo. Loaded from the
    /// `dashboard_sort_mode` setting at startup and persisted on toggle.
    pub sort_mode: SortMode,
    /// How long a blocked workspace keeps its top-of-list pin, from the
    /// `dashboard_blocked_pin_max_age_secs` setting.
    pub blocked_pin_max_age_secs: u64,
    /// Tags that keep a workspace in the attached view's top bar despite the
    /// repo's tag hide rules, from the `top_bar_show_tags` setting.
    pub top_bar_show_tags: Vec<std::sync::Arc<str>>,
    /// Explicit user fold overrides; absent = use `default_fold(counts)`.
    pub folded: HashMap<u64, bool>,
    /// Hidden groups the user expanded this session. Not persisted.
    pub expanded_hidden: std::collections::HashSet<(
        crate::data::store::RepoId,
        crate::data::hide_rules::HideRule,
    )>,
    pub filter: Option<String>,
    /// The status picked by clicking a `$statuses` tally item in the header,
    /// or `None`. Narrows the list alongside `filter` (both must match);
    /// Esc clears it. Not persisted.
    pub status_filter: Option<Status>,
    pub selection: Option<SelectionTarget>,
    /// Index into `App::selectable`. Owned here so that nav handlers in
    /// `app.rs` can mutate it without touching ratatui internals; the
    /// renderer uses `selection` (resolved `SelectionTarget`) for display.
    pub selected: usize,
    /// In-flight reply text for the detail bar input. Tied to whichever
    /// workspace is selected at the time keystrokes arrived; cleared on
    /// selection change, Enter (send), or Esc (cancel).
    pub reply_draft: String,
    /// Wall-clock deadline (epoch ms) at which `reply_draft` should
    /// auto-clear. Set by chip dispatch so the dispatched command is
    /// briefly echoed into the reply input as visual confirmation, then
    /// wiped. `None` when no auto-clear is pending. Any user
    /// interaction with the draft (typing, Backspace) clears the
    /// deadline so it doesn't wipe their fresh input mid-edit.
    pub reply_draft_clear_at_ms: Option<u64>,
    /// The repo the dashboard is focused on, or `None` for every repo. While
    /// set, the list shows only that repo's workspaces, as tall rows. Not
    /// persisted.
    pub focused_repo: Option<RepoId>,
    /// The focus view's first visible line. The focus view scrolls by lines,
    /// not list items, so it keeps its own offset.
    pub focus_scroll: usize,
    /// The selection the focus view last scrolled to. When the selection
    /// differs, the next frame scrolls it into view; otherwise the offset
    /// is left to the wheel and PgUp/PgDn.
    pub focus_scroll_anchor: Option<SelectionTarget>,
    /// Lines the focus view's rows had last frame, for paging.
    pub focus_page: usize,
    /// The commits / files side panel, shared by the list and the focus
    /// view.
    pub panel: side_panel::SidePanelState,
    /// Whether the focus view sections workspaces by PR state (`G` toggles;
    /// `dashboard_focus_group_by_pr` setting) or keeps the list's order.
    pub focus_group_by_pr: bool,
}

/// Settings key holding the by-repo sort mode.
pub const SORT_MODE_SETTING: &str = "dashboard_sort_mode";
/// Settings key for the focus view's PR-state grouping (on/off).
pub const FOCUS_GROUP_BY_PR_SETTING: &str = "dashboard_focus_group_by_pr";
/// Settings key holding how long a blocked row keeps its pin, in seconds.
pub const BLOCKED_PIN_SETTING: &str = "dashboard_blocked_pin_max_age_secs";
/// Settings key holding the tags exempt from tag hide rules in the top bar.
pub const TOP_BAR_SHOW_TAGS_SETTING: &str = "top_bar_show_tags";

/// Comma- or space-separated tag names, normalized; invalid names are skipped.
pub fn parse_show_tags(raw: &str) -> Vec<std::sync::Arc<str>> {
    let mut tags: Vec<std::sync::Arc<str>> = Vec::new();
    for t in raw
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter_map(crate::data::tags::normalize_tag)
    {
        if !tags.iter().any(|x| **x == *t) {
            tags.push(t.into());
        }
    }
    tags
}

impl DashboardState {
    /// Load the persisted ordering and top-bar preferences. Each falls back to its
    /// default when unset or unparseable — a hand-edited settings row should
    /// not stop the dashboard from drawing.
    pub fn load_ordering_prefs(&mut self, store: &crate::data::store::Store) {
        // Trimmed before parsing, like `dashboard_branch_width`: `config set
        // <key> @file` stores the file verbatim, and a trailing newline would
        // otherwise send a perfectly good value down the fallback path while
        // the CLI reported it saved.
        // Unset resets to the default: the settings drawer re-runs this after
        // clearing a key.
        let get = |key| store.get_setting(key).ok().flatten();
        self.sort_mode = get(SORT_MODE_SETTING)
            .map(|v| SortMode::from_str_or_default(v.trim()))
            .unwrap_or_default();
        self.blocked_pin_max_age_secs = get(BLOCKED_PIN_SETTING)
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(BLOCKED_PIN_MAX_AGE_DEFAULT_SECS);
        self.top_bar_show_tags = get(TOP_BAR_SHOW_TAGS_SETTING)
            .map(|v| parse_show_tags(&v))
            .unwrap_or_default();
        self.focus_group_by_pr =
            crate::config::settings::toggle_on(store, FOCUS_GROUP_BY_PR_SETTING);
    }

    /// Flip the focus view between PR-state sections and the list's order,
    /// and remember it.
    pub fn toggle_focus_grouping(&mut self, store: &crate::data::store::Store) {
        self.focus_group_by_pr = !self.focus_group_by_pr;
        // Rows reorder, so the next frame scrolls the selection into view.
        self.focus_scroll_anchor = None;
        let value = if self.focus_group_by_pr { "on" } else { "off" };
        let _ = store.set_setting(FOCUS_GROUP_BY_PR_SETTING, value);
    }

    /// Flip between repo and attention grouping. Shared by the dashboard's
    /// `G` and the workspace-updates panel's, which mirrors it.
    pub fn toggle_group_mode(&mut self) {
        self.group_mode = match self.group_mode {
            GroupMode::Repo => GroupMode::Attention,
            GroupMode::Attention => GroupMode::Repo,
        };
    }

    /// Filter the list to `status`, or clear the filter when `status` is
    /// already the active one. Clicking another status replaces it.
    pub fn toggle_status_filter(&mut self, status: Status) {
        self.status_filter = (self.status_filter != Some(status)).then_some(status);
    }

    /// Move to the next sort mode and remember it, so the choice survives a
    /// restart the way the theme does.
    pub fn cycle_sort_mode(&mut self, store: &crate::data::store::Store) {
        self.sort_mode = self.sort_mode.cycle();
        let _ = store.set_setting(SORT_MODE_SETTING, self.sort_mode.as_str());
    }

    /// Focus the dashboard on `repo`, scrolled to its top.
    pub fn focus_repo(&mut self, repo: RepoId) {
        self.focused_repo = Some(repo);
        self.focus_scroll = 0;
        self.focus_scroll_anchor = None;
    }

    /// Scroll the focus view by `lines` (negative is up). The next frame
    /// clamps it to the content and keeps part of the selected row showing.
    pub fn scroll_focus(&mut self, lines: isize) {
        self.focus_scroll = self.focus_scroll.saturating_add_signed(lines);
    }

    /// Lines PgUp / PgDn move the focus view: a page, less two lines of
    /// overlap so the reader keeps their place.
    pub fn focus_page_lines(&self) -> isize {
        self.focus_page.saturating_sub(2).max(1) as isize
    }
}

// Hand-written rather than derived: `blocked_pin_max_age_secs` must default to
// the pin window, not to `u64`'s zero, which would silently disable the pin.
impl Default for DashboardState {
    fn default() -> Self {
        Self {
            list_state: ListState::default(),
            group_mode: GroupMode::default(),
            focus_group_by_pr: true,
            sort_mode: SortMode::default(),
            blocked_pin_max_age_secs: BLOCKED_PIN_MAX_AGE_DEFAULT_SECS,
            top_bar_show_tags: Vec::new(),
            folded: HashMap::new(),
            expanded_hidden: Default::default(),
            filter: None,
            status_filter: None,
            selection: None,
            selected: 0,
            reply_draft: String::new(),
            reply_draft_clear_at_ms: None,
            focused_repo: None,
            focus_scroll: 0,
            focus_scroll_anchor: None,
            focus_page: 0,
            panel: Default::default(),
        }
    }
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &DashboardInputs<'_>,
    state: &mut DashboardState,
    tick: u32,
    theme: &Theme,
    specs: &crate::config::theme_file::BarSpecs,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // list + chrome
            Constraint::Length(1), // footer
        ])
        .split(area);
    let _ = render_without_footer(f, chunks[0], inputs, state, tick, theme, specs);
    let _ = render_footer(
        f,
        chunks[1],
        inputs.activity,
        theme,
        specs,
        "24h",
        matches!(state.selection, Some(SelectionTarget::Workspace(_))),
        false,
        None,
        &[],
        inputs.fleet,
    );
}

/// A workspace row's clickable PR chip, positioned by flat list index:
/// `(workspace, flat item index, (char offset in row, char width))`.
type PrChipSpan = (crate::data::store::WorkspaceId, usize, (u16, u16));

/// Everything on the dashboard list a click can land on. Populated during
/// draw and read by the mouse handler, per the `chip_rects` pattern.
#[derive(Debug, Default)]
pub struct ListClickTargets {
    /// Each visible workspace row's PR chip — opens that PR in the browser.
    pub pr_chips: Vec<(crate::data::store::WorkspaceId, Rect)>,
    /// Each visible repo header's PR link — opens that repo's open PRs
    /// filtered to the signed-in user. Empty outside the by-repo view.
    pub repo_pr_links: Vec<(crate::data::store::RepoId, Rect)>,
    /// Each `$statuses` tally item in the header — filters the list to that
    /// status, or clears the filter when it is the active one. Empty when
    /// the theme leaves `$statuses` off the header (the fallback strip row
    /// is not clickable).
    pub statuses: Vec<(Status, Rect)>,
    /// Each visible repo header's name — focuses the dashboard on that repo.
    pub repo_names: Vec<(crate::data::store::RepoId, Rect)>,
    /// Each tall row in the focus view — selects and attaches its workspace.
    pub focus_rows: Vec<(crate::data::store::WorkspaceId, Rect)>,
    /// Each hidden-group summary row in the focus view — expands or
    /// collapses that group.
    pub focus_groups: Vec<((RepoId, HideRule), Rect)>,
    /// The side panel's tabs — a click shows that tab.
    pub panel_tabs: Vec<(side_panel::PanelTab, Rect)>,
    /// The side panel itself, for routing the wheel to it.
    pub panel_area: Option<Rect>,
    /// The side panel's file rows — a click opens that file's diff.
    pub panel_files: Vec<(usize, Rect)>,
    /// The focus view's `◂ all repos` link — leaves the focus view.
    pub unfocus: Option<Rect>,
}

/// Resolve `(key, flat item index, (char offset, width))` spans into screen
/// rects within a just-rendered list: `offset` is the list's scroll
/// position, and every item is one row tall, so a flat index maps straight
/// to a y offset. Spans scrolled out of view or off the right edge drop.
fn spans_to_rects<K: Copy>(
    list_area: Rect,
    offset: usize,
    spans: impl IntoIterator<Item = (K, usize, (u16, u16))>,
) -> Vec<(K, Rect)> {
    let max_x = list_area.x.saturating_add(list_area.width);
    spans
        .into_iter()
        .filter_map(|(key, flat_idx, (dx, w))| {
            let dy = flat_idx.checked_sub(offset)?;
            if dy >= list_area.height as usize {
                return None;
            }
            let x = list_area.x.saturating_add(dx);
            if x >= max_x {
                return None;
            }
            Some((
                key,
                Rect {
                    x,
                    y: list_area.y + dy as u16,
                    width: w.min(max_x - x),
                    height: 1,
                },
            ))
        })
        .collect()
}

/// Render chrome, status strip, and the workspace list into `area` without
/// painting a footer row. The caller is responsible for rendering the footer
/// (usually in a separately-carved row below the detail/PM regions so the
/// spec order list/detail/pm/footer is respected).
///
/// Returns the on-screen rects of everything clickable in the list and the
/// header so the caller can hit-test against them.
pub fn render_without_footer(
    f: &mut Frame,
    area: Rect,
    inputs: &DashboardInputs<'_>,
    state: &mut DashboardState,
    tick: u32,
    theme: &Theme,
    specs: &crate::config::theme_file::BarSpecs,
) -> ListClickTargets {
    // The status tally rides in the header when the theme places
    // `$statuses` there; otherwise it keeps its own row below it.
    let strip = !specs.places(&specs.dashboard_header, "statuses");
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),                // top chrome
            Constraint::Length(u16::from(strip)), // status strip
            Constraint::Length(1),                // spacer
            Constraint::Min(0),                   // main list
        ])
        .split(area);

    let global_counts = StatusCounts::from_iter(inputs.workspaces.iter().map(|w| w.status));

    let header = crate::ui::bar::dashboard_header(
        specs,
        theme,
        &crate::ui::bar::DashboardHeaderInputs {
            group: state.group_mode,
            sort: state.sort_mode,
            repos: inputs.repos.len(),
            workspaces: inputs.workspaces.len(),
            statuses: global_counts,
            status_filter: state.status_filter,
            filter: state.filter.as_deref(),
            view: "dashboard",
            fleet: inputs.fleet,
        },
        chunks[0].width,
    );
    let statuses: Vec<(Status, Rect)> = crate::ui::bar::render::hit_rects(chunks[0], &header.hits)
        .into_iter()
        .filter_map(|(r, hit)| match hit {
            crate::ui::bar::segment::Hit::Status(s) => Some((s, r)),
            _ => None,
        })
        .collect();
    f.render_widget(Paragraph::new(header.line), chunks[0]);
    if strip {
        f.render_widget(
            Paragraph::new(layout::status_strip(global_counts, theme)),
            chunks[1],
        );
    }

    if let Some(repo) = focused_repo(inputs, state) {
        let glyphs = tall::AgentGlyphs::from_specs(specs);
        let mut targets = render_focus(f, chunks[3], inputs, state, repo, &glyphs, tick, theme);
        targets.statuses = statuses;
        return targets;
    }

    // The side panel takes the right of the list when open and there's
    // room, as in the focus view.
    let (list_area, panel) = state
        .panel
        .carve(f, chunks[3], inputs.panel.as_ref(), theme);
    let width = list_area.width as usize;
    let (items, chip_spans, repo_link_spans, name_spans) = match state.group_mode {
        GroupMode::Repo => render_by_repo(inputs, state, tick, width, theme),
        GroupMode::Attention => {
            let (items, chips) = render_by_attention(inputs, state, tick, width, theme);
            (items, chips, Vec::new(), Vec::new())
        }
    };
    // Nothing to show. An empty body reads as "the tool is broken", and the
    // two causes need opposite responses: a filter that hid every row wants
    // the filter cleared, while a fresh install wants a repo registered —
    // which is CLI-only, so the remedy names the command rather than a key.
    // Those are the only two reachable causes: once a repo exists, by-repo
    // always draws its header and by-attention always draws it under QUIET
    // REPOS, so `items` is empty only when there are no repos at all or a
    // filter hid every row.
    if items.is_empty() {
        let msg = if RowFilter::of(state).is_active() {
            "(no matching workspaces)"
        } else {
            "(no repos · run wsx repo add <path>)"
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg.to_string(), theme.dim_style()))),
            list_area,
        );
        return ListClickTargets {
            statuses,
            panel_files: panel.files,
            panel_tabs: panel.tabs,
            panel_area: panel.area,
            ..Default::default()
        };
    }
    let list = List::new(items).highlight_style(theme.selected_bg_style());
    f.render_stateful_widget(list, list_area, &mut state.list_state);

    // Convert flat-index spans into screen rects. The list has just
    // rendered, so `list_state.offset()` reflects this frame's scroll
    // position.
    let offset = state.list_state.offset();
    let repo_spans = |spans: Vec<by_repo::RepoPrLinkSpan>| {
        spans_to_rects(
            list_area,
            offset,
            spans
                .into_iter()
                .map(|(id, idx, span)| (crate::data::store::RepoId(id as i64), idx, span)),
        )
    };
    ListClickTargets {
        pr_chips: spans_to_rects(list_area, offset, chip_spans),
        repo_pr_links: repo_spans(repo_link_spans),
        repo_names: repo_spans(name_spans),
        statuses,
        panel_files: panel.files,
        panel_tabs: panel.tabs,
        panel_area: panel.area,
        ..Default::default()
    }
}

/// The focused repo, when focus is set and the repo still exists. A repo
/// unregistered while focused simply drops the dashboard back to every repo.
fn focused_repo<'a>(inputs: &DashboardInputs<'a>, state: &DashboardState) -> Option<&'a Repo> {
    let id = state.focused_repo?;
    inputs.repos.iter().copied().find(|r| r.id == id)
}

/// The focused repo's workspaces in display order: the same filter, hide rules
/// and sort the by-repo list applies, so a workspace sits in the same place
/// relative to its siblings in both views. Shared by the renderer and
/// `visible_targets` so the rows and keyboard nav cannot drift.
fn focused_workspaces<'w, 'a>(
    inputs: &'w DashboardInputs<'a>,
    state: &DashboardState,
    repo: RepoId,
) -> Vec<&'w WorkspaceItem<'a>> {
    let filter = RowFilter::of(state);
    let mut rows: Vec<NavRow> = inputs
        .workspaces
        .iter()
        .filter(|w| w.repo.id == repo)
        .filter(|w| filter.matches(w))
        .filter(|w| hidden_by(w, filter).is_none())
        .map(NavRow::from)
        .collect();
    // Sections already order by PR state, so pinning applies only ungrouped.
    let pin = if state.focus_group_by_pr {
        0
    } else {
        state.blocked_pin_max_age_secs
    };
    order_workspaces(&mut rows, state.sort_mode, pin);
    let by_id: HashMap<crate::data::store::WorkspaceId, &WorkspaceItem<'a>> = inputs
        .workspaces
        .iter()
        .filter(|w| w.repo.id == repo)
        .map(|w| (w.workspace_id, w))
        .collect();
    rows.iter()
        .filter_map(|r| by_id.get(&r.workspace_id).copied())
        .collect()
}

/// The PR-state sections of the focus view, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FocusSection {
    NoPr,
    Draft,
    Open,
    Done,
}

impl FocusSection {
    pub(crate) fn label(self) -> &'static str {
        match self {
            FocusSection::NoPr => "No PR",
            FocusSection::Draft => "Draft",
            FocusSection::Open => "Open",
            FocusSection::Done => "Done",
        }
    }
}

/// The section a workspace sits in; an unpolled branch counts as no PR.
pub(crate) fn section_of(lc: Option<BranchLifecycle>) -> FocusSection {
    match lc {
        None | Some(BranchLifecycle::NoPr) => FocusSection::NoPr,
        Some(BranchLifecycle::PrDraft) => FocusSection::Draft,
        Some(BranchLifecycle::PrOpen | BranchLifecycle::PrConflicted) => FocusSection::Open,
        Some(BranchLifecycle::PrMerged | BranchLifecycle::PrClosed) => FocusSection::Done,
    }
}

/// The color a section's header is drawn in; every section gets one so the
/// headers stand out from the rows' dim sub-lines.
fn section_color(section: FocusSection, theme: &Theme) -> Color {
    match section {
        FocusSection::NoPr => theme.header_fg,
        FocusSection::Draft => theme.waiting,
        FocusSection::Open => theme.ok,
        FocusSection::Done => theme.merged,
    }
}

/// `━━ Open · 2 ━━━…` to `width`, all in the section's color, label bold.
fn section_header_line(
    section: FocusSection,
    count: usize,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let color = Style::default().fg(section_color(section, theme));
    let label = format!("{} · {} ", section.label(), count);
    let rule = width.saturating_sub(3 + label.chars().count());
    Line::from(vec![
        Span::styled("━━ ", color),
        Span::styled(label, color.add_modifier(Modifier::BOLD)),
        Span::styled("━".repeat(rule), color),
    ])
}

/// One entry in the focus view, in display order.
enum FocusEntry<'w, 'a> {
    /// A PR-state header; never a selection or click target.
    Section {
        section: FocusSection,
        count: usize,
    },
    Workspace(&'w WorkspaceItem<'a>),
    /// A hide rule's summary row, with its rows ordered for display.
    Group(HiddenGroupView),
}

/// The focus view's entries: the repo's shown workspaces, then each hidden
/// group's summary row followed by its workspaces when expanded — the same
/// sequence the by-repo list draws under the repo's header. Shared by the
/// renderer and `visible_targets` so the drawn order and nav order agree.
fn focus_entries<'w, 'a>(
    inputs: &'w DashboardInputs<'a>,
    state: &DashboardState,
    repo: &Repo,
) -> Vec<FocusEntry<'w, 'a>> {
    let shown = focused_workspaces(inputs, state, repo.id);
    let mut out: Vec<FocusEntry<'w, 'a>> = Vec::new();
    let mut buckets: [Vec<&'w WorkspaceItem<'a>>; 4] = Default::default();
    if state.focus_group_by_pr {
        for w in shown {
            buckets[section_of(w.row.lifecycle) as usize].push(w);
        }
    } else {
        out.extend(shown.into_iter().map(FocusEntry::Workspace));
    }
    for (section, rows) in [
        FocusSection::NoPr,
        FocusSection::Draft,
        FocusSection::Open,
        FocusSection::Done,
    ]
    .into_iter()
    .zip(buckets)
    {
        if rows.is_empty() {
            continue;
        }
        out.push(FocusEntry::Section {
            section,
            count: rows.len(),
        });
        out.extend(rows.into_iter().map(FocusEntry::Workspace));
    }
    let by_id: HashMap<crate::data::store::WorkspaceId, &'w WorkspaceItem<'a>> = inputs
        .workspaces
        .iter()
        .filter(|w| w.repo.id == repo.id)
        .map(|w| (w.workspace_id, w))
        .collect();
    for (rule, rows) in hidden_groups(repo, &inputs.workspaces, RowFilter::of(state)) {
        let view = hidden_group_view(repo.id, rule, &rows, state);
        let members: Vec<&'w WorkspaceItem<'a>> = if view.expanded {
            view.workspaces
                .iter()
                .filter_map(|r| by_id.get(&r.workspace_id).copied())
                .collect()
        } else {
            Vec::new()
        };
        out.push(FocusEntry::Group(view));
        out.extend(members.into_iter().map(FocusEntry::Workspace));
    }
    out
}

impl FocusEntry<'_, '_> {
    fn target(&self, repo: RepoId) -> Option<SelectionTarget> {
        match self {
            FocusEntry::Section { .. } => None,
            FocusEntry::Workspace(w) => Some(SelectionTarget::Workspace(w.workspace_id)),
            FocusEntry::Group(g) => Some(SelectionTarget::HiddenGroup {
                repo,
                rule: g.rule.clone(),
            }),
        }
    }
}

/// Draw the repo focus view into `area`: the breadcrumb, a spacer, then the
/// repo's workspaces as tall rows and its hidden groups as summary rows (or
/// an empty-state line).
#[allow(clippy::too_many_arguments)]
fn render_focus(
    f: &mut Frame,
    area: Rect,
    inputs: &DashboardInputs<'_>,
    state: &mut DashboardState,
    repo: &Repo,
    glyphs: &tall::AgentGlyphs,
    tick: u32,
    theme: &Theme,
) -> ListClickTargets {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // breadcrumb
            Constraint::Length(1), // spacer
            Constraint::Min(0),    // rows
        ])
        .split(area);
    let entries = focus_entries(inputs, state, repo);
    // The header counts what shows normally, as the list's repo header does.
    let counts = StatusCounts::from_iter(
        focused_workspaces(inputs, state, repo.id)
            .iter()
            .map(|w| w.status),
    );
    let (crumb, (dx, w)) = by_repo::breadcrumb_line(
        &repo.name,
        &repo.path.to_string_lossy(),
        counts,
        chunks[0].width as usize,
        theme,
    );
    f.render_widget(Paragraph::new(crumb), chunks[0]);
    let unfocus = Some(Rect {
        x: chunks[0].x + dx,
        y: chunks[0].y,
        width: w.min(chunks[0].width.saturating_sub(dx)),
        height: 1,
    });

    // The side panel takes the right of the body when open and there's
    // room; the rows get the rest.
    let (rows_area, panel_targets) = state
        .panel
        .carve(f, chunks[2], inputs.panel.as_ref(), theme);

    if entries.is_empty() {
        let msg = if RowFilter::of(state).is_active() {
            "(no matching workspaces)"
        } else {
            "(no workspaces · n to create one)"
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg.to_string(), theme.dim_style()))),
            rows_area,
        );
        return ListClickTargets {
            unfocus,
            panel_files: panel_targets.files,
            panel_tabs: panel_targets.tabs,
            panel_area: panel_targets.area,
            ..Default::default()
        };
    }
    let selection = state.selection.clone();
    let selected = entries.iter().position(|e| {
        e.target(repo.id)
            .is_some_and(|t| Some(&t) == selection.as_ref())
    });
    // Mark the selected row the way the list does, so the row composer
    // paints the thicker gutter.
    let rows: Vec<Option<RowInputs>> = entries
        .iter()
        .map(|e| match e {
            FocusEntry::Workspace(w) => {
                let mut row = w.row.clone();
                row.selected = selection == Some(SelectionTarget::Workspace(w.workspace_id));
                Some(row)
            }
            FocusEntry::Group(_) | FocusEntry::Section { .. } => None,
        })
        .collect();
    let widths = inputs
        .column_widths
        .with_agent(derived_agent_width(rows.iter().flatten()));
    let tall_entries: Vec<tall::TallEntry<'_>> = entries
        .iter()
        .zip(&rows)
        .map(|(e, row)| match (e, row) {
            (FocusEntry::Workspace(w), Some(row)) => tall::TallEntry::Row(tall::TallRow {
                row,
                detail: &w.detail,
            }),
            (FocusEntry::Section { section, count }, _) => tall::TallEntry::Section(
                section_header_line(*section, *count, rows_area.width as usize, theme),
            ),
            (FocusEntry::Group(g), _) => {
                tall::TallEntry::Group(hidden::hidden_group_line(g, None, inputs.nerd_fonts, theme))
            }
            (FocusEntry::Workspace(_), None) => unreachable!("rows mirror entries"),
        })
        .collect();
    let follow = state.focus_scroll_anchor != selection;
    state.focus_scroll_anchor = selection.clone();
    state.focus_page = rows_area.height as usize;
    let targets = tall::render(
        f,
        rows_area,
        &tall_entries,
        glyphs,
        selected,
        &mut state.focus_scroll,
        follow,
        widths,
        tick,
        theme,
    );
    let focus_groups = targets
        .groups
        .into_iter()
        .filter_map(|(i, rect)| match &entries[i] {
            FocusEntry::Group(g) => Some(((repo.id, g.rule.clone()), rect)),
            FocusEntry::Workspace(_) | FocusEntry::Section { .. } => None,
        })
        .collect();
    ListClickTargets {
        pr_chips: targets.pr_chips,
        focus_rows: targets.rows,
        focus_groups,
        unfocus,
        panel_files: panel_targets.files,
        panel_tabs: panel_targets.tabs,
        panel_area: panel_targets.area,
        ..Default::default()
    }
}

/// The dashboard footer's click targets, in screen coordinates.
#[derive(Debug, Default)]
pub struct FooterRects {
    /// The usage graph, when the `usage` segment is present.
    pub graph: Option<Rect>,
    /// Each clickable key hint.
    pub hints: Vec<(Rect, crate::ui::footer::FooterHintAction)>,
    /// Each `$hosts` chip, by index into the `hosts` it was drawn from.
    pub hosts: Vec<(usize, Rect)>,
}

/// Render only the footer line into `area` (exactly 1 row tall) through the
/// bar engine, returning its click targets.
#[allow(clippy::too_many_arguments)]
pub fn render_footer(
    f: &mut Frame,
    area: Rect,
    activity: &[u32],
    theme: &Theme,
    specs: &crate::config::theme_file::BarSpecs,
    window_label: &str,
    workspace_selected: bool,
    setup_log_available: bool,
    notice: Option<&str>,
    hosts: &[crate::commands::shared_hosts::SharedHost],
    fleet: &crate::ui::bar::segment::SegmentMap,
) -> FooterRects {
    use crate::ui::bar::segment::Hit;
    if let Some(msg) = notice {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(msg.to_string(), theme.err_style()))),
            area,
        );
        return FooterRects::default();
    }
    let rendered = crate::ui::bar::dashboard_footer(
        specs,
        theme,
        &crate::ui::bar::DashboardFooterInputs {
            activity,
            version: env!("CARGO_PKG_VERSION"),
            window_label,
            workspace_selected,
            setup_log_available,
            hosts,
            fleet,
        },
        area.width,
    );
    f.render_widget(Paragraph::new(rendered.line), area);
    let mut out = FooterRects::default();
    for (rect, hit) in crate::ui::bar::render::hit_rects(area, &rendered.hits) {
        match hit {
            Hit::UsageGraph => out.graph = Some(rect),
            Hit::RemoteHost(i) => out.hosts.push((i, rect)),
            _ => {
                if let Some(action) = hit.footer_action() {
                    out.hints.push((rect, action));
                }
            }
        }
    }
    out
}

/// Return the sequence of selectable targets in *visible order*, matching
/// what `render()` produces. The caller (`app.rs::draw`) writes this
/// into `App::selectable` so arrow-key navigation walks the same order
/// the user sees on screen instead of the raw `app.workspaces` order
/// (which the V5 renderer reshuffles by sort_order / status priority /
/// fold state / filter).
///
/// By-repo: emits `Repo(id)` for each visible header followed by
/// `Workspace(id)` for each visible workspace inside expanded repos.
///
/// By-attention: emits `Workspace(id)` for each row across the four
/// active sections (NEEDS ATTENTION / WORKING / RECENT / IDLE) in the
/// order `partition` produces. QUIET REPOS entries are skipped — they
/// have no per-repo selection model in v1.
/// One workspace as the nav-index builder sees it: the fields the shared
/// comparator reads, plus the id the row resolves to. Exists so nav ordering
/// runs through the same `order_workspaces` call the renderer uses instead of
/// a hand-copied sort that could drift from it.
struct NavRow {
    status: Status,
    ago_secs: Option<u64>,
    name: String,
    workspace_id: crate::data::store::WorkspaceId,
}

impl From<&WorkspaceItem<'_>> for NavRow {
    fn from(w: &WorkspaceItem<'_>) -> Self {
        NavRow {
            status: w.status,
            ago_secs: w.row.ago_secs,
            name: w.row.branch.clone(),
            workspace_id: w.workspace_id,
        }
    }
}

impl sort::SortRow for NavRow {
    fn sort_status(&self) -> Status {
        self.status
    }
    fn sort_ago_secs(&self) -> Option<u64> {
        self.ago_secs
    }
    fn sort_name(&self) -> &str {
        &self.name
    }
}

/// Where each focus-view section starts in `visible_targets`, in display
/// order; the hidden-groups tail counts as a section. Empty when unfocused.
pub fn focus_section_starts(inputs: &DashboardInputs<'_>, state: &DashboardState) -> Vec<usize> {
    let Some(repo) = focused_repo(inputs, state) else {
        return Vec::new();
    };
    let entries = focus_entries(inputs, state, repo);
    let mut starts = Vec::new();
    // Ungrouped, the workspaces before any hidden group form one section.
    if matches!(entries.first(), Some(FocusEntry::Workspace(_))) && !state.focus_group_by_pr {
        starts.push(0);
    }
    let mut seen = 0;
    let mut in_tail = false;
    for e in entries {
        match e {
            FocusEntry::Section { .. } => starts.push(seen),
            FocusEntry::Group(_) if !in_tail => {
                in_tail = true;
                starts.push(seen);
                seen += 1;
            }
            _ => seen += 1,
        }
    }
    starts
}

pub fn visible_targets(
    inputs: &DashboardInputs<'_>,
    state: &DashboardState,
) -> Vec<SelectionTarget> {
    if let Some(repo) = focused_repo(inputs, state) {
        return focus_entries(inputs, state, repo)
            .iter()
            .filter_map(|e| e.target(repo.id))
            .collect();
    }
    let filter = RowFilter::of(state);
    let shown: Vec<WorkspaceItem<'_>> = inputs
        .workspaces
        .iter()
        .filter(|w| filter.matches(w))
        .filter(|w| hidden_by(w, filter).is_none())
        .cloned()
        .collect();
    let mut out: Vec<SelectionTarget> = Vec::new();
    match state.group_mode {
        GroupMode::Repo => {
            let sections = ordered_sections(
                &inputs.repos,
                &shown,
                GroupMode::Repo,
                state.sort_mode,
                state.blocked_pin_max_age_secs,
            );
            for s in &sections {
                let SectionKind::Repo(repo_id) = s.kind else {
                    continue;
                };
                out.push(SelectionTarget::Repo(repo_id));
                let groups = inputs
                    .repos
                    .iter()
                    .find(|r| r.id == repo_id)
                    .map(|r| hidden_groups(r, &inputs.workspaces, filter))
                    .unwrap_or_default();
                let expanded = match state.folded.get(&(repo_id.0 as u64)).copied() {
                    Some(explicit) => !explicit,
                    None => default_expanded(s.counts, !groups.is_empty()),
                };
                if expanded {
                    out.extend(
                        s.workspace_ids
                            .iter()
                            .map(|w| SelectionTarget::Workspace(*w)),
                    );
                    for (rule, rows) in &groups {
                        push_group_targets(&mut out, repo_id, rule, rows, state);
                    }
                }
            }
        }
        GroupMode::Attention => {
            // Mirror render_by_attention: drop idle rows that appear under
            // QUIET REPOS (they have no per-repo selection model), then
            // order what's left.
            let quiet_names = quiet_repo_names(inputs, filter);
            let shown: Vec<WorkspaceItem<'_>> = shown
                .into_iter()
                .filter(|w| {
                    !matches!(w.status, Status::Idle) || !quiet_names.contains(&w.repo.name)
                })
                .collect();
            for s in ordered_sections(
                &inputs.repos,
                &shown,
                GroupMode::Attention,
                state.sort_mode,
                state.blocked_pin_max_age_secs,
            ) {
                out.extend(
                    s.workspace_ids
                        .iter()
                        .map(|w| SelectionTarget::Workspace(*w)),
                );
            }
            for (r, rule, rows) in all_hidden_groups(inputs, filter) {
                push_group_targets(&mut out, r.id, &rule, &rows, state);
            }
        }
    }
    out
}

/// The repos `render_by_attention` lists under QUIET REPOS for `filter`:
/// every repo with no (matching) workspaces — unless a filter is active and
/// misses the repo's name — plus every repo whose matching workspaces are
/// all idle, unless a status filter is active.
fn quiet_repo_names(
    inputs: &DashboardInputs<'_>,
    filter: RowFilter<'_>,
) -> std::collections::HashSet<String> {
    let mut quiet_names: std::collections::HashSet<String> = Default::default();
    for r in &inputs.repos {
        let repo_rows: Vec<&WorkspaceItem<'_>> = inputs
            .workspaces
            .iter()
            .filter(|w| w.repo.id == r.id)
            .filter(|w| filter.matches(w))
            .filter(|w| hidden_by(w, filter).is_none())
            .collect();
        let count = repo_rows.len();
        let all_idle = filter.status.is_none()
            && !repo_rows.is_empty()
            && repo_rows.iter().all(|w| matches!(w.status, Status::Idle));
        let has_hidden = !hidden_groups(r, &inputs.workspaces, filter).is_empty();
        let include_empty = count == 0 && !has_hidden && filter.keeps_empty_repo(&r.name);
        if include_empty || all_idle {
            quiet_names.insert(r.name.clone());
        }
    }
    quiet_names
}

/// One of the by-attention view's four row sections, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttentionSection {
    NeedsAttention,
    Working,
    Recent,
    Idle,
}

impl AttentionSection {
    /// The section's on-screen label, as the by-attention header draws it.
    pub fn label(self) -> &'static str {
        match self {
            AttentionSection::NeedsAttention => "◆ NEEDS ATTENTION",
            AttentionSection::Working => "● WORKING",
            AttentionSection::Recent => "✓ RECENT",
            AttentionSection::Idle => "  IDLE",
        }
    }
}

/// What a section of the ordered list is headed by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionKind {
    /// By-repo grouping: one section per repo, empty repos included.
    Repo(crate::data::store::RepoId),
    /// By-attention grouping: only non-empty sections are emitted.
    Attention(AttentionSection),
}

/// A run of workspace rows under one header, in display order.
#[derive(Debug, Clone)]
pub struct OrderedSection {
    pub kind: SectionKind,
    /// Status tally of the rows in this section — what `default_fold`
    /// reads for a repo section.
    pub counts: StatusCounts,
    pub workspace_ids: Vec<crate::data::store::WorkspaceId>,
}

/// The dashboard's workspace ordering, as sections of rows.
///
/// By-repo: one section per repo ascending by `(sort_order, id)` (the same
/// key `by_repo::order_repos` uses), rows inside each ordered by
/// [`order_workspaces`] under `sort_mode`. By-attention: the four urgency
/// sections in display order, rows inside each ordered by
/// [`by_attention::partition`]; empty sections are omitted.
///
/// Pure with respect to visibility: it neither filters nor folds nor
/// collapses quiet repos. `visible_targets` layers the dashboard's rules on
/// top; the workspace-updates panel layers its own. Both therefore walk one
/// ordering and cannot drift from each other or from the renderer.
pub fn ordered_sections(
    repos: &[&Repo],
    workspaces: &[WorkspaceItem<'_>],
    group_mode: GroupMode,
    sort_mode: SortMode,
    blocked_pin_max_age_secs: u64,
) -> Vec<OrderedSection> {
    match group_mode {
        GroupMode::Repo => {
            let mut repos: Vec<&Repo> = repos.to_vec();
            repos.sort_by_key(|r| (r.sort_order, r.id.0));
            repos
                .into_iter()
                .map(|r| {
                    let mut rows: Vec<NavRow> = workspaces
                        .iter()
                        .filter(|w| w.repo.id == r.id)
                        .map(NavRow::from)
                        .collect();
                    order_workspaces(&mut rows, sort_mode, blocked_pin_max_age_secs);
                    OrderedSection {
                        kind: SectionKind::Repo(r.id),
                        counts: StatusCounts::from_iter(rows.iter().map(|r| r.status)),
                        workspace_ids: rows.into_iter().map(|r| r.workspace_id).collect(),
                    }
                })
                .collect()
        }
        GroupMode::Attention => {
            let rows: Vec<FlatRow> = workspaces
                .iter()
                .map(|w| FlatRow {
                    repo_name: w.repo.name.clone(),
                    row: w.row.clone(),
                })
                .collect();
            let data = by_attention::partition(rows, Vec::new());
            [
                (AttentionSection::NeedsAttention, &data.needs_attention),
                (AttentionSection::Working, &data.working),
                (AttentionSection::Recent, &data.recent),
                (AttentionSection::Idle, &data.idle),
            ]
            .into_iter()
            .filter(|(_, rows)| !rows.is_empty())
            .map(|(section, rows)| OrderedSection {
                kind: SectionKind::Attention(section),
                counts: StatusCounts::from_iter(rows.iter().map(|r| r.row.status)),
                workspace_ids: rows.iter().map(|r| r.row.workspace_id).collect(),
            })
            .collect()
        }
    }
}

/// Resolve the durable selection against a freshly-rebuilt `selectable` list.
/// Returns the `(selection, selected-index)` the dashboard should store.
///
/// - **Visible:** the target is still in `new_selectable` → follow it to its
///   current index (this is how selection survives reorders and restores after
///   a fold re-expands).
/// - **Hidden but existing workspace:** a `Workspace` target left
///   `new_selectable` (its repo auto-folded, a filter hid it, or it dropped to
///   QUIET REPOS) yet still exists per `target_exists` → *park*: keep the same
///   target, clamp the nav cursor for safety, and do NOT reassign identity to a
///   neighbor. The renderer simply draws no highlight until the row returns.
///   Parking is restricted to workspaces: a `Repo` header is always present in
///   the by-repo view, so a repo target absent from `new_selectable` means the
///   view no longer shows repo headers (`GroupMode::Attention`) or the repo was
///   removed — either way it falls back to a visible neighbor rather than
///   parking on an invisible repo (which would leave no highlight and let
///   repo-scoped actions fire in attention view).
/// - **Gone / no prior selection:** the target was archived (`target_exists`
///   false), is a non-visible repo, or there was no selection → fall back to
///   whatever sits at the clamped index (`None` when the list is empty).
pub fn reconcile_selection(
    old_selection: Option<SelectionTarget>,
    old_selected: usize,
    new_selectable: &[SelectionTarget],
    target_exists: impl Fn(SelectionTarget) -> bool,
) -> (Option<SelectionTarget>, usize) {
    if let Some(t) = old_selection {
        if let Some(idx) = new_selectable.iter().position(|s| *s == t) {
            return (Some(t), idx);
        }
        if matches!(t, SelectionTarget::Workspace(_)) && target_exists(t.clone()) {
            let idx = old_selected.min(new_selectable.len().saturating_sub(1));
            return (Some(t), idx);
        }
    }
    if new_selectable.is_empty() {
        (None, 0)
    } else {
        let idx = old_selected.min(new_selectable.len() - 1);
        (new_selectable.get(idx).cloned(), idx)
    }
}

/// The rule hiding `w`, or `None` when it shows normally. An active filter
/// bypasses every rule so a search can find any row.
pub(crate) fn hidden_by(w: &WorkspaceItem<'_>, filter: RowFilter<'_>) -> Option<HideRule> {
    if filter.is_active() {
        return None;
    }
    w.repo.hide_rules.classify(&w.hide)
}

/// A hidden group's summary view: its rows in display order, plus whether
/// the group is expanded.
fn hidden_group_view(
    repo: RepoId,
    rule: HideRule,
    rows: &[&WorkspaceItem<'_>],
    state: &DashboardState,
) -> HiddenGroupView {
    let mut workspaces: Vec<RowInputs> = rows.iter().map(|w| w.row.clone()).collect();
    order_workspaces(
        &mut workspaces,
        state.sort_mode,
        state.blocked_pin_max_age_secs,
    );
    HiddenGroupView {
        expanded: state.expanded_hidden.contains(&(repo, rule.clone())),
        rule,
        workspaces,
    }
}

/// Push a group's row target, then its ordered workspaces when expanded.
fn push_group_targets(
    out: &mut Vec<SelectionTarget>,
    repo: RepoId,
    rule: &HideRule,
    rows: &[&WorkspaceItem<'_>],
    state: &DashboardState,
) {
    out.push(SelectionTarget::HiddenGroup {
        repo,
        rule: rule.clone(),
    });
    if state.expanded_hidden.contains(&(repo, rule.clone())) {
        let mut nav: Vec<NavRow> = rows.iter().map(|w| NavRow::from(*w)).collect();
        order_workspaces(&mut nav, state.sort_mode, state.blocked_pin_max_age_secs);
        out.extend(
            nav.into_iter()
                .map(|n| SelectionTarget::Workspace(n.workspace_id)),
        );
    }
}

/// `repo`'s hidden rows grouped by rule, in the repo's rule order. Empty
/// groups are omitted.
pub(crate) fn hidden_groups<'w, 'a>(
    repo: &Repo,
    workspaces: &'w [WorkspaceItem<'a>],
    filter: RowFilter<'_>,
) -> Vec<(HideRule, Vec<&'w WorkspaceItem<'a>>)> {
    repo.hide_rules
        .iter()
        .filter_map(|rule| {
            let rows: Vec<_> = workspaces
                .iter()
                .filter(|w| w.repo.id == repo.id && hidden_by(w, filter).as_ref() == Some(rule))
                .collect();
            (!rows.is_empty()).then(|| (rule.clone(), rows))
        })
        .collect()
}

/// Every repo's hidden groups in display order: repos by `(sort_order, id)`, then rule.
fn all_hidden_groups<'w, 'a>(
    inputs: &'w DashboardInputs<'a>,
    filter: RowFilter<'_>,
) -> Vec<(&'w Repo, HideRule, Vec<&'w WorkspaceItem<'a>>)> {
    let mut repos: Vec<&Repo> = inputs.repos.to_vec();
    repos.sort_by_key(|r| (r.sort_order, r.id.0));
    repos
        .into_iter()
        .flat_map(|r| {
            hidden_groups(r, &inputs.workspaces, filter)
                .into_iter()
                .map(move |(rule, rows)| (r, rule, rows))
        })
        .collect()
}

/// Default fold: a repo with no rows at all starts folded; one whose rows
/// are all hidden stays open so its summary rows show.
fn default_expanded(counts: StatusCounts, has_hidden: bool) -> bool {
    !default_fold(counts) || has_hidden
}

/// What narrows the list: the typed `/` needle and the clicked status.
/// Both apply when both are set. An empty needle filters nothing.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RowFilter<'f> {
    text: Option<&'f str>,
    status: Option<Status>,
}

impl<'f> RowFilter<'f> {
    fn of(state: &'f DashboardState) -> Self {
        RowFilter {
            text: state.filter.as_deref().filter(|f| !f.is_empty()),
            status: state.status_filter,
        }
    }

    fn is_active(self) -> bool {
        self.text.is_some() || self.status.is_some()
    }

    fn matches(self, w: &WorkspaceItem<'_>) -> bool {
        self.status.is_none_or(|s| w.status == s) && self.text.is_none_or(|f| matches_filter(w, f))
    }

    /// Whether a repo with no matching rows still lists under QUIET REPOS:
    /// always with no filter, and when only a needle naming the repo is set.
    fn keeps_empty_repo(self, repo_name: &str) -> bool {
        match (self.text, self.status) {
            (None, None) => true,
            (Some(f), None) => repo_name.to_lowercase().contains(&f.to_lowercase()),
            (_, Some(_)) => false,
        }
    }
}

/// Case-insensitive substring match against the workspace branch, owning
/// repo name, tags, and the row's status-adaptive column (status token, recap
/// segments, or fallback text — whichever the column carries). A leading `#`
/// matches one tag exactly and nothing else.
fn matches_filter(w: &WorkspaceItem<'_>, filter: &str) -> bool {
    let needle = filter.to_lowercase();
    if let Some(tag) = needle.strip_prefix('#') {
        return w.row.tags.iter().any(|t| **t == *tag);
    }
    if w.row.branch.to_lowercase().contains(&needle)
        || w.repo.name.to_lowercase().contains(&needle)
        || w.row.tags.iter().any(|t| t.contains(&needle))
    {
        return true;
    }
    let Some(col) = w.row.column.as_ref() else {
        return false;
    };
    if col.token.to_lowercase().contains(&needle) {
        return true;
    }
    match &col.body {
        column_content::ColumnBody::Recap { segments, .. } => segments.iter().any(|s| {
            s.text.to_lowercase().contains(&needle)
                || s.full
                    .as_ref()
                    .is_some_and(|f| f.to_lowercase().contains(&needle))
        }),
        column_content::ColumnBody::Fallback { text, .. } => text.to_lowercase().contains(&needle),
        column_content::ColumnBody::Empty => false,
    }
}

/// Cells the agent strip needs for a set of rows: the widest live-agent
/// count among them, capped. Derived per frame from the rows actually
/// being drawn — a peer inside a folded repo or filtered out by the search
/// box must not tax the recap column of the rows that ARE drawn.
fn derived_agent_width<'a>(rows: impl Iterator<Item = &'a RowInputs>) -> usize {
    rows.map(|r| r.peers.len() + 1)
        .max()
        .unwrap_or(1)
        .clamp(1, row::MAX_AGENT_WIDTH)
}

fn render_by_repo<'a>(
    inputs: &DashboardInputs<'a>,
    state: &mut DashboardState,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> (
    Vec<ratatui::widgets::ListItem<'static>>,
    Vec<PrChipSpan>,
    Vec<by_repo::RepoPrLinkSpan>,
    Vec<by_repo::RepoPrLinkSpan>,
) {
    let filter = RowFilter::of(state);
    let mut views: Vec<RepoView<'a>> = inputs
        .repos
        .iter()
        .map(|r| {
            let mut workspaces: Vec<RowInputs> = inputs
                .workspaces
                .iter()
                .filter(|w| w.repo.id == r.id)
                .filter(|w| filter.matches(w))
                .filter(|w| hidden_by(w, filter).is_none())
                .map(|w| w.row.clone())
                .collect();
            order_workspaces(
                &mut workspaces,
                state.sort_mode,
                state.blocked_pin_max_age_secs,
            );
            let hidden: Vec<HiddenGroupView> = hidden_groups(r, &inputs.workspaces, filter)
                .into_iter()
                .map(|(rule, rows)| hidden_group_view(r.id, rule, &rows, state))
                .collect();
            let counts = StatusCounts::from_iter(workspaces.iter().map(|w| w.status));
            let repo_id_u64 = r.id.0 as u64;
            let expanded = match state.folded.get(&repo_id_u64).copied() {
                Some(explicit) => !explicit,
                None => default_expanded(counts, !hidden.is_empty()),
            };
            RepoView {
                id: repo_id_u64,
                name: &r.name,
                path: r.path.to_string_lossy().into_owned(),
                counts,
                expanded,
                sort_order: r.sort_order,
                workspaces,
                hidden,
                show_pr_link: inputs.github_remotes.is_github(r.id),
                nerd_fonts: inputs.nerd_fonts,
            }
        })
        .collect();
    by_repo::order_repos(&mut views);

    // Only expanded repos render rows, so only they may widen the strip.
    let widths = inputs.column_widths.with_agent(derived_agent_width(
        views.iter().filter(|v| v.expanded).flat_map(|v| {
            v.workspaces.iter().chain(
                v.hidden
                    .iter()
                    .filter(|g| g.expanded)
                    .flat_map(|g| g.workspaces.iter()),
            )
        }),
    ));

    // Walk the same item sequence that render_list will emit to determine
    // which flat list index corresponds to the current selection. Also
    // flip `selected: true` on the matching workspace so the row composer
    // can paint a thicker gutter glyph for the selected row, and collect
    // each visible row's PR chip span for click hit-testing.
    let mut selected_idx: Option<usize> = None;
    let mut chip_spans: Vec<PrChipSpan> = Vec::new();
    let mut flat_idx: usize = 0;
    let selection = state.selection.clone();
    for view in &mut views {
        // Header item
        if let Some(SelectionTarget::Repo(rid)) = selection {
            if view.id == rid.0 as u64 {
                selected_idx = Some(flat_idx);
            }
        }
        flat_idx += 1;
        if !view.expanded {
            continue;
        }
        for w in &mut view.workspaces {
            if let Some(SelectionTarget::Workspace(wid)) = selection {
                if w.workspace_id == wid {
                    selected_idx = Some(flat_idx);
                    w.selected = true;
                }
            }
            if let Some(span) = row::pr_chip_hit_span(w, widths) {
                chip_spans.push((w.workspace_id, flat_idx, span));
            }
            flat_idx += 1;
        }
        for g in &mut view.hidden {
            if selection
                == Some(SelectionTarget::HiddenGroup {
                    repo: RepoId(view.id as i64),
                    rule: g.rule.clone(),
                })
            {
                selected_idx = Some(flat_idx);
            }
            flat_idx += 1;
            if !g.expanded {
                continue;
            }
            for w in &mut g.workspaces {
                if selection == Some(SelectionTarget::Workspace(w.workspace_id)) {
                    selected_idx = Some(flat_idx);
                    w.selected = true;
                }
                if let Some(span) = row::pr_chip_hit_span(w, widths) {
                    chip_spans.push((w.workspace_id, flat_idx, span));
                }
                flat_idx += 1;
            }
        }
        // Spacer item
        flat_idx += 1;
    }
    state.list_state.select(selected_idx);

    let list = by_repo::render_list(&views, widths, tick, width, theme);
    (list.items, chip_spans, list.pr_links, list.names)
}

fn render_by_attention<'a>(
    inputs: &DashboardInputs<'a>,
    state: &mut DashboardState,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> (Vec<ratatui::widgets::ListItem<'static>>, Vec<PrChipSpan>) {
    let filter = RowFilter::of(state);
    let rows: Vec<FlatRow> = inputs
        .workspaces
        .iter()
        .filter(|w| filter.matches(w))
        .filter(|w| hidden_by(w, filter).is_none())
        .map(|w| FlatRow {
            repo_name: w.repo.name.clone(),
            row: w.row.clone(),
        })
        .collect();
    let mut quiet: Vec<QuietRepo> = Vec::new();
    for r in &inputs.repos {
        let repo_rows: Vec<&WorkspaceItem<'_>> = inputs
            .workspaces
            .iter()
            .filter(|w| w.repo.id == r.id)
            .filter(|w| filter.matches(w))
            .filter(|w| hidden_by(w, filter).is_none())
            .collect();
        let count = repo_rows.len();
        let all_idle = filter.status.is_none()
            && !repo_rows.is_empty()
            && repo_rows.iter().all(|w| matches!(w.status, Status::Idle));
        // Empty repos only show in QUIET REPOS when no filter is active
        // OR when the text filter matches the repo name itself. A status
        // filter keeps its matching idle rows as rows, not a quiet repo.
        let has_hidden = !hidden_groups(r, &inputs.workspaces, filter).is_empty();
        let include_empty = count == 0 && !has_hidden && filter.keeps_empty_repo(&r.name);
        if include_empty || all_idle {
            quiet.push(QuietRepo {
                name: r.name.clone(),
                path: r.path.to_string_lossy().into_owned(),
                workspace_count: count,
                all_idle,
            });
        }
    }
    // Drop idle rows that already appear under QUIET REPOS, so they
    // don't double-render across IDLE and QUIET REPOS sections.
    let quiet_repo_names: std::collections::HashSet<&str> =
        quiet.iter().map(|q| q.name.as_str()).collect();
    let rows: Vec<FlatRow> = rows
        .into_iter()
        .filter(|r| {
            !matches!(r.row.status, Status::Idle)
                || !quiet_repo_names.contains(r.repo_name.as_str())
        })
        .collect();
    // `partition` distributes rows into sections AND applies the
    // per-section ordering rules (priority-then-recency for NEEDS,
    // recency-only for WORKING / RECENT / IDLE).
    let mut data = by_attention::partition(rows, quiet);
    data.hidden = all_hidden_groups(inputs, filter)
        .into_iter()
        .map(|(r, rule, rows)| {
            (
                r.name.clone(),
                r.id,
                hidden_group_view(r.id, rule, &rows, state),
            )
        })
        .collect();

    // Quiet repos render no rows; expanded hidden groups do.
    let widths = inputs.column_widths.with_agent(derived_agent_width(
        [
            &data.needs_attention,
            &data.working,
            &data.recent,
            &data.idle,
        ]
        .into_iter()
        .flat_map(|s| s.iter())
        .map(|r| &r.row)
        .chain(
            data.hidden
                .iter()
                .filter(|(_, _, g)| g.expanded)
                .flat_map(|(_, _, g)| g.workspaces.iter()),
        ),
    ));

    // Walk the same item sequence that render_list will emit to determine
    // which flat list index corresponds to the current selection, and
    // mark the matching row so the row composer paints a thicker gutter.
    // Also collect each row's PR chip span for click hit-testing. Quiet
    // repos have no selection model (or PR chips) in v1 — skip them.
    let mut selected_idx: Option<usize> = None;
    let mut chip_spans: Vec<PrChipSpan> = Vec::new();
    let mut flat_idx: usize = 0;
    let selection = state.selection.clone();
    for section in [
        &mut data.needs_attention,
        &mut data.working,
        &mut data.recent,
        &mut data.idle,
    ] {
        if !section.is_empty() {
            // Section header
            flat_idx += 1;
            for row in section.iter_mut() {
                if let Some(SelectionTarget::Workspace(wid)) = selection {
                    if row.row.workspace_id == wid {
                        selected_idx = Some(flat_idx);
                        row.row.selected = true;
                    }
                }
                if let Some(span) = row::pr_chip_hit_span(&row.row, widths) {
                    chip_spans.push((row.row.workspace_id, flat_idx, span));
                }
                flat_idx += 1;
            }
        }
    }
    if !data.hidden.is_empty() {
        flat_idx += 1; // HIDDEN header
        for (_, repo_id, g) in &mut data.hidden {
            if selection
                == Some(SelectionTarget::HiddenGroup {
                    repo: *repo_id,
                    rule: g.rule.clone(),
                })
            {
                selected_idx = Some(flat_idx);
            }
            flat_idx += 1;
            if !g.expanded {
                continue;
            }
            for w in &mut g.workspaces {
                if selection == Some(SelectionTarget::Workspace(w.workspace_id)) {
                    selected_idx = Some(flat_idx);
                    w.selected = true;
                }
                if let Some(span) = row::pr_chip_hit_span(w, widths) {
                    chip_spans.push((w.workspace_id, flat_idx, span));
                }
                flat_idx += 1;
            }
        }
    }
    state.list_state.select(selected_idx);

    (
        by_attention::render_list(&data, widths, tick, width, theme),
        chip_spans,
    )
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod state_defaults {
    use super::*;

    #[test]
    fn default_state_has_empty_reply_draft() {
        let s = DashboardState::default();
        assert_eq!(s.reply_draft, "");
    }
}

#[cfg(test)]
mod reconcile_selection_tests {
    use super::*;
    use crate::data::store::{RepoId, WorkspaceId};

    fn ws(n: i64) -> SelectionTarget {
        SelectionTarget::Workspace(WorkspaceId(n))
    }
    fn repo(n: i64) -> SelectionTarget {
        SelectionTarget::Repo(RepoId(n))
    }

    #[test]
    fn follows_target_to_new_index_while_visible() {
        let new = vec![ws(1), ws(3), ws(2)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 0, &new, |_| true);
        assert_eq!(sel, Some(ws(2)), "identity preserved");
        assert_eq!(idx, 2, "index follows the target");
    }

    #[test]
    fn parks_on_same_target_when_hidden_but_exists() {
        let new = vec![repo(1), ws(1)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 1, &new, |t| t == ws(2));
        assert_eq!(sel, Some(ws(2)), "selection parked on the same workspace");
        assert!(idx < new.len(), "nav cursor clamped in-bounds");
    }

    #[test]
    fn restores_index_when_target_reappears() {
        let new = vec![repo(1), ws(1), ws(2)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 1, &new, |_| true);
        assert_eq!(sel, Some(ws(2)));
        assert_eq!(idx, 2, "highlight resolves back to the workspace");
    }

    #[test]
    fn hidden_repo_target_falls_back_instead_of_parking() {
        // A selected Repo header is absent from `new_selectable` — e.g. the user
        // toggled to attention view, where `visible_targets` emits only
        // Workspace targets. Even though the repo still exists, it must NOT park
        // (which would leave no highlight and fire repo-scoped actions); it
        // falls back to a visible neighbor at the clamped index.
        let new = vec![ws(1), ws(2)];
        let (sel, idx) = reconcile_selection(Some(repo(9)), 0, &new, |_| true);
        assert_eq!(idx, 0, "clamped to a visible row");
        assert_eq!(
            sel,
            Some(ws(1)),
            "selection falls back to a visible neighbor"
        );
    }

    #[test]
    fn falls_back_to_neighbor_when_target_gone() {
        let new = vec![repo(1), ws(1), ws(3)];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 2, &new, |_| false);
        assert_eq!(idx, 2, "clamped to old index");
        assert_eq!(
            sel,
            Some(ws(3)),
            "selection becomes the neighbor at that slot"
        );
    }

    #[test]
    fn empty_selectable_yields_none() {
        let new: Vec<SelectionTarget> = vec![];
        let (sel, idx) = reconcile_selection(Some(ws(2)), 5, &new, |_| false);
        assert_eq!(sel, None);
        assert_eq!(idx, 0);
    }

    #[test]
    fn none_selection_selects_clamped_index() {
        let new = vec![repo(1), ws(1)];
        let (sel, idx) = reconcile_selection(None, 5, &new, |_| true);
        assert_eq!(idx, 1, "clamped to last");
        assert_eq!(sel, Some(ws(1)));
    }
}

#[cfg(test)]
mod section_header_tests {
    use super::*;

    /// Every section's header is colored throughout with a bold label, so
    /// No PR and Draft stand out as much as Open and Done.
    #[test]
    fn every_section_header_is_colored_and_bold() {
        let theme = Theme::wsx();
        let mut seen = Vec::new();
        for section in [
            FocusSection::NoPr,
            FocusSection::Draft,
            FocusSection::Open,
            FocusSection::Done,
        ] {
            let line = section_header_line(section, 2, 40, &theme);
            let fg = section_color(section, &theme);
            assert_ne!(fg, theme.dim, "{section:?} falls back to dim");
            assert!(line.spans.iter().all(|s| s.style.fg == Some(fg)));
            assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
            assert_eq!(line.width(), 40);
            seen.push(fg);
        }
        seen.dedup();
        assert_eq!(seen.len(), 4, "sections share a color: {seen:?}");
    }
}
