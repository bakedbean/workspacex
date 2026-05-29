//! Detail-bar modules. Pluggable units that render into a container
//! slot in the workspace detail bar. The host (chrome layer in
//! `src/ui/dashboard/detail.rs`) iterates over configured container
//! IDs, looks each up in the `Registry`, and dispatches `render`.
//!
//! See `docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`.

use crate::events::WorkspaceEvents;
use crate::git::forge::BranchLifecycle;
use crate::git::DiffStats;
use crate::proc::ProcInfo;
use crate::data::store::{Repo, Workspace};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use std::collections::HashMap;

/// Borrowed snapshot of everything a module might need to render.
/// Built once per draw by the chrome layer in
/// `src/ui/dashboard/detail.rs` and passed by reference to each module.
/// Zero allocations per draw — all fields are borrowed or `Copy`.
pub struct DetailContext<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub diff_per_file: Option<&'a HashMap<String, DiffStats>>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<&'a str>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub events_scanned: bool,
    pub theme: &'a Theme,
}

pub trait DetailModule: Send + Sync {
    /// Stable identifier used in config JSON. Lowercase snake_case.
    fn id(&self) -> &'static str;

    /// Heading drawn above the module's body by the host. Modules do
    /// not render their own title.
    fn title(&self) -> &'static str;

    /// Produce the module's content lines. The container will paint
    /// these via `Paragraph` and slice them by scroll offset. `width`
    /// is the column width the content will be drawn into (use for
    /// wrapping/truncation decisions).
    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<ratatui::text::Line<'static>>;
}

pub struct Registry {
    modules: HashMap<&'static str, Box<dyn DetailModule>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self.modules.keys().copied().collect();
        f.debug_struct("Registry").field("modules", &ids).finish()
    }
}

impl Registry {
    pub fn new() -> Self {
        Self {
            modules: HashMap::new(),
        }
    }

    pub fn register(&mut self, m: Box<dyn DetailModule>) {
        let id = m.id();
        self.modules.insert(id, m);
    }

    pub fn get(&self, id: &str) -> Option<&dyn DetailModule> {
        self.modules.get(id).map(|b| b.as_ref())
    }

    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.modules.keys().copied()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

pub mod processes;
pub mod recent_chat;
pub mod recent_files;
pub mod session_summary;

/// Populate `reg` with the four built-in modules: session summary,
/// recent chat, processes, and recent files.
pub fn register_builtins(reg: &mut Registry) {
    reg.register(Box::new(session_summary::SessionSummary));
    reg.register(Box::new(recent_chat::RecentChat));
    reg.register(Box::new(processes::Processes));
    reg.register(Box::new(recent_files::RecentFiles));
}

#[cfg(test)]
pub(crate) mod tests_helpers {
    use super::*;
    use crate::data::store::{Repo, RepoId, SetupStatus, Workspace, WorkspaceId, WorkspaceState};
    use std::path::PathBuf;

    /// Build a minimal `DetailContext` backed by leaked allocations.
    /// Sufficient for unit-testing module methods that don't need
    /// realistic data. Test-only — leaks are fine.
    pub fn stub_context() -> DetailContext<'static> {
        let repo: &'static Repo = Box::leak(Box::new(Repo {
            id: RepoId(1),
            name: "demo".into(),
            path: PathBuf::from("/r"),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
        }));
        let workspace: &'static Workspace = Box::leak(Box::new(Workspace {
            id: WorkspaceId(1),
            repo_id: repo.id,
            name: "ws".into(),
            branch: "br".into(),
            worktree_path: PathBuf::from("/wt"),
            state: WorkspaceState::Ready,
            setup_status: SetupStatus::Ok,
            created_at: 0,
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
        }));
        let theme: &'static Theme = Box::leak(Box::new(Theme::default()));
        DetailContext {
            repo,
            workspace,
            events: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: Status::Idle,
            ago_secs: None,
            events_scanned: false,
            theme,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockModule {
        id: &'static str,
        title: &'static str,
    }
    impl DetailModule for MockModule {
        fn id(&self) -> &'static str {
            self.id
        }
        fn title(&self) -> &'static str {
            self.title
        }
        fn lines(
            &self,
            _ctx: &DetailContext<'_>,
            _width: u16,
        ) -> Vec<ratatui::text::Line<'static>> {
            Vec::new()
        }
    }

    #[test]
    fn empty_registry_returns_none() {
        let reg = Registry::new();
        assert!(reg.get("anything").is_none());
    }

    #[test]
    fn register_and_get_round_trip() {
        let mut reg = Registry::new();
        reg.register(Box::new(MockModule {
            id: "foo",
            title: "FOO",
        }));
        let m = reg.get("foo").expect("module foo should be registered");
        assert_eq!(m.id(), "foo");
        assert_eq!(m.title(), "FOO");
    }

    #[test]
    fn get_unknown_returns_none() {
        let mut reg = Registry::new();
        reg.register(Box::new(MockModule {
            id: "foo",
            title: "FOO",
        }));
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn ids_enumerates_all_registered() {
        let mut reg = Registry::new();
        reg.register(Box::new(MockModule {
            id: "a",
            title: "A",
        }));
        reg.register(Box::new(MockModule {
            id: "b",
            title: "B",
        }));
        let ids: std::collections::HashSet<_> = reg.ids().collect();
        assert!(ids.contains("a"));
        assert!(ids.contains("b"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn register_builtins_includes_session_summary() {
        let mut reg = Registry::new();
        register_builtins(&mut reg);
        assert!(reg.get("session_summary").is_some());
    }

    #[test]
    fn register_builtins_includes_recent_chat() {
        let mut reg = Registry::new();
        register_builtins(&mut reg);
        assert!(reg.get("recent_chat").is_some());
    }

    #[test]
    fn register_builtins_includes_processes() {
        let mut reg = Registry::new();
        register_builtins(&mut reg);
        assert!(reg.get("processes").is_some());
    }

    #[test]
    fn register_builtins_includes_recent_files() {
        let mut reg = Registry::new();
        register_builtins(&mut reg);
        assert!(reg.get("recent_files").is_some());
    }
}
