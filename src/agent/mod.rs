//! Provisioning and orchestrating the Claude agent session.
//!
//! Everything that shapes a launched agent: prompt injection (`doctrine`,
//! `related`), launch configuration (`remote_control`, `mcp`), session
//! orchestration (`pm`), and skill installation (`skill`).

pub mod codex_commands;
pub mod doctrine;
pub mod handoff;
pub mod mcp;
pub mod pm;
pub mod related;
pub mod remote_control;
pub mod skill;
pub mod status;
