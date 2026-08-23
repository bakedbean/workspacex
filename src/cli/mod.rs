//! The `wsx` command-line interface.
//!
//! Three stages, one module each:
//!
//!   argv --[`parse`]--> [`CliAction`] --[`run`]--> effects
//!
//! [`groups`] is the registry both `parse` and [`help`] read from, so the
//! commands wsx advertises and the commands it accepts cannot drift apart.
//! [`resolve`] holds the lookups and validation more than one `run` arm needs.

pub mod action;
pub mod groups;
pub mod help;
pub mod parse;
pub mod resolve;
pub mod run;

#[cfg(test)]
mod tests;

pub use action::{CliAction, HelpTopic, ValueSource};
pub use groups::{CmdInfo, GROUPS, GroupInfo, group_name};
pub use help::{render_group_help, render_root_help, render_usage_error, report_cli_error};
pub use parse::parse_args;
pub use run::run_cli;
