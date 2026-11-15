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
//! Pure layout and painting; the caller owns which tab is showing, the
//! scroll offset, and the data.

use crate::git::{BranchCommit, DiffStats};
use crate::ui::dashboard::tall::wrap;
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

/// What the panel shows for the selected workspace.
#[derive(Debug, Clone, Copy)]
pub struct PanelInputs<'a> {
    pub branch: &'a str,
    /// The repo's base branch; without one there is nothing to list
    /// against.
    pub base: Option<&'a str>,
    /// `None` until the first poll has run.
    pub commits: Option<&'a [BranchCommit]>,
    /// Sorted by path; `None` until the first poll has run.
    pub files: Option<&'a [(String, DiffStats)]>,
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
            PanelTab::Commits => s.commits.map(<[_]>::len),
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
        for l in wrap(&title, width) {
            head.push(Line::from(Span::styled(l, theme.row_dim_style())));
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
            Some(commits) => commit_lines(commits, width, theme),
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
/// the path's first line; long paths wrap rather than truncate.
fn file_lines(files: &[(String, DiffStats)], width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let (added, removed) = files
        .iter()
        .fold((0u32, 0u32), |(a, r), (_, d)| (a + d.added, r + d.removed));
    let mut out = vec![
        Line::from(vec![
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
        ]),
        Line::default(),
    ];
    for (path, d) in files {
        let counts = format!("+{} −{}", d.added, d.removed);
        let cw = display_width(&counts);
        let path_w = width.saturating_sub(cw + 2).max(8);
        for (i, l) in wrap(path, path_w).into_iter().enumerate() {
            let mut spans = vec![Span::raw(l.clone())];
            if i == 0 {
                let pad = width.saturating_sub(display_width(&l) + cw);
                spans.push(Span::raw(" ".repeat(pad.max(1))));
                spans.push(Span::styled(format!("+{}", d.added), theme.ok_style()));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(format!("−{}", d.removed), theme.err_style()));
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

    fn files() -> Vec<(String, DiffStats)> {
        vec![
            (
                "src/ui/dashboard/tall.rs".into(),
                DiffStats {
                    added: 412,
                    removed: 63,
                },
            ),
            (
                "docs/book/src/dashboard/a-rather-long-file-name-that-wraps.md".into(),
                DiffStats {
                    added: 5,
                    removed: 1,
                },
            ),
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
        };
        let mut scroll = 999;
        let (lines, _) = draw(PanelTab::Commits, Some(inputs), 40, 10, &mut scroll);
        assert!(scroll < 50, "clamped: {scroll}");
        assert!(lines.last().unwrap().contains("commit 49"), "{lines:?}");
    }
}
