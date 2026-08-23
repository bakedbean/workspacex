//! The `wsx` command-line interface.
//!
//! Three stages, one module each:
//!
//!   argv --[`parse`]--> [`CliAction`] --[`run`]--> effects
//!
//! [`groups`] is the registry both `parse` and [`help`] read from, so the
//! commands wsx advertises and the commands it accepts cannot drift apart.
//! [`resolve`] holds the lookups and validation more than one `run` arm needs.

pub(crate) mod action;
pub(crate) mod groups;
pub(crate) mod help;
pub(crate) mod parse;
pub(crate) mod resolve;
pub(crate) mod run;

#[cfg(test)]
mod tests;

pub use action::{CliAction, HelpTopic, ValueSource};
pub use groups::{CmdInfo, GROUPS, GroupInfo, group_name};
pub use help::{render_group_help, render_root_help, render_usage_error, report_cli_error};
pub use parse::parse_args;
pub use run::run_cli;
