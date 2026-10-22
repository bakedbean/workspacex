//! Renderer for `Modal::RemoteWorkspaceList`. Mirrors `process_list.rs`'s
//! and `updates_panel.rs`'s structure: a dedicated fn reading live App
//! state (here, `app.remote_list`), called directly from `app/render.rs`'s
//! `draw()` because the floating panel needs the panel-frame + notice-row
//! idiom that the generic `render()` dispatcher doesn't provide.

use super::*;
use crate::app::{RemoteList, remote_rows};
use crate::ui::text::{display_width, fit_tags, truncate, truncate_pad};

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
    let rows = remote_rows(list);

    // A notice can carry a host path or a host's error: wrap it over up to
    // three lines rather than clip the part that says what to do.
    let has_notice = notice.is_some();
    let notice_lines = notice.map_or(0, |n| {
        let width = usize::from(crate::ui::drawer::body_width(area).max(1));
        display_width(n).div_ceil(width).clamp(1, 3) as u16
    });
    let inner = panel_frame(
        f,
        area,
        rows.len().max(1) + usize::from(notice_lines) + 1,
        if list.can_share {
            format!(" workspaces on {} ", list.host_name)
        } else {
            format!(" shared workspaces on {} ", list.host_name)
        },
        theme,
    );

    let constraints = if has_notice {
        vec![
            Constraint::Min(1),
            Constraint::Length(notice_lines),
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
        let msg = if list.records.is_empty() && list.can_share {
            format!("no workspaces on {}", list.host_name)
        } else if list.records.is_empty() {
            format!("no shared workspaces on {}", list.host_name)
        } else {
            format!("no live sessions on {}", list.host_name)
        };
        f.render_widget(Paragraph::new(msg).style(theme.dim_style()), body_area);
    } else {
        // Three aligned columns: agent | <glyph> #<num> branch #tags | repo/workspace.
        // The branch cell keeps the dashboard's lifecycle glyph + color (dim when
        // status is unknown / no PR) and gains the PR number when known. A
        // dormant row (no live session; Enter shares it) names its state in the
        // agent column instead and is drawn dim, all but its status prefix.
        //
        // The branch prefix (glyph + `#<num>`) is kept separate from the branch
        // name so truncation trims the *name* first; the prefix is cut only
        // when the column is narrower than the prefix itself. Tags follow the
        // name, dim, as on the dashboard row: only in room the whole name leaves.
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
        let agent_cells: Vec<&str> = rows
            .iter()
            .map(|r| match (r.attach_session(), r.shared) {
                (Some(_), _) => r.label,
                (None, true) => "stopped",
                (None, false) => "not shared",
            })
            .collect();
        let agent_desired = agent_cells
            .iter()
            .map(|l| display_width(l))
            .max()
            .unwrap_or(1)
            .clamp(1, 14);
        let ws_desired = ws_cells
            .iter()
            .map(|s| display_width(s))
            .max()
            .unwrap_or(1)
            .clamp(1, 34);
        let branch_desired = rows
            .iter()
            .zip(&prefixes)
            .map(|(r, p)| display_width(p) + display_width(r.branch))
            .max()
            .unwrap_or(1);
        let budget = (body_area.width as usize).saturating_sub(1 + 2 + 2);
        let (agent_w, branch_w, ws_w) =
            fit_columns(budget, agent_desired, branch_desired, ws_desired);
        // Tags are the least important cell: the branch column grows for them
        // only into room no other column wanted.
        let tagged_desired = rows
            .iter()
            .zip(&prefixes)
            .map(|(r, p)| {
                display_width(p)
                    + display_width(r.branch)
                    + display_width(&fit_tags(r.tags, usize::MAX))
            })
            .max()
            .unwrap_or(1);
        let spare = budget.saturating_sub(agent_w + branch_w + ws_w);
        let branch_w = branch_w + tagged_desired.saturating_sub(branch_w).min(spare);

        let mut lines: Vec<Line> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let dormant = row.attach_session().is_none();
            // The glyph + `#<num>` prefix keeps its PR-status color even on a
            // dormant row, so a stopped workspace's PR still reads at a glance;
            // only the branch name dims with the rest of the row.
            let prefix_style = theme
                .lifecycle_style(row.lifecycle)
                .unwrap_or_else(|| theme.dim_style());
            let name_style = if dormant {
                theme.dim_style()
            } else {
                prefix_style
            };
            let rest_style = if dormant {
                theme.dim_style()
            } else {
                Style::default()
            };
            // Truncate the branch *name* to the room left after the prefix, so the
            // glyph and `#<num>` survive unless the column can't fit even them.
            let prefix = truncate(&prefixes[i], branch_w);
            let name_room = branch_w.saturating_sub(display_width(&prefix));
            let name = truncate(row.branch, name_room);
            let name_w = display_width(&name);
            let tags = if display_width(row.branch) <= name_room {
                fit_tags(row.tags, name_room - name_w)
            } else {
                String::new()
            };
            let pad = " ".repeat(name_room - name_w - display_width(&tags));
            let mut spans = vec![
                Span::styled(
                    format!(" {}  ", truncate_pad(agent_cells[i], agent_w)),
                    rest_style,
                ),
                Span::styled(prefix, prefix_style),
                Span::styled(name, name_style),
                Span::styled(tags, theme.row_dim_style()),
                Span::styled(
                    format!("{pad}  {}", truncate_pad(&ws_cells[i], ws_w)),
                    rest_style,
                ),
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
        f.render_widget(
            Paragraph::new(text)
                .style(theme.dim_style())
                .wrap(ratatui::widgets::Wrap { trim: true }),
            area,
        );
    }

    f.render_widget(
        Paragraph::new(if list.can_share {
            crate::ui::drawer::hint_line(
                &[
                    ("↑↓", "move"),
                    ("↵", "attach / share"),
                    ("n", "new"),
                    ("r", "refresh"),
                    ("esc", "close"),
                ],
                theme,
            )
        } else {
            crate::ui::drawer::hint_line(
                &[
                    ("↑↓", "move"),
                    ("↵", "attach"),
                    ("r", "refresh"),
                    ("esc", "close"),
                ],
                theme,
            )
        }),
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
            can_share: false,
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                shared: true,
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
                tags: vec![],
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

    #[test]
    fn a_shareable_host_lists_dormant_workspaces_by_state() {
        let mut list = list_with_rows();
        list.can_share = true;
        let dormant = |ws: &str, shared: bool| SharedWorkspaceRecord {
            repo: "r".into(),
            workspace: ws.into(),
            branch: format!("r/{ws}"),
            worktree_path: format!("/x/{ws}"),
            shared,
            agents: vec![SharedAgentRecord {
                label: "claude".into(),
                agent: "claude".into(),
                tmux_session: None,
                alive: false,
            }],
            lifecycle: None,
            pr_number: None,
            tags: vec![],
        };
        list.records.push(dormant("direct", false));
        list.records.push(dormant("idle", true));

        let text = render_to_string(&list, 0, None);

        assert!(text.contains(" workspaces on mini "), "{text}");
        assert!(!text.contains("shared workspaces on"), "{text}");
        let line = |ws: &str| {
            text.lines()
                .find(|l| l.contains(&format!("r/{ws} ")))
                .unwrap()
        };
        assert!(line("direct").contains("not shared"), "{text}");
        assert!(line("idle").contains("stopped"), "{text}");
        assert!(text.contains("↵  attach / share"), "{text}");
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
            can_share: false,
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "feature".into(),
                worktree_path: "/x".into(),
                shared: true,
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: Some("wsx-r-w".into()),
                    alive: true,
                }],
                lifecycle,
                pr_number: None,
                tags: vec![],
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
            can_share: false,
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                shared: true,
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: None,
                    alive: false,
                }],
                lifecycle: None,
                pr_number: None,
                tags: vec![],
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
            can_share: false,
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

    #[test]
    fn dormant_rows_keep_the_pr_status_color_on_the_number() {
        use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
        use crate::git::forge::BranchLifecycle::PrOpen;
        let theme = Theme::wsx();
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            can_share: true,
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "feature".into(),
                worktree_path: "/x".into(),
                shared: false,
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: None,
                    alive: false,
                }],
                lifecycle: Some(PrOpen),
                pr_number: Some(42),
                tags: vec![],
            }],
        };
        // Selected, so the row also carries the selection background: it must
        // tint without flattening either foreground.
        let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
        term.draw(|f| {
            render_remote_workspace_list(f, f.area(), &list, 0, None, &theme, false);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let bg = theme.selected_bg_style().bg;
        let fg_of = |needle: &str| {
            let first = needle.chars().next().unwrap().to_string();
            for y in 0..buf.area.height {
                let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
                if let Some(col) = line.find(needle) {
                    let x = line[..col].chars().count() as u16;
                    assert_eq!(buf[(x, y)].symbol(), first);
                    assert_eq!(
                        buf[(x, y)].style().bg,
                        bg,
                        "{needle:?} lost the selection bg"
                    );
                    return buf[(x, y)].style().fg.unwrap();
                }
            }
            panic!("{needle:?} not found in render");
        };
        assert_eq!(fg_of("#42"), theme.ok_style().fg.unwrap());
        assert_eq!(fg_of("feature"), theme.dim_style().fg.unwrap());
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
            shared: true,
            agents: vec![SharedAgentRecord {
                label: "claude".into(),
                agent: "claude".into(),
                tmux_session: Some(format!("wsx-{ws}")),
                alive: true,
            }],
            lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
            pr_number: num,
            tags: vec![],
        };
        RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            can_share: false,
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
            can_share: false,
            records: vec![SharedWorkspaceRecord {
                repo: "repo".into(),
                workspace: "alpha".into(),
                branch: "feature".into(),
                worktree_path: "/x".into(),
                shared: true,
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: Some("wsx-a".into()),
                    alive: true,
                }],
                lifecycle: None,
                pr_number: None,
                tags: vec![],
            }],
        };
        let text = render_to_string(&list, usize::MAX, None);
        assert!(text.contains("feature"), "branch missing:\n{text}");
        assert!(
            !text.contains('#'),
            "no #num when pr_number is None:\n{text}"
        );
    }

    /// `layout_list` with tags on its short-branch row.
    fn tagged_layout_list() -> RemoteList {
        let mut list = layout_list();
        list.records[0].tags = vec!["infra".into(), "wip".into()];
        list
    }

    #[test]
    fn tags_render_dim_after_the_branch_name() {
        let list = tagged_layout_list();
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(80, 20)).unwrap();
        term.draw(|f| {
            render_remote_workspace_list(f, f.area(), &list, 0, None, &theme, false);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let (x, y) = (0..buf.area.height)
            .find_map(|y| {
                let line: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
                line.find("short #infra #wip")
                    .map(|col| (line[..col].chars().count() as u16 + 6, y))
            })
            .expect("tags not drawn after the branch");
        let cell = &buf[(x, y)];
        assert_eq!(cell.symbol(), "#");
        assert_eq!(cell.style().fg, theme.row_dim_style().fg);
        // Selected row: the tags take the selection bg like every other span.
        assert_eq!(cell.style().bg, theme.selected_bg_style().bg);
    }

    #[test]
    fn tags_never_squeeze_another_column() {
        // Tags past the panel's spare room collapse to `+N` rather than cut
        // the workspace column or the other row's branch name.
        let mut list = tagged_layout_list();
        list.records[0].tags = (1..=9).map(|n| format!("tag-number-{n}")).collect();
        let text = render_to_string(&list, usize::MAX, None);
        assert!(text.contains("short #tag-number-1"), "{text}");
        assert!(text.contains(" +"), "overflow count missing:\n{text}");
        assert!(
            text.contains("repo/alpha") && text.contains("repo/beta"),
            "{text}"
        );
        assert!(text.contains("a-much-longer-branch-name"), "{text}");
        let lines: Vec<&str> = text.lines().collect();
        let r1 = lines.iter().find(|l| l.contains("repo/alpha")).unwrap();
        let r2 = lines.iter().find(|l| l.contains("repo/beta")).unwrap();
        assert_eq!(r1.find("repo/alpha"), r2.find("repo/beta"), "{text}");
    }

    #[test]
    fn a_truncated_branch_shows_no_tags() {
        let mut list = tagged_layout_list();
        list.records[0].branch = "a-branch-name-too-long-for-the-narrow-panel".into();
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(44, 12)).unwrap();
        term.draw(|f| {
            render_remote_workspace_list(f, f.area(), &list, usize::MAX, None, &theme, false);
        })
        .unwrap();
        let buf = term.backend().buffer();
        let text: String = (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .map(|p| buf[p].symbol().to_string())
            .collect();
        assert!(text.contains('…'), "{text}");
        assert!(!text.contains("#infra"), "{text}");
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
    fn wide_names_under_the_caps_get_columns_that_show_them_whole() {
        // Each name is 6 chars but 12 cells: columns sized in chars would
        // cut every one of them with `…` despite the room.
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "mini:".into(),
            can_share: false,
            records: vec![SharedWorkspaceRecord {
                repo: "倉庫".into(),
                workspace: "作業スペース".into(),
                branch: "機能ブランチ".into(),
                worktree_path: "/x".into(),
                shared: true,
                agents: vec![SharedAgentRecord {
                    label: "エージェント".into(),
                    agent: "claude".into(),
                    tmux_session: Some("wsx-a".into()),
                    alive: true,
                }],
                lifecycle: None,
                pr_number: None,
                tags: vec![],
            }],
        };
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(80, 12)).unwrap();
        term.draw(|f| {
            render_remote_workspace_list(f, f.area(), &list, usize::MAX, None, &theme, false);
        })
        .unwrap();
        let buf = term.backend().buffer();
        // Read each row as drawn: a wide char also covers the cell after it.
        let text = (0..buf.area.height)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < buf.area.width {
                    let symbol = buf[(x, y)].symbol();
                    row.push_str(symbol);
                    x += display_width(symbol).max(1) as u16;
                }
                row
            })
            .collect::<Vec<_>>()
            .join("\n");
        for name in ["エージェント", "機能ブランチ", "倉庫/作業スペース"] {
            assert!(text.contains(name), "{name} cut short:\n{text}");
        }
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
