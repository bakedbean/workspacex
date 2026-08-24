//! `wsx agent` — listing agents and sending them prompts.

use super::Args;
use crate::cli::action::CliAction;
use crate::error::{Error, Result};

pub(in crate::cli) const USAGE_AGENT_SEND: &str =
    "agent send [--workspace <repo>/<slug>] <label> <prompt>";

pub(in crate::cli) fn parse_agent(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("list") => Ok(CliAction::AgentList),
        Some("send") => {
            let mut workspace: Option<String> = None;
            // Flags are recognised ONLY before the label. Everything from the
            // label onward is positional, so a message body that itself starts
            // with `--` is preserved verbatim.
            let target = loop {
                let arg = it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: USAGE_AGENT_SEND.into(),
                })?;
                match arg.as_str() {
                    "--workspace" => {
                        workspace = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--workspace needs value (<repo>/<slug>)".into(),
                        })?);
                    }
                    _ => break arg,
                }
            };
            let rest: Vec<String> = it.collect();
            if rest.is_empty() {
                return Err(Error::Usage {
                    group: None,
                    msg: USAGE_AGENT_SEND.into(),
                });
            }
            Ok(CliAction::AgentSend {
                target,
                prompt: rest.join(" "),
                workspace,
            })
        }
        Some("add") => {
            let kind = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "agent add <kind>".into(),
            })?;
            // Validate against the canonical agent set so this can't drift
            // from `AgentKind` as kinds are added/renamed.
            use crate::pty::session::AgentKind;
            if !AgentKind::ALL.iter().any(|k| k.display_name() == kind) {
                let valid = AgentKind::ALL
                    .iter()
                    .map(|k| k.display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::Usage {
                    group: None,
                    msg: format!("agent add: kind must be one of [{valid}], got '{kind}'"),
                });
            }
            Ok(CliAction::AgentAdd { kind })
        }
        _ => Err(Error::Usage {
            group: None,
            msg: "agent <list|send|add> ...".into(),
        }),
    }
}
