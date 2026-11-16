//! The repo focus view's side panel: the selected workspace's commits or
//! changed files, one at a time under a tab bar.
//!
//! ```text
//! │ Commits 4   Files 7
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

/// Which list the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelTab {
    #[default]
    Commits,
    Files,
}

impl PanelTab {
    pub const ALL: [PanelTab; 2] = [PanelTab::Commits, PanelTab::Files];

    fn label(self) -> &'static str {
        match self {
            PanelTab::Commits => "Commits",
            PanelTab::Files => "Files",
        }
    }
}

/// Narrowest terminal that gets the panel; below it there isn't room for
/// both the rows and a useful panel, so the panel stays hidden.
pub const PANEL_MIN_TOTAL: u16 = 100;

/// The panel's width for a `total`-cell body: about a third, kept within
/// bounds that hold a commit subject or a path without crowding the rows.
pub fn panel_width(total: u16) -> u16 {
    (total / 3).clamp(36, 64)
}

/// One surface's panel: whether it shows, its tab, and its scroll.
#[derive(Debug, Clone, Default)]
pub struct SidePanelState {
    pub open: bool,
    /// The tab, kept while the panel is closed.
    pub tab: PanelTab,
    /// The body's first visible line, clamped on draw.
    pub scroll: usize,
    /// The workspace and tab the panel last drew, so the scroll resets
    /// when either changes.
    anchor: Option<(Option<WorkspaceId>, PanelTab)>,
}

impl SidePanelState {
    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Open the panel on `tab`.
    pub fn show(&mut self, tab: PanelTab) {
        self.open = true;
        self.tab = tab;
    }

    /// Switch to the other tab, opening the panel if closed.
    pub fn next_tab(&mut self) {
        let next = match self.tab {
            PanelTab::Commits => PanelTab::Files,
            PanelTab::Files => PanelTab::Commits,
        };
        self.show(next);
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
        let w = panel_width(area.width);
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
            self.anchor = Some(anchor);
        }
        let targets = render(
            f,
            panel_area,
            self.tab,
            data.map(PanelData::inputs),
            &mut self.scroll,
            theme,
        );
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
}

impl PanelData {
    pub fn inputs(&self) -> PanelInputs<'_> {
        PanelInputs {
            branch: &self.branch,
            base: self.base.as_deref(),
            commits: self.lists.as_deref().map(|l| l.commits.as_slice()),
            files: self.lists.as_deref().map(|l| l.files.as_slice()),
            ahead: self.ahead,
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
}

/// Click targets in the panel, in screen coordinates.
#[derive(Debug, Default)]
pub struct PanelTargets {
    pub tabs: Vec<(PanelTab, Rect)>,
    /// The whole panel, for routing the mouse wheel to it.
    pub area: Option<Rect>,
}

/// Draw the panel into `area`. `selected` is `None` when no workspace is
/// selected. `scroll` is the body's first visible line, clamped here.
pub fn render(
    f: &mut Frame,
    area: Rect,
    tab: PanelTab,
    selected: Option<PanelInputs<'_>>,
    scroll: &mut usize,
    theme: &Theme,
) -> PanelTargets {
    if area.width < 4 || area.height == 0 {
        return PanelTargets::default();
    }
    // A rule down the left edge separates the panel from the rows.
    let edge: Vec<Line<'static>> = (0..area.height)
        .map(|_| Line::from(Span::styled("│", theme.dim_style())))
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
    let mut bar: Vec<Span<'static>> = Vec::new();
    let mut x = inner.x;
    for t in PanelTab::ALL {
        let count = selected.and_then(|s| match t {
            // The true total, not just the listed (capped) commits.
            PanelTab::Commits => s
                .commits
                .map(|c| c.len().max(s.ahead.map_or(0, |a| a as usize))),
            PanelTab::Files => s.files.map(<[_]>::len),
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
        if x + w <= inner.x + inner.width {
            targets.tabs.push((
                t,
                Rect {
                    x,
                    y: inner.y,
                    width: w,
                    height: 1,
                },
            ));
        }
        bar.push(Span::styled(text, style));
        bar.push(Span::raw(" "));
        x += w + 1;
    }

    let mut head: Vec<Line<'static>> = vec![Line::from(bar)];
    if let Some(s) = selected {
        let title = match s.base {
            Some(b) => format!("{} vs {}", s.branch, b.strip_prefix("origin/").unwrap_or(b)),
            None => s.branch.to_string(),
        };
        // At most two lines, and never so many that the tab bar, the rule
        // and at least one line of the list don't fit: a long branch name
        // must not push the list it introduces off the bottom.
        let room = (inner.height as usize).saturating_sub(3).min(2);
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

    let body = body_lines(tab, selected, width, theme);
    let body_h = (inner.height as usize).saturating_sub(head.len());
    *scroll = (*scroll).min(body.len().saturating_sub(body_h));
    let mut lines = head;
    lines.extend(body.into_iter().skip(*scroll).take(body_h));
    f.render_widget(Paragraph::new(lines), inner);
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
        return note("Select a workspace to see its commits and changed files.");
    };
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
            Some(files) => file_lines(files, width, theme),
        },
    }
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
/// counts); long paths wrap rather than truncate.
fn file_lines(files: &[ChangedFile], width: usize, theme: &Theme) -> Vec<Line<'static>> {
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
    for f in files {
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
        for (i, l) in wrap(&f.path, path_w).into_iter().enumerate() {
            let mut spans = vec![Span::raw(l.clone())];
            if i == 0 {
                let pad = width.saturating_sub(display_width(&l) + cw);
                spans.push(Span::raw(" ".repeat(pad.max(1))));
                spans.extend(counts.iter().cloned());
            }
            out.push(Line::from(spans));
        }
    }
    out
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
        let mut term = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let mut targets = PanelTargets::default();
        term.draw(|f| targets = render(f, f.area(), tab, selected, scroll, &Theme::wsx()))
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
            },
            BranchCommit {
                hash: "9f8e7d6".into(),
                subject: "fix: drop clean".into(),
            },
        ]
    }

    fn files() -> Vec<ChangedFile> {
        use crate::git::DiffStats;
        vec![
            ChangedFile {
                path: "src/ui/dashboard/tall.rs".into(),
                stats: Some(DiffStats {
                    added: 412,
                    removed: 63,
                }),
            },
            ChangedFile {
                path: "docs/book/src/dashboard/a-rather-long-file-name-that-wraps.md".into(),
                stats: Some(DiffStats {
                    added: 5,
                    removed: 1,
                }),
            },
        ]
    }

    #[test]
    fn panel_width_stays_in_bounds() {
        assert_eq!(panel_width(90), 36);
        assert_eq!(panel_width(150), 50);
        assert_eq!(panel_width(400), 64);
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
        };
        let (lines, targets) = draw(PanelTab::Commits, Some(inputs), 40, 14, &mut 0);
        assert!(
            lines[0].contains("Commits 2") && lines[0].contains("Files 2"),
            "{lines:?}"
        );
        assert!(lines[1].contains("bakedbean/x vs main"), "{lines:?}");
        let all = lines.join("\n");
        assert!(all.contains("a1b2c3d  feat(dashboard)"), "{all}");
        assert!(
            all.contains("view rows") && all.contains("prose"),
            "subject wraps, not cut:\n{all}"
        );
        assert!(!all.contains('…'), "{all}");
        assert_eq!(targets.tabs.len(), 2);
        assert_eq!(targets.tabs[0].0, PanelTab::Commits);
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
            })
            .collect();
        let inputs = PanelInputs {
            branch: "b",
            base: Some("main"),
            commits: Some(&many),
            files: None,
            ahead: None,
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
            stats: None,
        }];
        let inputs = PanelInputs {
            branch: "b",
            base: Some("main"),
            commits: Some(&c),
            files: Some(&f),
            ahead: Some(2),
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
            })
            .collect();
        let inputs = PanelInputs {
            branch: "b",
            base: Some("main"),
            commits: Some(&listed),
            files: None,
            ahead: Some(250),
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
        }];
        let branch = "bakedbean/".to_string() + &"very-long-branch-name-".repeat(6);
        let inputs = PanelInputs {
            branch: &branch,
            base: Some("main"),
            commits: Some(&c),
            files: None,
            ahead: Some(1),
        };
        for h in [6u16, 4] {
            let (lines, _) = draw(PanelTab::Commits, Some(inputs), 40, h, &mut 999);
            assert!(
                lines.join("\n").contains("UNIQUE-COMMIT"),
                "height {h}: {lines:?}"
            );
        }
    }
}
