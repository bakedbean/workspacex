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

        // `↑4 vs main  ⇡2 unpushed ⇣1 behind`: the base count, then each
        // sync item on its own (sync_cell joins them with a bare space).
        let mut counts: Vec<Vec<Span<'static>>> = Vec::new();
        if let Some(n) = ctx.ahead_of_base {
            let label = tall::ahead_label(n, ctx.repo.base_branch.as_deref());
            counts.push(vec![Span::styled(label, theme.row_dim_style())]);
        }
        counts.extend(
            tall::sync_cell(ctx.lifecycle, ctx.git_status, ctx.ahead_of_base, theme)
                .into_iter()
                .filter(|s| s.content != " ")
                .map(|s| vec![s]),
        );
        pack(&mut out, counts, "  ", width);

        // `3 mod 1 untr`, kept as `3 mod` / `1 untr` pairs, or `clean` once
        // the tree is known to be.
        let mut dirty: Vec<Vec<Span<'static>>> = tall::dirty_parts(ctx.git_status)
            .into_iter()
            .map(|d| vec![Span::styled(d, theme.row_dim_style())])
            .collect();
        if dirty.is_empty() && ctx.git_status.is_some() {
            dirty.push(vec![Span::styled("clean", theme.dim_style())]);
        }
        pack(&mut out, dirty, " ", width);

        // `▂▅▁▃█▂▄ +184 −22`: the newest commits' sizes, oldest left, scaled
        // to this branch's largest commit, then the branch's totals.
        let sizes = tall::commit_sizes(ctx.commits);
        let max = tall::commit_size_max(&sizes);
        if let Some(totals) = tall::diff_totals(ctx.diff, theme) {
            let bars = tall::commit_bars(&sizes, max, theme);
            let parts = [bars, totals]
                .into_iter()
                .filter(|p| !p.is_empty())
                .collect();
            pack(&mut out, parts, " ", width);
        }

        if out.is_empty() {
            out.push(Line::from(Span::styled("—", theme.dim_style())));
        }
        out
    }
}

/// Pack `parts` onto lines of at most `width` cells, joined by `sep`, so a
/// narrow column wraps between parts rather than clipping one off; a part
/// wider than a line gets one to itself, cut with an ellipsis.
fn pack(out: &mut Vec<Line<'static>>, parts: Vec<Vec<Span<'static>>>, sep: &str, width: usize) {
    let mut line: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for part in parts {
        let w: usize = part.iter().map(Span::width).sum();
        if !line.is_empty() && used + display_width(sep) + w > width {
            out.push(Line::from(std::mem::take(&mut line)));
            used = 0;
        }
        if !line.is_empty() {
            line.push(Span::raw(sep.to_string()));
            used += display_width(sep);
        }
        let part = tall::fit_spans(part, width);
        used += part.iter().map(Span::width).sum::<usize>();
        line.extend(part);
    }
    if !line.is_empty() {
        out.push(Line::from(line));
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
        assert_eq!(lines, vec!["↑4 base", "⇡2 unpushed", "⇣1 behind", "clean"]);
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

    fn full_ctx(base: &str) -> DetailContext<'static> {
        let commits: &'static [BranchCommit] =
            Box::leak(vec![commit(5000, 0), commit(1, 9), commit(30, 2)].into_boxed_slice());
        let repo = Box::leak(Box::new(stub_context().repo.clone()));
        repo.base_branch = Some(base.into());
        DetailContext {
            repo,
            git_status: Some(WorkspaceStatus {
                modified: 3,
                untracked: 12,
                ahead: 2,
                behind: 7,
                upstream: true,
            }),
            ahead_of_base: Some(40),
            commits,
            diff: Some(DiffStats {
                added: 18_400,
                removed: 2_200,
            }),
            ..stub_context()
        }
    }

    /// No line outgrows its column, whatever the width: the container
    /// paints without wrapping, so anything wider would be clipped off.
    #[test]
    fn every_line_fits_the_column() {
        let ctx = full_ctx("origin/a-rather-long-base-branch-name");
        for width in 1..60u16 {
            for line in Git.lines(&ctx, width) {
                assert!(
                    line.width() <= width as usize,
                    "width {width}: {:?} is {} wide",
                    text(std::slice::from_ref(&line)),
                    line.width()
                );
            }
        }
    }

    #[test]
    fn narrow_columns_wrap_between_parts_instead_of_dropping_them() {
        let ctx = full_ctx("origin/main");
        let lines = text(&Git.lines(&ctx, 12));
        assert_eq!(
            lines,
            vec![
                "↑40 vs main",
                "⇡2 unpushed",
                "⇣7 behind",
                "3 mod",
                "12 untr",
                "▄▃█",
                "+18400 −2200",
            ]
        );
    }

    #[test]
    fn a_part_wider_than_the_column_is_cut_with_an_ellipsis() {
        let ctx = full_ctx("origin/a-rather-long-base-branch-name");
        let lines = text(&Git.lines(&ctx, 16));
        assert_eq!(display_width(&lines[0]), 16);
        assert!(lines[0].starts_with("↑40 vs a-rather") && lines[0].ends_with('…'));
    }
}
