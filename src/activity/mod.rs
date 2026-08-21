//! Read-only introspection of live agent sessions and OS processes.
//!
//! The Claude Code / Codex / Pi JSONL parsers now live in the `sessionx`
//! crate and are re-exported here so existing `crate::activity::events` (and
//! `codex_events`/`pi_events`) paths keep resolving. `hermes_events`
//! (SQLite-backed, via `~/.hermes/state.db`) and `proc` (lsof) remain
//! wsx-local — they depend on wsx infrastructure, not JSONL files.
//!
//! `omp_events` is wsx-local for a different reason: oh-my-pi writes the same
//! JSONL schema pi does, so only the *location* differs. It reimplements the
//! cwd encoding and re-exports `pi_events::tail_session` unchanged.

pub use sessionx::activity::{codex_events, events, pi_events};

pub mod hermes_events;
pub mod omp_events;
pub mod proc;
