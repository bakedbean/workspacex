//! `wsx status`, `wsx recap`, and `wsx context` — what an agent reports about itself.
//!
//! Both write to the dashboard rather than doing work, and both accept
//! the same hook/notify shapes, so they share a file.

use super::{Args, ReadFlags, parse_read_flags};
use crate::cli::action::{CliAction, RecapFields};
use crate::error::{Error, Result};

/// The flags of `<group> show`, which is also the bare `<group>`: `first`
/// is the word after the group, when it was a flag rather than `show`.
fn parse_show_flags(first: Option<String>, it: &mut Args, group: &str) -> Result<ReadFlags> {
    let usage = format!("{group} [show] [--workspace <repo>/<slug>] [--json]");
    // `Args` is a `'static` trait object, so a chain borrowing `it` can't
    // stand in for it; the tail is a handful of flags, so just collect.
    let rest: Vec<String> = first.into_iter().chain(it).collect();
    parse_read_flags(&mut rest.into_iter(), &usage, true)
}

pub(in crate::cli) fn parse_status(it: &mut Args) -> Result<CliAction> {
    let first = it.next();
    match first.as_deref() {
        Some("set") => {
            let state = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "usage: wsx status set <working|waiting|blocked|done> [--message <text>] \
                      [--goal|--state|--next <text>] [--goal-short|--state-short|--next-short <text>]"
                    .into(),
            })?;
            let mut message = None;
            let mut recap = RecapFields::default();
            while let Some(arg) = it.next() {
                let slot = if arg == "--message" || arg == "-m" {
                    &mut message
                } else if let Some(slot) = recap.slot(&arg) {
                    slot
                } else {
                    return Err(Error::Usage {
                        group: None,
                        msg: format!("unexpected argument: {arg}"),
                    });
                };
                *slot = Some(it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: format!("{arg} requires a value"),
                })?);
            }
            Ok(CliAction::StatusSet {
                state,
                message,
                recap,
            })
        }
        Some("clear") => Ok(CliAction::StatusClear),
        // Bare `wsx status` (with or without `show`'s flags) reads rather
        // than erroring: agents reach for it to check what they last reported.
        None | Some("show") => {
            let f = parse_show_flags(None, it, "status")?;
            Ok(CliAction::StatusShow {
                workspace: f.workspace,
                json: f.json,
            })
        }
        Some(flag) if flag.starts_with('-') => {
            let f = parse_show_flags(first, it, "status")?;
            Ok(CliAction::StatusShow {
                workspace: f.workspace,
                json: f.json,
            })
        }
        Some("from-hook") => {
            let mut agent = None;
            while let Some(arg) = it.next() {
                if arg == "--agent" {
                    agent = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: "--agent requires a value".into(),
                    })?);
                } else {
                    return Err(Error::Usage {
                        group: None,
                        msg: format!("unexpected argument: {arg}"),
                    });
                }
            }
            Ok(CliAction::StatusFromHook { agent })
        }
        Some("from-notify") => {
            let mut agent = None;
            let mut payload = None;
            while let Some(arg) = it.next() {
                if arg == "--agent" {
                    agent = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: "--agent requires a value".into(),
                    })?);
                } else {
                    // Codex appends the JSON payload as the final positional arg.
                    payload = Some(arg);
                }
            }
            Ok(CliAction::StatusFromNotify { agent, payload })
        }
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown status subcommand: {}", other.unwrap_or_default()),
        }),
    }
}

pub(in crate::cli) fn parse_recap(it: &mut Args) -> Result<CliAction> {
    let first = it.next();
    match first.as_deref() {
        Some("set") => {
            let mut fields = RecapFields::default();
            while let Some(arg) = it.next() {
                let Some(slot) = fields.slot(&arg) else {
                    return Err(Error::Usage {
                        group: None,
                        msg: format!("unexpected argument: {arg}"),
                    });
                };
                *slot = Some(it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: format!("{arg} requires a value"),
                })?);
            }
            if fields.is_empty() {
                return Err(Error::Usage {
                    group: None,
                    msg: "usage: wsx recap set [--goal|--state|--next <text>] \
                          [--goal-short|--state-short|--next-short <text>] (at least one)"
                        .into(),
                });
            }
            let RecapFields {
                goal,
                state,
                next,
                goal_short,
                state_short,
                next_short,
            } = fields;
            Ok(CliAction::RecapSet {
                goal,
                state,
                next,
                goal_short,
                state_short,
                next_short,
            })
        }
        // Bare `wsx recap` is `recap show`, for the same reason.
        None | Some("show") => {
            let f = parse_show_flags(None, it, "recap")?;
            Ok(CliAction::RecapShow {
                workspace: f.workspace,
                json: f.json,
            })
        }
        Some(flag) if flag.starts_with('-') => {
            let f = parse_show_flags(first, it, "recap")?;
            Ok(CliAction::RecapShow {
                workspace: f.workspace,
                json: f.json,
            })
        }
        Some("clear") => Ok(CliAction::RecapClear),
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown recap subcommand: {}", other.unwrap_or_default()),
        }),
    }
}

pub(in crate::cli) fn parse_context(it: &mut Args) -> Result<CliAction> {
    let action = match it.next().as_deref() {
        Some("show") => {
            return Ok(CliAction::ContextShow {
                workspace: parse_read_flags(
                    it,
                    "wsx context show [--workspace <repo>/<slug>]",
                    false,
                )?
                .workspace,
            });
        }
        Some("write") => CliAction::ContextWrite,
        other => {
            return Err(Error::Usage {
                group: None,
                msg: format!(
                    "unknown context subcommand: {} (usage: wsx context <show|write>)",
                    other.unwrap_or("(none)")
                ),
            });
        }
    };
    if let Some(extra) = it.next() {
        return Err(Error::Usage {
            group: None,
            msg: format!("unexpected argument: {extra} (usage: wsx context <show|write>)"),
        });
    }
    Ok(action)
}
