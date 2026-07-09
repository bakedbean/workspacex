//! Renderer for `Modal::RemoteWorkspaceList`. Mirrors `process_list.rs`'s
//! and `updates_panel.rs`'s structure: a dedicated fn reading live App
//! state (here, `app.remote_list`), called directly from `app/render.rs`'s
//! `draw()` because the floating panel needs the panel-frame + notice-row
//! idiom that the generic `render()` dispatcher doesn't provide.

use super::*;
use crate::app::{RemoteList, remote_rows};

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
        let mut lines: Vec<Line> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            let marker = if row.alive { "\u{25CF}" } else { "\u{2717}" };
            // Branch cell mirrors the dashboard: a PR-lifecycle glyph plus the
            // branch name, colored by `lifecycle_style` (dim when the status is
            // unknown or has no PR). The rest of the row stays neutral.
            let glyph = crate::ui::theme::branch_glyph(row.lifecycle, nerd_fonts);
            let branch_style = theme
                .lifecycle_style(row.lifecycle)
                .unwrap_or_else(|| theme.dim_style());
            let mut spans = vec![
                Span::raw(format!("  {}/{}  ", row.repo, row.workspace)),
                Span::styled(format!("{glyph} {}", row.branch), branch_style),
                Span::raw(format!("  {}  {marker}", row.label)),
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
        assert!(text.contains('\u{25CF}'), "alive marker missing:\n{text}");
        assert!(
            !text.contains('\u{2717}'),
            "no dead marker should render when dead rows are hidden:\n{text}"
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
}
