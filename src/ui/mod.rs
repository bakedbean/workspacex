pub mod attached;
pub mod dashboard;
pub mod modal;
pub mod theme;

use crate::store::WorkspaceId;

#[derive(Debug, Clone)]
pub enum View {
    Dashboard,
    Attached(WorkspaceId),
}
