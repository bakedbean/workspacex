//! `wsx status` and `wsx recap` — what an agent reports about itself.
//!
//! Both write to the dashboard rather than doing work, and both accept
//! the same hook/notify shapes, so they share a file.

use super::Args;
use crate::cli::action::CliAction;
use crate::error::{Error, Result};

pub(in crate::cli) fn parse_status(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("set") => {
            let state = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "usage: wsx status set <working|waiting|blocked|done> [--message <text>]"
                    .into(),
            })?;
            let mut message = None;
            while let Some(arg) = it.next() {
                if arg == "--message" || arg == "-m" {
                    message = Some(it.next().ok_or_else(|| Error::Usage {
                        group: None,
                        msg: "--message requires a value".into(),
                    })?);
                } else {
                    return Err(Error::Usage {
                        group: None,
                        msg: format!("unexpected argument: {arg}"),
                    });
                }
            }
            Ok(CliAction::StatusSet { state, message })
        }
        Some("clear") => Ok(CliAction::StatusClear),
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
            msg: format!("unknown status subcommand: {}", other.unwrap_or("(none)")),
        }),
    }
}

pub(in crate::cli) fn parse_recap(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("set") => {
            let mut goal = None;
            let mut state = None;
            let mut next = None;
            let mut goal_short = None;
            let mut state_short = None;
            let mut next_short = None;
            while let Some(arg) = it.next() {
                let slot = match arg.as_str() {
                    "--goal" => &mut goal,
                    "--state" => &mut state,
                    "--next" => &mut next,
                    "--goal-short" => &mut goal_short,
                    "--state-short" => &mut state_short,
                    "--next-short" => &mut next_short,
                    _ => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unexpected argument: {arg}"),
                        });
                    }
                };
                *slot = Some(it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: format!("{arg} requires a value"),
                })?);
            }
            if [&goal, &state, &next, &goal_short, &state_short, &next_short]
                .iter()
                .all(|o| o.is_none())
            {
                return Err(Error::Usage {
                    group: None,
                    msg: "usage: wsx recap set [--goal|--state|--next <text>] \
                          [--goal-short|--state-short|--next-short <text>] (at least one)"
                        .into(),
                });
            }
            Ok(CliAction::RecapSet {
                goal,
                state,
                next,
                goal_short,
                state_short,
                next_short,
            })
        }
        Some("show") => Ok(CliAction::RecapShow),
        Some("clear") => Ok(CliAction::RecapClear),
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown recap subcommand: {}", other.unwrap_or("(none)")),
        }),
    }
}
