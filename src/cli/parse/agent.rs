//! `wsx agent` — listing agents, sending them prompts, and reading replies.

use super::{Args, parse_read_flags};
use crate::cli::action::{
    CliAction, DEFAULT_MESSAGES_LIMIT, DEFAULT_WAIT_TIMEOUT_SECS, MessageBody, MessagesView,
};
use crate::error::{Error, Result};

pub(in crate::cli) const USAGE_AGENT_SEND: &str =
    "agent send [--workspace <repo>/<slug>] [--file <path>|-] <label|instance-id> [<message…>|-]";
const USAGE_AGENT_REPLY: &str = "agent reply [--file <path>|-] [<msg-id>] [<message…>|-]";
const USAGE_AGENT_MESSAGES: &str =
    "agent messages [--sent|--all] [--undelivered] [--limit <n>] [--id <msg-id>] [--json]";
const USAGE_AGENT_WAIT: &str =
    "agent wait [--from <sender>] [--after <msg-id>] [--timeout <secs>] [--done <agent>]";

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

/// The flags `send`/`reply` take ahead of the message.
#[derive(Default)]
struct BodyFlags {
    /// Whether `--workspace` is a flag here (`send`) or not (`reply`).
    takes_workspace: bool,
    workspace: Option<String>,
    file: Option<MessageBody>,
}

impl BodyFlags {
    fn names(&self) -> &'static [&'static str] {
        if self.takes_workspace {
            &["--file", "--workspace"]
        } else {
            &["--file"]
        }
    }

    /// Consume flags up to the next positional word and return it. Called
    /// between positionals too, so flags may sit either side of the label
    /// or message id; only the body itself is left unparsed.
    fn take_until_word(&mut self, it: &mut Args) -> Result<Option<String>> {
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--file" => {
                    self.file = Some(file_source(flag_value(it, "--file", "<path> or -")?));
                }
                "--workspace" if self.takes_workspace => {
                    self.workspace = Some(flag_value(it, "--workspace", "<repo>/<slug>")?);
                }
                _ => return Ok(Some(arg)),
            }
        }
        Ok(None)
    }
}

/// The body for `send`/`reply` from the positional words left after the
/// target, given whether `--file` already supplied one. `flags` are the
/// flag names the command takes: one of them as a word of an inline body
/// was surely meant as a flag, so it is refused rather than sent as text.
fn body_from(
    file: Option<MessageBody>,
    rest: Vec<String>,
    flags: &[&str],
    usage_line: &str,
) -> Result<MessageBody> {
    if file.is_none() && rest.len() > 1 {
        if let Some(flag) = rest.iter().find(|w| flags.contains(&w.as_str())) {
            return Err(usage(format!(
                "{flag} must come before the message (quote the message if it is text)\n{usage_line}"
            )));
        }
    }
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
        Some("list") => {
            let f = parse_read_flags(it, "agent list [--workspace <repo>/<slug>] [--json]", true)?;
            Ok(CliAction::AgentList {
                workspace: f.workspace,
                json: f.json,
            })
        }
        Some("whoami") => match it.next() {
            None => Ok(CliAction::AgentWhoami),
            Some(extra) => Err(usage(format!(
                "agent whoami takes no arguments, got '{extra}'"
            ))),
        },
        Some("send") => {
            let mut flags = BodyFlags {
                takes_workspace: true,
                ..Default::default()
            };
            // Flags are recognised before and after the label, up to the
            // first message word. From there on the body is verbatim, so a
            // message that itself starts with `--` is preserved.
            let target = flags
                .take_until_word(it)?
                .ok_or_else(|| usage(USAGE_AGENT_SEND))?;
            let rest: Vec<String> = flags.take_until_word(it)?.into_iter().chain(it).collect();
            let names = flags.names();
            let body = body_from(flags.file, rest, names, USAGE_AGENT_SEND)?;
            Ok(CliAction::AgentSend {
                target,
                body,
                workspace: flags.workspace,
            })
        }
        Some("reply") => {
            let mut flags = BodyFlags::default();
            // Same rule as `send`, with the optional message id in the
            // label's place.
            let first = flags.take_until_word(it)?;
            let (to, rest) = match first.as_deref().and_then(parse_msg_id) {
                Some(id) => {
                    let next = flags.take_until_word(it)?;
                    // The first word is the message id only when something is
                    // left to be the body — `wsx agent reply 42` sends "42" to
                    // the latest sender rather than failing for want of a body.
                    if next.is_some() || flags.file.is_some() {
                        (Some(id), next.into_iter().chain(it).collect())
                    } else {
                        (None, first.into_iter().collect())
                    }
                }
                None => (None, first.into_iter().chain(it).collect()),
            };
            let names = flags.names();
            let body = body_from(flags.file, rest, names, USAGE_AGENT_REPLY)?;
            Ok(CliAction::AgentReply { to, body })
        }
        Some("messages") => {
            let mut view = MessagesView::Inbox;
            let mut undelivered = false;
            let mut limit = DEFAULT_MESSAGES_LIMIT;
            let mut id = None;
            let mut json = false;
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
                    "--json" => json = true,
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
                json,
            })
        }
        Some("wait") => {
            let mut from = None;
            let mut after = None;
            let mut timeout_secs = DEFAULT_WAIT_TIMEOUT_SECS;
            let mut done = None;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--from" => from = Some(flag_value(it, "--from", "<sender>")?),
                    "--done" => done = Some(flag_value(it, "--done", "<agent>")?),
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
                done,
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
        Some("remove") => {
            let label = it.next().ok_or_else(|| usage("agent remove <label>"))?;
            if let Some(extra) = it.next() {
                return Err(usage(format!(
                    "unexpected argument: {extra} (usage: agent remove <label>)"
                )));
            }
            Ok(CliAction::AgentRemove { label })
        }
        _ => Err(usage(
            "agent <list|add|remove|send|messages|reply|wait|whoami> ...",
        )),
    }
}
