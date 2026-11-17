//! `wsx series show`: a workspace's series as text or JSON.

use crate::data::scm_cache::lifecycle_to_str;
use crate::data::series::Series;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PartView {
    pub part: u32,
    pub slug: String,
    pub branch: String,
    pub pr_number: Option<u32>,
    pub pr_state: Option<&'static str>,
    pub archived: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SeriesView {
    pub title: Option<String>,
    pub display_title: String,
    pub parts: Vec<PartView>,
}

impl From<&Series> for SeriesView {
    fn from(s: &Series) -> Self {
        Self {
            title: s.title.clone(),
            display_title: s.display_title().to_string(),
            parts: s
                .parts
                .iter()
                .map(|p| PartView {
                    part: p.part,
                    slug: p.slug.clone(),
                    branch: p.branch.clone(),
                    pr_number: p.pr_number,
                    pr_state: p.pr_lifecycle.map(lifecycle_to_str),
                    archived: p.archived_at.is_some(),
                })
                .collect(),
        }
    }
}

/// The title, then one aligned line per part: number, slug, branch, PR,
/// and `archived` for parts whose workspace is gone.
pub fn render_series(v: &SeriesView) -> String {
    let w = |f: fn(&PartView) -> usize| v.parts.iter().map(f).max().unwrap_or(0);
    let (num_w, slug_w, branch_w) = (
        w(|p| p.part.to_string().len()),
        w(|p| p.slug.chars().count()),
        w(|p| p.branch.chars().count()),
    );
    let pr = |p: &PartView| match (p.pr_number, p.pr_state) {
        (Some(n), Some(s)) => format!("#{n} {s}"),
        (Some(n), None) => format!("#{n}"),
        _ => String::new(),
    };
    let pr_w = v.parts.iter().map(|p| pr(p).len()).max().unwrap_or(0);
    let mut out = format!("{}\n", v.display_title);
    for p in &v.parts {
        let line = format!(
            "  {:>num_w$}  {:<slug_w$}  {:<branch_w$}  {:<pr_w$}  {}",
            p.part,
            p.slug,
            p.branch,
            pr(p),
            if p.archived { "archived" } else { "" },
        );
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::series::{Series, SeriesId, SeriesPart};
    use crate::data::store::{RepoId, WorkspaceId};
    use crate::git::forge::BranchLifecycle;

    fn part(n: u32, slug: &str, pr: Option<(u32, BranchLifecycle)>, archived: bool) -> SeriesPart {
        SeriesPart {
            series_id: SeriesId(1),
            part: n,
            workspace_id: WorkspaceId(n as i64),
            slug: slug.into(),
            branch: format!("x/{slug}"),
            pr_number: pr.map(|p| p.0),
            pr_lifecycle: pr.map(|p| p.1),
            archived_at: archived.then_some(5),
        }
    }

    fn series(title: Option<&str>) -> Series {
        Series {
            id: SeriesId(1),
            repo_id: RepoId(1),
            title: title.map(Into::into),
            parts: vec![
                part(
                    1,
                    "focus-view-declutter",
                    Some((466, BranchLifecycle::PrMerged)),
                    true,
                ),
                part(2, "bars", Some((469, BranchLifecycle::PrDraft)), false),
                part(3, "next", None, false),
            ],
        }
    }

    #[test]
    fn renders_title_then_parts_in_order() {
        let out = render_series(&SeriesView::from(&series(Some("Focus view"))));
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines[0], "Focus view");
        assert!(lines[1].contains("1") && lines[1].contains("focus-view-declutter"));
        assert!(lines[1].contains("#466 merged") && lines[1].ends_with("archived"));
        assert!(lines[2].contains("x/bars") && lines[2].contains("#469 draft"));
        assert!(!lines[2].contains("archived"));
        assert!(!lines[3].contains('#'), "no PR column text: {:?}", lines[3]);
        assert_eq!(lines.len(), 4);
    }

    #[test]
    fn untitled_series_heads_with_part_one_slug() {
        let view = SeriesView::from(&series(None));
        assert_eq!(view.title, None);
        assert_eq!(view.display_title, "focus-view-declutter");
        assert!(render_series(&view).starts_with("focus-view-declutter\n"));
    }

    #[test]
    fn columns_line_up() {
        let out = render_series(&SeriesView::from(&series(None)));
        let col = |l: &str| l.find("x/").unwrap();
        let lines: Vec<&str> = out.lines().skip(1).collect();
        assert!(lines.iter().all(|l| col(l) == col(lines[0])), "{out}");
    }
}
