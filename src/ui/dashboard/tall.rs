//! Tall rows for the repo focus view: one repo's workspaces as a list in
//! which each workspace spans several lines.
//!
//! Each row has three zones: a strip of fixed columns (branch, PR chip,
//! status, ahead, uncommitted, diff, age) sized once per frame so fields
//! line up down the view; the prose under the branch (initial prompt, recap
//! goal and state → next, or the agent's last message), wrapped at a
//! comfortable reading width; and a footer of agents and tags. Alternate
//! rows take a background tint so each workspace reads as one block.
//!
//! When the strip doesn't fit on one line it breaks in two: branch, PR chip
//! and age, then status, ahead, uncommitted and diff under the branch.
//!
//! Rows scroll by line rather than by item, so a workspace taller than the
//! screen still shows from its first line.
//!
//! Pure layout and painting. The caller picks the workspaces and their
//! order (see `focused_workspaces` in the parent module) and owns the
//! selection.

use crate::data::store::{WorkspaceId, WorkspaceRecap};
use crate::git::WorkspaceStatus;
use crate::pty::session::AgentKind;
use crate::ui::dashboard::column_content::{ColumnBody, ColumnEmphasis};
use crate::ui::dashboard::row::{self, ColumnWidths, RowInputs};
use crate::ui::text::{display_width, truncate, truncate_words};
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// What a tall row shows beyond the dashboard row: the full recap fields
/// and the git counts the one-line row has no room for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceDetail {
    pub recap: Option<WorkspaceRecap>,
    /// Working-tree counts against upstream.
    pub git: Option<WorkspaceStatus>,
    /// Commits on the branch that the repo's base branch lacks.
    pub ahead_of_base: Option<u32>,
    /// The repo's base branch as configured (`origin/main`), naming what
    /// `ahead_of_base` counts against.
    pub base_branch: Option<String>,
    /// The first prompt the workspace's agent was given, from its session
    /// log; `None` until the log is scanned or when it has none.
    pub first_prompt: Option<String>,
}

/// Each agent kind's icon, as the theme's `[agent_bar.symbols]` table
/// draws it, so the focus view names agents with the same glyphs as the
/// agent bar and pills. Kinds without an entry use the bar's `symbol`.
#[derive(Debug, Clone)]
pub struct AgentGlyphs {
    by_kind: Vec<(AgentKind, String)>,
    fallback: String,
}

impl Default for AgentGlyphs {
    fn default() -> Self {
        Self {
            by_kind: Vec::new(),
            fallback: "▎".to_string(),
        }
    }
}

impl AgentGlyphs {
    /// Read the resolved theme's `agent_bar` segment.
    pub fn from_specs(specs: &crate::config::theme_file::BarSpecs) -> Self {
        let Some(cfg) = specs.segments.get("agent_bar") else {
            return Self::default();
        };
        Self {
            by_kind: cfg.symbols.clone(),
            fallback: cfg.symbol.clone().unwrap_or_else(|| "▎".to_string()),
        }
    }

    /// The icon for `kind`, or `None` when the theme blanks it — the kind
    /// is then named by its label alone.
    pub fn glyph(&self, kind: AgentKind) -> Option<&str> {
        let g = self
            .by_kind
            .iter()
            .find(|(k, _)| *k == kind)
            .map_or(self.fallback.as_str(), |(_, g)| g.as_str());
        (!g.is_empty()).then_some(g)
    }
}

/// One tall row's inputs.
#[derive(Debug, Clone)]
pub struct TallRow<'a> {
    pub row: &'a RowInputs,
    pub detail: &'a WorkspaceDetail,
}

/// Click targets on the drawn rows, in screen coordinates.
#[derive(Debug, Default)]
pub struct TallTargets {
    /// Each visible row's whole block — a click attaches.
    pub rows: Vec<(WorkspaceId, Rect)>,
    /// Each visible hidden-group summary line, by its index in the entries
    /// passed to `render` — a click expands or collapses it.
    pub groups: Vec<(usize, Rect)>,
    /// Each visible row's PR chip — a click opens the PR.
    pub pr_chips: Vec<(WorkspaceId, Rect)>,
}

/// One entry in the focus view: a workspace's tall row, or the one-line
/// summary of a hidden group (`▸ #parked · 3 hidden`), drawn as on the list.
#[derive(Debug, Clone)]
pub enum TallEntry<'a> {
    Row(TallRow<'a>),
    Group(Line<'static>),
    /// A PR-state header line; never a target.
    Section(Line<'static>),
}

/// Width of the label column on detail lines (`prompt  `, `goal    `, …):
/// the longest label plus two spaces.
const LABEL_WIDTH: usize = 8;

/// Most lines the initial prompt may wrap onto. Prompts are often pasted
/// briefs many paragraphs long; the opening is enough to say what the
/// workspace was started for.
const PROMPT_MAX_LINES: usize = 2;

/// Most lines the no-recap fallback text may wrap onto.
const FALLBACK_MAX_LINES: usize = 2;

/// Widest the prose column gets, label included: lines much past ~80
/// characters of text get tiring to read.
const PROSE_MAX: usize = 92;

/// One labelled run of prose on a tall row, before wrapping.
struct Prose {
    label: &'static str,
    text: String,
    style: Style,
    /// Most lines it may wrap onto.
    max: usize,
}

/// The prose a row shows, in order: the initial prompt, then the recap's
/// goal and state → next, or without a recap the row's fallback text.
fn prose_items(row: &RowInputs, detail: &WorkspaceDetail, theme: &Theme) -> Vec<Prose> {
    let mut out = Vec::new();
    let recap = detail.recap.as_ref().filter(|r| has_recap(r));
    // What the workspace was started for, above what it is doing now.
    let prompt = detail
        .first_prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    if let Some(prompt) = prompt {
        out.push(Prose {
            label: "prompt",
            text: prompt.to_string(),
            style: theme
                .row_dim_style()
                .add_modifier(ratatui::style::Modifier::ITALIC),
            max: PROMPT_MAX_LINES,
        });
    }
    match recap {
        Some(r) => {
            let goal = field(&r.goal, &r.goal_short);
            if !goal.is_empty() {
                out.push(Prose {
                    label: "goal",
                    text: goal,
                    style: Style::default(),
                    max: usize::MAX,
                });
            }
            let state = field(&r.state, &r.state_short);
            let next = field(&r.next, &r.next_short);
            let now = match (state.is_empty(), next.is_empty()) {
                (false, false) => format!("{state} → {next}"),
                (false, true) => state,
                (true, false) => format!("→ {next}"),
                (true, true) => String::new(),
            };
            if !now.is_empty() {
                out.push(Prose {
                    label: "now",
                    text: now,
                    style: theme.row_dim_style(),
                    max: usize::MAX,
                });
            }
        }
        None => {
            // Without a recap, the row's fallback text (often the agent's
            // last message, which can run to paragraphs) moves here from the
            // first line, gets a little more room, and stays capped so one
            // chatty workspace can't push its siblings off screen. It's
            // labelled `last`, not `now`: it's what the agent last said, not
            // a state it reported.
            //
            // An idle (or finished, before its first reply) workspace's
            // fallback *is* the initial prompt; the `prompt` line already
            // shows it, so it isn't repeated as if the agent said it.
            let is_prompt = |text: &str| {
                prompt.is_some_and(|p| p.split_whitespace().eq(text.split_whitespace()))
            };
            if let Some(ColumnBody::Fallback { text, emphasis }) =
                row.column.as_ref().map(|c| &c.body)
                && !is_prompt(text)
            {
                let style = match emphasis {
                    ColumnEmphasis::Dim => theme.row_dim_style(),
                    ColumnEmphasis::Status => theme.status_style(row.status),
                    ColumnEmphasis::Warn => theme.warn_style(),
                };
                out.push(Prose {
                    label: "last",
                    text: text.clone(),
                    style,
                    max: FALLBACK_MAX_LINES,
                });
            }
        }
    }
    out
}

/// Bounds on the strip's branch column, its status glyph included.
const BRANCH_MIN: usize = 12;
const BRANCH_MAX: usize = 44;
/// Blank cells between strip columns.
const STRIP_GAP: usize = 2;

/// Each strip cell's width, the widest across every row in the frame, so
/// fields line up down the view. A zero-width column is skipped with its gap.
#[derive(Debug, Clone, Copy)]
pub struct StripColumns {
    pub widths: ColumnWidths,
    pub branch: usize,
    pub pr: usize,
    pub status: usize,
    pub ahead: usize,
    pub sync: usize,
    pub dirty: usize,
    pub diff: usize,
    pub age: usize,
    pub two_line: bool,
    pub prose: usize,
}

/// Size the strip's columns for `rows` at `width`. The branch column fits
/// the longest branch within its bounds; when that strip overflows `width`
/// it breaks in two and the branch column shrinks to fit line 1, never
/// below `BRANCH_MIN`. A longer branch wraps.
pub fn strip_columns(rows: &[TallRow<'_>], widths: ColumnWidths, width: usize) -> StripColumns {
    let max = |f: &dyn Fn(&TallRow<'_>) -> usize| rows.iter().map(f).max().unwrap_or(0);
    let spans_width = |s: &[Span<'_>]| s.iter().map(Span::width).sum::<usize>();
    // Styles don't change widths; any theme sizes the cells.
    let theme = Theme::wsx();
    let pr = max(&|r| full_chip(r.row).map_or(0, |c| c.width()))
        .clamp(display_width(NO_PR), row::MAX_PR_WIDTH);
    let status = max(&|r| spans_width(&status_spans(r.row, 0, &theme)));
    let ahead = max(&|r| ahead_text(r.detail).map_or(0, |t| display_width(&t)));
    let sync = max(&|r| spans_width(&sync_spans(r.row, r.detail, &theme)));
    let dirty = max(&|r| dirty_text(r.detail).map_or(0, |t| display_width(&t)));
    let diff = max(&|r| spans_width(&diff_spans(r.row, &theme)));
    let age = max(&|r| display_width(&age_text(r.row)));
    let prefix = widths.agent + row::GUTTER_WIDTH + 1;
    let natural = max(&|r| 2 + display_width(&r.row.branch)).clamp(BRANCH_MIN, BRANCH_MAX);
    let one_line =
        prefix + natural + STRIP_GAP + pr + cells_width([status, ahead, sync, dirty, diff, age]);
    let two_line = one_line > width;
    let prose = width.saturating_sub(prefix + 2).min(PROSE_MAX);
    if !two_line {
        return StripColumns {
            widths,
            branch: natural,
            pr,
            status,
            ahead,
            sync,
            dirty,
            diff,
            age,
            two_line,
            prose,
        };
    }
    // Line 1 holds branch, PR and age: the branch gives way first, then the
    // PR chip (which drops its lifecycle word when fitted narrower).
    let room = width.saturating_sub(prefix + STRIP_GAP + cells_width([age]));
    let branch = natural.min(room.saturating_sub(pr)).max(BRANCH_MIN);
    let pr = pr
        .min(room.saturating_sub(branch))
        .max(display_width(NO_PR));
    // Below the branch, a field wider than the line is cut to fit it.
    let fit = |w: usize| w.min(prose);
    StripColumns {
        widths,
        branch,
        pr,
        status: fit(status),
        ahead: fit(ahead),
        sync: fit(sync),
        dirty: fit(dirty),
        diff: fit(diff),
        age,
        two_line,
        prose,
    }
}

/// The status cell: report mark and token, process count, lifecycle badge,
/// and undelivered-mail mark.
fn status_spans(row: &RowInputs, tick: u32, theme: &Theme) -> Vec<Span<'static>> {
    use crate::ui::dashboard::status::Status;
    let mut status: Vec<Span<'static>> = Vec::new();
    if let Some(col) = row.column.as_ref() {
        let prefix = if col.reported { "▸ " } else { "└ " };
        let token_style = if row.status == Status::Stalled && !col.reported {
            theme.warn_style()
        } else {
            theme.status_style(row.status)
        };
        status.push(Span::styled(prefix, theme.status_style(row.status)));
        status.push(Span::styled(col.token.clone(), token_style));
    }
    if row.procs > 0 {
        status.push(Span::styled(
            format!("  ● {}p", row.procs),
            theme.status_style(Status::Thinking),
        ));
    }
    if let Some(b) = row.badge {
        status.push(Span::styled(b.glyph(tick), b.style(theme)));
    }
    if row.undelivered_mail {
        status.push(Span::styled(" ✉!", theme.err_style()));
    }
    status
}

fn ahead_text(detail: &WorkspaceDetail) -> Option<String> {
    detail
        .ahead_of_base
        .map(|n| ahead_label(n, detail.base_branch.as_deref()))
}

/// Where the branch stands against its remote: `⇡2 unpushed` and `⇣3
/// behind` against its upstream, or a dim `not pushed` for a branch with
/// commits but no upstream and no PR (a PR means it was pushed; a merged
/// branch's upstream is often deleted). Nothing when in sync or unknown.
fn sync_spans(row: &RowInputs, detail: &WorkspaceDetail, theme: &Theme) -> Vec<Span<'static>> {
    use crate::git::forge::BranchLifecycle;
    let Some(g) = detail.git else {
        return Vec::new();
    };
    if !g.upstream {
        let no_pr = matches!(row.lifecycle, None | Some(BranchLifecycle::NoPr));
        return if no_pr && detail.ahead_of_base.is_some_and(|n| n > 0) {
            vec![Span::styled("not pushed", theme.row_dim_style())]
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    for (n, label) in [(g.ahead, "⇡{} unpushed"), (g.behind, "⇣{} behind")] {
        if n == 0 {
            continue;
        }
        if !out.is_empty() {
            out.push(Span::raw(" "));
        }
        out.push(Span::styled(
            label.replace("{}", &n.to_string()),
            theme.warn_style(),
        ));
    }
    out
}

/// `3 mod 1 untr`; `None` for a clean or unknown tree.
fn dirty_text(detail: &WorkspaceDetail) -> Option<String> {
    let g = detail.git?;
    let parts: Vec<String> = [(g.modified, "mod"), (g.untracked, "untr")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, l)| format!("{n} {l}"))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// `+184 −22`, or nothing without changes.
fn diff_spans(row: &RowInputs, theme: &Theme) -> Vec<Span<'static>> {
    match row.diff.filter(|d| d.added > 0 || d.removed > 0) {
        Some(d) => vec![
            Span::styled(format!("+{}", d.added), theme.ok_style()),
            Span::raw(" "),
            Span::styled(format!("−{}", d.removed), theme.err_style()),
        ],
        None => Vec::new(),
    }
}

fn age_text(row: &RowInputs) -> String {
    crate::ui::dashboard::column_content::format_ago_short(row.ago_secs)
}

/// The PR column's text for a row without a PR.
const NO_PR: &str = "no PR";

/// A row's PR chip at its natural width, or `None` without a PR.
fn full_chip(row: &RowInputs) -> Option<crate::ui::theme::PrChip> {
    crate::ui::theme::pr_chip(
        row.lifecycle?,
        row.pr_number,
        row.review,
        row.unresolved,
        usize::MAX,
    )
}

/// The PR column's cell, exactly `pr` cells wide, and the chip's painted
/// width (`None` for the dim `no PR` placeholder, which isn't clickable).
/// The width comes from the chip fitted to `pr`, as painted: fitting can
/// drop the lifecycle word, leaving padding that must not open the PR.
fn pr_cell(row: &RowInputs, pr: usize, theme: &Theme) -> (Vec<Span<'static>>, Option<u16>) {
    let fitted = row.lifecycle.and_then(|lc| {
        crate::ui::theme::pr_chip(lc, row.pr_number, row.review, row.unresolved, pr)
    });
    match fitted {
        Some(c) => (
            row::pr_chip_spans(
                row.lifecycle,
                row.pr_number,
                row.review,
                row.unresolved,
                pr,
                theme,
            ),
            Some(c.width().min(pr) as u16),
        ),
        None => (
            vec![Span::styled(format!("{NO_PR:<pr$}"), theme.dim_style())],
            None,
        ),
    }
}

/// A row's lines plus where its PR chip landed: `(line, x offset, width)`.
pub struct RowLayout {
    pub lines: Vec<Line<'static>>,
    pub pr_hit: Option<(usize, u16, u16)>,
}

/// Lay out one row: the strip (branch continuation lines under it), the
/// prose under the branch, then the agents-and-tags footer.
pub fn row_layout(
    tall: &TallRow<'_>,
    glyphs: &AgentGlyphs,
    cols: StripColumns,
    tick: u32,
    theme: &Theme,
) -> RowLayout {
    let row = tall.row;
    let widths = cols.widths;
    let gutter = Span::styled(
        if row.selected { "▍" } else { "▎" },
        theme.status_style(row.status),
    );
    // Every line after the strip: agent padding, gutter, and the indent
    // that puts content under the branch name.
    let lead = || {
        vec![
            Span::raw(" ".repeat(widths.agent)),
            gutter.clone(),
            Span::raw("   "),
        ]
    };

    let (glyph, glyph_style) = if row.status.is_live() {
        (
            crate::ui::dashboard::spinner::frame(tick),
            theme.agent_style(row.agent),
        )
    } else {
        (row.status.glyph(), theme.status_style(row.status))
    };
    let mut name_style = Style::default().add_modifier(ratatui::style::Modifier::BOLD);
    if row.yolo {
        name_style = name_style.fg(theme.warn);
    }
    if let Some(c) = row.name_color {
        name_style = name_style.fg(c);
    }
    let name_width = cols.branch.saturating_sub(2).max(1);
    let mut branch = wrap(&row.branch, name_width).into_iter();
    let first = branch.next().unwrap_or_default();

    let mut strip = row::agent_strip_spans(row, widths, tick, theme);
    strip.push(gutter.clone());
    strip.push(Span::raw(" "));
    strip.push(Span::styled(format!("{glyph} "), glyph_style));
    let pad = name_width.saturating_sub(display_width(&first));
    strip.push(Span::styled(first, name_style));
    strip.push(Span::raw(" ".repeat(pad + STRIP_GAP)));
    let (pr_spans, pr_width) = pr_cell(row, cols.pr, theme);
    strip.extend(pr_spans);
    let fields = [
        (cols.status, status_spans(row, tick, theme)),
        (cols.ahead, dim_cell(ahead_text(tall.detail), theme)),
        (cols.sync, sync_spans(row, tall.detail, theme)),
        (cols.dirty, dim_cell(dirty_text(tall.detail), theme)),
        (cols.diff, diff_spans(row, theme)),
    ];
    let age = (
        cols.age,
        vec![Span::styled(age_text(row), theme.row_dim_style())],
        true,
    );
    // Two-line strips keep only the age on line 1; the rest go under the
    // branch, after its continuation lines.
    let below = if cols.two_line {
        push_cells(&mut strip, [age]);
        fields.to_vec()
    } else {
        let cells = fields.into_iter().map(|(w, s)| (w, s, false));
        push_cells(&mut strip, cells.chain([age]));
        Vec::new()
    };
    let mut lines = vec![Line::from(strip)];
    for part in branch {
        let mut l = lead();
        l.push(Span::styled(part, name_style));
        lines.push(Line::from(l));
    }
    // A column that overflows the line starts the next; widths are shared,
    // so every row breaks at the same column and rows still align.
    let mut groups: Vec<Vec<(usize, Vec<Span<'static>>)>> = Vec::new();
    let mut used = 0;
    for (w, spans) in below.into_iter().filter(|(w, _)| *w > 0) {
        match groups.last_mut() {
            Some(g) if used + w <= cols.prose => g.push((w, spans)),
            _ => {
                groups.push(vec![(w, spans)]);
                used = 0;
            }
        }
        used += w + STRIP_GAP;
    }
    for group in groups {
        if group.iter().all(|(_, s)| s.is_empty()) {
            continue;
        }
        let mut cells = Vec::new();
        let fitted = group.into_iter().map(|(w, s)| (w, fit_spans(s, w), false));
        push_cells(&mut cells, fitted);
        let mut l = lead();
        // Drop the leading gap: the lead already indents to the branch.
        l.extend(cells.into_iter().skip(1));
        lines.push(Line::from(l));
    }

    let text_width = cols.prose.saturating_sub(LABEL_WIDTH).max(8);
    for item in prose_items(row, tall.detail, theme) {
        for (i, text) in wrap_capped(&item.text, text_width, item.max)
            .into_iter()
            .enumerate()
        {
            let label = if i == 0 { item.label } else { "" };
            let mut l = lead();
            l.push(Span::styled(
                format!("{label:<LABEL_WIDTH$}"),
                theme.dim_style(),
            ));
            l.push(Span::styled(text, item.style));
            lines.push(Line::from(l));
        }
    }

    for part in pack(
        footer_parts(row, glyphs, theme, cols.prose),
        &Span::raw("  "),
        cols.prose,
    ) {
        let mut l = lead();
        l.extend(part);
        lines.push(Line::from(l));
    }

    let pr_x = (widths.agent + row::GUTTER_WIDTH + 1 + cols.branch + STRIP_GAP) as u16;
    RowLayout {
        lines,
        pr_hit: pr_width.map(|w| (0, pr_x, w)),
    }
}

/// Append strip cells, each led by a gap and padded to its column (right
/// aligned when flagged); zero-width columns are skipped.
fn push_cells(
    out: &mut Vec<Span<'static>>,
    cells: impl IntoIterator<Item = (usize, Vec<Span<'static>>, bool)>,
) {
    for (w, spans, right) in cells {
        if w == 0 {
            continue;
        }
        let pad = w.saturating_sub(spans.iter().map(Span::width).sum());
        out.push(Span::raw(" ".repeat(STRIP_GAP)));
        if right {
            out.push(Span::raw(" ".repeat(pad)));
            out.extend(spans);
        } else {
            out.extend(spans);
            out.push(Span::raw(" ".repeat(pad)));
        }
    }
}

/// Cut `spans` to `width` cells, ending in `…` when anything was dropped.
fn fit_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    if spans.iter().map(Span::width).sum::<usize>() <= width {
        return spans;
    }
    let mut out = Vec::new();
    let mut left = width;
    for s in spans {
        let w = s.width();
        if w < left {
            left -= w;
            out.push(s);
        } else {
            // The input overflows, so even a span that fills `left` exactly
            // is cut: the marker forces truncate's ellipsis.
            let cut = truncate(&format!("{}…", s.content), left);
            out.push(Span::styled(cut, s.style));
            break;
        }
    }
    out
}

/// Cells taken by columns of widths `ws`, each with its leading gap.
fn cells_width(ws: impl IntoIterator<Item = usize>) -> usize {
    ws.into_iter()
        .filter(|&w| w > 0)
        .map(|w| STRIP_GAP + w)
        .sum()
}

fn dim_cell(text: Option<String>, theme: &Theme) -> Vec<Span<'static>> {
    text.map(|t| vec![Span::styled(t, theme.row_dim_style())])
        .unwrap_or_default()
}

/// Agents (primary first, each led by its icon; active peers in their
/// hue), shared/layout badges, then tags each fitted to `width`, as parts
/// for [`pack`].
fn footer_parts(
    row: &RowInputs,
    glyphs: &AgentGlyphs,
    theme: &Theme,
    width: usize,
) -> Vec<Vec<Span<'static>>> {
    use crate::ui::dashboard::status::Status;
    let agents = std::iter::once((row.agent, false))
        .chain(row.peers.iter().map(|p| (p.agent, p.active)))
        .map(|(kind, active)| {
            let mut a = Vec::new();
            if let Some(g) = glyphs.glyph(kind) {
                a.push(Span::styled(format!("{g} "), theme.agent_style(kind)));
            }
            let label_style = if active {
                theme.agent_style(kind)
            } else {
                theme.row_dim_style()
            };
            a.push(Span::styled(kind.display_name().to_string(), label_style));
            a
        });
    // Shared-session and layout badges, as the dashboard row draws them.
    let mut badges = Vec::new();
    if row.shared {
        let (glyph, label, style) = match (row.nerd_fonts, row.shared_active) {
            (true, true) => ("\u{f0c53}", "shared", theme.status_style(Status::Complete)),
            (true, false) => ("\u{f015b}", "shared, no session", theme.err_style()),
            (false, true) => ("◇", "shared", theme.status_style(Status::Complete)),
            (false, false) => ("◇", "shared, no session", theme.err_style()),
        };
        badges.push(vec![Span::styled(format!("{glyph} {label}"), style)]);
    }
    if row.has_multi_pane_layout {
        let label = if row.nerd_fonts {
            "\u{f0db} layout"
        } else {
            "layout"
        };
        badges.push(vec![Span::styled(label, theme.row_dim_style())]);
    }
    // Each tag fits the footer on its own, so `pack` never spills one.
    let tags = row.tags.iter().map(|t| {
        let tag = truncate(&format!("#{t}"), width);
        vec![Span::styled(tag, theme.row_dim_style())]
    });
    agents.chain(badges).chain(tags).collect()
}

/// `↑4 vs main`: commits ahead of the base branch, named without its
/// `origin/` remote prefix. Falls back to `↑4 base` if the name is unknown.
fn ahead_label(n: u32, base: Option<&str>) -> String {
    match base.map(str::trim).filter(|b| !b.is_empty()) {
        Some(b) => format!("↑{n} vs {}", b.strip_prefix("origin/").unwrap_or(b)),
        None => format!("↑{n} base"),
    }
}

/// Pack `parts` onto lines of at most `width` cells, joined by `sep`; a
/// part wider than a line gets a line of its own.
fn pack(
    parts: Vec<Vec<Span<'static>>>,
    sep: &Span<'static>,
    width: usize,
) -> Vec<Vec<Span<'static>>> {
    let w = |p: &[Span<'static>]| p.iter().map(|s| s.width()).sum::<usize>();
    let mut out: Vec<Vec<Span<'static>>> = Vec::new();
    let mut cur: Vec<Span<'static>> = Vec::new();
    for part in parts {
        let pw = w(&part);
        if !cur.is_empty() && w(&cur) + sep.width() + pw > width {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(sep.clone());
        }
        cur.extend(part);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Draw the rows into `area`, scrolled by whole lines so a row taller than
/// the viewport still shows (its top first), and return what a click can
/// land on. A blank line separates rows. `scroll` is the first visible
/// line; it is moved as little as possible to keep the selected row in
/// view.
///
/// Rows are painted line by line rather than as `List` items: `List` skips
/// any item that doesn't fit whole, which would blank out a selected row
/// with a long recap and leave click targets over lines it never drew.
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    entries: &[TallEntry<'_>],
    glyphs: &AgentGlyphs,
    selected: Option<usize>,
    scroll: &mut usize,
    follow: bool,
    widths: ColumnWidths,
    tick: u32,
    theme: &Theme,
) -> TallTargets {
    let width = area.width as usize;
    let rows: Vec<TallRow<'_>> = entries
        .iter()
        .filter_map(|e| match e {
            TallEntry::Row(r) => Some(r.clone()),
            TallEntry::Group(_) | TallEntry::Section(_) => None,
        })
        .collect();
    let cols = strip_columns(&rows, widths, width);
    // Every line of every entry, plus each entry's `(first line, line
    // count)` and, for rows, where the PR chip landed.
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(entries.len());
    let mut pr_hits: Vec<Option<(usize, u16, u16)>> = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            lines.push(Line::default());
        }
        let (entry, pr_hit) = match e {
            TallEntry::Row(r) => {
                let l = row_layout(r, glyphs, cols, tick, theme);
                (l.lines, l.pr_hit)
            }
            TallEntry::Group(line) | TallEntry::Section(line) => (vec![line.clone()], None),
        };
        spans.push((lines.len(), entry.len()));
        pr_hits.push(pr_hit);
        lines.extend(entry);
    }
    // On follow frames a row right under a section header scrolls as one
    // block with it, so the header comes into view too. Manual scrolls
    // keep only the row's own lines pinned.
    let height = area.height as usize;
    let sel_span = selected.and_then(|i| {
        let (top, len) = spans.get(i).copied()?;
        match i.checked_sub(1).and_then(|p| entries.get(p)) {
            // Only when header, gap and the row's first line all fit.
            Some(TallEntry::Section(_)) if follow && top - spans[i - 1].0 < height => {
                let header_top = spans[i - 1].0;
                Some((header_top, top + len - header_top))
            }
            _ => Some((top, len)),
        }
    });
    *scroll = scroll_for(*scroll, sel_span, height, lines.len(), follow);

    let visible: Vec<Line<'static>> = lines.into_iter().skip(*scroll).take(height).collect();
    f.render_widget(Paragraph::new(visible), area);

    let mut targets = TallTargets::default();
    // Alternate workspaces take a faint background so each reads as one
    // block; counted over rows only, so a group line doesn't flip the
    // pattern.
    let mut row_ordinal = 0usize;
    for (i, &(top, len)) in spans.iter().enumerate() {
        let is_row = matches!(entries[i], TallEntry::Row(_));
        if matches!(entries[i], TallEntry::Section(_)) {
            row_ordinal = 0;
        }
        let tinted = is_row && row_ordinal % 2 == 1;
        if is_row {
            row_ordinal += 1;
        }
        let Some(rect) = visible_rect(area, *scroll, top, len) else {
            continue;
        };
        if tinted {
            f.buffer_mut()
                .set_style(rect, Style::default().bg(theme.bg_alt));
        }
        if Some(i) == selected {
            // Tint only the background so status and agent colors survive,
            // as the list's highlight does.
            f.buffer_mut().set_style(rect, theme.selected_bg_style());
        }
        let r = match &entries[i] {
            TallEntry::Row(r) => r.row,
            TallEntry::Group(_) => {
                targets.groups.push((i, rect));
                continue;
            }
            TallEntry::Section(_) => continue,
        };
        targets.rows.push((r.workspace_id, rect));
        // The PR chip is clickable only while its line is on screen.
        if let Some((line, dx, w)) = pr_hits[i]
            && top + line >= *scroll
            && top + line < *scroll + height
        {
            let x = area.x.saturating_add(dx);
            if x < area.x.saturating_add(area.width) {
                targets.pr_chips.push((
                    r.workspace_id,
                    Rect {
                        x,
                        y: area.y + (top + line - *scroll) as u16,
                        width: w.min(area.x + area.width - x),
                        height: 1,
                    },
                ));
            }
        }
    }
    targets
}

/// The first visible line after keeping `selected` (a row's `(first line,
/// line count)`) in view: scroll up to its top when it starts above the
/// viewport, down until its bottom shows when it ends below — but never
/// past its top, so a row taller than the viewport shows from its start.
/// Clamped so the viewport never hangs past the last line.
///
/// `follow` is set on the frame the selection changes. Otherwise the
/// offset is the user's (moved by the wheel or PgUp/PgDn) and is only held
/// so some of the selected row stays on screen, which lets a row taller
/// than the viewport be scrolled through instead of snapping to its top.
pub fn scroll_for(
    scroll: usize,
    selected: Option<(usize, usize)>,
    height: usize,
    total: usize,
    follow: bool,
) -> usize {
    let max = total.saturating_sub(height);
    let mut scroll = scroll.min(max);
    if let Some((top, len)) = selected {
        let bottom = top + len;
        if follow {
            if top < scroll {
                scroll = top;
            } else if bottom > scroll + height {
                scroll = (bottom.saturating_sub(height)).min(top);
            }
        } else {
            let lo = (top + 1).saturating_sub(height);
            let hi = bottom.saturating_sub(1);
            scroll = scroll.clamp(lo, hi.max(lo)).min(max);
        }
    }
    scroll
}

/// The on-screen rect of a row spanning lines `top..top + len`, clipped to
/// `area` scrolled to `scroll`, or `None` when none of it shows.
fn visible_rect(area: Rect, scroll: usize, top: usize, len: usize) -> Option<Rect> {
    let start = top.max(scroll);
    let end = (top + len).min(scroll + area.height as usize);
    (start < end).then(|| Rect {
        x: area.x,
        y: area.y + (start - scroll) as u16,
        width: area.width,
        height: (end - start) as u16,
    })
}

fn has_recap(r: &WorkspaceRecap) -> bool {
    [
        &r.goal,
        &r.state,
        &r.next,
        &r.goal_short,
        &r.state_short,
        &r.next_short,
    ]
    .iter()
    .any(|f| f.as_deref().is_some_and(|s| !s.trim().is_empty()))
}

/// The full field when set, else the short form, whitespace-collapsed.
fn field(full: &Option<String>, short: &Option<String>) -> String {
    let pick = full
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(short.as_deref())
        .unwrap_or("");
    pick.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// [`wrap`], keeping at most `max` lines; when text is cut, the last kept
/// line ends with `…` at a word boundary.
pub(crate) fn wrap_capped(text: &str, width: usize, max: usize) -> Vec<String> {
    // `max` lines hold at most `width * max` cells, so a prefix one line
    // longer is enough to fill them and know to mark the cut. Wrapping only
    // that keeps a paragraphs-long prompt from being wrapped in full every
    // frame just to throw most of it away.
    let budget = width.max(1).saturating_mul(max.saturating_add(1));
    let head: std::borrow::Cow<'_, str> = match text.char_indices().nth(budget) {
        Some((cut, _)) => format!("{} …", &text[..cut]).into(),
        None => text.into(),
    };
    let mut lines = wrap(&head, width);
    if lines.len() > max && max > 0 {
        let rest = lines.split_off(max).join(" ");
        let last = lines.pop().unwrap_or_default();
        lines.push(truncate_words(&format!("{last} {rest}"), width.max(1)));
    }
    lines
}

/// Greedy word wrap into lines of at most `width` cells. Never drops text:
/// a word wider than the line is broken across lines.
pub(crate) fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let sep = usize::from(!cur.is_empty());
        if display_width(&cur) + sep + display_width(word) <= width {
            if sep == 1 {
                cur.push(' ');
            }
            cur.push_str(word);
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        // Break an over-long word into line-sized pieces.
        let mut piece = String::new();
        for ch in word.chars() {
            if display_width(&piece) + display_width(ch.encode_utf8(&mut [0; 4])) > width {
                lines.push(std::mem::take(&mut piece));
            }
            piece.push(ch);
        }
        cur = piece;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::session::AgentKind;
    use crate::ui::dashboard::column_content::RowColumn;
    use crate::ui::dashboard::status::Status;

    fn base_row(branch: &str) -> RowInputs {
        RowInputs {
            agent: AgentKind::Claude,
            peers: Vec::new(),
            status: Status::Idle,
            branch: branch.into(),
            pr_number: None,
            procs: 0,
            diff: None,
            column: Some(RowColumn {
                token: "working".into(),
                reported: true,
                body: ColumnBody::Empty,
            }),
            ago_secs: Some(60),
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
            workspace_id: WorkspaceId(1),
            tags: Vec::new(),
        }
    }

    /// One row's lines at `width`, with strip columns sized to it alone.
    fn layout(t: &TallRow<'_>, glyphs: &AgentGlyphs, width: usize) -> Vec<Line<'static>> {
        let cols = strip_columns(std::slice::from_ref(t), ColumnWidths::default(), width);
        row_layout(t, glyphs, cols, 0, &Theme::wsx()).lines
    }

    fn text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn wrap_capped_ellipsizes_past_the_limit() {
        let text = "one two three four five six seven eight nine ten";
        assert_eq!(wrap_capped(text, 100, 2), vec![text], "fits: untouched");
        let capped = wrap_capped(text, 10, 2);
        assert_eq!(capped.len(), 2);
        assert!(capped[1].ends_with('…'), "{capped:?}");
        assert!(capped.iter().all(|l| display_width(l) <= 10), "{capped:?}");
    }

    /// A long last message without a recap stays at two lines; a long
    /// recap goal still wraps in full.
    #[test]
    fn fallback_text_is_capped_but_recap_is_not() {
        let long = "word ".repeat(200);
        let mut row = base_row("b");
        row.column = Some(RowColumn {
            token: "idle".into(),
            reported: false,
            body: ColumnBody::Fallback {
                text: long.clone(),
                emphasis: ColumnEmphasis::Dim,
            },
        });
        let detail = WorkspaceDetail::default();
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let draw = |t: &TallRow<'_>| layout(t, &AgentGlyphs::default(), 120);
        // Strip, two `last` lines, footer.
        assert_eq!(draw(&tall).len(), 4);

        let detail = WorkspaceDetail {
            recap: Some(WorkspaceRecap {
                goal: Some(long),
                ..Default::default()
            }),
            ..Default::default()
        };
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        assert!(draw(&tall).len() > 4, "recap goal not capped");
    }

    #[test]
    fn wrap_keeps_every_word() {
        assert!(wrap("", 10).is_empty());
        assert_eq!(wrap("one two three", 8), vec!["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        let long = "alpha beta gamma delta epsilon zeta eta theta";
        assert_eq!(wrap(long, 11).join(" "), long, "nothing dropped");
    }

    #[test]
    fn row_shows_the_full_recap_and_branch() {
        let row = base_row("bakedbean/a-very-long-branch-name-for-testing");
        let detail = WorkspaceDetail {
            recap: Some(WorkspaceRecap {
                goal: Some(
                    "Single-repo dashboard view with richer per-workspace detail, \
                     including every recap field in full without truncation"
                        .into(),
                ),
                state: Some("mapping dashboard code".into()),
                next: Some("draft design options".into()),
                ..Default::default()
            }),
            git: Some(WorkspaceStatus {
                modified: 3,
                untracked: 1,
                ahead: 0,
                behind: 0,
                upstream: false,
            }),
            ahead_of_base: Some(4),
            base_branch: Some("origin/main".into()),
            first_prompt: None,
        };
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let width = 120;
        let lines: Vec<String> = layout(&tall, &AgentGlyphs::default(), width)
            .iter()
            .map(text)
            .collect();
        let all = lines.join("\n");
        let head = lines[0]
            .split_whitespace()
            .find(|w| w.starts_with("bakedbean"));
        let rest = lines[1].trim_start_matches([' ', '▎']);
        assert_eq!(format!("{}{rest}", head.unwrap()), row.branch, "{all}");
        assert!(lines[2].contains("goal"), "{all}");
        // Drop the gutter bars so wrapped text reads as one run.
        let joined = lines
            .join(" ")
            .replace('▎', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            joined.contains("including every recap field in full without truncation"),
            "goal wraps in full:\n{all}"
        );
        assert!(
            joined.contains("mapping dashboard code → draft design options"),
            "{all}"
        );
        assert!(
            lines[0].contains("↑4 vs main") && lines[0].contains("3 mod 1 untr"),
            "{all}"
        );
        for l in &lines {
            assert!(display_width(l) <= width, "line too wide: {l:?}");
        }
    }

    #[test]
    fn first_line_drops_the_recap_but_keeps_the_token() {
        let mut row = base_row("b");
        row.column = Some(RowColumn {
            token: "working".into(),
            reported: true,
            body: ColumnBody::Recap {
                segments: vec![crate::ui::dashboard::column_content::RecapSegment {
                    text: "SQUEEZED-GOAL".into(),
                    authored: true,
                    full: None,
                }],
            },
        });
        let detail = WorkspaceDetail {
            recap: Some(WorkspaceRecap {
                goal_short: Some("SQUEEZED-GOAL".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let lines = layout(&tall, &AgentGlyphs::default(), 120);
        assert!(text(&lines[0]).contains("working"));
        assert!(!text(&lines[0]).contains("SQUEEZED-GOAL"));
        assert!(text(&lines[1]).contains("SQUEEZED-GOAL"));
    }

    /// Agents are named by the theme's per-kind icon, falling back to the
    /// bar `symbol`; a blanked icon leaves the label alone.
    #[test]
    fn agents_take_their_theme_icons() {
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[agent_bar.symbols]\nclaude = \"C\"\ncodex = \"\"\n",
            )
            .unwrap(),
            &Theme::wsx(),
        )
        .unwrap();
        let glyphs = AgentGlyphs::from_specs(&specs);
        assert_eq!(glyphs.glyph(AgentKind::Claude), Some("C"));
        assert_eq!(glyphs.glyph(AgentKind::Codex), None, "blanked");
        assert_eq!(glyphs.glyph(AgentKind::Pi), Some("▎"), "falls back");

        let mut row = base_row("b");
        row.peers = vec![
            crate::ui::dashboard::row::PeerIndicator {
                agent: AgentKind::Codex,
                active: false,
            },
            crate::ui::dashboard::row::PeerIndicator {
                agent: AgentKind::Pi,
                active: true,
            },
        ];
        let detail = WorkspaceDetail::default();
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let lines = layout(&tall, &glyphs, 120);
        let footer = text(lines.last().unwrap());
        assert!(footer.contains("C claude  codex  ▎ pi"), "{footer:?}");
    }

    #[test]
    fn scroll_keeps_the_selected_row_in_view() {
        // Rows at lines 0..3, 4..7, 8..11; viewport 6 lines; 11 total.
        assert_eq!(scroll_for(0, Some((0, 3)), 6, 11, true), 0);
        assert_eq!(
            scroll_for(0, Some((4, 3)), 6, 11, true),
            1,
            "bottom just shows"
        );
        assert_eq!(
            scroll_for(5, Some((4, 3)), 6, 11, true),
            4,
            "top above: snap to it"
        );
        assert_eq!(scroll_for(9, None, 6, 11, true), 5, "never past the end");
        assert_eq!(scroll_for(0, None, 20, 11, true), 0, "short content");
    }

    /// A row taller than the viewport shows from its first line.
    #[test]
    fn scroll_shows_the_top_of_an_oversized_row() {
        assert_eq!(scroll_for(0, Some((4, 12)), 6, 20, true), 4);
        assert_eq!(scroll_for(10, Some((4, 12)), 6, 20, true), 4);
    }

    #[test]
    fn visible_rect_clips_to_the_viewport() {
        let area = Rect::new(0, 10, 80, 6);
        assert_eq!(visible_rect(area, 0, 0, 3), Some(Rect::new(0, 10, 80, 3)));
        assert_eq!(
            visible_rect(area, 2, 0, 3),
            Some(Rect::new(0, 10, 80, 1)),
            "top clipped"
        );
        assert_eq!(
            visible_rect(area, 0, 4, 5),
            Some(Rect::new(0, 14, 80, 2)),
            "bottom clipped"
        );
        assert_eq!(visible_rect(area, 0, 6, 3), None, "below");
        assert_eq!(visible_rect(area, 4, 0, 3), None, "above");
    }

    /// A narrow terminal, where long strips break in two.
    const NARROW: u16 = 80;

    fn render_rows(
        rows: &[TallRow<'_>],
        selected: Option<usize>,
        h: u16,
    ) -> (String, TallTargets, usize) {
        let entries: Vec<TallEntry<'_>> = rows.iter().cloned().map(TallEntry::Row).collect();
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(NARROW, h)).unwrap();
        let mut scroll = 0;
        let mut targets = TallTargets::default();
        term.draw(|f| {
            targets = render(
                f,
                f.area(),
                &entries,
                &AgentGlyphs::default(),
                selected,
                &mut scroll,
                true,
                ColumnWidths::default(),
                0,
                &Theme::wsx(),
            );
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        let text = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, targets, scroll)
    }

    /// A selected row taller than the viewport still paints, from its top,
    /// and its click target covers exactly the viewport.
    #[test]
    fn an_oversized_selected_row_paints_its_top() {
        let row = base_row("bakedbean/huge");
        let detail = WorkspaceDetail {
            recap: Some(WorkspaceRecap {
                goal: Some("word ".repeat(300)),
                ..Default::default()
            }),
            ..Default::default()
        };
        let rows = [TallRow {
            row: &row,
            detail: &detail,
        }];
        let (text, targets, scroll) = render_rows(&rows, Some(0), 6);
        assert_eq!(scroll, 0);
        assert!(text.contains("bakedbean/huge"), "head painted:\n{text}");
        assert!(text.contains("goal"), "{text}");
        assert_eq!(
            targets.rows,
            vec![(row.workspace_id, Rect::new(0, 0, NARROW, 6))]
        );
    }

    /// A partly visible last row's target covers just its painted lines,
    /// and nothing claims the blank screen below the content.
    #[test]
    fn click_targets_stop_at_the_painted_lines() {
        let mut a = base_row("first");
        a.workspace_id = WorkspaceId(1);
        let mut b = base_row("second");
        b.workspace_id = WorkspaceId(2);
        b.lifecycle = Some(crate::git::forge::BranchLifecycle::PrOpen);
        b.pr_number = Some(7);
        let detail = WorkspaceDetail::default();
        let rows = [
            TallRow {
                row: &a,
                detail: &detail,
            },
            TallRow {
                row: &b,
                detail: &detail,
            },
        ];
        // Each row is a strip and a footer, with a spacer between: 5 lines.
        let (text, targets, _) = render_rows(&rows, Some(0), 4);
        assert!(text.contains("second"), "second head paints:\n{text}");
        assert_eq!(targets.rows[1].1, Rect::new(0, 3, NARROW, 1));
        assert_eq!(targets.pr_chips.len(), 1, "head on screen: chip live");

        let (_, targets, _) = render_rows(&rows, Some(0), 10);
        let last = targets.rows.last().unwrap().1;
        assert_eq!(last.y + last.height, 5, "nothing below the content");
    }

    /// Group lines sit between rows, take the selection tint, and report
    /// their own click targets by entry index.
    #[test]
    fn group_entries_paint_and_report_targets() {
        let a = base_row("first");
        let detail = WorkspaceDetail::default();
        let entries = [
            TallEntry::Row(TallRow {
                row: &a,
                detail: &detail,
            }),
            TallEntry::Group(Line::from("  ▸ #parked  ·  2 hidden")),
        ];
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(NARROW, 10)).unwrap();
        let mut scroll = 0;
        let mut targets = TallTargets::default();
        term.draw(|f| {
            targets = render(
                f,
                f.area(),
                &entries,
                &AgentGlyphs::default(),
                Some(1),
                &mut scroll,
                true,
                ColumnWidths::default(),
                0,
                &Theme::wsx(),
            );
        })
        .unwrap();
        // Row: strip + footer (lines 0-1), spacer, group on line 3.
        assert_eq!(targets.rows.len(), 1);
        assert_eq!(targets.groups, vec![(1, Rect::new(0, 3, NARROW, 1))]);
        let buf = term.backend().buffer();
        assert_eq!(buf[(5, 3)].bg, Theme::wsx().selected_bg, "group tinted");
    }

    /// Without a recap, the fallback text shows once: under a `last` label,
    /// not also squeezed onto the first line beside the status word.
    #[test]
    fn fallback_text_shows_once_under_last() {
        let mut row = base_row("b");
        row.column = Some(RowColumn {
            token: "idle".into(),
            reported: false,
            body: ColumnBody::Fallback {
                text: "UNIQUE-LAST-MESSAGE".into(),
                emphasis: ColumnEmphasis::Dim,
            },
        });
        let detail = WorkspaceDetail::default();
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let lines: Vec<String> = layout(&tall, &AgentGlyphs::default(), 120)
            .iter()
            .map(text)
            .collect();
        assert!(lines[0].contains("idle"), "token kept: {:?}", lines[0]);
        assert!(!lines[0].contains("UNIQUE-LAST-MESSAGE"), "{:?}", lines[0]);
        assert!(lines[1].contains("last") && lines[1].contains("UNIQUE-LAST-MESSAGE"));
        assert!(!lines[1].contains("now"), "{:?}", lines[1]);
        assert_eq!(
            lines
                .iter()
                .filter(|l| l.contains("UNIQUE-LAST-MESSAGE"))
                .count(),
            1
        );
    }

    /// The initial prompt shows first, labelled, italic, and capped at two
    /// lines however long the brief was; it shows alongside a recap too.
    #[test]
    fn initial_prompt_leads_the_detail_lines() {
        let row = base_row("b");
        let detail = WorkspaceDetail {
            first_prompt: Some(format!("FIRST-PROMPT {}", "brief ".repeat(200))),
            recap: Some(WorkspaceRecap {
                goal: Some("the goal".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let lines = layout(&tall, &AgentGlyphs::default(), 120);
        let texts: Vec<String> = lines.iter().map(text).collect();
        assert!(
            texts[1].contains("prompt") && texts[1].contains("FIRST-PROMPT"),
            "{texts:?}"
        );
        assert!(
            texts[2].ends_with('…') || texts[2].trim_end().ends_with('…'),
            "capped: {texts:?}"
        );
        assert!(texts[3].contains("goal"), "recap follows: {texts:?}");
        let prompt_span = lines[1].spans.last().unwrap();
        assert!(
            prompt_span
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );

        // No prompt known yet: no line.
        let detail = WorkspaceDetail::default();
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let texts: Vec<String> = layout(&tall, &AgentGlyphs::default(), 120)
            .iter()
            .map(text)
            .collect();
        assert!(!texts.iter().any(|l| l.contains("prompt")), "{texts:?}");
    }

    /// Without `follow`, a scrolled offset is kept as long as some of the
    /// selected row shows, so an oversized row can be read to its end.
    #[test]
    fn manual_scroll_reads_through_an_oversized_row() {
        // One 30-line row in a 6-line viewport.
        assert_eq!(scroll_for(6, Some((0, 30)), 6, 30, true), 0, "follow snaps");
        assert_eq!(scroll_for(6, Some((0, 30)), 6, 30, false), 6, "held");
        assert_eq!(scroll_for(24, Some((0, 30)), 6, 30, false), 24, "its end");
        assert_eq!(scroll_for(99, Some((0, 30)), 6, 30, false), 24, "clamped");
        // A short row can't be scrolled entirely off screen.
        assert_eq!(scroll_for(20, Some((4, 3)), 6, 30, false), 6);
        assert_eq!(scroll_for(0, Some((20, 3)), 6, 30, false), 15);
    }

    /// An idle workspace's fallback is its initial prompt; it shows once,
    /// under `prompt`, not again under `last`.
    #[test]
    fn initial_prompt_is_not_repeated_as_last() {
        let mut row = base_row("b");
        row.column = Some(RowColumn {
            token: "idle".into(),
            reported: false,
            body: ColumnBody::Fallback {
                text: "fix the   login redirect".into(),
                emphasis: ColumnEmphasis::Dim,
            },
        });
        let detail = WorkspaceDetail {
            first_prompt: Some("fix the login\nredirect".into()),
            ..Default::default()
        };
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let texts: Vec<String> = layout(&tall, &AgentGlyphs::default(), 120)
            .iter()
            .map(text)
            .collect();
        assert!(texts.iter().any(|l| l.contains("prompt")), "{texts:?}");
        assert!(!texts.iter().any(|l| l.contains("last")), "{texts:?}");
    }

    /// End to end with the real synthesizer: an idle workspace without a
    /// recap, whose dashboard fallback is its first prompt, shows that
    /// prompt once.
    #[test]
    fn idle_fallback_from_the_prompt_shows_once() {
        let events = crate::activity::events::WorkspaceEvents {
            first_user_text: Some("UNIQUE-INITIAL-PROMPT for the workspace".into()),
            ..Default::default()
        };
        let mut row = base_row("b");
        row.column = Some(crate::ui::dashboard::column_content::row_column(
            crate::ui::dashboard::status::Status::Idle,
            Some(&events),
            0,
            None,
            None,
        ));
        let detail = WorkspaceDetail {
            first_prompt: events.first_user_text.clone(),
            ..Default::default()
        };
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let all: Vec<String> = layout(&tall, &AgentGlyphs::default(), 120)
            .iter()
            .map(text)
            .collect();
        assert_eq!(
            all.iter()
                .filter(|l| l.contains("UNIQUE-INITIAL-PROMPT"))
                .count(),
            1,
            "{all:?}"
        );
        assert!(all.iter().any(|l| l.contains("prompt")), "{all:?}");
    }

    #[test]
    fn wrap_capped_only_wraps_what_it_keeps() {
        let huge = "word ".repeat(100_000);
        let lines = wrap_capped(&huge, 40, 2);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('…'), "{lines:?}");
    }

    fn split_render(
        entries: &[TallEntry<'_>],
        width: u16,
        h: u16,
    ) -> (Vec<String>, TallTargets, ratatui::buffer::Buffer) {
        let mut term =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, h)).unwrap();
        let mut scroll = 0;
        let mut targets = TallTargets::default();
        term.draw(|f| {
            targets = render(
                f,
                f.area(),
                entries,
                &AgentGlyphs::default(),
                None,
                &mut scroll,
                true,
                ColumnWidths::default(),
                0,
                &Theme::wsx(),
            );
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        let lines = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        (lines, targets, buf)
    }

    /// The PR chip carries the same review mark and unresolved count as
    /// the dashboard list, on wide and narrow terminals alike,
    /// colored by the verdict rather than the lifecycle.
    #[test]
    fn pr_chip_shows_the_review_mark() {
        use crate::git::forge::{BranchLifecycle, ReviewDecision};
        let theme = Theme::wsx();
        for (verdict, glyph) in [
            (ReviewDecision::Approved, "✓"),
            (ReviewDecision::ChangesRequested, "✗ 3"),
            (ReviewDecision::ReviewRequired, "◌ 3"),
        ] {
            let mut row = base_row("bakedbean/review-mark");
            row.lifecycle = Some(BranchLifecycle::PrOpen);
            row.pr_number = Some(462);
            row.review = Some(verdict);
            row.unresolved = (verdict != ReviewDecision::Approved).then_some(3);
            let detail = WorkspaceDetail::default();
            let tall = TallRow {
                row: &row,
                detail: &detail,
            };

            // Wide: the chip sits in its own column beside the branch.
            let entries = [TallEntry::Row(TallRow {
                row: &row,
                detail: &detail,
            })];
            let (lines, _, _) = split_render(&entries, 220, 20);
            assert!(
                lines
                    .iter()
                    .any(|l| l.contains("#462") && l.contains(glyph)),
                "{verdict:?} wide: {lines:?}"
            );
            let (cell, _) = pr_cell(&row, row::MAX_PR_WIDTH, &theme);
            let mark = cell
                .iter()
                .find(|s| s.content.contains(glyph))
                .unwrap_or_else(|| panic!("{verdict:?}: no mark span"));
            assert_eq!(mark.style.fg, theme.review_style(verdict).fg, "{verdict:?}");

            // Narrow: the chip stays on the strip's first line.
            let narrow = layout(&tall, &AgentGlyphs::default(), 80);
            let head = text(&narrow[0]);
            assert!(
                head.contains("#462") && head.contains(glyph),
                "{verdict:?} narrow: {head:?}"
            );
        }
    }

    /// Each row's lines at `width`, laid out with one shared set of
    /// strip columns as `render` does.
    fn strip_rows(rows: &[TallRow<'_>], width: usize) -> Vec<Vec<String>> {
        let cols = strip_columns(rows, ColumnWidths::default(), width);
        rows.iter()
            .map(|r| {
                row_layout(r, &AgentGlyphs::default(), cols, 0, &Theme::wsx())
                    .lines
                    .iter()
                    .map(text)
                    .collect()
            })
            .collect()
    }

    /// Cell column where `needle` starts in `line`.
    fn col(line: &str, needle: &str) -> usize {
        display_width(
            &line[..line
                .find(needle)
                .unwrap_or_else(|| panic!("{needle:?} in {line:?}"))],
        )
    }

    #[test]
    fn strip_fields_line_up_across_rows() {
        use crate::git::forge::BranchLifecycle;
        let mut a = base_row("bakedbean/short");
        a.lifecycle = Some(BranchLifecycle::PrOpen);
        a.pr_number = Some(7);
        a.diff = Some(crate::git::DiffStats {
            added: 10,
            removed: 2,
        });
        let mut b = base_row("bakedbean/a-much-longer-branch");
        b.workspace_id = WorkspaceId(2);
        let none = WorkspaceDetail::default();
        let ahead = WorkspaceDetail {
            ahead_of_base: Some(3),
            base_branch: Some("origin/main".into()),
            ..Default::default()
        };
        let rows = [
            TallRow {
                row: &a,
                detail: &none,
            },
            TallRow {
                row: &b,
                detail: &ahead,
            },
        ];
        let (lines, targets, _) = split_render(&rows.clone().map(TallEntry::Row), 140, 20);
        let firsts: Vec<&String> = targets
            .rows
            .iter()
            .map(|(_, r)| &lines[r.y as usize])
            .collect();
        let chip_x = targets.pr_chips[0].1.x as usize;
        assert_eq!(col(firsts[1], "no PR"), chip_x, "{lines:#?}");
        assert!(col(firsts[0], "#7") > chip_x, "{lines:#?}");
        assert_eq!(
            col(firsts[0], "working"),
            col(firsts[1], "working"),
            "{lines:#?}"
        );
        assert!(firsts[0].contains("+10 −2") && firsts[1].contains("↑3 vs main"));
        let age = |l: &str| display_width(&l[..l.rfind("1m").unwrap()]);
        assert_eq!(age(firsts[0]), age(firsts[1]), "{lines:#?}");
    }

    #[test]
    fn long_branch_wraps_without_moving_the_strip() {
        use crate::git::forge::BranchLifecycle;
        let long = "bakedbean/a-branch-name-much-longer-than-the-branch-column-allows";
        let mut a = base_row(long);
        a.lifecycle = Some(BranchLifecycle::PrOpen);
        a.pr_number = Some(8);
        let mut b = base_row("bakedbean/short");
        b.lifecycle = Some(BranchLifecycle::PrOpen);
        b.pr_number = Some(9);
        let detail = WorkspaceDetail::default();
        let rows = [
            TallRow {
                row: &a,
                detail: &detail,
            },
            TallRow {
                row: &b,
                detail: &detail,
            },
        ];
        let out = strip_rows(&rows, 140);
        assert_eq!(col(&out[0][0], "#8"), col(&out[1][0], "#9"), "{out:#?}");
        assert!(out[0][0].contains("working"), "strip on line 1: {out:#?}");
        let joined: String = out[0][..2]
            .iter()
            .map(|l| {
                l.split_whitespace()
                    .find(|w| w.starts_with("bakedbean") || w.contains("allows"))
                    .unwrap_or("")
            })
            .collect();
        assert_eq!(joined, long, "{out:#?}");
        for l in out.iter().flatten() {
            assert!(display_width(l) <= 140, "too wide: {l:?}");
            assert!(!l.contains('…'), "no ellipsis: {l:?}");
        }
    }

    #[test]
    fn row_has_strip_then_prose_then_footer() {
        let mut row = base_row("bakedbean/zones");
        row.peers = vec![crate::ui::dashboard::row::PeerIndicator {
            agent: AgentKind::Codex,
            active: true,
        }];
        row.tags = vec!["tag".into()];
        let detail = WorkspaceDetail {
            first_prompt: Some("the PROMPT".into()),
            recap: Some(WorkspaceRecap {
                goal: Some("the GOAL".into()),
                state: Some("STATE".into()),
                next: Some("NEXT".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let rows = [TallRow {
            row: &row,
            detail: &detail,
        }];
        let lines = &strip_rows(&rows, 140)[0];
        assert!(lines[0].contains("bakedbean/zones") && lines[0].contains("1m"));
        // After the agent strip and gutter.
        let body = |l: &String| {
            l.chars()
                .skip(2)
                .collect::<String>()
                .trim_start()
                .to_string()
        };
        let labels: Vec<String> = lines[1..4].iter().map(body).collect();
        assert!(labels[0].starts_with("prompt"), "{lines:#?}");
        assert!(labels[1].starts_with("goal"), "{lines:#?}");
        assert!(labels[2].starts_with("now"), "{lines:#?}");
        let x = col(&lines[0], "bakedbean");
        assert_eq!(col(&lines[1], "prompt"), x, "prose under the branch");
        let last = lines.last().unwrap();
        assert_eq!(lines.len(), 5, "{lines:#?}");
        assert!(last.contains("claude") && last.contains("codex") && last.contains("#tag"));
    }

    #[test]
    fn dirty_counts_are_short_and_skip_clean() {
        let row = base_row("b");
        let dirty = WorkspaceDetail {
            git: Some(WorkspaceStatus {
                modified: 3,
                untracked: 1,
                ahead: 0,
                behind: 0,
                upstream: false,
            }),
            ..Default::default()
        };
        let clean = WorkspaceDetail {
            git: Some(WorkspaceStatus::default()),
            ..Default::default()
        };
        let lines = &strip_rows(
            &[TallRow {
                row: &row,
                detail: &dirty,
            }],
            140,
        )[0];
        assert!(lines[0].contains("3 mod 1 untr"), "{lines:#?}");
        let lines = &strip_rows(
            &[TallRow {
                row: &row,
                detail: &clean,
            }],
            140,
        )[0];
        assert!(
            !lines
                .iter()
                .any(|l| l.contains("mod") || l.contains("untr")),
            "{lines:#?}"
        );
    }

    /// The strip says what's not on the remote yet: commits to push or
    /// pull against the upstream, or a branch with work never pushed.
    #[test]
    fn sync_cell_shows_unpushed_behind_and_never_pushed() {
        use crate::git::forge::BranchLifecycle;
        let strip = |row: &RowInputs, git: WorkspaceStatus, ahead_of_base: u32| {
            let detail = WorkspaceDetail {
                git: Some(git),
                ahead_of_base: Some(ahead_of_base),
                ..Default::default()
            };
            strip_rows(
                &[TallRow {
                    row,
                    detail: &detail,
                }],
                140,
            )[0][0]
                .clone()
        };
        let tracked = |ahead, behind| WorkspaceStatus {
            ahead,
            behind,
            upstream: true,
            ..Default::default()
        };
        let row = base_row("b");
        let line = strip(&row, tracked(2, 3), 4);
        assert!(line.contains("⇡2 unpushed ⇣3 behind"), "{line}");
        let line = strip(&row, tracked(0, 0), 4);
        assert!(!line.contains('⇡') && !line.contains("pushed"), "{line}");

        let untracked = WorkspaceStatus::default();
        assert!(strip(&row, untracked, 4).contains("not pushed"));
        assert!(!strip(&row, untracked, 0).contains("not pushed"));
        // A PR means the branch was pushed, even once its upstream is gone.
        let mut merged = base_row("b");
        merged.lifecycle = Some(BranchLifecycle::PrMerged);
        merged.pr_number = Some(7);
        assert!(!strip(&merged, untracked, 4).contains("not pushed"));
    }

    /// A chip wider than the column drops its lifecycle word to fit; the
    /// click target ends where the painted chip does, not at the padding.
    #[test]
    fn pr_target_matches_the_painted_chip() {
        use crate::git::forge::{BranchLifecycle, ReviewDecision};
        let mut row = base_row("b");
        row.lifecycle = Some(BranchLifecycle::PrOpen);
        row.pr_number = Some(1_234_567_890);
        row.review = Some(ReviewDecision::ChangesRequested);
        row.unresolved = Some(12_345);
        let detail = WorkspaceDetail::default();
        let entries = [TallEntry::Row(TallRow {
            row: &row,
            detail: &detail,
        })];
        let (lines, targets, _) = split_render(&entries, 140, 12);
        let (_, r) = targets.pr_chips[0];
        let chars: Vec<char> = lines[r.y as usize].chars().collect();
        let x = r.x as usize;
        let end = x + r.width as usize;
        let chip: String = chars[x..end].iter().collect();
        assert_eq!(chip, "⏺ #1234567890 ✗ 12345", "{lines:?}");
        assert_eq!(chars[end], ' ', "first cell past the target is padding");
    }

    /// Every other workspace takes the alternate background; a group line
    /// between them doesn't flip the pattern.
    #[test]
    fn alternate_rows_are_tinted() {
        let mut a = base_row("a");
        a.workspace_id = WorkspaceId(1);
        let mut b = base_row("b");
        b.workspace_id = WorkspaceId(2);
        let mut c = base_row("c");
        c.workspace_id = WorkspaceId(3);
        let detail = WorkspaceDetail::default();
        let entries = [
            TallEntry::Row(TallRow {
                row: &a,
                detail: &detail,
            }),
            TallEntry::Group(Line::from("  ▸ #parked  ·  1 hidden")),
            TallEntry::Row(TallRow {
                row: &b,
                detail: &detail,
            }),
            TallEntry::Row(TallRow {
                row: &c,
                detail: &detail,
            }),
        ];
        let (_, targets, buf) = split_render(&entries, 160, 40);
        let bg = |id: i64| {
            let r = targets.rows.iter().find(|(w, _)| w.0 == id).unwrap().1;
            buf[(r.x + 100, r.y)].bg
        };
        let alt = Theme::wsx().bg_alt;
        assert_ne!(bg(1), alt, "first row plain");
        assert_eq!(bg(2), alt, "second row tinted despite the group between");
        assert_ne!(bg(3), alt, "third row plain");
    }

    /// A section header restarts the alternate tint: the first row under it
    /// is plain.
    #[test]
    fn section_header_restarts_the_tint() {
        let mk = |id: i64, name: &str| {
            let mut r = base_row(name);
            r.workspace_id = WorkspaceId(id);
            r
        };
        let (a, b, c) = (mk(1, "a"), mk(2, "b"), mk(3, "c"));
        let detail = WorkspaceDetail::default();
        let row = |r| {
            TallEntry::Row(TallRow {
                row: r,
                detail: &detail,
            })
        };
        let entries = [
            TallEntry::Section(Line::from("── Open · 2 ──")),
            row(&a),
            row(&b),
            TallEntry::Section(Line::from("── Done · 1 ──")),
            row(&c),
        ];
        let (_, targets, buf) = split_render(&entries, 160, 40);
        let bg = |id: i64| {
            let r = targets.rows.iter().find(|(w, _)| w.0 == id).unwrap().1;
            buf[(r.x + 100, r.y)].bg
        };
        let alt = Theme::wsx().bg_alt;
        assert_ne!(bg(1), alt, "first row of a section is plain");
        assert_eq!(bg(2), alt);
        assert_ne!(bg(3), alt, "first row after the next header is plain");
        assert!(targets.groups.is_empty(), "headers are not click targets");
    }

    /// A clean working tree adds nothing to the git facts, at any width.
    #[test]
    fn a_clean_tree_is_not_mentioned() {
        let row = base_row("b");
        let detail = WorkspaceDetail {
            git: Some(WorkspaceStatus::default()),
            ahead_of_base: Some(2),
            ..Default::default()
        };
        let entries = [TallEntry::Row(TallRow {
            row: &row,
            detail: &detail,
        })];
        for width in [NARROW, 160] {
            let (lines, _, _) = split_render(&entries, width, 12);
            let all = lines.join("\n");
            assert!(all.contains("↑2 base"), "{all}");
            assert!(!all.contains("clean"), "width {width}:\n{all}");
        }
    }

    /// A row with every strip field long: chip with a review mark, a long
    /// status token with procs and a badge, ahead, dirty and diff.
    fn busy_row(branch: &str) -> RowInputs {
        use crate::git::forge::{BranchLifecycle, ReviewDecision};
        let mut r = base_row(branch);
        r.lifecycle = Some(BranchLifecycle::PrOpen);
        r.pr_number = Some(1234);
        r.review = Some(ReviewDecision::ChangesRequested);
        r.unresolved = Some(3);
        r.column = Some(RowColumn {
            token: "waiting-for-input".into(),
            reported: true,
            body: ColumnBody::Empty,
        });
        r.procs = 3;
        r.badge = Some(row::LifecycleBadge::SetupFailed);
        r.diff = Some(crate::git::DiffStats {
            added: 1234,
            removed: 567,
        });
        r
    }

    fn busy_detail() -> WorkspaceDetail {
        WorkspaceDetail {
            git: Some(WorkspaceStatus {
                modified: 12,
                untracked: 3,
                ahead: 0,
                behind: 0,
                upstream: false,
            }),
            ahead_of_base: Some(3),
            base_branch: Some("origin/some-long-base".into()),
            ..Default::default()
        }
    }

    #[test]
    fn narrow_strip_breaks_in_two() {
        let mut a = busy_row("bakedbean/narrow");
        (a.procs, a.badge) = (0, None);
        let mut b = a.clone();
        b.branch = "bakedbean/other".into();
        b.workspace_id = WorkspaceId(2);
        b.diff = Some(crate::git::DiffStats {
            added: 5,
            removed: 1,
        });
        let detail = WorkspaceDetail {
            base_branch: Some("origin/main".into()),
            ..busy_detail()
        };
        let rows = [
            TallRow {
                row: &a,
                detail: &detail,
            },
            TallRow {
                row: &b,
                detail: &detail,
            },
        ];
        assert!(strip_columns(&rows, ColumnWidths::default(), 80).two_line);
        let out = strip_rows(&rows, 80);
        for r in &out {
            assert!(
                r[0].contains("bakedbean/") && r[0].contains("#1234"),
                "{out:#?}"
            );
            assert!(r[0].trim_end().ends_with("1m"), "age right: {out:#?}");
            assert!(r[1].contains("waiting-for-input"), "{out:#?}");
            assert!(!r[0].contains("waiting"), "{out:#?}");
        }
        assert!(
            out[0][1].contains("+1234 −567") && out[1][1].contains("+5 −1"),
            "{out:#?}"
        );
        assert_eq!(col(&out[0][0], "#1234"), col(&out[1][0], "#1234"));
        for needle in ["waiting", "↑3", "12 mod", "+"] {
            assert_eq!(col(&out[0][1], needle), col(&out[1][1], needle), "{out:#?}");
        }
        assert_eq!(
            col(&out[0][1], "▸"),
            col(&out[0][0], "bakedbean"),
            "under the branch"
        );
        assert_eq!(col(&out[0][0], "1m"), col(&out[1][0], "1m"));
    }

    /// A manual scroll never leaves the selected row fully below the fold,
    /// even when it sits right under a section header.
    #[test]
    fn manual_scroll_keeps_a_line_of_the_row_under_a_header() {
        let row = base_row("b");
        let detail = WorkspaceDetail::default();
        let entries = [
            TallEntry::Section(Line::from("── Open · 1 ──")),
            TallEntry::Row(TallRow {
                row: &row,
                detail: &detail,
            }),
        ];
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 2)).unwrap();
        let mut scroll = 0;
        let mut targets = TallTargets::default();
        term.draw(|f| {
            targets = render(
                f,
                f.area(),
                &entries,
                &AgentGlyphs::default(),
                Some(1),
                &mut scroll,
                false,
                ColumnWidths::default(),
                0,
                &Theme::wsx(),
            );
        })
        .unwrap();
        assert!(scroll >= 1, "scroll {scroll}");
        assert_eq!(targets.rows.len(), 1, "row still clickable on screen");
    }

    #[test]
    fn narrow_strip_breaks_and_pr_target_matches_chip() {
        let row = busy_row("bakedbean/a-rather-long-branch-name-that-wraps");
        let detail = busy_detail();
        let rows = [TallRow {
            row: &row,
            detail: &detail,
        }];
        let (lines, targets, _) = split_render(&rows.clone().map(TallEntry::Row), 80, 20);
        assert!(strip_columns(&rows, ColumnWidths::default(), 80).two_line);
        let (_, r) = targets.pr_chips[0];
        let chars: Vec<char> = lines[r.y as usize].chars().collect();
        let (x, end) = (r.x as usize, (r.x + r.width) as usize);
        let chip: String = chars[x..end].iter().collect();
        assert!(chip.contains("#1234") && chip.contains("✗ 3"), "{chip:?}");
        assert!(!chip.ends_with(' ') && chars[end] == ' ', "{lines:#?}");
        for l in strip_rows(&rows, 80).iter().flatten() {
            assert!(display_width(l) <= 80, "too wide: {l:?}");
        }
    }

    #[test]
    fn wide_strip_stays_on_one_line() {
        let a = busy_row("bakedbean/narrow");
        let detail = busy_detail();
        let rows = [TallRow {
            row: &a,
            detail: &detail,
        }];
        assert!(!strip_columns(&rows, ColumnWidths::default(), 140).two_line);
        let out = strip_rows(&rows, 140);
        assert!(out[0][0].contains("waiting-for-input") && out[0][0].contains("+1234"));
    }

    /// Just past the old split width, long fields break the strip rather
    /// than squeezing the branch and spilling past the edge.
    #[test]
    fn strip_fits_at_96_with_long_fields() {
        let row = busy_row("bakedbean/a-rather-long-branch-name");
        let detail = busy_detail();
        let rows = [TallRow {
            row: &row,
            detail: &detail,
        }];
        let cols = strip_columns(&rows, ColumnWidths::default(), 96);
        assert!(cols.branch >= BRANCH_MIN, "{cols:?}");
        let out = &strip_rows(&rows, 96)[0];
        for l in out {
            assert!(display_width(l) <= 96, "too wide: {l:?}");
        }
        let all = out.join("\n");
        assert!(all.contains("1m") && all.contains("+1234 −567"), "{all}");
    }

    #[test]
    fn a_long_tag_fits_the_footer() {
        let mut row = base_row("b");
        row.tags = vec!["x".repeat(200).into()];
        let detail = WorkspaceDetail::default();
        let rows = [TallRow {
            row: &row,
            detail: &detail,
        }];
        for width in [80, 140] {
            let out = &strip_rows(&rows, width)[0];
            for l in out {
                assert!(display_width(l) <= width, "too wide: {l:?}");
            }
            assert!(out.last().unwrap().contains("#xxx"), "{out:#?}");
        }
    }

    /// A side panel dragged wide leaves the rows 40 cells; a long base
    /// branch must not push any field past the edge.
    #[test]
    fn strip_fits_at_40_with_a_long_base_branch() {
        let row = busy_row("bakedbean/a-rather-long-branch-name");
        let mut detail = busy_detail();
        detail.base_branch = Some(format!("release/{}", "x".repeat(100)));
        let rows = [TallRow {
            row: &row,
            detail: &detail,
        }];
        let out = &strip_rows(&rows, 40)[0];
        for l in out {
            assert!(display_width(l) <= 40, "too wide: {l:?}");
        }
        let all = out.join("\n");
        assert!(
            all.contains("↑3 vs release/x") && all.contains('…'),
            "{all}"
        );
        assert!(all.contains("#1234") && all.contains("1m"), "{all}");
    }

    #[test]
    fn footer_shows_shared_and_layout_badges() {
        let detail = WorkspaceDetail::default();
        let footer = |row: &RowInputs| {
            let rows = [TallRow {
                row,
                detail: &detail,
            }];
            strip_rows(&rows, 120)[0].last().unwrap().clone()
        };
        let mut row = base_row("b");
        row.shared = true;
        row.shared_active = true;
        assert!(footer(&row).contains("◇ shared"), "{}", footer(&row));
        row.shared_active = false;
        assert!(
            footer(&row).contains("◇ shared, no session"),
            "{}",
            footer(&row)
        );
        row.shared = false;
        row.has_multi_pane_layout = true;
        assert!(footer(&row).contains("layout"), "{}", footer(&row));
        row.has_multi_pane_layout = false;
        assert!(!footer(&row).contains("shared") && !footer(&row).contains("layout"));
    }

    /// In a viewport too short for header, gap and the row's first line,
    /// following the selection shows the row, not just its header.
    #[test]
    fn follow_skips_the_header_when_the_row_would_not_show() {
        let row = base_row("b");
        let detail = WorkspaceDetail::default();
        let entries = [
            TallEntry::Section(Line::from("── Open · 1 ──")),
            TallEntry::Row(TallRow {
                row: &row,
                detail: &detail,
            }),
        ];
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 2)).unwrap();
        let mut scroll = 0;
        let mut targets = TallTargets::default();
        term.draw(|f| {
            targets = render(
                f,
                f.area(),
                &entries,
                &AgentGlyphs::default(),
                Some(1),
                &mut scroll,
                true,
                ColumnWidths::default(),
                0,
                &Theme::wsx(),
            );
        })
        .unwrap();
        assert_eq!(scroll, 2, "row's first line at the top");
        assert_eq!(targets.rows.len(), 1);
    }

    #[test]
    fn fit_spans_marks_every_cut() {
        let text =
            |v: Vec<Span<'static>>| v.iter().map(|s| s.content.to_string()).collect::<String>();
        let diff = || vec![Span::raw("+1234"), Span::raw(" "), Span::raw("−567")];
        assert_eq!(text(fit_spans(diff(), 10)), "+1234 −567", "fits: untouched");
        assert_eq!(
            text(fit_spans(diff(), 5)),
            "+123…",
            "cut on a span boundary"
        );
        assert_eq!(text(fit_spans(diff(), 7)), "+1234 …");
        assert_eq!(text(fit_spans(diff(), 3)), "+1…");
    }

    #[test]
    fn ahead_label_names_the_base_branch() {
        assert_eq!(ahead_label(4, Some("origin/main")), "↑4 vs main");
        assert_eq!(ahead_label(1, Some("develop")), "↑1 vs develop");
        assert_eq!(
            ahead_label(2, Some("upstream/trunk")),
            "↑2 vs upstream/trunk"
        );
        assert_eq!(ahead_label(3, None), "↑3 base");
        assert_eq!(ahead_label(3, Some("  ")), "↑3 base");
    }
}
