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
            Status::Idle => idle.push(r),
        }
    }
    needs.sort_by(|a, b| b.row.status.priority().cmp(&a.row.status.priority()));
    AttentionData {
        needs_attention: needs,
        working,
        recent,
        idle,
        quiet_repos,
    }
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
    let mut spans = vec![label_span, count_span, Span::raw(" ".to_string()),
        Span::styled("─".repeat(rule), theme.dim_style()),
        Span::raw(" ".to_string()),
    ];
    if !meta_str.is_empty() {
        spans.push(Span::styled(meta_str.to_string(), Style::default().fg(theme.path)));
    }
    Line::from(spans)
}

fn quiet_line(q: &QuietRepo, width: usize, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled("▎".to_string(), theme.dim_style()));
    spans.push(Span::raw("  ·  ".to_string()));
    let mut name_padded = q.name.clone();
    while name_padded.chars().count() < 18 { name_padded.push(' '); }
    spans.push(Span::styled(name_padded, Style::default().fg(theme.dim).add_modifier(Modifier::BOLD)));
    let mut path_padded = q.path.clone();
    while path_padded.chars().count() < 36 { path_padded.push(' '); }
    spans.push(Span::styled(path_padded, theme.dim_style()));
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
    use crate::ui::dashboard::fixture;
    use crate::git::DiffStats;

    fn make_rows() -> Vec<FlatRow> {
        let repos = fixture::repos();
        let mut out = Vec::new();
        for r in &repos {
            for (i, w) in r.workspaces.iter().enumerate() {
                out.push(FlatRow {
                    repo_name: r.name.clone(),
                    row: RowInputs {
                        status: w.status,
                        name: w.name.clone(),
                        branch: w.branch.clone(),
                        procs: w.procs,
                        diff: Some(DiffStats { added: w.diff_added, removed: w.diff_removed }),
                        last_message: w.last_message.clone(),
                        ago_secs: w.ago_secs,
                        selected: false,
                        yolo: false,
                        setup_failed: false,
                        lifecycle: None,
                        nerd_fonts: false,
                        workspace_id: crate::store::WorkspaceId(i as i64),
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
            .filter(|r| r.workspaces.is_empty() || r.workspaces.iter().all(|w| matches!(w.status, Status::Idle)))
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
                status: Status::Question,
                name: "repo-overview".into(),
                branch: "bakedbean/repo-overview".into(),
                procs: 2,
                diff: Some(DiffStats { added: 12, removed: 3 }),
                last_message: Some("hi".into()),
                ago_secs: Some(29),
                selected: false,
                yolo: false,
                setup_failed: false,
                lifecycle: None,
                nerd_fonts: false,
                workspace_id: crate::store::WorkspaceId(0),
            },
        };
        let line = flat_row_line(&row, row::ColumnWidths::default(), 0, &theme, 120);
        let t: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(t.contains("wsx/repo-overview"));
    }
}
