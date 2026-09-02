//! `CliAction` — the vocabulary `parse` produces and `run` consumes.
//!
//! One variant per command. Keeping it in its own module lets `parse` and
//! `run` sit in separate files without either owning the shared type.

use crate::error::{Error, Result};
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
pub enum HelpTopic {
    Root,
    Group(&'static str),
}

#[derive(Debug)]
pub enum CliAction {
    Tui {
        select: Option<(String, String)>,
    },
    Help(HelpTopic),
    Version,
    RepoAdd {
        path: PathBuf,
        name: String,
        branch_prefix: String,
    },
    RepoList,
    RepoRemove {
        name: String,
    },
    RepoSetPrefix {
        name: String,
        prefix: String,
    },
    RepoSetBaseBranch {
        name: String,
        value: String,
    },
    RepoSetInstructions {
        name: String,
        source: ValueSource,
    },
    RepoSetSetup {
        name: String,
        source: ValueSource,
    },
    RepoSetArchive {
        name: String,
        source: ValueSource,
    },
    RepoEditSetup {
        name: String,
    },
    RepoEditArchive {
        name: String,
    },
    RepoSetPinnedCommands {
        name: String,
        source: ValueSource,
    },
    RepoEditPinnedCommands {
        name: String,
    },
    RepoSetName {
        name: String,
        new_name: String,
    },
    RepoSetRelatedRepos {
        name: String,
        source: ValueSource,
    },
    RepoEditRelatedRepos {
        name: String,
    },
    ConfigGet {
        key: String,
    },
    ConfigSet {
        key: String,
        source: ValueSource,
    },
    ConfigList,
    ConfigEdit {
        key: String,
    },
    RemoteList,
    RemoteRun {
        name: String,
    },
    SharedList {
        json: bool,
    },
    WorkspaceCreate {
        repo: String,
        name: Option<String>,
        yolo: bool,
        shared: bool,
        agent: Option<String>,
        /// Seed the new workspace's primary agent with this prompt, as if
        /// `wsx agent send` had been run against it immediately after.
        prompt: Option<String>,
    },
    WorkspaceList {
        repo: Option<String>,
    },
    WorkspacePath {
        repo: String,
        name: String,
    },
    WorkspaceRename {
        repo: String,
        name: String,
        new_name: String,
    },
    WorkspaceArchive {
        repo: String,
        name: String,
        keep_worktree: bool,
        force_delete_branch: bool,
    },
    WorkspaceShare {
        repo: String,
        name: String,
        shared: bool,
    },
    SetupInstallSkill,
    SetupWaybar,
    WaybarStatus,
    WaybarMenu,
    WaybarJump {
        repo: String,
        slug: String,
    },
    WaybarMenuEntries,
    WaybarRefreshPrs,
    SetupMenubar,
    MenubarPlugin,
    MenubarJump {
        repo: String,
        slug: String,
    },
    MenubarCopyPath {
        repo: String,
        slug: String,
    },
    MenubarRefresh,
    AgentList,
    AgentSend {
        target: String,
        prompt: String,
        /// `<repo>/<slug>` when addressing an agent in ANOTHER workspace;
        /// `None` means the current workspace (the pre-existing behavior).
        workspace: Option<String>,
    },
    AgentAdd {
        kind: String,
    },
    StatusSet {
        state: String,
        message: Option<String>,
    },
    StatusClear,
    StatusFromHook {
        /// The harness whose event payload is on stdin. `None` falls back to
        /// the resolved workspace's agent kind.
        agent: Option<String>,
    },
    StatusFromNotify {
        /// The harness whose `notify` payload is the trailing positional arg.
        /// `None` falls back to the resolved workspace's agent kind.
        agent: Option<String>,
        /// The raw JSON payload Codex passes as the final argv element. If
        /// multiple bare positional args appear, the last one wins; extra args
        /// are tolerated rather than rejected (unlike `from-hook`) because
        /// `notify` must never fail a turn.
        payload: Option<String>,
    },
    RecapSet {
        goal: Option<String>,
        state: Option<String>,
        next: Option<String>,
        goal_short: Option<String>,
        state_short: Option<String>,
        next_short: Option<String>,
    },
    RecapShow,
    RecapClear,
    /// `wsx context show` — print the workspace context digest.
    ContextShow,
    /// `wsx context write` — write the digest under the state dir, print its path.
    ContextWrite,
}

#[derive(Debug)]
pub enum ValueSource {
    Literal(String),
    File(PathBuf),
}

impl ValueSource {
    pub fn from_arg(value: String) -> Self {
        if let Some(path) = value.strip_prefix('@') {
            ValueSource::File(PathBuf::from(path))
        } else {
            ValueSource::Literal(value)
        }
    }

    pub fn resolve(self) -> Result<String> {
        match self {
            ValueSource::Literal(s) => Ok(s),
            ValueSource::File(p) => std::fs::read_to_string(&p)
                .map_err(|e| Error::UserInput(format!("read {}: {e}", p.display()))),
        }
    }
}
