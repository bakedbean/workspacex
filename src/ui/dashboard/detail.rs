//! Bottom-pinned detail bar shown when a workspace is selected on the
//! dashboard. Renders header strip, three-column body, and an inline
//! reply input.
//!
//! See `docs/superpowers/specs/2026-05-24-dashboard-workspace-detail-design.md`.

/// Minimum rows the bar needs to render usefully (1 header + 1 rule + 3
/// body + 1 rule + 1 input + 1 spacing slack).
pub const MIN_HEIGHT: u16 = 8;

/// Compute the detail bar's preferred height given the total available
/// height. Targets ~22% of the area, clamped to `[MIN_HEIGHT, 14]`.
pub fn preferred_height(total_height: u16) -> u16 {
    let target = (u32::from(total_height) * 22 / 100) as u16;
    target.clamp(MIN_HEIGHT, 14)
}

use crate::events::WorkspaceEvents;
use crate::forge::BranchLifecycle;
use crate::git::DiffStats;
use crate::proc::ProcInfo;
use crate::store::{Repo, Workspace};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;

/// What `app.rs::draw` assembles for the detail bar. Borrowed for the
/// duration of a single draw call.
#[derive(Debug)]
pub struct DetailInputs<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<String>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub reply_draft: &'a str,
    pub reply_focused: bool,
    /// True once the workspace's JSONL has been scanned at least once
    /// (`workspace_events_scanned` on `App`). When false, SESSION
    /// SUMMARY and RECENT CHAT show `loading…` placeholders instead
    /// of derived content.
    pub events_scanned: bool,
}

/// Render the detail bar into `area`. No-op when `area.height < MIN_HEIGHT`
/// (caller is expected to fall back to a condensed banner — see
/// `app.rs::draw`).
pub fn render(f: &mut Frame, area: Rect, _inputs: &DetailInputs<'_>, _theme: &Theme) {
    if area.height == 0 || area.height < MIN_HEIGHT {
        return;
    }
    // Real rendering arrives in subsequent tasks.
    let _ = f;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::status::Status;
    use crate::ui::theme::Theme;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_to_text(inputs: &DetailInputs<'_>, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|f| {
                let theme = Theme::wsx();
                render(f, Rect::new(0, 0, w, h), inputs, &theme);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        let mut s = String::new();
        for y in 0..h {
            for x in 0..w {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    fn seed_workspace() -> (crate::store::Store, crate::store::Repo, crate::store::Workspace) {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ws",
                branch: "repo/ws",
                worktree_path: std::path::Path::new("/tmp/r/ws"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let repo = store
            .repos()
            .unwrap()
            .into_iter()
            .find(|r| r.id == repo_id)
            .unwrap();
        let ws = store
            .workspaces(repo_id)
            .unwrap()
            .into_iter()
            .find(|w| w.id == id)
            .unwrap();
        (store, repo, ws)
    }

    #[test]
    fn render_into_zero_area_is_a_noop() {
        // Sanity: rendering into a zero-height area must not panic.
        let backend = TestBackend::new(80, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let (_store, repo, ws) = seed_workspace();
        let result = terminal.draw(|f| {
            let theme = Theme::wsx();
            let inputs = DetailInputs {
                repo: &repo,
                workspace: &ws,
                events: None,
                procs: &[],
                diff: None,
                lifecycle: None,
                pr_title: None,
                pr_number: None,
                status: Status::Idle,
                ago_secs: None,
                reply_draft: "",
                reply_focused: false,
                events_scanned: false,
            };
            render(f, Rect::new(0, 0, 80, 0), &inputs, &theme);
        });
        assert!(result.is_ok());
    }

    #[test]
    fn preferred_height_clamps_to_min_on_short_terminal() {
        // 22% of 20 = 4 → clamps up to MIN_HEIGHT (8).
        assert_eq!(preferred_height(20), MIN_HEIGHT);
    }

    #[test]
    fn preferred_height_returns_22_percent_for_typical_terminal() {
        // 22% of 50 = 11 → within range.
        assert_eq!(preferred_height(50), 11);
    }

    #[test]
    fn preferred_height_clamps_to_14_on_tall_terminal() {
        // 22% of 100 = 22 → clamps down to 14.
        assert_eq!(preferred_height(100), 14);
    }

    #[test]
    fn preferred_height_handles_zero_height() {
        // 22% of 0 = 0 → clamps up to MIN_HEIGHT.
        assert_eq!(preferred_height(0), MIN_HEIGHT);
    }
}
