//! The side panel: the selected workspace's first prompt, recap goal, next
//! step, agent's latest message, commits or changed files, one at a time
//! under a tab bar.
//!
//! ```text
//! │ Prompt  Goal  Next  Commits 4   Files 7  Chat
//! │ bakedbean/repo-focus-view vs main
//! │ ─────────────────────────────────
//! │ a1b2c3d  feat(dashboard): split focus view
//! │          rows into facts and prose
//! │ 9f8e7d6  fix(dashboard): drop "clean"
//! ```
//!
//! Shared by every surface that offers it — the dashboard list, the repo
//! focus view and the attached view. Each surface keeps its own
//! [`SidePanelState`] and hands [`SidePanelState::carve`] the selected
//! workspace's [`PanelData`]; the panel takes the right of the area it is
//! given and returns what is left.
//!
//! The repo focus view's panel starts open, on the Next tab. Text tabs are
//! never cut short — the body scrolls.
//!
//! While the panel holds keyboard focus (`→` on the dashboard, `Ctrl-x f`
//! attached), `j` / `k` scroll its body. The Files tab has a cursor
//! instead, which they move; Enter, or a click on a file, opens the file's
//! diff, and `o` opens the file in the editor.

use crate::data::store::WorkspaceId;
use crate::git::{BranchCommit, ChangedFile};
use crate::ui::dashboard::tall::wrap;
use crate::ui::dashboard::tall::wrap_capped;
use crate::ui::text::display_width;
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

/// What the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelTab {
    /// The first prompt the workspace's agent was given.
    Prompt,
    /// The recap goal.
    Goal,
    /// The recap's next step and state, or the agent's last message.
    Next,
    #[default]
    Commits,
    Files,
    /// The agent's latest message, as the detail bar's RECENT CHAT shows it.
    Chat,
}

impl PanelTab {
    /// Every tab, in tab-bar order; `1`–`6` on the dashboard pick them.
    pub const ALL: [PanelTab; 6] = [
        PanelTab::Prompt,
        PanelTab::Goal,
        PanelTab::Next,
        PanelTab::Commits,
        PanelTab::Files,
        PanelTab::Chat,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelTab::Prompt => "Prompt",
            PanelTab::Goal => "Goal",
            PanelTab::Next => "Next",
            PanelTab::Chat => "Chat",
            PanelTab::Commits => "Commits",
            PanelTab::Files => "Files",
        }
    }

    /// The value the `dashboard_focus_tab` setting stores.
    pub fn key(self) -> &'static str {
        match self {
            PanelTab::Prompt => "prompt",
            PanelTab::Goal => "goal",
            PanelTab::Next => "next",
            PanelTab::Chat => "chat",
            PanelTab::Commits => "commits",
            PanelTab::Files => "files",
        }
    }

    /// Prompt, Goal, Next or Chat: a tab of prose rather than a git list.
    pub fn is_text(self) -> bool {
        matches!(
            self,
            PanelTab::Prompt | PanelTab::Goal | PanelTab::Next | PanelTab::Chat
        )
    }

    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key() == s)
    }

    /// The tab `d` places along the bar, wrapping.
    fn step(self, d: isize) -> Self {
        let n = Self::ALL.len() as isize;
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as isize;
        Self::ALL[(i + d).rem_euclid(n) as usize]
    }
}

/// Narrowest terminal that gets the panel; below it there isn't room for
/// both the rows and a useful panel, so the panel stays hidden.
pub const PANEL_MIN_TOTAL: u16 = 100;

/// The setting holding the dragged width.
pub const WIDTH_SETTING: &str = "side_panel_width";

/// Narrowest a dragged panel gets.
pub const PANEL_MIN_WIDTH: u16 = 24;

/// Columns a dragged panel always leaves the surface's own content.
const REST_MIN_WIDTH: u16 = 40;

/// The panel's width for a `total`-cell body: the user's dragged `chosen`
/// width when set, kept clear of the rows; otherwise about a third, kept
/// within bounds that hold a commit subject or a path.
pub fn panel_width(total: u16, chosen: Option<u16>) -> u16 {
    match chosen {
        Some(w) => w.clamp(
            PANEL_MIN_WIDTH,
            total
                .saturating_sub(REST_MIN_WIDTH + 1)
                .max(PANEL_MIN_WIDTH),
        ),
        None => (total / 3).clamp(36, 64),
    }
}

/// One surface's panel: whether it shows, its tab, and its scroll.
#[derive(Debug, Clone, Default)]
pub struct SidePanelState {
    pub open: bool,
    /// The tab, kept while the panel is closed.
    pub tab: PanelTab,
    /// The body's first visible line, clamped on draw.
    pub scroll: usize,
    /// The Files tab's highlighted file, clamped on draw.
    pub file_cursor: usize,
    /// The panel has keyboard focus; see [`Self::is_focused`].
    pub focused: bool,
    /// The width the user dragged the panel to; `None` sizes it to the
    /// surface. Shared by both surfaces and kept in `side_panel_width`.
    pub width: Option<u16>,
    /// The left edge is being dragged: it draws highlighted.
    pub resizing: bool,
    /// The cursor moved since the last draw, so the draw scrolls it into
    /// view. The wheel scrolls freely otherwise.
    follow: bool,
    /// The workspace and tab the panel last drew, so the scroll resets
    /// when either changes.
    anchor: Option<(Option<WorkspaceId>, PanelTab)>,
    /// The body width of the last draw, for clamping a dragged width.
    body_width: u16,
}

impl SidePanelState {
    /// A panel open on `tab`.
    pub fn open_on(tab: PanelTab) -> Self {
        Self {
            open: true,
            tab,
            ..Default::default()
        }
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open = true;
        }
    }

    /// Close the panel. Focus goes with it, so reopening never brings back
    /// a focus the user didn't ask for.
    pub fn close(&mut self) {
        self.open = false;
        self.focused = false;
        self.resizing = false;
    }

    /// Whether the panel has keyboard focus: only ever an open panel.
    pub fn is_focused(&self) -> bool {
        self.focused && self.open
    }

    /// Give the panel keyboard focus: on its tab when open, else opened
    /// on Files.
    pub fn focus(&mut self) {
        if self.open {
            self.focused = true;
            self.follow = true;
        } else {
            self.focus_files();
        }
    }

    /// Open the panel on Files with keyboard focus in the list.
    pub fn focus_files(&mut self) {
        self.show(PanelTab::Files);
        self.focused = true;
        self.follow = true;
    }

    /// Move the file cursor by `d` files; the draw clamps it to the list.
    pub fn move_cursor(&mut self, d: isize) {
        self.file_cursor = self.file_cursor.saturating_add_signed(d);
        self.follow = true;
    }

    /// Put the file cursor on file `i` and scroll it into view.
    pub fn set_cursor(&mut self, i: usize) {
        self.file_cursor = i;
        self.follow = true;
    }

    /// Open the panel on `tab`; keyboard focus stays where it was.
    pub fn show(&mut self, tab: PanelTab) {
        self.open = true;
        self.tab = tab;
    }

    /// Switch to the next tab, wrapping, opening the panel if closed.
    pub fn next_tab(&mut self) {
        self.show(self.tab.step(1));
    }

    /// Switch to the previous tab, wrapping, opening the panel if closed.
    pub fn prev_tab(&mut self) {
        self.show(self.tab.step(-1));
    }

    /// Move the body by `lines`, up when `up`.
    pub fn scroll_by(&mut self, lines: usize, up: bool) {
        self.scroll = if up {
            self.scroll.saturating_sub(lines)
        } else {
            self.scroll.saturating_add(lines)
        };
    }

    /// Draw the panel on the right of `area` when it is open and `area` is
    /// wide enough, for the workspace in `data` (`None`: none selected).
    /// Returns the area left for the surface's own content — all of `area`
    /// when the panel isn't drawn — and the panel's click targets.
    /// `width` clamped as the last draw's body would draw it.
    pub fn clamp_width(&self, width: u16) -> u16 {
        panel_width(self.body_width, Some(width))
    }

    pub fn carve(
        &mut self,
        f: &mut Frame,
        area: Rect,
        data: Option<&PanelData>,
        theme: &Theme,
    ) -> (Rect, PanelTargets) {
        if !self.open || area.width < PANEL_MIN_TOTAL {
            return (area, PanelTargets::default());
        }
        self.body_width = area.width;
        let w = panel_width(area.width, self.width);
        let panel_area = Rect {
            x: area.x + area.width - w,
            width: w,
            ..area
        };
        let rest = Rect {
            width: area.width - w - 1,
            ..area
        };
        let anchor = (data.map(|d| d.workspace_id), self.tab);
        if self.anchor != Some(anchor) {
            self.scroll = 0;
            // A new workspace starts its files at the top; switching tabs
            // keeps the place.
            if self.anchor.map(|a| a.0) != Some(anchor.0) {
                self.file_cursor = 0;
            }
            self.anchor = Some(anchor);
        }
        let mut view = PanelView {
            scroll: self.scroll,
            cursor: self.file_cursor,
            focused: self.is_focused(),
            follow: std::mem::take(&mut self.follow),
            resizing: self.resizing,
        };
        let targets = render(
            f,
            panel_area,
            self.tab,
            data.map(PanelData::inputs),
            &mut view,
            theme,
        );
        (self.scroll, self.file_cursor) = (view.scroll, view.cursor);
        (rest, targets)
    }
}

/// The panel's data for one workspace, owned so a surface can gather it
/// before borrowing its own state mutably.
#[derive(Debug, Clone)]
pub struct PanelData {
    pub workspace_id: WorkspaceId,
    pub branch: String,
    pub base: Option<String>,
    /// The branch's commits and changed files (sorted by path), polled
    /// against `base`; `None` until then.
    pub lists: Option<std::sync::Arc<crate::git::BranchLists>>,
    pub ahead: Option<u32>,
    pub prose: PanelProse,
}

/// What the text tabs show, in full.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelProse {
    /// The first prompt the agent was given; `None` until its session log
    /// has been read.
    pub prompt: Option<String>,
    pub goal: Option<String>,
    pub next: Option<String>,
    pub state: Option<String>,
    /// How old the recap is (`5h`) when the workspace kept working well
    /// after it was set, so it may be out of date.
    pub stale: Option<String>,
    /// Without a recap, what the agent last said.
    pub last: Option<String>,
    /// The agent's latest message, recap or not; markdown.
    pub chat: Option<String>,
}

impl PanelData {
    pub fn inputs(&self) -> PanelInputs<'_> {
        PanelInputs {
            branch: &self.branch,
            base: self.base.as_deref(),
            commits: self.lists.as_deref().map(|l| l.commits.as_slice()),
            files: self.lists.as_deref().map(|l| l.files.as_slice()),
            ahead: self.ahead,
            prose: Some(&self.prose),
        }
    }
}

/// What the panel shows for the selected workspace.
#[derive(Debug, Clone, Copy)]
pub struct PanelInputs<'a> {
    pub branch: &'a str,
    /// The repo's base branch; without one there is nothing to list
    /// against.
    pub base: Option<&'a str>,
    /// `None` until the first poll has run.
    pub commits: Option<&'a [BranchCommit]>,
    /// Sorted by path, binary files included; `None` until the first poll
    /// has run.
    pub files: Option<&'a [ChangedFile]>,
    /// How many commits the branch has ahead of its base. The commit list
    /// stops at `BRANCH_COMMITS_LIMIT`, so this is the true total when it
    /// is larger.
    pub ahead: Option<u32>,
    /// The text tabs' content; `None` reads as nothing known yet.
    pub prose: Option<&'a PanelProse>,
}

/// How the panel is scrolled and where its file cursor is, for one draw.
#[derive(Debug, Clone, Copy, Default)]
pub struct PanelView {
    /// The body's first visible line, clamped by the draw.
    pub scroll: usize,
    /// The Files tab's highlighted file, clamped by the draw.
    pub cursor: usize,
    /// The panel has keyboard focus: the cursor shows.
    pub focused: bool,
    /// Scroll the cursor into view.
    pub follow: bool,
    /// The left edge is being dragged.
    pub resizing: bool,
}

/// Click targets in the panel, in screen coordinates.
#[derive(Debug, Default)]
pub struct PanelTargets {
    pub tabs: Vec<(PanelTab, Rect)>,
    /// Each visible row of a file in the Files tab, by the file's index.
    pub files: Vec<(usize, Rect)>,
    /// The whole panel, for routing the mouse wheel to it.
    pub area: Option<Rect>,
}

/// Draw the panel into `area`. `selected` is `None` when no workspace is
/// selected. `view`'s scroll and cursor are clamped here.
pub fn render(
    f: &mut Frame,
    area: Rect,
    tab: PanelTab,
    selected: Option<PanelInputs<'_>>,
    view: &mut PanelView,
    theme: &Theme,
) -> PanelTargets {
    if area.width < 4 || area.height == 0 {
        return PanelTargets::default();
    }
    // A rule down the left edge separates the panel from the rows; drag
    // it to resize the panel.
    let edge_style = if view.resizing {
        theme.selected_style()
    } else {
        theme.dim_style()
    };
    let edge: Vec<Line<'static>> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", edge_style)))
        .collect();
    f.render_widget(Paragraph::new(edge), Rect { width: 1, ..area });
    let inner = Rect {
        x: area.x + 2,
        width: area.width - 2,
        ..area
    };
    let width = inner.width as usize;

    // Tab bar: each tab with its count, the active one highlighted.
    let mut targets = PanelTargets {
        area: Some(area),
        ..Default::default()
    };
    // Tabs that don't fit on the bar's line wrap onto another.
    let mut bars: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut x = inner.x;
    let mut y = inner.y;
    for t in PanelTab::ALL {
        let count = selected.and_then(|s| match t {
            // The true total, not just the listed (capped) commits.
            PanelTab::Commits => s
                .commits
                .map(|c| c.len().max(s.ahead.map_or(0, |a| a as usize))),
            PanelTab::Files => s.files.map(<[_]>::len),
            PanelTab::Prompt | PanelTab::Goal | PanelTab::Next | PanelTab::Chat => None,
        });
        let text = match count {
            Some(n) => format!(" {} {n} ", t.label()),
            None => format!(" {} ", t.label()),
        };
        let w = display_width(&text) as u16;
        let style = if t == tab {
            theme.selected_style()
        } else {
            theme.dim_style()
        };
        if x > inner.x && x + w > inner.x + inner.width && y + 1 < inner.bottom() {
            bars.push(Vec::new());
            x = inner.x;
            y += 1;
        }
        if x + w <= inner.x + inner.width {
            targets.tabs.push((
                t,
                Rect {
                    x,
                    y,
                    width: w,
                    height: 1,
                },
            ));
        }
        // Each tab's own padding spaces it from the next.
        bars.last_mut()
            .expect("one bar at least")
            .push(Span::styled(text, style));
        x += w;
    }

    let mut head: Vec<Line<'static>> = bars.into_iter().map(Line::from).collect();
    if let Some(s) = selected {
        let title = match s.base {
            Some(b) => format!("{} vs {}", s.branch, b.strip_prefix("origin/").unwrap_or(b)),
            None => s.branch.to_string(),
        };
        // At most two lines, and never so many that the tab bar, the rule
        // and at least one line of the list don't fit: a long branch name
        // must not push the list it introduces off the bottom.
        let room = (inner.height as usize)
            .saturating_sub(head.len() + 2)
            .min(2);
        if room > 0 {
            for l in wrap_capped(&title, width, room) {
                head.push(Line::from(Span::styled(l, theme.row_dim_style())));
            }
        }
    }
    head.push(Line::from(Span::styled(
        "─".repeat(width),
        theme.dim_style(),
    )));

    let files = match (tab, selected.and_then(|s| s.base.and(s.files))) {
        (PanelTab::Files, Some(files)) if !files.is_empty() => Some(files),
        _ => None,
    };
    // Clamped only on Files, so another tab keeps the place in the list.
    if tab == PanelTab::Files {
        view.cursor = view.cursor.min(files.map_or(0, |f| f.len() - 1));
    }
    // While focused, the bottom row says what the keys do.
    let hint = (view.focused && inner.height as usize > head.len() + 1).then(|| {
        if files.is_some() {
            crate::ui::drawer::hint_line(&[("↵", "diff"), ("o", "edit"), ("esc", "back")], theme)
        } else {
            crate::ui::drawer::hint_line(&[("j/k", "scroll"), ("esc", "back")], theme)
        }
    });
    let (body, spans) = match files {
        Some(files) => file_lines(files, width, view.focused.then_some(view.cursor), theme),
        None => (body_lines(tab, selected, width, theme), Vec::new()),
    };
    let body_h = (inner.height as usize)
        .saturating_sub(head.len())
        .saturating_sub(usize::from(hint.is_some()));
    if view.follow
        && let Some(&(start, len)) = spans.get(view.cursor)
    {
        if start < view.scroll {
            view.scroll = start;
        } else if start + len > view.scroll + body_h {
            view.scroll = (start + len).saturating_sub(body_h);
        }
    }
    view.scroll = view.scroll.min(body.len().saturating_sub(body_h));
    let body_y = inner.y + head.len() as u16;
    for (i, &(start, len)) in spans.iter().enumerate() {
        for line in start..start + len {
            if line >= view.scroll && line < view.scroll + body_h {
                let rect = Rect {
                    y: body_y + (line - view.scroll) as u16,
                    height: 1,
                    ..inner
                };
                targets.files.push((i, rect));
            }
        }
    }
    let mut lines = head;
    lines.extend(body.into_iter().skip(view.scroll).take(body_h));
    f.render_widget(Paragraph::new(lines), inner);
    if let Some(hint) = hint {
        let row = Rect {
            y: inner.bottom() - 1,
            height: 1,
            ..inner
        };
        f.render_widget(Paragraph::new(hint), row);
    }
    targets
}

/// The body for `tab`, or the empty state that explains why there's none.
fn body_lines(
    tab: PanelTab,
    selected: Option<PanelInputs<'_>>,
    width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let note = |t: &str| {
        wrap(t, width)
            .into_iter()
            .map(|l| Line::from(Span::styled(l, theme.dim_style())))
            .collect()
    };
    let Some(s) = selected else {
        return note("Select a workspace to see what it is doing.");
    };
    if let Some(lines) = prose_lines(tab, s.prose, width, theme) {
        return lines;
    }
    let Some(base) = s.base else {
        return note(
            "Set a base branch in repo settings (s) to list this branch's commits and files.",
        );
    };
    let base = base.strip_prefix("origin/").unwrap_or(base);
    match tab {
        PanelTab::Commits => match s.commits {
            None => note("loading…"),
            Some([]) => note(&format!("No commits ahead of {base}.")),
            Some(commits) => {
                let mut lines = commit_lines(commits, width, theme);
                let hidden = s
                    .ahead
                    .map_or(0, |a| (a as usize).saturating_sub(commits.len()));
                if hidden > 0 {
                    lines.push(Line::default());
                    lines.extend(note(&format!(
                        "{hidden} older commit{} not listed.",
                        if hidden == 1 { "" } else { "s" }
                    )));
                }
                lines
            }
        },
        PanelTab::Files => match s.files {
            None => note("loading…"),
            Some([]) => note(&format!("No files changed against {base}.")),
            Some(files) => file_lines(files, width, None, theme).0,
        },
        PanelTab::Prompt | PanelTab::Goal | PanelTab::Next | PanelTab::Chat => {
            unreachable!("prose_lines draws the text tabs")
        }
    }
}

/// The body of a text tab, in full; `None` for the list tabs.
fn prose_lines(
    tab: PanelTab,
    prose: Option<&PanelProse>,
    width: usize,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    let empty = PanelProse::default();
    let p = prose.unwrap_or(&empty);
    fn text(t: &Option<String>) -> Option<&str> {
        t.as_deref().map(str::trim).filter(|t| !t.is_empty())
    }
    let note = |t: &str| paragraphs(t, width, theme.dim_style());
    let label = |t: &str| Line::from(Span::styled(t.to_string(), theme.dim_style()));
    let lines = match tab {
        PanelTab::Commits | PanelTab::Files => return None,
        PanelTab::Prompt => match text(&p.prompt) {
            Some(t) => paragraphs(t, width, Style::default()),
            None => note("No prompt read from the agent's session log yet."),
        },
        PanelTab::Goal => match text(&p.goal) {
            Some(t) => paragraphs(t, width, Style::default()),
            None => note("No recap goal set."),
        },
        PanelTab::Chat => match text(&p.chat) {
            Some(t) => crate::ui::detail_modules::markdown::render(t, width as u16, theme),
            None => note("No agent message read from its session log yet."),
        },
        PanelTab::Next => {
            let mut out = Vec::new();
            let (next, state) = (text(&p.next), text(&p.state));
            if let Some(t) = next {
                out.extend(paragraphs(t, width, Style::default()));
            }
            if let Some(t) = state {
                if !out.is_empty() {
                    out.push(Line::default());
                }
                out.push(label("state"));
                out.extend(paragraphs(t, width, theme.row_dim_style()));
            }
            if !out.is_empty()
                && let Some(age) = &p.stale
            {
                out.push(Line::default());
                out.extend(paragraphs(
                    &format!("Recap set {age} ago; the workspace has kept working since."),
                    width,
                    theme.warn_style(),
                ));
            }
            if out.is_empty()
                && let Some(t) = text(&p.last)
            {
                out.push(label("last message"));
                out.extend(paragraphs(t, width, theme.row_dim_style()));
            }
            if out.is_empty() {
                out = note("No recap yet.");
            }
            out
        }
    };
    Some(lines)
}

/// `text` wrapped to `width`, keeping its line breaks; runs of blank lines
/// collapse to one.
fn paragraphs(text: &str, width: usize, style: Style) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let mut blank = false;
    for para in text.lines() {
        if para.trim().is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if std::mem::take(&mut blank) {
            out.push(Line::default());
        }
        out.extend(
            wrap(para, width)
                .into_iter()
                .map(|l| Line::from(Span::styled(l, style))),
        );
    }
    out
}

/// `a1b2c3d  subject`, the subject wrapping under itself so the hashes
/// stay in a column.
fn commit_lines(commits: &[BranchCommit], width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    for c in commits {
        let indent = display_width(&c.hash) + 2;
        let subject = if c.subject.is_empty() {
            "(no subject)".to_string()
        } else {
            c.subject.clone()
        };
        for (i, l) in wrap(&subject, width.saturating_sub(indent).max(8))
            .into_iter()
            .enumerate()
        {
            let lead = if i == 0 {
                Span::styled(format!("{}  ", c.hash), Style::default().fg(theme.code))
            } else {
                Span::raw(" ".repeat(indent))
            };
            out.push(Line::from(vec![lead, Span::raw(l)]));
        }
    }
    out
}

/// A total line, then each file's path with its `+N −M` flush right on
/// the path's first line (`binary` for a binary file, which has no line
/// counts); long paths wrap rather than truncate. The file at `cursor` is
/// highlighted. Also returns each file's first line and line count.
fn file_lines(
    files: &[ChangedFile],
    width: usize,
    cursor: Option<usize>,
    theme: &Theme,
) -> (Vec<Line<'static>>, Vec<(usize, usize)>) {
    let (added, removed) = files
        .iter()
        .filter_map(|f| f.stats)
        .fold((0u32, 0u32), |(a, r), d| (a + d.added, r + d.removed));
    let binary = files.iter().filter(|f| f.stats.is_none()).count();
    let mut total = vec![
        Span::styled(
            format!(
                "{} file{}  ",
                files.len(),
                if files.len() == 1 { "" } else { "s" }
            ),
            theme.row_dim_style(),
        ),
        Span::styled(format!("+{added}"), theme.ok_style()),
        Span::raw(" "),
        Span::styled(format!("−{removed}"), theme.err_style()),
    ];
    if binary > 0 {
        total.push(Span::styled(
            format!("  {binary} binary"),
            theme.row_dim_style(),
        ));
    }
    let mut out = vec![Line::from(total), Line::default()];
    let mut extents = Vec::with_capacity(files.len());
    for (fi, f) in files.iter().enumerate() {
        let start = out.len();
        let counts: Vec<Span<'static>> = match f.stats {
            Some(d) => vec![
                Span::styled(format!("+{}", d.added), theme.ok_style()),
                Span::raw(" "),
                Span::styled(format!("−{}", d.removed), theme.err_style()),
            ],
            None => vec![Span::styled("binary", theme.row_dim_style())],
        };
        let cw: usize = counts.iter().map(|s| s.width()).sum();
        let path_w = width.saturating_sub(cw + 2).max(8);
        for (i, l) in wrap(&f.display_path(), path_w).into_iter().enumerate() {
            let mut spans = vec![Span::raw(l.clone())];
            if i == 0 {
                let pad = width.saturating_sub(display_width(&l) + cw);
                spans.push(Span::raw(" ".repeat(pad.max(1))));
                spans.extend(counts.iter().cloned());
            }
            let line = Line::from(spans);
            out.push(if cursor == Some(fi) {
                line.style(theme.selected_style())
            } else {
                line
            });
        }
        extents.push((start, out.len() - start));
    }
    (out, extents)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(
        tab: PanelTab,
        selected: Option<PanelInputs<'_>>,
        w: u16,
        h: u16,
        scroll: &mut usize,
    ) -> (Vec<String>, PanelTargets) {
        let mut view = PanelView {
            scroll: *scroll,
            ..Default::default()
        };
        let out = draw_view(tab, selected, w, h, &mut view);
        *scroll = view.scroll;
        out
    }

    fn draw_view(
        tab: PanelTab,
        selected: Option<PanelInputs<'_>>,
        w: u16,
        h: u16,
        view: &mut PanelView,
    ) -> (Vec<String>, PanelTargets) {
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let mut targets = PanelTargets::default();
        term.draw(|f| targets = render(f, f.area(), tab, selected, view, &Theme::wsx()))
            .unwrap();
        let buf = term.backend().buffer().clone();
        let lines = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        (lines, targets)
    }

    fn commits() -> Vec<BranchCommit> {
        vec![
            BranchCommit {
                hash: "a1b2c3d".into(),
                subject: "feat(dashboard): split focus view rows into facts and prose".into(),
                stats: None,
            },
            BranchCommit {
                hash: "9f8e7d6".into(),
                subject: "fix: drop clean".into(),
                stats: None,
            },
        ]
    }

    fn files() -> Vec<ChangedFile> {
        use crate::git::DiffStats;
        vec![
            ChangedFile {
                path: "src/ui/dashboard/tall.rs".into(),
                old_path: None,
                stats: Some(DiffStats {
                    added: 412,
                    removed: 63,
                }),
            },
            ChangedFile {
                path: "docs/book/src/dashboard/a-rather-long-file-name-that-wraps.md".into(),
                old_path: None,
                stats: Some(DiffStats {
                    added: 5,
                    removed: 1,
                }),
            },
        ]
    }

    #[test]
    fn panel_width_stays_in_bounds() {
        assert_eq!(panel_width(90, None), 36);
        assert_eq!(panel_width(150, None), 50);
        assert_eq!(panel_width(400, None), 64);
        // A dragged width holds, within its own bounds.
        assert_eq!(panel_width(150, Some(80)), 80);
        assert_eq!(panel_width(150, Some(5)), PANEL_MIN_WIDTH);
        assert_eq!(panel_width(150, Some(140)), 109);
    }

    #[test]
    fn commits_tab_lists_hashes_and_wrapped_subjects() {
        let c = commits();
        let f = files();
        let inputs = PanelInputs {
            branch: "bakedbean/x",
            base: Some("origin/main"),
            commits: Some(&c),
            files: Some(&f),
            ahead: None,
            prose: None,
        };
        let (lines, targets) = draw(PanelTab::Commits, Some(inputs), 44, 14, &mut 0);
        assert!(
            lines[0].contains("Prompt")
                && lines[0].contains("Commits 2")
                && lines[0].contains("Files 2"),
            "{lines:?}"
        );
        assert!(lines[1].contains("Chat"), "Chat wraps: {lines:?}");
        assert!(lines[2].contains("bakedbean/x vs main"), "{lines:?}");
        let all = lines.join("\n");
        assert!(all.contains("a1b2c3d  feat(dashboard)"), "{all}");
        assert!(
            all.contains("rows into") && all.contains("prose"),
            "subject wraps, not cut:\n{all}"
        );
        assert!(!all.contains('…'), "{all}");
        let tabs: Vec<PanelTab> = targets.tabs.iter().map(|t| t.0).collect();
        assert_eq!(tabs, PanelTab::ALL);
    }

    /// Tabs that don't fit wrap onto a second bar line, each still a click
    /// target on the line it landed on.
    #[test]
    fn a_narrow_tab_bar_wraps() {
        let (lines, targets) = draw(PanelTab::Next, None, 30, 10, &mut 0);
        assert!(
            lines[0].contains("Prompt") && !lines[0].contains("Files"),
            "{lines:?}"
        );
        assert!(lines[1].contains("Files"), "{lines:?}");
        assert_eq!(targets.tabs.len(), 6);
        let files = targets
            .tabs
            .iter()
            .find(|t| t.0 == PanelTab::Files)
            .unwrap()
            .1;
        assert_eq!(files.y, 1);
    }

    fn prose_draw(tab: PanelTab, prose: &PanelProse, h: u16, scroll: &mut usize) -> Vec<String> {
        let inputs = PanelInputs {
            branch: "b",
            base: None,
            commits: None,
            files: None,
            ahead: None,
            prose: Some(prose),
        };
        draw(tab, Some(inputs), 50, h, scroll).0
    }

    /// Text tabs show their field in full, keeping paragraph breaks, and
    /// need no base branch.
    #[test]
    fn text_tabs_show_their_field_in_full() {
        let prompt = format!("FIRST {}\n\n\nSECOND paragraph END", "word ".repeat(60));
        let prose = PanelProse {
            prompt: Some(prompt),
            goal: Some("the GOAL".into()),
            ..Default::default()
        };
        let lines = prose_draw(PanelTab::Prompt, &prose, 40, &mut 0);
        let all = lines.join("\n");
        assert!(
            all.contains("FIRST") && all.contains("SECOND paragraph END"),
            "{all}"
        );
        assert!(!all.contains('…'), "never cut short: {all}");
        let second = lines.iter().position(|l| l.contains("SECOND")).unwrap();
        assert_eq!(
            lines[second - 1].trim_matches(['│', ' ']),
            "",
            "one blank between"
        );
        assert_ne!(
            lines[second - 2].trim_matches(['│', ' ']),
            "",
            "blank runs collapse"
        );

        let all = prose_draw(PanelTab::Goal, &prose, 20, &mut 0).join("\n");
        assert!(all.contains("the GOAL") && !all.contains("FIRST"), "{all}");
        assert!(!all.contains("base branch"), "{all}");
    }

    #[test]
    fn next_tab_shows_next_state_and_staleness() {
        let prose = PanelProse {
            next: Some("UNIQUE-NEXT".into()),
            state: Some("UNIQUE-STATE".into()),
            stale: Some("5h".into()),
            last: Some("UNIQUE-LAST".into()),
            ..Default::default()
        };
        let all = prose_draw(PanelTab::Next, &prose, 20, &mut 0).join("\n");
        assert!(
            all.contains("UNIQUE-NEXT") && all.contains("UNIQUE-STATE"),
            "{all}"
        );
        assert!(all.contains("Recap set 5h ago"), "{all}");
        assert!(
            !all.contains("UNIQUE-LAST"),
            "a recap wins over the last message"
        );

        let prose = PanelProse {
            last: Some("UNIQUE-LAST".into()),
            stale: Some("5h".into()),
            ..Default::default()
        };
        let all = prose_draw(PanelTab::Next, &prose, 20, &mut 0).join("\n");
        assert!(
            all.contains("last message") && all.contains("UNIQUE-LAST"),
            "{all}"
        );
        assert!(!all.contains("Recap set"), "{all}");

        let all = prose_draw(PanelTab::Next, &PanelProse::default(), 20, &mut 0).join("\n");
        assert!(all.contains("No recap yet."), "{all}");
        let all = prose_draw(PanelTab::Prompt, &PanelProse::default(), 20, &mut 0).join("\n");
        assert!(all.contains("No prompt read"), "{all}");
    }

    #[test]
    fn chat_tab_renders_the_latest_message_as_markdown() {
        let prose = PanelProse {
            next: Some("UNIQUE-NEXT".into()),
            chat: Some("**Done.** See `foo.rs`\n\n- one\n- two".into()),
            ..Default::default()
        };
        let all = prose_draw(PanelTab::Chat, &prose, 20, &mut 0).join("\n");
        assert!(all.contains("Done.") && !all.contains("**"), "{all}");
        assert!(all.contains("foo.rs") && all.contains("two"), "{all}");
        assert!(!all.contains("UNIQUE-NEXT"), "{all}");

        let all = prose_draw(PanelTab::Chat, &PanelProse::default(), 20, &mut 0).join("\n");
        assert!(all.contains("No agent message"), "{all}");
    }

    /// A long text tab scrolls, clamped to its end.
    #[test]
    fn a_long_text_tab_scrolls() {
        let prose = PanelProse {
            goal: Some(
                (0..60)
                    .map(|i| format!("g{i}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            ..Default::default()
        };
        let mut scroll = 5;
        let all = prose_draw(PanelTab::Goal, &prose, 12, &mut scroll).join("\n");
        assert!(
            all.contains("g5") && !all.contains("g4\n") && !all.contains(" g4 "),
            "{all}"
        );
        let mut scroll = 999;
        let all = prose_draw(PanelTab::Goal, &prose, 12, &mut scroll).join("\n");
        assert!(all.contains("g59"), "{all}");
        assert!(scroll < 60);
    }

    #[test]
    fn files_tab_totals_and_right_aligns_counts() {
        let c = commits();
        let f = files();
        let inputs = PanelInputs {
            branch: "bakedbean/x",
            base: Some("main"),
            commits: Some(&c),
            files: Some(&f),
            ahead: None,
            prose: None,
        };
        let (lines, _) = draw(PanelTab::Files, Some(inputs), 44, 16, &mut 0);
        let all = lines.join("\n");
        assert!(all.contains("2 files  +417 −64"), "{all}");
        let tall = lines.iter().find(|l| l.contains("tall.rs")).unwrap();
        assert!(tall.trim_end().ends_with("+412 −63"), "{tall:?}");
        assert!(all.contains("wraps.md"), "long path wraps:\n{all}");
    }

    #[test]
    fn empty_states_explain_themselves() {
        let (lines, _) = draw(PanelTab::Commits, None, 40, 8, &mut 0);
        assert!(lines.join(" ").contains("Select a workspace"));

        let none: &[BranchCommit] = &[];
        let inputs = PanelInputs {
            branch: "b",
            base: None,
            commits: Some(none),
            files: None,
            ahead: None,
            prose: None,
        };
        let (lines, _) = draw(PanelTab::Commits, Some(inputs), 40, 8, &mut 0);
        assert!(lines.join(" ").contains("base branch"), "{lines:?}");

        let inputs = PanelInputs {
            base: Some("origin/main"),
            ..inputs
        };
        let (lines, _) = draw(PanelTab::Commits, Some(inputs), 40, 8, &mut 0);
        assert!(
            lines.join(" ").contains("No commits ahead of main"),
            "{lines:?}"
        );
        let (lines, _) = draw(PanelTab::Files, Some(inputs), 40, 8, &mut 0);
        assert!(lines.join(" ").contains("loading"), "{lines:?}");
    }

    #[test]
    fn body_scroll_is_clamped() {
        let many: Vec<BranchCommit> = (0..50)
            .map(|i| BranchCommit {
                hash: format!("{i:07}"),
                subject: format!("commit {i}"),
                stats: None,
            })
            .collect();
        let inputs = PanelInputs {
            branch: "b",
            base: Some("main"),
            commits: Some(&many),
            files: None,
            ahead: None,
            prose: None,
        };
        let mut scroll = 999;
        let (lines, _) = draw(PanelTab::Commits, Some(inputs), 40, 10, &mut scroll);
        assert!(scroll < 50, "clamped: {scroll}");
        assert!(lines.last().unwrap().contains("commit 49"), "{lines:?}");
    }

    /// A branch that only adds a binary file still lists it, as `binary`.
    #[test]
    fn binary_files_are_listed() {
        let c = commits();
        let f = vec![ChangedFile {
            path: "assets/logo.png".into(),
            old_path: None,
            stats: None,
        }];
        let inputs = PanelInputs {
            branch: "b",
            base: Some("main"),
            commits: Some(&c),
            files: Some(&f),
            ahead: Some(2),
            prose: None,
        };
        let (lines, _) = draw(PanelTab::Files, Some(inputs), 44, 10, &mut 0);
        let all = lines.join("\n");
        assert!(all.contains("1 file  +0 −0  1 binary"), "{all}");
        let row = lines.iter().find(|l| l.contains("logo.png")).unwrap();
        assert!(row.trim_end().ends_with("binary"), "{row:?}");
        assert!(!all.contains("No files changed"), "{all}");
    }

    /// Past the listing cap, the tab shows the true total and the list
    /// says how many older commits it left out.
    #[test]
    fn capped_commit_lists_say_so() {
        let listed: Vec<BranchCommit> = (0..3)
            .map(|i| BranchCommit {
                hash: format!("{i:07}"),
                subject: format!("commit {i}"),
                stats: None,
            })
            .collect();
        let inputs = PanelInputs {
            branch: "b",
            base: Some("main"),
            commits: Some(&listed),
            files: None,
            ahead: Some(250),
            prose: None,
        };
        let (lines, _) = draw(PanelTab::Commits, Some(inputs), 44, 14, &mut 0);
        let all = lines.join("\n");
        assert!(lines[0].contains("Commits 250"), "{all}");
        assert!(all.contains("247 older commits not listed."), "{all}");
    }

    /// A long branch name never crowds out the list: the title stops at
    /// two lines, fewer when the panel is short.
    #[test]
    fn a_long_title_leaves_room_for_the_list() {
        let c = vec![BranchCommit {
            hash: "a1b2c3d".into(),
            subject: "UNIQUE-COMMIT".into(),
            stats: None,
        }];
        let branch = "bakedbean/".to_string() + &"very-long-branch-name-".repeat(6);
        let inputs = PanelInputs {
            branch: &branch,
            base: Some("main"),
            commits: Some(&c),
            files: None,
            ahead: Some(1),
            prose: None,
        };
        for h in [6u16, 4] {
            let (lines, _) = draw(PanelTab::Commits, Some(inputs), 40, h, &mut 999);
            assert!(
                lines.join("\n").contains("UNIQUE-COMMIT"),
                "height {h}: {lines:?}"
            );
        }
    }

    fn many_files(n: usize) -> Vec<ChangedFile> {
        (0..n)
            .map(|i| ChangedFile {
                path: format!("src/file{i:02}.rs"),
                old_path: None,
                stats: Some(crate::git::DiffStats {
                    added: 1,
                    removed: 0,
                }),
            })
            .collect()
    }

    fn file_inputs(files: &[ChangedFile]) -> PanelInputs<'_> {
        PanelInputs {
            branch: "b",
            base: Some("main"),
            commits: Some(&[]),
            files: Some(files),
            ahead: Some(0),
            prose: None,
        }
    }

    /// Focused, the cursor's file is highlighted and a hint names the keys;
    /// unfocused, neither shows.
    #[test]
    fn focus_shows_the_cursor_and_hint() {
        let files = many_files(3);
        let mut view = PanelView {
            cursor: 1,
            focused: true,
            ..Default::default()
        };
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 12)).unwrap();
        let theme = Theme::wsx();
        term.draw(|f| {
            render(
                f,
                f.area(),
                PanelTab::Files,
                Some(file_inputs(&files)),
                &mut view,
                &theme,
            );
        })
        .unwrap();
        let buf = term.backend().buffer().clone();
        let row = |y: u16| (0..40).map(|x| buf[(x, y)].symbol()).collect::<String>();
        let y = (0..12).find(|&y| row(y).contains("file01")).unwrap();
        assert_eq!(buf[(2, y)].bg, theme.selected_bg);
        let other = (0..12).find(|&y| row(y).contains("file00")).unwrap();
        assert_ne!(buf[(2, other)].bg, theme.selected_bg);
        assert!(
            row(11).contains("diff") && row(11).contains("back"),
            "{:?}",
            row(11)
        );

        view.focused = false;
        let (lines, _) = draw_view(
            PanelTab::Files,
            Some(file_inputs(&files)),
            40,
            12,
            &mut view,
        );
        assert!(!lines[11].contains("back"), "{lines:?}");
    }

    /// Moving the cursor past the bottom scrolls it into view, the cursor
    /// clamps to the list, and every visible file row is a click target.
    #[test]
    fn the_cursor_scrolls_into_view_and_clamps() {
        let files = many_files(30);
        let mut view = PanelView {
            cursor: 25,
            focused: true,
            follow: true,
            ..Default::default()
        };
        let (lines, targets) = draw_view(
            PanelTab::Files,
            Some(file_inputs(&files)),
            40,
            12,
            &mut view,
        );
        assert!(lines.join("\n").contains("file25"), "{lines:?}");
        assert!(view.scroll > 0);
        let (i, r) = targets.files.iter().find(|(i, _)| *i == 25).unwrap();
        assert!(lines[r.y as usize].contains(&format!("file{i}")));

        view.cursor = 99;
        draw_view(
            PanelTab::Files,
            Some(file_inputs(&files)),
            40,
            12,
            &mut view,
        );
        assert_eq!(view.cursor, 29);
    }

    #[test]
    fn focus_holds_only_on_a_shown_panel() {
        let mut p = SidePanelState::default();
        p.focus_files();
        assert!(p.is_focused());
        p.show(PanelTab::Commits);
        assert!(p.is_focused(), "focus follows the tab");
        p.toggle();
        p.toggle();
        assert!(p.open && !p.is_focused(), "closing drops focus for good");
        p.focus();
        assert!(
            p.is_focused() && p.tab == PanelTab::Commits,
            "focus keeps the tab"
        );
    }

    #[test]
    fn focus_on_an_open_panel_keeps_its_tab() {
        let mut p = SidePanelState::open_on(PanelTab::Next);
        p.focus();
        assert!(p.is_focused() && p.tab == PanelTab::Next);
        p.toggle();
        assert!(!p.open && !p.is_focused());
    }

    /// Another tab leaves the Files cursor where it was; only Files clamps
    /// it.
    #[test]
    fn another_tab_keeps_the_file_cursor() {
        let files = many_files(10);
        let mut view = PanelView {
            cursor: 7,
            ..Default::default()
        };
        draw_view(
            PanelTab::Commits,
            Some(file_inputs(&files)),
            40,
            12,
            &mut view,
        );
        assert_eq!(view.cursor, 7);
        view.cursor = 99;
        draw_view(
            PanelTab::Files,
            Some(file_inputs(&files)),
            40,
            12,
            &mut view,
        );
        assert_eq!(view.cursor, 9, "clamped on Files");
    }

    /// However short the panel, every tab draws without panicking.
    #[test]
    fn a_tiny_panel_still_draws() {
        let files = many_files(3);
        for h in 1..5 {
            for w in [4, 12, 30] {
                for tab in PanelTab::ALL {
                    let mut view = PanelView {
                        focused: true,
                        follow: true,
                        ..Default::default()
                    };
                    draw_view(tab, Some(file_inputs(&files)), w, h, &mut view);
                }
            }
        }
    }

    #[test]
    fn tabs_step_both_ways_and_round_trip_their_keys() {
        let mut p = SidePanelState::default();
        p.show(PanelTab::Chat);
        p.next_tab();
        assert_eq!(p.tab, PanelTab::Prompt, "wraps");
        p.prev_tab();
        assert_eq!(p.tab, PanelTab::Chat);
        for t in PanelTab::ALL {
            assert_eq!(PanelTab::from_key(t.key()), Some(t));
        }
    }
}
