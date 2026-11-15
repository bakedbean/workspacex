//! Tall rows for the repo focus view: one repo's workspaces as a list in
//! which each workspace spans several lines.
//!
//! The first line is the ordinary dashboard row, with the branch column
//! widened to fit the longest branch so no name truncates. Under it, the
//! recap goal and state → next wrap in full, and a last line carries the
//! git counts and the agent roster. Nothing on a tall row truncates except
//! the one-line columns the dashboard row already owns.
//!
//! Pure layout and painting. The caller picks the workspaces and their
//! order (see `focused_workspaces` in the parent module) and owns the
//! selection.

use crate::data::store::{WorkspaceId, WorkspaceRecap};
use crate::git::WorkspaceStatus;
use crate::pty::session::AgentKind;
use crate::ui::dashboard::column_content::{ColumnBody, ColumnEmphasis, RowColumn};
use crate::ui::dashboard::row::{self, ColumnWidths, RowInputs};
use crate::ui::text::{display_width, fit_tags, truncate, truncate_words};
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

/// Narrowest the first line's message column may get when the branch
/// column widens, so the status token still shows.
const MIN_MESSAGE_WIDTH: usize = 10;

/// The branch column width for the focus view: wide enough for the longest
/// branch (plus its glyph, tags and a badge) so names never truncate, but
/// never so wide the message column vanishes, and never narrower than the
/// user's configured width.
pub fn branch_width_for(rows: &[TallRow<'_>], widths: ColumnWidths, total_width: usize) -> usize {
    let needed = rows
        .iter()
        .map(|r| {
            let tags: usize = r.row.tags.iter().map(|t| display_width(t) + 2).sum();
            // Branch glyph and its space, the name, tags, and room for a
            // two-cell badge plus a trailing gap.
            2 + display_width(&r.row.branch) + tags + 4
        })
        .max()
        .unwrap_or(0);
    let fixed = widths.agent + row::FIXED_COLUMNS_WIDTH + widths.pr + MIN_MESSAGE_WIDTH;
    let room = total_width.saturating_sub(fixed);
    needed
        .min(row::MAX_BRANCH_WIDTH)
        .min(room)
        .max(widths.branch)
}

/// The lines of one tall row, first line included. `width` is the list's
/// full width.
pub fn row_lines(
    tall: &TallRow<'_>,
    glyphs: &AgentGlyphs,
    widths: ColumnWidths,
    tick: u32,
    theme: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let row = tall.row;
    let detail = tall.detail;
    let recap = detail.recap.as_ref().filter(|r| has_recap(r));

    // The first line keeps the status token but not the recap or fallback
    // text: the lines below show them with more room, and repeating a
    // squeezed copy beside the branch would only add noise.
    let mut head = row.clone();
    let fallback_below = recap.is_none()
        && matches!(
            row.column.as_ref().map(|c| &c.body),
            Some(ColumnBody::Fallback { .. })
        );
    if recap.is_some() || fallback_below {
        head.column = row.column.as_ref().map(|c| RowColumn {
            body: ColumnBody::Empty,
            ..c.clone()
        });
    }
    let mut lines = vec![row::render(&head, widths, tick, theme, width)];

    let indent = widths.agent + row::GUTTER_WIDTH;
    let text_width = width.saturating_sub(indent + 2 + LABEL_WIDTH).max(8);
    let gutter = |label: &str| -> Vec<Span<'static>> {
        vec![
            Span::raw(" ".repeat(widths.agent)),
            Span::styled(
                if row.selected { "▍" } else { "▎" },
                theme.status_style(row.status),
            ),
            Span::raw("  "),
            Span::styled(format!("{label:<LABEL_WIDTH$}"), theme.dim_style()),
        ]
    };
    let mut push_wrapped = |label: &str, text: &str, style: Style, max: usize| {
        for (i, l) in wrap_capped(text, text_width, max).into_iter().enumerate() {
            let mut spans = gutter(if i == 0 { label } else { "" });
            spans.push(Span::styled(l, style));
            lines.push(Line::from(spans));
        }
    };

    // What the workspace was started for, above what it is doing now.
    let prompt = detail
        .first_prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    if let Some(prompt) = prompt {
        push_wrapped(
            "prompt",
            prompt,
            theme
                .row_dim_style()
                .add_modifier(ratatui::style::Modifier::ITALIC),
            PROMPT_MAX_LINES,
        );
    }

    match recap {
        Some(r) => {
            let goal = field(&r.goal, &r.goal_short);
            if !goal.is_empty() {
                push_wrapped("goal", &goal, Style::default(), usize::MAX);
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
                push_wrapped("now", &now, theme.row_dim_style(), usize::MAX);
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
            // fallback *is* the initial prompt; the `prompt` line above
            // already shows it, so it isn't repeated as if the agent said it.
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
                push_wrapped("last", text, style, FALLBACK_MAX_LINES);
            }
        }
    }

    let mut spans = gutter("git");
    spans.extend(git_spans(row, detail, glyphs, theme));
    let used: usize = spans.iter().map(|s| s.width()).sum();
    let mut line = clip_spans(spans, width);
    let tags = fit_tags(&row.tags, width.saturating_sub(used + 3));
    if !tags.is_empty() && used + 3 + display_width(&tags) <= width {
        line.spans.push(Span::styled("   ", theme.dim_style()));
        line.spans.push(Span::styled(tags, theme.row_dim_style()));
    }
    lines.push(line);
    lines
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
            TallEntry::Group(_) => None,
        })
        .collect();
    let widths = ColumnWidths {
        branch: branch_width_for(&rows, widths, width),
        ..widths
    };
    // Every line of every entry, plus each entry's `(first line, line count)`.
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        if i > 0 {
            lines.push(Line::default());
        }
        let entry = match e {
            TallEntry::Row(r) => row_lines(r, glyphs, widths, tick, theme, width),
            TallEntry::Group(line) => vec![line.clone()],
        };
        spans.push((lines.len(), entry.len()));
        lines.extend(entry);
    }
    let height = area.height as usize;
    let sel_span = selected.and_then(|i| spans.get(i).copied());
    *scroll = scroll_for(*scroll, sel_span, height, lines.len(), follow);

    let visible: Vec<Line<'static>> = lines.into_iter().skip(*scroll).take(height).collect();
    f.render_widget(Paragraph::new(visible), area);

    let mut targets = TallTargets::default();
    for (i, &(top, len)) in spans.iter().enumerate() {
        let Some(rect) = visible_rect(area, *scroll, top, len) else {
            continue;
        };
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
        };
        targets.rows.push((r.workspace_id, rect));
        // The PR chip sits on the row's first line; it is clickable only
        // when that line is on screen.
        if top >= *scroll
            && let Some((dx, w)) = row::pr_chip_hit_span(r, widths)
        {
            let x = area.x.saturating_add(dx);
            if x < area.x.saturating_add(area.width) {
                targets.pr_chips.push((
                    r.workspace_id,
                    Rect {
                        x,
                        y: rect.y,
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

/// `↑4 base · 3 modified · +184 −22 ·  claude  codex`. Each part is left
/// out when its source is unknown, so a fresh workspace shows only what has
/// loaded. The agents always show, primary first, each led by its icon.
fn git_spans(
    row: &RowInputs,
    detail: &WorkspaceDetail,
    glyphs: &AgentGlyphs,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let sep = || Span::styled(" · ", theme.dim_style());
    let mut spans: Vec<Span<'static>> = Vec::new();
    if let Some(n) = detail.ahead_of_base {
        spans.push(Span::styled(format!("↑{n} base"), theme.row_dim_style()));
    }
    if let Some(g) = detail.git {
        if !spans.is_empty() {
            spans.push(sep());
        }
        let text = if g.modified == 0 && g.untracked == 0 {
            "clean".to_string()
        } else {
            let mut parts = Vec::new();
            if g.modified > 0 {
                parts.push(format!("{} modified", g.modified));
            }
            if g.untracked > 0 {
                parts.push(format!("{} untracked", g.untracked));
            }
            parts.join(" ")
        };
        spans.push(Span::styled(text, theme.row_dim_style()));
    }
    if let Some(d) = row.diff.filter(|d| d.added > 0 || d.removed > 0) {
        if !spans.is_empty() {
            spans.push(sep());
        }
        spans.push(Span::styled(format!("+{}", d.added), theme.ok_style()));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(format!("−{}", d.removed), theme.err_style()));
    }
    if !spans.is_empty() {
        spans.push(sep());
    }
    // The primary, then peers in roster order; an active peer's label takes
    // its agent color so live work stands out.
    let agents =
        std::iter::once((row.agent, false)).chain(row.peers.iter().map(|p| (p.agent, p.active)));
    for (i, (kind, active)) in agents.enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        if let Some(g) = glyphs.glyph(kind) {
            spans.push(Span::styled(format!("{g} "), theme.agent_style(kind)));
        }
        let label_style = if active {
            theme.agent_style(kind)
        } else {
            theme.row_dim_style()
        };
        spans.push(Span::styled(kind.display_name().to_string(), label_style));
    }
    spans
}

/// Drop whole spans past `width`, truncating the one that straddles it.
fn clip_spans(spans: Vec<Span<'static>>, width: usize) -> Line<'static> {
    let mut used = 0;
    let mut out = Vec::new();
    for s in spans {
        let w = s.width();
        if used + w <= width {
            used += w;
            out.push(s);
        } else {
            let room = width.saturating_sub(used);
            if room > 0 {
                out.push(Span::styled(truncate(&s.content, room), s.style));
            }
            break;
        }
    }
    Line::from(out)
}

/// [`wrap`], keeping at most `max` lines; when text is cut, the last kept
/// line ends with `…` at a word boundary.
fn wrap_capped(text: &str, width: usize, max: usize) -> Vec<String> {
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
fn wrap(text: &str, width: usize) -> Vec<String> {
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
        let draw = |t: &TallRow<'_>| {
            row_lines(
                t,
                &AgentGlyphs::default(),
                ColumnWidths::default(),
                0,
                &Theme::wsx(),
                120,
            )
        };
        // Head, two fallback lines, git.
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
    fn branch_column_grows_to_fit_the_longest_branch() {
        let long = base_row("bakedbean/a-very-long-branch-name-for-testing");
        let detail = WorkspaceDetail::default();
        let rows = [TallRow {
            row: &long,
            detail: &detail,
        }];
        let widths = ColumnWidths::default();
        let w = branch_width_for(&rows, widths, 200);
        assert!(w >= 2 + long.branch.len(), "fits the name: {w}");
        assert_eq!(
            branch_width_for(&rows, widths, 60),
            widths.branch,
            "never below the configured width on a narrow screen"
        );
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
            }),
            ahead_of_base: Some(4),
            first_prompt: None,
        };
        let tall = TallRow {
            row: &row,
            detail: &detail,
        };
        let rows = [tall.clone()];
        let width = 120;
        let widths = ColumnWidths {
            branch: branch_width_for(&rows, ColumnWidths::default(), width),
            ..ColumnWidths::default()
        };
        let lines: Vec<String> = row_lines(
            &tall,
            &AgentGlyphs::default(),
            widths,
            0,
            &Theme::wsx(),
            width,
        )
        .iter()
        .map(text)
        .collect();
        let all = lines.join("\n");
        assert!(lines[0].contains(&row.branch), "branch whole:\n{all}");
        assert!(lines[1].contains("goal"), "{all}");
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
            lines
                .last()
                .unwrap()
                .contains("↑4 base · 3 modified 1 untracked"),
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
        let lines = row_lines(
            &tall,
            &AgentGlyphs::default(),
            ColumnWidths::default(),
            0,
            &Theme::wsx(),
            120,
        );
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
        let lines = row_lines(
            &tall,
            &glyphs,
            ColumnWidths::default(),
            0,
            &Theme::wsx(),
            120,
        );
        let git = text(lines.last().unwrap());
        assert!(git.contains("C claude  codex  ▎ pi"), "{git:?}");
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

    fn render_rows(
        rows: &[TallRow<'_>],
        selected: Option<usize>,
        h: u16,
    ) -> (String, TallTargets, usize) {
        let entries: Vec<TallEntry<'_>> = rows.iter().cloned().map(TallEntry::Row).collect();
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, h)).unwrap();
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
            vec![(row.workspace_id, Rect::new(0, 0, 120, 6))]
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
        // Each row is a head and a git line, with a spacer between: 5 lines.
        let (text, targets, _) = render_rows(&rows, Some(0), 4);
        assert!(text.contains("second"), "second head paints:\n{text}");
        assert_eq!(targets.rows[1].1, Rect::new(0, 3, 120, 1));
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
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 10)).unwrap();
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
        // Row: head + git (lines 0-1), spacer, group on line 3.
        assert_eq!(targets.rows.len(), 1);
        assert_eq!(targets.groups, vec![(1, Rect::new(0, 3, 120, 1))]);
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
        let lines: Vec<String> = row_lines(
            &tall,
            &AgentGlyphs::default(),
            ColumnWidths::default(),
            0,
            &Theme::wsx(),
            120,
        )
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
        let lines = row_lines(
            &tall,
            &AgentGlyphs::default(),
            ColumnWidths::default(),
            0,
            &Theme::wsx(),
            120,
        );
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
        let texts: Vec<String> = row_lines(
            &tall,
            &AgentGlyphs::default(),
            ColumnWidths::default(),
            0,
            &Theme::wsx(),
            120,
        )
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
        let texts: Vec<String> = row_lines(
            &tall,
            &AgentGlyphs::default(),
            ColumnWidths::default(),
            0,
            &Theme::wsx(),
            120,
        )
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
        let all: Vec<String> = row_lines(
            &tall,
            &AgentGlyphs::default(),
            ColumnWidths::default(),
            0,
            &Theme::wsx(),
            120,
        )
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
}
