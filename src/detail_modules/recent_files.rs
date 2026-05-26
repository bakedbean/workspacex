//! Recent files module. Shows files the agent has recently edited
//! within the workspace, with per-file diff stats.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct RecentFiles;

impl DetailModule for RecentFiles {
    fn id(&self) -> &'static str {
        "recent_files"
    }
    fn title(&self) -> &'static str {
        "RECENT FILES"
    }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Min(3)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let lines = crate::ui::dashboard::detail::build_recent_files(
            ctx.events,
            ctx.diff_per_file,
            &ctx.workspace.worktree_path,
            ctx.theme,
            area.width as usize,
        );
        frame.render_widget(Paragraph::new(lines), area);
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
    fn height_hint_is_min_three() {
        let ctx = stub_context();
        assert_eq!(RecentFiles.height_hint(&ctx), Constraint::Min(3));
    }
}
