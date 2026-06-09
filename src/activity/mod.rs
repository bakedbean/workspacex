//! Read-only introspection of live agent sessions and OS processes.
//!
//! The Claude Code / Codex / Pi JSONL parsers now live in the `sessionx`
//! crate and are re-exported here so existing `crate::activity::events` (and
//! `codex_events`/`pi_events`) paths keep resolving. `hermes_events`
//! (SQLite-backed, via `~/.hermes/state.db`) and `proc` (lsof) remain
//! wsx-local — they depend on wsx infrastructure, not JSONL files.

pub use sessionx::activity::{codex_events, events, pi_events};

pub mod hermes_events;
pub mod proc;
