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
mod process_list;
mod remote_workspace_list;
mod repo_settings;
mod updates_panel;
mod usage_picker;

// Panel renderers called from app::render via `crate::ui::modal::*`.
pub use agents_panel::render_agents_panel;
pub use process_list::render_process_list;
pub use remote_workspace_list::render_remote_workspace_list;
pub use repo_settings::render_repo_settings;
pub use updates_panel::{
    PanelInputs, PanelView, UpdatesSort, ordered_workspaces_for_panel, render_updates_panel,
};
pub use usage_picker::render_usage_window_picker;

#[derive(Debug, Clone)]
pub enum Modal {
    NewWorkspace {
        repo_id: RepoId,
        name_buffer: String,
        yolo: bool,
        shared: bool,
        agent: crate::pty::session::AgentKind,
        /// Inline error line (e.g. a duplicate name), cleared on next edit.
        /// Mirrors `RenameWorkspace`'s `notice` field.
        notice: Option<String>,
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
        /// Instances WITHOUT a running session at modal-open time. Sharing
        /// eagerly starts these inside tmux (see `toggle_workspace_shared`),
        /// and the confirmation message must say so.
        stopped_count: usize,
    },
    SetupProgress {
        workspace_id: crate::data::store::WorkspaceId,
    },
    /// Shown when `q` is pressed while `App::in_flight` is non-empty.
    /// `y` cancels any in-flight creates and quits; archive is abandoned
    /// mid-flight, which is safe because it is self-healing (see the
    /// key handler for detail).
    ConfirmQuit {
        creates: usize,
        archives: usize,
    },
    Error {
        message: String,
    },
    UpdatesPanel {
        /// Index into the modal's ordered workspace list. Up/Down adjust
        /// it; Enter switches `app.view` to that workspace.
        selected: usize,
        /// Active sort mode; `o` cycles it. Not persisted — reset to
        /// `Default` on every open.
        sort: UpdatesSort,
        /// `None` = normal key handling. `Some(buf)` = filter-input mode,
        /// where printable keys are filter text rather than shortcuts.
        /// `Some("")` is a real state: `/` was pressed, nothing typed yet,
        /// every row still visible. Not persisted, like `sort`.
        filter: Option<String>,
    },
    ProcessList {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize,
        /// `None` = list mode; `Some(buffer)` = the user is typing a command to run.
        input: Option<String>,
        /// Last launch result (success path or error), shown below the list.
        notice: Option<String>,
    },
    RenameWorkspace {
        workspace_id: crate::data::store::WorkspaceId,
        /// Pre-filled with the current name; edited in place.
        name_buffer: String,
        /// Inline error line (e.g. rename failure); cleared on next edit.
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
    /// host (`App::remote_list`), opened by `reconcile_remote_list`. Rows are
    /// flattened per agent instance via `crate::app::remote_rows`; `selected`
    /// indexes into that flattened list. `notice` surfaces inline feedback
    /// (e.g. "no live session to attach to") the way `ProcessList::notice`
    /// does, without needing a separate modal round-trip.
    RemoteWorkspaceList {
        selected: usize,
        notice: Option<String>,
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

pub fn render(
    f: &mut Frame,
    area: Rect,
    modal: &Modal,
    in_flight: &HashMap<crate::data::store::WorkspaceId, crate::data::in_flight::InFlight>,
    tick: u32,
    theme: &Theme,
) {
    // UpdatesPanel, ProcessList, and RemoteWorkspaceList are rendered by
    // their dedicated helpers directly from `draw()` because they need live
    // App state. This function should never be called with those variants;
    // guard defensively.
    if matches!(
        modal,
        Modal::UpdatesPanel { .. }
            | Modal::ProcessList { .. }
            | Modal::RepoSettings { .. }
            | Modal::AgentsPanel { .. }
            | Modal::UsageWindowPicker { .. }
            | Modal::RemoteWorkspaceList { .. }
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
            notice,
            ..
        } => {
            let agent_label = agent.display_name();
            let shared_line = if *shared {
                "shared (tmux): on — ^s toggles\n"
            } else {
                "shared (tmux): off — ^s toggles\n"
            };
            let notice_line = notice
                .as_deref()
                .map(|n| format!("{n}\n"))
                .unwrap_or_default();
            (
                if *yolo {
                    "new workspace (permissive)"
                } else {
                    "new workspace"
                },
                format!(
                    "name: {name_buffer}\nagent: {agent_label}  [tab] toggle\n{shared_line}\n{notice_line}[enter] create   [esc] cancel"
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
            stopped_count,
            ..
        } => {
            let dest = if *to_shared {
                "shared (tmux)"
            } else {
                "direct (not tmux)"
            };
            let restart_note = if *to_shared {
                // Sharing eagerly puts EVERY agent inside tmux: running
                // sessions restart wrapped in tmux, stopped ones spawn.
                let mut parts = Vec::new();
                if *running_count > 0 {
                    parts.push(format!(
                        "Restarts {running_count} running session(s) inside tmux (conversation resumes via --continue)."
                    ));
                }
                if *stopped_count > 0 {
                    parts.push(format!(
                        "Starts {stopped_count} stopped agent(s) inside tmux."
                    ));
                }
                if parts.is_empty() {
                    "All agents will run inside tmux.".to_string()
                } else {
                    parts.join("\n")
                }
            } else {
                match running_count {
                    0 => "No running sessions to restart.".to_string(),
                    n => format!(
                        "This restarts {n} running session(s) outside tmux (conversation resumes via --continue)."
                    ),
                }
            };
            (
                "toggle sharing",
                format!(
                    "switch '{name}' to {dest}?\n\n{restart_note}\n\n[y] yes   [n]/[esc] cancel"
                ),
            )
        }
        Modal::SetupProgress { workspace_id } => match in_flight.get(workspace_id) {
            // The task finished while the viewer was open. Say so rather than
            // rendering a stale tail; the reconciler has already dropped the entry.
            None => (
                "workspace setup",
                "  setup finished.\n\n  [esc] close".to_string(),
            ),
            Some(f) => {
                use crate::data::in_flight::InFlightKind;
                let frame = crate::ui::dashboard::spinner::frame(tick);
                // Archive never sets a `SetupPhase` (there is no phase concept
                // for it), so reading `p.phase().label()` unconditionally
                // always shows create's default phase ("Fetching base…")
                // even while a worktree is mid-deletion. Derive both the
                // title and the status line from the entry's `InFlightKind`
                // instead so archive gets something truthful; create's
                // rendering is unchanged.
                let (title, status_label) = match f.kind {
                    InFlightKind::Create => {
                        let label = match f.progress.lock() {
                            Ok(p) => p.phase().label(),
                            Err(_) => "Working",
                        };
                        ("workspace setup", label)
                    }
                    InFlightKind::Archive => ("archiving workspace", "Archiving"),
                };
                let tail = match f.progress.lock() {
                    Ok(p) => p.recent(6),
                    Err(_) => Vec::new(),
                };
                let secs = f.started.elapsed().as_secs();
                let elapsed = format!("{:02}:{:02}", secs / 60, secs % 60);
                let mut body = format!("  {frame} {status_label}…   ({elapsed})\n\n");
                if tail.is_empty() {
                    body.push_str("  (waiting for output…)\n");
                } else {
                    for line in &tail {
                        body.push_str(&format!("  {}\n", truncate_to(line, 54)));
                    }
                }
                body.push_str("\n  [esc] close");
                (title, body)
            }
        },
        Modal::ConfirmQuit { creates, archives } => {
            let mut what = Vec::new();
            if *creates > 0 {
                what.push(format!("{creates} setup(s)"));
            }
            if *archives > 0 {
                what.push(format!("{archives} archive(s)"));
            }
            (
                "work in progress",
                format!(
                    "{} still running.\n\n\
                     Quitting stops them: setups are cancelled, and an archive is\n\
                     left part-done (archiving again finishes it).\n\n\
                     [y] quit anyway   [n]/[esc] stay",
                    what.join(" and ")
                ),
            )
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
             c   chronox     r   rename\n  \
             o   setup log   x   cancel setup\n\n  \
             ?/Esc  close"
                .to_string(),
        ),
        Modal::RenameWorkspace {
            name_buffer,
            notice,
            ..
        } => {
            let notice_line = notice
                .as_deref()
                .map(|n| format!("{n}\n"))
                .unwrap_or_default();
            (
                "rename workspace",
                format!(
                    "name: {name_buffer}\u{2588}\n\n{notice_line}[enter] rename   [esc] cancel"
                ),
            )
        }
        // RemoteWorkspaceList is handled by the early-return above; this arm
        // is unreachable but required for exhaustiveness.
        Modal::RemoteWorkspaceList { .. } => {
            unreachable!("RemoteWorkspaceList must not reach render()")
        }
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

    fn render_to_text(
        modal: &Modal,
        in_flight: &HashMap<crate::data::store::WorkspaceId, crate::data::in_flight::InFlight>,
    ) -> String {
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render(f, f.area(), modal, in_flight, 0, &theme))
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
        let workspace_id = crate::data::store::WorkspaceId(1);
        let mut in_flight = HashMap::new();
        in_flight.insert(
            workspace_id,
            crate::data::in_flight::InFlight::create(
                progress,
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        let modal = Modal::SetupProgress { workspace_id };
        let text = render_to_text(&modal, &in_flight);
        assert!(text.contains("Running setup"), "missing phase:\n{text}");
        assert!(
            text.contains("Installing dependencies"),
            "missing line:\n{text}"
        );
        assert!(text.contains("[esc] close"), "missing footer:\n{text}");
    }

    /// F7 regression: an archive entry never sets a `SetupPhase` (there is no
    /// phase concept for it), so reading `p.phase().label()` unconditionally
    /// showed create's default phase ("Fetching base…") while a worktree was
    /// mid-deletion, under a "workspace setup" title. The renderer must
    /// derive both the title and the status line from the entry's
    /// `InFlightKind` instead, so an archive viewer reads truthfully.
    #[test]
    fn setup_progress_labels_archive_truthfully_not_as_workspace_setup() {
        use crate::data::progress::SetupProgress;
        let progress = SetupProgress::shared();
        progress.lock().unwrap().push_line("removing worktree");
        let workspace_id = crate::data::store::WorkspaceId(1);
        let mut in_flight = HashMap::new();
        in_flight.insert(
            workspace_id,
            crate::data::in_flight::InFlight::archive(
                progress,
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        let modal = Modal::SetupProgress { workspace_id };
        let text = render_to_text(&modal, &in_flight);
        assert!(
            text.contains("archiving workspace"),
            "expected an archive-truthful title:\n{text}"
        );
        assert!(
            !text.contains("workspace setup"),
            "must not show create's title for an archive:\n{text}"
        );
        assert!(
            !text.contains("Fetching base"),
            "must not show create's default phase for an archive:\n{text}"
        );
        assert!(
            text.contains("removing worktree"),
            "missing appended progress line:\n{text}"
        );
    }

    /// Sharing eagerly starts stopped agents (see `toggle_workspace_shared`),
    /// so the confirmation must say so — a bare "No running sessions to
    /// restart." would read as "nothing will happen", which is exactly the
    /// expectation mismatch that made shares of stopped workspaces look
    /// like failures.
    #[test]
    fn confirm_share_to_shared_mentions_starting_stopped_agents() {
        let modal = Modal::ConfirmShare {
            workspace_id: crate::data::store::WorkspaceId(1),
            name: "w".into(),
            to_shared: true,
            running_count: 1,
            stopped_count: 2,
        };
        let text = render_to_text(&modal, &HashMap::new());
        assert!(
            text.contains("Restarts 1 running session(s)"),
            "missing running-restart note:\n{text}"
        );
        assert!(
            text.contains("Starts 2 stopped agent(s)"),
            "missing stopped-start note:\n{text}"
        );
    }

    /// Unsharing keeps the old semantics: only running sessions restart,
    /// stopped agents stay stopped — the copy must not promise spawns.
    #[test]
    fn confirm_share_to_direct_keeps_no_running_sessions_note() {
        let modal = Modal::ConfirmShare {
            workspace_id: crate::data::store::WorkspaceId(1),
            name: "w".into(),
            to_shared: false,
            running_count: 0,
            stopped_count: 2,
        };
        let text = render_to_text(&modal, &HashMap::new());
        assert!(
            text.contains("No running sessions to restart."),
            "missing no-op note:\n{text}"
        );
        assert!(
            !text.contains("stopped agent"),
            "unshare must not promise to start stopped agents:\n{text}"
        );
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
        let workspace_id = crate::data::store::WorkspaceId(1);
        let mut in_flight = HashMap::new();
        in_flight.insert(
            workspace_id,
            crate::data::in_flight::InFlight::create(
                progress,
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        let modal = Modal::SetupProgress { workspace_id };
        let text = render_to_text(&modal, &in_flight);
        assert!(
            text.contains('…'),
            "over-wide line should be truncated:\n{text}"
        );
    }

    #[test]
    fn workspace_actions_overlay_lists_all_actions() {
        let text = render_to_text(&Modal::WorkspaceActions, &HashMap::new());
        assert!(text.contains("edit"), "missing 'edit':\n{text}");
        assert!(text.contains("term"), "missing 'term':\n{text}");
        assert!(text.contains("diff"), "missing 'diff':\n{text}");
        assert!(text.contains("lazygit"), "missing 'lazygit':\n{text}");
        assert!(text.contains("chronox"), "missing 'chronox':\n{text}");
    }
}
