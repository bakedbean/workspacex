//! `wsx agent` — listing agents, sending them prompts, and reading replies.

use super::{Args, parse_workspace_flag};
use crate::cli::action::{
    CliAction, DEFAULT_MESSAGES_LIMIT, DEFAULT_WAIT_TIMEOUT_SECS, MessageBody, MessagesView,
};
use crate::error::{Error, Result};

pub(in crate::cli) const USAGE_AGENT_SEND: &str =
    "agent send [--workspace <repo>/<slug>] [--file <path>|-] <label|instance-id> [<message…>|-]";
const USAGE_AGENT_REPLY: &str = "agent reply [--file <path>|-] [<msg-id>] [<message…>|-]";
const USAGE_AGENT_MESSAGES: &str =
    "agent messages [--sent|--all] [--undelivered] [--limit <n>] [--id <msg-id>]";
const USAGE_AGENT_WAIT: &str = "agent wait [--from <sender>] [--after <msg-id>] [--timeout <secs>]";

fn usage(msg: impl Into<String>) -> Error {
    Error::Usage {
        group: None,
        msg: msg.into(),
    }
}

fn flag_value(it: &mut Args, flag: &str, what: &str) -> Result<String> {
    it.next()
        .ok_or_else(|| usage(format!("{flag} needs a value ({what})")))
}

/// A message id as typed: `561` or `#561` (the form the delivery banner and
/// `agent messages` print). `None` for anything else.
fn parse_msg_id(s: &str) -> Option<i64> {
    let digits = s.strip_prefix('#').unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn msg_id_value(it: &mut Args, flag: &str) -> Result<i64> {
    let v = flag_value(it, flag, "<msg-id>")?;
    parse_msg_id(&v).ok_or_else(|| usage(format!("{flag} expects a message id, got '{v}'")))
}

/// The body for `send`/`reply` from the positional words left after the
/// target, given whether `--file` already supplied one.
fn body_from(
    file: Option<MessageBody>,
    rest: Vec<String>,
    usage_line: &str,
) -> Result<MessageBody> {
    match (file, rest.as_slice()) {
        (Some(b), []) => Ok(b),
        (Some(_), _) => Err(usage(format!(
            "--file and an inline message are mutually exclusive\n{usage_line}"
        ))),
        (None, []) => Err(usage(usage_line)),
        (None, [only]) if only == "-" => Ok(MessageBody::Stdin),
        (None, words) => Ok(MessageBody::Inline(words.join(" "))),
    }
}

fn file_source(path: String) -> MessageBody {
    if path == "-" {
        MessageBody::Stdin
    } else {
        MessageBody::File(path.into())
    }
}

pub(in crate::cli) fn parse_agent(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("list") => Ok(CliAction::AgentList {
            workspace: parse_workspace_flag(it, "agent list [--workspace <repo>/<slug>]")?,
        }),
        Some("whoami") => match it.next() {
            None => Ok(CliAction::AgentWhoami),
            Some(extra) => Err(usage(format!(
                "agent whoami takes no arguments, got '{extra}'"
            ))),
        },
        Some("send") => {
            let mut workspace: Option<String> = None;
            let mut file: Option<MessageBody> = None;
            // Flags are recognised ONLY before the label. Everything from the
            // label onward is positional, so a message body that itself starts
            // with `--` is preserved verbatim.
            let target = loop {
                let arg = it.next().ok_or_else(|| usage(USAGE_AGENT_SEND))?;
                match arg.as_str() {
                    "--workspace" => {
                        workspace = Some(flag_value(it, "--workspace", "<repo>/<slug>")?);
                    }
                    "--file" => file = Some(file_source(flag_value(it, "--file", "<path> or -")?)),
                    _ => break arg,
                }
            };
            let body = body_from(file, it.collect(), USAGE_AGENT_SEND)?;
            Ok(CliAction::AgentSend {
                target,
                body,
                workspace,
            })
        }
        Some("reply") => {
            let mut file: Option<MessageBody> = None;
            let mut rest: Vec<String> = Vec::new();
            // Same rule as `send`: flags only before the first positional.
            while let Some(arg) = it.next() {
                if arg == "--file" {
                    file = Some(file_source(flag_value(it, "--file", "<path> or -")?));
                } else {
                    rest.push(arg);
                    rest.extend(&mut *it);
                }
            }
            // The first word is the message id only when something is left to
            // be the body — `wsx agent reply 42` sends "42" to the latest
            // sender rather than failing for want of a body.
            let has_body_after_first = file.is_some() || rest.len() > 1;
            let to = match rest.first().and_then(|w| parse_msg_id(w)) {
                Some(id) if has_body_after_first => {
                    rest.remove(0);
                    Some(id)
                }
                _ => None,
            };
            let body = body_from(file, rest, USAGE_AGENT_REPLY)?;
            Ok(CliAction::AgentReply { to, body })
        }
        Some("messages") => {
            let mut view = MessagesView::Inbox;
            let mut undelivered = false;
            let mut limit = DEFAULT_MESSAGES_LIMIT;
            let mut id = None;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--sent" | "--all" => {
                        let v = if arg == "--sent" {
                            MessagesView::Sent
                        } else {
                            MessagesView::Workspace
                        };
                        if view != MessagesView::Inbox && view != v {
                            return Err(usage(format!(
                                "--sent and --all are mutually exclusive\n{USAGE_AGENT_MESSAGES}"
                            )));
                        }
                        view = v;
                    }
                    "--undelivered" => undelivered = true,
                    "--limit" => {
                        let v = flag_value(it, "--limit", "<n>")?;
                        limit = v.parse().ok().filter(|n| *n > 0).ok_or_else(|| {
                            usage(format!("--limit expects a positive number, got '{v}'"))
                        })?;
                    }
                    "--id" => id = Some(msg_id_value(it, "--id")?),
                    other => {
                        return Err(usage(format!(
                            "unexpected argument '{other}'\n{USAGE_AGENT_MESSAGES}"
                        )));
                    }
                }
            }
            Ok(CliAction::AgentMessages {
                view,
                undelivered,
                limit,
                id,
            })
        }
        Some("wait") => {
            let mut from = None;
            let mut after = None;
            let mut timeout_secs = DEFAULT_WAIT_TIMEOUT_SECS;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--from" => from = Some(flag_value(it, "--from", "<sender>")?),
                    "--after" => after = Some(msg_id_value(it, "--after")?),
                    "--timeout" => {
                        let v = flag_value(it, "--timeout", "<secs>")?;
                        timeout_secs = v.parse().map_err(|_| {
                            usage(format!("--timeout expects whole seconds, got '{v}'"))
                        })?;
                    }
                    other => {
                        return Err(usage(format!(
                            "unexpected argument '{other}'\n{USAGE_AGENT_WAIT}"
                        )));
                    }
                }
            }
            Ok(CliAction::AgentWait {
                from,
                after,
                timeout_secs,
            })
        }
        Some("add") => {
            let kind = it.next().ok_or_else(|| usage("agent add <kind>"))?;
            // Validate against the canonical agent set so this can't drift
            // from `AgentKind` as kinds are added/renamed.
            use crate::pty::session::AgentKind;
            if !AgentKind::ALL.iter().any(|k| k.display_name() == kind) {
                let valid = AgentKind::ALL
                    .iter()
                    .map(|k| k.display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(usage(format!(
                    "agent add: kind must be one of [{valid}], got '{kind}'"
                )));
            }
            Ok(CliAction::AgentAdd { kind })
        }
        _ => Err(usage(
            "agent <list|add|send|messages|reply|wait|whoami> ...",
        )),
    }
}
