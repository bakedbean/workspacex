pub mod theme;
pub mod dashboard;
pub mod attached;
pub mod modal;

use crate::store::WorkspaceId;

#[derive(Debug, Clone)]
pub enum View {
    Dashboard,
    Attached(WorkspaceId),
}
