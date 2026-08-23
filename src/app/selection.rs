//! What the dashboard cursor is pointing at, and moving it by name.

use super::*;

impl App {
    /// The durable, authoritative selection target. Returns
    /// `dashboard.selection` rather than indexing `selectable`, so the
    /// selection survives a temporarily-hidden row (folded repo / filter /
    /// quiet repo) instead of silently following the index onto a neighbor.
    pub fn selected_target(&self) -> Option<SelectionTarget> {
        self.dashboard.selection
    }

    /// Set the selection by index into the current `selectable`, keeping the
    /// durable `selection` target and the `selected` nav cursor in sync. Use
    /// this anywhere selection *intent* changes via an index (nav, click,
    /// landing on a freshly-created workspace).
    pub(crate) fn select_index(&mut self, idx: usize) {
        self.dashboard.selected = idx;
        self.dashboard.selection = self.selectable.get(idx).copied();
    }

    /// Select a workspace by repo name + workspace slug, unfolding its repo.
    /// Returns false if the pair doesn't exist or isn't currently selectable.
    /// Used by automation surfaces (waybar jump, `wsx --select`).
    pub fn select_workspace_by_name(&mut self, repo_name: &str, slug: &str) -> bool {
        let Some(repo_id) = self
            .repos
            .iter()
            .find(|r| r.name == repo_name)
            .map(|r| r.id)
        else {
            return false;
        };
        let Some(ws_id) = self
            .workspaces
            .iter()
            .find(|(rid, w)| *rid == repo_id && w.name == slug)
            .map(|(_, w)| w.id)
        else {
            return false;
        };
        self.dashboard.folded.insert(repo_id.0 as u64, false);
        match self
            .selectable
            .iter()
            .position(|t| *t == SelectionTarget::Workspace(ws_id))
        {
            Some(idx) => {
                self.select_index(idx);
                true
            }
            None => false,
        }
    }

    /// Select a workspace by repo name + slug and attach to it — the
    /// programmatic equivalent of highlighting the row and pressing Enter.
    /// Returns false when the pair doesn't exist. A missing agent binary
    /// surfaces as the in-TUI `AgentMissing` modal (attach aborts, selection
    /// and any attention marker stay); other attach errors are logged. Either
    /// way automation callers still land on the right row.
    /// Used by automation surfaces (waybar jump, `wsx --select`).
    pub fn open_workspace_by_name(&mut self, repo_name: &str, slug: &str) -> bool {
        if !self.select_workspace_by_name(repo_name, slug) {
            return false;
        }
        let Some(SelectionTarget::Workspace(ws_id)) = self.selected_target() else {
            return false;
        };
        if let Err(e) = attach_workspace(self, ws_id) {
            tracing::warn!(error = %e, "automation attach failed; staying on dashboard");
        }
        true
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
        // then collapse the whole split. Same for Refused (a live archive
        // refused the ensure). Mirror attach_workspace and bail.
        match ensure_instance_session(self, inst, true)? {
            AttachReady::Ok => {}
            AttachReady::AgentMissing | AttachReady::Refused => return Ok(()),
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

#[cfg(test)]
mod select_by_name_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};
    use std::path::PathBuf;

    fn app_with_one_workspace() -> App {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        store
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
        App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap()
    }

    #[test]
    fn selects_existing_workspace_and_unfolds_repo() {
        let mut app = app_with_one_workspace();
        let repo_name = app.repos[0].name.clone();
        let ws = app.workspaces[0].1.clone();
        app.dashboard.folded.insert(app.repos[0].id.0 as u64, true); // folded → must unfold
        assert!(app.select_workspace_by_name(&repo_name, &ws.name));
        assert_eq!(
            app.dashboard.folded.get(&(app.repos[0].id.0 as u64)),
            Some(&false)
        );
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(ws.id))
        );
    }

    #[test]
    fn unknown_names_return_false_and_leave_selection_alone() {
        let mut app = app_with_one_workspace();
        let before = app.selected_target();
        assert!(!app.select_workspace_by_name("nope", "nothing"));
        assert!(!app.select_workspace_by_name(&app.repos[0].name.clone(), "nothing"));
        assert_eq!(app.selected_target(), before);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn open_selects_and_attaches() {
        let mut env = crate::test_support::EnvGuard::new();
        env.set(
            "WSX_CLAUDE_BIN",
            crate::test_support::cat_ignore_args_path(),
        );
        let mut app = app_with_one_workspace();
        let repo_name = app.repos[0].name.clone();
        let ws = app.workspaces[0].1.clone();
        assert!(app.open_workspace_by_name(&repo_name, &ws.name));
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(ws.id))
        );
        assert!(
            matches!(&app.view, crate::ui::View::Attached(s)
                if s.focused_target().map(|t| t.workspace_id) == Some(ws.id)),
            "open_workspace_by_name should attach like Enter does; got {:?}",
            app.view
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn open_unknown_returns_false_and_stays_on_dashboard() {
        let mut app = app_with_one_workspace();
        assert!(!app.open_workspace_by_name("nope", "nothing"));
        assert!(matches!(app.view, crate::ui::View::Dashboard));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn open_with_missing_agent_keeps_attention_and_dashboard() {
        let mut env = crate::test_support::EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", "/nonexistent/claude-not-here");
        let mut app = app_with_one_workspace();
        let repo_name = app.repos[0].name.clone();
        let ws = app.workspaces[0].1.clone();
        app.workspace_needs_attention.insert(ws.id);
        assert!(app.open_workspace_by_name(&repo_name, &ws.name));
        assert!(matches!(app.view, crate::ui::View::Dashboard));
        assert!(
            app.workspace_needs_attention.contains(&ws.id),
            "a jump that could not attach must not dismiss attention"
        );
        assert_eq!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(ws.id)),
            "selection should still land on the row"
        );
    }
}
