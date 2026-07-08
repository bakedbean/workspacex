use crate::config::usage_window::UsageWindow;
use crate::data::store::RepoId;
use crate::git::forge::BranchLifecycle;
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::style::Modifier;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use std::collections::{HashMap, HashSet};

mod agents_panel;
mod archive;
mod process_list;
mod repo_settings;
mod updates_panel;
mod usage_picker;

// `render()` below dispatches the ArchiveRunning variant to this.
use archive::render_archive_steps;
// Panel renderers called from app::render via `crate::ui::modal::*`.
pub use agents_panel::render_agents_panel;
pub use process_list::render_process_list;
pub use repo_settings::render_repo_settings;
pub use updates_panel::{ordered_workspaces_for_panel, render_updates_panel};
pub use usage_picker::render_usage_window_picker;

/// Which phase of `workspace::archive_with_app` is currently running.
/// Used by `Modal::ArchiveRunning` to drive the per-step progress UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveStep {
    /// Phase 1: running the repo's archive script (if any).
    Script,
    /// Phase 2: `git worktree remove` — usually the slow one.
    RemoveWorktree,
    /// Phase 3: `git branch -D`.
    DeleteBranch,
    /// Phase 4: sqlite row + MCP entry cleanup.
    Cleanup,
}

#[derive(Debug, Clone)]
pub enum Modal {
    NewWorkspace {
        repo_id: RepoId,
        name_buffer: String,
        yolo: bool,
        shared: bool,
        agent: crate::pty::session::AgentKind,
    },
    ConfirmArchive {
        workspace_id: crate::data::store::WorkspaceId,
        name: String,
    },
    ConfirmShare {
        workspace_id: crate::data::store::WorkspaceId,
        name: String,
        /// `true` = converting to tmux-shared, `false` = converting to direct.
        to_shared: bool,
        /// Snapshot of how many instances currently have a running session,
        /// taken when the modal was opened (by `T`'s dashboard handler) —
        /// purely for the confirmation message; the actual restart in
        /// `toggle_workspace_shared` re-checks liveness at commit time.
        running_count: usize,
    },
    SetupRunning {
        cancel: tokio_util::sync::CancellationToken,
        progress: crate::data::progress::SharedProgress,
        started: std::time::Instant,
    },
    ArchiveRunning {
        step: ArchiveStep,
        /// Whether the repo has an archive script configured. Drives
        /// whether the Script row renders as in-progress/done or
        /// "(skipped)".
        script_present: bool,
    },
    Error {
        message: String,
    },
    UpdatesPanel {
        /// Index into the modal's ordered workspace list. Up/Down adjust
        /// it; Enter switches `app.view` to that workspace.
        selected: usize,
    },
    ProcessList {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize,
        /// `None` = list mode; `Some(buffer)` = the user is typing a command to run.
        input: Option<String>,
        /// Last launch result (success path or error), shown below the list.
        notice: Option<String>,
    },
    RepoSettings {
        repo_id: crate::data::store::RepoId,
        selected: usize,
    },
    AgentMissing {
        ws_id: crate::data::store::WorkspaceId,
        agent: crate::pty::session::AgentKind,
        binary: String,
    },
    AgentPicker {
        ws_id: crate::data::store::WorkspaceId,
        selected: usize,
        current: crate::pty::session::AgentKind,
    },
    AgentsPanel {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize, // index into AgentKind::ALL for the add-picker
    },
    UsageWindowPicker {
        /// Index into `UsageWindow::ALL` for the cursor selection. The current
        /// (applied) window is read separately from the store at render time.
        selected: usize,
    },
    /// Static reference card for the workspace-only actions
    /// (edit/term/diff/lazygit/chronox) — the ones that act only on a
    /// selected workspace. Carries no state — dismissed without side effects.
    WorkspaceActions,
    /// Browse the tmux-shared workspace listing fetched from a remote wsx
    /// host (`App::remote_list`). Landed here as a bare variant so
    /// `reconcile_remote_list` (Task 4) can open it; the real keybindings
    /// and list rendering come in Task 6. For now Esc closes it and it
    /// renders a placeholder via the generic `render()` below.
    RemoteWorkspaceList {
        selected: usize,
    },
    /// `H`-key picker over the configured shared hosts (`shared_hosts`
    /// setting), sorted by name. Self-contained snapshot like
    /// `AgentPicker` — `(name, dest)` pairs plus a cursor. Enter allocates
    /// a remote-fetch generation and swaps to `RemoteListLoading`.
    RemoteHostPicker {
        hosts: Vec<(String, String)>,
        selected: usize,
    },
    /// Shown while the background `fetch_shared_list` task for `host_name`
    /// is in flight. Esc closes it and clears `pending_remote_gen`, so the
    /// eventual (stale) reconcile no-ops via its gen guard instead of
    /// reopening a modal the user backed out of.
    RemoteListLoading {
        host_name: String,
    },
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(h),
            Constraint::Min(0),
        ])
        .split(area)[1];
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(w),
            Constraint::Min(0),
        ])
        .split(popup)[1]
}

/// Draw a centered, bordered modal box of size `w`×`h` (centered within
/// `area`) titled `title`: clears the region, paints the dim-styled border,
/// and returns the inner content area. Shared framing for the floating panel
/// renderers (updates, processes, repo settings, agents) so they look
/// identical; each caller lays its own body/footer split inside the returned
/// inner rect.
fn panel_frame<'a>(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    title: impl Into<Line<'a>>,
    theme: &Theme,
) -> Rect {
    let rect = centered(area, w, h);
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .style(theme.dim_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    inner
}

pub fn render(f: &mut Frame, area: Rect, modal: &Modal, tick: u32, theme: &Theme) {
    // UpdatesPanel and ProcessList are rendered by their dedicated
    // helpers directly from `draw()` because they need live App state.
    // This function should never be called with those variants; guard
    // defensively.
    if matches!(
        modal,
        Modal::UpdatesPanel { .. }
            | Modal::ProcessList { .. }
            | Modal::RepoSettings { .. }
            | Modal::AgentsPanel { .. }
            | Modal::UsageWindowPicker { .. }
    ) {
        return;
    }
    let rect = centered(area, 60, 14);
    f.render_widget(Clear, rect);
    let (title, body) = match modal {
        Modal::NewWorkspace {
            name_buffer,
            yolo,
            shared,
            agent,
            ..
        } => {
            let agent_label = agent.display_name();
            let shared_line = if *shared {
                "shared (tmux): on — ^s toggles\n"
            } else {
                "shared (tmux): off — ^s toggles\n"
            };
            (
                if *yolo {
                    "new workspace (permissive)"
                } else {
                    "new workspace"
                },
                format!(
                    "name: {name_buffer}\nagent: {agent_label}  [tab] toggle\n{shared_line}\n[enter] create   [esc] cancel"
                ),
            )
        }
        Modal::ConfirmArchive { name, .. } => (
            "archive workspace",
            format!("archive '{name}'?\n\n[y] yes   [n]/[esc] cancel"),
        ),
        Modal::ConfirmShare {
            name,
            to_shared,
            running_count,
            ..
        } => {
            let dest = if *to_shared {
                "shared (tmux)"
            } else {
                "direct (not tmux)"
            };
            let restart_note = match running_count {
                0 => "No running sessions to restart.".to_string(),
                1 => format!(
                    "This restarts 1 running session {} tmux (conversation resumes via --continue).",
                    if *to_shared { "inside" } else { "outside" }
                ),
                n => format!(
                    "This restarts {n} running session(s) {} tmux (conversation resumes via --continue).",
                    if *to_shared { "inside" } else { "outside" }
                ),
            };
            (
                "toggle sharing",
                format!(
                    "switch '{name}' to {dest}?\n\n{restart_note}\n\n[y] yes   [n]/[esc] cancel"
                ),
            )
        }
        Modal::SetupRunning {
            progress, started, ..
        } => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let (phase_label, tail) = match progress.lock() {
                Ok(p) => (p.phase().label(), p.recent(6)),
                Err(_) => ("Working", Vec::new()),
            };
            let secs = started.elapsed().as_secs();
            let elapsed = format!("{:02}:{:02}", secs / 60, secs % 60);
            let mut body = format!("  {frame} {phase_label}…   ({elapsed})\n\n");
            if tail.is_empty() {
                body.push_str("  (waiting for output…)\n");
            } else {
                for line in &tail {
                    body.push_str(&format!("  {}\n", truncate_to(line, 54)));
                }
            }
            body.push_str("\n  [esc] cancel");
            ("new workspace", body)
        }
        Modal::ArchiveRunning {
            step,
            script_present,
        } => {
            let body = render_archive_steps(*step, *script_present, tick);
            ("archive workspace", body)
        }
        Modal::Error { message } => ("error", message.clone()),
        // UpdatesPanel is handled by the early-return above; this arm is
        // unreachable but required for exhaustiveness.
        Modal::UpdatesPanel { .. } => unreachable!("UpdatesPanel must not reach render()"),
        Modal::ProcessList { .. } => unreachable!("ProcessList must not reach render()"),
        Modal::RepoSettings { .. } => unreachable!("RepoSettings must not reach render()"),
        Modal::AgentsPanel { .. } => unreachable!("AgentsPanel must not reach render()"),
        Modal::UsageWindowPicker { .. } => {
            unreachable!("UsageWindowPicker must not reach render()")
        }
        Modal::AgentMissing { agent, binary, .. } => (
            "agent not installed",
            format!(
                "{name} is not installed.\n\n\
                 The `{binary}` binary was not found on PATH.\n\
                 Install it, then re-enter the workspace.\n\n\
                 s    switch agent for this workspace\n\
                 Esc  dismiss",
                name = capitalize_first(agent.display_name()),
                binary = binary,
            ),
        ),
        Modal::WorkspaceActions => (
            "workspace actions",
            "These apply to the selected workspace:\n\n  \
             e   edit        t   term\n  \
             v   diff        g   lazygit\n  \
             c   chronox\n\n  \
             ?/Esc  close"
                .to_string(),
        ),
        // Placeholder body: the real list rendering (from `app.remote_list`)
        // lands in Task 6. Rendered here (rather than skipped via the
        // early-return guard above) so the modal is never blank in the
        // interim — `render.rs`'s modal dispatch already falls through to
        // this generic `render()` for any variant it doesn't special-case.
        Modal::RemoteWorkspaceList { .. } => (
            "remote workspaces",
            "loading remote list…\n\n[esc] close".to_string(),
        ),
        Modal::RemoteHostPicker { hosts, selected } => {
            let list = hosts
                .iter()
                .enumerate()
                .map(|(i, (name, dest))| {
                    let marker = if i == *selected { ">" } else { " " };
                    format!("{marker}  {name}  {dest}")
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                "pick a shared host",
                format!(
                    "Choose a host to browse shared workspaces:\n\n{list}\n\n\
                     \u{2191}\u{2193} move   Enter fetch   Esc cancel"
                ),
            )
        }
        Modal::RemoteListLoading { host_name } => (
            "remote workspaces",
            format!("fetching shared workspaces from {host_name}…\n\n[esc] cancel"),
        ),
        Modal::AgentPicker {
            selected, current, ..
        } => {
            let list = crate::pty::session::AgentKind::ALL
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    let marker = if i == *selected { ">" } else { " " };
                    let current_tag = if *k == *current { "  (current)" } else { "" };
                    format!("{marker}  {name}{current_tag}", name = k.display_name())
                })
                .collect::<Vec<_>>()
                .join("\n");
            (
                "pick an agent",
                format!(
                    "Choose an agent for this workspace:\n\n{list}\n\n\
                     \u{2191}\u{2193} move   Enter confirm   Esc cancel"
                ),
            )
        }
    };
    let style = if matches!(modal, Modal::Error { .. }) {
        theme.err_style()
    } else {
        theme.header_style()
    };
    let para = Paragraph::new(body)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .title_alignment(Alignment::Left),
        )
        .style(style);
    f.render_widget(para, rect);
}

/// Truncate `s` to at most `max` characters, appending '…' (which counts
/// toward `max`) when characters are dropped. Single pass over the input. Used
/// to keep setup-output tail lines inside the modal's inner width.
fn truncate_to(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(max);
    let mut chars = s.chars();
    for _ in 0..max {
        match chars.next() {
            // `s` fit entirely within `max` — no truncation, return as-is.
            None => return out,
            Some(c) => out.push(c),
        }
    }
    // Consumed exactly `max` chars; if any remain, truncation occurred.
    if chars.next().is_some() {
        // Drop the last kept char for the ellipsis so the total stays ≤ `max`.
        // When `max == 0` nothing was kept, so there's no room even for '…'.
        if out.pop().is_some() {
            out.push('…');
        }
    }
    out
}

/// Uppercase only the first character of `s`. Used to render the agent
/// name in the AgentMissing modal as a proper sentence-start without
/// changing the canonical lowercase form returned by `AgentKind::display_name`.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn render_to_text(modal: &Modal) -> String {
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render(f, f.area(), modal, 0, &theme))
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
    fn setup_running_shows_phase_and_recent_lines() {
        use crate::data::progress::{SetupPhase, SetupProgress};
        let progress = SetupProgress::shared();
        {
            let mut p = progress.lock().unwrap();
            p.set_phase(SetupPhase::RunningSetup);
            p.push_line("mise install");
            p.push_line("Installing dependencies");
        }
        let modal = Modal::SetupRunning {
            cancel: tokio_util::sync::CancellationToken::new(),
            progress,
            started: std::time::Instant::now(),
        };
        let text = render_to_text(&modal);
        assert!(text.contains("Running setup"), "missing phase:\n{text}");
        assert!(
            text.contains("Installing dependencies"),
            "missing line:\n{text}"
        );
        assert!(text.contains("[esc] cancel"), "missing footer:\n{text}");
    }

    #[test]
    fn truncate_to_handles_fit_truncate_and_zero() {
        // Fits exactly — unchanged, no ellipsis.
        assert_eq!(truncate_to("abc", 3), "abc");
        // Shorter than max — unchanged.
        assert_eq!(truncate_to("ab", 5), "ab");
        // Longer than max — ellipsis counts toward the budget (total == max).
        assert_eq!(truncate_to("abcdef", 3), "ab…");
        assert_eq!(truncate_to("abcdef", 3).chars().count(), 3);
        // max == 0 — never exceeds the budget, even when truncating.
        assert_eq!(truncate_to("abc", 0), "");
        assert_eq!(truncate_to("", 0), "");
        // Multi-byte chars are counted by char, not byte.
        assert_eq!(truncate_to("héllo", 2), "h…");
    }

    #[test]
    fn setup_running_truncates_overwide_line() {
        use crate::data::progress::SetupProgress;
        let progress = SetupProgress::shared();
        progress.lock().unwrap().push_line(&"x".repeat(200));
        let modal = Modal::SetupRunning {
            cancel: tokio_util::sync::CancellationToken::new(),
            progress,
            started: std::time::Instant::now(),
        };
        let text = render_to_text(&modal);
        assert!(
            text.contains('…'),
            "over-wide line should be truncated:\n{text}"
        );
    }

    #[test]
    fn workspace_actions_overlay_lists_all_actions() {
        let text = render_to_text(&Modal::WorkspaceActions);
        assert!(text.contains("edit"), "missing 'edit':\n{text}");
        assert!(text.contains("term"), "missing 'term':\n{text}");
        assert!(text.contains("diff"), "missing 'diff':\n{text}");
        assert!(text.contains("lazygit"), "missing 'lazygit':\n{text}");
        assert!(text.contains("chronox"), "missing 'chronox':\n{text}");
    }
}
