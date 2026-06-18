//! Domain model, persistence, and workspace lifecycle.
//!
//! `store` is the SQLite-backed persistence hub (repos, workspaces, settings);
//! `repo` and `workspace` are CRUD/lifecycle orchestration over it; `setup`
//! runs the per-worktree setup script during workspace creation.

pub mod agents;
pub mod messages;
pub mod progress;
pub mod repo;
pub mod setup;
pub mod setup_log;
pub mod store;
pub mod workspace;

// Internal `impl Store` blocks split out of store.rs by concern; no public
// surface of their own, so they stay private modules.
mod activity;
mod layout;
mod schema;
mod settings;
mod status;
