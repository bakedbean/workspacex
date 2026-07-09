//! Renderer for `Modal::RemoteWorkspaceList`. Mirrors `process_list.rs`'s
//! and `updates_panel.rs`'s structure: a dedicated fn reading live App
//! state (here, `app.remote_list`), called directly from `app/render.rs`'s
//! `draw()` because the floating panel needs the panel-frame + notice-row
//! idiom that the generic `render()` dispatcher doesn't provide.

use super::*;
use crate::app::{RemoteList, remote_rows};
use crate::ui::text::{truncate, truncate_pad};

/// Render the floating remote-workspace-list modal. Rows are flattened per
/// agent instance by `crate::app::remote_rows` — the same helper the key
/// handler in `app/input.rs` uses to resolve `selected` — so the row drawn
/// at a given index always matches the row `Enter` would act on.
pub fn render_remote_workspace_list(
    f: &mut Frame,
    area: Rect,
    list: &RemoteList,
    selected: usize,
    notice: Option<&str>,
    theme: &Theme,
    nerd_fonts: bool,
) {
    let w = area.width.clamp(20, 100);
    let h = area.height.clamp(8, 25);
    let inner = panel_frame(
        f,
        area,
        w,
        h,
        format!(" shared workspaces on {} ", list.host_name),
        theme,
    );

    let rows = remote_rows(list);

    let has_notice = notice.is_some();
    let constraints = if has_notice {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    let body_area = chunks[0];
    let (notice_area, footer_area) = if has_notice {
        (Some(chunks[1]), chunks[2])
    } else {
        (None, chunks[1])
    };

    if rows.is_empty() {
        // Two distinct empty states: the host may have no shared workspaces at
        // all, or it may have some whose tmux sessions are all offline (those
        // rows are filtered out by `remote_rows`, which is attach-only). Say
        // which so a workspace the user knows is shared not showing up reads as
        // "session offline" rather than "wsx forgot it".
        let msg = if list.records.is_empty() {
            format!("no shared workspaces on {}", list.host_name)
        } else {
            format!("no live sessions on {}", list.host_name)
        };
        f.render_widget(Paragraph::new(msg).style(theme.dim_style()), body_area);
    } else {
        // Three aligned columns: agent | <glyph> #<num> branch | repo/workspace.
        // The branch cell keeps the dashboard's lifecycle glyph + color (dim when
        // status is unknown / no PR) and gains the PR number when known. No
        // liveness marker: `remote_rows` is attach-only, so every row is alive.
        //
        // The branch prefix (glyph + `#<num>`) is kept separate from the branch
        // name so truncation trims the *name*, never the status prefix.
        let prefixes: Vec<String> = rows
            .iter()
            .map(|r| {
                let glyph = crate::ui::theme::branch_glyph(r.lifecycle, nerd_fonts);
                match r.pr_number {
                    Some(n) => format!("{glyph} #{n} "),
                    None => format!("{glyph} "),
                }
            })
            .collect();
        let ws_cells: Vec<String> = rows
            .iter()
            .map(|r| format!("{}/{}", r.repo, r.workspace))
            .collect();

        // Desired column widths derive from content (capped) so rows line up,
        // then shrink to fit the panel. Layout per row:
        // indent(1) + agent + gap(2) + branch + gap(2) + ws.
        let agent_desired = rows
            .iter()
            .map(|r| r.label.chars().count())
            .max()
            .unwrap_or(1)
            .clamp(1, 14);
        let ws_desired = ws_cells
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(1)
            .clamp(1, 34);
        let branch_desired = rows
            .iter()
            .zip(&prefixes)
            .map(|(r, p)| p.chars().count() + r.branch.chars().count())
            .max()
            .unwrap_or(1);
        let budget = (body_area.width as usize).saturating_sub(1 + 2 + 2);
        let (agent_w, branch_w, ws_w) =
            fit_columns(budget, agent_desired, branch_desired, ws_desired);

        let mut lines: Vec<Line> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let branch_style = theme
                .lifecycle_style(row.lifecycle)
                .unwrap_or_else(|| theme.dim_style());
            // Truncate the branch *name* to the room left after the prefix, so the
            // glyph and `#<num>` always survive even in a narrow panel.
            let prefix_w = prefixes[i].chars().count();
            let name = truncate(row.branch, branch_w.saturating_sub(prefix_w));
            let branch_cell = truncate_pad(&format!("{}{name}", prefixes[i]), branch_w);
            let mut spans = vec![
                Span::raw(format!(" {}  ", truncate_pad(row.label, agent_w))),
                Span::styled(branch_cell, branch_style),
                Span::raw(format!("  {}", truncate_pad(&ws_cells[i], ws_w))),
            ];
            // Selected row: tint only the background so the lifecycle color and
            // the neutral spans stay readable — the same `selected_bg_style`
            // patch the dashboard list uses (a full `selected_style` would flatten
            // the branch color).
            if i == selected {
                let bg = theme.selected_bg_style();
                for s in &mut spans {
                    s.style = s.style.patch(bg);
                }
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), body_area);
    }

    if let (Some(area), Some(text)) = (notice_area, notice) {
        f.render_widget(Paragraph::new(text).style(theme.dim_style()), area);
    }

    f.render_widget(
        Paragraph::new("[\u{2191}/\u{2193}] move   [enter] attach   [r] refresh   [esc] close")
            .style(theme.dim_style()),
        footer_area,
    );
}

/// Shrink desired column widths so the composed row fits `budget` (the panel's
/// inner width minus the fixed indent + gaps). Sacrifices workspace first, then
/// the branch, then the agent — keeping the agent (row identifier) and branch
/// (primary data, carrying the PR #) legible longest. Guarantees the returned
/// widths sum to at most `budget`, so the row never overflows and gets clipped
/// by Ratatui. In the common wide-panel case everything fits and the desired
/// widths pass through unchanged.
fn fit_columns(budget: usize, agent: usize, branch: usize, ws: usize) -> (usize, usize, usize) {
    // Indexed [agent, branch, ws].
    let mut w = [agent, branch, ws];
    let mut total: usize = w.iter().sum();
    if total <= budget {
        return (w[0], w[1], w[2]);
    }
    // Sacrifice order and per-column soft floors that keep each column
    // meaningful. Trim to the floors first; if a very narrow panel still
    // overflows, trim below them in the same order so the row always fits.
    let order = [2usize, 1, 0]; // ws, then branch, then agent
    let floors = [3usize, 6, 4]; // agent, branch, ws
    for &i in &order {
        if total <= budget {
            break;
        }
        let give = (total - budget).min(w[i].saturating_sub(floors[i]));
        w[i] -= give;
        total -= give;
    }
    for &i in &order {
        if total <= budget {
            break;
        }
        let give = (total - budget).min(w[i]);
        w[i] -= give;
        total -= give;
    }
    (w[0], w[1], w[2])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn list_with_rows() -> RemoteList {
        RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                agents: vec![
                    SharedAgentRecord {
                        label: "claude".into(),
                        agent: "claude".into(),
                        tmux_session: Some("wsx-r-w".into()),
                        alive: true,
                    },
                    SharedAgentRecord {
                        label: "codex#2".into(),
                        agent: "codex".into(),
                        tmux_session: None,
                        alive: false,
                    },
                ],
                lifecycle: None,
                pr_number: None,
            }],
        }
    }

    fn render_to_string(list: &RemoteList, selected: usize, notice: Option<&str>) -> String {
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
        term.draw(|f| {
            render_remote_workspace_list(f, f.area(), list, selected, notice, &theme, false);
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Render one shared workspace at `lifecycle` and return the foreground
    /// color of the first cell of its branch glyph — the picker's analog of the
    /// dashboard's branch coloring. Used to pin PR-status colors.
    fn branch_glyph_color(
        lifecycle: Option<crate::git::forge::BranchLifecycle>,
        nerd_fonts: bool,
    ) -> ratatui::style::Color {
        use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
        let theme = Theme::wsx();
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "feature".into(),
                worktree_path: "/x".into(),
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: Some("wsx-r-w".into()),
                    alive: true,
                }],
                lifecycle,
                pr_number: None,
            }],
        };
        let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
        term.draw(|f| {
            render_remote_workspace_list(f, f.area(), &list, usize::MAX, None, &theme, nerd_fonts);
        })
        .unwrap();
        let buf = term.backend().buffer();
        // Find the branch glyph cell: the first cell of the row's branch span.
        // The row is "  r/w  <glyph> feature  ...", so scan for the glyph that
        // `branch_glyph` would have emitted and read its fg.
        let glyph = crate::ui::theme::branch_glyph(lifecycle, nerd_fonts);
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                if buf[(x, y)].symbol() == glyph {
                    return buf[(x, y)].style().fg.unwrap();
                }
            }
        }
        panic!("branch glyph {glyph:?} not found in render");
    }

    #[test]
    fn shows_host_and_only_live_agent_rows() {
        // `list_with_rows` carries one live (`claude`) and one dead (`codex#2`)
        // agent. The picker is attach-only, so only the live row is drawn.
        let list = list_with_rows();
        let text = render_to_string(&list, 0, None);
        assert!(text.contains("mini"), "host name missing:\n{text}");
        assert!(text.contains("r/w"), "repo/workspace missing:\n{text}");
        assert!(text.contains("claude"), "alive agent row missing:\n{text}");
        assert!(
            !text.contains("codex#2"),
            "dead agent row must be hidden:\n{text}"
        );
        // The liveness marker was dropped: the picker is attach-only, so every
        // row is alive and a per-row ●/✗ carried no information.
        assert!(
            !text.contains('\u{25CF}') && !text.contains('\u{2717}'),
            "no liveness marker should render:\n{text}"
        );
    }

    #[test]
    fn all_dead_records_show_no_live_sessions_message() {
        // The host has a shared workspace, but its only agent's session is
        // dead. After attach-only filtering there are no rows — the message
        // must explain the sessions are offline, not claim the host has no
        // shared workspaces at all.
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: None,
                    alive: false,
                }],
                lifecycle: None,
                pr_number: None,
            }],
        };
        let text = render_to_string(&list, 0, None);
        assert!(
            text.contains("no live sessions on mini"),
            "offline-sessions message missing:\n{text}"
        );
    }

    #[test]
    fn empty_records_shows_no_workspaces_message() {
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            records: vec![],
        };
        let text = render_to_string(&list, 0, None);
        assert!(
            text.contains("no shared workspaces on mini"),
            "empty-state message missing:\n{text}"
        );
        assert!(text.contains("esc"), "esc hint missing:\n{text}");
    }

    #[test]
    fn notice_renders_below_rows() {
        let list = list_with_rows();
        let text = render_to_string(&list, 1, Some("no live session to attach to"));
        assert!(
            text.contains("no live session to attach to"),
            "notice missing:\n{text}"
        );
    }

    #[test]
    fn branch_colored_by_pr_lifecycle_in_both_font_modes() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::wsx();
        // Same lifecycle → color mapping the dashboard uses, and it must hold
        // regardless of nerd-font mode (the glyph changes, the color doesn't).
        for nerd_fonts in [false, true] {
            assert_eq!(
                branch_glyph_color(Some(PrOpen), nerd_fonts),
                theme.ok_style().fg.unwrap(),
                "open PR must be the ok color (nerd_fonts={nerd_fonts})"
            );
            assert_eq!(
                branch_glyph_color(Some(PrConflicted), nerd_fonts),
                theme.warn_style().fg.unwrap(),
                "conflicted PR must be the warn color (nerd_fonts={nerd_fonts})"
            );
            assert_eq!(
                branch_glyph_color(Some(PrMerged), nerd_fonts),
                theme.merged_style().fg.unwrap(),
                "merged PR must be the merged color (nerd_fonts={nerd_fonts})"
            );
            assert_eq!(
                branch_glyph_color(Some(PrClosed), nerd_fonts),
                theme.err_style().fg.unwrap(),
                "closed PR must be the err color (nerd_fonts={nerd_fonts})"
            );
        }
    }

    #[test]
    fn unknown_and_no_pr_branches_render_dim() {
        use crate::git::forge::BranchLifecycle::*;
        let theme = Theme::wsx();
        let dim = theme.dim_style().fg.unwrap();
        // `None` (older host / gh unavailable) and NoPr/PrDraft have no
        // colorable status, so the branch falls back to dim — matching the
        // dashboard's `lifecycle_style(..).unwrap_or(dim)`.
        assert_eq!(branch_glyph_color(None, false), dim);
        assert_eq!(branch_glyph_color(Some(NoPr), false), dim);
        assert_eq!(branch_glyph_color(Some(PrDraft), true), dim);
    }

    /// Build a list of shared workspaces with varying branch lengths / PR
    /// numbers, one live agent each, for layout assertions.
    fn layout_list() -> RemoteList {
        use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
        let mk = |branch: &str, num: Option<u32>, ws: &str| SharedWorkspaceRecord {
            repo: "repo".into(),
            workspace: ws.into(),
            branch: branch.into(),
            worktree_path: "/x".into(),
            agents: vec![SharedAgentRecord {
                label: "claude".into(),
                agent: "claude".into(),
                tmux_session: Some(format!("wsx-{ws}")),
                alive: true,
            }],
            lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
            pr_number: num,
        };
        RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            records: vec![
                mk("short", Some(42), "alpha"),
                mk("a-much-longer-branch-name", Some(2087), "beta"),
            ],
        }
    }

    #[test]
    fn pr_number_renders_next_to_branch() {
        let text = render_to_string(&layout_list(), usize::MAX, None);
        assert!(text.contains("#42 short"), "pr #42 next to branch:\n{text}");
        assert!(
            text.contains("#2087 a-much-longer-branch-name"),
            "pr #2087 next to branch:\n{text}"
        );
    }

    #[test]
    fn no_pr_number_omits_the_hash_prefix() {
        use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            records: vec![SharedWorkspaceRecord {
                repo: "repo".into(),
                workspace: "alpha".into(),
                branch: "feature".into(),
                worktree_path: "/x".into(),
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: Some("wsx-a".into()),
                    alive: true,
                }],
                lifecycle: None,
                pr_number: None,
            }],
        };
        let text = render_to_string(&list, usize::MAX, None);
        assert!(text.contains("feature"), "branch missing:\n{text}");
        assert!(
            !text.contains('#'),
            "no #num when pr_number is None:\n{text}"
        );
    }

    #[test]
    fn columns_align_across_rows() {
        // Despite different branch lengths, the branch column and the
        // repo/workspace column must start at the same x on every row.
        let text = render_to_string(&layout_list(), usize::MAX, None);
        let lines: Vec<&str> = text.lines().collect();
        let r1 = lines.iter().find(|l| l.contains("repo/alpha")).unwrap();
        let r2 = lines.iter().find(|l| l.contains("repo/beta")).unwrap();
        // Branch column (glyph is ⎇ in plain-font render) aligns.
        assert_eq!(
            r1.find('\u{2387}'),
            r2.find('\u{2387}'),
            "branch column not aligned:\n{text}"
        );
        // Workspace column aligns.
        assert_eq!(
            r1.find("repo/alpha"),
            r2.find("repo/beta"),
            "workspace column not aligned:\n{text}"
        );
    }

    #[test]
    fn fit_columns_passes_through_when_it_fits() {
        // Wide budget: desired widths are returned unchanged.
        assert_eq!(fit_columns(100, 6, 33, 10), (6, 33, 10));
    }

    #[test]
    fn fit_columns_sacrifices_ws_then_branch_then_agent_and_always_fits() {
        // Progressively tighter budgets. The sum must never exceed the budget,
        // and columns shrink in order: ws first, then branch, then agent.
        for budget in 0..=60 {
            let (a, b, w) = fit_columns(budget, 6, 33, 10);
            assert!(
                a + b + w <= budget || budget == 0,
                "overflow at budget={budget}: {a}+{b}+{w}"
            );
            // ws never grows past agent's territory: once ws is at its floor,
            // the branch takes the next cut before agent does.
            if a < 6 {
                // agent only shrinks after branch has hit its floor (6).
                assert!(
                    b <= 6,
                    "agent shrank before branch at budget={budget}: b={b}"
                );
            }
        }
    }

    #[test]
    fn narrow_panel_keeps_pr_prefix_and_does_not_overflow() {
        // At a tight width the branch name truncates but the glyph + `#<num>`
        // prefix survive, and no row overflows the panel (which would clip).
        let list = layout_list();
        let theme = Theme::wsx();
        let width: u16 = 44;
        let mut term = Terminal::new(TestBackend::new(width, 12)).unwrap();
        term.draw(|f| {
            render_remote_workspace_list(f, f.area(), &list, usize::MAX, None, &theme, false);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let lines: Vec<String> = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect();
        let joined = lines.join("\n");
        // The PR-number prefix is preserved despite the tight width (the branch
        // *name* truncates instead of the glyph/#num).
        assert!(joined.contains("#42"), "pr #42 prefix dropped:\n{joined}");
        assert!(
            joined.contains("#2087"),
            "pr #2087 prefix dropped:\n{joined}"
        );
        // The workspace column is the rightmost and the first thing an
        // overflowing row would clip; its (truncated) presence proves the row
        // fits and `fit_columns` shrank rather than letting Ratatui clip.
        assert!(
            joined.contains("rep"),
            "workspace column clipped off a too-wide row:\n{joined}"
        );
    }
}
