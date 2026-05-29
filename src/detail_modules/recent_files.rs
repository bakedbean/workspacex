//! Recent files module. Shows files the agent has recently edited
//! within the workspace, with per-file diff stats.

use crate::detail_modules::{DetailContext, DetailModule};

pub struct RecentFiles;

impl DetailModule for RecentFiles {
    fn id(&self) -> &'static str {
        "recent_files"
    }
    fn title(&self) -> &'static str {
        "RECENT FILES"
    }
    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
        crate::ui::dashboard::detail::build_recent_files(
            ctx.events,
            ctx.diff_per_file,
            &ctx.workspace.worktree_path,
            ctx.theme,
            width as usize,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;

    #[test]
    fn id_is_recent_files() {
        assert_eq!(RecentFiles.id(), "recent_files");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(RecentFiles.title(), "RECENT FILES");
    }

    #[test]
    fn lines_empty_events_returns_one_dash_line() {
        let ctx = stub_context();
        let out = RecentFiles.lines(&ctx, 40);
        assert_eq!(out.len(), 1, "empty state should emit one '—' line");
    }
}
