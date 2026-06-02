//! Read-only introspection of live agent sessions and OS processes.
//!
//! `events` tails Claude Code JSONL session logs; `codex_events`,
//! `hermes_events`, and `pi_events` are the Codex/Hermes/Pi variants built
//! on top of it. `proc`
//! detects per-workspace processes via `lsof` (wsx observes, never spawns).

pub mod events;
pub mod codex_events;
pub mod hermes_events;
pub mod pi_events;
pub mod proc;
