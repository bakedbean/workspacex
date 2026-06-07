pub mod attached;
pub mod dashboard;
pub mod modal;
pub mod pm_pane;
pub mod split;
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Dashboard,
    ProjectManager,
    /// Reply input in the dashboard's detail bar. Active only while a
    /// workspace is selected. See `src/ui/dashboard/detail.rs`.
    DetailBarReply,
}
