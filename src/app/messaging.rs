//! Drains the agent_messages inbox and delivers each message into the target
//! instance's live session, tagged so the receiver knows it is peer mail.

use crate::data::messages::AgentMessage;
use crate::data::store::Store;

/// The banner injected into the receiving agent. Pure + testable.
pub fn delivery_banner(from_label: Option<&str>, body: &str) -> String {
    match from_label {
        Some(f) => format!("[message from {f}]\n{body}"),
        None => format!("[message]\n{body}"),
    }
}

/// Resolve the human-readable sender label for a message (None → CLI/human origin).
///
/// The sender is looked up GLOBALLY by instance id rather than within
/// `msg.workspace_id`, because a handoff is enqueued against the *target's*
/// workspace while its sender lives elsewhere. When the sender is in a
/// different workspace than the message, the label is qualified with
/// `<repo>/<slug> ` so the recipient can see where the work came from.
pub fn sender_label(store: &Store, msg: &AgentMessage) -> Option<String> {
    let from = msg.from_agent_id?;
    let sender = store.workspace_agents_by_id(from).ok()??;
    let label = sender.label();
    if sender.workspace_id == msg.workspace_id {
        return Some(label);
    }
    match workspace_ref(store, sender.workspace_id) {
        Some(origin) => Some(format!("{origin} {label}")),
        // The instance row resolved but its workspace or repo row didn't
        // (an inconsistent DB — `delete_workspace` clears `workspace_agents`
        // before `workspaces`, so a normal archive can't reach this): the
        // bare label is still better than dropping the sender entirely.
        None => Some(label),
    }
}

/// `<repo>/<slug>` for a workspace id, or None if either row is missing.
fn workspace_ref(store: &Store, ws: crate::data::store::WorkspaceId) -> Option<String> {
    let w = store.workspace_by_id(ws).ok()??;
    let repo = store
        .repos()
        .ok()?
        .into_iter()
        .find(|r| r.id == w.repo_id)?;
    Some(format!("{}/{}", repo.name, w.name))
}

impl crate::app::App {
    /// Deliver all undelivered inbox messages into their target sessions
    /// (spawning on demand). Best-effort; called from the tick when an
    /// external DB commit is detected. Never blocks: the actual injection is
    /// a detached task because `send_text_when_settled` may wait seconds.
    ///
    /// Messages are grouped by target so that two messages to the same agent
    /// are delivered sequentially (in id/FIFO order) in a single detached
    /// task, preventing interleaving in the PTY.
    ///
    /// Outcome semantics per target:
    /// - `Ok(Ok)` + session found  → spawn one task, mark all delivered
    ///   optimistically on spawn (best-effort; a crash before the task drains
    ///   can lose an injection — acceptable for a long-lived TUI).
    /// - `Ok(AgentMissing)`        → binary not installed; drop (mark
    ///   delivered) so we never retry against a never-installable agent.
    /// - `Err(_)` (transient)      → leave pending; next external-change tick
    ///   retries. Do NOT mark delivered.
    /// - `Ok(Ok)` but no session   → leave pending to retry rather than
    ///   silently dropping (shouldn't happen right after a successful ensure).
    pub(crate) fn drain_agent_messages(&mut self) {
        let pending = match self.store.undelivered_messages() {
            Ok(p) => p,
            Err(_) => return, // transient; retry next external-change tick
        };
        if pending.is_empty() {
            return;
        }

        // Group by target, PRESERVING id order within each group
        // (undelivered_messages is ORDER BY id ASC). A Vec-of-(target,
        // Vec<msg>) keeps insertion order.
        let mut groups: Vec<(crate::data::store::AgentInstanceId, Vec<AgentMessage>)> = Vec::new();
        for msg in pending {
            match groups.iter_mut().find(|(t, _)| *t == msg.target_agent_id) {
                Some((_, v)) => v.push(msg),
                None => groups.push((msg.target_agent_id, vec![msg])),
            }
        }

        for (target, msgs) in groups {
            // Resolve the target session ONCE per target. Quiet
            // (surface_missing=false) so a missing binary doesn't pop a modal
            // over the user's unrelated view.
            let session = match crate::app::ensure_instance_session(self, target, false) {
                Ok(crate::app::AttachReady::Ok) => self.session_for(target),
                Ok(crate::app::AttachReady::AgentMissing) => {
                    // Binary not installed: drop these messages (mark
                    // delivered) so we don't retry forever.
                    for m in &msgs {
                        let _ = self.store.mark_delivered(m.id);
                    }
                    continue;
                }
                Err(e) => {
                    // TRANSIENT failure (DB lock, PTY alloc, etc.): leave the
                    // messages pending so the next external-change tick retries.
                    // Do NOT mark delivered.
                    tracing::warn!(
                        error = %e,
                        target = target.0,
                        "deliver: ensure session failed; will retry"
                    );
                    continue;
                }
            };
            let Some(session) = session else {
                // Ok(Ok) but no session (shouldn't happen right after a
                // successful ensure): leave pending to retry rather than
                // silently dropping.
                continue;
            };

            // Build one banner per message (FIFO), then deliver them
            // SEQUENTIALLY in a single detached task so two messages to the
            // same target can't interleave in the PTY. Order is preserved
            // (id order). Messages are marked delivered OPTIMISTICALLY here
            // (on spawn, not on completion) — best-effort: if wsx exits
            // before the task drains, an injection can be lost. Acceptable
            // for a long-lived TUI delivering agent prompts.
            let banners: Vec<String> = msgs
                .iter()
                .map(|m| {
                    let from = sender_label(&self.store, m);
                    delivery_banner(from.as_deref(), &m.body)
                })
                .collect();
            let sess = session.clone();
            tokio::spawn(async move {
                for b in banners {
                    // The `false` (timed out, nothing written) case still can't
                    // be acted on here: the messages were already marked
                    // delivered below. Wiring the outcome back so a failed
                    // injection retries is the next commit.
                    let _ = sess.send_text_when_settled(&b, 400, 5_000).await;
                }
            });
            for m in &msgs {
                let _ = self.store.mark_delivered(m.id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;

    #[test]
    fn banner_tags_sender() {
        assert_eq!(
            delivery_banner(Some("claude#2"), "hi"),
            "[message from claude#2]\nhi"
        );
        assert_eq!(delivery_banner(None, "hi"), "[message]\nhi");
    }

    #[test]
    fn sender_label_qualifies_a_cross_workspace_origin() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "workspacex", "wsx")
            .unwrap();
        let mk = |name: &str, path: &str| {
            store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name,
                    branch: &format!("wsx/{name}"),
                    worktree_path: std::path::Path::new(path),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap()
        };
        let origin = mk("parent-task", "/tmp/r/parent-task");
        let child = mk("child-task", "/tmp/r/child-task");
        let sender = store
            .add_primary_agent(origin, AgentKind::Claude, 1)
            .unwrap();
        let target = store
            .add_primary_agent(child, AgentKind::Claude, 1)
            .unwrap();

        // A handoff: enqueued against the TARGET's workspace, sent from `origin`.
        store
            .enqueue_message(child, target.id, Some(sender.id), "TASK: build it")
            .unwrap();
        let msg = store.undelivered_messages().unwrap().pop().unwrap();

        let label = sender_label(&store, &msg);
        assert_eq!(label.as_deref(), Some("workspacex/parent-task claude"));
        assert_eq!(
            delivery_banner(label.as_deref(), "TASK: build it"),
            "[message from workspacex/parent-task claude]\nTASK: build it"
        );
    }

    #[test]
    fn sender_label_resolves_originating_instance() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "wsx/w",
                worktree_path: std::path::Path::new("/tmp/r/w"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        // Primary claude (ordinal 1 -> bare label "claude") is the sender;
        // a codex instance is the target.
        let sender = store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        let target = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();
        store
            .enqueue_message(ws, target.id, Some(sender.id), "review please")
            .unwrap();
        let msg = store.undelivered_messages().unwrap().pop().unwrap();
        assert_eq!(sender_label(&store, &msg).as_deref(), Some("claude"));

        // A message with no originating instance (human/CLI origin) yields None.
        store.enqueue_message(ws, target.id, None, "hi").unwrap();
        let from_cli = store
            .undelivered_messages()
            .unwrap()
            .into_iter()
            .find(|m| m.from_agent_id.is_none())
            .unwrap();
        assert_eq!(sender_label(&store, &from_cli), None);
    }
}
