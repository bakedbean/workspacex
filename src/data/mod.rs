//! Domain model, persistence, and workspace lifecycle.
//!
//! `store` is the SQLite-backed persistence hub (repos, workspaces, settings);
//! `repo` and `workspace` are CRUD/lifecycle orchestration over it; `setup`
//! runs the per-worktree setup script during workspace creation.

pub mod agents;
pub mod messages;
pub mod repo;
pub mod setup;
pub mod store;
pub mod workspace;
