#![allow(clippy::collapsible_if)]

use crate::error::Result;
use crate::pty::session::SessionManager;
use crate::store::{Repo, Store, Workspace, WorkspaceId};
use crate::ui::View;
use crate::ui::dashboard::DashboardState;
use crate::ui::modal::Modal;
#[cfg(test)]
use crate::ui::split::AttachedState;
use crate::ui::split::{Arrow, CloseOutcome, SplitDirection};
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
    RepoName,
    BranchPrefix,
    BaseBranch,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
    PinnedCommands,
    RelatedRepos,
}

impl RepoSettingField {
    pub const ALL: [Self; 8] = [
        Self::RepoName,
        Self::BranchPrefix,
        Self::BaseBranch,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
        Self::RelatedRepos,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RepoName => "name",
            Self::BranchPrefix => "branch_prefix",
            Self::BaseBranch => "base_branch",
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
    /// Monotonic counter handed out to in-flight workspace creation tasks.
    pub next_create_gen: u64,
    /// Generation id of the currently in-flight workspace creation, if any.
    /// Used by the reconcile step to detect stale completions (user cancelled,
    /// new create started, etc.).
    pub pending_create_gen: Option<u64>,
    pub dashboard: DashboardState,
    pub repos: Vec<Repo>,
    pub workspaces: Vec<(crate::store::RepoId, Workspace)>,
    pub selectable: Vec<SelectionTarget>,
    pub worktree_base: PathBuf,
    pub leader_pending: bool,
    pub z_leader_pending: bool,
    pub quit: bool,
    pub workspace_status:
        std::collections::HashMap<crate::store::WorkspaceId, crate::git::WorkspaceStatus>,
    /// Cached PR lifecycle per workspace. Absent key = never polled; present
    /// key = last successful poll's result.
    pub pr_lifecycle:
        std::collections::HashMap<crate::store::WorkspaceId, crate::forge::BranchLifecycle>,
    /// Last epoch-ms we attempted a PR fetch per workspace (throttle key).
    pub pr_last_poll_ms: std::collections::HashMap<crate::store::WorkspaceId, i64>,
    /// Last epoch-ms we attempted a `git diff --shortstat` per workspace
    /// (throttle key). 10s minimum interval keeps the dashboard
    /// `+N −N` cell fresh without re-running diff on every 2s tick.
    pub diff_last_poll_ms: std::collections::HashMap<crate::store::WorkspaceId, i64>,
    pub workspace_events:
        std::collections::HashMap<crate::store::WorkspaceId, crate::events::WorkspaceEvents>,
    /// Per-workspace tracking for attention-alert state.
    pub workspace_activity: std::collections::HashMap<crate::store::WorkspaceId, ActivityState>,
    /// Workspaces whose JSONL events have been read at least once by the
    /// tail loop. Until a workspace is in this set the classifier's output
    /// is provisional (it can only see session-liveness, not stop_reason),
    /// so we hold off on recording activity / firing bells for it. Without
    /// this gate the classifier flickers from Active → AwaitingAnswer the
    /// instant the tail loop catches up, which the bell loop would treat
    /// as a legitimate transition and ring on cold start.
    pub workspace_events_scanned: std::collections::HashSet<crate::store::WorkspaceId>,
    /// Workspaces whose alert hasn't been acknowledged (cleared on attach).
    pub workspace_needs_attention: std::collections::HashSet<crate::store::WorkspaceId>,
    /// Anchors whose saved layout has more than one pane. Used by the
    /// dashboard to render the split-layout indicator. Recomputed by
    /// `App::refresh`.
    pub workspaces_with_multi_pane_layouts: std::collections::HashSet<crate::store::WorkspaceId>,
    /// Processes detected per workspace (cwd inside the workspace's
    /// worktree). Refreshed every ~10s by branch_drift_poll.
    pub workspace_processes:
        std::collections::HashMap<crate::store::WorkspaceId, Vec<crate::proc::ProcInfo>>,
    /// Monotonic counter incremented every animation tick. Drives
    /// dashboard spinner phase + any other tick-driven UI animation.
    pub tick: u32,
    /// Cached `git diff --shortstat` output per workspace (added/deleted).
    /// Populated lazily by the workspace-status poller.
    pub workspace_diff: std::collections::HashMap<crate::store::WorkspaceId, crate::git::DiffStats>,
    /// Rolling 24-hour history of `(hour_epoch_secs, max_live_count)` for
    /// the dashboard footer sparkline. Hydrated from `store.recent_activity_buckets`
    /// at startup; updated each tick. Newest bucket at the back.
    pub activity_history: std::collections::VecDeque<(u64, u32)>,
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
    /// When the process started — used to distinguish cold-start
    /// first-observations (suppress bell) from mid-session ones (ring).
    pub started_at: std::time::Instant,
    /// Last `PRAGMA data_version` value observed from the store. Compared
    /// each tick by `poll_external_changes` to detect writes from sibling
    /// `wsx` CLI processes (e.g. `wsx workspace create`) so the dashboard
    /// picks them up without needing a restart.
    pub last_data_version: i64,
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
            z_leader_pending: false,
            quit: false,
            workspace_status: std::collections::HashMap::new(),
            pr_lifecycle: std::collections::HashMap::new(),
            pr_last_poll_ms: std::collections::HashMap::new(),
            diff_last_poll_ms: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
            workspace_activity: std::collections::HashMap::new(),
            workspace_events_scanned: std::collections::HashSet::new(),
            workspace_needs_attention: std::collections::HashSet::new(),
            workspaces_with_multi_pane_layouts: std::collections::HashSet::new(),
            workspace_processes: std::collections::HashMap::new(),
            tick: 0,
            workspace_diff: std::collections::HashMap::new(),
            activity_history: std::collections::VecDeque::new(),
            last_proc_scan_ms: 0,
            pending_edit: None,
            theme,
            pm: None,
            pm_visible: false,
            focus: crate::ui::PaneFocus::Dashboard,
            pm_auto_summary_sent: false,
            next_create_gen: 0,
            pending_create_gen: None,
            chip_rects: Vec::new(),
            pinned_commands_cache: Vec::new(),
            pending_bells: Vec::new(),
            started_at: std::time::Instant::now(),
            last_data_version: 0,
        };
        // Sweep stale Pending rows from previous runs.
        let _ = app
            .store
            .sweep_stale_pending(std::time::Duration::from_secs(300));
        // Load up to 24 hours of bucketed activity for the sparkline.
        if let Ok(buckets) = app.store.recent_activity_buckets(24) {
            app.activity_history.extend(buckets);
        }
        app.refresh()?;
        app.last_data_version = app.store.data_version().unwrap_or(0);
        Ok(app)
    }

    /// Detect writes committed by other processes (e.g. `wsx workspace
    /// create` from a sibling CLI) and pull them into the dashboard. Uses
    /// SQLite's `data_version` pragma — bumps only on external commits, so
    /// this is a no-op when we're the only writer. Returns true when a
    /// refresh was triggered.
    pub fn poll_external_changes(&mut self) -> bool {
        let Ok(v) = self.store.data_version() else {
            return false;
        };
        if v == self.last_data_version {
            return false;
        }
        // Advance the cached version only after a successful refresh, so a
        // transient error (e.g. brief DB lock) leaves us in a state where
        // the next tick retries instead of silently staying stale.
        if let Err(e) = self.refresh() {
            tracing::warn!(error = %e, "external-change refresh failed; will retry next tick");
            return false;
        }
        self.last_data_version = v;
        true
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
        self.workspaces_with_multi_pane_layouts = self
            .store
            .list_multi_pane_layout_anchors()
            .unwrap_or_default()
            .into_iter()
            .collect();
        Ok(())
    }

    /// Allocate a fresh generation id for a new workspace-creation task.
    pub fn alloc_create_gen(&mut self) -> u64 {
        let g = self.next_create_gen;
        self.next_create_gen = self.next_create_gen.wrapping_add(1);
        self.pending_create_gen = Some(g);
        g
    }

    pub fn selected_target(&self) -> Option<SelectionTarget> {
        self.selectable.get(self.dashboard.selected).copied()
    }

    /// If the workspace has any pending tool_use that is a real permission
    /// prompt (NOT AskUserQuestion / ExitPlanMode, which are question tools
    /// surfaced separately as AwaitingAnswer, and NOT Agent subagent
    /// dispatches, which run for minutes by design), return the oldest
    /// pending tool's (name, first-seen epoch ms). Returns None otherwise.
    ///
    /// 3 seconds is well past the latency of any auto-approved tool, so a
    /// pending entry that crosses that threshold is usually waiting on a
    /// permission prompt — but the classifier additionally suppresses this
    /// signal when the PTY is still actively streaming (see
    /// `Status::classify`) to avoid false positives from long-running
    /// shell commands.
    pub fn awaiting_permission(&self, ws_id: crate::store::WorkspaceId) -> Option<(String, i64)> {
        let evt = self.workspace_events.get(&ws_id)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        evt.pending_permission_tool(now, 3_000)
    }

    /// Classify a workspace into the V5 dashboard `Status` vocabulary.
    /// Combines session liveness, JSONL stopped/stalled signals, and
    /// pending tool_use into one canonical state used by the renderer.
    pub fn classify_status(
        &self,
        ws: &crate::store::Workspace,
    ) -> crate::ui::dashboard::status::Status {
        let session = self.sessions.get(ws.id);
        let running = session.as_ref().is_some_and(|s| {
            matches!(
                *s.status.read().unwrap(),
                crate::pty::session::SessionStatus::Running { .. }
            )
        });
        // Returns `None` (not `Some(0)`) when the session is attached but
        // no PTY output has been observed yet, so `Status::classify`'s
        // PTY-active guard treats it as "unknown" rather than "fresh
        // output" — otherwise a permission prompt that fires before the
        // first PTY byte would be misclassified as Thinking.
        let secs = session.as_ref().and_then(|s| {
            let last = s.activity_ms.load(std::sync::atomic::Ordering::Relaxed);
            if last == 0 {
                return None;
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            Some(now.saturating_sub(last) / 1000)
        });
        // `has_prior_session` does filesystem I/O (canonicalize +
        // read_dir); skip it when we already have a live session, since
        // the classifier only looks at it in the no-session branch.
        let has_prior = if running {
            false
        } else {
            crate::pty::session::has_prior_session_for(&ws.worktree_path, ws.agent)
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let stopped_kind = self
            .workspace_events
            .get(&ws.id)
            .and_then(derive_stopped_kind);
        let stalled = self
            .workspace_events
            .get(&ws.id)
            .is_some_and(|e| e.is_stalled(now_ms, 60_000));
        let awaiting = self.awaiting_permission(ws.id).is_some();
        crate::ui::dashboard::status::Status::classify(
            awaiting,
            stopped_kind,
            stalled,
            secs,
            running,
            has_prior,
        )
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
    // A user-initiated interrupt mid-tool-call ends the turn from the
    // agent's perspective: it was told to stop. Claude Code logs this as
    // a synthetic user text block but never emits a follow-up end_turn,
    // so without this branch the session drifts into Stalled after 60s.
    if e.last_user_interrupted {
        return Some(StoppedKind::Complete);
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
            RepoSettingField::RepoName => (repo.name.clone(), "txt"),
            RepoSettingField::BranchPrefix => (repo.branch_prefix.clone(), "txt"),
            RepoSettingField::BaseBranch => (repo.base_branch.clone().unwrap_or_default(), "txt"),
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
            _ = tick.tick() => {
                let mut g = app.lock().await;
                g.tick = g.tick.wrapping_add(1);
                // Pick up workspaces/repos written by sibling `wsx` CLI
                // processes (e.g. `wsx workspace create` invoked by Claude
                // during a related-repos flow). Cheap: PRAGMA data_version
                // is in-process and only triggers refresh on external commits.
                g.poll_external_changes();
                let now_secs = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let now_hour = now_secs - (now_secs % 3600);
                let live = g
                    .workspaces
                    .iter()
                    .filter(|(_rid, ws)| {
                        let s = g.classify_status(ws);
                        matches!(s,
                            crate::ui::dashboard::status::Status::Thinking
                            | crate::ui::dashboard::status::Status::Waiting)
                    })
                    .count() as u32;
                match g.activity_history.back().copied() {
                    Some((h, prev_max)) if h == now_hour => {
                        if live > prev_max {
                            g.activity_history.pop_back();
                            g.activity_history.push_back((h, live));
                        }
                    }
                    Some(_) | None => {
                        if let Some((h, m)) = g.activity_history.back().copied() {
                            let _ = g.store.set_activity_bucket(h, m);
                        }
                        g.activity_history.push_back((now_hour, live));
                        while g.activity_history.len() > 24 {
                            g.activity_history.pop_front();
                        }
                        let _ = g.store.prune_activity_buckets_before(now_hour.saturating_sub(24 * 3600));
                    }
                }
            }
            maybe_evt = events.next() => {
                let Some(Ok(evt)) = maybe_evt else { break; };
                let mut g = app.lock().await;
                handle_event(&mut g, &app, evt).await?;
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
            let selection_is_workspace = matches!(
                app.selected_target(),
                Some(SelectionTarget::Workspace(_))
            );
            let detail_visible = selection_is_workspace
                && area.height >= crate::ui::dashboard::detail::MIN_HEIGHT + 10;
            // Carve a 1-row footer off the bottom of the full area so the
            // spec order (list / detail / pm / footer) is respected. The
            // detail and PM regions are placed ABOVE the footer row.
            let inner_area = if area.height > 1 {
                ratatui::layout::Rect { height: area.height - 1, ..area }
            } else {
                area
            };
            let footer_area = ratatui::layout::Rect {
                y: area.y + area.height.saturating_sub(1),
                height: 1,
                ..area
            };
            let (dashboard_area, detail_area, pm_area) =
                dashboard_regions(inner_area, app.pm_visible, detail_visible);
            let notifications_on = notifications_enabled(&app.store);
            let nerd_fonts = nerd_fonts_enabled(&app.store);

            // Build per-workspace inputs in V5 shape.
            let mut workspaces: Vec<dashboard::WorkspaceItem<'_>> = Vec::new();
            for repo in &app.repos {
                for (rid, ws) in &app.workspaces {
                    if *rid != repo.id {
                        continue;
                    }
                    let status = app.classify_status(ws);
                    let session = app.sessions.get(ws.id);
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
                    let latest = app
                        .workspace_events
                        .get(&ws.id)
                        .and_then(|e| e.latest.clone());
                    let setup_failed = ws.setup_status == crate::store::SetupStatus::Failed;
                    let row = crate::ui::dashboard::row::RowInputs {
                        status,
                        name: ws.name.clone(),
                        branch: ws.branch.clone(),
                        procs: app
                            .workspace_processes
                            .get(&ws.id)
                            .map(|v| v.len() as u32)
                            .unwrap_or(0),
                        diff: app.workspace_diff.get(&ws.id).copied(),
                        last_message: latest.map(|ev| ev.display),
                        ago_secs: secs,
                        selected: matches!(app.selected_target(),
                            Some(SelectionTarget::Workspace(id)) if id == ws.id),
                        yolo: ws.yolo,
                        setup_failed,
                        lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                        nerd_fonts,
                        workspace_id: ws.id,
                        has_multi_pane_layout: app
                            .workspaces_with_multi_pane_layouts
                            .contains(&ws.id),
                    };
                    workspaces.push(dashboard::WorkspaceItem {
                        repo,
                        workspace_id: ws.id,
                        status,
                        row,
                    });
                }
            }

            // Commit the new activity states. Fires the bell on:
            //   - transition from any non-alertable state into
            //     AwaitingAnswer / Complete / Awaiting / Stalled,
            //   - transition between two different alertable states
            //     (e.g. Complete -> Awaiting when a permission prompt
            //     arrives while the user hasn't yet replied to the prior
            //     end_turn).
            // Activity is not recorded — and the bell is not considered —
            // until the tail loop has scanned the workspace's JSONL at
            // least once (see `workspace_events_scanned`). Without that
            // gate the classifier flickers from a provisional Active to
            // a real AwaitingAnswer/Complete the instant events arrive,
            // which would ring on cold start for every already-waiting
            // workspace. Once scanned, the first observation still skips
            // the bell (see `alert_decision`) so the visual marker can
            // surface alertable state without making noise. Does NOT
            // re-fire while an alertable state persists across polls.
            //
            // Keeps the legacy `ActivityState` vocabulary (via
            // `classify_activity_with_events`) for the bell pipeline —
            // the V5 `Status` enum is for display only and would lose the
            // `Active`/`Off`/`Awaiting` distinctions `alert_decision`
            // depends on.
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
                let stopped_kind = app
                    .workspace_events
                    .get(&ws.id)
                    .and_then(derive_stopped_kind);
                let stalled = app
                    .workspace_events
                    .get(&ws.id)
                    .is_some_and(|e| e.is_stalled(now_ms, 60_000));
                let activity =
                    classify_activity_with_events(secs, running, awaiting, stopped_kind, stalled);
                if app.workspace_events_scanned.contains(&ws.id) {
                    let prev = app.workspace_activity.get(&ws.id).copied();
                    let is_cold_start = app.started_at.elapsed() < COLD_START_WINDOW;
                    let (mark_attention, fire_bell) =
                        alert_decision(prev, activity, notifications_on, is_cold_start);
                    if mark_attention {
                        app.workspace_needs_attention.insert(ws.id);
                    }
                    if fire_bell {
                        app.pending_bells.push(activity);
                    }
                    app.workspace_activity.insert(ws.id, activity);
                }
            }

            let activity: Vec<u32> = app.activity_history.iter().map(|(_h, m)| *m).collect();
            let column_widths = read_column_widths(&app.store);
            let inputs = dashboard::DashboardInputs {
                repos: app.repos.iter().collect(),
                workspaces,
                activity: &activity,
                column_widths,
            };
            // Rebuild `selectable` in the V5 visible order (noise-sort
            // across repos, priority-sort within repo, hide folded
            // workspaces, apply filter). Nav keys index into this Vec,
            // so it must match what the renderer emits below or the
            // selection will appear to skip rows / jump back.
            let new_selectable = dashboard::visible_targets(&inputs, &app.dashboard);
            if new_selectable != app.selectable {
                // Preserve the user's *target* across reorderings, not
                // their *index* — keep arrow nav anchored to the same
                // workspace even if the visible order shifts (e.g.
                // status change moves it up/down).
                let prev_target = app.selectable.get(app.dashboard.selected).copied();
                app.selectable = new_selectable;
                if let Some(t) = prev_target {
                    if let Some(idx) = app.selectable.iter().position(|s| *s == t) {
                        app.dashboard.selected = idx;
                    } else if !app.selectable.is_empty() {
                        app.dashboard.selected =
                            app.dashboard.selected.min(app.selectable.len() - 1);
                    } else {
                        app.dashboard.selected = 0;
                    }
                } else if !app.selectable.is_empty() {
                    app.dashboard.selected = app.dashboard.selected.min(app.selectable.len() - 1);
                }
            }
            app.dashboard.selection = app.selected_target();
            dashboard::render_without_footer(
                f,
                dashboard_area,
                &inputs,
                &mut app.dashboard,
                app.tick,
                &app.theme,
            );
            if let Some(pm_area) = pm_area {
                if let Some(session) = app.pm.as_ref() {
                    crate::ui::pm_pane::resize_session(session, pm_area);
                }
                crate::ui::pm_pane::render(f, pm_area, app.pm.as_ref(), app.focus, &app.theme);
            }
            if let (Some(detail_area), Some(SelectionTarget::Workspace(ws_id))) =
                (detail_area, app.selected_target())
            {
                if let Some((rid, ws)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
                    if let Some(repo) = app.repos.iter().find(|r| r.id == *rid) {
                        let session = app.sessions.get(ws.id);
                        // Activity timestamp: prefer whichever signal is more
                        // recent. `session.activity_ms` only exists for
                        // workspaces wsx is currently attached to. The JSONL
                        // event's own `timestamp_ms` (parsed from the line's
                        // `timestamp` field) is the actual event time — this
                        // is what we want, NOT `last_log_activity_ms`, which
                        // is the wall-clock time when wsx observed the log
                        // growing (gets stamped to "now" on the first tail
                        // pass after startup, so all workspaces would
                        // otherwise show the same age starting from zero).
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0);
                        let session_last_ms = session
                            .as_ref()
                            .map(|s| {
                                s.activity_ms.load(std::sync::atomic::Ordering::Relaxed) as i64
                            })
                            .unwrap_or(0);
                        let event_last_ms = app
                            .workspace_events
                            .get(&ws.id)
                            .and_then(|e| e.latest.as_ref().map(|ev| ev.timestamp_ms))
                            .unwrap_or(0);
                        let last_ms = session_last_ms.max(event_last_ms);
                        let ago_secs = if last_ms == 0 {
                            None
                        } else {
                            Some(((now_ms - last_ms).max(0) / 1000) as u64)
                        };
                        let status = app.classify_status(ws);
                        let procs: &[crate::proc::ProcInfo] = app
                            .workspace_processes
                            .get(&ws.id)
                            .map(Vec::as_slice)
                            .unwrap_or(&[]);
                        let inputs = crate::ui::dashboard::detail::DetailInputs {
                            repo,
                            workspace: ws,
                            events: app.workspace_events.get(&ws.id),
                            procs,
                            diff: app.workspace_diff.get(&ws.id).copied(),
                            lifecycle: app.pr_lifecycle.get(&ws.id).copied(),
                            pr_title: None,
                            pr_number: None,
                            status,
                            ago_secs,
                            reply_draft: &app.dashboard.reply_draft,
                            reply_focused: matches!(
                                app.focus,
                                crate::ui::PaneFocus::DetailBarReply
                            ),
                            events_scanned: app.workspace_events_scanned.contains(&ws.id),
                        };
                        crate::ui::dashboard::detail::render(f, detail_area, &inputs, &app.theme);
                    }
                }
            }
            // Render footer below detail/PM so the spec order
            // list / detail / pm / footer is respected.
            dashboard::render_footer(f, footer_area, &activity, &app.theme);
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
                .map(|(_, w)| {
                    let repo_name = app
                        .repos
                        .iter()
                        .find(|r| r.id == w.repo_id)
                        .map(|r| r.name.as_str())
                        .unwrap_or("");
                    if repo_name.is_empty() {
                        w.name.clone()
                    } else {
                        format!("{}/{}", repo_name, w.name)
                    }
                })
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
            let crate::ui::split::LayoutResult { panes, dividers } = state.layout(pane_area);
            let multi_pane = panes.len() > 1;

            // Resize each session's PTY to its pane area (minus title row when multi-pane).
            for (ws_id, _path, rect) in &panes {
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
            )> = panes
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
                &dividers,
                chip_area,
                status_area,
                footer_area,
                &focused_label,
                multi_pane,
                line,
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
                    &[],
                    chip_area,
                    status_area,
                    footer_area,
                    "project-manager",
                    false,
                    line,
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
                let statuses: std::collections::HashMap<
                    crate::store::WorkspaceId,
                    crate::ui::dashboard::status::Status,
                > = app
                    .workspaces
                    .iter()
                    .map(|(_, w)| (w.id, app.classify_status(w)))
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
                    &statuses,
                    &app.pr_lifecycle,
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
            other => modal::render(f, area, other, app.tick, &app.theme),
        }
    }
}

#[doc(hidden)]
pub fn draw_for_test(f: &mut ratatui::Frame, app: &mut App) {
    draw(f, app);
}

/// Carve the dashboard area into list / detail / pm regions based on
/// whether PM is visible and whether a workspace is selected.
fn dashboard_regions(
    area: ratatui::layout::Rect,
    pm_visible: bool,
    detail_visible: bool,
) -> (
    ratatui::layout::Rect,
    Option<ratatui::layout::Rect>,
    Option<ratatui::layout::Rect>,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let detail_h = crate::ui::dashboard::detail::preferred_height(area.height);
    match (pm_visible, detail_visible) {
        (false, false) => (area, None, None),
        (false, true) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(detail_h)])
                .split(area);
            (chunks[0], Some(chunks[1]), None)
        }
        (true, false) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(area);
            (chunks[0], None, Some(chunks[1]))
        }
        (true, true) => {
            let pm_h = ((u32::from(area.height) * 33 / 100) as u16).max(6);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(detail_h),
                    Constraint::Length(pm_h),
                ])
                .split(area);
            (chunks[0], Some(chunks[1]), Some(chunks[2]))
        }
    }
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

/// Resolve the dashboard's user-tunable column widths from settings,
/// clamped to safe min/max. Unset or unparseable values fall back to the
/// V5 defaults (24 / 28).
fn read_column_widths(store: &crate::store::Store) -> crate::ui::dashboard::row::ColumnWidths {
    use crate::ui::dashboard::row::{ColumnWidths, DEFAULT_BRANCH_WIDTH, DEFAULT_NAME_WIDTH};
    let name = store
        .get_setting("dashboard_name_width")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_NAME_WIDTH);
    let branch = store
        .get_setting("dashboard_branch_width")
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_BRANCH_WIDTH);
    ColumnWidths::clamped(name, branch)
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

/// Window after wsx starts during which a first-observation of an
/// alertable workspace is treated as cold-start noise (visual marker
/// only, no bell). Sized to comfortably cover the 2s tail-loop tick so
/// every initial scan lands inside the window.
const COLD_START_WINDOW: std::time::Duration = std::time::Duration::from_secs(3);

/// Decide what to do when a workspace's activity classification changes.
/// Returns `(mark_attention, fire_bell)`.
///
/// During the cold-start window the bell is suppressed on the very first
/// observation of a workspace (`prev.is_none()`), so wsx doesn't ring
/// once per workspace at launch when several agents were already waiting
/// before startup. The visual attention marker still fires so the
/// dashboard reflects current state. Outside the window a first
/// observation rings normally — a workspace that just appeared
/// (newly created or freshly imported) and is already alertable is
/// something the user wants to be notified about.
fn alert_decision(
    prev: Option<ActivityState>,
    activity: ActivityState,
    notifications_on: bool,
    is_cold_start: bool,
) -> (bool, bool) {
    if !notifications_on || !activity.is_alertable() || prev == Some(activity) {
        return (false, false);
    }
    let fire_bell = prev.is_some() || !is_cold_start;
    (true, fire_bell)
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

async fn handle_event(app: &mut App, shared: &SharedApp, evt: CtEvent) -> Result<()> {
    match evt {
        CtEvent::Key(k) if k.kind == KeyEventKind::Press => dispatch_key(app, shared, k).await?,
        CtEvent::Mouse(m) => handle_mouse(app, m).await,
        CtEvent::Paste(content) => handle_paste(app, shared, content).await?,
        CtEvent::Resize(_, _) => {}
        _ => {}
    }
    Ok(())
}

async fn dispatch_key(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    if app.modal.is_some() {
        handle_key_modal(app, shared, k).await?;
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

async fn handle_paste(app: &mut App, shared: &SharedApp, content: String) -> Result<()> {
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
        dispatch_key(app, shared, paste_char_to_key(c)).await?;
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

/// Handle a key event while [`PaneFocus::DetailBarReply`] is active.
///
/// Returns `true` if the key was consumed (caller should early-return),
/// or `false` if the key should fall through to the main dashboard handler
/// (e.g. navigation keys that also move the selection).
async fn handle_detail_bar_reply_key(
    app: &mut App,
    k: crossterm::event::KeyEvent,
) -> bool {
    use crossterm::event::{KeyCode, KeyModifiers};
    match (k.code, k.modifiers) {
        (KeyCode::Tab, _) => {
            // Spec: Dashboard → DetailBarReply → ProjectManager (when visible)
            // → Dashboard. If PM is not visible, skip straight back to Dashboard.
            if app.pm_visible {
                app.focus = crate::ui::PaneFocus::ProjectManager;
            } else {
                app.focus = crate::ui::PaneFocus::Dashboard;
            }
            true
        }
        (KeyCode::Esc, _) => {
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            true
        }
        (KeyCode::Enter, _) => {
            let draft = std::mem::take(&mut app.dashboard.reply_draft);
            if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
                if let Some(session) = app.sessions.get(ws_id) {
                    let mut bytes = draft.into_bytes();
                    bytes.push(b'\r');
                    session.scroll_to_live();
                    let _ = session.writer.send(bytes).await;
                }
            }
            app.focus = crate::ui::PaneFocus::Dashboard;
            true
        }
        (KeyCode::Backspace, _) => {
            app.dashboard.reply_draft.pop();
            true
        }
        (KeyCode::Char(c), m) if m == KeyModifiers::NONE || m == KeyModifiers::SHIFT => {
            app.dashboard.reply_draft.push(c);
            true
        }
        (KeyCode::Up, _)
        | (KeyCode::Down, _)
        | (KeyCode::Left, _)
        | (KeyCode::Right, _)
        | (KeyCode::PageUp, _)
        | (KeyCode::PageDown, _)
        | (KeyCode::Home, _)
        | (KeyCode::End, _) => {
            // Yield to dashboard: it will handle the navigation. Discard draft.
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            false
        }
        _ => true, // unknown key — swallow rather than fall through
    }
}

async fn handle_key_dashboard(app: &mut App, k: crossterm::event::KeyEvent) -> Result<()> {
    // PM pane focus handling. When PM is focused, all keystrokes forward
    // to its PTY — including 'p' and 'r' (typing words containing those
    // letters must not toggle the pane or trigger refresh). To use the
    // dashboard's 'p' / 'r' shortcuts, the user presses Tab/Esc first to
    // return focus to the dashboard.
    if app.pm_visible && matches!(app.focus, crate::ui::PaneFocus::ProjectManager) {
        // Defensive: PM focus means the dashboard's z-leader cannot be
        // meaningfully consumed here (keys forward to the PM PTY). Clear
        // it so it doesn't leak across focus transitions.
        app.z_leader_pending = false;
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
    // DetailBarReply focus: keystrokes go to the reply input.
    if matches!(app.focus, crate::ui::PaneFocus::DetailBarReply) {
        // If the selected target is no longer a workspace (e.g.
        // refresh moved selection), auto-return focus and discard.
        if !matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
            app.focus = crate::ui::PaneFocus::Dashboard;
            app.dashboard.reply_draft.clear();
            return Ok(());
        }
        let consumed = handle_detail_bar_reply_key(app, k).await;
        if consumed {
            return Ok(());
        }
        // Not consumed → fall through so the dashboard handler picks up
        // the key (e.g. arrow nav). `handle_detail_bar_reply_key` has
        // already cleared the draft and reset focus when bailing out.
    }
    // Tab when focus is on Dashboard: workspace selection → DetailBarReply;
    // repo selection with PM visible → ProjectManager.
    if matches!(app.focus, crate::ui::PaneFocus::Dashboard) && k.code == KeyCode::Tab {
        // Treat Tab as a "never mind" for any armed z-leader so it
        // doesn't unexpectedly eat the next dashboard key after the
        // user Tabs back from PM.
        app.z_leader_pending = false;
        if matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
            app.focus = crate::ui::PaneFocus::DetailBarReply;
        } else if app.pm_visible {
            app.focus = crate::ui::PaneFocus::ProjectManager;
        }
        return Ok(());
    }
    // Filter input mode: while a filter buffer is active, intercept
    // printable chars, Backspace, and Esc so they edit the buffer
    // rather than triggering single-key shortcuts like 'n' / 'q' / '/'.
    // Navigation keys (arrows, Enter, etc.) still flow through.
    if app.dashboard.filter.is_some() {
        match k.code {
            KeyCode::Esc => {
                app.dashboard.filter = None;
                return Ok(());
            }
            KeyCode::Backspace => {
                if let Some(buf) = app.dashboard.filter.as_mut() {
                    buf.pop();
                }
                return Ok(());
            }
            KeyCode::Char(c)
                if !c.is_control()
                    && !k.modifiers.contains(KeyModifiers::CONTROL)
                    && !k.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(buf) = app.dashboard.filter.as_mut() {
                    buf.push(c);
                }
                return Ok(());
            }
            _ => {}
        }
    }
    // Z-leader chord. When armed by the prior `z` keypress, the next
    // key dispatches and the leader clears unconditionally. Unknown
    // follow-ups are eaten (no fall-through to the main key handler)
    // so accidental `zj` etc. don't move the selection silently.
    if app.z_leader_pending {
        app.z_leader_pending = false;
        match (k.code, k.modifiers) {
            (KeyCode::Char('z'), _) => toggle_focused_fold(app),
            // Vim `zr` / `zR` (reduce fold / open all folds).
            (KeyCode::Char('r'), _) | (KeyCode::Char('R'), _) | (KeyCode::Char('a'), _) => {
                expand_all_repos(app)
            }
            // Match bare `Char('M')` (no SHIFT guard) to match the
            // codebase convention for capital-letter binds like `G` —
            // some terminals + CapsLock report uppercase without SHIFT.
            // Also accept lowercase `m` (Vim `zm`) for muscle-memory compat.
            (KeyCode::Char('M'), _) | (KeyCode::Char('m'), _) => fold_all_repos(app),
            _ => {} // Esc, unknown key, anything else: just clear.
        }
        return Ok(());
    }
    match (k.code, k.modifiers) {
        (KeyCode::Char('q'), _) => app.quit = true,
        (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            app.dashboard.selected = if app.dashboard.selected == 0 {
                max
            } else {
                app.dashboard.selected - 1
            };
            // Clear any in-flight reply draft so it can't leak to the newly
            // selected workspace (draft is tied to the workspace at the time
            // keystrokes arrived, not to wherever the cursor ends up).
            app.dashboard.reply_draft.clear();
        }
        (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
            let max = app.selectable.len().saturating_sub(1);
            app.dashboard.selected = if app.dashboard.selected >= max {
                0
            } else {
                app.dashboard.selected + 1
            };
            // Clear any in-flight reply draft (same rationale as Up/k above).
            app.dashboard.reply_draft.clear();
        }
        (KeyCode::Char('h'), _) => set_focused_fold(app, true),
        (KeyCode::Char('l'), _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => attach_workspace(app, id)?,
            Some(SelectionTarget::Repo(_)) => set_focused_fold(app, false),
            None => {}
        },
        (KeyCode::Enter, _) | (KeyCode::Char('i'), _) => match app.selected_target() {
            Some(SelectionTarget::Workspace(id)) => attach_workspace(app, id)?,
            Some(SelectionTarget::Repo(id)) => {
                app.modal = Some(Modal::NewWorkspace {
                    repo_id: id,
                    name_buffer: String::new(),
                    yolo: false,
                    agent: crate::pty::session::AgentKind::from_store(&app.store),
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
                    agent: crate::pty::session::AgentKind::from_store(&app.store),
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
        (KeyCode::Char('g'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                let info = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = info {
                    let cmd = app.store.get_setting("lazygit_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_lazygit(&path, cmd.as_deref()) {
                        app.modal = Some(Modal::Error {
                            message: e.to_string(),
                        });
                    }
                }
            }
            // 'g' on a Repo header is intentionally a no-op.
        }
        (KeyCode::Char('K'), _) => {
            if let Some(SelectionTarget::Workspace(id)) = app.selected_target() {
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                });
            }
            // 'K' on a Repo header is intentionally a no-op.
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
        (KeyCode::Char('G'), _) => {
            use crate::ui::dashboard::layout::GroupMode;
            app.dashboard.group_mode = match app.dashboard.group_mode {
                GroupMode::Repo => GroupMode::Attention,
                GroupMode::Attention => GroupMode::Repo,
            };
        }
        (KeyCode::Char('z'), _) => {
            app.z_leader_pending = true;
        }
        (KeyCode::Char('/'), _) => {
            app.dashboard.filter = Some(String::new());
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

/// Aggregate the current `StatusCounts` for one repo by classifying each
/// of its live workspaces. Used by the `z` (fold) keybinding so we can
/// look up the same default-fold state the renderer would compute.
fn current_repo_counts(
    app: &App,
    rid: crate::store::RepoId,
) -> crate::ui::dashboard::sort::StatusCounts {
    let iter = app
        .workspaces
        .iter()
        .filter(|(r, _)| *r == rid)
        .map(|(_, w)| app.classify_status(w));
    crate::ui::dashboard::sort::StatusCounts::from_iter(iter)
}

/// Toggle the fold state of the currently focused repo on the
/// dashboard. If a workspace is focused, the repo containing it is
/// the target. Extracted from the original single-key `z` arm so the
/// `zz` chord branch can reuse it.
fn toggle_focused_fold(app: &mut App) {
    let target_rid = match app.selected_target() {
        Some(SelectionTarget::Workspace(wid)) => app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == wid)
            .map(|(rid, _)| *rid),
        Some(SelectionTarget::Repo(rid)) => Some(rid),
        None => None,
    };
    if let Some(rid) = target_rid {
        let id = rid.0 as u64;
        let counts = current_repo_counts(app, rid);
        let currently_expanded = match app.dashboard.folded.get(&id).copied() {
            Some(explicit) => !explicit,
            None => !crate::ui::dashboard::sort::default_fold(counts),
        };
        // Store `true` = folded (i.e. !expanded).
        app.dashboard.folded.insert(id, currently_expanded);
    }
}

/// Vim-style `h` (fold) / `l` (unfold) on the focused row. Unlike
/// [`toggle_focused_fold`], this is idempotent: pressing `h` on an
/// already-folded repo leaves it folded.
fn set_focused_fold(app: &mut App, fold: bool) {
    let target_rid = match app.selected_target() {
        Some(SelectionTarget::Workspace(wid)) => app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == wid)
            .map(|(rid, _)| *rid),
        Some(SelectionTarget::Repo(rid)) => Some(rid),
        None => None,
    };
    if let Some(rid) = target_rid {
        app.dashboard.folded.insert(rid.0 as u64, fold);
    }
}

/// `za` action: expand every registered repo by inserting an explicit
/// `false` in `dashboard.folded`. Overrides the renderer's
/// default-fold heuristic so even default-folded repos open.
fn expand_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, false);
    }
}

/// `zM` action: fold every registered repo by inserting an explicit
/// `true` in `dashboard.folded`. Overrides the renderer's heuristic.
fn fold_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, true);
    }
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
        RepoSettingField::RepoName => {
            app.store.set_repo_name(repo_id, trimmed)?;
            Ok(())
        }
        RepoSettingField::BranchPrefix => app.store.set_repo_branch_prefix(repo_id, trimmed),
        RepoSettingField::BaseBranch => app.store.set_repo_base_branch(repo_id, opt),
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
    crate::pty::session::AgentKind,
)> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let custom = crate::repo::resolve_custom_instructions(repo, &app.store)
        .ok()
        .flatten();
    let yolo = ws.yolo;
    let agent = ws.agent;
    // Resolve related repos (per-repo names → source paths), filter out
    // the spawning repo itself, build the read-only system-prompt
    // fragment, and fold it into custom_instructions before the agent sees it.
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
    let mode = if crate::pty::session::has_prior_session_for(&ws.worktree_path, agent) {
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
    Some((
        ws_id,
        ws.worktree_path.clone(),
        mode,
        repo.path.clone(),
        agent,
    ))
}

fn save_layout_for(app: &mut App, state: crate::ui::AttachedState) {
    let Some(anchor) = state.leaves().first().copied() else {
        return;
    };
    if let Err(e) = app
        .store
        .set_workspace_layout(anchor, &state.tree, &state.focus)
    {
        tracing::warn!(error = %e, "failed to save workspace layout");
    }
    // Recompute the dashboard indicator cache so the badge updates
    // immediately when the user returns to the dashboard.
    let _ = app.refresh();
}

/// Restore a saved layout for `anchor`, pruning any workspaces that no longer
/// exist. Spawns missing sessions for surviving side panes. Falls back to a
/// single-pane view if no layout is saved or all panes were pruned.
fn restore_attached_state(
    app: &mut App,
    anchor: crate::store::WorkspaceId,
) -> crate::ui::AttachedState {
    let Some((mut tree, mut focus)) = app.store.get_workspace_layout(anchor).ok().flatten() else {
        return crate::ui::AttachedState::single(anchor);
    };
    let valid: std::collections::HashSet<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    use crate::ui::split::PruneOutcome;
    let outcome = tree.prune(&|id| valid.contains(&id));
    match outcome {
        PruneOutcome::Empty => {
            let _ = app.store.delete_workspace_layout(anchor);
            let _ = app.refresh();
            crate::ui::AttachedState::single(anchor)
        }
        PruneOutcome::Kept => {
            if tree.leaf_at(&focus).is_none() {
                focus = tree.first_leaf_path();
            }
            // Spawn any missing sessions for the side panes. Anchor was
            // already spawned by the caller. Skip on failure and continue
            // with remaining panes — partial restore is better than no restore.
            for leaf_id in tree.leaves() {
                if leaf_id == anchor || app.sessions.get(leaf_id).is_some() {
                    continue;
                }
                if let Some((sid, sp, mode, repo_path, agent)) = build_spawn_info(app, leaf_id) {
                    maybe_mirror_mcp(app, &repo_path, &sp);
                    let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
                    let _ = app.sessions.spawn(sid, &sp, 80, 24, mode, remote, agent);
                }
            }
            crate::ui::AttachedState { tree, focus }
        }
    }
}

/// Attach to a workspace: spawn a session, restore layout, and switch to
/// attached view. Shared by the `Enter` / `i` / `l` key handlers.
fn attach_workspace(app: &mut App, ws_id: crate::store::WorkspaceId) -> Result<()> {
    app.workspace_needs_attention.remove(&ws_id);
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, ws_id) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
        let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote, agent)?;
        let restored = restore_attached_state(app, id);
        app.view = View::Attached(restored);
    }
    Ok(())
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
            KeyCode::Esc => {
                if let View::Attached(state) = &app.view {
                    save_layout_for(app, state.clone());
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
            KeyCode::Char('g') => {
                let path = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == id)
                    .map(|(_, w)| w.worktree_path.clone());
                if let Some(path) = path {
                    let cmd = app.store.get_setting("lazygit_cmd").ok().flatten();
                    if let Err(e) = crate::external::open_in_lazygit(&path, cmd.as_deref()) {
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

async fn handle_key_modal(
    app: &mut App,
    shared: &SharedApp,
    k: crossterm::event::KeyEvent,
) -> Result<()> {
    let modal = app.modal.clone().unwrap();
    match modal {
        Modal::NewWorkspace {
            repo_id,
            mut name_buffer,
            yolo,
            mut agent,
        } => match k.code {
            KeyCode::Esc => {
                app.modal = None;
            }
            KeyCode::Tab => {
                agent = match agent {
                    crate::pty::session::AgentKind::Claude => crate::pty::session::AgentKind::Pi,
                    crate::pty::session::AgentKind::Pi => crate::pty::session::AgentKind::Claude,
                };
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    agent,
                });
            }
            KeyCode::Enter => {
                let name = if name_buffer.trim().is_empty() {
                    None
                } else {
                    Some(name_buffer.clone())
                };
                let repo = app.repos.iter().find(|r| r.id == repo_id).unwrap().clone();
                let base = app.worktree_base.clone();
                let cancel = tokio_util::sync::CancellationToken::new();
                let create_gen = app.alloc_create_gen();
                app.modal = Some(Modal::SetupRunning {
                    cancel: cancel.clone(),
                });
                let shared_clone = shared.clone();
                tokio::spawn(async move {
                    let result = crate::workspace::create_with_app(
                        shared_clone.clone(),
                        repo,
                        name,
                        base,
                        yolo,
                        agent,
                        cancel,
                    )
                    .await;
                    reconcile_create_result(shared_clone, create_gen, result).await;
                });
            }
            KeyCode::Backspace => {
                name_buffer.pop();
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    agent,
                });
            }
            KeyCode::Char(c) => {
                name_buffer.push(c);
                app.modal = Some(Modal::NewWorkspace {
                    repo_id,
                    name_buffer,
                    yolo,
                    agent,
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
        Modal::SetupRunning { cancel } => {
            // Esc cancels in-flight create; every other key (including Enter)
            // is intentionally ignored during creation.
            if k.code == KeyCode::Esc {
                cancel.cancel();
                app.modal = None;
                app.pending_create_gen = None;
            }
        }
        Modal::Error { .. } => {
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
                KeyCode::Up | KeyCode::Char('k') => {
                    let new_sel = selected_now.saturating_sub(1);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let max = order.len().saturating_sub(1);
                    let new_sel = (selected_now + 1).min(max);
                    app.modal = Some(Modal::UpdatesPanel { selected: new_sel });
                }
                KeyCode::Enter => {
                    if let Some(ws_id) = order.get(selected_now).copied() {
                        // Mirror the dashboard-attach flow: clear the
                        // alert, spawn (or resume) the PTY, switch view.
                        app.workspace_needs_attention.remove(&ws_id);
                        if let Some((id, path, mode, repo_path, agent)) =
                            build_spawn_info(app, ws_id)
                        {
                            maybe_mirror_mcp(app, &repo_path, &path);
                            let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
                            let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote, agent)?;
                            let restored = restore_attached_state(app, id);
                            app.view = View::Attached(restored);
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
                        if let Some((id, path, mode, repo_path, agent)) =
                            build_spawn_info(app, ws_id)
                        {
                            maybe_mirror_mcp(app, &repo_path, &path);
                            let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
                            let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote, agent)?;
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
                                    // No attached pane yet — restore saved layout or attach plainly.
                                    let restored = restore_attached_state(app, id);
                                    app.view = View::Attached(restored);
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
                // ProcessList intentionally does NOT alias j/k to nav like
                // the other list modals: `k` here means SIGTERM and `K` means
                // SIGKILL, so vim-style movement would clash with the kill
                // verbs. Arrow keys are the only navigation.
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
            KeyCode::Up | KeyCode::Char('k') => {
                selected = selected.saturating_sub(1);
                app.modal = Some(Modal::RepoSettings { repo_id, selected });
            }
            KeyCode::Down | KeyCode::Char('j') => {
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
) -> Option<ratatui::text::Line<'static>> {
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
    crate::ui::updates_bar::format_attention_line_styled(&entries, now_ms, max_width, &app.theme)
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

/// Reconcile the outcome of a spawned `workspace::create_with_app` task.
/// Locks the app briefly; if the modal is still `SetupRunning` AND the
/// generation matches ours, applies the outcome (close modal on success,
/// switch to `Modal::Error` on failure). Otherwise — user dismissed or
/// started a new create — leaves the modal alone but still calls
/// `refresh()` so the dashboard reflects any state we wrote to the store.
async fn reconcile_create_result(
    app: SharedApp,
    my_gen: u64,
    result: Result<crate::workspace::CreatedWorkspace>,
) {
    let mut g = app.lock().await;
    let is_mine = g.pending_create_gen == Some(my_gen);
    if is_mine {
        g.pending_create_gen = None;
    }
    let new_ws_id = result.as_ref().ok().map(|c| c.workspace.id);
    match result {
        Ok(_) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::SetupRunning { .. })) {
                g.modal = None;
            }
            let _ = g.refresh();
            // Select the newly created workspace so the dashboard lands on it.
            if let Some(id) = new_ws_id {
                if let Some(idx) = g
                    .selectable
                    .iter()
                    .position(|t| *t == SelectionTarget::Workspace(id))
                {
                    g.dashboard.selected = idx;
                }
            }
        }
        Err(crate::error::Error::Cancelled) => {
            // User cancelled — modal already cleared by Esc handler. Refresh
            // so the dashboard reflects setup_status=Cancelled.
            let _ = g.refresh();
        }
        Err(e) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::SetupRunning { .. })) {
                g.modal = Some(crate::ui::modal::Modal::Error {
                    message: e.to_string(),
                });
            }
            let _ = g.refresh();
        }
    }
}

/// Periodically check each live workspace's current git branch against
/// the DB; if claude (or a user) renamed it, update name + branch in the
/// store. Runs forever; cheap when nothing has drifted.
pub async fn branch_drift_poll(app: SharedApp) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let snapshot: Vec<(
            WorkspaceId,
            std::path::PathBuf,
            String,
            String,
            Option<String>,
            crate::pty::session::AgentKind,
        )> = {
            let g = app.lock().await;
            g.workspaces
                .iter()
                .filter_map(|(_, w)| {
                    let repo = g.repos.iter().find(|r| r.id == w.repo_id)?;
                    let prefix =
                        crate::repo::resolve_branch_prefix(repo, &g.store).unwrap_or_default();
                    Some((
                        w.id,
                        w.worktree_path.clone(),
                        w.branch.clone(),
                        prefix,
                        repo.base_branch.clone(),
                        w.agent,
                    ))
                })
                .collect()
        };

        for (id, path, db_branch, prefix, base_branch, ws_agent) in snapshot {
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
                    // New branch → different ancestry from `base_branch`,
                    // so the cached diff and its throttle stamp are
                    // stale. Drop them to force a fresh poll.
                    g.workspace_diff.remove(&id);
                    g.diff_last_poll_ms.remove(&id);
                }
            }

            // 2) Workspace status — refresh the cache for this workspace.
            if let Ok(status) = crate::git::workspace_status(&path).await {
                let mut g = app.lock().await;
                g.workspace_status.insert(id, status);
            }

            // 2b) Diff stats vs. base branch (for dashboard +N/-M column).
            //     Throttled to once per 10s per workspace: running
            //     `git diff --shortstat <base>...HEAD` on every 2s tick
            //     is wasteful on large repos and the column doesn't need
            //     sub-10s freshness.
            if let Some(base) = base_branch.as_deref() {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let should_poll = {
                    let g = app.lock().await;
                    g.diff_last_poll_ms
                        .get(&id)
                        .map(|t| now_ms.saturating_sub(*t) >= 10_000)
                        .unwrap_or(true)
                };
                if should_poll {
                    {
                        let mut g = app.lock().await;
                        g.diff_last_poll_ms.insert(id, now_ms);
                    }
                    if let Some(diff) = crate::git::workspace_diff_stats(&path, base).await {
                        let mut g = app.lock().await;
                        g.workspace_diff.insert(id, diff);
                    }
                }
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

            // 4) Tail agent session JSONL for events.
            //
            // Lock-ordering: snapshot the previous offset and agent kind
            // under the lock, do the file I/O without the lock held, then
            // re-acquire to commit the new offset + events. This keeps the
            // UI responsive even when sessions grow large.
            //
            // Branches on per-workspace agent kind.
            let current_file = match ws_agent {
                crate::pty::session::AgentKind::Claude => crate::events::locate_session_file(&path),
                crate::pty::session::AgentKind::Pi => crate::pi_events::locate_session_file(&path),
            };
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
                let tail_result = match ws_agent {
                    crate::pty::session::AgentKind::Claude => {
                        crate::events::tail_session(&file, prev_offset)
                    }
                    crate::pty::session::AgentKind::Pi => {
                        crate::pi_events::tail_session(&file, prev_offset)
                    }
                };
                if let Ok(update) = tail_result {
                    let crate::events::TailUpdate {
                        new_offset,
                        events,
                        tool_use_starts,
                        tool_use_resolves,
                        last_stop_reason,
                        human_replied_after_last_stop,
                        reset_from_zero,
                        last_assistant_text,
                        last_user_interrupted,
                        first_user_text,
                        tool_use_counts,
                        edited_file_paths,
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
                    if evt.first_user_text.is_none() {
                        if let Some(t) = first_user_text {
                            evt.first_user_text = Some(t);
                        }
                    }
                    evt.tool_use_counts.read =
                        evt.tool_use_counts.read.saturating_add(tool_use_counts.read);
                    evt.tool_use_counts.edit =
                        evt.tool_use_counts.edit.saturating_add(tool_use_counts.edit);
                    evt.tool_use_counts.write =
                        evt.tool_use_counts.write.saturating_add(tool_use_counts.write);
                    evt.tool_use_counts.bash =
                        evt.tool_use_counts.bash.saturating_add(tool_use_counts.bash);
                    evt.tool_use_counts.other =
                        evt.tool_use_counts.other.saturating_add(tool_use_counts.other);
                    for path in edited_file_paths {
                        if evt.recent_edited_files.front().map(|s| s.as_str()) != Some(&path) {
                            evt.recent_edited_files.push_front(path);
                            while evt.recent_edited_files.len() > 7 {
                                evt.recent_edited_files.pop_back();
                            }
                        }
                    }
                    // Sticky between batches: only overwrite when the batch
                    // had a definitive signal. Some(true) = batch ended on
                    // the interrupt sentinel; Some(false) = batch had a
                    // newer assistant message or real user text overriding
                    // it; None = batch was silent on this axis.
                    if let Some(v) = last_user_interrupted {
                        evt.last_user_interrupted = v;
                    }
                    for e in events {
                        crate::events::push_event(evt, e);
                    }
                    // First successful tail of this workspace's JSONL.
                    // After this point the classifier sees the agent's
                    // real stop_reason, so the bell loop can start
                    // trusting activity transitions for this workspace.
                    g.workspace_events_scanned.insert(id);
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
            classify_activity_with_events(Some(0), true, true, Some(StoppedKind::Complete), false,),
            ActivityState::Awaiting
        );
        assert_eq!(
            classify_activity_with_events(Some(0), true, true, None, false),
            ActivityState::Awaiting
        );
        // stopped beats PTY recency.
        assert_eq!(
            classify_activity_with_events(Some(0), true, false, Some(StoppedKind::Complete), false,),
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
            classify_activity_with_events(Some(0), true, false, Some(StoppedKind::Complete), true,),
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
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        handle_key_modal(
            &mut app,
            &shared,
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
                    agent: crate::pty::session::AgentKind::Claude,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        handle_key_modal(
            &mut app,
            &shared,
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
            &shared,
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
            &shared,
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
    async fn updates_panel_modal_j_k_aliases_down_up() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
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
                    agent: crate::pty::session::AgentKind::Claude,
                })
                .unwrap();
            store
                .set_workspace_state(id, WorkspaceState::Ready)
                .unwrap();
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                app.modal,
                Some(crate::ui::modal::Modal::UpdatesPanel { selected: 1 })
            ),
            "j should advance like Down; got {:?}",
            app.modal
        );
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                app.modal,
                Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 })
            ),
            "k should retreat like Up; got {:?}",
            app.modal
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn repo_settings_modal_j_k_aliases_down_up() {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(crate::ui::modal::Modal::RepoSettings {
            repo_id,
            selected: 0,
        });
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                app.modal,
                Some(crate::ui::modal::Modal::RepoSettings { selected: 1, .. })
            ),
            "j should advance in RepoSettings; got {:?}",
            app.modal
        );
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(
            matches!(
                app.modal,
                Some(crate::ui::modal::Modal::RepoSettings { selected: 0, .. })
            ),
            "k should retreat in RepoSettings; got {:?}",
            app.modal
        );
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
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        app.workspace_needs_attention.insert(ws_id);
        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        handle_key_modal(
            &mut app,
            &shared,
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
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let second_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "second",
                branch: "repo/second",
                worktree_path: std::path::Path::new("/tmp/wsx-split-2"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
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
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
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
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
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
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        handle_key_modal(
            &mut app,
            &shared,
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
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let second_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "second",
                branch: "repo/second",
                worktree_path: std::path::Path::new("/tmp/wsx-close-2"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
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
                    crate::remote_control::RemoteOpts::disabled(),
                    crate::pty::session::AgentKind::Claude,
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
                    agent: crate::pty::session::AgentKind::Claude,
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
                    crate::remote_control::RemoteOpts::disabled(),
                    crate::pty::session::AgentKind::Claude,
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
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));
        handle_key_modal(
            &mut app,
            &shared,
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
                agent: crate::pty::session::AgentKind::Claude,
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
                agent: crate::pty::session::AgentKind::Claude,
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

    #[test]
    fn updates_panel_render_scrolls_to_keep_selected_visible() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let store = Store::open_in_memory().unwrap();
        // 5 repos × 8 workspaces = 40 ws rows + 5 headers + 5 blank
        // separators = 50 visual lines. The panel clamps height to ≤25,
        // so without scrolling the last workspaces are invisible.
        for r in 0..5 {
            let repo_path = format!("/tmp/scroll-test/r{r}");
            let repo_name = format!("repo-{r}");
            let repo_id = store
                .add_repo(std::path::Path::new(&repo_path), &repo_name, "")
                .unwrap();
            for w in 0..8 {
                let ws_name = format!("ws-{r}-{w}");
                let branch = format!("{repo_name}/{ws_name}");
                let worktree = format!("/tmp/scroll-test/{ws_name}");
                let ws_id = store
                    .insert_workspace(&NewWorkspace {
                        repo_id,
                        name: &ws_name,
                        branch: &branch,
                        worktree_path: std::path::Path::new(&worktree),
                        yolo: false,
                        agent: crate::pty::session::AgentKind::Claude,
                    })
                    .unwrap();
                store
                    .set_workspace_state(ws_id, WorkspaceState::Ready)
                    .unwrap();
            }
        }

        let mut app = App::new(store, PathBuf::from("/tmp/scroll-test")).unwrap();

        // Build the same order the renderer uses, so we can select the
        // very last workspace — the one that would be clipped without
        // scroll support.
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
        assert!(
            order.len() >= 40,
            "expected ≥40 workspaces, got {}",
            order.len()
        );
        let last_selected = order.len() - 1;
        let last_ws_id = order[last_selected];
        let last_ws_name = app
            .workspaces
            .iter()
            .find(|(_, w)| w.id == last_ws_id)
            .expect("last workspace present")
            .1
            .name
            .clone();

        app.modal = Some(crate::ui::modal::Modal::UpdatesPanel {
            selected: last_selected,
        });

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
            rendered.contains(&last_ws_name),
            "selected workspace '{last_ws_name}' should be scrolled into \
             view but is not present in rendered modal:\n{rendered}"
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
                agent: crate::pty::session::AgentKind::Claude,
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
                agent: crate::pty::session::AgentKind::Claude,
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
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
            .unwrap();
        app.view = crate::ui::View::Attached(AttachedState::single(attached_id));
        // The new status row exclusively surfaces workspaces with
        // `needs_attention` set — recent activity alone no longer qualifies.
        // In production both flags are set together when `alert_decision`
        // fires; mirror that here so the V5 status glyph (`!` for stalled)
        // is what the styled line renders.
        app.workspace_needs_attention.insert(other_id);
        app.workspace_activity
            .insert(other_id, crate::app::ActivityState::Stalled);

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
            rendered.contains("! repo/the-other"),
            "expected V5 stalled glyph next to workspace name on status row:\n{rendered}"
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
                agent: crate::pty::session::AgentKind::Claude,
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
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
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
            .spawn_pm(
                &cwd,
                80,
                24,
                mode,
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
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
            .spawn_pm(
                &cwd,
                80,
                24,
                mode,
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
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
                agent: crate::pty::session::AgentKind::Claude,
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
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
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
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));

        handle_event(&mut app, &shared, CtEvent::Paste("hello paste".into()))
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
        let shared = Arc::new(Mutex::new(
            App::new(
                Store::open_in_memory().unwrap(),
                PathBuf::from("/tmp/wsx-test"),
            )
            .unwrap(),
        ));

        handle_event(&mut app, &shared, CtEvent::Paste("hello pm".into()))
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
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let info = build_spawn_info(&app, ws_id);
        assert!(info.is_some());
        let (_id, _path, mode, _repo_path, _agent) = info.unwrap();
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
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();

        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        let (_id, _path, mode, _repo_path, _agent) = build_spawn_info(&app, ws_id).unwrap();
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

    /// Test helper: create an App with N repos registered in the store
    /// and loaded into app.repos. Uses a unique tmpdir per call so paths
    /// don't collide.
    fn make_app_with_n_repos(n: usize) -> (App, Vec<crate::store::RepoId>) {
        let store = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for i in 0..n {
            let path =
                std::env::temp_dir().join(format!("wsx-fold-test-{}-{}", std::process::id(), i));
            let id = store.add_repo(&path, &format!("repo-{i}"), "").unwrap();
            ids.push(id);
        }
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-fold-test")).unwrap();
        app.refresh().unwrap();
        (app, ids)
    }

    async fn press(app: &mut App, ch: char, mods: KeyModifiers) {
        handle_key_dashboard(app, KeyEvent::new(KeyCode::Char(ch), mods))
            .await
            .unwrap();
    }

    async fn press_key(app: &mut App, code: KeyCode) {
        handle_key_dashboard(app, KeyEvent::new(code, KeyModifiers::NONE))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn z_alone_arms_leader_without_action() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        assert!(app.z_leader_pending, "z should arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "z alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn zz_toggles_focused_repo_fold() {
        let (mut app, ids) = make_app_with_n_repos(2);
        app.dashboard.selected = 0;
        let rid = ids[0];
        let key = rid.0 as u64;
        let before = app.dashboard.folded.get(&key).copied();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'z', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zz");
        let after = app.dashboard.folded.get(&key).copied();
        assert_ne!(
            before, after,
            "zz should change the fold state for the focused repo"
        );
    }

    #[tokio::test]
    async fn za_expands_all_repos() {
        let (mut app, ids) = make_app_with_n_repos(3);
        // Pre-fold one repo explicitly so we can see the "expand all" override.
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'a', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after za");
        for id in &ids {
            let key = id.0 as u64;
            assert_eq!(
                app.dashboard.folded.get(&key).copied(),
                Some(false),
                "za should set repo {key} to expanded (false)"
            );
        }
    }

    #[tokio::test]
    async fn z_shift_m_folds_all_repos() {
        let (mut app, ids) = make_app_with_n_repos(3);
        // Pre-expand one repo explicitly so we can see the "fold all" override.
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'M', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "leader should clear after zM");
        for id in &ids {
            let key = id.0 as u64;
            assert_eq!(
                app.dashboard.folded.get(&key).copied(),
                Some(true),
                "zM should set repo {key} to folded (true)"
            );
        }
    }

    #[tokio::test]
    async fn z_then_unknown_clears_leader_without_action() {
        let (mut app, _) = make_app_with_n_repos(2);
        let selected_before = app.dashboard.selected;
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'x', KeyModifiers::NONE).await;
        assert!(
            !app.z_leader_pending,
            "leader should clear after unknown key"
        );
        assert_eq!(
            app.dashboard.folded, folded_before,
            "unknown follow-up should leave fold state unchanged"
        );
        assert_eq!(
            app.dashboard.selected, selected_before,
            "unknown follow-up should be eaten, not pass through to selection"
        );
    }

    #[tokio::test]
    async fn z_then_esc_clears_leader() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press_key(&mut app, KeyCode::Esc).await;
        assert!(!app.z_leader_pending, "Esc should clear the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "Esc should not change fold state"
        );
    }

    #[tokio::test]
    async fn j_alias_advances_selection_like_down() {
        let (mut app, _) = make_app_with_n_repos(3);
        app.dashboard.selected = 0;
        press(&mut app, 'j', KeyModifiers::NONE).await;
        assert_eq!(app.dashboard.selected, 1, "j should advance like Down");
    }

    #[tokio::test]
    async fn k_alias_retreats_selection_like_up() {
        let (mut app, _) = make_app_with_n_repos(3);
        app.dashboard.selected = 2;
        press(&mut app, 'k', KeyModifiers::NONE).await;
        assert_eq!(app.dashboard.selected, 1, "k should retreat like Up");
    }

    #[tokio::test]
    async fn k_does_not_open_process_list_anymore() {
        // `k` is now a nav alias for Up. Process list must be opened by `K`.
        let (mut app, _) = make_app_with_n_repos(1);
        app.dashboard.selected = 0;
        press(&mut app, 'k', KeyModifiers::NONE).await;
        assert!(
            app.modal.is_none(),
            "k must not open ProcessList; it's now a nav alias"
        );
    }

    #[tokio::test]
    async fn shift_k_opens_process_list_on_workspace() {
        use crate::store::{NewWorkspace, WorkspaceState};
        let (mut app, ids) = make_app_with_n_repos(1);
        let ws_id = app
            .store
            .insert_workspace(&NewWorkspace {
                repo_id: ids[0],
                name: "alpha",
                branch: "repo-0/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        app.store
            .set_workspace_state(ws_id, WorkspaceState::Ready)
            .unwrap();
        app.refresh().unwrap();
        // Find and select the workspace row.
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(id) if *id == ws_id))
            .expect("workspace should appear in selectable list");
        app.dashboard.selected = idx;
        press(&mut app, 'K', KeyModifiers::SHIFT).await;
        assert!(
            matches!(app.modal, Some(Modal::ProcessList { workspace_id, .. }) if workspace_id == ws_id),
            "K on a workspace row should open ProcessList"
        );
    }

    #[tokio::test]
    async fn i_alias_opens_new_workspace_modal_like_enter_on_repo() {
        // On a repo header, Enter opens the New Workspace modal. `i` (vim
        // insert) should do the same — it's the "enter this thing" verb.
        let (mut app, _) = make_app_with_n_repos(1);
        app.dashboard.selected = 0;
        assert!(matches!(
            app.selected_target(),
            Some(SelectionTarget::Repo(_))
        ));
        press(&mut app, 'i', KeyModifiers::NONE).await;
        assert!(
            matches!(app.modal, Some(Modal::NewWorkspace { .. })),
            "i on a repo row should open NewWorkspace like Enter; got {:?}",
            app.modal
        );
    }

    #[tokio::test]
    async fn h_folds_focused_repo() {
        let (mut app, ids) = make_app_with_n_repos(2);
        app.dashboard.selected = 0;
        // Start expanded so we can observe the fold.
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'h', KeyModifiers::NONE).await;
        assert_eq!(
            app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
            Some(true),
            "h should fold the focused repo"
        );
    }

    #[tokio::test]
    async fn l_unfolds_focused_repo() {
        let (mut app, ids) = make_app_with_n_repos(2);
        app.dashboard.selected = 0;
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'l', KeyModifiers::NONE).await;
        assert_eq!(
            app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
            Some(false),
            "l should unfold the focused repo"
        );
    }

    #[tokio::test]
    async fn h_is_idempotent_on_already_folded_repo() {
        // Unlike `zz`, `h` should not toggle — pressing it twice keeps the
        // repo folded. This is the behavior that lets you mash `h` while
        // navigating without accidentally re-opening a row.
        let (mut app, ids) = make_app_with_n_repos(2);
        app.dashboard.selected = 0;
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'h', KeyModifiers::NONE).await;
        press(&mut app, 'h', KeyModifiers::NONE).await;
        assert_eq!(
            app.dashboard.folded.get(&(ids[0].0 as u64)).copied(),
            Some(true),
            "h on an already-folded repo must stay folded"
        );
    }

    #[tokio::test]
    async fn a_alone_is_no_op_on_dashboard() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'a', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "a alone should not arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "a alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn shift_m_alone_is_no_op_on_dashboard() {
        let (mut app, _) = make_app_with_n_repos(2);
        let folded_before = app.dashboard.folded.clone();
        press(&mut app, 'M', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "M alone should not arm the leader");
        assert_eq!(
            app.dashboard.folded, folded_before,
            "M alone should not change fold state"
        );
    }

    #[tokio::test]
    async fn z_m_folds_all_repos_without_shift_modifier() {
        // Some terminals (or CapsLock) report `Char('M')` without
        // KeyModifiers::SHIFT. The chord should still fire — matches
        // the codebase convention for capital-letter binds like `G`.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'M', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zM");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(true),
                "zM (no SHIFT) should fold every repo"
            );
        }
    }

    #[tokio::test]
    async fn zm_folds_all_repos() {
        // Vim `zm` (lowercase m) should fold all repos, same as `zM`.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, false);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'm', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zm");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(true),
                "zm should set repo {id:?} to folded (true)"
            );
        }
    }

    #[tokio::test]
    async fn zr_expands_all_repos() {
        // Vim `zr` (lowercase r) should expand all repos, same as `za`.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'r', KeyModifiers::NONE).await;
        assert!(!app.z_leader_pending, "leader should clear after zr");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(false),
                "zr should set repo {id:?} to expanded (false)"
            );
        }
    }

    #[tokio::test]
    async fn z_shift_r_expands_all_repos() {
        // Vim `zR` (uppercase R) should also expand all repos.
        let (mut app, ids) = make_app_with_n_repos(3);
        app.dashboard.folded.insert(ids[0].0 as u64, true);
        press(&mut app, 'z', KeyModifiers::NONE).await;
        press(&mut app, 'R', KeyModifiers::SHIFT).await;
        assert!(!app.z_leader_pending, "leader should clear after zR");
        for id in &ids {
            assert_eq!(
                app.dashboard.folded.get(&(id.0 as u64)).copied(),
                Some(false),
                "zR should set repo {id:?} to expanded (false)"
            );
        }
    }

    #[tokio::test]
    async fn tab_swap_clears_armed_z_leader() {
        // If the user arms `z` then Tabs over to PM, the leader must
        // clear — otherwise their next key after Tabbing back would
        // be unexpectedly eaten by the z-leader dispatcher.
        let (mut app, _) = make_app_with_n_repos(2);
        // Tab swap path requires PM visible.
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::Dashboard;
        press(&mut app, 'z', KeyModifiers::NONE).await;
        assert!(app.z_leader_pending, "z should arm the leader");
        press_key(&mut app, KeyCode::Tab).await;
        assert!(
            !app.z_leader_pending,
            "Tab to PM should clear the armed leader"
        );
        assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
    }

    fn init_git_repo() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        let r = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .current_dir(dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        };
        r(&["init", "-q", "-b", "main"]);
        r(&["config", "user.email", "t@e"]);
        r(&["config", "user.name", "t"]);
        r(&["commit", "--allow-empty", "-q", "-m", "init"]);
        dir
    }

    #[tokio::test]
    async fn enter_in_new_workspace_modal_transitions_to_setup_running_and_spawns_task() {
        use crate::ui::modal::Modal;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
        }
        // Send Enter.
        let evt = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        {
            let mut g = app.lock().await;
            handle_event(&mut g, &app, CtEvent::Key(evt)).await.unwrap();
            // Immediately after Enter, modal should be SetupRunning.
            assert!(
                matches!(g.modal, Some(Modal::SetupRunning { .. })),
                "modal should transition to SetupRunning immediately; got {:?}",
                g.modal
            );
            assert!(g.pending_create_gen.is_some());
        }
        // Yield so the spawned task gets a chance to complete. 1500ms gives
        // slow CI runners headroom over git init + fetch + worktree create.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        // Eventually, modal should be None and a workspace should exist.
        let g = app.lock().await;
        assert!(
            g.modal.is_none(),
            "modal should clear after create succeeds; got {:?}",
            g.modal
        );
        assert!(g.pending_create_gen.is_none());
        assert_eq!(g.workspaces.len(), 1);
        let _ = repo_id; // suppress unused warning if not referenced above
    }

    #[tokio::test]
    async fn esc_in_setup_running_cancels_and_closes_modal() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        store
            .set_repo_setup_script(repo_id, Some("sleep 5"))
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        // Open the modal and press Enter.
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            assert!(matches!(g.modal, Some(Modal::SetupRunning { .. })));
        }
        // Brief yield so the spawned task gets to start the setup script.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        // Press Esc.
        {
            let mut g = app.lock().await;
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
            assert!(g.modal.is_none(), "modal should close immediately on Esc");
            assert!(g.pending_create_gen.is_none());
        }
        // Wait for the spawned task to wind down.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let g = app.lock().await;
        assert_eq!(g.workspaces.len(), 1);
        assert_eq!(
            g.workspaces[0].1.setup_status,
            crate::store::SetupStatus::Cancelled
        );
        // Modal should still be None — the late reconcile must not pop an error.
        assert!(g.modal.is_none());
    }

    #[tokio::test]
    async fn enter_during_setup_running_is_a_noop() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        store
            .set_repo_setup_script(repo_id, Some("sleep 1"))
            .unwrap();
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            // Press Enter again — should not spawn a second create.
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
        }
        // Wait for the (single) setup to finish.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let g = app.lock().await;
        assert_eq!(
            g.workspaces.len(),
            1,
            "exactly one workspace should be created"
        );
    }

    #[tokio::test]
    async fn successful_create_after_esc_does_not_show_error_modal() {
        use crate::ui::modal::Modal;
        use std::sync::Arc;
        use tokio::sync::Mutex;
        let store = crate::store::Store::open_in_memory().unwrap();
        let repo_dir = init_git_repo();
        let repo_id = crate::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
        // No setup script — create is very fast.
        let tmp = tempfile::TempDir::new().unwrap();
        let app = Arc::new(Mutex::new(
            App::new(store, tmp.path().to_path_buf()).unwrap(),
        ));
        {
            let mut g = app.lock().await;
            g.modal = Some(Modal::NewWorkspace {
                repo_id,
                name_buffer: "alpha".to_string(),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            });
            let enter = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(enter))
                .await
                .unwrap();
            // Immediately Esc — race against the spawned create completing.
            let esc = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Esc,
                crossterm::event::KeyModifiers::empty(),
            );
            handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let g = app.lock().await;
        // Regardless of which side won the race, modal must not be Error.
        assert!(
            !matches!(g.modal, Some(Modal::Error { .. })),
            "Esc race should never produce an error modal, got {:?}",
            g.modal
        );
    }

    fn seed_app_with_workspace() -> App {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Idle repos fold by default; force-expand so the workspace row is
        // visible in `visible_targets` during draw.
        app.dashboard.folded.insert(repo_id.0 as u64, false);
        app
    }

    #[test]
    fn detail_bar_renders_when_workspace_is_selected() {
        let mut app = seed_app_with_workspace();
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(_)))
            .expect("workspace target present");
        app.dashboard.selected = idx;

        let backend = TestBackend::new(120, 30);
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
        assert!(rendered.contains("Reply to agent"), "bar visible: {rendered}");
    }

    #[test]
    fn detail_bar_absent_when_repo_header_is_selected() {
        let mut app = seed_app_with_workspace();
        let repo_idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Repo(_)))
            .expect("repo target present");
        app.dashboard.selected = repo_idx;

        let backend = TestBackend::new(120, 30);
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
            !rendered.contains("Reply to agent"),
            "bar absent on repo header: {rendered}"
        );
    }
}

#[cfg(test)]
mod external_change_polling_tests {
    use super::*;
    use crate::store::{NewWorkspace, Store};

    /// Simulates the bug from issue #70: the dashboard process is holding a
    /// snapshot of workspaces; a separate process (e.g. `wsx workspace
    /// create` driven by Claude during a related-repos flow) writes a new
    /// workspace to the same DB. `poll_external_changes` must pick it up.
    #[test]
    fn poll_external_changes_pulls_in_workspace_added_by_other_process() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("wsx.db");

        // The "TUI" process: opens the store, starts the App.
        let store_tui = Store::open(&db).unwrap();
        let repo_id = store_tui
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let mut app = App::new(store_tui, PathBuf::from("/tmp/wsx-poll-test")).unwrap();
        assert!(app.workspaces.is_empty(), "no workspaces at startup");

        // The "CLI" process: separate connection, writes a new workspace.
        let store_cli = Store::open(&db).unwrap();
        store_cli
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "from-cli",
                branch: "backend/from-cli",
                worktree_path: std::path::Path::new("/wt/from-cli"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();

        // Back in the TUI: the next tick polls and must pick the new row up.
        let changed = app.poll_external_changes();
        assert!(changed, "external commit should trigger a refresh");
        assert_eq!(app.workspaces.len(), 1);
        assert_eq!(app.workspaces[0].1.name, "from-cli");

        // And a second poll with no further writes must be a no-op so we
        // don't churn refresh every tick.
        assert!(
            !app.poll_external_changes(),
            "idle poll must not trigger refresh"
        );
    }
}

#[cfg(test)]
mod layout_indicator_cache_tests {
    use super::*;
    use std::path::PathBuf;

    #[tokio::test]
    async fn app_refresh_populates_layout_indicator_cache_from_store() {
        use crate::store::{NewWorkspace, Store};
        use crate::ui::split::{SplitDirection, SplitTree};
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let a = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/tmp/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let mut pair = SplitTree::Leaf(a);
        pair.split(&[], SplitDirection::Vertical, a);
        store.set_workspace_layout(a, &pair, &[1]).unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        assert!(
            app.workspaces_with_multi_pane_layouts.contains(&a),
            "cache should contain anchor with multi-pane layout"
        );
        // Replace with a single-pane layout — should drop from the cache after refresh.
        app.store
            .set_workspace_layout(a, &SplitTree::Leaf(a), &[])
            .unwrap();
        app.refresh().unwrap();
        assert!(
            !app.workspaces_with_multi_pane_layouts.contains(&a),
            "single-pane layouts should not appear in the cache"
        );
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

    #[test]
    fn alert_decision_suppresses_bell_on_first_observation_during_cold_start() {
        // Cold start: prev is None, workspace already alertable.
        // Visual marker should light up, bell should stay silent.
        let (mark, ring) = alert_decision(None, ActivityState::AwaitingAnswer, true, true);
        assert!(mark, "visual marker must surface on first observation");
        assert!(!ring, "bell must NOT ring during cold start");
    }

    #[test]
    fn alert_decision_rings_on_first_observation_after_cold_start() {
        // A new workspace appears mid-session and is already alertable
        // (e.g. it raced ahead and asked a question before the tail loop
        // could record an intermediate Active). User wants to know.
        let (mark, ring) = alert_decision(None, ActivityState::AwaitingAnswer, true, false);
        assert!(mark);
        assert!(
            ring,
            "bell must ring for a fresh workspace after cold start"
        );
    }

    #[test]
    fn alert_decision_rings_on_transition_into_alertable() {
        // Active -> AwaitingAnswer: real mid-session transition, ring
        // regardless of cold-start window.
        for is_cold_start in [true, false] {
            let (mark, ring) = alert_decision(
                Some(ActivityState::Active),
                ActivityState::AwaitingAnswer,
                true,
                is_cold_start,
            );
            assert!(mark);
            assert!(ring, "transition with prev=Some must always ring");
        }
    }

    #[test]
    fn alert_decision_rings_on_transition_between_alertable_states() {
        // Complete -> Awaiting: permission prompt arrives before the user
        // replied to a prior end_turn. Both alertable, different — ring.
        let (mark, ring) = alert_decision(
            Some(ActivityState::Complete),
            ActivityState::Awaiting,
            true,
            false,
        );
        assert!(mark);
        assert!(ring);
    }

    #[test]
    fn alert_decision_silent_when_alertable_state_persists() {
        // Re-classifying as the same alertable state across polls must
        // not re-ring or re-mark.
        let (mark, ring) = alert_decision(
            Some(ActivityState::AwaitingAnswer),
            ActivityState::AwaitingAnswer,
            true,
            false,
        );
        assert!(!mark);
        assert!(!ring);
    }

    #[test]
    fn alert_decision_silent_for_non_alertable_target() {
        // Transition into Active or Idle is not an alert.
        let (mark, ring) = alert_decision(
            Some(ActivityState::Complete),
            ActivityState::Active,
            true,
            false,
        );
        assert!(!mark);
        assert!(!ring);
    }

    #[test]
    fn alert_decision_silent_when_notifications_off() {
        // Global notification kill switch suppresses everything, even
        // legitimate mid-session transitions.
        let (mark, ring) = alert_decision(
            Some(ActivityState::Active),
            ActivityState::AwaitingAnswer,
            false,
            false,
        );
        assert!(!mark);
        assert!(!ring);
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
        let mut evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            ..Default::default()
        };
        evt.pending_tool_uses
            .insert("t1".into(), ("AskUserQuestion".into(), 0));
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }

    #[test]
    fn awaiting_answer_when_exit_plan_mode_pending_mid_turn() {
        let mut evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            ..Default::default()
        };
        evt.pending_tool_uses
            .insert("t1".into(), ("ExitPlanMode".into(), 0));
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }

    #[test]
    fn complete_when_end_turn_with_no_question_signal() {
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            user_replied_since_stop: false,
            last_assistant_text: Some("Done.".into()),
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::Complete));
    }

    #[test]
    fn awaiting_answer_when_end_turn_with_trailing_question() {
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            user_replied_since_stop: false,
            last_assistant_text: Some("Want me to also handle X?".into()),
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }

    #[test]
    fn none_when_user_has_already_replied() {
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            user_replied_since_stop: true,
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), None);
    }

    #[test]
    fn complete_when_user_interrupted_mid_tool_use() {
        // The exact failure case observed in the lively-myrtle session:
        // last assistant emitted a Bash tool_use (stop_reason=tool_use),
        // tool resolved, then the human hit interrupt. Without the
        // interrupt branch wsx falls through to Stalled after 60s; with
        // it, this is Complete (the agent was told to stop).
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            last_user_interrupted: true,
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::Complete));
    }

    #[test]
    fn awaiting_answer_still_wins_over_interrupt_if_question_tool_pending() {
        // Edge case: interrupt fires while an AskUserQuestion is in
        // flight. The pending question tool should take precedence —
        // there's a real question to answer.
        let mut evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            last_user_interrupted: true,
            ..Default::default()
        };
        evt.pending_tool_uses
            .insert("t1".into(), ("AskUserQuestion".into(), 0));
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }
}

#[cfg(test)]
mod ctrl_x_esc_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_esc_saves_layout_and_returns_to_dashboard() {
        use crate::store::{NewWorkspace, Store, WorkspaceState};
        use crate::ui::split::{AttachedState, SplitDirection};
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
                worktree_path: std::path::Path::new("/tmp/wsx-esc-1"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let second_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "second",
                branch: "repo/second",
                worktree_path: std::path::Path::new("/tmp/wsx-esc-2"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(first_id, WorkspaceState::Ready)
            .unwrap();
        store
            .set_workspace_state(second_id, WorkspaceState::Ready)
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
                first_id,
                std::path::Path::new("."),
                80,
                24,
                mode,
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
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
                crate::remote_control::RemoteOpts::disabled(),
                crate::pty::session::AgentKind::Claude,
            )
            .unwrap();

        let mut state = AttachedState::single(first_id);
        state.split(SplitDirection::Vertical, second_id);
        app.view = crate::ui::View::Attached(state);

        // Send Ctrl-x then Esc.
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
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();

        assert!(
            matches!(app.view, crate::ui::View::Dashboard),
            "should return to dashboard"
        );
        let saved = app.store.get_workspace_layout(first_id).unwrap();
        assert!(saved.is_some(), "layout should be saved under first leaf");
        let (tree, _focus) = saved.unwrap();
        assert_eq!(tree.leaves(), vec![first_id, second_id]);
        assert!(
            app.workspaces_with_multi_pane_layouts.contains(&first_id),
            "cache should refresh to include the new layout's anchor"
        );

        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
}

#[cfg(test)]
mod restore_layout_tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn setup_two_workspaces_with_sessions(
        slug: &str,
    ) -> (App, crate::store::WorkspaceId, crate::store::WorkspaceId) {
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
                worktree_path: std::path::Path::new(&format!("/tmp/wsx-{slug}-1")),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        let second_id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "second",
                branch: "repo/second",
                worktree_path: std::path::Path::new(&format!("/tmp/wsx-{slug}-2")),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
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
                    crate::remote_control::RemoteOpts::disabled(),
                    crate::pty::session::AgentKind::Claude,
                )
                .unwrap();
        }
        (app, first_id, second_id)
    }

    fn select_workspace_in_app(app: &mut App, id: crate::store::WorkspaceId) {
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(w) if *w == id))
            .expect("workspace in selectable list");
        app.dashboard.selected = idx;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_enter_restores_saved_layout() {
        use crate::ui::split::{SplitDirection, SplitTree};
        let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("restore");
        let mut tree = SplitTree::Leaf(first_id);
        tree.split(&[], SplitDirection::Vertical, second_id);
        app.store
            .set_workspace_layout(first_id, &tree, &[1])
            .unwrap();
        app.refresh().unwrap();
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                assert_eq!(s.leaves(), vec![first_id, second_id]);
                assert_eq!(s.focus, vec![1]);
            }
            _ => panic!("expected attached view with restored layout"),
        }
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dashboard_enter_falls_back_to_single_pane_when_no_layout() {
        let (mut app, first_id, _second_id) = setup_two_workspaces_with_sessions("fallback");
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                assert_eq!(s.leaves(), vec![first_id]);
            }
            _ => panic!("expected single-pane attached view"),
        }
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn l_key_opens_workspace_like_enter() {
        let (mut app, first_id, _second_id) = setup_two_workspaces_with_sessions("l-key");
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(
            &mut app,
            KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                assert_eq!(s.leaves(), vec![first_id]);
            }
            _ => panic!("expected single-pane attached view after 'l' on workspace"),
        }
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn restore_prunes_archived_side_panes() {
        use crate::ui::split::{SplitDirection, SplitTree};
        let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("prune");
        let mut tree = SplitTree::Leaf(first_id);
        tree.split(&[], SplitDirection::Vertical, second_id);
        app.store
            .set_workspace_layout(first_id, &tree, &[1])
            .unwrap();
        // Archive second_id directly and refresh so app.workspaces drops it.
        app.store.delete_workspace(second_id).unwrap();
        app.refresh().unwrap();
        select_workspace_in_app(&mut app, first_id);
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE))
            .await
            .unwrap();
        match &app.view {
            crate::ui::View::Attached(s) => {
                assert_eq!(s.leaves(), vec![first_id], "side pane pruned");
            }
            _ => panic!("expected attached view with pruned layout"),
        }
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ctrl_x_d_does_not_modify_saved_layout() {
        use crate::ui::split::{AttachedState, SplitDirection, SplitTree};
        let (mut app, first_id, second_id) = setup_two_workspaces_with_sessions("ctrlxd");
        let mut tree = SplitTree::Leaf(first_id);
        tree.split(&[], SplitDirection::Vertical, second_id);
        app.store
            .set_workspace_layout(first_id, &tree, &[1])
            .unwrap();
        let mut state = AttachedState::single(first_id);
        state.split(SplitDirection::Vertical, second_id);
        app.view = crate::ui::View::Attached(state);
        // Close second pane with Ctrl-x d (focus is on second_id from the split).
        handle_key_attached(
            &mut app,
            second_id,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        )
        .await
        .unwrap();
        handle_key_attached(
            &mut app,
            second_id,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        // Close last pane → dashboard.
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
        // The stored layout is unchanged.
        let (saved, _) = app.store.get_workspace_layout(first_id).unwrap().unwrap();
        assert_eq!(saved.leaves(), vec![first_id, second_id]);
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }
}

#[cfg(test)]
mod detail_bar_focus_tests {
    use super::*;
    use crate::store::{NewWorkspace, Store, WorkspaceState};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn make_app_with_workspace_selected() -> App {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(std::path::Path::new("/tmp/r"), "repo", "")
            .unwrap();
        let id = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "alpha",
                branch: "repo/alpha",
                worktree_path: std::path::Path::new("/tmp/wsx-test/alpha"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(id, WorkspaceState::Ready)
            .unwrap();
        let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        // Force-expand the repo so the workspace stays in `selectable`
        // (idle repos default-fold).
        app.dashboard.folded.insert(repo_id.0 as u64, false);
        let idx = app
            .selectable
            .iter()
            .position(|t| matches!(t, SelectionTarget::Workspace(_)))
            .unwrap();
        app.dashboard.selected = idx;
        app
    }

    #[tokio::test]
    async fn tab_on_workspace_moves_focus_to_detail_bar_reply() {
        let mut app = make_app_with_workspace_selected();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
    }

    #[tokio::test]
    async fn tab_in_detail_bar_returns_focus_to_dashboard() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
    }

    #[tokio::test]
    async fn esc_in_detail_bar_clears_draft_and_returns_to_dashboard() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "half-typed message".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "");
    }

    #[tokio::test]
    async fn char_in_detail_bar_appends_to_draft() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await
            .unwrap();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.reply_draft, "hi");
        // Focus must NOT have changed (this is a regression guard
        // against accidentally letting dashboard hotkeys fire).
        assert!(matches!(app.focus, crate::ui::PaneFocus::DetailBarReply));
    }

    #[tokio::test]
    async fn backspace_in_detail_bar_pops_last_char() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "abc".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.reply_draft, "ab");
    }

    #[tokio::test]
    async fn arrow_down_while_focused_returns_to_dashboard_and_clears_draft() {
        let mut app = make_app_with_workspace_selected();
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        app.dashboard.reply_draft = "draft".to_string();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "");
    }

    // Issue 2: Tab cycle should include PM when visible.
    #[tokio::test]
    async fn tab_in_detail_bar_routes_to_pm_when_visible() {
        let mut app = make_app_with_workspace_selected();
        app.pm_visible = true;
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::ProjectManager));
    }

    // Issue 3: Arrow navigation in Dashboard focus must clear the reply draft
    // so it cannot be sent to the wrong workspace.
    #[tokio::test]
    async fn arrow_down_in_dashboard_focus_clears_reply_draft() {
        let mut app = make_app_with_workspace_selected();
        // Compose a draft in DetailBarReply, then Tab back to Dashboard.
        app.focus = crate::ui::PaneFocus::DetailBarReply;
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE))
            .await
            .unwrap();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE))
            .await
            .unwrap();
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE))
            .await
            .unwrap();
        assert!(matches!(app.focus, crate::ui::PaneFocus::Dashboard));
        assert_eq!(app.dashboard.reply_draft, "hi");

        // Now arrow-navigate. The draft should be discarded so it can't
        // be sent to the wrong workspace.
        handle_key_dashboard(&mut app, KeyEvent::new(KeyCode::Down, KeyModifiers::NONE))
            .await
            .unwrap();
        assert_eq!(app.dashboard.reply_draft, "", "draft must clear on navigation");
    }
}
