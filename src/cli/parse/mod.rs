//! argv -> [`CliAction`].
//!
//! `parse_args` reads the leading group token and hands the rest of the
//! iterator to that group's parser. Group parsers live one per file and
//! mirror the groups in [`super::groups::GROUPS`].

use super::action::{CliAction, HelpTopic};
use super::groups::group_name;
use crate::error::{Error, Result};

pub mod agent;
pub mod config;
pub mod desktop;
pub mod repo;
pub mod reporting;
pub mod workspace;

use agent::parse_agent;
use config::parse_config;
use desktop::{parse_menubar, parse_remote, parse_setup, parse_waybar};
use repo::parse_repo;
use reporting::{parse_recap, parse_status};
use workspace::{parse_shared, parse_workspace};

/// The dashed help flags. Bare `help` is handled separately — only in a
/// subcommand position — because it is a legitimate argument value/name
/// elsewhere (e.g. a repo named `help`).
fn is_help_flag(tok: &str) -> bool {
    matches!(tok, "--help" | "-h")
}

fn is_version(tok: &str) -> bool {
    matches!(tok, "--version" | "-V")
}

pub(in crate::cli) type Args = dyn Iterator<Item = String>;

pub fn parse_args(args: Vec<String>) -> Result<CliAction> {
    let mut rest: Vec<String> = args.into_iter().skip(1).collect();
    let first = if rest.is_empty() {
        None
    } else {
        Some(rest.remove(0))
    };

    match first.as_deref() {
        None => return Ok(CliAction::Tui { select: None }),
        // Match the literal `help` subcommand before the is_help() flag guard,
        // so `wsx help <group>` resolves the group instead of collapsing to Root.
        Some("help") => {
            let topic = match rest.first().and_then(|s| group_name(s)) {
                Some(g) => HelpTopic::Group(g),
                None => HelpTopic::Root,
            };
            return Ok(CliAction::Help(topic));
        }
        Some(t) if is_help_flag(t) => return Ok(CliAction::Help(HelpTopic::Root)),
        Some(t) if is_version(t) => return Ok(CliAction::Version),
        Some("--select") => {
            let Some(target) = rest.first().cloned() else {
                return Err(Error::Usage {
                    group: None,
                    msg: "--select needs <repo>/<slug>".into(),
                });
            };
            let Some((repo, slug)) = target.split_once('/') else {
                return Err(Error::Usage {
                    group: None,
                    msg: "--select target must be <repo>/<slug>".into(),
                });
            };
            return Ok(CliAction::Tui {
                select: Some((repo.to_string(), slug.to_string())),
            });
        }
        _ => {}
    }

    let group = first.as_deref().expect("None handled above");

    // Per-group help. `--help`/`-h` are flag-style and may appear anywhere
    // (`wsx agent send --help`); bare `help` is only a help request in the
    // subcommand slot (`wsx agent help`), since it is a valid value/name
    // elsewhere (e.g. `wsx repo remove help` removes a repo named `help`).
    let wants_group_help =
        rest.iter().any(|a| is_help_flag(a)) || rest.first().map(|a| a.as_str()) == Some("help");
    if wants_group_help {
        if let Some(g) = group_name(group) {
            return Ok(CliAction::Help(HelpTopic::Group(g)));
        }
    }

    let mut it = rest.into_iter();
    match group {
        "repo" => parse_repo(&mut it).map_err(|e| tag_group(e, group)),
        "config" => parse_config(&mut it).map_err(|e| tag_group(e, group)),
        "remote" => parse_remote(&mut it).map_err(|e| tag_group(e, group)),
        "shared" => parse_shared(&mut it).map_err(|e| tag_group(e, group)),
        "workspace" => parse_workspace(&mut it).map_err(|e| tag_group(e, group)),
        "agent" => parse_agent(&mut it).map_err(|e| tag_group(e, group)),
        "setup" => parse_setup(&mut it).map_err(|e| tag_group(e, group)),
        "status" => parse_status(&mut it).map_err(|e| tag_group(e, group)),
        "recap" => parse_recap(&mut it).map_err(|e| tag_group(e, group)),
        "waybar" => parse_waybar(&mut it).map_err(|e| tag_group(e, group)),
        "menubar" => parse_menubar(&mut it).map_err(|e| tag_group(e, group)),
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown command: {other}"),
        }),
    }
}

fn tag_group(e: Error, group: &str) -> Error {
    match e {
        Error::Usage { group: None, msg } => Error::Usage {
            group: group_name(group),
            msg,
        },
        other => other,
    }
}
