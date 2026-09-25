//! The agent-messaging arms of `wsx agent`: who is calling, where a message
//! goes, where its body comes from, and how messages are printed.
//!
//! Delivery itself is not here. The CLI only ever writes `agent_messages`
//! rows (and reads them back); the dashboard's drain in `app::messaging` is
//! the single writer of `delivered_at`. `wait` and `messages` are reads, so
//! nothing in this module can double-deliver or lose a message.

use crate::app::messaging::{instance_label_relative_to, workspace_ref};
use crate::cli::action::MessageBody;
use crate::data::agents::AgentInstance;
use crate::data::messages::AgentMessage;
use crate::data::store::{AgentInstanceId, Store, Workspace, WorkspaceId};
use crate::error::{Error, Result};

/// The agent this invocation runs inside, from `$WSX_AGENT_INSTANCE_ID`.
///
/// Unlike `resolve_current_instance` (which guards hook attribution and
/// returns `None` on any doubt), this is for commands that are meaningless
/// without an identity, so every failure explains itself.
pub(in crate::cli) fn current_agent(store: &Store) -> Result<AgentInstance> {
    let raw = std::env::var("WSX_AGENT_INSTANCE_ID").map_err(|_| {
        Error::UserInput(
            "not running inside a wsx agent session ($WSX_AGENT_INSTANCE_ID is unset)".into(),
        )
    })?;
    let id = raw.trim().parse::<i64>().map_err(|_| {
        Error::UserInput(format!(
            "$WSX_AGENT_INSTANCE_ID is not an instance id: '{raw}'"
        ))
    })?;
    store
        .workspace_agents_by_id(AgentInstanceId(id))?
        .ok_or_else(|| {
            Error::UserInput(format!(
                "agent instance {id} ($WSX_AGENT_INSTANCE_ID) no longer exists"
            ))
        })
}

/// Read a message body from its source. Trailing whitespace is dropped (a
/// file's final newline is not part of the message) and an empty result is
/// refused: an empty delivery is almost always a shell-quoting slip, and
/// delivering it would only confuse the receiver.
pub(in crate::cli) fn read_body(body: &MessageBody) -> Result<String> {
    let text = match body {
        MessageBody::Inline(s) => s.clone(),
        MessageBody::File(p) => std::fs::read_to_string(p)
            .map_err(|e| Error::UserInput(format!("--file {}: {e}", p.display())))?,
        MessageBody::Stdin => {
            use std::io::{IsTerminal, Read};
            let mut stdin = std::io::stdin();
            if stdin.is_terminal() {
                return Err(Error::UserInput(
                    "message body is `-` (stdin), but stdin is a terminal; \
                     pipe the body in or use --file <path>"
                        .into(),
                ));
            }
            let mut s = String::new();
            stdin
                .read_to_string(&mut s)
                .map_err(|e| Error::UserInput(format!("reading message body from stdin: {e}")))?;
            s
        }
    };
    let trimmed = text.trim_end();
    if trimmed.trim_start().is_empty() {
        return Err(Error::UserInput(
            "message body is empty; nothing queued (check your quoting, or pass \
             the body with --file <path> / stdin)"
                .into(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Resolve `wsx agent send`'s target to its workspace and instance.
///
/// A target of all digits is an instance id (labels never are), which is
/// globally unique, so it needs no `--workspace`; if one is given anyway it
/// must agree. Anything else is a label (or `primary`) in the `--workspace`
/// workspace, else the current one.
pub(in crate::cli) fn resolve_send_target(
    store: &Store,
    target: &str,
    workspace: Option<&str>,
) -> Result<(Workspace, AgentInstanceId)> {
    use super::resolve::{join_or_none, resolve_current_workspace, resolve_workspace_spec};
    if !target.is_empty() && target.bytes().all(|b| b.is_ascii_digit()) {
        let id = AgentInstanceId(
            target
                .parse()
                .map_err(|_| Error::UserInput(format!("instance id out of range: {target}")))?,
        );
        let inst = store.workspace_agents_by_id(id)?.ok_or_else(|| {
            Error::UserInput(format!(
                "no agent instance {target} (instance ids come from `wsx agent whoami` \
                 or `wsx agent list`)"
            ))
        })?;
        let ws = store
            .workspace_by_id(inst.workspace_id)?
            .ok_or_else(|| Error::UserInput(format!("agent instance {target} has no workspace")))?;
        if let Some(spec) = workspace {
            let named = resolve_workspace_spec(store, spec)?;
            if named.id != ws.id {
                return Err(Error::UserInput(format!(
                    "agent instance {target} is in {}, not {spec}",
                    workspace_ref(store, ws.id).unwrap_or_else(|| ws.name.clone())
                )));
            }
        }
        return Ok((ws, id));
    }
    let target_ws = match workspace {
        Some(spec) => resolve_workspace_spec(store, spec)?,
        None => resolve_current_workspace(store)?,
    };
    let target_id = store
        .resolve_instance_label(target_ws.id, target)?
        .ok_or_else(|| {
            // `wsx agent list` only reports the CURRENT workspace, so
            // list the target's labels inline instead of pointing at it.
            let labels = store
                .workspace_agents(target_ws.id)
                .map(|v| {
                    let names: Vec<String> = v.iter().map(|i| i.label()).collect();
                    join_or_none(names.iter().map(|s| s.as_str()))
                })
                .unwrap_or_else(|_| "(unknown)".to_string());
            Error::UserInput(format!(
                "no agent '{target}' in workspace {}; agents there: {labels} \
                 (or `primary` for whichever is that workspace's primary agent)",
                target_ws.name
            ))
        })?;
    Ok((target_ws, target_id))
}

/// Resolve `wait --from` to an instance id: an instance id, a label (or
/// `primary`) in `home`, or `<repo>/<slug> <label>` exactly as a
/// cross-workspace sender is displayed in banners and `agent messages`.
pub(in crate::cli) fn resolve_sender(
    store: &Store,
    home: WorkspaceId,
    from: &str,
) -> Result<AgentInstanceId> {
    if !from.is_empty() && from.bytes().all(|b| b.is_ascii_digit()) {
        let id = AgentInstanceId(from.parse().unwrap_or(-1));
        return match store.workspace_agents_by_id(id)? {
            Some(_) => Ok(id),
            None => Err(Error::UserInput(format!(
                "--from: no agent instance {from}"
            ))),
        };
    }
    let (ws, label) = match from.rsplit_once(' ') {
        Some((spec, label)) => (
            super::resolve::resolve_workspace_spec(store, spec)?.id,
            label,
        ),
        None => (home, from),
    };
    store
        .resolve_instance_label(ws, label)?
        .ok_or_else(|| Error::UserInput(format!("--from: no agent '{from}'")))
}

/// Byte length plus the `(n bytes)` suffix `send`/`reply` print.
fn bytes(body: &str) -> String {
    format!("{} bytes", body.len())
}

/// `queued message #561 to claude (2747 bytes)` — the id is what a later
/// `agent messages --id` / `agent wait --after` needs.
pub(in crate::cli) fn queued_line(id: i64, to: &str, body: &str) -> String {
    format!("queued message #{id} to {to} ({})", bytes(body))
}

/// A sender or recipient as `viewer` sees it: bare label at home,
/// `<repo>/<slug> <label>` elsewhere, `-` for a CLI/editor-agent sender, and
/// `instance <id>` for an agent that has since been removed.
pub(in crate::cli) fn party(
    store: &Store,
    id: Option<AgentInstanceId>,
    viewer: WorkspaceId,
) -> String {
    match id {
        None => "-".to_string(),
        Some(i) => instance_label_relative_to(store, i, viewer)
            .unwrap_or_else(|| format!("instance {}", i.0)),
    }
}

pub(in crate::cli) const LISTING_HEADER: &str = "ID\tFROM\tTO\tBYTES\tCREATED\tDELIVERED";

/// One TSV row of `wsx agent messages`.
pub(in crate::cli) fn listing_row(store: &Store, m: &AgentMessage, viewer: WorkspaceId) -> String {
    use crate::util::time::format_utc_ms;
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}",
        m.id,
        party(store, m.from_agent_id, viewer),
        party(store, Some(m.target_agent_id), viewer),
        m.body.len(),
        format_utc_ms(m.created_at),
        delivery_state(m, false),
    )
}

/// The DELIVERED column: the injection time, `queued`, or `dropped` for a
/// message wsx retired without it ever reaching the agent. `long` adds the
/// explanation `--id`/`wait` print.
fn delivery_state(m: &AgentMessage, long: bool) -> String {
    use crate::util::time::format_utc_ms;
    match (m.delivered_at, &m.drop_reason) {
        (None, _) if long => "queued (not yet injected into the session)".to_string(),
        (None, _) => "queued".to_string(),
        (Some(t), Some(why)) if long => {
            format!(
                "dropped at {}, never reached the agent: {why}",
                format_utc_ms(t)
            )
        }
        (Some(_), Some(_)) => "dropped".to_string(),
        (Some(t), None) => format_utc_ms(t),
    }
}

/// A whole message: a header block, a blank line, then the body verbatim.
pub(in crate::cli) fn full_message(store: &Store, m: &AgentMessage, viewer: WorkspaceId) -> String {
    use crate::util::time::format_utc_ms;
    format!(
        "message #{}\nfrom: {}\nto: {}\nbytes: {}\ncreated: {}\ndelivered: {}\n\n{}",
        m.id,
        party(store, m.from_agent_id, viewer),
        party(store, Some(m.target_agent_id), viewer),
        m.body.len(),
        format_utc_ms(m.created_at),
        delivery_state(m, true),
        m.body
    )
}

/// How often `wait` re-reads the inbox.
const WAIT_POLL_MS: u64 = 500;

/// Block until a message for `me` (optionally only from `from`) is recorded,
/// and return it without marking it delivered.
///
/// With `after`, only ids above it count. Without, the baseline is the newest
/// id at the moment the wait starts, and messages still queued for `me` count
/// too — they have not reached the agent yet, so a reply that landed between
/// `send` and `wait` is not missed. `timeout_secs == 0` waits forever.
pub(in crate::cli) async fn wait_for_message(
    store: &Store,
    me: AgentInstanceId,
    from: Option<AgentInstanceId>,
    after: Option<i64>,
    timeout_secs: u64,
) -> Result<Option<AgentMessage>> {
    let (baseline, include_queued) = match after {
        Some(id) => (id, false),
        None => (store.max_message_id()?, true),
    };
    let deadline = (timeout_secs > 0)
        .then(|| std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs));
    loop {
        let hit = store
            .messages_to_since(me, baseline, include_queued)?
            .into_iter()
            .find(|m| from.is_none() || m.from_agent_id == from);
        if hit.is_some() {
            return Ok(hit);
        }
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return Ok(None);
        }
        tokio::time::sleep(std::time::Duration::from_millis(WAIT_POLL_MS)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_body_trims_the_tail_and_refuses_empty() {
        assert_eq!(
            read_body(&MessageBody::Inline("hi `there`\n\n".into())).unwrap(),
            "hi `there`"
        );
        assert!(read_body(&MessageBody::Inline("  \n".into())).is_err());

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("brief.md");
        std::fs::write(&p, "line `one`\n$(not run)\n").unwrap();
        assert_eq!(
            read_body(&MessageBody::File(p.clone())).unwrap(),
            "line `one`\n$(not run)"
        );
        let e = read_body(&MessageBody::File(dir.path().join("missing"))).unwrap_err();
        assert!(e.to_string().contains("missing"), "{e}");
    }

    #[test]
    fn delivery_state_tells_dropped_from_delivered() {
        let mut m = AgentMessage {
            id: 1,
            workspace_id: WorkspaceId(1),
            target_agent_id: AgentInstanceId(1),
            from_agent_id: None,
            body: "x".into(),
            created_at: 0,
            delivered_at: None,
            drop_reason: None,
        };
        assert_eq!(delivery_state(&m, false), "queued");
        m.delivered_at = Some(1_000);
        assert_eq!(delivery_state(&m, false), "1970-01-01T00:00:01Z");
        m.drop_reason = Some("binary missing".into());
        assert_eq!(delivery_state(&m, false), "dropped");
        assert!(delivery_state(&m, true).ends_with("never reached the agent: binary missing"));
    }

    #[test]
    fn queued_line_reports_id_and_size() {
        assert_eq!(
            queued_line(561, "claude", "héllo"),
            "queued message #561 to claude (6 bytes)"
        );
    }
}
