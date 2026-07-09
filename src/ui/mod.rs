pub mod attached;
pub mod dashboard;
pub mod footer;
pub mod modal;
pub mod pm_pane;
pub mod split;
pub mod text;
pub mod theme;
pub mod updates_bar;

pub use split::{AttachedState, SplitDirection, SplitTree};

#[derive(Debug, Clone)]
pub enum View {
    Dashboard,
    /// One or more workspace PTYs arranged in a recursive vim-style split
    /// tree, with one pane focused for input.
    Attached(AttachedState),
    /// Full-screen view of the Project Manager session. Reached from the
    /// dashboard's PM pane via `Ctrl-O`; detach back with `Ctrl-x d`.
    AttachedPm,
    /// Full-screen view of a remote tmux-shared workspace attached over ssh.
    /// Reached by pressing Enter on an alive row in `Modal::RemoteWorkspaceList`;
    /// detach back with `Ctrl-x d`. The backing `Session` (in `app.remote`)
    /// wraps a local `ssh -t … tmux attach` client — detach/quit sever only
    /// that client, never the remote agent (the Phase 1 persistence contract).
    AttachedRemote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Dashboard,
    ProjectManager,
    /// Reply input in the dashboard's detail bar. Active only while a
    /// workspace is selected. See `src/ui/dashboard/detail.rs`.
    DetailBarReply,
}
