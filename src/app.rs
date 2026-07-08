#![allow(clippy::collapsible_if)]

use crate::data::store::{Repo, Store, Workspace, WorkspaceId};
use crate::error::Result;
use crate::pty::session::SessionManager;
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
pub mod messaging;
pub mod render;
pub mod resize_sync;
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
    Repo(crate::data::store::RepoId),
    Workspace(crate::data::store::WorkspaceId),
}

/// Outcome of `ensure_workspace_session`. `AgentMissing` signals to callers
/// that the spawn failed because the agent binary was not on PATH; the
/// helper already set `Modal::AgentMissing`, so callers should skip the
/// view switch and leave the modal up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachReady {
    Ok,
    AgentMissing,
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
    pub repo_id: crate::data::store::RepoId,
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

/// How many hourly activity buckets to retain, in memory and in the DB. Sized
/// to the largest selectable usage-graph window (30 days), so the setting is
/// purely a view over already-collected data rather than affecting retention.
const MAX_ACTIVITY_HOURS: u64 = 720;

pub struct App {
    pub store: Store,
    pub sessions: SessionManager,
    /// Coalesces terminal-resize events so backgrounded sessions are resized
    /// once the resize settles. See `crate::app::resize_sync`.
    pub resize_debounce: crate::app::resize_sync::ResizeDebounce,
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
    pub workspaces: Vec<(crate::data::store::RepoId, Workspace)>,
    pub selectable: Vec<SelectionTarget>,
    pub worktree_base: PathBuf,
    pub leader_pending: bool,
    /// Highlighted row in the Ctrl-x navigation overlay. Reset to 0 each time
    /// the attached/PM leader is armed; adjusted by ↑↓ while the overlay is up.
    pub leader_selected: usize,
    pub z_leader_pending: bool,
    pub quit: bool,
    pub workspace_status:
        std::collections::HashMap<crate::data::store::WorkspaceId, crate::git::WorkspaceStatus>,
    /// Cached PR lifecycle per workspace. Absent key = never polled; present
    /// key = last successful poll's result.
    pub pr_lifecycle: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::git::forge::BranchLifecycle,
    >,
    /// Cached PR number per workspace, populated alongside `pr_lifecycle`.
    /// Absent key = unknown. Used to render `#<n>` in the detail-bar chip.
    pub pr_number: std::collections::HashMap<crate::data::store::WorkspaceId, u32>,
    /// Screen rect of the clickable PR chip in the detail-bar header, with
    /// the workspace it belongs to. Set during draw, read by the mouse
    /// handler. Mirrors the `chip_rects` draw-populates / input-reads pattern.
    pub pr_link_rect: Option<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
    /// Screen rect of the clickable running-process count (`● Np`) on the
    /// attached view's chip row, with the workspace it belongs to. Set during
    /// draw, read by the mouse handler to open the process-list modal on click.
    /// Mirrors the `pr_link_rect` draw-populates / input-reads pattern.
    pub procs_link_rect: Option<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
    /// Last epoch-ms we attempted a PR fetch per workspace (throttle key).
    pub pr_last_poll_ms: std::collections::HashMap<crate::data::store::WorkspaceId, i64>,
    /// Last epoch-ms we attempted a `git diff --shortstat` per workspace
    /// (throttle key). 10s minimum interval keeps the dashboard
    /// `+N −N` cell fresh without re-running diff on every 2s tick.
    pub diff_last_poll_ms: std::collections::HashMap<crate::data::store::WorkspaceId, i64>,
    pub workspace_events: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::activity::events::WorkspaceEvents,
    >,
    /// Last agent-pushed status per workspace, loaded from the store in
    /// `refresh()` (which fires on every external-change tick — a sibling
    /// `wsx status` write bumps `data_version`).
    pub pushed_status: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::data::store::ReportedStatus,
    >,
    /// Per-workspace tracking for attention-alert state.
    pub workspace_activity:
        std::collections::HashMap<crate::data::store::WorkspaceId, ActivityState>,
    /// Workspaces whose JSONL events have been read at least once by the
    /// tail loop. Until a workspace is in this set the classifier's output
    /// is provisional (it can only see session-liveness, not stop_reason),
    /// so we hold off on recording activity / firing bells for it. Without
    /// this gate the classifier flickers from Active → AwaitingAnswer the
    /// instant the tail loop catches up, which the bell loop would treat
    /// as a legitimate transition and ring on cold start.
    pub workspace_events_scanned: std::collections::HashSet<crate::data::store::WorkspaceId>,
    /// Workspaces whose alert hasn't been acknowledged (cleared on attach).
    pub workspace_needs_attention: std::collections::HashSet<crate::data::store::WorkspaceId>,
    /// Anchors whose saved layout has more than one pane. Used by the
    /// dashboard to render the split-layout indicator. Recomputed by
    /// `App::refresh`.
    pub workspaces_with_multi_pane_layouts:
        std::collections::HashSet<crate::data::store::WorkspaceId>,
    /// Processes detected per workspace (cwd inside the workspace's
    /// worktree). Refreshed every ~10s by branch_drift_poll.
    pub workspace_processes: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        Vec<crate::activity::proc::ProcInfo>,
    >,
    /// Monotonic counter incremented every animation tick. Drives
    /// dashboard spinner phase + any other tick-driven UI animation.
    pub tick: u32,
    /// Cached `git diff --shortstat` output per workspace (added/deleted).
    /// Populated lazily by the workspace-status poller.
    pub workspace_diff:
        std::collections::HashMap<crate::data::store::WorkspaceId, crate::git::DiffStats>,
    /// Per-file diff stats keyed by `WorkspaceId`, then by path relative
    /// to the worktree root (as `git diff --numstat` emits them).
    /// Populated by the same poller that maintains `workspace_diff`.
    /// Used by the detail bar's RECENT FILES section to annotate each
    /// file with its `+X −Y` delta.
    pub workspace_diff_per_file: std::collections::HashMap<
        crate::data::store::WorkspaceId,
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
    /// Rects of the rendered attention-row entries from the last draw tick,
    /// each paired with the workspace it points to. Consumed by `handle_mouse`
    /// to attach on click. Mirrors the `chip_rects` draw-populates /
    /// input-reads pattern; cleared each frame.
    pub attention_rects: Vec<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
    /// Per-slot scroll offset for detail-bar containers. Bumped by mouse
    /// wheel via `handle_mouse`, clamped on every draw to
    /// `content_height - visible_height` for the matching container.
    pub detail_scroll_offsets: [u16; 4],
    /// Sentinel for reset-on-workspace-switch. When the selected
    /// workspace changes, `detail_scroll_offsets` zeroes out and this
    /// updates. See `src/ui/dashboard/detail.rs::render`.
    pub detail_scroll_last_workspace: Option<crate::data::store::WorkspaceId>,
    /// Rect for each rendered detail-bar container slot, populated each
    /// draw and consumed by `handle_mouse` for hit-testing wheel events.
    /// Mirrors the `chip_rects` draw-populates-input-reads pattern.
    pub detail_container_rects: [Option<ratatui::layout::Rect>; 4],
    /// Per-pane `(session, content rect)` from the last attached-view draw.
    /// Consumed by `handle_mouse` to find the pane under the cursor and
    /// forward wheel events to a mouse-aware agent. Storing the `Arc<Session>`
    /// directly lets the PM pane (which lives in `app.pm`, not `app.sessions`)
    /// be recorded the same way as workspace panes. Cleared each frame.
    pub attached_pane_rects: Vec<(
        std::sync::Arc<crate::pty::session::Session>,
        ratatui::layout::Rect,
    )>,
    /// `(instance id, rect)` for each agent pill in the footer agents row,
    /// populated each attached-view draw and consumed by `handle_mouse` to
    /// retarget the focused pane on click. Mirrors the `chip_rects`
    /// draw-populates / input-reads pattern; cleared each frame.
    pub agent_chip_rects: Vec<(crate::data::store::AgentInstanceId, ratatui::layout::Rect)>,
    /// Rect of the footer activity graph from the last draw, used by
    /// `handle_mouse` to open the usage-window picker on click. `None` when the
    /// footer is not currently drawn. Mirrors the `chip_rects` draw-populates /
    /// input-reads pattern; reset each frame.
    pub usage_graph_rect: Option<ratatui::layout::Rect>,
    /// `(rect, action)` for each clickable footer keybind hint from the last
    /// draw (dashboard, attached, and PM footers). Consumed by `handle_mouse`
    /// to fire the matching key/leader on click. Cleared each frame.
    pub footer_hint_rects: Vec<(ratatui::layout::Rect, crate::ui::footer::FooterHintAction)>,
    /// Per-option row rects of the open usage-window picker, in `UsageWindow::ALL`
    /// order, consumed by `handle_mouse` to apply a clicked option. Cleared each
    /// frame; only populated while the picker modal is open.
    pub usage_window_option_rects: Vec<ratatui::layout::Rect>,
    /// Resolved pinned commands from the last draw tick (matches `chip_rects`).
    pub pinned_commands_cache: Vec<crate::commands::pinned::PinnedCommand>,
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
    pub pending_workspace_refresh: std::collections::HashSet<crate::data::store::WorkspaceId>,
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
            resize_debounce: Default::default(),
            view: View::Dashboard,
            modal: None,
            dashboard: DashboardState::default(),
            repos: Vec::new(),
            workspaces: Vec::new(),
            selectable: Vec::new(),
            worktree_base,
            leader_pending: false,
            leader_selected: 0,
            z_leader_pending: false,
            quit: false,
            workspace_status: std::collections::HashMap::new(),
            pr_lifecycle: std::collections::HashMap::new(),
            pr_number: std::collections::HashMap::new(),
            pr_link_rect: None,
            procs_link_rect: None,
            pr_last_poll_ms: std::collections::HashMap::new(),
            diff_last_poll_ms: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
            pushed_status: std::collections::HashMap::new(),
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
            attention_rects: Vec::new(),
            detail_scroll_offsets: [0; 4],
            detail_scroll_last_workspace: None,
            detail_container_rects: [None; 4],
            attached_pane_rects: Vec::new(),
            agent_chip_rects: Vec::new(),
            usage_graph_rect: None,
            footer_hint_rects: Vec::new(),
            usage_window_option_rects: Vec::new(),
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
        // Load the retained bucketed activity for the sparkline (up to
        // MAX_ACTIVITY_HOURS); the configured window selects how much is shown.
        if let Ok(buckets) = app
            .store
            .recent_activity_buckets(MAX_ACTIVITY_HOURS as usize)
        {
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
        self.pushed_status = self.store.all_workspace_status().unwrap_or_default();
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

    /// The durable, authoritative selection target. Returns
    /// `dashboard.selection` rather than indexing `selectable`, so the
    /// selection survives a temporarily-hidden row (folded repo / filter /
    /// quiet repo) instead of silently following the index onto a neighbor.
    pub fn selected_target(&self) -> Option<SelectionTarget> {
        self.dashboard.selection
    }

    /// Worktree path of the workspace with `id`, or `None` if it's not in the
    /// current list. Centralizes the `workspaces.iter().find(...).map(...)`
    /// lookup that the key handlers repeat to launch external tools.
    pub(crate) fn workspace_path(
        &self,
        id: crate::data::store::WorkspaceId,
    ) -> Option<std::path::PathBuf> {
        self.workspaces
            .iter()
            .find(|(_, w)| w.id == id)
            .map(|(_, w)| w.worktree_path.clone())
    }

    /// Set the selection by index into the current `selectable`, keeping the
    /// durable `selection` target and the `selected` nav cursor in sync. Use
    /// this anywhere selection *intent* changes via an index (nav, click,
    /// landing on a freshly-created workspace).
    pub(crate) fn select_index(&mut self, idx: usize) {
        self.dashboard.selected = idx;
        self.dashboard.selection = self.selectable.get(idx).copied();
    }

    /// Whether a selection target still refers to a live repo/workspace.
    /// Used by `reconcile_selection` to tell a temporarily-hidden target
    /// (park it) from a removed one (fall back to a neighbor).
    pub(crate) fn selection_target_exists(&self, t: SelectionTarget) -> bool {
        match t {
            SelectionTarget::Repo(id) => self.repos.iter().any(|r| r.id == id),
            SelectionTarget::Workspace(id) => self.workspaces.iter().any(|(_, w)| w.id == id),
        }
    }

    /// The primary agent instance for a workspace (the creation-time agent).
    ///
    /// Deliberately collapses a DB error to `None` (same as "no primary
    /// instance") — callers are read/render paths where degrading to
    /// "session-less for this frame" is acceptable and self-heals next frame.
    /// Spawn paths that must not silently skip seeding use
    /// `resolve_primary_instance` (which returns `Result`) instead.
    pub(crate) fn primary_instance(
        &self,
        ws: crate::data::store::WorkspaceId,
    ) -> Option<crate::data::store::AgentInstanceId> {
        self.store.primary_instance_id(ws).ok().flatten()
    }

    /// The live session for a given agent instance, if any.
    pub(crate) fn session_for(
        &self,
        inst: crate::data::store::AgentInstanceId,
    ) -> Option<std::sync::Arc<crate::pty::session::Session>> {
        self.sessions.get(inst)
    }

    /// Apply a settled terminal resize to backgrounded sessions. Computes the
    /// projected single-pane size for the new terminal dimensions and resizes
    /// every running, non-visible session so re-attaching after a resize shows
    /// a freshly-repainted frame instead of one the vt100 parser clipped to
    /// stale dimensions. Visible panes are handled by the render path and left
    /// untouched here.
    ///
    /// The PM session (`app.pm`) is render-synced on the dashboard and in
    /// `AttachedPm`, but goes stale while attached to an agent — no render path
    /// touches it there. So when attached to an agent we resize it too, to the
    /// projected size `AttachedPm` will use next. See `crate::app::resize_sync`.
    pub fn apply_backgrounded_resize(&self, cols: u16, rows: u16) {
        let (w, h) = crate::app::resize_sync::projected_pane_size(cols, rows);
        let visible = crate::app::resize_sync::visible_instances(&self.view);
        self.sessions.resize_backgrounded(w, h, &visible);
        if crate::app::resize_sync::should_sync_pm(&self.view)
            && let Some(pm) = &self.pm
        {
            let _ = pm.resize(w, h);
        }
    }

    /// Retarget the focused attached pane to `inst` (switching the visible agent
    /// in place), spawning its session if needed. No-op if not in attached view
    /// or the instance is unknown.
    pub(crate) fn switch_focused_pane_to(
        &mut self,
        inst: crate::data::store::AgentInstanceId,
    ) -> Result<()> {
        // Only retarget once the session is actually ready. On a missing
        // binary, ensure_instance_session sets the AgentMissing modal and
        // returns AgentMissing WITHOUT spawning — retargeting anyway would
        // point the focused leaf at a sessionless instance, and the next
        // draw's "leaf session missing -> bounce to Dashboard" guard would
        // then collapse the whole split. Mirror attach_workspace and bail.
        match ensure_instance_session(self, inst, true)? {
            AttachReady::Ok => {}
            AttachReady::AgentMissing => return Ok(()),
        }
        let Some(instance) = self.store.workspace_agents_by_id(inst)? else {
            return Ok(());
        };
        if let crate::ui::View::Attached(state) = &mut self.view {
            let target = crate::ui::split::AttachTarget {
                workspace_id: instance.workspace_id,
                instance: inst,
            };
            let path = state.focus.clone();
            state.set_leaf_target(&path, target);
        }
        Ok(())
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
    pub fn awaiting_permission(
        &self,
        ws_id: crate::data::store::WorkspaceId,
    ) -> Option<(String, i64)> {
        let evt = self.workspace_events.get(&ws_id)?;
        let now = crate::time::now_ms();
        evt.pending_permission_tool(now, 3_000)
    }

    /// Classify a workspace into the V5 dashboard `Status` vocabulary.
    /// Combines session liveness, JSONL stopped/stalled signals, and
    /// pending tool_use into one canonical state used by the renderer.
    pub fn classify_status(
        &self,
        ws: &crate::data::store::Workspace,
    ) -> crate::ui::dashboard::status::Status {
        let session = self
            .primary_instance(ws.id)
            .and_then(|i| self.sessions.get(i));
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
        let secs = session.as_ref().and_then(|s| s.idle_secs());
        // `has_prior_session` does filesystem I/O (canonicalize +
        // read_dir); skip it when we already have a live session, since
        // the classifier only looks at it in the no-session branch.
        let has_prior = if running {
            false
        } else {
            crate::pty::session::has_prior_session_for(&ws.worktree_path, ws.agent)
        };
        let now_ms = crate::time::now_ms();
        let stopped_kind = self
            .workspace_events
            .get(&ws.id)
            .and_then(derive_stopped_kind);
        let stalled = self
            .workspace_events
            .get(&ws.id)
            .is_some_and(|e| e.is_stalled(now_ms, 60_000));
        let awaiting = self.awaiting_permission(ws.id).is_some();
        let user_has_prompted = self
            .workspace_events
            .get(&ws.id)
            .is_some_and(|e| e.first_user_text.is_some());
        let last_log_activity = self
            .workspace_events
            .get(&ws.id)
            .map(|e| e.last_log_activity_ms)
            .unwrap_or(0);
        let reported = fresh_reported_state(self.pushed_status.get(&ws.id), last_log_activity);
        crate::ui::dashboard::status::Status::classify(
            awaiting,
            stopped_kind,
            stalled,
            secs,
            running,
            user_has_prompted,
            has_prior,
            reported,
        )
    }

    /// The freshness-gated agent-pushed status for a workspace, or `None` when
    /// there is no fresh push. Same liveness rule as the status classifier, so
    /// the message and the glyph appear/disappear together.
    pub fn fresh_reported_status(
        &self,
        ws_id: crate::data::store::WorkspaceId,
    ) -> Option<&crate::data::store::ReportedStatus> {
        let last_log_activity = self
            .workspace_events
            .get(&ws_id)
            .map(|e| e.last_log_activity_ms)
            .unwrap_or(0);
        fresh_reported(self.pushed_status.get(&ws_id), last_log_activity)
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
    last_workspace: &mut Option<crate::data::store::WorkspaceId>,
    current: Option<crate::data::store::WorkspaceId>,
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
pub(crate) fn derive_stopped_kind(
    e: &crate::activity::events::WorkspaceEvents,
) -> Option<StoppedKind> {
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

/// Decide whether a pushed status is still authoritative, returning the full
/// record. For snapshot states the push wins while no JSONL activity has
/// happened strictly after it; once the log grows past `reported_at`, the agent
/// has acted since reporting and the heuristic re-arms. `last_log_activity_ms`
/// of 0 means "no log activity observed", which never contradicts a push.
///
/// `Busy` is the exception: it is exempt from the gate entirely (it predicts
/// future log growth from background work rather than snapshotting the present)
/// and stays authoritative until the next hook push supersedes it. See the
/// in-body comment for the rationale.
pub(crate) fn fresh_reported(
    reported: Option<&crate::data::store::ReportedStatus>,
    last_log_activity_ms: i64,
) -> Option<&crate::data::store::ReportedStatus> {
    use crate::data::store::ReportedState;
    let r = reported?;
    // `Busy` is exempt from the freshness gate: it explicitly means background
    // work (subagents / shell tasks) is in flight, which legitimately grows the
    // main transcript — a completing subagent writes its result notification
    // back into the session log — *without* the agent being done. Gating it on
    // log growth (as every snapshot state correctly is) would drop the push the
    // instant a sibling subagent finishes, flipping the workspace to ✓ complete
    // mid-work. It is superseded by the next hook push (a `Stop` reporting Done
    // once `background_tasks` empties, or a `UserPromptSubmit` reporting
    // Working), and `classify` falls back to the heuristic if the session dies.
    if r.state == ReportedState::Busy {
        return Some(r);
    }
    if r.reported_at >= last_log_activity_ms {
        Some(r)
    } else {
        None
    }
}

/// The freshness-gated reported *state* (convenience over `fresh_reported`).
pub(crate) fn fresh_reported_state(
    reported: Option<&crate::data::store::ReportedStatus>,
    last_log_activity_ms: i64,
) -> Option<crate::data::store::ReportedState> {
    fresh_reported(reported, last_log_activity_ms).map(|r| r.state)
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

    let result = crate::commands::external::edit_in_editor(&current, ext);

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
                let now_ms = crate::time::now_ms_u64();
                if matches!(g.dashboard.reply_draft_clear_at_ms, Some(t) if now_ms >= t) {
                    g.dashboard.reply_draft.clear();
                    g.dashboard.reply_draft_clear_at_ms = None;
                }
                // Apply a settled terminal resize to backgrounded sessions so
                // re-attaching doesn't show a vt100 frame clipped to the old
                // size. Visible panes are sized by the render path above.
                if let Some((cols, rows)) = g.resize_debounce.take_due(now_ms) {
                    g.apply_backgrounded_resize(cols, rows);
                }
                // Pick up workspaces/repos written by sibling `wsx` CLI
                // processes (e.g. `wsx workspace create` invoked by Claude
                // during a related-repos flow). Cheap: PRAGMA data_version
                // is in-process and only triggers refresh on external commits.
                // Only scan the inbox when a sibling commit was detected
                // (e.g. a `wsx agent send`), avoiding a per-frame DB query.
                if g.poll_external_changes() {
                    g.drain_agent_messages();
                }
                let now_secs = crate::time::now_secs();
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
                        while g.activity_history.len() > MAX_ACTIVITY_HOURS as usize {
                            g.activity_history.pop_front();
                        }
                        let _ = g.store.prune_activity_buckets_before(
                            now_hour.saturating_sub(MAX_ACTIVITY_HOURS * 3600),
                        );
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
    let procs = crate::activity::proc::scan().await;
    let worktrees: Vec<(crate::data::store::WorkspaceId, std::path::PathBuf)> = app
        .workspaces
        .iter()
        .map(|(_, w)| (w.id, w.worktree_path.clone()))
        .collect();
    let worktree_refs: Vec<(crate::data::store::WorkspaceId, &std::path::Path)> = worktrees
        .iter()
        .map(|(id, path)| (*id, path.as_path()))
        .collect();
    app.workspace_processes = crate::activity::proc::bucket_by_worktree(&procs, &worktree_refs);
    app.last_proc_scan_ms = crate::time::now_ms();
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
    repo_id: crate::data::store::RepoId,
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
                match serde_json::from_str::<crate::config::detail_bar_config::DetailBarOverride>(
                    trimmed,
                ) {
                    Ok(_) => app.store.set_repo_detail_bar_config(repo_id, Some(trimmed)),
                    Err(e) => Err(crate::error::Error::UserInput(format!(
                        "detail_bar_config is not valid JSON: {e}"
                    ))),
                }
            }
        }
    }
}

/// Resolve the primary agent instance id for a workspace, defensively seeding
/// a primary instance row for any (pre-migration / freshly created) workspace
/// that somehow lacks one. Used by the spawn paths to key sessions.
pub(crate) fn resolve_primary_instance(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<crate::data::store::AgentInstanceId> {
    match app.store.primary_instance_id(ws_id)? {
        Some(i) => Ok(i),
        None => {
            let (_, ws) = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == ws_id)
                .ok_or_else(|| crate::error::Error::Store(rusqlite::Error::QueryReturnedNoRows))?;
            Ok(app
                .store
                .add_primary_agent(ws_id, ws.agent, ws.created_at)?
                .id)
        }
    }
}

/// Shared spawn context for a workspace: the bits common to spawning the
/// primary agent or any added instance. Keeping this in one place avoids
/// duplicating the custom-instructions / related-repo / doctrine /
/// additional-dirs computation between `build_spawn_info` and
/// `build_added_spawn_info`.
struct SpawnContext {
    repo_path: std::path::PathBuf,
    worktree: std::path::PathBuf,
    /// Repo custom instructions merged with the related-repo read-only prompt.
    custom: Option<String>,
    additional_dirs: Vec<std::path::PathBuf>,
    yolo: bool,
}

fn resolve_spawn_context(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
) -> Option<SpawnContext> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let custom = crate::data::repo::resolve_custom_instructions(repo, &app.store)
        .ok()
        .flatten();
    // Resolve related repos (per-repo names → source paths), filter out
    // the spawning repo itself, build the read-only system-prompt
    // fragment, and fold it into custom_instructions before the agent sees it.
    let resolved = crate::agent::related::resolve(repo.related_repos.as_deref(), &app.repos);
    let resolved: Vec<(String, std::path::PathBuf)> = resolved
        .into_iter()
        .filter(|(_, p)| p != &repo.path)
        .collect();
    let additional_dirs: Vec<std::path::PathBuf> =
        resolved.iter().map(|(_, p)| p.clone()).collect();
    let related_prompt = crate::agent::related::build_read_only_prompt(&resolved);
    let custom = match (custom, related_prompt) {
        (None, None) => None,
        (Some(c), None) => Some(c),
        (None, Some(r)) => Some(r),
        (Some(c), Some(r)) => Some(format!("{c}\n\n{r}")),
    };
    Some(SpawnContext {
        repo_path: repo.path.clone(),
        worktree: ws.worktree_path.clone(),
        custom,
        additional_dirs,
        yolo: ws.yolo,
    })
}

pub(crate) fn build_spawn_info(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
) -> Option<(
    crate::data::store::WorkspaceId,
    std::path::PathBuf,
    crate::pty::session::SpawnMode,
    std::path::PathBuf,
    crate::pty::session::AgentKind,
)> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let agent = ws.agent;
    let doctrine = crate::agent::doctrine::resolve_effective_doctrine(&app.store, agent);
    let ctx = resolve_spawn_context(app, ws_id)?;
    let SpawnContext {
        custom,
        additional_dirs,
        yolo,
        worktree,
        repo_path,
        ..
    } = ctx;
    let mode = if crate::pty::session::has_prior_session_for(&worktree, agent) {
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
            doctrine: doctrine.clone(),
            additional_dirs,
            yolo,
        }
    } else {
        let rename_ctx = if crate::names::is_generated_slug(&ws.name) {
            let resolved_prefix =
                crate::data::repo::resolve_branch_prefix(repo, &app.store).unwrap_or_default();
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
            doctrine,
            additional_dirs,
            yolo,
        }
    };
    Some((ws_id, worktree, mode, repo_path, agent))
}

/// Build spawn parameters for an *added* (non-primary) instance. Added agents
/// always spawn `Fresh` with an injected handoff note so they re-orient from
/// the shared worktree + git diff (no session resume — see Task 8 scope).
/// Returns `(worktree, SpawnMode, repo_path)`.
fn build_added_spawn_info(
    app: &App,
    instance: &crate::data::agents::AgentInstance,
) -> Option<(
    std::path::PathBuf,
    crate::pty::session::SpawnMode,
    std::path::PathBuf,
)> {
    let ws_id = instance.workspace_id;
    let (_, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == ws.repo_id)?;
    let base_ref = repo.base_branch.as_deref().unwrap_or("main");
    // The primary instance's label, for the handoff note's "alongside `X`" line.
    let primary_label = app
        .store
        .workspace_agents(ws_id)
        .ok()
        .and_then(|agents| agents.into_iter().find(|a| a.is_primary).map(|a| a.label()))
        .unwrap_or_else(|| "the primary agent".to_string());
    let note = crate::agent::handoff::context_note(
        instance.agent,
        &crate::agent::handoff::HandoffContext {
            primary_label: &primary_label,
            branch: &ws.branch,
            base_ref,
            workspace_name: &ws.name,
        },
    );
    let ctx = resolve_spawn_context(app, ws_id)?;
    // Put the handoff note LAST so repo/related context precedes it.
    let custom_instructions = match ctx.custom {
        Some(c) => format!("{c}\n\n{note}"),
        None => note,
    };
    let doctrine = crate::agent::doctrine::resolve_effective_doctrine(&app.store, instance.agent);
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: Some(custom_instructions),
        doctrine,
        additional_dirs: ctx.additional_dirs,
        yolo: ctx.yolo,
    };
    Some((ctx.worktree, mode, ctx.repo_path))
}

pub(crate) fn save_layout_for(app: &mut App, state: crate::ui::AttachedState) {
    let Some(anchor) = state.leaves().first().map(|t| t.workspace_id) else {
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
/// single-pane view if no layout is saved or all panes were pruned. Returns
/// `None` only if the anchor has no resolvable primary instance (unreachable
/// in normal use — all callers guard on `primary_instance(...).is_some()`).
pub(crate) fn restore_attached_state(
    app: &mut App,
    anchor: crate::data::store::WorkspaceId,
) -> Option<crate::ui::AttachedState> {
    // Fallback single-pane target: the anchor workspace's primary instance.
    // Matches pre-multi-agent behavior — a single-agent workspace's leaf is
    // its primary instance.
    let single = |app: &App| {
        app.primary_instance(anchor).map(|instance| {
            crate::ui::AttachedState::single(crate::ui::split::AttachTarget {
                workspace_id: anchor,
                instance,
            })
        })
    };
    let Some((mut tree, mut focus)) = app.store.get_workspace_layout(anchor).ok().flatten() else {
        return single(app);
    };
    let valid_ws: std::collections::HashSet<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    use crate::ui::split::PruneOutcome;
    // A leaf is stale if its workspace no longer exists OR its agent instance
    // no longer exists in the store.
    let outcome = tree.prune(&|t| {
        valid_ws.contains(&t.workspace_id)
            && app
                .store
                .workspace_agents_by_id(t.instance)
                .ok()
                .flatten()
                .is_some()
    });
    match outcome {
        PruneOutcome::Empty => {
            let _ = app.store.delete_workspace_layout(anchor);
            let _ = app.refresh();
            single(app)
        }
        PruneOutcome::Kept => {
            if tree.leaf_at(&focus).is_none() {
                focus = tree.first_leaf_path();
            }
            // Spawn any missing sessions for the side panes. The focused
            // anchor instance was already spawned by the caller. Skip on
            // failure and continue with remaining panes — partial restore is
            // better than no restore.
            for leaf in tree.leaves() {
                if app.sessions.get(leaf.instance).is_some() {
                    continue;
                }
                let _ = ensure_instance_session(app, leaf.instance, true);
            }
            Some(crate::ui::AttachedState { tree, focus })
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
    ws_id: crate::data::store::WorkspaceId,
) -> Result<AttachReady> {
    if app
        .primary_instance(ws_id)
        .and_then(|i| app.sessions.get(i))
        .is_some()
    {
        return Ok(AttachReady::Ok);
    }
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, ws_id) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::agent::remote_control::RemoteOpts::from_store(&app.store);
        // Resolve the primary agent instance for this workspace, defensively
        // seeding one for any row that somehow lacks a primary instance.
        let inst = resolve_primary_instance(app, id)?;
        match app
            .sessions
            .spawn(inst, id, &path, 80, 24, mode, remote, agent, None)
        {
            Ok(_) => {}
            Err(crate::error::Error::AgentBinaryMissing(binary)) => {
                app.modal = Some(crate::ui::modal::Modal::AgentMissing {
                    ws_id,
                    agent,
                    binary,
                });
                return Ok(AttachReady::AgentMissing);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(AttachReady::Ok)
}

/// Ensure a specific agent *instance* has a live PTY session, spawning one in
/// place if missing. Primary instances delegate to `ensure_workspace_session`
/// so the primary path is never duplicated. Added (non-primary) instances
/// spawn `Fresh` with an injected handoff note (see `build_added_spawn_info`).
/// Mirrors `ensure_workspace_session`'s return/error conventions, including the
/// `AgentMissing` modal for a missing agent binary.
///
/// `surface_missing` controls whether a missing-binary error raises
/// `Modal::AgentMissing`. Pass `true` for interactive callers (keyboard
/// handlers, `switch_focused_pane_to`, `restore_attached_state`) so the user
/// sees the modal. Pass `false` for background callers (e.g. the message drain)
/// so a missing binary doesn't pop a modal over the user's unrelated view.
pub(crate) fn ensure_instance_session(
    app: &mut App,
    inst: crate::data::store::AgentInstanceId,
    surface_missing: bool,
) -> Result<AttachReady> {
    if app.sessions.get(inst).is_some() {
        return Ok(AttachReady::Ok);
    }
    // Unknown instance id: treat as a no-op (matches `build_spawn_info`
    // returning `None` for a workspace whose setup hasn't completed).
    let Some(instance) = app.store.workspace_agents_by_id(inst)? else {
        return Ok(AttachReady::Ok);
    };
    if instance.is_primary {
        return ensure_workspace_session(app, instance.workspace_id);
    }
    let ws_id = instance.workspace_id;
    if let Some((path, mode, repo_path)) = build_added_spawn_info(app, &instance) {
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::agent::remote_control::RemoteOpts::from_store(&app.store);
        match app.sessions.spawn(
            inst,
            ws_id,
            &path,
            80,
            24,
            mode,
            remote,
            instance.agent,
            None,
        ) {
            Ok(_) => {}
            Err(crate::error::Error::AgentBinaryMissing(binary)) => {
                if surface_missing {
                    app.modal = Some(crate::ui::modal::Modal::AgentMissing {
                        ws_id,
                        agent: instance.agent,
                        binary,
                    });
                }
                return Ok(AttachReady::AgentMissing);
            }
            Err(e) => return Err(e),
        }
    }
    Ok(AttachReady::Ok)
}

/// Attach to a workspace: ensure a session, restore layout, and switch
/// to attached view. Shared by the `Enter` / `i` / `l` key handlers.
pub(crate) fn attach_workspace(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<()> {
    app.workspace_needs_attention.remove(&ws_id);
    match ensure_workspace_session(app, ws_id)? {
        AttachReady::Ok => {}
        AttachReady::AgentMissing => return Ok(()),
    }
    if app
        .primary_instance(ws_id)
        .and_then(|i| app.sessions.get(i))
        .is_some()
    {
        if let Some(restored) = restore_attached_state(app, ws_id) {
            app.view = View::Attached(restored);
        }
    }
    Ok(())
}

/// Best-effort MCP server mirror. Logs and continues on any failure.
pub(crate) fn maybe_mirror_mcp(
    app: &App,
    repo_path: &std::path::Path,
    worktree_path: &std::path::Path,
) {
    if !crate::agent::mcp::enabled(&app.store) {
        return;
    }
    if let Err(e) = crate::agent::mcp::mirror_mcp_servers(repo_path, worktree_path) {
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
    result: Result<crate::data::workspace::CreatedWorkspace>,
) {
    let mut g = app.lock().await;
    let is_mine = g.pending_create_gen == Some(my_gen);
    if is_mine {
        g.pending_create_gen = None;
    }
    let new_ws = result
        .as_ref()
        .ok()
        .map(|c| (c.workspace.id, c.workspace.repo_id));
    match result {
        Ok(_) => {
            if is_mine && matches!(g.modal, Some(crate::ui::modal::Modal::SetupRunning { .. })) {
                g.modal = None;
            }
            let _ = g.refresh();
            // Select the newly created workspace so the dashboard lands on it.
            if let Some((id, repo_id)) = new_ws {
                // Unfold the owning repo first. If it was collapsed (explicit
                // fold or `default_fold` of an idle/empty repo), the new
                // workspace would be hidden from `visible_targets` on the next
                // draw, so the selection below would land on an invisible row
                // and get parked — no highlight, and the nav cursor clamped
                // onto an unrelated neighbor. Expanding makes the row visible
                // so the selection sticks.
                g.dashboard.folded.insert(repo_id.0 as u64, false);
                if let Some(idx) = g
                    .selectable
                    .iter()
                    .position(|t| *t == SelectionTarget::Workspace(id))
                {
                    g.select_index(idx);
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
    result: Result<crate::data::setup::SetupResult>,
) {
    let mut g = app.lock().await;
    let is_mine = g.pending_archive_gen == Some(my_gen);
    if is_mine {
        g.pending_archive_gen = None;
    }
    match result {
        Ok(_) => {
            if is_mine
                && matches!(
                    g.modal,
                    Some(crate::ui::modal::Modal::ArchiveRunning { .. })
                )
            {
                g.modal = None;
            }
            let _ = g.refresh();
        }
        Err(e) => {
            if is_mine
                && matches!(
                    g.modal,
                    Some(crate::ui::modal::Modal::ArchiveRunning { .. })
                )
            {
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
    use crate::data::setup::SetupResult;
    use crate::error::Error;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    fn make_app() -> (App, TempDir) {
        let store = crate::data::store::Store::open_in_memory().unwrap();
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
        assert!(
            g.modal.is_none(),
            "modal should clear on Ok; got {:?}",
            g.modal
        );
        assert!(
            g.pending_archive_gen.is_none(),
            "pending_archive_gen should clear after matching reconcile"
        );
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
        reconcile_archive_result(shared.clone(), 7, Err(Error::Setup("boom".into()))).await;
        let g = shared.lock().await;
        match &g.modal {
            Some(crate::ui::modal::Modal::Error { message }) => {
                assert!(
                    message.contains("boom"),
                    "error message should contain failure detail; got {message:?}"
                );
            }
            other => panic!("expected Modal::Error, got {other:?}"),
        }
        assert!(
            g.pending_archive_gen.is_none(),
            "pending_archive_gen should clear after matching reconcile"
        );
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
                assert_eq!(
                    message, "untouched",
                    "stale reconcile must not overwrite modal"
                );
            }
            other => panic!("expected the pre-existing Error modal to survive, got {other:?}"),
        }
        assert_eq!(
            g.pending_archive_gen,
            Some(99),
            "stale reconcile must not clear pending_archive_gen"
        );
    }
}

#[cfg(test)]
mod added_spawn_tests {
    use super::*;
    use crate::data::store::NewWorkspace;
    use crate::pty::session::{AgentKind, SpawnMode};
    use tempfile::TempDir;

    #[test]
    fn build_added_spawn_info_is_fresh_with_handoff_note() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "feat",
                branch: "wsx/feat",
                worktree_path: std::path::Path::new("/tmp/r/feat"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        let added = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();

        let tmp = TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
        app.refresh().unwrap();

        let (_worktree, mode, _repo_path) =
            build_added_spawn_info(&app, &added).expect("spawn info");
        match mode {
            SpawnMode::Fresh {
                rename_ctx,
                custom_instructions,
                ..
            } => {
                assert!(rename_ctx.is_none(), "added agents never rename");
                let note = custom_instructions.expect("handoff note present");
                // References the primary's label, the branch, and the
                // base-ref-driven git diff hint (default "main").
                assert!(note.contains("claude"), "note mentions primary: {note}");
                assert!(note.contains("wsx/feat"), "note mentions branch: {note}");
                assert!(
                    note.contains("git diff main...HEAD"),
                    "note mentions base ref: {note}"
                );
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod derive_stopped_kind_tests {
    use super::*;
    use crate::activity::events::{StopReason, WorkspaceEvents};

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
        use crate::data::store::WorkspaceId;
        let mut offsets = [3u16, 7, 0, 2];
        let mut last = Some(WorkspaceId(100));

        super::reset_detail_scroll_on_workspace_change(
            &mut offsets,
            &mut last,
            Some(WorkspaceId(200)),
        );

        assert_eq!(offsets, [0; 4]);
        assert_eq!(last, Some(WorkspaceId(200)));
    }

    #[test]
    fn reset_detail_scroll_preserves_offsets_on_same_workspace() {
        use crate::data::store::WorkspaceId;
        let mut offsets = [3u16, 7, 0, 2];
        let mut last = Some(WorkspaceId(100));

        super::reset_detail_scroll_on_workspace_change(
            &mut offsets,
            &mut last,
            Some(WorkspaceId(100)),
        );

        assert_eq!(offsets, [3, 7, 0, 2]);
        assert_eq!(last, Some(WorkspaceId(100)));
    }

    #[test]
    fn reset_detail_scroll_handles_initial_none_to_some() {
        use crate::data::store::WorkspaceId;
        // App starts with detail_scroll_last_workspace = None and offsets
        // already zero; first draw with a selected workspace should update
        // the sentinel even though the offsets are technically unchanged.
        let mut offsets = [5u16, 0, 0, 0]; // seeded non-zero
        let mut last: Option<WorkspaceId> = None;

        super::reset_detail_scroll_on_workspace_change(
            &mut offsets,
            &mut last,
            Some(WorkspaceId(42)),
        );

        assert_eq!(offsets, [0; 4]);
        assert_eq!(last, Some(WorkspaceId(42)));
    }
}

#[cfg(test)]
mod reported_freshness_tests {
    use super::{fresh_reported, fresh_reported_state};
    use crate::data::store::{ReportedState, ReportedStatus};

    fn status(at: i64) -> ReportedStatus {
        ReportedStatus {
            state: ReportedState::Done,
            message: None,
            source: "model".into(),
            reported_at: at,
        }
    }

    #[test]
    fn push_newer_than_last_log_activity_is_fresh() {
        assert_eq!(
            fresh_reported_state(Some(&status(1000)), 900),
            Some(ReportedState::Done)
        );
        assert_eq!(
            fresh_reported_state(Some(&status(1000)), 1000),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn jsonl_activity_after_push_re_arms_heuristic() {
        assert_eq!(fresh_reported_state(Some(&status(1000)), 1500), None);
    }

    #[test]
    fn no_push_is_none() {
        assert_eq!(fresh_reported_state(None, 1500), None);
    }

    #[test]
    fn busy_survives_log_growth_from_background_work() {
        // `Busy` means background work (subagents / shell tasks) is in flight.
        // That work legitimately grows the main transcript — a completing
        // subagent writes its result notification back into the session log —
        // *without* the agent being done. So a `Busy` push must NOT be gated
        // out when `last_log_activity_ms` advances past it; otherwise the
        // workspace flips to ✓ complete mid-work in the window before the next
        // `Stop` re-pushes `Busy`. Every other state still re-arms normally.
        let busy = ReportedStatus {
            state: ReportedState::Busy,
            message: None,
            source: "hook".into(),
            reported_at: 1000,
        };
        assert_eq!(
            fresh_reported_state(Some(&busy), 1500),
            Some(ReportedState::Busy),
            "Busy stays authoritative even after the log grows"
        );
        assert!(fresh_reported(Some(&busy), 1500).is_some());
    }

    #[test]
    fn fresh_reported_returns_ref_on_tie_and_none_after() {
        let s = status(1000);
        // tie: reported_at == last_log_activity_ms -> still fresh, returns the ref
        assert!(fresh_reported(Some(&s), 1000).is_some());
        assert!(fresh_reported(Some(&s), 900).is_some());
        // log grew after the push -> stale
        assert!(fresh_reported(Some(&s), 1500).is_none());
        // no push -> none
        assert!(fresh_reported(None, 1500).is_none());
    }
}

#[cfg(test)]
mod selection_helper_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};
    use std::path::PathBuf;

    fn app_with_one_workspace() -> (App, crate::data::store::WorkspaceId) {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let w = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "a",
                branch: "x/a",
                worktree_path: std::path::Path::new("/tmp/r/a"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
        (app, w)
    }

    #[test]
    fn select_index_sets_both_fields() {
        let (mut app, w) = app_with_one_workspace();
        let idx = app
            .selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(w))
            .unwrap();
        app.select_index(idx);
        assert_eq!(app.dashboard.selected, idx);
        assert_eq!(app.dashboard.selection, Some(SelectionTarget::Workspace(w)));
    }

    #[test]
    fn selected_target_returns_durable_selection_not_index() {
        let (mut app, w) = app_with_one_workspace();
        app.dashboard.selection = Some(SelectionTarget::Workspace(w));
        // Deliberately desync the index to a different slot (the repo header).
        app.dashboard.selected = 0;
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(w)),
            "selected_target follows the durable selection, not the index"
        );
    }

    #[test]
    fn selection_target_exists_tracks_workspaces_and_repos() {
        let (app, w) = app_with_one_workspace();
        let repo_id = app.repos[0].id;
        assert!(app.selection_target_exists(SelectionTarget::Workspace(w)));
        assert!(app.selection_target_exists(SelectionTarget::Repo(repo_id)));
        assert!(!app.selection_target_exists(SelectionTarget::Workspace(
            crate::data::store::WorkspaceId(9999)
        )));
    }
}
