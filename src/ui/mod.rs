pub mod attached;
pub mod dashboard;
pub mod modal;
pub mod pm_pane;
pub mod theme;

use crate::store::WorkspaceId;

#[derive(Debug, Clone)]
pub enum View {
    Dashboard,
    Attached(WorkspaceId),
    /// Full-screen view of the Project Manager session. Reached from the
    /// dashboard's PM pane via `Ctrl-O`; detach back with `Ctrl-a d`.
    AttachedPm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Dashboard,
    ProjectManager,
}
