#![allow(clippy::collapsible_if)]

use crate::error::Result;
use crate::pty::session::SessionManager;
use crate::store::{Repo, Store, Workspace, WorkspaceId};
use crate::ui::View;
use crate::ui::dashboard::DashboardState;
use crate::ui::modal::Modal;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

pub mod activity;
pub mod background;
pub mod bell;
pub mod input;
pub mod render;
pub use crate::app::activity::{ActivityState, classify_activity, classify_activity_with_events};
pub use crate::app::background::{branch_drift_poll, tail_workspace_events};
pub use crate::app::bell::{BellPattern, COLD_START_WINDOW, alert_decision, fire_bell};
pub use crate::app::render::draw_for_test;

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
    DetailBarConfig,
}

impl RepoSettingField {
    pub const ALL: [Self; 9] = [
        Self::RepoName,
        Self::BranchPrefix,
        Self::BaseBranch,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
        Self::RelatedRepos,
        Self::DetailBarConfig,
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
            Self::DetailBarConfig => "detail_bar_config",
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
    /// Monotonic counter handed out to in-flight workspace archive tasks.
    pub next_archive_gen: u64,
    /// Generation id of the currently in-flight workspace archive, if any.
    /// Used by the reconcile step to detect stale completions.
    pub pending_archive_gen: Option<u64>,
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
    /// Per-file diff stats keyed by `WorkspaceId`, then by path relative
    /// to the worktree root (as `git diff --numstat` emits them).
    /// Populated by the same poller that maintains `workspace_diff`.
    /// Used by the detail bar's RECENT FILES section to annotate each
    /// file with its `+X −Y` delta.
    pub workspace_diff_per_file: std::collections::HashMap<
        crate::store::WorkspaceId,
        std::collections::HashMap<String, crate::git::DiffStats>,
    >,
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
    /// Per-slot scroll offset for detail-bar containers. Bumped by mouse
    /// wheel via `handle_mouse`, clamped on every draw to
    /// `content_height - visible_height` for the matching container.
    pub detail_scroll_offsets: [u16; 4],
    /// Sentinel for reset-on-workspace-switch. When the selected
    /// workspace changes, `detail_scroll_offsets` zeroes out and this
    /// updates. See `src/ui/dashboard/detail.rs::render`.
    pub detail_scroll_last_workspace: Option<crate::store::WorkspaceId>,
    /// Rect for each rendered detail-bar container slot, populated each
    /// draw and consumed by `handle_mouse` for hit-testing wheel events.
    /// Mirrors the `chip_rects` draw-populates-input-reads pattern.
    pub detail_container_rects: [Option<ratatui::layout::Rect>; 4],
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
    /// Workspaces whose detail-bar data needs an out-of-band refresh on
    /// the next run-loop iteration. Populated by detach handlers so the
    /// dashboard shows fresh JSONL events the moment the user returns
    /// from attached view instead of waiting for the next 2s poll.
    /// Drained by `run_loop` after each handled event.
    pub pending_workspace_refresh: std::collections::HashSet<crate::store::WorkspaceId>,
    pub registry: crate::detail_modules::Registry,
}

impl App {
    pub fn new(store: Store, worktree_base: PathBuf) -> Result<Self> {
        let theme_name = store
            .get_setting("theme")
            .ok()
            .flatten()
            .unwrap_or_default();
        let theme = crate::ui::theme::Theme::by_name(&theme_name);
        let mut registry = crate::detail_modules::Registry::new();
        crate::detail_modules::register_builtins(&mut registry);
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
            workspace_diff_per_file: std::collections::HashMap::new(),
            activity_history: std::collections::VecDeque::new(),
            last_proc_scan_ms: 0,
            pending_workspace_refresh: std::collections::HashSet::new(),
            pending_edit: None,
            theme,
            pm: None,
            pm_visible: false,
            focus: crate::ui::PaneFocus::Dashboard,
            pm_auto_summary_sent: false,
            next_create_gen: 0,
            pending_create_gen: None,
            next_archive_gen: 0,
            pending_archive_gen: None,
            chip_rects: Vec::new(),
            detail_scroll_offsets: [0; 4],
            detail_scroll_last_workspace: None,
            detail_container_rects: [None; 4],
            pinned_commands_cache: Vec::new(),
            pending_bells: Vec::new(),
            started_at: std::time::Instant::now(),
            last_data_version: 0,
            registry,
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

    /// Allocate a fresh generation id for a new workspace-archive task.
    pub fn alloc_archive_gen(&mut self) -> u64 {
        let g = self.next_archive_gen;
        self.next_archive_gen = self.next_archive_gen.wrapping_add(1);
        self.pending_archive_gen = Some(g);
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

/// Zero detail-bar scroll offsets and update the sentinel when the
/// selected workspace changes. Called by `app::render::draw` before the
/// detail bar renders. Takes the two fields by mutable reference rather
/// than `&mut App` so the caller can hold an immutable borrow of
/// `app.workspaces` (or another field) at the same call site — direct
/// field access lets the borrow checker split disjoint borrows where a
/// method on `&mut App` cannot.
pub(crate) fn reset_detail_scroll_on_workspace_change(
    offsets: &mut [u16; 4],
    last_workspace: &mut Option<crate::store::WorkspaceId>,
    current: Option<crate::store::WorkspaceId>,
) {
    if *last_workspace != current {
        *offsets = [0; 4];
        *last_workspace = current;
    }
}

/// Derive the StoppedKind for a workspace based on its WorkspaceEvents.
/// Returns Some when the agent is paused waiting on the user (either
/// mid-turn with a pending question tool, or end-of-turn with a
/// trailing question / completion).
pub(crate) fn derive_stopped_kind(e: &crate::events::WorkspaceEvents) -> Option<StoppedKind> {
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

use crossterm::event::EventStream;
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
            RepoSettingField::DetailBarConfig => {
                let raw = repo
                    .detail_bar_config
                    .clone()
                    .unwrap_or_else(|| "{}\n".to_string());
                (raw, "json")
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
            if let Err(e) = apply_repo_setting(&mut g, edit.repo_id, edit.field, &new) {
                g.modal = Some(crate::ui::modal::Modal::Error {
                    message: e.to_string(),
                });
            } else {
                let _ = g.refresh();
            }
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
            terminal.draw(|f| crate::app::render::draw(f, &mut g))?;
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
                // Expire any ephemeral chip-dispatch echo in the reply
                // input. Set by `fire_chip` so the user briefly sees
                // which command was sent; wiped here once the deadline
                // is reached.
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                if matches!(g.dashboard.reply_draft_clear_at_ms, Some(t) if now_ms >= t) {
                    g.dashboard.reply_draft.clear();
                    g.dashboard.reply_draft_clear_at_ms = None;
                }
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
                // Drain any refreshes scheduled by detach handlers while
                // we held the lock; resolve each id to its (path, agent)
                // pair under the same lock so the spawned tail doesn't
                // need to walk `App::workspaces`. Then spawn outside the
                // lock so the tails don't serialize event handling.
                let pending: Vec<(
                    WorkspaceId,
                    std::path::PathBuf,
                    crate::pty::session::AgentKind,
                )> = {
                    let mut g = app.lock().await;
                    crate::app::input::handle_event(&mut g, &app, evt).await?;
                    let ids: Vec<WorkspaceId> =
                        g.pending_workspace_refresh.drain().collect();
                    ids.into_iter()
                        .filter_map(|id| {
                            g.workspaces
                                .iter()
                                .find(|(_, w)| w.id == id)
                                .map(|(_, w)| (id, w.worktree_path.clone(), w.agent))
                        })
                        .collect()
                };
                for (id, path, agent) in pending {
                    let app_clone = app.clone();
                    tokio::spawn(async move {
                        tail_workspace_events(app_clone, id, path, agent).await;
                    });
                }
            }
        }
    }
    Ok(())
}

/// Immediately re-run `proc::scan` and re-bucket. Used after a kill
/// so the modal reflects the new state without waiting for the
/// next 10s poll tick.
pub(crate) async fn rescan_processes(app: &mut App) {
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

pub(crate) fn apply_repo_setting(
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
        RepoSettingField::DetailBarConfig => {
            if trimmed.is_empty() {
                app.store.set_repo_detail_bar_config(repo_id, None)
            } else {
                // Validate. Use DetailBarOverride (not DetailBarConfig)
                // because per-repo entries are partial overrides.
                match serde_json::from_str::<crate::detail_bar_config::DetailBarOverride>(trimmed) {
                    Ok(_) => app.store.set_repo_detail_bar_config(repo_id, Some(trimmed)),
                    Err(e) => Err(crate::error::Error::UserInput(format!(
                        "detail_bar_config is not valid JSON: {e}"
                    ))),
                }
            }
        }
    }
}

pub(crate) fn build_spawn_info(
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
                repo_name: repo.name.clone(),
                current_slug: ws.name.clone(),
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

pub(crate) fn save_layout_for(app: &mut App, state: crate::ui::AttachedState) {
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
pub(crate) fn restore_attached_state(
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

/// Ensure a workspace has a live PTY session, spawning one in place if
/// missing. Used by `attach_workspace` and by inline-dispatch paths
/// (chip click / chord / reply Enter) so writes from the dashboard
/// don't silently drop on workspaces the user hasn't attached to.
/// No-op when the workspace already has a session, or when
/// `build_spawn_info` returns `None` (e.g., setup hasn't completed).
pub(crate) fn ensure_workspace_session(
    app: &mut App,
    ws_id: crate::store::WorkspaceId,
) -> Result<()> {
    if app.sessions.get(ws_id).is_some() {
        return Ok(());
    }
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, ws_id) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::remote_control::RemoteOpts::from_store(&app.store);
        let _ = app.sessions.spawn(id, &path, 80, 24, mode, remote, agent)?;
    }
    Ok(())
}

/// Attach to a workspace: ensure a session, restore layout, and switch
/// to attached view. Shared by the `Enter` / `i` / `l` key handlers.
pub(crate) fn attach_workspace(app: &mut App, ws_id: crate::store::WorkspaceId) -> Result<()> {
    app.workspace_needs_attention.remove(&ws_id);
    ensure_workspace_session(app, ws_id)?;
    if app.sessions.get(ws_id).is_some() {
        let restored = restore_attached_state(app, ws_id);
        app.view = View::Attached(restored);
    }
    Ok(())
}

/// Best-effort MCP server mirror. Logs and continues on any failure.
pub(crate) fn maybe_mirror_mcp(
    app: &App,
    repo_path: &std::path::Path,
    worktree_path: &std::path::Path,
) {
    if !crate::mcp::enabled(&app.store) {
        return;
    }
    if let Err(e) = crate::mcp::mirror_mcp_servers(repo_path, worktree_path) {
        tracing::warn!(error = %e, "failed to mirror MCP servers; continuing");
    }
}

/// Mark `ids` for an immediate out-of-band refresh: clear the per-workspace
/// throttle stamps (so the next periodic poll re-fetches diff/PR right
/// away), reset `last_proc_scan_ms` to 0 so the next tick reruns `lsof`,
/// and queue the workspaces into `pending_workspace_refresh` so `run_loop`
/// spawns an immediate JSONL events tail. Called by detach handlers so
/// the dashboard detail bar reflects work the user just did in the
/// attached session instead of waiting for the next 2s tick.
pub(crate) fn schedule_detach_refresh(app: &mut App, ids: impl IntoIterator<Item = WorkspaceId>) {
    app.last_proc_scan_ms = 0;
    for id in ids {
        app.diff_last_poll_ms.remove(&id);
        app.pr_last_poll_ms.remove(&id);
        app.pending_workspace_refresh.insert(id);
    }
}
/// Reconcile the outcome of a spawned `workspace::create_with_app` task.
/// Locks the app briefly; if the modal is still `SetupRunning` AND the
/// generation matches ours, applies the outcome (close modal on success,
/// switch to `Modal::Error` on failure). Otherwise — user dismissed or
/// started a new create — leaves the modal alone but still calls
/// `refresh()` so the dashboard reflects any state we wrote to the store.
pub(crate) async fn reconcile_create_result(
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

/// Reconcile the outcome of a spawned `workspace::archive` task.
/// Locks the app briefly; if the modal is still `ArchiveRunning` AND the
/// generation matches ours, applies the outcome (close modal on success,
/// switch to `Modal::Error` on failure). Otherwise — user dismissed or
/// some other flow replaced the modal — leaves the modal alone but still
/// calls `refresh()` so the dashboard reflects the store mutation.
pub(crate) async fn reconcile_archive_result(
    app: SharedApp,
    my_gen: u64,
    result: Result<crate::setup::SetupResult>,
) {
    let mut g = app.lock().await;
    let is_mine = g.pending_archive_gen == Some(my_gen);
    if is_mine {
        g.pending_archive_gen = None;
    }
    match result {
        Ok(_) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::ArchiveRunning { .. })) {
                g.modal = None;
            }
            let _ = g.refresh();
        }
        Err(e) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::ArchiveRunning { .. })) {
                g.modal = Some(crate::ui::modal::Modal::Error {
                    message: e.to_string(),
                });
            }
            let _ = g.refresh();
        }
    }
}

#[cfg(test)]
mod reconcile_archive_tests {
    use super::*;
    use crate::error::Error;
    use crate::setup::SetupResult;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    fn make_app() -> (App, TempDir) {
        let store = crate::store::Store::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        let app = App::new(store, tmp.path().to_path_buf()).unwrap();
        (app, tmp)
    }

    #[tokio::test]
    async fn reconcile_ok_closes_archive_running_modal() {
        let (mut app, _tmp) = make_app();
        app.modal = Some(crate::ui::modal::Modal::ArchiveRunning {
            step: crate::ui::modal::ArchiveStep::RemoveWorktree,
            script_present: false,
        });
        app.pending_archive_gen = Some(7);
        app.next_archive_gen = 8;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(shared.clone(), 7, Ok(SetupResult::Ok)).await;
        let g = shared.lock().await;
        assert!(g.modal.is_none(), "modal should clear on Ok; got {:?}", g.modal);
        assert!(g.pending_archive_gen.is_none(), "pending_archive_gen should clear after matching reconcile");
    }

    #[tokio::test]
    async fn reconcile_err_sets_error_modal() {
        let (mut app, _tmp) = make_app();
        app.modal = Some(crate::ui::modal::Modal::ArchiveRunning {
            step: crate::ui::modal::ArchiveStep::RemoveWorktree,
            script_present: false,
        });
        app.pending_archive_gen = Some(7);
        app.next_archive_gen = 8;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(
            shared.clone(),
            7,
            Err(Error::Setup("boom".into())),
        )
        .await;
        let g = shared.lock().await;
        match &g.modal {
            Some(crate::ui::modal::Modal::Error { message }) => {
                assert!(message.contains("boom"), "error message should contain failure detail; got {message:?}");
            }
            other => panic!("expected Modal::Error, got {other:?}"),
        }
        assert!(g.pending_archive_gen.is_none(), "pending_archive_gen should clear after matching reconcile");
    }

    #[tokio::test]
    async fn reconcile_skips_modal_mutation_when_gen_mismatch() {
        let (mut app, _tmp) = make_app();
        // Simulate: a different modal is already showing (e.g. an Error
        // popped by another flow) and pending_archive_gen advanced past
        // the value our stale task carries.
        app.modal = Some(crate::ui::modal::Modal::Error {
            message: "untouched".into(),
        });
        app.pending_archive_gen = Some(99);
        app.next_archive_gen = 100;
        let shared = Arc::new(Mutex::new(app));
        reconcile_archive_result(
            shared.clone(),
            7, // stale — does not match pending_archive_gen
            Err(Error::Setup("ignored".into())),
        )
        .await;
        let g = shared.lock().await;
        match &g.modal {
            Some(crate::ui::modal::Modal::Error { message }) => {
                assert_eq!(message, "untouched", "stale reconcile must not overwrite modal");
            }
            other => panic!("expected the pre-existing Error modal to survive, got {other:?}"),
        }
        assert_eq!(g.pending_archive_gen, Some(99), "stale reconcile must not clear pending_archive_gen");
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

    #[test]
    fn reset_detail_scroll_zeroes_offsets_on_workspace_change() {
        use crate::store::WorkspaceId;
        let mut offsets = [3u16, 7, 0, 2];
        let mut last = Some(WorkspaceId(100));

        super::reset_detail_scroll_on_workspace_change(&mut offsets, &mut last, Some(WorkspaceId(200)));

        assert_eq!(offsets, [0; 4]);
        assert_eq!(last, Some(WorkspaceId(200)));
    }

    #[test]
    fn reset_detail_scroll_preserves_offsets_on_same_workspace() {
        use crate::store::WorkspaceId;
        let mut offsets = [3u16, 7, 0, 2];
        let mut last = Some(WorkspaceId(100));

        super::reset_detail_scroll_on_workspace_change(&mut offsets, &mut last, Some(WorkspaceId(100)));

        assert_eq!(offsets, [3, 7, 0, 2]);
        assert_eq!(last, Some(WorkspaceId(100)));
    }

    #[test]
    fn reset_detail_scroll_handles_initial_none_to_some() {
        use crate::store::WorkspaceId;
        // App starts with detail_scroll_last_workspace = None and offsets
        // already zero; first draw with a selected workspace should update
        // the sentinel even though the offsets are technically unchanged.
        let mut offsets = [5u16, 0, 0, 0]; // seeded non-zero
        let mut last: Option<WorkspaceId> = None;

        super::reset_detail_scroll_on_workspace_change(&mut offsets, &mut last, Some(WorkspaceId(42)));

        assert_eq!(offsets, [0; 4]);
        assert_eq!(last, Some(WorkspaceId(42)));
    }
}
