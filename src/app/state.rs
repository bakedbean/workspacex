//! [`App`] -- the TUI's whole mutable state, and the methods that build
//! it, refresh it from the store, and report which sessions are live.
//!
//! Selection lives in [`super::selection`] and status classification in
//! [`super::status`]; both add methods to `App` from their own files.

use super::*;

impl App {
    pub fn new(store: Store, worktree_base: PathBuf) -> Result<Self> {
        let theme_name = store
            .get_setting("theme")
            .ok()
            .flatten()
            .unwrap_or_default();
        let theme = crate::ui::theme::Theme::by_name(&theme_name);
        let mut registry = crate::ui::detail_modules::Registry::new();
        crate::ui::detail_modules::register_builtins(&mut registry);
        let mut dashboard = DashboardState::default();
        dashboard.load_ordering_prefs(&store);
        let mut app = Self {
            store,
            sessions: SessionManager::new(),
            resize_debounce: Default::default(),
            view: View::Dashboard,
            modal: None,
            dashboard,
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
            pr_review: std::collections::HashMap::new(),
            pr_unresolved: std::collections::HashMap::new(),
            pr_number: std::collections::HashMap::new(),
            pr_link_rect: None,
            dashboard_pr_rects: Vec::new(),
            dashboard_repo_pr_rects: Vec::new(),
            procs_link_rect: None,
            github_remotes: Default::default(),
            pr_last_poll_ms: std::collections::HashMap::new(),
            diff_last_poll_ms: std::collections::HashMap::new(),
            workspace_events: std::collections::HashMap::new(),
            pushed_status: std::collections::HashMap::new(),
            agent_roster: std::collections::HashMap::new(),
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
            delivering: std::collections::HashMap::new(),
            delivery_outcomes: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            delivery_attempts: std::collections::HashMap::new(),
            stuck_mail: std::collections::HashSet::new(),
            // Due immediately: the first tick recovers mail queued while wsx
            // was not running.
            next_mail_drain_ms: Some(0),
            pending_acks: Vec::new(),
            ack_fails: std::collections::HashMap::new(),
            pending_edit: None,
            theme,
            pm_visible: false,
            focus: crate::ui::PaneFocus::Dashboard,
            recaps: Default::default(),
            pm_digest_selected: 0,
            pm_filter: None,
            next_create_gen: 0,
            pending_create_gen: None,
            in_flight: std::collections::HashMap::new(),
            next_archive_gen: 0,
            pending_archive_gen: None,
            remote_list: None,
            remote: None,
            remote_target: None,
            next_remote_gen: 0,
            pending_remote_gen: None,
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
            name_color_swatch_rects: Vec::new(),
            pinned_commands_cache: Vec::new(),
            pending_bells: Vec::new(),
            startup_workspace_ids: std::collections::HashSet::new(),
            last_data_version: 0,
            registry,
            shared_detached: std::collections::HashSet::new(),
            shared_detached_polled_ms: 0,
        };
        // Sweep stale Pending rows from previous runs.
        let _ = app
            .store
            .sweep_stale_pending(std::time::Duration::from_secs(300));
        // Resolve setup rows stranded by a crashed process (see sweep_stale_running).
        let _ = app.store.sweep_stale_running();
        // Load the retained bucketed activity for the sparkline (up to
        // MAX_ACTIVITY_HOURS); the configured window selects how much is shown.
        if let Ok(buckets) = app
            .store
            .recent_activity_buckets(MAX_ACTIVITY_HOURS as usize)
        {
            app.activity_history.extend(buckets);
        }
        app.refresh()?;
        // Everything present after the initial refresh predates this
        // process — its first activity observation must not ring.
        app.startup_workspace_ids = app.workspaces.iter().map(|(_, w)| w.id).collect();
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
        // A sibling process committed. Memoized settings were read through our
        // own connection and may now be stale, so drop them before refreshing.
        self.store.invalidate_settings_cache();
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
        // Probes only repos it hasn't seen before, so this stays free after
        // the first refresh even though refresh runs on every data change.
        self.github_remotes.sync(&self.repos);
        self.workspaces = Vec::new();
        for r in &self.repos {
            for w in self.store.workspaces(r.id)? {
                self.workspaces.push((r.id, w));
            }
        }
        // `refresh_shared_detached` reads `agent_roster` (both the liveness
        // and the tmux-session-ref checks), so this must be filled before
        // it runs — otherwise the sweep sees a cycle-stale (or, at cold
        // start, empty) roster. `all_workspace_agents` only depends on
        // `self.store`, and nothing between the workspaces rebuild above and
        // here writes to the DB or reads `agent_roster`, so hoisting it this
        // early is safe.
        self.agent_roster = self.store.all_workspace_agents().unwrap_or_default();
        // Needs `self.workspaces` populated above (it iterates shared
        // workspaces) — must run after the rebuild, not before.
        self.refresh_shared_detached();
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
        self.recaps = self.store.all_workspace_recaps().unwrap_or_default();
        Ok(())
    }

    /// Whether `id` currently has a live (`SessionStatus::Running`) session
    /// in `self.sessions`. The shared liveness predicate:
    /// `has_live_instance` and any caller with its own already-fetched
    /// instance list (e.g. `toggle_workspace_shared`, which cannot use
    /// `agent_roster` — see its comment) all filter through this so there is
    /// exactly one definition of "running" in the app.
    pub fn instance_is_running(&self, id: crate::data::store::AgentInstanceId) -> bool {
        self.sessions.get(id).is_some_and(|s| {
            matches!(
                *s.status.read().unwrap(),
                crate::pty::session::SessionStatus::Running { .. }
            )
        })
    }

    /// Whether the instance was spawned in this wsx process and its agent has
    /// since exited. Distinct from "not running": an instance with no session
    /// entry at all was never spawned *this run* — typically because the
    /// previous wsx quit killed every PTY — and its conversation resumes with
    /// `--continue` on the next attach, so it is not "gone" the way an exited
    /// one is.
    pub fn instance_has_exited(&self, id: crate::data::store::AgentInstanceId) -> bool {
        self.sessions.get(id).is_some_and(|s| {
            matches!(
                *s.status.read().unwrap(),
                crate::pty::session::SessionStatus::Exited { .. }
            )
        })
    }

    /// The workspace's agent instances to draw on the dashboard agent strip,
    /// in roster order (primary first). Every registered instance counts
    /// except those whose session exited in this wsx run: a finished
    /// reviewer drops off the strip, but a peer with no session entry —
    /// registered in the DB, killed by a previous wsx quit — keeps its bar,
    /// since sessions never survive a restart and the roster is the only
    /// record that the agent exists.
    ///
    /// Reads the cached `agent_roster`, so it can lag behind the DB by up to
    /// one `refresh()` — fine for the dashboard render path this feeds, but
    /// wrong for a caller that just mutated the roster and needs the result
    /// to reflect that mutation immediately (see `toggle_workspace_shared`,
    /// which filters its own freshly-fetched instance list instead of
    /// calling this).
    pub fn strip_instances(
        &self,
        ws: crate::data::store::WorkspaceId,
    ) -> Vec<crate::data::agents::AgentInstance> {
        self.agent_roster
            .get(&ws)
            .map(|instances| {
                instances
                    .iter()
                    .filter(|inst| !self.instance_has_exited(inst.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the workspace has any instance with a running session — not
    /// just the primary. Same cache-staleness caveat as `strip_instances`
    /// applies. Note the asymmetry with the strip: this is strictly
    /// `Running`, since it feeds liveness checks (the shared badge), while
    /// the strip also keeps never-started-this-run instances.
    pub fn has_live_instance(&self, ws: crate::data::store::WorkspaceId) -> bool {
        self.agent_roster.get(&ws).is_some_and(|instances| {
            instances
                .iter()
                .any(|inst| self.instance_is_running(inst.id))
        })
    }

    /// Sweep for shared workspaces whose tmux session is alive on the
    /// server while wsx holds no client for it (e.g. right after a wsx
    /// restart). Populates `shared_detached`, consumed by the shared-badge
    /// liveness check in `render` — a detached-but-alive session must keep
    /// the badge green even though this wsx holds no client for it.
    /// Throttled to one sweep per 10s — `tmux has-session` is a subprocess,
    /// so this must not run on every tick.
    fn refresh_shared_detached(&mut self) {
        let now = crate::util::time::now_ms_u64();
        if now.saturating_sub(self.shared_detached_polled_ms) < 10_000 {
            return;
        }
        self.shared_detached_polled_ms = now;
        self.shared_detached.clear();
        for (_, ws) in &self.workspaces {
            if !ws.shared {
                continue;
            }
            // One `agent_roster` lookup serves both checks (`refresh` fills
            // it before calling this method, so it's current for this
            // cycle — see the comment in `refresh`). A client on ANY
            // instance (not just the primary) means the workspace isn't
            // detached — e.g. only a side-pane codex#2 is attached while the
            // primary exited.
            if self.has_live_instance(ws.id) {
                continue;
            }
            let alive = self
                .agent_roster
                .get(&ws.id)
                .into_iter()
                .flatten()
                .filter_map(|i| i.session_ref.as_deref())
                .any(crate::pty::tmux::has_session);
            if alive {
                self.shared_detached.insert(ws.id);
            }
        }
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

    /// Allocate a fresh generation id for a new remote-list fetch task.
    pub fn alloc_remote_gen(&mut self) -> u64 {
        let g = self.next_remote_gen;
        self.next_remote_gen = self.next_remote_gen.wrapping_add(1);
        self.pending_remote_gen = Some(g);
        g
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
    pub fn apply_backgrounded_resize(&self, cols: u16, rows: u16) {
        let (w, h) = crate::app::resize_sync::projected_pane_size(cols, rows);
        let visible = crate::app::resize_sync::visible_instances(&self.view);
        self.sessions.resize_backgrounded(w, h, &visible);
    }
}

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
    /// Background create/archive work, keyed by the workspace it targets.
    /// Sole source of truth for the dashboard's in-flight badges. An entry is
    /// inserted when the task is spawned and removed by the reconciler on
    /// every exit path — success, failure, and cancellation alike.
    pub in_flight: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::data::in_flight::InFlight,
    >,
    /// Monotonic counter handed out to in-flight workspace archive tasks.
    pub next_archive_gen: u64,
    /// Generation id of the currently in-flight workspace archive, if any.
    /// Used by the reconcile step to detect stale completions.
    pub pending_archive_gen: Option<u64>,
    /// Most recently fetched remote (tmux-shared) workspace listing, if any
    /// fetch has completed. Consumed by `Modal::RemoteWorkspaceList`
    /// rendering.
    pub remote_list: Option<RemoteList>,
    /// The live ssh-attach session while in `View::AttachedRemote`, else None.
    /// The child is a local `ssh -t … tmux attach` client; its `tmux_session`
    /// is deliberately None so `kill()`/`Drop` sever only that client, never
    /// the remote agent (the Phase 1 persistence contract, one ssh hop away).
    pub remote: Option<std::sync::Arc<crate::pty::session::Session>>,
    /// The target backing `app.remote`, kept for the `AttachedRemote` header
    /// label and the "session ended" error message. Cleared alongside `remote`.
    pub remote_target: Option<RemoteTarget>,
    /// Monotonic counter handed out to in-flight remote-list fetch tasks.
    pub next_remote_gen: u64,
    /// Generation id of the currently in-flight remote-list fetch, if any.
    /// Used by the reconcile step to detect stale completions.
    pub pending_remote_gen: Option<u64>,
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
    /// Cached PR review verdict per workspace, populated alongside
    /// `pr_lifecycle`. Absent key = no approval gate, never polled, or a
    /// verdict `gh` didn't report — all three render as no approval mark.
    /// Unlike `pr_lifecycle` this key is *removed* when a poll reports no
    /// verdict, so a dismissed approval can't leave a stale tick behind.
    pub pr_review: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::git::forge::ReviewDecision,
    >,
    /// Cached unresolved review-thread count per workspace, populated
    /// alongside `pr_review` and removed on the same rule: a poll that
    /// reports no count (probe failed, no verdict to hang it on) clears the
    /// key rather than leaving a stale number behind.
    pub pr_unresolved: std::collections::HashMap<crate::data::store::WorkspaceId, u32>,
    /// Screen rect of the clickable PR chip in the detail-bar header, with
    /// the workspace it belongs to. Set during draw, read by the mouse
    /// handler. Mirrors the `chip_rects` draw-populates / input-reads pattern.
    pub pr_link_rect: Option<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
    /// Screen rects of the clickable PR chips in the dashboard list's PR
    /// column, one per visible row with a chip. Set during draw, read by the
    /// mouse handler to open that row's PR in the browser — same action as
    /// the detail-bar chip (`pr_link_rect`), just reachable per row.
    pub dashboard_pr_rects: Vec<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
    /// Screen rects of the clickable PR links on the dashboard's by-repo
    /// headers, one per visible header that has one. Set during draw, read
    /// by the mouse handler to open that repo's open PRs in the browser.
    pub dashboard_repo_pr_rects: Vec<(crate::data::store::RepoId, ratatui::layout::Rect)>,
    /// Screen rect of the clickable running-process count (`● Np`) on the
    /// attached view's chip row, with the workspace it belongs to. Set during
    /// draw, read by the mouse handler to open the process-list modal on click.
    /// Mirrors the `pr_link_rect` draw-populates / input-reads pattern.
    pub procs_link_rect: Option<(crate::data::store::WorkspaceId, ratatui::layout::Rect)>,
    /// Which repos live on github.com, probed once each in `refresh`. Gates
    /// the per-repo PR affordance on the by-repo dashboard headers.
    pub github_remotes: crate::git::github_remotes::GithubRemotes,
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
    /// Every workspace's agent instances, refilled by `refresh`. Cached so
    /// the per-frame dashboard build can resolve a workspace's agents
    /// without a SQLite round-trip per row. Liveness is NOT cached — it
    /// comes from `sessions`, which is already in memory.
    pub agent_roster: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        Vec<crate::data::agents::AgentInstance>,
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
    pub pm_visible: bool,
    pub focus: crate::ui::PaneFocus,
    /// Recaps for every workspace, loaded from the store each `refresh()`.
    /// Feeds `build_pm_digest`'s per-card summary text.
    pub recaps: std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::data::store::WorkspaceRecap,
    >,
    /// Index of the selected card in the flattened PM digest (`card_at`
    /// order), clamped on render against the current card count.
    pub pm_digest_selected: usize,
    /// Live PM digest filter buffer. `None` = inactive; `Some(buf)` = filter
    /// mode, matched case-insensitively against workspace names.
    pub pm_filter: Option<String>,
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
    /// forward wheel events to a mouse-aware agent. Cleared each frame.
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
    /// `(palette index, rect)` per swatch drawn by the open name-color picker,
    /// consumed by `handle_mouse` to apply a clicked color. Cleared each frame;
    /// only populated while that modal is open.
    pub name_color_swatch_rects: Vec<(u8, ratatui::layout::Rect)>,
    /// Resolved pinned commands from the last draw tick (matches `chip_rects`).
    pub pinned_commands_cache: Vec<crate::commands::pinned::PinnedCommand>,
    /// Bells queued up by the most recent draw tick. Drained and fired
    /// AFTER `terminal.draw()` returns to avoid interleaving `\x07` writes
    /// with ratatui's escape sequences.
    pub pending_bells: Vec<ActivityState>,
    /// Workspaces that already existed when wsx started. Their first
    /// activity observation is cold-start catch-up, not news — the bell
    /// loop suppresses the ring (visual marker only). Keyed on identity
    /// rather than a time window because the first JSONL scan of each
    /// workspace is queued behind the sequential 2s poll loop and can
    /// land arbitrarily late when many workspaces exist. Never pruned:
    /// membership only matters for a workspace's first observation.
    pub startup_workspace_ids: std::collections::HashSet<crate::data::store::WorkspaceId>,
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
    pub registry: crate::ui::detail_modules::Registry,
    /// Workspaces whose shared tmux session is alive on the server while wsx
    /// holds no client for it (e.g. right after a wsx restart). Refreshed by
    /// `refresh_shared_detached`, throttled — `tmux has-session` is a subprocess.
    pub shared_detached: std::collections::HashSet<crate::data::store::WorkspaceId>,
    /// Epoch-ms of the last `refresh_shared_detached` sweep (throttle key).
    pub shared_detached_polled_ms: u64,
    /// Inbox message ids with an injection task currently in flight, mapped to
    /// the agent they are being injected into. An injection waits for the
    /// target agent to be ready, which can take many seconds; without this the
    /// retry drain would dispatch the same row again on every tick and the
    /// agent would receive it several times. The target half also gates a
    /// second worker against the same session — see `target_in_flight`.
    pub(crate) delivering: std::collections::HashMap<i64, crate::data::store::AgentInstanceId>,
    /// Outcomes reported back by those detached injection tasks. Written from
    /// the tasks (hence the `Arc<Mutex<_>>`; `Store` holds a bare
    /// `rusqlite::Connection` and can't cross the spawn), applied on the next
    /// tick by `apply_delivery_outcomes` — the only place that touches the DB.
    pub(crate) delivery_outcomes:
        std::sync::Arc<std::sync::Mutex<Vec<crate::app::messaging::DeliveryOutcome>>>,
    /// Failed injection attempts per inbox message id. Cleared on success;
    /// at `MAX_DELIVERY_ATTEMPTS` the message stops being retried. In memory
    /// rather than on the row, so a wsx restart — which also restarts the
    /// agents — gets a fresh set of attempts.
    pub(crate) delivery_attempts: std::collections::HashMap<i64, u32>,
    /// Workspaces with a queued message wsx has given up injecting. Drives the
    /// row's `✉!` badge, so a message that can't be delivered is visible on the
    /// dashboard instead of only in the log file.
    pub(crate) stuck_mail: std::collections::HashSet<crate::data::store::WorkspaceId>,
    /// When the inbox should next be drained, epoch-ms. `Some(0)` at startup so
    /// mail queued while wsx was down is picked up on the first tick — nothing
    /// else would, since `App::new` snapshots `last_data_version` and so
    /// `poll_external_changes` sees no edge. Re-armed whenever a drain leaves a
    /// message waiting on something no DB commit or outcome will announce (an
    /// agent that failed to spawn, a session that wasn't there).
    pub(crate) next_mail_drain_ms: Option<u64>,
    /// Acks whose `mark_delivered` failed, parked until their backoff expires.
    pub(crate) pending_acks: Vec<crate::app::messaging::PendingAck>,
    /// Consecutive ack failures per message id, carried across the park so the
    /// backoff keeps doubling.
    pub(crate) ack_fails: std::collections::HashMap<i64, u32>,
}

#[cfg(test)]
mod external_change_tests {
    use super::*;

    /// End-to-end guard for the settings-cache wiring. `data::settings` tests
    /// invalidate by hand; this proves the TUI actually calls it when a sibling
    /// process commits, which is the only thing that makes a memoized setting
    /// safe to serve from the render path.
    #[test]
    fn poll_external_changes_invalidates_the_settings_cache() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.db");
        let store = crate::data::store::Store::open(&path).unwrap();
        let mut app = App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();

        // Warm the cache through the app's own connection.
        app.store.set_setting("theme", "dark").unwrap();
        assert_eq!(
            app.store.get_setting("theme").unwrap().as_deref(),
            Some("dark")
        );

        // A sibling `wsx` process writes through a different connection.
        let sibling = crate::data::store::Store::open(&path).unwrap();
        sibling.set_setting("theme", "light").unwrap();

        assert!(
            app.poll_external_changes(),
            "a sibling commit must be detected via data_version"
        );
        assert_eq!(
            app.store.get_setting("theme").unwrap().as_deref(),
            Some("light"),
            "detecting the commit must also drop the stale memo"
        );
    }
}

#[cfg(test)]
mod strip_instances_tests {
    use super::*;
    use crate::data::store::NewWorkspace;
    use crate::pty::session::{AgentKind, SessionStatus};

    fn test_app() -> App {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap()
    }

    impl App {
        /// Insert a fresh repo + workspace for this test, refresh the app so
        /// it's immediately reflected, and return the new workspace id. Each
        /// call needs a distinct `name` — `workspaces.worktree_path` is
        /// UNIQUE, and `name` seeds the fixture path.
        ///
        /// `pub(crate)` (not just private to this module) so other `cfg(test)`
        /// modules in the crate — e.g. `app::render`'s row-building tests —
        /// can reuse it instead of duplicating the fixture setup.
        pub(crate) fn test_workspace(&mut self, name: &str) -> crate::data::store::WorkspaceId {
            let repo = self
                .store
                .add_repo(
                    std::path::Path::new(&format!("/tmp/{name}-repo")),
                    name,
                    "wsx",
                )
                .unwrap();
            let ws = self
                .store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name,
                    branch: &format!("wsx/{name}"),
                    worktree_path: &std::path::PathBuf::from(format!("/tmp/{name}-repo/{name}")),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            self.refresh().unwrap();
            ws
        }

        /// Register a fake session for `id` with a directly-chosen `status`,
        /// via `SessionManager::insert_fake_session` — bypasses the real
        /// spawn path (no process, no agent binary, no Tokio runtime needed)
        /// so liveness-filtering tests can pick any status synchronously.
        ///
        /// `pub(crate)`, see `test_workspace` above for why.
        pub(crate) fn test_spawn_session(
            &mut self,
            id: crate::data::store::AgentInstanceId,
            status: SessionStatus,
        ) {
            self.sessions.insert_fake_session(id, status);
        }
    }

    #[test]
    fn strip_instances_excludes_exited_but_keeps_never_started_peers() {
        let mut app = test_app();
        let ws = app.test_workspace("multi");
        let primary = app
            .store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap();
        let peer_running = app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        let peer_exited = app.store.add_workspace_agent(ws, AgentKind::Pi).unwrap();
        // `peer_never_started` gets no session entry at all — the state every
        // registered peer is in right after a wsx restart.
        let peer_never_started = app
            .store
            .add_workspace_agent(ws, AgentKind::Hermes)
            .unwrap();
        app.refresh().unwrap();

        app.test_spawn_session(primary.id, SessionStatus::Running { pid: 1 });
        app.test_spawn_session(peer_running.id, SessionStatus::Running { pid: 2 });
        app.test_spawn_session(peer_exited.id, SessionStatus::Exited { code: 0 });

        let strip: Vec<_> = app.strip_instances(ws).into_iter().map(|i| i.id).collect();
        assert_eq!(
            strip,
            vec![primary.id, peer_running.id, peer_never_started.id]
        );
    }

    #[test]
    fn strip_instances_is_empty_for_unknown_workspace() {
        let app = test_app();
        assert!(
            app.strip_instances(crate::data::store::WorkspaceId(9999))
                .is_empty()
        );
    }

    #[test]
    fn refresh_repopulates_the_agent_roster() {
        let mut app = test_app();
        let ws = app.test_workspace("rostered");
        app.store
            .add_primary_agent(ws, AgentKind::Claude, 1)
            .unwrap();
        app.refresh().unwrap();
        assert_eq!(app.agent_roster.get(&ws).map(|v| v.len()), Some(1));

        app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        // Stale until refresh — this is the cache contract every mutation
        // path has to respect.
        assert_eq!(app.agent_roster.get(&ws).map(|v| v.len()), Some(1));
        app.refresh().unwrap();
        assert_eq!(app.agent_roster.get(&ws).map(|v| v.len()), Some(2));
    }

    #[test]
    fn attach_refuses_a_workspace_being_archived() {
        let mut app = test_app();
        let ws = app.test_workspace("doomed");
        app.in_flight.insert(
            ws,
            crate::data::in_flight::InFlight::archive(
                crate::data::progress::SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        // Archive kills the workspace's tmux sessions first, precisely so a live
        // agent cannot dirty the worktree during teardown. Attaching would
        // respawn one into a directory that is being deleted.
        attach_workspace(&mut app, ws).unwrap();
        assert!(
            !matches!(app.view, View::Attached(_)),
            "attach must be refused while an archive is in flight"
        );
    }

    #[test]
    fn attach_allows_a_workspace_that_is_only_provisioning() {
        let mut app = test_app();
        let ws = app.test_workspace("fresh");
        app.in_flight.insert(
            ws,
            crate::data::in_flight::InFlight::create(
                crate::data::progress::SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        // Dropping in while setup runs is the entire point of this feature.
        // Only archive is unsafe. (`assert!(!x)`, not `assert!(x == false)` —
        // clippy's `bool_assert_comparison` is denied by CI.)
        assert!(
            !crate::app::session::attach_is_blocked(&app, ws),
            "provisioning must not block attach"
        );
    }

    /// F4 regression: `ensure_workspace_session` itself must refuse while an
    /// archive is in flight, not merely `attach_workspace`'s wrapper. This is
    /// what closes the bypass routes (chip click / chord / reply Enter,
    /// `restore_attached_state`'s side-pane spawns, `switch_focused_pane_to`)
    /// that called `ensure_workspace_session`/`ensure_instance_session`
    /// directly without ever consulting `attach_is_blocked`.
    #[test]
    fn ensure_workspace_session_refuses_when_archive_in_flight() {
        let mut app = test_app();
        let ws = app.test_workspace("archiving");
        app.in_flight.insert(
            ws,
            crate::data::in_flight::InFlight::archive(
                crate::data::progress::SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        let outcome = crate::app::ensure_workspace_session(&mut app, ws).unwrap();
        assert_eq!(outcome, crate::app::AttachReady::Refused);
        assert!(
            app.primary_instance(ws)
                .and_then(|i| app.sessions.get(i))
                .is_none(),
            "must not spawn a session while an archive is tearing the workspace down"
        );
    }

    /// F4 regression, non-primary half: `ensure_instance_session` delegates
    /// to `ensure_workspace_session` for primary instances (covered above),
    /// but a non-primary (added) instance never reaches that delegation, so
    /// it needs its own `attach_is_blocked` check — this is exactly the
    /// "make sure non-primary instances are guarded too" requirement.
    #[test]
    fn ensure_instance_session_refuses_non_primary_when_archive_in_flight() {
        let mut app = test_app();
        let ws = app.test_workspace("archiving-peer");
        let peer = app.store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        app.in_flight.insert(
            ws,
            crate::data::in_flight::InFlight::archive(
                crate::data::progress::SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        let outcome = crate::app::ensure_instance_session(&mut app, peer.id, false).unwrap();
        assert_eq!(outcome, crate::app::AttachReady::Refused);
        assert!(
            app.sessions.get(peer.id).is_none(),
            "must not spawn a peer session while an archive is tearing the workspace down"
        );
    }

    /// A primary instance routed through `ensure_instance_session` must be
    /// refused too — via delegation to `ensure_workspace_session` — and must
    /// NOT be double-guarded (i.e. this must not panic/loop and must return
    /// the same single `Refused`, proving the delegation path is taken
    /// rather than a duplicate local check).
    #[test]
    fn ensure_instance_session_refuses_primary_via_delegation_when_archive_in_flight() {
        let mut app = test_app();
        let ws = app.test_workspace("archiving-primary");
        let primary = app
            .store
            .add_primary_agent(ws, AgentKind::Claude, 0)
            .unwrap();
        app.in_flight.insert(
            ws,
            crate::data::in_flight::InFlight::archive(
                crate::data::progress::SetupProgress::shared(),
                tokio_util::sync::CancellationToken::new(),
            ),
        );
        let outcome = crate::app::ensure_instance_session(&mut app, primary.id, false).unwrap();
        assert_eq!(outcome, crate::app::AttachReady::Refused);
    }
}
