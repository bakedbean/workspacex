//! Top-level dashboard render entry point. Owns `DashboardState` and
//! the public `DashboardInputs` type that the caller assembles in `app.rs`.

pub mod by_attention;
pub mod by_repo;
pub mod detail;
#[cfg(test)]
pub(crate) mod fixture;
pub mod layout;
pub mod row;
pub mod sort;
pub mod sparkline;
pub mod spinner;
pub mod status;

use crate::app::SelectionTarget;
use crate::data::store::Repo;
use crate::ui::dashboard::by_attention::{FlatRow, QuietRepo};
use crate::ui::dashboard::by_repo::RepoView;
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::dashboard::sort::{StatusCounts, default_fold};
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
}

/// What `app.rs` passes to `render()`. Replaces the old `Item` enum.
#[derive(Debug, Clone)]
pub struct DashboardInputs<'a> {
    pub repos: Vec<&'a Repo>,
    pub workspaces: Vec<WorkspaceItem<'a>>,
    pub activity: &'a [u32],
    pub column_widths: row::ColumnWidths,
}

#[derive(Debug, Default)]
pub struct DashboardState {
    pub list_state: ListState,
    pub group_mode: GroupMode,
    /// Explicit user fold overrides; absent = use `default_fold(counts)`.
    pub folded: HashMap<u64, bool>,
    pub filter: Option<String>,
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
}

pub fn render(
    f: &mut Frame,
    area: Rect,
    inputs: &DashboardInputs<'_>,
    state: &mut DashboardState,
    tick: u32,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // list + chrome
            Constraint::Length(1), // footer
        ])
        .split(area);
    render_without_footer(f, chunks[0], inputs, state, tick, theme);
    let _ = render_footer(f, chunks[1], inputs.activity, theme, "24h");
}

/// Convert a footer line's relative hint spans into absolute screen rects,
/// clipped to `area`. Shared by the dashboard and attached footers so click
/// hit-testing stays consistent. `row` is the absolute y of the keys line.
pub(crate) fn footer_hint_rects(
    area: Rect,
    row: u16,
    hints: &[crate::ui::footer::FooterHintSpan],
) -> Vec<(Rect, crate::ui::footer::FooterHintAction)> {
    let max_col = area.x.saturating_add(area.width);
    hints
        .iter()
        .filter_map(|h| {
            let x = area.x.saturating_add(h.start_col);
            if x >= max_col {
                return None; // hint scrolled entirely off the right edge
            }
            let width = h.width.min(max_col - x);
            Some((
                Rect {
                    x,
                    y: row,
                    width,
                    height: 1,
                },
                h.action,
            ))
        })
        .collect()
}

/// Render chrome, status strip, and the workspace list into `area` without
/// painting a footer row. The caller is responsible for rendering the footer
/// (usually in a separately-carved row below the detail/PM regions so the
/// spec order list/detail/pm/footer is respected).
pub fn render_without_footer(
    f: &mut Frame,
    area: Rect,
    inputs: &DashboardInputs<'_>,
    state: &mut DashboardState,
    tick: u32,
    theme: &Theme,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // top chrome
            Constraint::Length(1), // status strip
            Constraint::Length(1), // spacer
            Constraint::Min(0),    // main list
        ])
        .split(area);
    let width = chunks[3].width as usize;

    let global_counts = StatusCounts::from_iter(inputs.workspaces.iter().map(|w| w.status));

    f.render_widget(
        Paragraph::new(layout::top_chrome(
            state.group_mode,
            inputs.repos.len(),
            inputs.workspaces.len(),
            chunks[0].width as usize,
            theme,
        )),
        chunks[0],
    );
    f.render_widget(
        Paragraph::new(layout::status_strip(global_counts, theme)),
        chunks[1],
    );

    let items = match state.group_mode {
        GroupMode::Repo => render_by_repo(inputs, state, tick, width, theme),
        GroupMode::Attention => render_by_attention(inputs, state, tick, width, theme),
    };
    let list = List::new(items).highlight_style(theme.selected_bg_style());
    f.render_stateful_widget(list, chunks[3], &mut state.list_state);
}

/// Render only the footer line (key hints + sparkline) into `area`.
/// `area` should be exactly 1 row tall. Returns the on-screen `Rect` of the
/// clickable activity graph (the trailing "<label> <sparkline>" run) plus the
/// clickable rect + action of each keybind hint, so the caller can hit-test
/// clicks on them.
pub fn render_footer(
    f: &mut Frame,
    area: Rect,
    activity: &[u32],
    theme: &Theme,
    window_label: &str,
) -> (Rect, Vec<(Rect, crate::ui::footer::FooterHintAction)>) {
    let (line, graph_w, hints) = layout::footer(
        activity,
        env!("CARGO_PKG_VERSION"),
        area.width as usize,
        theme,
        window_label,
    );
    f.render_widget(Paragraph::new(line), area);
    let hint_rects = footer_hint_rects(area, area.y, &hints);
    // The graph is right-aligned within the footer row.
    let x = area.x + area.width.saturating_sub(graph_w);
    let graph_rect = Rect {
        x,
        y: area.y,
        width: graph_w.min(area.width),
        height: 1,
    };
    (graph_rect, hint_rects)
}

/// Return the sequence of selectable targets in *visible order*, matching
/// what `render()` produces. The caller (`app.rs::draw`) writes this
/// into `App::selectable` so arrow-key navigation walks the same order
/// the user sees on screen instead of the raw `app.workspaces` order
/// (which the V5 renderer reshuffles by noise score / status priority /
/// fold state / filter).
///
/// By-repo: emits `Repo(id)` for each visible header followed by
/// `Workspace(id)` for each visible workspace inside expanded repos.
///
/// By-attention: emits `Workspace(id)` for each row across the four
/// active sections (NEEDS ATTENTION / WORKING / RECENT / IDLE) in the
/// order `partition` produces. QUIET REPOS entries are skipped — they
/// have no per-repo selection model in v1.
pub fn visible_targets(
    inputs: &DashboardInputs<'_>,
    state: &DashboardState,
) -> Vec<SelectionTarget> {
    let filter = state.filter.as_deref().filter(|f| !f.is_empty());
    let mut out: Vec<SelectionTarget> = Vec::new();
    match state.group_mode {
        GroupMode::Repo => {
            // Mirror render_by_repo's ordering: per-repo filter + sort,
            // then persisted sort_order ordering across repos.
            #[derive(Clone)]
            struct Pending {
                repo_id: crate::data::store::RepoId,
                counts: StatusCounts,
                sort_order: i64,
                workspace_ids: Vec<crate::data::store::WorkspaceId>,
            }
            let mut pending: Vec<Pending> = inputs
                .repos
                .iter()
                .map(|r| {
                    let mut rows: Vec<(Status, crate::data::store::WorkspaceId)> = inputs
                        .workspaces
                        .iter()
                        .filter(|w| w.repo.id == r.id)
                        .filter(|w| filter.map(|f| matches_filter(w, f)).unwrap_or(true))
                        .map(|w| (w.status, w.workspace_id))
                        .collect();
                    rows.sort_by_key(|r| std::cmp::Reverse(r.0.priority()));
                    let counts = StatusCounts::from_iter(rows.iter().map(|(s, _)| *s));
                    Pending {
                        repo_id: r.id,
                        counts,
                        sort_order: r.sort_order,
                        workspace_ids: rows.into_iter().map(|(_, id)| id).collect(),
                    }
                })
                .collect();
            // Mirror by_repo::order_repos: stable manual order, ascending.
            pending.sort_by_key(|p| p.sort_order);
            for p in &pending {
                out.push(SelectionTarget::Repo(p.repo_id));
                let expanded = match state.folded.get(&(p.repo_id.0 as u64)).copied() {
                    Some(explicit) => !explicit,
                    None => !default_fold(p.counts),
                };
                if expanded {
                    for wid in &p.workspace_ids {
                        out.push(SelectionTarget::Workspace(*wid));
                    }
                }
            }
        }
        GroupMode::Attention => {
            // Mirror render_by_attention: filter, drop idle rows that
            // appear under QUIET REPOS, then partition (which applies
            // the per-section ordering).
            let rows: Vec<FlatRow> = inputs
                .workspaces
                .iter()
                .filter(|w| filter.map(|f| matches_filter(w, f)).unwrap_or(true))
                .map(|w| FlatRow {
                    repo_name: w.repo.name.clone(),
                    row: w.row.clone(),
                })
                .collect();
            // Build the same quiet-repo set the renderer uses so we drop
            // the right idle rows.
            let mut quiet_names: std::collections::HashSet<String> = Default::default();
            for r in &inputs.repos {
                let repo_rows: Vec<&WorkspaceItem<'_>> = inputs
                    .workspaces
                    .iter()
                    .filter(|w| w.repo.id == r.id)
                    .filter(|w| filter.map(|f| matches_filter(w, f)).unwrap_or(true))
                    .collect();
                let count = repo_rows.len();
                let all_idle = !repo_rows.is_empty()
                    && repo_rows.iter().all(|w| matches!(w.status, Status::Idle));
                let repo_matches_filter = filter
                    .map(|f| r.name.to_lowercase().contains(&f.to_lowercase()))
                    .unwrap_or(true);
                let include_empty = count == 0 && (filter.is_none() || repo_matches_filter);
                if include_empty || all_idle {
                    quiet_names.insert(r.name.clone());
                }
            }
            let rows: Vec<FlatRow> = rows
                .into_iter()
                .filter(|r| {
                    !matches!(r.row.status, Status::Idle) || !quiet_names.contains(&r.repo_name)
                })
                .collect();
            // We don't need the quiet_repos for selection (skipped),
            // but partition wants the type; pass an empty Vec.
            let data = by_attention::partition(rows, Vec::new());
            for section in [
                &data.needs_attention,
                &data.working,
                &data.recent,
                &data.idle,
            ] {
                for r in section {
                    out.push(SelectionTarget::Workspace(r.row.workspace_id));
                }
            }
        }
    }
    out
}

/// Case-insensitive substring match against the workspace name, branch,
/// owning repo name, and the last assistant message (when present).
fn matches_filter(w: &WorkspaceItem<'_>, filter: &str) -> bool {
    let needle = filter.to_lowercase();
    w.row.name.to_lowercase().contains(&needle)
        || w.row.branch.to_lowercase().contains(&needle)
        || w.repo.name.to_lowercase().contains(&needle)
        || w.row
            .last_message
            .as_deref()
            .map(|m| m.to_lowercase().contains(&needle))
            .unwrap_or(false)
}

fn render_by_repo<'a>(
    inputs: &DashboardInputs<'a>,
    state: &mut DashboardState,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ratatui::widgets::ListItem<'static>> {
    let filter = state.filter.as_deref().filter(|f| !f.is_empty());
    let mut views: Vec<RepoView<'a>> = inputs
        .repos
        .iter()
        .map(|r| {
            let mut workspaces: Vec<RowInputs> = inputs
                .workspaces
                .iter()
                .filter(|w| w.repo.id == r.id)
                .filter(|w| filter.map(|f| matches_filter(w, f)).unwrap_or(true))
                .map(|w| w.row.clone())
                .collect();
            workspaces.sort_by_key(|w| std::cmp::Reverse(w.status.priority()));
            let counts = StatusCounts::from_iter(workspaces.iter().map(|w| w.status));
            let repo_id_u64 = r.id.0 as u64;
            let expanded = match state.folded.get(&repo_id_u64).copied() {
                Some(explicit) => !explicit,
                None => !default_fold(counts),
            };
            RepoView {
                id: repo_id_u64,
                name: &r.name,
                path: r.path.to_string_lossy().into_owned(),
                counts,
                expanded,
                sort_order: r.sort_order,
                workspaces,
            }
        })
        .collect();
    by_repo::order_repos(&mut views);

    // Walk the same item sequence that render_list will emit to determine
    // which flat list index corresponds to the current selection. Also
    // flip `selected: true` on the matching workspace so the row composer
    // can paint a thicker gutter glyph for the selected row.
    let mut selected_idx: Option<usize> = None;
    let mut flat_idx: usize = 0;
    let selection = state.selection;
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
            flat_idx += 1;
        }
        // Spacer item
        flat_idx += 1;
    }
    state.list_state.select(selected_idx);

    by_repo::render_list(&views, inputs.column_widths, tick, width, theme)
}

fn render_by_attention<'a>(
    inputs: &DashboardInputs<'a>,
    state: &mut DashboardState,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ratatui::widgets::ListItem<'static>> {
    let filter = state.filter.as_deref().filter(|f| !f.is_empty());
    let rows: Vec<FlatRow> = inputs
        .workspaces
        .iter()
        .filter(|w| filter.map(|f| matches_filter(w, f)).unwrap_or(true))
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
            .filter(|w| filter.map(|f| matches_filter(w, f)).unwrap_or(true))
            .collect();
        let count = repo_rows.len();
        let all_idle =
            !repo_rows.is_empty() && repo_rows.iter().all(|w| matches!(w.status, Status::Idle));
        let repo_matches_filter = filter
            .map(|f| r.name.to_lowercase().contains(&f.to_lowercase()))
            .unwrap_or(true);
        // Empty repos only show in QUIET REPOS when no filter is active
        // OR when the filter matches the repo name itself.
        let include_empty = count == 0 && (filter.is_none() || repo_matches_filter);
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

    // Walk the same item sequence that render_list will emit to determine
    // which flat list index corresponds to the current selection, and
    // mark the matching row so the row composer paints a thicker gutter.
    // Quiet repos have no selection model in v1 — skip them.
    let mut selected_idx: Option<usize> = None;
    let mut flat_idx: usize = 0;
    let selection = state.selection;
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
                flat_idx += 1;
            }
        }
    }
    state.list_state.select(selected_idx);

    by_attention::render_list(&data, inputs.column_widths, tick, width, theme)
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
