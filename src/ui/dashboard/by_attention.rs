//! By-attention view: drops repo grouping and sorts every workspace
//! into urgency sections.

use crate::ui::dashboard::row::{self, RowInputs};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::ListItem;

/// One flat row with the repo carried alongside so the name column can
/// render `<repo>/<workspace>`.
#[derive(Debug, Clone)]
pub struct FlatRow {
    pub repo_name: String,
    pub row: RowInputs,
}

#[derive(Debug, Clone)]
pub struct QuietRepo {
    pub name: String,
    pub path: String,
    pub workspace_count: usize,
    pub all_idle: bool,
}

#[derive(Debug, Clone)]
pub struct AttentionData {
    pub needs_attention: Vec<FlatRow>,
    pub working: Vec<FlatRow>,
    pub recent: Vec<FlatRow>,
    pub idle: Vec<FlatRow>,
    pub quiet_repos: Vec<QuietRepo>,
}

pub fn partition(rows: Vec<FlatRow>, quiet_repos: Vec<QuietRepo>) -> AttentionData {
    let mut needs = Vec::new();
    let mut working = Vec::new();
    let mut recent = Vec::new();
    let mut idle = Vec::new();
    for r in rows {
        match r.row.status {
            Status::Question | Status::Stalled | Status::Waiting => needs.push(r),
            Status::Thinking => working.push(r),
            Status::Complete => recent.push(r),
            Status::Idle | Status::Detached => idle.push(r),
        }
    }
    // Within NEEDS ATTENTION: priority desc, then most-recent first
    // (small `ago_secs` first; `None` treated as oldest).
    needs.sort_by(|a, b| {
        b.row
            .status
            .priority()
            .cmp(&a.row.status.priority())
            .then_with(|| ago_key(a.row.ago_secs).cmp(&ago_key(b.row.ago_secs)))
    });
    // WORKING / RECENT / IDLE: most-recent first by `ago_secs`.
    for section in [&mut working, &mut recent, &mut idle] {
        section.sort_by_key(|a| ago_key(a.row.ago_secs));
    }
    AttentionData {
        needs_attention: needs,
        working,
        recent,
        idle,
        quiet_repos,
    }
}

/// Sort key for "most recent first". Smaller `ago_secs` ⇒ more recent
/// ⇒ sorts earlier. `None` (no last-activity timestamp) is treated as
/// oldest so it falls to the bottom of its section.
fn ago_key(ago_secs: Option<u64>) -> u64 {
    ago_secs.unwrap_or(u64::MAX)
}

fn section_header(
    label: &str,
    count: usize,
    meta: Option<&str>,
    color: Style,
    width: usize,
    theme: &Theme,
) -> Line<'static> {
    let count_str = format!("  {count} sessions");
    let meta_str = meta.unwrap_or("");
    let label_span = Span::styled(label.to_string(), color.add_modifier(Modifier::BOLD));
    let count_span = Span::styled(count_str.clone(), theme.dim_style());
    let used = label.chars().count() + count_str.chars().count() + meta_str.chars().count();
    let rule = width.saturating_sub(used + 3).max(1);
    let mut spans = vec![
        label_span,
        count_span,
        Span::raw(" ".to_string()),
        Span::styled("─".repeat(rule), theme.dim_style()),
        Span::raw(" ".to_string()),
    ];
    if !meta_str.is_empty() {
        spans.push(Span::styled(
            meta_str.to_string(),
            Style::default().fg(theme.path),
        ));
    }
    Line::from(spans)
}

fn quiet_line(q: &QuietRepo, width: usize, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled("▎".to_string(), theme.dim_style()),
        Span::raw("  ·  ".to_string()),
        Span::styled(
            truncate_pad(&q.name, 18),
            Style::default().fg(theme.dim).add_modifier(Modifier::BOLD),
        ),
        Span::styled(truncate_pad(&q.path, 36), theme.dim_style()),
    ];
    let suffix = if q.workspace_count == 0 {
        "no workspaces · press n to create".to_string()
    } else {
        format!("{} idle", q.workspace_count)
    };
    spans.push(Span::styled(suffix, theme.dim_style()));
    let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
    if width > used {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    Line::from(spans)
}

/// Truncate `s` to fit `target` chars (replacing the last char with `…`
/// when over) and right-pad with spaces when under.
fn truncate_pad(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len > target && target > 0 {
        let mut out: String = s.chars().take(target - 1).collect();
        out.push('…');
        out
    } else if len < target {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(target - len));
        out
    } else {
        s.to_string()
    }
}

fn flat_row_line(
    fr: &FlatRow,
    widths: row::ColumnWidths,
    tick: u32,
    theme: &Theme,
    width: usize,
) -> Line<'static> {
    // Reuse the same composer but rewrite the name field to "<repo>/<name>"
    // so we keep alignment math centralized. The composer's name column
    // truncates; we leave that as-is for v1.
    let mut adjusted = fr.row.clone();
    adjusted.name = format!("{}/{}", fr.repo_name, fr.row.name);
    row::render(&adjusted, widths, tick, theme, width)
}

pub fn render_list(
    data: &AttentionData,
    widths: row::ColumnWidths,
    tick: u32,
    width: usize,
    theme: &Theme,
) -> Vec<ListItem<'static>> {
    let mut items: Vec<ListItem<'static>> = Vec::new();
    if !data.needs_attention.is_empty() {
        items.push(ListItem::new(section_header(
            "◆ NEEDS ATTENTION",
            data.needs_attention.len(),
            Some("sorted by urgency"),
            theme.status_style(Status::Question),
            width,
            theme,
        )));
        for r in &data.needs_attention {
            items.push(ListItem::new(flat_row_line(r, widths, tick, theme, width)));
        }
    }
    if !data.working.is_empty() {
        items.push(ListItem::new(section_header(
            "● WORKING",
            data.working.len(),
            Some("live"),
            theme.status_style(Status::Thinking),
            width,
            theme,
        )));
        for r in &data.working {
            items.push(ListItem::new(flat_row_line(r, widths, tick, theme, width)));
        }
    }
    if !data.recent.is_empty() {
        items.push(ListItem::new(section_header(
            "✓ RECENT",
            data.recent.len(),
            None,
            theme.status_style(Status::Complete),
            width,
            theme,
        )));
        for r in &data.recent {
            items.push(ListItem::new(flat_row_line(r, widths, tick, theme, width)));
        }
    }
    if !data.idle.is_empty() {
        items.push(ListItem::new(section_header(
            "  IDLE",
            data.idle.len(),
            None,
            Style::default().fg(theme.path),
            width,
            theme,
        )));
        for r in &data.idle {
            items.push(ListItem::new(flat_row_line(r, widths, tick, theme, width)));
        }
    }
    if !data.quiet_repos.is_empty() {
        items.push(ListItem::new(section_header(
            "  QUIET REPOS",
            data.quiet_repos.len(),
            None,
            Style::default().fg(theme.path),
            width,
            theme,
        )));
        for q in &data.quiet_repos {
            items.push(ListItem::new(quiet_line(q, width, theme)));
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::DiffStats;
    use crate::ui::dashboard::column_content::{ColumnEmphasis, RowColumn};
    use crate::ui::dashboard::fixture;

    fn make_rows() -> Vec<FlatRow> {
        let repos = fixture::repos();
        let mut out = Vec::new();
        for r in &repos {
            for (i, w) in r.workspaces.iter().enumerate() {
                out.push(FlatRow {
                    repo_name: r.name.clone(),
                    row: RowInputs {
                        agent: crate::pty::session::AgentKind::Claude,
                        status: w.status,
                        name: w.name.clone(),
                        branch: w.branch.clone(),
                        procs: w.procs,
                        diff: Some(DiffStats {
                            added: w.diff_added,
                            removed: w.diff_removed,
                        }),
                        column: w.last_message.clone().map(|t| RowColumn {
                            text: t,
                            emphasis: ColumnEmphasis::Dim,
                        }),
                        ago_secs: w.ago_secs,
                        selected: false,
                        yolo: false,
                        setup_failed: false,
                        shared: false,
                        shared_active: false,
                        lifecycle: None,
                        nerd_fonts: false,
                        workspace_id: crate::data::store::WorkspaceId(i as i64),
                        has_multi_pane_layout: false,
                    },
                });
            }
        }
        out
    }

    fn make_quiet() -> Vec<QuietRepo> {
        let repos = fixture::repos();
        repos
            .iter()
            .filter(|r| {
                r.workspaces.is_empty()
                    || r.workspaces
                        .iter()
                        .all(|w| matches!(w.status, Status::Idle))
            })
            .map(|r| QuietRepo {
                name: r.name.clone(),
                path: r.path.clone(),
                workspace_count: r.workspaces.len(),
                all_idle: !r.workspaces.is_empty(),
            })
            .collect()
    }

    #[test]
    fn partition_sorts_attention_by_priority() {
        let rows = make_rows();
        let quiet = make_quiet();
        let data = partition(rows, quiet);
        // theme-tokens (stalled) > anything else in needs.
        assert_eq!(data.needs_attention[0].row.name, "theme-tokens");
        // The next is question-statuses, then waiting.
        let next = &data.needs_attention[1].row.status;
        assert_eq!(*next, Status::Question);
    }

    #[test]
    fn partition_sorts_working_recent_idle_by_recency() {
        // RECENT contains brave-cedar (8m), tech-stack-question (34s),
        // and rate-limit (1h). Most-recent-first should yield 34s, 8m, 1h.
        let rows = make_rows();
        let quiet = make_quiet();
        let data = partition(rows, quiet);
        let recent_names: Vec<&str> = data.recent.iter().map(|r| r.row.name.as_str()).collect();
        assert_eq!(
            recent_names,
            vec!["tech-stack-question", "brave-cedar", "rate-limit"],
        );
        // WORKING: recipe-importer (11s) is newer than quiet-fennel (4s)?
        // No — quiet-fennel @ 4s is more recent than recipe-importer @ 11s.
        let working_names: Vec<&str> = data.working.iter().map(|r| r.row.name.as_str()).collect();
        assert_eq!(working_names, vec!["quiet-fennel", "recipe-importer"]);
    }

    #[test]
    fn partition_breaks_priority_ties_with_recency() {
        // Two QUESTION rows: repo-overview (29s) and driver-map-v2 (3m).
        // Same priority → 29s should come first.
        let rows = make_rows();
        let quiet = make_quiet();
        let data = partition(rows, quiet);
        let question_names: Vec<&str> = data
            .needs_attention
            .iter()
            .filter(|r| r.row.status == Status::Question)
            .map(|r| r.row.name.as_str())
            .collect();
        assert_eq!(question_names, vec!["repo-overview", "driver-map-v2"]);
    }

    #[test]
    fn quiet_line_truncates_long_repo_name_and_path() {
        let theme = Theme::wsx();
        let q = QuietRepo {
            name: "this-is-a-really-long-repo-name-that-exceeds-the-column".into(),
            path: "/home/eben/way/too/deep/a/path/for/the/quiet/repos/row".into(),
            workspace_count: 0,
            all_idle: false,
        };
        let line = quiet_line(&q, 120, &theme);
        // Name column is 18 chars, path is 36 chars. Both must end in `…`
        // when over.
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.contains("this-is-a-really-…"),
            "name truncated: {text:?}"
        );
        assert!(
            text.contains("/home/eben/way/too/deep/a/path/for/…"),
            "path truncated: {text:?}",
        );
    }

    #[test]
    fn section_headers_render_expected_labels() {
        let theme = Theme::wsx();
        for (label, color) in [
            ("◆ NEEDS ATTENTION", theme.status_style(Status::Question)),
            ("● WORKING", theme.status_style(Status::Thinking)),
            ("✓ RECENT", theme.status_style(Status::Complete)),
            ("  IDLE", Style::default().fg(theme.path)),
            ("  QUIET REPOS", Style::default().fg(theme.path)),
        ] {
            let line = section_header(label, 3, None, color, 120, &theme);
            let t: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
            assert!(t.starts_with(label), "label {label:?} missing in {t:?}");
            assert!(t.contains("3 sessions"), "count present in {t:?}");
        }
    }

    #[test]
    fn render_list_emits_expected_item_count_for_fixture() {
        let theme = Theme::wsx();
        let rows = make_rows();
        let quiet = make_quiet();
        let data = partition(rows, quiet);
        let items = render_list(&data, row::ColumnWidths::default(), 0, 120, &theme);
        // Headers + content per section. Fixture totals (cross-check
        // against fixture::repos()):
        //   needs: stalled(theme-tokens) + question(repo-overview, driver-map-v2)
        //          + waiting(list-virtualization, auth-refactor) = 5
        //   working: thinking(quiet-fennel, recipe-importer) = 2
        //   recent:  complete(brave-cedar, tech-stack-question, rate-limit) = 3
        //   idle:    ssk has 3 idle workspaces (not quiet — also has thinking+complete);
        //            api has 1 idle workspace (webhook-retry, not quiet — also has complete)
        //            → 4
        //   quiet:   frontend (empty), scp-api (empty) = 2
        // → 5 sections × header + (5+2+3+4+2) rows = 5 + 16 = 21
        assert_eq!(items.len(), 21);
    }

    #[test]
    fn flat_row_renders_repo_slash_workspace_in_name() {
        let theme = Theme::wsx();
        let row = FlatRow {
            repo_name: "wsx".into(),
            row: RowInputs {
                agent: crate::pty::session::AgentKind::Claude,
                status: Status::Question,
                name: "repo-overview".into(),
                branch: "bakedbean/repo-overview".into(),
                procs: 2,
                diff: Some(DiffStats {
                    added: 12,
                    removed: 3,
                }),
                column: Some(RowColumn {
                    text: "hi".into(),
                    emphasis: ColumnEmphasis::Dim,
                }),
                ago_secs: Some(29),
                selected: false,
                yolo: false,
                setup_failed: false,
                shared: false,
                shared_active: false,
                lifecycle: None,
                nerd_fonts: false,
                workspace_id: crate::data::store::WorkspaceId(0),
                has_multi_pane_layout: false,
            },
        };
        let line = flat_row_line(&row, row::ColumnWidths::default(), 0, &theme, 120);
        let t: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(t.contains("wsx/repo-overview"));
    }
}
