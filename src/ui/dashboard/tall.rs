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
use ratatui::widgets::{List, ListItem, ListState};

/// What a tall row shows beyond the dashboard row: the full recap fields
/// and the git counts the one-line row has no room for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceDetail {
    pub recap: Option<WorkspaceRecap>,
    /// Working-tree counts against upstream.
    pub git: Option<WorkspaceStatus>,
    /// Commits on the branch that the repo's base branch lacks.
    pub ahead_of_base: Option<u32>,
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
    /// Each visible row's PR chip — a click opens the PR.
    pub pr_chips: Vec<(WorkspaceId, Rect)>,
}

/// Width of the label column on detail lines (`goal  `, `now   `, `git   `).
const LABEL_WIDTH: usize = 6;

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

    // The first line keeps the status token but not the recap: the lines
    // below show the recap in full, and repeating a squeezed copy beside
    // the branch would only add noise.
    let mut head = row.clone();
    if recap.is_some() {
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
            // last message, which can run to paragraphs) gets a little more
            // room than the dashboard row gives it, but stays capped so one
            // chatty workspace can't push its siblings off screen.
            if let Some(ColumnBody::Fallback { text, emphasis }) =
                row.column.as_ref().map(|c| &c.body)
            {
                let style = match emphasis {
                    ColumnEmphasis::Dim => theme.row_dim_style(),
                    ColumnEmphasis::Status => theme.status_style(row.status),
                    ColumnEmphasis::Warn => theme.warn_style(),
                };
                push_wrapped("now", text, style, FALLBACK_MAX_LINES);
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

/// Draw the rows into `area` as a scrolling list, selecting `selected`, and
/// return what a click can land on. A blank line separates rows.
#[allow(clippy::too_many_arguments)]
pub fn render(
    f: &mut Frame,
    area: Rect,
    rows: &[TallRow<'_>],
    glyphs: &AgentGlyphs,
    selected: Option<usize>,
    list_state: &mut ListState,
    widths: ColumnWidths,
    tick: u32,
    theme: &Theme,
) -> TallTargets {
    let width = area.width as usize;
    let widths = ColumnWidths {
        branch: branch_width_for(rows, widths, width),
        ..widths
    };
    // Items alternate row, spacer, row, spacer… so row `i` is item `2i`.
    let mut items: Vec<ListItem<'static>> = Vec::with_capacity(rows.len() * 2);
    let mut heights: Vec<usize> = Vec::with_capacity(rows.len() * 2);
    for r in rows {
        let lines = row_lines(r, glyphs, widths, tick, theme, width);
        heights.push(lines.len());
        items.push(ListItem::new(lines));
        heights.push(1);
        items.push(ListItem::new(""));
    }
    list_state.select(selected.map(|i| i * 2));
    let list = List::new(items).highlight_style(theme.selected_bg_style());
    f.render_stateful_widget(list, area, list_state);

    // Walk the items from the scroll offset to place each visible row.
    let mut targets = TallTargets::default();
    let bottom = area.y.saturating_add(area.height);
    let mut y = area.y;
    for (item, h) in heights.iter().enumerate().skip(list_state.offset()) {
        if y >= bottom {
            break;
        }
        let height = (*h as u16).min(bottom - y);
        if item % 2 == 0 {
            let r = rows[item / 2].row;
            targets.rows.push((
                r.workspace_id,
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height,
                },
            ));
            if let Some((dx, w)) = row::pr_chip_hit_span(r, widths) {
                let x = area.x.saturating_add(dx);
                if x < area.x.saturating_add(area.width) {
                    targets.pr_chips.push((
                        r.workspace_id,
                        Rect {
                            x,
                            y,
                            width: w.min(area.x + area.width - x),
                            height: 1,
                        },
                    ));
                }
            }
        }
        y = y.saturating_add(*h as u16);
    }
    targets
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
    let mut lines = wrap(text, width);
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
}
