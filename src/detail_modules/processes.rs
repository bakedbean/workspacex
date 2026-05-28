//! Processes module. Shows the running processes attached to the
//! selected workspace (capped at 6, scaled to procs count).

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct Processes;

impl DetailModule for Processes {
    fn id(&self) -> &'static str {
        "processes"
    }
    fn title(&self) -> &'static str {
        "PROCESSES"
    }
    fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Length(ctx.procs.len().clamp(1, 6) as u16)
    }
    fn lines(
        &self,
        ctx: &DetailContext<'_>,
        width: u16,
    ) -> Vec<ratatui::text::Line<'static>> {
        build_lines(ctx, width)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let lines = build_lines(ctx, area.width);
        frame.render_widget(Paragraph::new(lines), area);
    }
}

fn build_lines(
    ctx: &DetailContext<'_>,
    width: u16,
) -> Vec<ratatui::text::Line<'static>> {
    crate::ui::dashboard::detail::build_processes(
        ctx.procs,
        ctx.theme,
        width as usize,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;
    use crate::proc::ProcInfo;
    use std::path::PathBuf;

    fn proc(pid: i32, cmd: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid: 0,
            command: cmd.into(),
            cwd: PathBuf::from("/"),
        }
    }

    #[test]
    fn id_is_processes() {
        assert_eq!(Processes.id(), "processes");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(Processes.title(), "PROCESSES");
    }

    #[test]
    fn height_hint_zero_procs_returns_length_one() {
        let ctx = stub_context();
        assert_eq!(Processes.height_hint(&ctx), Constraint::Length(1));
    }

    #[test]
    fn height_hint_three_procs_returns_length_three() {
        let procs = vec![proc(1, "a"), proc(2, "b"), proc(3, "c")];
        let mut ctx = stub_context();
        ctx.procs = Box::leak(procs.into_boxed_slice());
        assert_eq!(Processes.height_hint(&ctx), Constraint::Length(3));
    }

    #[test]
    fn height_hint_ten_procs_capped_at_six() {
        let procs: Vec<ProcInfo> = (0..10).map(|i| proc(i, &format!("c{i}"))).collect();
        let mut ctx = stub_context();
        ctx.procs = Box::leak(procs.into_boxed_slice());
        assert_eq!(Processes.height_hint(&ctx), Constraint::Length(6));
    }

    #[test]
    fn lines_returns_one_line_per_proc() {
        let procs = vec![proc(1, "a"), proc(2, "b"), proc(3, "c")];
        let mut ctx = stub_context();
        ctx.procs = Box::leak(procs.into_boxed_slice());
        let out = Processes.lines(&ctx, 40);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn lines_zero_procs_returns_one_dash_line() {
        let ctx = stub_context();
        let out = Processes.lines(&ctx, 40);
        assert_eq!(out.len(), 1);
    }
}
