//! Git module. The branch's git state as the repo focus view's tall rows
//! show it: commits ahead of the base branch, where it stands against its
//! upstream, uncommitted changes, and a bar per recent commit with the
//! branch's line totals.
//!
//! The cells come from `crate::ui::dashboard::tall`, so the two views
//! can't drift apart.

use crate::ui::dashboard::tall;
use crate::ui::detail_modules::{DetailContext, DetailModule};
use crate::ui::text::display_width;
use ratatui::text::{Line, Span};

pub struct Git;

impl DetailModule for Git {
    fn id(&self) -> &'static str {
        "git"
    }
    fn title(&self) -> &'static str {
        "GIT"
    }
    fn description(&self) -> &'static str {
        "Commits ahead, unpushed and uncommitted counts, and a commit-size graph"
    }
    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<Line<'static>> {
        let theme = ctx.theme;
        let width = width as usize;
        let mut out: Vec<Line<'static>> = Vec::new();

        // `↑4 vs main  ⇡2 unpushed`, split over two lines when too wide.
        let ahead = ctx
            .ahead_of_base
            .map(|n| tall::ahead_label(n, ctx.repo.base_branch.as_deref()));
        let sync = tall::sync_cell(ctx.lifecycle, ctx.git_status, ctx.ahead_of_base, theme);
        let sync_width: usize = sync.iter().map(Span::width).sum();
        match ahead {
            Some(a) if !sync.is_empty() && display_width(&a) + 2 + sync_width <= width => {
                let mut spans = vec![Span::styled(a, theme.row_dim_style()), Span::raw("  ")];
                spans.extend(sync);
                out.push(Line::from(spans));
            }
            Some(a) => {
                out.push(Line::from(Span::styled(a, theme.row_dim_style())));
                if !sync.is_empty() {
                    out.push(Line::from(sync));
                }
            }
            None if !sync.is_empty() => out.push(Line::from(sync)),
            None => {}
        }

        // `3 mod 1 untr`, or `clean` once the tree is known to be.
        match tall::dirty_cell(ctx.git_status) {
            Some(d) => out.push(Line::from(Span::styled(d, theme.row_dim_style()))),
            None if ctx.git_status.is_some() => {
                out.push(Line::from(Span::styled("clean", theme.dim_style())))
            }
            None => {}
        }

        // `▂▅▁▃█▂▄ +184 −22`: the newest commits' sizes, oldest left.
        let sizes = tall::commit_sizes(ctx.commits);
        let max = tall::commit_size_max(&sizes);
        // Padding to its own bar count leaves one space before the totals.
        let diff = tall::diff_cell(ctx.diff, &sizes, max, sizes.len(), theme);
        if !diff.is_empty() {
            out.push(Line::from(diff));
        }

        if out.is_empty() {
            out.push(Line::from(Span::styled("—", theme.dim_style())));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{BranchCommit, DiffStats, WorkspaceStatus};
    use crate::ui::detail_modules::tests_helpers::stub_context;

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn commit(added: u32, removed: u32) -> BranchCommit {
        BranchCommit {
            hash: "abc1234".into(),
            subject: "subject".into(),
            stats: Some(DiffStats { added, removed }),
        }
    }

    #[test]
    fn id_and_title() {
        assert_eq!(Git.id(), "git");
        assert_eq!(Git.title(), "GIT");
    }

    #[test]
    fn without_git_data_shows_a_dash() {
        let ctx = stub_context();
        assert_eq!(text(&Git.lines(&ctx, 40)), vec!["—"]);
    }

    #[test]
    fn shows_counts_dirty_tree_and_commit_graph() {
        let commits: &'static [BranchCommit] =
            Box::leak(vec![commit(5000, 0), commit(1, 0)].into_boxed_slice());
        let repo = Box::leak(Box::new(stub_context().repo.clone()));
        repo.base_branch = Some("origin/main".into());
        let ctx = DetailContext {
            repo,
            git_status: Some(WorkspaceStatus {
                modified: 3,
                untracked: 1,
                ahead: 2,
                behind: 0,
                upstream: true,
            }),
            ahead_of_base: Some(4),
            commits,
            diff: Some(DiffStats {
                added: 184,
                removed: 22,
            }),
            ..stub_context()
        };
        let lines = text(&Git.lines(&ctx, 40));
        assert_eq!(lines[0], "↑4 vs main  ⇡2 unpushed");
        assert_eq!(lines[1], "3 mod 1 untr");
        // Two bars, oldest (the one-line commit) first, then the totals.
        assert_eq!(display_width(&lines[2]), "▁█ +184 −22".chars().count());
        assert!(lines[2].ends_with(" +184 −22"), "{:?}", lines[2]);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn narrow_columns_split_ahead_and_sync() {
        let ctx = DetailContext {
            git_status: Some(WorkspaceStatus {
                ahead: 2,
                behind: 1,
                upstream: true,
                ..Default::default()
            }),
            ahead_of_base: Some(4),
            ..stub_context()
        };
        let lines = text(&Git.lines(&ctx, 12));
        assert_eq!(lines, vec!["↑4 base", "⇡2 unpushed ⇣1 behind", "clean"]);
    }

    #[test]
    fn unpushed_branch_without_a_pr_says_not_pushed() {
        let ctx = DetailContext {
            git_status: Some(WorkspaceStatus::default()),
            ahead_of_base: Some(1),
            ..stub_context()
        };
        let lines = text(&Git.lines(&ctx, 40));
        assert_eq!(lines[0], "↑1 base  not pushed");
    }
}
