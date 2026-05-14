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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Dashboard,
    ProjectManager,
}
