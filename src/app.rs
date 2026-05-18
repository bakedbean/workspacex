#![allow(clippy::collapsible_if)]

use crate::error::Result;
use crate::pty::session::SessionManager;
use crate::store::{Repo, Store, Workspace, WorkspaceId};
use crate::ui::View;
use crate::ui::dashboard::DashboardState;
use crate::ui::modal::Modal;
use crate::ui::split::{Arrow, AttachedState, CloseOutcome, SplitDirection};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Leader key for attached-view actions (detach, open updates panel, send
/// literal leader to claude). Chosen to be free in raw mode and to avoid
/// collision with tmux's default `Ctrl-b` prefix (or any non-default
/// `Ctrl-a` setup).
const LEADER_KEY: crossterm::event::KeyCode = crossterm::event::KeyCode::Char('x');

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum AppEvent {
    Tick,
    Key(crossterm::event::KeyEvent),
    Resize(u16, u16),
    SetupLine(String),
    SetupFinished { id: WorkspaceId, ok: bool },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionTarget {
    Repo(crate::store::RepoId),
    Workspace(crate::store::WorkspaceId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSettingField {
    BranchPrefix,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
    PinnedCommands,
    RelatedRepos,
}

impl RepoSettingField {
    pub const ALL: [Self; 6] = [
        Self::BranchPrefix,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
        Self::RelatedRepos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::BranchPrefix => "branch_prefix",
            Self::CustomInstructions => "custom_instructions",
            Self::SetupScript => "setup_script",
            Self::ArchiveScript => "archive_script",
            Self::PinnedCommands => "pinned_commands",
            Self::RelatedRepos => "related_repos",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingEdit {
    pub repo_id: crate::store::RepoId,
    pub field: RepoSettingField,
}

/// Why the agent paused at end-of-turn. Distinguishes "asked the user
/// something and is waiting for an answer" from "finished a task".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoppedKind {
    /// The agent invoked `AskUserQuestion` or `ExitPlanMode` and the
    /// user hasn't responded yet, OR the final assistant text ended
    /// with `?` (fallback). Maps to the "?" dashboard glyph.
    AwaitingAnswer,
    /// The agent finished without asking the user anything. Maps to
    /// the "✓" dashboard glyph.
    Complete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// The agent has stopped its turn and is waiting for an answer
    /// from the user. Higher priority than PTY-recency states.
    AwaitingAnswer,
    /// The agent has stopped its turn with a completed task and is
    /// awaiting acknowledgment. Higher priority than PTY-recency states.
    Complete,
    /// A tool_use has been pending for ≥3s (almost always a permission
    /// prompt). Higher priority than `AwaitingAnswer` / `Complete`.
    Awaiting,
    /// < 2s since last PTY output.
    Active,
    /// 2–30s since last PTY output.
    Idle,
    /// Claude has stalled between turns: the JSONL log hasn't been
    /// appended for >60s, no tool_use is pending, and we've seen at
    /// least one stop_reason in this session. Alertable.
    Stalled,
    /// More than 30s since last PTY output but no JSONL stop signal.
    /// Retained for the recency column; does NOT drive the bell.
    Waiting,
    /// No session attached at all.
    Off,
}

impl ActivityState {
    /// States that should fire a bell + attention marker when entered.
    pub fn is_alertable(self) -> bool {
        matches!(
            self,
            ActivityState::AwaitingAnswer
                | ActivityState::Complete
                | ActivityState::Awaiting
                | ActivityState::Stalled
        )
    }
}

fn classify_activity(secs: Option<u64>) -> ActivityState {
    match secs {
        Some(s) if s < 2 => ActivityState::Active,
        Some(s) if s < 30 => ActivityState::Idle,
        Some(_) => ActivityState::Waiting,
        None => ActivityState::Off,
    }
}

/// Compute the activity state for a workspace, combining JSONL-derived
/// signals with PTY-output recency.
///
/// Priority: `Awaiting` (permission prompt) > `AwaitingAnswer` /
/// `Complete` (turn ended) > `Stalled` (mid-tool-chain quiet) >
/// PTY-recency > `Off`.
fn classify_activity_with_events(
    secs: Option<u64>,
    running: bool,
    awaiting: bool,
    stopped_kind: Option<StoppedKind>,
    stalled: bool,
) -> ActivityState {
    if awaiting {
        return ActivityState::Awaiting;
    }
    match stopped_kind {
        Some(StoppedKind::AwaitingAnswer) => return ActivityState::AwaitingAnswer,
        Some(StoppedKind::Complete) => return ActivityState::Complete,
        None => {}
    }
    if stalled {
        return ActivityState::Stalled;
    }
    if !running {
        return ActivityState::Off;
    }
    classify_activity(secs)
}

fn encode_key_for_pty(k: &crossterm::event::KeyEvent) -> Option<Vec<u8>> {
    use crossterm::event::{KeyCode, KeyModifiers};
    match (k.code, k.modifiers) {
        (KeyCode::Char(c), m) if m == KeyModifiers::NONE || m == KeyModifiers::SHIFT => {
            Some(c.to_string().into_bytes())
        }
        (KeyCode::Char(c), m) if m.contains(KeyModifiers::CONTROL) => {
            let upper = c.to_ascii_uppercase();
            if ('@'..='_').contains(&upper) {
                Some(vec![(upper as u8) - b'@'])
            } else {
                None
            }
        }
        (KeyCode::Enter, _) => Some(b"\r".to_vec()),
        (KeyCode::Backspace, _) => Some(vec![0x7f]),
        (KeyCode::Up, _) => Some(b"\x1b[A".to_vec()),
        (KeyCode::Down, _) => Some(b"\x1b[B".to_vec()),
        (KeyCode::Right, _) => Some(b"\x1b[C".to_vec()),
        (KeyCode::Left, _) => Some(b"\x1b[D".to_vec()),
        (KeyCode::Tab, _) => Some(b"\t".to_vec()),
        _ => None,
    }
}

pub struct App {
    pub store: Store,
    pub sessions: SessionManager,
    pub view: View,
    pub modal: Option<Modal>,
    pub dashboard: DashboardState,
    pub repos: Vec<Repo>,
    pub workspaces: Vec<(crate::store::RepoId, Workspace)>,
    pub selectable: Vec<SelectionTarget>,
    pub worktree_base: PathBuf,
    pub leader_pending: bool,
    pub quit: bool,
    pub workspace_status:
        std::collections::HashMap<crate::store::WorkspaceId, crate::git::WorkspaceStatus>,
    /// Cached PR lifecycle per workspace. Absent key = never polled; present
    /// key = last successful poll's result.
    pub pr_lifecycle:
        std::collections::HashMap<crate::store::WorkspaceId, crate::forge::BranchLifecycle>,
    /// Last epoch-ms we attempted a PR fetch per workspace (throttle key).
    pub pr_last_poll_ms: std::collections::HashMap<crate::store::WorkspaceId, i64>,
    pub workspace_events:
        std::collections::HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    /// Per-workspace tracking for attention-alert state.
    pub workspace_activity: std::collections::HashMap<crate::store::WorkspaceId, ActivityState>,
    /// Workspaces whose alert hasn't been acknowledged (cleared on attach).
    pub workspace_needs_attention: std::collections::HashSet<crate::store::WorkspaceId>,
    /// Processes detected per workspace (cwd inside the workspace's
    /// worktree). Refreshed every ~10s by branch_drift_poll.
    pub workspace_processes:
        std::collections::HashMap<crate::store::WorkspaceId, Vec<crate::proc::ProcInfo>>,
    /// Epoch-ms of last completed `proc::scan` — throttle source.
    pub last_proc_scan_ms: i64,
    /// Set by the repo-settings modal when the user presses Enter on a
    /// field. The run loop detects this BEFORE the next draw, suspends
    /// the TUI, invokes `external::edit_in_editor`, resumes, and saves.
    pub pending_edit: Option<PendingEdit>,
    pub theme: crate::ui::theme::Theme,
    pub pm: Option<std::sync::Arc<crate::pty::session::Session>>,
    pub pm_visible: bool,
    pub focus: crate::ui::PaneFocus,
    pub pm_auto_summary_sent: bool,
    /// Rects of the rendered chip row buttons from the last draw tick.
    /// Used by mouse/key handlers (Tasks 8 and 9) to dispatch clicks.
    pub chip_rects: Vec<ratatui::layout::Rect>,
    /// Resolved pinned commands from the last draw tick (matches `chip_rects`).
    pub pinned_commands_cache: Vec<crate::pinned::PinnedCommand>,
    /// Bells queued up by the most recent draw tick. Drained and fired
    /// AFTER `terminal.draw()` returns to avoid interleaving `\x07` writes
    /// with ratatui's escape sequences. See Task 4 / Critical review.
    pub pending_bells: Vec<ActivityState>,
}

impl App {
    pub fn new(store: Store, worktree_base: PathBuf) -> Result<Self> {
        let theme_name = store
            .get_setting("theme")
            .ok()
            .flatten()
            .unwrap_or_default();
        let theme = crate::ui::theme::Theme::by_name(&theme_name);
        let mut app = Self {
            store,
            sessions: SessionManager::new(),
            view: View::Dashboard,
            modal: None,
            dashboard: DashboardState::default(),
            repos: Vec::new(),
            workspaces: Vec::new(),
            selectable: Vec::new(),
            worktree_base,
            leader_pending: false,
            quit: false,
            workspace_status: std::collections::HashMap::new(),
            pr_lifecycle: std::collections::HashMap::new(),
            pr_last_poll_ms: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
            workspace_activity: std::collections::HashMap::new(),
            workspace_needs_attention: std::collections::HashSet::new(),
            workspace_processes: std::collections::HashMap::new(),
            last_proc_scan_ms: 0,
            pending_edit: None,
            theme,
            pm: None,
            pm_visible: false,
            focus: crate::ui::PaneFocus::Dashboard,
            pm_auto_summary_sent: false,
            chip_rects: Vec::new(),
            pinned_commands_cache: Vec::new(),
            pending_bells: Vec::new(),
        };
        // Sweep stale Pending rows from previous runs.
        let _ = app
            .store
            .sweep_stale_pending(std::time::Duration::from_secs(300));
        app.refresh()?;
        Ok(app)
    }

    pub fn refresh(&mut self) -> Result<()> {
        self.repos = self.store.repos()?;
        self.workspaces = Vec::new();
        for r in &self.repos {
            for w in self.store.workspaces(r.id)? {
                self.workspaces.push((r.id, w));
            }
        }
        // Rebuild selection targets: repos in order, each followed by its workspaces.
        self.selectable.clear();
        for repo in &self.repos {
            self.selectable.push(SelectionTarget::Repo(repo.id));
            for (rid, w) in &self.workspaces {
                if *rid == repo.id {
                    self.selectable.push(SelectionTarget::Workspace(w.id));
                }
            }
        }
        if !self.selectable.is_empty() && self.dashboard.selected >= self.selectable.len() {
            self.dashboard.selected = self.selectable.len() - 1;
        }
        Ok(())
    }

    pub fn selected_target(&self) -> Option<SelectionTarget> {
        self.selectable.get(self.dashboard.selected).copied()
    }

    /// If the workspace has any pending tool_use that is a real permission
    /// prompt (NOT AskUserQuestion / ExitPlanMode, which are question tools
    /// surfaced separately as AwaitingAnswer), return the oldest pending
    /// tool's (name, first-seen epoch ms). Returns None otherwise.
    ///
    /// 3 seconds is well past the latency of any auto-approved tool, so a
    /// pending entry that crosses that threshold is almost certainly waiting
    /// on a permission prompt the user needs to address.
    pub fn awaiting_permission(&self, ws_id: crate::store::WorkspaceId) -> Option<(String, i64)> {
        let evt = self.workspace_events.get(&ws_id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        const STALE_MS: i64 = 3000;
        let mut oldest: Option<(&str, i64)> = None;
        for (name, ts) in evt.pending_tool_uses.values() {
            if name == "AskUserQuestion" || name == "ExitPlanMode" {
                continue;
            }
            let age = now - *ts;
            if age >= STALE_MS {
                match oldest {
                    None => oldest = Some((name.as_str(), *ts)),
                    Some((_, t)) if *ts < t => oldest = Some((name.as_str(), *ts)),
                    _ => {}
                }
            }
        }
        oldest.map(|(n, ts)| (n.to_string(), ts))
    }
}

/// Derive the StoppedKind for a workspace based on its WorkspaceEvents.
/// Returns Some when the agent is paused waiting on the user (either
/// mid-turn with a pending question tool, or end-of-turn with a
/// trailing question / completion).
fn derive_stopped_kind(e: &crate::events::WorkspaceEvents) -> Option<StoppedKind> {
    // Question tools fire even without a terminal stop_reason — the model
    // is mid-turn but has explicitly asked the user something.
    if e.pending_question_tool().is_some() {
        return Some(StoppedKind::AwaitingAnswer);
    }
    if !e.is_awaiting_user() {
        return None;
    }
    if e.last_text_ends_with_question() {
        Some(StoppedKind::AwaitingAnswer)
    } else {
        Some(StoppedKind::Complete)
    }
}

pub type SharedApp = Arc<Mutex<App>>;

use crossterm::event::{
    Event as CtEvent, EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::Backend;
use std::time::Duration;

async fn do_pending_edit<B>(
    terminal: &mut ratatui::Terminal<B>,
    app: &SharedApp,
    edit: PendingEdit,
) -> Result<()>
where
    B: ratatui::backend::Backend + std::io::Write,
{
    // Read current value + extension hint under the lock.
    let (current, ext) = {
        let g = app.lock().await;
        let Some(repo) = g.repos.iter().find(|r| r.id == edit.repo_id) else {
            return Ok(());
        };
        match edit.field {
            RepoSettingField::BranchPrefix => (repo.branch_prefix.clone(), "txt"),
            RepoSettingField::CustomInstructions => {
                (repo.custom_instructions.clone().unwrap_or_default(), "md")
            }
            RepoSettingField::SetupScript => {
                (repo.setup_script.clone().unwrap_or_default(), "bash")
            }
            RepoSettingField::ArchiveScript => {
                (repo.archive_script.clone().unwrap_or_default(), "bash")
            }
            RepoSettingField::PinnedCommands => {
                (repo.pinned_commands.clone().unwrap_or_default(), "txt")
            }
            RepoSettingField::RelatedRepos => {
                (repo.related_repos.clone().unwrap_or_default(), "txt")
            }
        }
    };

    // Suspend the TUI.
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;

    let result = crate::external::edit_in_editor(&current, ext);

    // Resume the TUI.
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen
    )?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    if let Ok(Some(new)) = result {
        if new.trim() != current.trim() {
            let mut g = app.lock().await;
            let _ = apply_repo_setting(&mut g, edit.repo_id, edit.field, &new);
            let _ = g.refresh();
        }
    }
    Ok(())
}

pub async fn run<B: Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: SharedApp,
) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(16));

    loop {
        // Handle any pending edit BEFORE drawing — the editor takes
        // over the terminal and we need a clean redraw after it exits.
        let pending = {
            let mut g = app.lock().await;
            g.pending_edit.take()
        };
        if let Some(edit) = pending {
            do_pending_edit(terminal, &app, edit).await?;
        }

        {
            let mut g = app.lock().await;
            terminal.draw(|f| draw(f, &mut g))?;
            // Drain bells queued during draw and fire them OUTSIDE the draw
            // closure so writes to stdout don't interleave with ratatui's
            // frame flush (mid-escape `\x07` is undefined per VT spec).
            let bells = std::mem::take(&mut g.pending_bells);
            for state in bells {
                fire_bell(state, &g.store);
            }
            if g.quit {
                break;
            }
        }

        tokio::select! {
            _ = tick.tick() => {}
            maybe_evt = events.next() => {
                let Some(Ok(evt)) = maybe_evt else { break; };
                let mut g = app.lock().await;
                handle_event(&mut g, evt).await?;
            }
        }
    }
    Ok(())
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    use crate::ui::{attached, dashboard, modal};
    let area = f.area();
    // Clear chip state at the start of every frame; only View::Attached
    // overwrites these with live values.
    app.chip_rects.clear();
    app.pinned_commands_cache.clear();
    match &app.view {
        View::Dashboard => {
            let (dashboard_area, pm_area) = if app.pm_visible {
                let chunks = ratatui::layout::Layout::default()
                    .direction(ratatui::layout::Direction::Vertical)
                    .constraints([
                        ratatui::layout::Constraint::Percentage(60),
                        ratatui::layout::Constraint::Percentage(40),
                    ])
                    .split(area);
                (chunks[0], Some(chunks[1]))
            } else {
                (area, None)
            };
            let notifications_on = notifications_enabled(&app.store);
            let mut items: Vec<dashboard::Item> = Vec::new();
            for repo in &app.repos {
                items.push(dashboard::Item::Header { repo });
                let mut count = 0usize;
                for (rid, ws) in &app.workspaces {
                    if *rid != repo.id {
                        continue;
                    }
                    count += 1;
                    let session = app.sessions.get(ws.id);
                    let running = session.as_ref().is_some_and(|s| {
                        matches!(
                            *s.status.read().unwrap(),
                            crate::pty::session::SessionStatus::Running { .. }
                        )
                    });
                    let secs = session.as_ref().map(|s| {
                        let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                        if last == 0 {
                            return 0;
                        }
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        now.saturating_sub(last) / 1000
                    });
                    let has_prior = crate::pty::session::has_prior_session(&ws.worktree_path);
                    let needs_attention = app.workspace_needs_attention.contains(&ws.id);
                    let awaiting = app.awaiting_permission(ws.id);
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0);
                    let stopped_kind =
                        app.workspace_events.get(&ws.id).and_then(derive_stopped_kind);
                    let stalled = app
                        .workspace_events
                        .get(&ws.id)
                        .is_some_and(|e| e.is_stalled(now_ms, 60_000));
                    items.push(dashboard::Item::Workspace {
                        repo,
                        workspace: ws,
                        session_running: running,
                        seconds_since_activity: secs,
                        has_prior_session: has_prior,
                        status: app.workspace_status.get(&ws.id).copied(),
                        latest_event: app
                            .workspace_events
                            .get(&ws.id)
                            .and_then(|e| e.latest.clone()),
                        needs_attention,
                        stopped_kind,
                        stalled,
                        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                        awaiting_tool: awaiting,
                        proc_count: app
                            .workspace_processes
                            .get(&ws.id)
                            .map(|v| v.len())
                            .unwrap_or(0),
                    });
                }
                if count == 0 {
                    items.push(dashboard::Item::EmptyHint);
                }
                items.push(dashboard::Item::Spacer);
            }

            // Commit the new activity states + fire bell on transitions
            // into an alertable state. Fires on:
            //   - first observation of a workspace already in an alertable
            //     state (e.g. wsx just started, agent was already waiting),
            //   - transition from any non-alertable state into
            //     AwaitingAnswer / Complete / Awaiting / Stalled,
            //   - transition between two different alertable states
            //     (e.g. Complete -> Awaiting when a permission prompt
            //     arrives while the user hasn't yet replied to the prior
            //     end_turn).
            // Does NOT re-fire while an alertable state persists across
            // polls.
            for (_rid, ws) in &app.workspaces {
                let session = app.sessions.get(ws.id);
                let running = session.as_ref().is_some_and(|s| {
                    matches!(
                        *s.status.read().unwrap(),
                        crate::pty::session::SessionStatus::Running { .. }
                    )
                });
                let secs = session.as_ref().map(|s| {
                    let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
                    if last == 0 {
                        return 0;
                    }
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    now.saturating_sub(last) / 1000
                });
                let awaiting = app.awaiting_permission(ws.id).is_some();
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let stopped_kind =
                    app.workspace_events.get(&ws.id).and_then(derive_stopped_kind);
                let stalled = app
                    .workspace_events
                    .get(&ws.id)
                    .is_some_and(|e| e.is_stalled(now_ms, 60_000));
                let activity = classify_activity_with_events(
                    secs,
                    running,
                    awaiting,
                    stopped_kind,
                    stalled,
                );
                let prev = app.workspace_activity.get(&ws.id).copied();
                if activity.is_alertable() && prev != Some(activity) && notifications_on {
                    app.workspace_needs_attention.insert(ws.id);
                    app.pending_bells.push(activity);
                }
                app.workspace_activity.insert(ws.id, activity);
            }

            let selected = app.selected_target();
            let nerd_fonts = nerd_fonts_enabled(&app.store);
            dashboard::render(
                f,
                dashboard_area,
                &items,
                selected,
                nerd_fonts,
                &app.theme,
                &mut app.dashboard,
            );
            if let Some(pm_area) = pm_area {
                if let Some(session) = app.pm.as_ref() {
                    crate::ui::pm_pane::resize_session(session, pm_area);
                }
                crate::ui::pm_pane::render(f, pm_area, app.pm.as_ref(), app.focus, &app.theme);
            }
        }
        View::Attached(state) => {
            // If any leaf's session has gone away (e.g. workspace was
            // archived from elsewhere), bounce back to dashboard. Matches
            // the previous single-pane fallback at handle_key_attached.
            if state
                .leaves()
                .iter()
                .any(|id| app.sessions.get(*id).is_none())
            {
                app.view = View::Dashboard;
                return;
            }
            let focused_id = match state.focused_id() {
                Some(id) => id,
                None => {
                    app.view = View::Dashboard;
                    return;
                }
            };
            let focused_label = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == focused_id)
                .map(|(_, w)| w.name.clone())
                .unwrap_or_default();

            // The status row gets the inner width minus the "⚠ " prefix
            // (glyph + space) that `attached::render_panes` prepends.
            let max_width = (area.width as usize).saturating_sub(3);
            let line = if matches!(
                app.modal,
                Some(crate::ui::modal::Modal::UpdatesPanel { .. })
            ) {
                None
            } else {
                compute_attention_line(app, Some(focused_id), max_width)
            };

            // Pinned commands resolve against the FOCUSED pane's workspace.
            let global_pinned = app.store.get_setting("pinned_commands").ok().flatten();
            let repo_pinned = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == focused_id)
                .and_then(|(_, w)| {
                    app.repos
                        .iter()
                        .find(|r| r.id == w.repo_id)
                        .and_then(|r| r.pinned_commands.clone())
                });
            let pinned = crate::pinned::resolve(global_pinned.as_deref(), repo_pinned.as_deref());

            let (pane_area, chip_area, status_area, footer_area) =
                attached::layout_chrome(area, line.is_some(), !pinned.is_empty());
            let pane_layouts = state.layout(pane_area);
            let multi_pane = pane_layouts.len() > 1;

            // Resize each session's PTY to its pane area (minus title row when multi-pane).
            for (ws_id, _path, rect) in &pane_layouts {
                if let Some(session) = app.sessions.get(*ws_id) {
                    attached::resize_pane(&session, *rect, multi_pane);
                }
            }

            // Build PaneSpec list. Use owned sessions + labels to keep
            // them alive while rendering.
            let pane_data: Vec<(
                std::sync::Arc<crate::pty::session::Session>,
                String,
                ratatui::layout::Rect,
                bool,
            )> = pane_layouts
                .into_iter()
                .filter_map(|(ws_id, path, rect)| {
                    let session = app.sessions.get(ws_id)?;
                    let label = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == ws_id)
                        .map(|(_, w)| w.name.clone())
                        .unwrap_or_default();
                    let focused = path == state.focus;
                    Some((session, label, rect, focused))
                })
                .collect();
            let specs: Vec<crate::ui::attached::PaneSpec<'_>> = pane_data
                .iter()
                .map(|(s, l, r, f)| crate::ui::attached::PaneSpec {
                    session: s,
                    label: l.as_str(),
                    rect: *r,
                    focused: *f,
                })
                .collect();

            let chip_rects = attached::render_panes(
                f,
                &specs,
                chip_area,
                status_area,
                footer_area,
                &focused_label,
                multi_pane,
                line.as_deref(),
                &pinned,
                &app.theme,
            );
            app.chip_rects = chip_rects;
            app.pinned_commands_cache = pinned;
        }
        View::AttachedPm => {
            if let Some(session) = app.pm.as_ref() {
                let max_width = (area.width as usize).saturating_sub(3);
                let line = if matches!(
                    app.modal,
                    Some(crate::ui::modal::Modal::UpdatesPanel { .. })
                ) {
                    None
                } else {
                    compute_attention_line(app, None, max_width)
                };
                // PM pane is out of scope for pinned commands per spec.
                let pinned: &[crate::pinned::PinnedCommand] = &[];
                let (pane_area, chip_area, status_area, footer_area) =
                    attached::layout_chrome(area, line.is_some(), false);
                attached::resize_pane(session, pane_area, false);
                let specs = [crate::ui::attached::PaneSpec {
                    session,
                    label: "project-manager",
                    rect: pane_area,
                    focused: true,
                }];
                let _chip_rects = attached::render_panes(
                    f,
                    &specs,
                    chip_area,
                    status_area,
                    footer_area,
                    "project-manager",
                    false,
                    line.as_deref(),
                    pinned,
                    &app.theme,
                );
            } else {
                // PM session went away; bounce to dashboard on next event.
                app.view = View::Dashboard;
            }
        }
    }
    if let Some(m) = &app.modal {
        match m {
            crate::ui::modal::Modal::UpdatesPanel { selected } => {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let mut awaiting: std::collections::HashMap<
                    crate::store::WorkspaceId,
                    (String, i64),
                > = std::collections::HashMap::new();
                for (_rid, w) in &app.workspaces {
                    if let Some(a) = app.awaiting_permission(w.id) {
                        awaiting.insert(w.id, a);
                    }
                }
                let activity_translated: std::collections::HashMap<
                    crate::store::WorkspaceId,
                    crate::ui::updates_bar::ActivityState,
                > = app
                    .workspace_activity
                    .iter()
                    .map(|(k, v)| (*k, translate_activity(*v)))
                    .collect();
                crate::ui::modal::render_updates_panel(
                    f,
                    area,
                    &app.repos,
                    &app.workspaces,
                    &app.workspace_events,
                    &activity_translated,
                    &app.workspace_needs_attention,
                    &awaiting,
                    *selected,
                    now_ms,
                    &app.theme,
                );
            }
            crate::ui::modal::Modal::ProcessList {
                workspace_id,
                selected,
            } => {
                let workspace_name = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == *workspace_id)
                    .map(|(_, w)| w.name.clone())
                    .unwrap_or_default();
                let procs = app
                    .workspace_processes
                    .get(workspace_id)
                    .cloned()
                    .unwrap_or_default();
                crate::ui::modal::render_process_list(
                    f,
                    area,
                    &workspace_name,
                    &procs,
                    *selected,
                    &app.theme,
                );
            }
            crate::ui::modal::Modal::RepoSettings { repo_id, selected } => {
                if let Some(repo) = app.repos.iter().find(|r| r.id == *repo_id) {
                    let repo_name = repo.name.clone();
                    crate::ui::modal::render_repo_settings(
                        f, area, &repo_name, repo, *selected, &app.theme,
                    );
                }
            }
            other => modal::render(f, area, other, &app.theme),
        }
    }
}

#[doc(hidden)]
pub fn draw_for_test(f: &mut ratatui::Frame, app: &mut App) {
    draw(f, app);
}

fn nerd_fonts_enabled(store: &crate::store::Store) -> bool {
    match store.get_setting("nerd_fonts").ok().flatten().as_deref() {
        Some("false") | Some("0") | Some("off") | Some("no") => false,
        _ => true, // default ON
    }
}

fn pm_enabled(store: &Store) -> bool {
    match store.get_setting("pm_enabled").ok().flatten() {
        None => true,
        Some(v) => !matches!(
            v.trim().to_lowercase().as_str(),
            "false" | "0" | "off" | "no"
        ),
    }
}

fn notifications_enabled(store: &crate::store::Store) -> bool {
    match store.get_setting("notifications").ok().flatten().as_deref() {
        Some("off") | Some("false") | Some("0") | Some("no") => false,
        _ => true, // default ON
    }
}

/// Bell patterns: how many `\x07` bytes to emit, with spacing.
#[derive(Debug, Clone, Copy)]
enum BellPattern {
    Off,
    Single,
    Double,
    Triple,
}

impl BellPattern {
    fn from_setting(s: Option<&str>) -> Option<Self> {
        match s {
            Some("off") | Some("false") | Some("0") => Some(BellPattern::Off),
            Some("single") => Some(BellPattern::Single),
            Some("double") => Some(BellPattern::Double),
            Some("triple") => Some(BellPattern::Triple),
            _ => None, // caller uses its own default
        }
    }
}

/// Pick the bell pattern for a given alertable state. Reads per-state
/// overrides from the store, falling back to sensible defaults.
fn bell_pattern_for(state: ActivityState, store: &crate::store::Store) -> BellPattern {
    let (key, default_pattern) = match state {
        ActivityState::AwaitingAnswer => ("notification_bell_question", BellPattern::Double),
        ActivityState::Complete => ("notification_bell_complete", BellPattern::Single),
        ActivityState::Awaiting => ("notification_bell_permission", BellPattern::Single),
        ActivityState::Stalled => ("notification_bell_stalled", BellPattern::Triple),
        // Non-alertable states never call fire_bell, but be safe.
        _ => return BellPattern::Off,
    };
    let stored = store.get_setting(key).ok().flatten();
    BellPattern::from_setting(stored.as_deref()).unwrap_or(default_pattern)
}

/// Emit a terminal-bell pattern for an alertable state. Multi-bell
/// patterns spawn a detached thread to space the writes (~120ms apart)
/// so the engine event loop isn't blocked.
///
/// Residual race: the first bell fires synchronously outside ratatui's
/// `draw()` closure (see the run loop's drain of `pending_bells`), but
/// the 2nd/3rd bells in a Double/Triple sequence land 120ms+ later,
/// which can overlap with subsequent frame flushes. `\x07` mid-escape
/// is undefined per the VT spec but is silently dropped by iTerm2 and
/// other modern terminals; visible corruption has not been observed.
/// The fully race-free alternative is a synchronized bell worker
/// coordinating with the TUI backend — non-trivial refactor for a
/// theoretical issue. Reassess if real-world corruption appears.
fn fire_bell(state: ActivityState, store: &crate::store::Store) {
    use std::io::Write;
    let pattern = bell_pattern_for(state, store);
    let count = match pattern {
        BellPattern::Off => return,
        BellPattern::Single => 1,
        BellPattern::Double => 2,
        BellPattern::Triple => 3,
    };
    if count == 1 {
        let _ = std::io::stdout().write_all(b"\x07");
        let _ = std::io::stdout().flush();
        return;
    }
    std::thread::spawn(move || {
        for i in 0..count {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(120));
            }
            let _ = std::io::stdout().write_all(b"\x07");
            let _ = std::io::stdout().flush();
        }
    });
}

async fn handle_event(app: &mut App, evt: CtEvent) -> Result<()> {
    match evt {
        CtEvent::Key(k) if k.kind == KeyEventKind::Press => dispatch_key(app, k).await?,
        CtEvent::Mouse(m) => handle_mouse(app, m).await,
        CtEvent::Paste(content) => handle_paste(app, content).await?,
        CtEvent::Resize(_, _) => {}
        _ => {}
    }
    Ok(())
}

async fn dispatch_key(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    if app.modal.is_some() {
        handle_key_modal(app, k).await?;
    } else {
        match &app.view {
            View::Dashboard => handle_key_dashboard(app, k).await?,
            View::Attached(state) => {
                let id = match state.focused_id() {
                    Some(id) => id,
                    None => {
                        app.view = View::Dashboard;
                        return Ok(());
                    }
                };
                handle_key_attached(app, id, k).await?
            }
            View::AttachedPm => handle_key_attached_pm(app, k).await?,
        }
    }
    Ok(())
}

/// Wrap a paste payload with the bracketed-paste escape markers claude
/// reads to render `[Pasted N lines]` instead of treating the content as
/// typed input. The output is what gets written to the PTY in one send.
pub(crate) fn wrap_paste_bytes(content: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + 12);
    out.extend_from_slice(b"\x1b[200~");
    out.extend_from_slice(content.as_bytes());
    out.extend_from_slice(b"\x1b[201~");
    out
}

/// Translate a pasted character into the `KeyEvent` crossterm would have
/// emitted if it were typed live. Matters for the non-attached fallback:
/// `\n`/`\r` are Enter (modal submit), `\t` is Tab (focus / autocomplete),
/// printable chars pass through as `Char(c)`.
fn paste_char_to_key(c: char) -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyEvent, KeyModifiers};
    let code = match c {
        '\n' | '\r' => KeyCode::Enter,
        '\t' => KeyCode::Tab,
        _ => KeyCode::Char(c),
    };
    KeyEvent::new(code, KeyModifiers::NONE)
}

async fn handle_paste(app: &mut App, content: String) -> Result<()> {
    // PTY path: forward the whole paste as one bracketed sequence to
    // whichever session is currently driving the foreground (attached
    // workspace, full-screen PM, or the embedded PM pane when focused
    // on the dashboard). When a modal owns the input (e.g. New Workspace
    // name field), skip this branch so the per-char fallback can feed
    // the modal handler.
    let session = if app.modal.is_none() {
        active_session(app)
    } else {
        None
    };
    if let Some(session) = session {
        session.scroll_to_live();
        let _ = session.writer.send(wrap_paste_bytes(&content)).await;
        return Ok(());
    }
    // Non-attached fallback: forward each char as if typed, translating
    // control chars to the KeyCodes crossterm would have emitted live so
    // modal handlers see paste-with-newlines as multiple Enter presses
    // rather than literal '\n' Chars.
    for c in content.chars() {
        dispatch_key(app, paste_char_to_key(c)).await?;
    }
    Ok(())
}

async fn handle_mouse(app: &App, m: MouseEvent) {
    match m.kind {
        MouseEventKind::ScrollUp => scroll_active(app, 3, true),
        MouseEventKind::ScrollDown => scroll_active(app, 3, false),
        MouseEventKind::Down(MouseButton::Left) => {
            if let Some(idx) = app.chip_rects.iter().position(|r| {
                m.column >= r.x
                    && m.column < r.x.saturating_add(r.width)
                    && m.row >= r.y
                    && m.row < r.y.saturating_add(r.height)
            }) {
                if let Some(cmd) = app.pinned_commands_cache.get(idx) {
                    if let Some(session) = active_session(app) {
                        let mut bytes = cmd.command.as_bytes().to_vec();
                        bytes.push(b'\r');
                        session.scroll_to_live();
                        let _ = session.writer.send(bytes).await;
                    }
                }
            }
        }
        _ => {}
    }
}

/// Apply a scroll delta to whichever session is currently in focus.
/// `up=true` scrolls toward older content (higher offset).
fn scroll_active(app: &App, rows: usize, up: bool) {
    let Some(session) = active_session(app) else {
        return;
    };
    if up {
        session.scroll_up(rows);
    } else {
        session.scroll_down(rows);
    }
}

/// Returns the session that should receive scroll input given the current
/// view + focus, or None when there is no targetable session.
fn active_session(app: &App) -> Option<std::sync::Arc<crate::pty::session::Session>> {
    match &app.view {
        View::Attached(state) => state.focused_id().and_then(|id| app.sessions.get(id)),
        View::AttachedPm => app.pm.clone(),
        View::Dashboard
            if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) =>
        {
            app.pm.clone()
        }
        _ => None,
    }
}

async fn handle_key_dashboard(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    // PM pane focus handling. When PM is focused, all keystrokes forward
    // to its PTY — including 'p' and 'r' (typing words containing those
    // letters must not toggle the pane or trigger refresh). To use the
    // dashboard's 'p' / 'r' shortcuts, the user presses Tab/Esc first to
    // return focus to the dashboard.
    if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) {
        match (k.code, k.modifiers) {
            (KeyCode::Tab, _) | (KeyCode::Esc, _) => {
                app.focus = crate::ui::PaneFocus::Dashboard;
                return Ok(());
            }
            (KeyCode::Char('o'), m) if m.contains(KeyModifiers::CONTROL) => {
                // Ctrl-O: expand PM to a full-screen attached view so the
                // user can scroll through claude's history naturally.
                if app.pm.is_some() {
                    app.view = View::AttachedPm;
                }
                return Ok(());
            }
            _ => {
                if let Some(session) = app.pm.as_ref() {
                    if let Some(bytes) = encode_key_for_pty(&k) {
                        session.scroll_to_live();
                        let _ = session.writer.send(bytes).await;
                    }
                }
                return Ok(());
            }
        }
    }
    // Tab when focus is on Dashboard and PM is visible: swap to PM.
    if app.pm_visible
        && matches!(app.focus, crate::ui::PaneFocus::Dashboard)
        && k.code == KeyCode::Tab
    {
        app.focus = crate::ui::PaneFocus::ProjectManager;
        return Ok(());
    }
    match (k.code, k.modifiers) {
        (KeyCode::Char('q'), _) => app.quit = true,
        (KeyCode::Up, _) => {
            let max = app.selectable.len().saturating_sub(1);
            app.dashboard.selected = if app.dashboard.selected == 0 {
                max
            } else {
                app.dashboard.selected - 1
            };
        }
        (KeyCode::Down, _) => {
            let max = app.selectable.len().saturating_sub(1);
            app.dashboard.selected = if app.dashboard.selected >= max {
                0
            } else {
                app.dashboard.selected + 1
            };
        }
        (KeyCode::Enter, _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => {
                app.workspace_needs_attention.remove(&id);
                if let Some((id, path, mode, repo_path)) = build_spawn_info(app, id) {
                    maybe_mirror_mcp(app, &repo_path, &path);
                    let remote = crate::remote::RemoteOpts::from_store(&app.store);
                    let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote)?;
                    app.view = View::Attached(AttachedState::single(id));
                }
            }
            Some(SelectionTarget::Repo(id)) => {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id: id,
                    name_buffer: String::new(),
                    yolo: false,
                });
            }
            None => {}
        },
        (KeyCode::Char('n'), _) | (KeyCode::Char('N'), _) => {
            // Resolve target repo from the current selection. Falls back to the
            // first repo if nothing is selected (shouldn't normally happen).
            // Capital N opens the modal in YOLO mode (claude launches with
            // --dangerously-skip-permissions on every attach).
            let yolo = matches!(k.code, KeyCode::Char('N'));
            let repo_id = match app.selected_target() {
                Some(SelectionTarget::Repo(id)) => Some(id),
                Some(SelectionTarget::Workspace(wid)) => app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == wid)
                    .map(|(rid, _)| *rid),
                None => app.repos.first().map(|r| r.id),
            };
            if let Some(id) = repo_id {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id: id,
                    name_buffer: String::new(),
                    yolo,
                });
            }
        }
        (KeyCode::Char('e'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let info = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(rid, w)| (*rid, w.worktree_path.clone()));
                if let Some((_, path)) = info {
                    let cmd = app.store.get_setting("editor_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_editor(&path, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
        (KeyCode::Char('t'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let info = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(rid, w)| (*rid, w.worktree_path.clone()));
                if let Some((_, path)) = info {
                    let cmd = app.store.get_setting("terminal_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_terminal(&path, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
        }
        (KeyCode::Char('v'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let info = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = info {
                    let cmd = app.store.get_setting("diff_cmd").ok().flatten();
                    let base = crate::git::resolve_base_branch(&path).await;
                    if let Err(e) = crate::external::open_diff(&path, &base, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
            // 'v' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('k'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                });
            }
            // 'k' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('s'), _) => {
            let repo_id = match app.selected_target() {
                Some(SelectionTarget::Repo(id)) => Some(id),
                Some(SelectionTarget::Workspace(wid)) => app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == wid)
                    .map(|(rid, _)| *rid),
                None => app.repos.first().map(|r| r.id),
            };
            if let Some(id) = repo_id {
                app.modal = Some(Modal::RepoSettings {
                    repo_id: id,
                    selected: 0,
                });
            }
        }
        (KeyCode::Char('d'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let name = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.name.clone());
                if let Some(name) = name {
                    app.modal = Some(Modal::ConfirmArchive {
                        workspace_id: id,
                        name,
                    });
                }
            }
            // 'd' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('r'), _)
            if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::Dashboard) =>
        {
            // Manual refresh of the PM pane. Only fires from Dashboard focus
            // so 'r' typed inside PM (when PM is focused) goes to the PTY.
            let dirs = crate::config::Dirs::discover();
            let pm_dir = dirs.pm_dir();
            if let Err(e) = crate::pm::refresh_pm(&mut app.sessions, &app.store, &pm_dir).await {
                app.modal = Some(Modal::Error {
                    message: e.to_string(),
                });
            }
        }
        (KeyCode::Char('p'), _) if pm_enabled(&app.store) => {
            if app.pm_visible {
                // Hide pane; session stays alive.
                app.pm_visible = false;
                app.focus = crate::ui::PaneFocus::Dashboard;
            } else {
                // Open pane. Spawn if not yet spawned this run.
                let dirs = crate::config::Dirs::discover();
                let pm_dir = dirs.pm_dir();
                let custom = app
                    .store
                    .get_setting("pm_custom_instructions")
                    .ok()
                    .flatten();
                let result = if app.pm_auto_summary_sent {
                    // Reopen path: refresh so PM picks up workspace
                    // changes that happened while the pane was hidden.
                    crate::pm::open_pm_with_refresh(&mut app.sessions, &app.store, &pm_dir, custom)
                        .await
                } else {
                    crate::pm::open_pm_with_auto_summary(
                        &mut app.sessions,
                        &app.store,
                        &pm_dir,
                        custom,
                    )
                    .await
                };
                if let Err(e) = result {
                    app.modal = Some(Modal::Error {
                        message: e.to_string(),
                    });
                    return Ok(());
                }
                app.pm_auto_summary_sent = true;
                app.pm = app.sessions.pm();
                app.pm_visible = true;
                app.focus = crate::ui::PaneFocus::ProjectManager;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Immediately re-run `proc::scan` and re-bucket. Used after a kill
/// so the modal reflects the new state without waiting for the
/// next 10s poll tick.
async fn rescan_processes(app: &mut App) {
    let procs = crate::proc::scan().await;
    let worktrees: Vec<(crate::store::WorkspaceId, std::path::PathBuf)> = app
        .workspaces
        .iter()
        .map(|(_, w)| (w.id, w.worktree_path.clone()))
        .collect();
    let worktree_refs: Vec<(crate::store::WorkspaceId, &std::path::Path)> = worktrees
        .iter()
        .map(|(id, path)| (*id, path.as_path()))
        .collect();
    app.workspace_processes = crate::proc::bucket_by_worktree(&procs, &worktree_refs);
    app.last_proc_scan_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    // Clamp the modal's `selected` index after the list size changes.
    // Read workspace_id out first (Copy) to avoid a simultaneous
    // borrow of `app.workspace_processes` and `app.modal`.
    let modal_ws_id = match &app.modal {
        Some(Modal::ProcessList { workspace_id, .. }) => Some(*workspace_id),
        _ => None,
    };
    if let Some(ws_id) = modal_ws_id {
        let len = app
            .workspace_processes
            .get(&ws_id)
            .map(|v| v.len())
            .unwrap_or(0);
        if let Some(Modal::ProcessList { selected, .. }) = &mut app.modal {
            *selected = if len == 0 {
                0
            } else {
                (*selected).min(len - 1)
            };
        }
    }
}

fn apply_repo_setting(
    app: &mut App,
    repo_id: crate::store::RepoId,
    field: RepoSettingField,
    value: &str,
) -> Result<()> {
    let trimmed = value.trim();
    let opt = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    match field {
        RepoSettingField::BranchPrefix => app.store.set_repo_branch_prefix(repo_id, trimmed),
        RepoSettingField::CustomInstructions => {
            app.store.set_repo_custom_instructions(repo_id, opt)
        }
        RepoSettingField::SetupScript => app.store.set_repo_setup_script(repo_id, opt),
        RepoSettingField::ArchiveScript => app.store.set_repo_archive_script(repo_id, opt),
        RepoSettingField::PinnedCommands => app.store.set_repo_pinned_commands(repo_id, opt),
        RepoSettingField::RelatedRepos => app.store.set_repo_related_repos(repo_id, opt),
    }
}

fn build_spawn_info(
    app: &App,
    ws_id: crate::store::WorkspaceId,
) -> Option<(
    crate::store::WorkspaceId,
    std::path::PathBuf,
    crate::pty::session::SpawnMode,
    std::path::PathBuf,
)> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let custom = crate::repo::resolve_custom_instructions(repo, &app.store)
        .ok()
        .flatten();
    let yolo = ws.yolo;
    // Resolve related repos (per-repo names → source paths), filter out
    // the spawning repo itself, build the read-only system-prompt
    // fragment, and fold it into custom_instructions before claude sees it.
    let resolved = crate::related::resolve(repo.related_repos.as_deref(), &app.repos);
    let resolved: Vec<(String, std::path::PathBuf)> = resolved
        .into_iter()
        .filter(|(_, p)| p != &repo.path)
        .collect();
    let additional_dirs: Vec<std::path::PathBuf> =
        resolved.iter().map(|(_, p)| p.clone()).collect();
    let related_prompt = crate::related::build_read_only_prompt(&resolved);
    let custom = match (custom, related_prompt) {
        (None, None) => None,
        (Some(c), None) => Some(c),
        (None, Some(r)) => Some(r),
        (Some(c), Some(r)) => Some(format!("{c}\n\n{r}")),
    };
    let mode = if crate::pty::session::has_prior_session(&ws.worktree_path) {
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
            additional_dirs,
            yolo,
        }
    } else {
        let rename_ctx = if crate::names::is_generated_slug(&ws.name) {
            let resolved_prefix =
                crate::repo::resolve_branch_prefix(repo, &app.store).unwrap_or_default();
            Some(crate::pty::session::RenameContext {
                current_branch: ws.branch.clone(),
                branch_prefix: resolved_prefix,
            })
        } else {
            None
        };
        crate::pty::session::SpawnMode::Fresh {
            rename_ctx,
            custom_instructions: custom,
            additional_dirs,
            yolo,
        }
    };
    Some((ws_id, ws.worktree_path.clone(), mode, repo.path.clone()))
}

/// Best-effort MCP server mirror. Logs and continues on any failure.
fn maybe_mirror_mcp(app: &App, repo_path: &std::path::Path, worktree_path: &std::path::Path) {
    if !crate::mcp::enabled(&app.store) {
        return;
    }
    if let Err(e) = crate::mcp::mirror_mcp_servers(repo_path, worktree_path) {
        tracing::warn!(error = %e, "failed to mirror MCP servers; continuing");
    }
}

async fn handle_key_attached(
    app: &mut App,
    id: WorkspaceId,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let session = match app.sessions.get(id) {
        Some(s) => s,
        None => {
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    // Leader-key prefix handling. See `LEADER_KEY`.
    if app.leader_pending {
        app.leader_pending = false;
        match k.code {
            KeyCode::Char('d') => {
                // In multi-pane mode, close just the focused pane; the
                // other panes' sessions keep running. Detach to dashboard
                // only when the last pane closes.
                if let View::Attached(state) = &mut app.view {
                    if state.leaf_count() > 1 {
                        match state.close_focused() {
                            CloseOutcome::Focus(_) => return Ok(()),
                            CloseOutcome::Empty => {
                                app.view = View::Dashboard;
                                return Ok(());
                            }
                        }
                    }
                }
                app.view = View::Dashboard;
                return Ok(());
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                let arrow = match k.code {
                    KeyCode::Left => Arrow::Left,
                    KeyCode::Right => Arrow::Right,
                    KeyCode::Up => Arrow::Up,
                    KeyCode::Down => Arrow::Down,
                    _ => unreachable!(),
                };
                if let View::Attached(state) = &mut app.view {
                    state.focus_direction(arrow);
                }
                return Ok(());
            }
            KeyCode::Char('x') => {
                // Send a literal Ctrl-x (0x18) to claude.
                session.scroll_to_live();
                let _ = session.writer.send(vec![0x18]).await;
                return Ok(());
            }
            KeyCode::Char('u') => {
                app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
                return Ok(());
            }
            KeyCode::Char('e') => {
                let path = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = path {
                    let cmd = app.store.get_setting("editor_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_editor(&path, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
                return Ok(());
            }
            KeyCode::Char('t') => {
                let path = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = path {
                    let cmd = app.store.get_setting("terminal_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_terminal(&path, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
                return Ok(());
            }
            KeyCode::Char('v') => {
                let path = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = path {
                    let cmd = app.store.get_setting("diff_cmd").ok().flatten();
                    let base = crate::git::resolve_base_branch(&path).await;
                    if let Err(e) = crate::external::open_diff(&path, &base, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
                return Ok(());
            }
            KeyCode::Char('k') => {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                });
                return Ok(());
            }
            KeyCode::Char(c @ '1'..='9') => {
                let idx = (c as u8 - b'1') as usize;
                if let Some(cmd) = app.pinned_commands_cache.get(idx) {
                    let mut bytes = cmd.command.as_bytes().to_vec();
                    bytes.push(b'\r');
                    session.scroll_to_live();
                    let _ = session.writer.send(bytes).await;
                }
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        return Ok(());
    }
    let bytes = encode_key(k);
    if !bytes.is_empty() {
        session.scroll_to_live();
        let _ = session.writer.send(bytes).await;
    }
    // Auto-rename capture (local mode only): buffer printable chars; on Enter,
    // attempt rename if the workspace name is still a generated slug. In the
    // default `claude` mode the rename happens via system-prompt + branch
    // poller, so the PTY-interception path stays inert.
    let mode = std::env::var("WSX_RENAME_MODE").unwrap_or_else(|_| "claude".to_string());
    if mode == "local" {
        match k.code {
            KeyCode::Char(c) if !k.modifiers.contains(KeyModifiers::CONTROL) => {
                session.capture_char(c)
            }
            KeyCode::Backspace => session.capture_backspace(),
            KeyCode::Enter => {
                if let Some(prompt) = session.take_first_prompt() {
                    if let Some(slug) = crate::workspace::slugify_prompt(&prompt) {
                        let ws_info = app
                            .workspaces
                            .iter()
                            .find(|(_, w)| w.id == id)
                            .map(|(_, w)| w.clone());
                        if let Some(ws) = ws_info {
                            if crate::names::is_generated_slug(&ws.name) {
                                let repo = app.repos.iter().find(|r| r.id == ws.repo_id).cloned();
                                if let Some(repo) = repo {
                                    // Fire-and-forget: rename failure shouldn't disrupt the keystroke.
                                    let _ = crate::workspace::rename(&app.store, &repo, &ws, &slug)
                                        .await;
                                    app.refresh()?;
                                }
                            }
                        }
                    }
                }
            }
            _ => {} // arrows, function keys, etc. — not part of the prompt
        }
    }
    Ok(())
}

fn encode_key(k: crossterm::event::KeyEvent) -> Vec<u8> {
    use KeyCode::*;
    match k.code {
        Char(c) => {
            if k.modifiers.contains(KeyModifiers::CONTROL) && c.is_ascii_alphabetic() {
                vec![(c.to_ascii_lowercase() as u8) - b'a' + 1]
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf).as_bytes().to_vec()
            }
        }
        Enter => b"\r".to_vec(),
        Backspace => b"\x7f".to_vec(),
        Tab => b"\t".to_vec(),
        Esc => b"\x1b".to_vec(),
        Left => b"\x1b[D".to_vec(),
        Right => b"\x1b[C".to_vec(),
        Up => b"\x1b[A".to_vec(),
        Down => b"\x1b[B".to_vec(),
        _ => vec![],
    }
}

async fn handle_key_attached_pm(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    let session = match app.pm.clone() {
        Some(s) => s,
        None => {
            app.view = View::Dashboard;
            return Ok(());
        }
    };
    if app.leader_pending {
        app.leader_pending = false;
        match k.code {
            KeyCode::Char('d') => {
                app.view = View::Dashboard;
                return Ok(());
            }
            KeyCode::Char('x') => {
                // Send a literal Ctrl-x (0x18) to claude.
                session.scroll_to_live();
                let _ = session.writer.send(vec![0x18]).await;
                return Ok(());
            }
            KeyCode::Char('u') => {
                app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        return Ok(());
    }
    let bytes = encode_key(k);
    if !bytes.is_empty() {
        session.scroll_to_live();
        let _ = session.writer.send(bytes).await;
    }
    Ok(())
}

async fn handle_key_modal(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    let modal = app.modal.clone().unwrap();
    match modal {
        Modal::NewWorkspace {
            repo_id,
            mut name_buffer,
            yolo,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Enter => {
                // Live-log streaming during create is intentionally deferred. The
                // borrow checker would force a channel-based dance to mutate
                // `app.modal` while `app.store` is borrowed inside `workspace::create`.
                // v1: show a static "running..." modal, swap it for the result.
                let name = if name_buffer.trim().is_empty() {
                    None
                } else {
                    Some(name_buffer.clone())
                };
                let repo = app.repos.iter().find(|r| r.id == repo_id).unwrap().clone();
                let base = app.worktree_base.clone();
                app.modal = Some(Modal::SetupRunning {
                    log: vec!["running setup...".into()],
                });
                let result = crate::workspace::create(
                    &app.store,
                    &repo,
                    name.as_deref(),
                    &base,
                    yolo,
                    |_| {},
                )
                .await;
                match result {
                    Ok(_) => {
                        app.modal = None;
                        app.refresh()?;
                    }
                    Err(e) => {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
            KeyCode::Backspace => {
                name_buffer.pop();
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                });
            }
            KeyCode::Char(c) => {
                name_buffer.push(c);
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                });
            }
            _ => {}
        },
        Modal::ConfirmArchive { workspace_id, name } => match k.code {
            KeyCode::Char('y') => {
                let (repo, ws) = {
                    let ws = app
                        .workspaces
                        .iter()
                        .find(|(_, w)| w.id == workspace_id)
                        .map(|(_, w)| w.clone());
                    let repo = ws
                        .as_ref()
                        .and_then(|w| app.repos.iter().find(|r| r.id == w.repo_id).cloned());
                    match (repo, ws) {
                        (Some(r), Some(w)) => (r, w),
                        _ => {
                            app.modal = None;
                            return Ok(());
                        }
                    }
                };
                let result = crate::workspace::archive(
                    &app.store,
                    &repo,
                    &ws,
                    crate::workspace::ArchiveOpts {
                        force_branch_delete: true,
                        ..Default::default()
                    },
                    |_| {},
                )
                .await;
                match result {
                    Ok(_) => {
                        app.modal = None;
                        app.refresh()?;
                    }
                    Err(e) => {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
                let _ = name;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                app.modal = None;
            }
            _ => {}
        },
        Modal::Error { .. } | Modal::SetupRunning { .. } => {
            if matches!(k.code, KeyCode::Esc | KeyCode::Enter) {
                app.modal = None;
            }
        }
        Modal::UpdatesPanel { selected } => {
            let selected_now = selected;
            // Build the same ordered workspace list the renderer uses, so
            // arrow keys and Enter operate on the same indices.
            let activity_translated: std::collections::HashMap<
                crate::store::WorkspaceId,
                crate::ui::updates_bar::ActivityState,
            > = app
                .workspace_activity
                .iter()
                .map(|(k, v)| (*k, translate_activity(*v)))
                .collect();
            let order = crate::ui::modal::ordered_workspaces_for_panel(
                &app.repos,
                &app.workspaces,
                &app.workspace_events,
                &activity_translated,
                &app.workspace_needs_attention,
            );
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up => {
                    let new_sel = selected_now.saturating_sub(1);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Down => {
                    let max = order.len().saturating_sub(1);
                    let new_sel = (selected_now + 1).min(max);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Enter => {
                    if let Some(ws_id) = order.get(selected_now).copied() {
                        // Mirror the dashboard-attach flow: clear the
                        // alert, spawn (or resume) the PTY, switch view.
                        app.workspace_needs_attention.remove(&ws_id);
                        if let Some((id, path, mode, repo_path)) = build_spawn_info(app, ws_id) {
                            maybe_mirror_mcp(app, &repo_path, &path);
                            let remote = crate::remote::RemoteOpts::from_store(&app.store);
                            let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote)?;
                            app.view = View::Attached(AttachedState::single(id));
                        }
                    }
                    app.modal = None;
                }
                KeyCode::Char('v') | KeyCode::Char('s') => {
                    // Vim-style splits: 'v' = vertical (panes side-by-side),
                    // 's' = horizontal (stacked). Only valid when there's
                    // already an attached pane to split — otherwise behaves
                    // like Enter (just attach).
                    let dir = if matches!(k.code, KeyCode::Char('v')) {
                        SplitDirection::Vertical
                    } else {
                        SplitDirection::Horizontal
                    };
                    if let Some(ws_id) = order.get(selected_now).copied() {
                        app.workspace_needs_attention.remove(&ws_id);
                        if let Some((id, path, mode, repo_path)) = build_spawn_info(app, ws_id) {
                            maybe_mirror_mcp(app, &repo_path, &path);
                            let remote = crate::remote::RemoteOpts::from_store(&app.store);
                            let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote)?;
                            match &mut app.view {
                                View::Attached(state) => {
                                    // Same pane already focused: switch focus
                                    // instead of splitting onto itself.
                                    if state.focused_id() == Some(id) {
                                        // no-op
                                    } else if state.leaves().contains(&id) {
                                        // Already open in another pane —
                                        // just refocus there.
                                        if let Some(p) = state
                                            .tree
                                            .leaf_paths()
                                            .into_iter()
                                            .find(|p| state.tree.leaf_at(p) == Some(id))
                                        {
                                            state.focus = p;
                                        }
                                    } else {
                                        state.split(dir, id);
                                    }
                                }
                                _ => {
                                    // No attached pane yet — attach plainly.
                                    app.view = View::Attached(AttachedState::single(id));
                                }
                            }
                        }
                    }
                    app.modal = None;
                }
                _ => {}
            }
        }
        Modal::ProcessList {
            workspace_id,
            mut selected,
        } => {
            let procs = app
                .workspace_processes
                .get(&workspace_id)
                .cloned()
                .unwrap_or_default();
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                    });
                }
                KeyCode::Down => {
                    if !procs.is_empty() {
                        selected = (selected + 1).min(procs.len() - 1);
                    }
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                    });
                }
                KeyCode::Char('k') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::proc::kill_pid(p.pid, "TERM").await;
                        rescan_processes(app).await;
                    }
                }
                KeyCode::Char('K') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::proc::kill_pid(p.pid, "KILL").await;
                        rescan_processes(app).await;
                    }
                }
                _ => {}
            }
        }
        Modal::RepoSettings {
            repo_id,
            mut selected,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Up => {
                selected = selected.saturating_sub(1);
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            KeyCode::Down => {
                let max = RepoSettingField::ALL.len() - 1;
                selected = (selected + 1).min(max);
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            KeyCode::Enter => {
                let field = RepoSettingField::ALL
                    [selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
                app.pending_edit = Some(PendingEdit { repo_id, field });
                app.modal = None;
            }
            KeyCode::Char('d') => {
                let field = RepoSettingField::ALL
                    [selected.min(RepoSettingField::ALL.len().saturating_sub(1))];
                let _ = apply_repo_setting(app, repo_id, field, "");
                let _ = app.refresh();
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            _ => {}
        },
    }
    Ok(())
}

fn compute_attention_line(
    app: &App,
    attached_id: Option<crate::store::WorkspaceId>,
    max_width: usize,
) -> Option<String> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let candidates: Vec<crate::ui::updates_bar::WorkspaceUpdateInfo> = app
        .workspaces
        .iter()
        .map(|(rid, w)| {
            let activity = app
                .workspace_activity
                .get(&w.id)
                .copied()
                .map(translate_activity)
                .unwrap_or(crate::ui::updates_bar::ActivityState::Off);
            let repo_name = app
                .repos
                .iter()
                .find(|r| r.id == *rid)
                .map(|r| r.name.as_str())
                .unwrap_or("");
            crate::ui::updates_bar::WorkspaceUpdateInfo {
                id: w.id,
                name: w.name.as_str(),
                repo_name,
                events: app.workspace_events.get(&w.id),
                activity,
                needs_attention: app.workspace_needs_attention.contains(&w.id),
                awaiting_tool: app.awaiting_permission(w.id),
            }
        })
        .collect();
    let entries = crate::ui::updates_bar::collect_attention(&candidates, attached_id, now_ms);
    crate::ui::updates_bar::format_attention_line(&entries, now_ms, max_width)
}

fn translate_activity(a: ActivityState) -> crate::ui::updates_bar::ActivityState {
    use crate::ui::updates_bar::ActivityState as U;
    match a {
        ActivityState::AwaitingAnswer => U::AwaitingAnswer,
        ActivityState::Complete => U::Complete,
        ActivityState::Awaiting => U::Awaiting,
        ActivityState::Active => U::Active,
        ActivityState::Idle => U::Idle,
        ActivityState::Stalled => U::Stalled,
        ActivityState::Waiting => U::Waiting,
        ActivityState::Off => U::Off,
    }
}

/// Periodically check each live workspace's current git branch against
/// the DB; if claude (or a user) renamed it, update name + branch in the
/// store. Runs forever; cheap when nothing has drifted.
pub async fn branch_drift_poll(app: SharedApp) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let snapshot: Vec<(WorkspaceId, std::path::PathBuf, String, String)> = {
            let g = app.lock().await;
            g.workspaces
                .iter()
                .filter_map(|(_, w)| {
                    let repo = g.repos.iter().find(|r| r.id == w.repo_id)?;
                    let prefix =
                        crate::repo::resolve_branch_prefix(repo, &g.store).unwrap_or_default();
                    Some((w.id, w.worktree_path.clone(), w.branch.clone(), prefix))
                })
                .collect()
        };

        for (id, path, db_branch, prefix) in snapshot {
            if !path.exists() {
                continue;
            }

            // 1) Branch drift (existing logic).
            if let Ok(current) = crate::git::current_branch(&path).await {
                if current != db_branch && current != "HEAD" {
                    let new_name = if prefix.is_empty() {
                        current.clone()
                    } else {
                        let strip = format!("{}/", prefix.trim_end_matches('/'));
                        current.strip_prefix(&strip).unwrap_or(&current).to_string()
                    };
                    let mut g = app.lock().await;
                    let _ = g.store.rename_workspace(id, &new_name);
                    let _ = g.store.set_workspace_branch(id, &current);
                    let _ = g.refresh();
                    // Invalidate cached PR state — the new branch may have a
                    // different (or no) PR. Clearing the throttle stamp
                    // makes the next tick poll immediately.
                    g.pr_lifecycle.remove(&id);
                    g.pr_last_poll_ms.remove(&id);
                }
            }

            // 2) Workspace status — refresh the cache for this workspace.
            if let Ok(status) = crate::git::workspace_status(&path).await {
                let mut g = app.lock().await;
                g.workspace_status.insert(id, status);
            }

            // 3) PR lifecycle — throttled to once per 30s per workspace.
            //    gh is a network call, so we don't run it every tick.
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let should_poll_pr = {
                let g = app.lock().await;
                g.pr_last_poll_ms
                    .get(&id)
                    .map(|t| now_ms.saturating_sub(*t) >= 30_000)
                    .unwrap_or(true)
            };
            if should_poll_pr {
                // Mark the attempt before awaiting the fetch, so concurrent
                // ticks don't queue up multiple gh processes.
                {
                    let mut g = app.lock().await;
                    g.pr_last_poll_ms.insert(id, now_ms);
                }
                if let Ok(Some(lifecycle)) =
                    crate::forge::fetch_branch_lifecycle(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, lifecycle);
                }
                // Ok(None) → leave any existing cached value alone; better
                // than clobbering a previously-known state on a transient
                // network error.
            }

            // 4) Tail Claude Code session JSONL for events.
            //
            // Lock-ordering: snapshot the previous offset under the lock,
            // do the file I/O without the lock held, then re-acquire to
            // commit the new offset + events. This keeps the UI responsive
            // even when sessions grow large.
            let current_file = crate::events::locate_session_file(&path);
            let prev_offset = {
                let g = app.lock().await;
                match (g.workspace_events.get(&id), current_file.as_ref()) {
                    (Some(evt), Some(f)) if evt.file_path.as_deref() == Some(f.as_path()) => {
                        evt.byte_offset
                    }
                    _ => 0,
                }
            };
            if let Some(file) = current_file {
                if let Ok(update) = crate::events::tail_session(&file, prev_offset) {
                    let crate::events::TailUpdate {
                        new_offset,
                        events,
                        tool_use_starts,
                        tool_use_resolves,
                        last_stop_reason,
                        human_replied_after_last_stop,
                        reset_from_zero,
                        last_assistant_text,
                    } = update;
                    let mut g = app.lock().await;
                    let evt = g.workspace_events.entry(id).or_default();
                    // If the session file was replaced (different path) or
                    // truncated/rewound (reset_from_zero), discard all
                    // session-derived state before applying the new batch.
                    // Otherwise stale tool_uses or stop_reasons from the
                    // prior session keep the dashboard stuck on "awaiting".
                    let file_changed = evt.file_path.as_deref() != Some(file.as_path());
                    if file_changed || reset_from_zero {
                        evt.reset_session_state();
                    }
                    if new_offset != prev_offset {
                        // The log grew this iteration — stamp the activity
                        // marker so is_stalled can compute time-since-last-write.
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        evt.last_log_activity_ms = now_ms;
                    }
                    evt.file_path = Some(file);
                    evt.byte_offset = new_offset;
                    for (tu_id, tu_name, ts) in tool_use_starts {
                        evt.pending_tool_uses.insert(tu_id, (tu_name, ts));
                    }
                    for tu_id in tool_use_resolves {
                        evt.pending_tool_uses.remove(&tu_id);
                    }
                    // Update the "agent is waiting on user" tracking.
                    // - A fresh assistant stop_reason replaces the prior one
                    //   and resets the user-replied latch (the agent just
                    //   produced a new stopping point).
                    // - `human_replied_after_last_stop` from this batch
                    //   already accounts for within-batch ordering: it's set
                    //   only if real user text appears AFTER the last
                    //   stop_reason in the batch (or anywhere in the batch
                    //   if there's no new stop_reason).
                    if let Some(sr) = last_stop_reason {
                        evt.last_stop_reason = Some(sr);
                        evt.user_replied_since_stop = false;
                    }
                    if human_replied_after_last_stop {
                        evt.user_replied_since_stop = true;
                    }
                    if let Some(text) = last_assistant_text {
                        evt.last_assistant_text = Some(text);
                    }
                    for e in events {
                        crate::events::push_event(evt, e);
                    }
                }
            }
        }

        // 5) Per-workspace process scan. Throttled to once per 10 s globally —
        //    lsof returns everything in a single call, so we don't pay per-workspace.
        let should_scan = {
            let g = app.lock().await;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            now_ms.saturating_sub(g.last_proc_scan_ms) >= 10_000
        };
        if should_scan {
            let procs = crate::proc::scan().await;
            let worktrees: Vec<(crate::store::WorkspaceId, std::path::PathBuf)> = {
                let g = app.lock().await;
                g.workspaces
                    .iter()
                    .map(|(_, w)| (w.id, w.worktree_path.clone()))
                    .collect()
            };
            let worktree_refs: Vec<(crate::store::WorkspaceId, &std::path::Path)> = worktrees
                .iter()
                .map(|(id, path)| (*id, path.as_path()))
                .collect();
            let bucketed = crate::proc::bucket_by_worktree(&procs, &worktree_refs);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let mut g = app.lock().await;
            g.workspace_processes = bucketed;
            g.last_proc_scan_ms = now_ms;
        }
    }
}

#[cfg(test)]
mod activity_classifier_tests {
    use super::*;

    #[test]
    fn awaiting_wins_over_stopped_over_recency() {
        // awaiting (permission) beats everything.
        assert_eq!(
            classify_activity_with_events(
                Some(0),
                true,
                true,
                Some(StoppedKind::Complete),
                false,
            ),
            ActivityState::Awaiting
        );
        assert_eq!(
            classify_activity_with_events(Some(0), true, true, None, false),
            ActivityState::Awaiting
        );
        // stopped beats PTY recency.
        assert_eq!(
            classify_activity_with_events(
                Some(0),
                true,
                false,
                Some(StoppedKind::Complete),
                false,
            ),
            ActivityState::Complete
        );
        assert_eq!(
            classify_activity_with_events(
                Some(0),
                true,
                false,
                Some(StoppedKind::AwaitingAnswer),
                false,
            ),
            ActivityState::AwaitingAnswer
        );
    }

    #[test]
    fn stopped_wins_over_stalled() {
        // If we have a terminal stop_reason waiting on the user, that
        // takes priority over the stall detector.
        assert_eq!(
            classify_activity_with_events(
                Some(0),
                true,
                false,
                Some(StoppedKind::Complete),
                true,
            ),
            ActivityState::Complete
        );
        assert_eq!(
            classify_activity_with_events(
                Some(0),
                true,
                false,
                Some(StoppedKind::AwaitingAnswer),
                true,
            ),
            ActivityState::AwaitingAnswer
        );
    }

    #[test]
    fn stalled_wins_over_pty_recency() {
        // Stall detector fires before PTY-recency Active/Idle/Waiting.
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, None, true),
            ActivityState::Stalled
        );
        assert_eq!(
            classify_activity_with_events(Some(60), true, false, None, true),
            ActivityState::Stalled
        );
    }

    #[test]
    fn no_session_is_off_even_if_running_false() {
        assert_eq!(
            classify_activity_with_events(None, false, false, None, false),
            ActivityState::Off
        );
        // Even with pty seconds, if running=false → Off.
        assert_eq!(
            classify_activity_with_events(Some(5), false, false, None, false),
            ActivityState::Off
        );
    }

    #[test]
    fn awaiting_fires_even_when_session_not_running() {
        // A pending tool_use is a strong signal regardless of pty state.
        assert_eq!(
            classify_activity_with_events(None, false, true, None, false),
            ActivityState::Awaiting
        );
    }

    #[test]
    fn pty_recency_drives_active_idle_waiting_when_no_event_signals() {
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, None, false),
            ActivityState::Active
        );
        assert_eq!(
            classify_activity_with_events(Some(10), true, false, None, false),
            ActivityState::Idle
        );
        assert_eq!(
            classify_activity_with_events(Some(60), true, false, None, false),
            ActivityState::Waiting
        );
    }

    #[test]
    fn is_alertable_includes_stopped_awaiting_and_stalled() {
        assert!(ActivityState::AwaitingAnswer.is_alertable());
        assert!(ActivityState::Complete.is_alertable());
        assert!(ActivityState::Awaiting.is_alertable());
        assert!(ActivityState::Stalled.is_alertable());
        assert!(!ActivityState::Active.is_alertable());
        assert!(!ActivityState::Idle.is_alertable());
        assert!(!ActivityState::Waiting.is_alertable());
        assert!(!ActivityState::Off.is_alertable());
    }
}

#[cfg(test)]
mod pm_state_tests {
    use super::*;
    use crate::store::Store;
    use std::path::PathBuf;

    #[test]
    fn app_initializes_pm_state_off() {
        let store = Store::open_in_memory().unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(app.pm.is_none());
        assert!(!app.pm_visible);
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn dashboard_renders_full_area_when_pm_hidden() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(!app.pm_visible);
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!rendered.contains("Project Manager"), "{rendered}");
    }

    #[test]
    fn dashboard_renders_split_with_pm_title_when_visible_even_without_session() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true; // No session yet — the pane shows a placeholder.
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Project Manager"),
            "expected pane title in rendered buffer:\n{rendered}"
        );
        assert!(
            rendered.contains("Tab to focus"),
            "expected unfocused hint:\n{rendered}"
        );
    }

    use crossterm::event::{KeyEvent, KeyModifiers};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tab_swaps_focus_when_pm_visible() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true;
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn esc_returns_focus_to_dashboard() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::ProjectManager;
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tab_no_op_when_pm_hidden() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(!app.pm_visible);
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Tab, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_down_at_last_entry_wraps_to_first() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![
            SelectionTarget::Repo(crate::store::RepoId(1)),
            SelectionTarget::Repo(crate::store::RepoId(2)),
            SelectionTarget::Repo(crate::store::RepoId(3)),
        ];
        app.dashboard.selected = 2;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(
            app.dashboard.selected, 0,
            "Down at last should wrap to first"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_up_at_first_entry_wraps_to_last() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![
            SelectionTarget::Repo(crate::store::RepoId(1)),
            SelectionTarget::Repo(crate::store::RepoId(2)),
            SelectionTarget::Repo(crate::store::RepoId(3)),
        ];
        app.dashboard.selected = 0;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.selected, 2, "Up at first should wrap to last");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_down_in_middle_advances_normally() {
        // Sanity check that wrap-around didn't break the non-edge case.
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.selectable = vec![
            SelectionTarget::Repo(crate::store::RepoId(1)),
            SelectionTarget::Repo(crate::store::RepoId(2)),
            SelectionTarget::Repo(crate::store::RepoId(3)),
        ];
        app.dashboard.selected = 1;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.selected, 2);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_esc_closes() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_none(), "Esc should close UpdatesPanel");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_down_advances_selection() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        // Two workspaces so Down has somewhere to go.
        for (name, branch, path) in [
            ("alpha", "repo/alpha", "/tmp/wsx-test/alpha"),
            ("beta", "repo/beta", "/tmp/wsx-test/beta"),
        ] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id,
                    name,
                    branch,
                    worktree_path: std::path::Path::new(path),
                    yolo: false,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
                assert_eq!(selected, 1, "Down should advance to index 1");
            }
            other => panic!("unexpected modal state: {other:?}"),
        }
        // Down again clamps at the last index.
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Down, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
                assert_eq!(selected, 1, "Down past last clamps at max");
            }
            other => panic!("unexpected modal state: {other:?}"),
        }
        // Up returns to 0.
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Up, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match app.modal {
            Some(crate::ui::modal::Modal::UpdatesPanel { selected }) => {
                assert_eq!(selected, 0, "Up should retreat to 0");
            }
            other => panic!("unexpected modal state: {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_enter_switches_view_and_clears_attention() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "blocked",
                branch: "repo/blocked",
                worktree_path: std::path::Path::new("."),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.workspace_needs_attention.insert(ws_id);
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_none(), "Enter should close the modal");
        assert!(
            matches!(&app.view, crate::ui::View::Attached(s) if s.focused_id() == Some(ws_id)),
            "Enter should switch view to the selected workspace; got {:?}",
            app.view
        );
        assert!(
            !app.workspace_needs_attention.contains(&ws_id),
            "attention flag should clear on Enter"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_v_splits_attached_view_vertically() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let first_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "first",
                branch: "repo/first",
                worktree_path: std::path::Path::new("/tmp/wsx-split-1"),
                yolo: false,
            })
            .unwrap();
        let second_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "second",
                branch: "repo/second",
                worktree_path: std::path::Path::new("/tmp/wsx-split-2"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(first_id, WorkspaceState::Ready)
            .unwrap();
        store
            .set_workspace_state(second_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Pre-spawn the "first" workspace and attach to it. Use `.` for the
        // spawn cwd so the PTY actually starts; the store-level
        // worktree_path is just a unique key for the row.
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        app.sessions
            .spawn(
                first_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::remote::RemoteOpts::disabled(),
            )
            .unwrap();
        let second_mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        app.sessions
            .spawn(
                second_id,
                std::path::Path::new("."),
                80,
                24,
                second_mode,
                crate::remote::RemoteOpts::disabled(),
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(first_id));

        // Open Updates panel, point at the second workspace, press 'v'.
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        // The renderer's order is grouped/sorted; in this minimal setup both
        // workspaces are in `repo`. Find the index of `second_id` from the
        // module's ordering helper.
        let order = crate::ui::modal::ordered_workspaces_for_panel(
            &app.repos,
            &app.workspaces,
            &app.workspace_events,
            &std::collections::HashMap::new(),
            &std::collections::HashSet::new(),
        );
        let target_idx = order.iter().position(|id| *id == second_id).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
            selected: target_idx,
        });
        handle_key_modal(
            &mut app,
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_none(), "v should close the modal");
        match &app.view {
            crate::ui::View::Attached(state) => {
                assert_eq!(state.leaf_count(), 2, "v should produce a 2-pane split");
                assert!(state.leaves().contains(&first_id));
                assert!(state.leaves().contains(&second_id));
                // Focus should be on the newly added pane.
                assert_eq!(state.focused_id(), Some(second_id));
            }
            other => panic!("expected Attached view; got {other:?}"),
        }
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_d_closes_focused_pane_when_split() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let first_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "first",
                branch: "repo/first",
                worktree_path: std::path::Path::new("/tmp/wsx-close-1"),
                yolo: false,
            })
            .unwrap();
        let second_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "second",
                branch: "repo/second",
                worktree_path: std::path::Path::new("/tmp/wsx-close-2"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(first_id, WorkspaceState::Ready)
            .unwrap();
        store
            .set_workspace_state(second_id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        for id in [first_id, second_id] {
            let mode = crate::pty::session::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                additional_dirs: vec![],
                yolo: false,
            };
            app.sessions
                .spawn(
                    id,
                    std::path::Path::new("."),
                    80,
                    24,
                    mode,
                    crate::remote::RemoteOpts::disabled(),
                )
                .unwrap();
        }
        // Start in a 2-pane split with `second` focused.
        let mut state = AttachedState::single(first_id);
        state.split(SplitDirection::Vertical, second_id);
        app.view = crate::ui::View::Attached(state);

        // Ctrl-x d closes JUST the focused pane; should leave `first` alone.
        handle_key_attached(
            &mut app,
            second_id,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);
        handle_key_attached(
            &mut app,
            second_id,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.view {
            crate::ui::View::Attached(state) => {
                assert_eq!(state.leaf_count(), 1, "should drop down to 1 pane");
                assert_eq!(state.focused_id(), Some(first_id));
            }
            other => panic!("expected Attached view; got {other:?}"),
        }

        // Ctrl-x d on the last pane detaches fully.
        handle_key_attached(
            &mut app,
            first_id,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            first_id,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(app.view, crate::ui::View::Dashboard));
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_arrow_moves_focus_in_split() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let mut ids = Vec::new();
        for name in ["a", "b"] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id,
                    name,
                    branch: &format!("repo/{name}"),
                    worktree_path: &std::path::PathBuf::from(format!("/tmp/wsx-arrow-{name}")),
                    yolo: false,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
            ids.push(id);
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        for id in &ids {
            let mode = crate::pty::session::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                additional_dirs: vec![],
                yolo: false,
            };
            app.sessions
                .spawn(
                    *id,
                    std::path::Path::new("."),
                    80,
                    24,
                    mode,
                    crate::remote::RemoteOpts::disabled(),
                )
                .unwrap();
        }
        let mut state = AttachedState::single(ids[0]);
        state.split(SplitDirection::Vertical, ids[1]);
        // Focus is on ids[1] post-split.
        app.view = crate::ui::View::Attached(state);

        handle_key_attached(
            &mut app,
            ids[1],
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            ids[1],
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.view {
            crate::ui::View::Attached(state) => {
                assert_eq!(state.focused_id(), Some(ids[0]));
            }
            other => panic!("expected Attached view; got {other:?}"),
        }
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn updates_panel_modal_swallows_other_keys() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        handle_key_modal(
            &mut app,
            KeyEvent::new(crossterm::event::KeyCode::Char('q'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(app.modal.is_some(), "q should not dismiss UpdatesPanel");
        assert!(!app.quit, "q should not propagate to App::quit");
    }

    #[test]
    fn updates_panel_render_shows_grouped_workspaces() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let store = Store::open_in_memory().unwrap();
        let repo1 = store
            .add_repo(std::path::Path::new("/tmp/r1"), "repo-alpha", "")
            .unwrap();
        let ws1 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo1,
                name: "alpha-ws",
                branch: "repo-alpha/alpha-ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha-ws"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws1, WorkspaceState::Ready)
            .unwrap();
        let repo2 = store
            .add_repo(std::path::Path::new("/tmp/r2"), "repo-beta", "")
            .unwrap();
        let ws2 = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo2,
                name: "beta-ws",
                branch: "repo-beta/beta-ws",
                worktree_path: std::path::Path::new("/tmp/wsx-test/beta-ws"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws2, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });

        let backend = TestBackend::new(100, 30);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("Workspace updates"),
            "missing panel title:\n{rendered}"
        );
        assert!(
            rendered.contains("repo-alpha"),
            "missing repo header:\n{rendered}"
        );
        assert!(
            rendered.contains("alpha-ws"),
            "missing workspace row:\n{rendered}"
        );
        assert!(
            rendered.contains("repo-beta"),
            "missing repo header:\n{rendered}"
        );
        assert!(
            rendered.contains("beta-ws"),
            "missing workspace row:\n{rendered}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_view_shows_status_row_for_other_workspace_needing_attention() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let attached_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "attached-here",
                branch: "repo/attached-here",
                worktree_path: std::path::Path::new("/tmp/wsx-test/attached"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(attached_id, WorkspaceState::Ready)
            .unwrap();
        let other_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "the-other",
                branch: "repo/the-other",
                worktree_path: std::path::Path::new("/tmp/wsx-test/other"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(other_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        app.sessions
            .spawn(
                attached_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::remote::RemoteOpts::disabled(),
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(attached_id));
        // The new status row exclusively surfaces workspaces with
        // `needs_attention` set — recent activity alone no longer qualifies.
        app.workspace_needs_attention.insert(other_id);

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            rendered.contains("the-other"),
            "expected status row mention of the other workspace:\n{rendered}"
        );
        assert!(
            rendered.contains('⚠'),
            "expected attention glyph on status row:\n{rendered}"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attached_view_no_status_row_when_no_other_activity() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let attached_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "only-one",
                branch: "repo/only-one",
                worktree_path: std::path::Path::new("/tmp/wsx-test/only"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(attached_id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        app.sessions
            .spawn(
                attached_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::remote::RemoteOpts::disabled(),
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(attached_id));

        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| draw_for_test(f, &mut app)).unwrap();
        let buf = term.backend().buffer();
        // The bottom row is the footer with "Ctrl-x d detach". The second-
        // to-last row should NOT contain a status indicator glyph.
        let h = buf.area.height;
        let second_to_last: String = (0..buf.area.width)
            .map(|x| buf[(x, h - 2)].symbol())
            .collect();
        assert!(
            !second_to_last.contains('⚠'),
            "unexpected attention glyph in row {}: {second_to_last:?}",
            h - 2
        );
        assert!(
            !second_to_last.contains('●'),
            "unexpected activity glyph in row {}: {second_to_last:?}",
            h - 2
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_u_in_attached_pm_opens_updates_panel() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Manually spawn a PM session so handle_key_attached_pm has one.
        let cwd = PathBuf::from(".");
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let s = app
            .sessions
            .spawn_pm(&cwd, 80, 24, mode, crate::remote::RemoteOpts::disabled())
            .unwrap();
        app.pm = Some(s);
        app.view = crate::ui::View::AttachedPm;

        // Send the leader (Ctrl-x) then 'u'.
        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);

        handle_key_attached_pm(
            &mut app,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);
        assert!(matches!(
            app.modal,
            Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 })
        ));

        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    fn mouse_event(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        }
    }

    fn spawn_pm_for_test(app: &mut App) {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let s = app
            .sessions
            .spawn_pm(&cwd, 80, 24, mode, crate::remote::RemoteOpts::disabled())
            .unwrap();
        app.pm = Some(s);
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    fn spawn_attached_workspace(app: &mut App) -> crate::store::WorkspaceId {
        use crate::store::NewWorkspace;
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let repo_id = app
            .store
            .add_repo(std::path::Path::new("."), "scratch", "test")
            .unwrap();
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "scrollback-test",
                branch: "main",
                worktree_path: std::path::Path::new("."),
                yolo: false,
            })
            .unwrap();
        let mode = crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        app.sessions
            .spawn(
                ws_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::remote::RemoteOpts::disabled(),
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(ws_id));
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
        ws_id
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_up_scrolls_attached_workspace() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        handle_mouse(&app, mouse_event(MouseEventKind::ScrollUp)).await;
        assert_eq!(
            app.sessions
                .get(ws_id)
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            3,
            "one wheel notch = 3 rows"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_down_decreases_offset_saturating() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        app.sessions.get(ws_id).unwrap().scroll_up(5);
        handle_mouse(&app, mouse_event(MouseEventKind::ScrollDown)).await;
        assert_eq!(
            app.sessions
                .get(ws_id)
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            2
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_targets_pm_when_pm_attached() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        spawn_pm_for_test(&mut app);
        app.view = crate::ui::View::AttachedPm;
        handle_mouse(&app, mouse_event(MouseEventKind::ScrollUp)).await;
        assert_eq!(
            app.pm
                .as_ref()
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_targets_pm_in_dashboard_when_pm_focused() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        spawn_pm_for_test(&mut app);
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::ProjectManager;
        // view stays Dashboard.
        handle_mouse(&app, mouse_event(MouseEventKind::ScrollUp)).await;
        assert_eq!(
            app.pm
                .as_ref()
                .unwrap()
                .scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            3
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_noop_when_dashboard_focused_no_target() {
        let store = Store::open_in_memory().unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // No PM, no attached workspace; view is Dashboard.
        // Just verify the call doesn't panic.
        handle_mouse(&app, mouse_event(MouseEventKind::ScrollUp)).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn keystroke_to_pty_resets_scrollback() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        app.sessions.get(ws_id).unwrap().scroll_up(20);
        assert!(app.sessions.get(ws_id).unwrap().is_scrolled());
        handle_key_attached(
            &mut app,
            ws_id,
            KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            !app.sessions.get(ws_id).unwrap().is_scrolled(),
            "any keystroke flowing to PTY must snap to live"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_keystroke_does_not_reset_scrollback() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        app.sessions.get(ws_id).unwrap().scroll_up(20);
        // Ctrl-x is the leader. It's consumed by wsx and never reaches the PTY.
        handle_key_attached(
            &mut app,
            ws_id,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);
        assert!(
            app.sessions.get(ws_id).unwrap().is_scrolled(),
            "leader key consumed by wsx; offset should be preserved"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn arrow_key_resets_scrollback_and_forwards_to_pty() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);
        app.sessions.get(ws_id).unwrap().scroll_up(20);
        // Up arrow flows to the PTY (Claude Code prompt history) — must
        // also snap scrollback back to live.
        handle_key_attached(
            &mut app,
            ws_id,
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.sessions.get(ws_id).unwrap().is_scrolled());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_digit_sends_pinned_command_to_pty() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        // Populate the cache directly (Task 7's resolution path is tested
        // separately via the resolve() unit tests).
        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];

        // Ctrl-x leader.
        handle_key_attached(
            &mut app,
            ws_id,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        assert!(app.leader_pending);

        // '1' — fires chip 1, clears leader.
        handle_key_attached(
            &mut app,
            ws_id,
            KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);

        // cat echoes input back. Verify the screen eventually contains
        // the command text.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = app.sessions.get(ws_id).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "expected '/pull-request' on screen; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn leader_digit_out_of_range_is_noop() {
        use crossterm::event::{KeyCode, KeyEvent};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws_id = spawn_attached_workspace(&mut app);

        // Only one chip in the cache.
        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];

        // Ctrl-x leader.
        handle_key_attached(
            &mut app,
            ws_id,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();

        // '5' — index 4, out of range for a 1-element cache.
        handle_key_attached(
            &mut app,
            ws_id,
            KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(!app.leader_pending);

        // No bytes should have been written; cat hasn't echoed anything.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let session = app.sessions.get(ws_id).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            !screen_text.contains("/pull-request"),
            "out-of-range digit must not fire any chip; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_in_chip_rect_fires_pinned_command() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        // Place a 7-wide chip at (5, 30): "[1] PR " = 7 cols.
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 6,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&app, click).await;

        // wait for PTY cat echo
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = active_session(&app).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("/pull-request"),
            "expected chip click to send /pull-request; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn click_outside_chip_rect_does_nothing() {
        use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        app.pinned_commands_cache = vec![crate::pinned::PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
        }];
        app.chip_rects = vec![ratatui::layout::Rect {
            x: 5,
            y: 30,
            width: 7,
            height: 1,
        }];

        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 50, // outside chip
            row: 10,    // outside chip
            modifiers: KeyModifiers::NONE,
        };
        handle_mouse(&app, click).await;

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let session = active_session(&app).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            !screen_text.contains("/pull-request"),
            "click outside any chip must not fire; got: {screen_text:?}"
        );
    }

    #[test]
    fn wrap_paste_bytes_wraps_with_bracketed_markers() {
        let out = wrap_paste_bytes("hello world");
        assert_eq!(out, b"\x1b[200~hello world\x1b[201~");
    }

    #[test]
    fn wrap_paste_bytes_handles_empty_content() {
        // Edge case: a paste of empty string still emits the markers so the
        // far side sees a zero-length paste boundary rather than nothing.
        let out = wrap_paste_bytes("");
        assert_eq!(out, b"\x1b[200~\x1b[201~");
    }

    #[test]
    fn wrap_paste_bytes_preserves_multiline_and_special_chars() {
        let out = wrap_paste_bytes("line1\nline2\t  trailing");
        assert_eq!(out, b"\x1b[200~line1\nline2\t  trailing\x1b[201~");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paste_in_attached_view_sends_bracketed_payload_to_pty() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let _ws_id = spawn_attached_workspace(&mut app);

        handle_event(&mut app, CtEvent::Paste("hello paste".into()))
            .await
            .unwrap();

        // cat echoes input back. The bracketed-paste markers are unknown
        // CSI sequences to vt100 and get swallowed; the inner content
        // appears on the screen verbatim.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let session = active_session(&app).unwrap();
        let parser = session.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("hello paste"),
            "paste content must reach the PTY; got: {screen_text:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paste_in_dashboard_with_pm_focused_sends_bracketed_to_pm() {
        let store = Store::open_in_memory().unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        spawn_pm_for_test(&mut app);
        // Dashboard view + PM visible + PM focused — same condition that
        // routes keystrokes to the PM session.
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::ProjectManager;

        handle_event(&mut app, CtEvent::Paste("hello pm".into()))
            .await
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let pm = app.pm.as_ref().unwrap();
        let parser = pm.parser.lock().unwrap();
        let screen_text = parser.screen().contents();
        assert!(
            screen_text.contains("hello pm"),
            "PM-focused paste must reach the PM PTY; got: {screen_text:?}"
        );
    }

    #[test]
    fn paste_char_to_key_translates_newline_to_enter() {
        let k = paste_char_to_key('\n');
        assert!(matches!(k.code, KeyCode::Enter));
    }

    #[test]
    fn paste_char_to_key_translates_cr_to_enter() {
        let k = paste_char_to_key('\r');
        assert!(matches!(k.code, KeyCode::Enter));
    }

    #[test]
    fn paste_char_to_key_translates_tab() {
        let k = paste_char_to_key('\t');
        assert!(matches!(k.code, KeyCode::Tab));
    }

    #[test]
    fn paste_char_to_key_passes_through_printable() {
        let k = paste_char_to_key('a');
        assert!(matches!(k.code, KeyCode::Char('a')));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_resolves_related_repos_to_additional_dirs() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let backend_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let _frontend_id = store
            .add_repo(std::path::Path::new("/work/frontend"), "frontend", "")
            .unwrap();
        store
            .set_repo_related_repos(backend_id, Some("frontend"))
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id: backend_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let info = build_spawn_info(&app, ws_id);
        assert!(info.is_some());
        let (_id, _path, mode, _repo_path) = info.unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh {
                additional_dirs,
                custom_instructions,
                ..
            } => {
                assert_eq!(
                    additional_dirs,
                    vec![std::path::PathBuf::from("/work/frontend")],
                    "additional_dirs should resolve to frontend's source path"
                );
                let prompt = custom_instructions.expect("read-only fragment must be folded in");
                assert!(
                    prompt.contains("/work/frontend"),
                    "system prompt missing related path: {prompt}"
                );
                assert!(
                    prompt.contains("MUST NOT edit"),
                    "system prompt missing read-only directive: {prompt}"
                );
            }
            other => panic!("expected Fresh mode; got {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn build_spawn_info_filters_self_reference() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let backend_id = store
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        store
            .set_repo_related_repos(backend_id, Some("backend"))
            .unwrap();
        let ws_id = store
            .insert_workspace(&NewWorkspace {
                repo_id: backend_id,
                name: "test-ws",
                branch: "backend/test-ws",
                worktree_path: std::path::Path::new("/wt/test-ws"),
                yolo: false,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let (_id, _path, mode, _repo_path) = build_spawn_info(&app, ws_id).unwrap();
        match mode {
            crate::pty::session::SpawnMode::Fresh {
                additional_dirs,
                custom_instructions,
                ..
            } => {
                assert!(
                    additional_dirs.is_empty(),
                    "self-reference must be filtered"
                );
                assert!(
                    custom_instructions.is_none(),
                    "no related dirs => no fragment"
                );
            }
            other => panic!("expected Fresh mode; got {other:?}"),
        }
    }
}

#[cfg(test)]
mod bell_tests {
    use super::*;

    #[test]
    fn bell_pattern_off_for_non_alertable() {
        let store = crate::store::Store::open_in_memory().expect("in-memory store");
        assert!(matches!(
            bell_pattern_for(ActivityState::Active, &store),
            BellPattern::Off
        ));
    }

    #[test]
    fn bell_pattern_defaults_match_spec() {
        let store = crate::store::Store::open_in_memory().expect("in-memory store");
        assert!(matches!(
            bell_pattern_for(ActivityState::AwaitingAnswer, &store),
            BellPattern::Double
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Complete, &store),
            BellPattern::Single
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Awaiting, &store),
            BellPattern::Single
        ));
        assert!(matches!(
            bell_pattern_for(ActivityState::Stalled, &store),
            BellPattern::Triple
        ));
    }

    #[test]
    fn bell_pattern_override_off_suppresses_default() {
        let store = crate::store::Store::open_in_memory().expect("in-memory store");
        store
            .set_setting("notification_bell_question", "off")
            .unwrap();
        assert!(matches!(
            bell_pattern_for(ActivityState::AwaitingAnswer, &store),
            BellPattern::Off
        ));
    }

    #[test]
    fn bell_pattern_override_single_replaces_default_double() {
        let store = crate::store::Store::open_in_memory().expect("in-memory store");
        store
            .set_setting("notification_bell_question", "single")
            .unwrap();
        assert!(matches!(
            bell_pattern_for(ActivityState::AwaitingAnswer, &store),
            BellPattern::Single
        ));
    }
}

#[cfg(test)]
mod derive_stopped_kind_tests {
    use super::*;
    use crate::events::{StopReason, WorkspaceEvents};

    #[test]
    fn returns_none_when_idle() {
        let evt = WorkspaceEvents::default();
        assert_eq!(derive_stopped_kind(&evt), None);
    }

    #[test]
    fn awaiting_answer_when_question_tool_pending_mid_turn() {
        // AskUserQuestion is in flight: stop_reason is ToolUse (so
        // is_awaiting_user() returns false), but the question tool is in
        // pending_tool_uses. Should still classify as AwaitingAnswer.
        let mut evt = WorkspaceEvents::default();
        evt.last_stop_reason = Some(StopReason::ToolUse);
        evt.pending_tool_uses
            .insert("t1".into(), ("AskUserQuestion".into(), 0));
        assert_eq!(
            derive_stopped_kind(&evt),
            Some(StoppedKind::AwaitingAnswer)
        );
    }

    #[test]
    fn awaiting_answer_when_exit_plan_mode_pending_mid_turn() {
        let mut evt = WorkspaceEvents::default();
        evt.last_stop_reason = Some(StopReason::ToolUse);
        evt.pending_tool_uses
            .insert("t1".into(), ("ExitPlanMode".into(), 0));
        assert_eq!(
            derive_stopped_kind(&evt),
            Some(StoppedKind::AwaitingAnswer)
        );
    }

    #[test]
    fn complete_when_end_turn_with_no_question_signal() {
        let mut evt = WorkspaceEvents::default();
        evt.last_stop_reason = Some(StopReason::EndTurn);
        evt.user_replied_since_stop = false;
        evt.last_assistant_text = Some("Done.".into());
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::Complete));
    }

    #[test]
    fn awaiting_answer_when_end_turn_with_trailing_question() {
        let mut evt = WorkspaceEvents::default();
        evt.last_stop_reason = Some(StopReason::EndTurn);
        evt.user_replied_since_stop = false;
        evt.last_assistant_text = Some("Want me to also handle X?".into());
        assert_eq!(
            derive_stopped_kind(&evt),
            Some(StoppedKind::AwaitingAnswer)
        );
    }

    #[test]
    fn none_when_user_has_already_replied() {
        let mut evt = WorkspaceEvents::default();
        evt.last_stop_reason = Some(StopReason::EndTurn);
        evt.user_replied_since_stop = true;
        assert_eq!(derive_stopped_kind(&evt), None);
    }
}
