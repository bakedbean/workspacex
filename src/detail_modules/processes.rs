//! Processes module. Shows the running processes attached to the
//! selected workspace (capped at 6, scaled to procs count).

use crate::detail_modules::{DetailContext, DetailModule};

pub struct Processes;

impl DetailModule for Processes {
    fn id(&self) -> &'static str {
        "processes"
    }
    fn title(&self) -> &'static str {
        "PROCESSES"
    }
    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
        build_lines(ctx, width)
    }
}

fn build_lines(ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>> {
    crate::ui::dashboard::detail::build_processes(ctx.procs, ctx.theme, width as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::proc::ProcInfo;
    use crate::detail_modules::tests_helpers::stub_context;
    use std::path::PathBuf;

    fn proc(pid: i32, cmd: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid: 0,
            command: cmd.into(),
            cmdline: String::new(),
            cwd: PathBuf::from("/"),
            listening: false,
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
