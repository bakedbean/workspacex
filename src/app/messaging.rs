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

/// How many times a message may fail to be injected before wsx stops retrying
/// it. Each attempt already waits `DELIVERY_TIMEOUT_MS` for the agent to become
/// ready, so exhausting the ceiling means the target has been unable to accept
/// input for many minutes.
pub(crate) const MAX_DELIVERY_ATTEMPTS: u32 = 5;

/// How long one injection waits for the target agent to be ready before giving
/// up and reporting failure.
///
/// Generously long on purpose. The wait is cheap (a detached task polling every
/// 50ms) and the common reasons for not being ready — a cold agent still
/// booting, or a live agent midway through a turn — resolve on their own. The
/// old 5s budget turned both into dropped messages.
const DELIVERY_TIMEOUT_MS: u64 = 120_000;

/// Quiet window the target's PTY must show before injecting, on top of
/// `ready_for_input`. Keeps a message from landing in the middle of a burst of
/// the agent's own output.
const DELIVERY_QUIET_MS: u64 = 400;

/// What one detached injection task reports back to the App loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeliveryOutcome {
    pub id: i64,
    /// Whether the text actually reached the PTY. `false` means the target
    /// never became ready — the message must stay queued.
    pub written: bool,
}

/// The queued messages worth dispatching right now: those with no injection
/// already in flight, and not past the attempt ceiling.
///
/// Exhausted messages are filtered out rather than marked delivered, so the row
/// survives for inspection instead of disappearing the way a dropped message
/// used to.
pub(crate) fn deliverable(
    pending: Vec<AgentMessage>,
    in_flight: &std::collections::HashSet<i64>,
    attempts: &std::collections::HashMap<i64, u32>,
) -> Vec<AgentMessage> {
    pending
        .into_iter()
        .filter(|m| !in_flight.contains(&m.id))
        .filter(|m| attempts.get(&m.id).copied().unwrap_or(0) < MAX_DELIVERY_ATTEMPTS)
        .collect()
}

/// Workspaces holding a queued message wsx has stopped trying to inject.
///
/// Derived from the queue itself rather than tracked alongside it: an exhausted
/// message is still an undelivered row, so `undelivered_messages` plus the
/// attempt counts is the whole story. That also means the flag clears itself
/// when the attempt counts reset (a wsx restart, which restarts the agents too).
pub(crate) fn stuck_workspaces(
    pending: &[AgentMessage],
    attempts: &std::collections::HashMap<i64, u32>,
) -> std::collections::HashSet<crate::data::store::WorkspaceId> {
    pending
        .iter()
        .filter(|m| attempts.get(&m.id).copied().unwrap_or(0) >= MAX_DELIVERY_ATTEMPTS)
        .map(|m| m.workspace_id)
        .collect()
}

impl crate::app::App {
    /// Apply the outcomes reported by finished injection tasks: mark the
    /// messages that actually landed as delivered, and count an attempt against
    /// the ones that didn't so they can be retried.
    ///
    /// Returns whether anything was applied, which is the run loop's signal to
    /// run `drain_agent_messages` again — a failed injection has to be
    /// redispatched by someone, and the external-change poll won't fire for it.
    pub(crate) fn apply_delivery_outcomes(&mut self) -> bool {
        let outcomes: Vec<DeliveryOutcome> = {
            let mut guard = self.delivery_outcomes.lock().unwrap();
            if guard.is_empty() {
                return false;
            }
            std::mem::take(&mut *guard)
        };
        for outcome in &outcomes {
            self.delivering.remove(&outcome.id);
            if outcome.written {
                let _ = self.store.mark_delivered(outcome.id);
                self.delivery_attempts.remove(&outcome.id);
                continue;
            }
            let attempts = self.delivery_attempts.entry(outcome.id).or_insert(0);
            *attempts += 1;
            if *attempts >= MAX_DELIVERY_ATTEMPTS {
                tracing::warn!(
                    id = outcome.id,
                    attempts = *attempts,
                    "deliver: giving up injecting agent message; it stays queued"
                );
            }
        }
        true
    }

    /// Deliver all undelivered inbox messages into their target sessions
    /// (spawning on demand). Best-effort; called from the tick when an
    /// external DB commit is detected or an injection reported back. Never
    /// blocks: the actual injection is a detached task because
    /// `send_text_when_settled` may wait minutes for the agent to be ready.
    ///
    /// Messages are grouped by target so that two messages to the same agent
    /// are delivered sequentially (in id/FIFO order) in a single detached
    /// task, preventing interleaving in the PTY.
    ///
    /// Nothing is marked delivered here. The injection task reports back
    /// through `delivery_outcomes` and `apply_delivery_outcomes` records the
    /// result — a message is only delivered once it has actually been written
    /// to the target's PTY.
    ///
    /// Outcome semantics per target:
    /// - `Ok(Ok)` + session found  → spawn one task, mark the ids in flight.
    /// - `Ok(AgentMissing)`        → binary not installed; drop (mark
    ///   delivered) so we never retry against a never-installable agent.
    /// - `Err(_)` (transient)      → leave pending; a later tick retries.
    ///   Do NOT mark delivered.
    /// - `Ok(Ok)` but no session   → leave pending to retry rather than
    ///   silently dropping (shouldn't happen right after a successful ensure).
    pub(crate) fn drain_agent_messages(&mut self) {
        let pending = match self.store.undelivered_messages() {
            Ok(p) => p,
            Err(_) => return, // transient; retry next external-change tick
        };
        // Recomputed before the dispatch filter, because the messages being
        // flagged are exactly the ones `deliverable` is about to drop.
        self.stuck_mail = stuck_workspaces(&pending, &self.delivery_attempts);
        let pending = deliverable(pending, &self.delivering, &self.delivery_attempts);
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
            // (id order).
            let items: Vec<(i64, String)> = msgs
                .iter()
                .map(|m| {
                    let from = sender_label(&self.store, m);
                    (m.id, delivery_banner(from.as_deref(), &m.body))
                })
                .collect();
            for m in &msgs {
                self.delivering.insert(m.id);
            }
            let sess = session.clone();
            let outcomes = self.delivery_outcomes.clone();
            tokio::spawn(async move {
                let mut failed = false;
                for (id, banner) in items {
                    // Once one injection fails, abandon the rest of this
                    // target's batch rather than writing them out of order:
                    // report them as unwritten so they are redispatched
                    // together, still in id order, on a later tick.
                    let written = if failed {
                        false
                    } else {
                        sess.send_text_when_settled(&banner, DELIVERY_QUIET_MS, DELIVERY_TIMEOUT_MS)
                            .await
                    };
                    failed |= !written;
                    outcomes
                        .lock()
                        .unwrap()
                        .push(DeliveryOutcome { id, written });
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;

    /// A store with one workspace, one primary agent, and `n` queued messages
    /// to it. Returns the App plus the queued message ids in FIFO order.
    fn app_with_queued_messages(n: usize) -> (crate::app::App, Vec<i64>) {
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
        let target = store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        for i in 0..n {
            store
                .enqueue_message(ws, target.id, None, &format!("msg {i}"))
                .unwrap();
        }
        let ids = store
            .undelivered_messages()
            .unwrap()
            .iter()
            .map(|m| m.id)
            .collect();
        let app = crate::app::App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
        (app, ids)
    }

    #[test]
    fn a_failed_injection_stays_queued_and_counts_an_attempt() {
        // The bug: the old code marked messages delivered when it SPAWNED the
        // injection task, so a `send_text_when_settled` that timed out without
        // writing lost the message permanently. A failed write must leave the
        // row undelivered so the next tick retries it.
        let (mut app, ids) = app_with_queued_messages(1);
        app.delivering.insert(ids[0]);
        app.delivery_outcomes.lock().unwrap().push(DeliveryOutcome {
            id: ids[0],
            written: false,
        });

        assert!(app.apply_delivery_outcomes(), "an outcome was applied");
        assert_eq!(
            app.store.undelivered_messages().unwrap().len(),
            1,
            "a message that was never written must stay queued"
        );
        assert_eq!(app.delivery_attempts.get(&ids[0]), Some(&1));
        assert!(
            !app.delivering.contains(&ids[0]),
            "no longer in flight, so the next drain can retry it"
        );
    }

    #[test]
    fn a_successful_injection_marks_the_message_delivered() {
        let (mut app, ids) = app_with_queued_messages(1);
        app.delivering.insert(ids[0]);
        app.delivery_outcomes.lock().unwrap().push(DeliveryOutcome {
            id: ids[0],
            written: true,
        });

        assert!(app.apply_delivery_outcomes());
        assert!(app.store.undelivered_messages().unwrap().is_empty());
        assert!(app.delivering.is_empty());
    }

    #[test]
    fn apply_delivery_outcomes_is_a_no_op_when_nothing_reported() {
        let (mut app, _ids) = app_with_queued_messages(1);
        assert!(
            !app.apply_delivery_outcomes(),
            "no outcomes reported means no redelivery work to do"
        );
    }

    #[test]
    fn stuck_workspaces_flags_only_the_attempt_exhausted() {
        let (app, ids) = app_with_queued_messages(2);
        let pending = app.store.undelivered_messages().unwrap();
        let ws = pending[0].workspace_id;
        let attempts =
            std::collections::HashMap::from([(ids[0], MAX_DELIVERY_ATTEMPTS), (ids[1], 1)]);

        assert_eq!(
            stuck_workspaces(&pending, &attempts),
            std::collections::HashSet::from([ws]),
        );
        assert!(
            stuck_workspaces(&pending, &std::collections::HashMap::new()).is_empty(),
            "a message still being retried is not stuck"
        );
    }

    #[test]
    fn deliverable_skips_messages_already_in_flight() {
        // `drain_agent_messages` runs on every tick while a delivery is
        // waiting for the agent to settle. Without this filter each tick would
        // spawn another injection task for the same row and the agent would
        // receive the message several times.
        let (app, ids) = app_with_queued_messages(2);
        let pending = app.store.undelivered_messages().unwrap();
        let in_flight = std::collections::HashSet::from([ids[0]]);

        let out = deliverable(pending, &in_flight, &std::collections::HashMap::new());

        assert_eq!(
            out.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![ids[1]],
            "the in-flight message must not be dispatched twice"
        );
    }

    #[test]
    fn deliverable_gives_up_after_the_attempt_ceiling() {
        let (app, ids) = app_with_queued_messages(2);
        let pending = app.store.undelivered_messages().unwrap();
        let attempts =
            std::collections::HashMap::from([(ids[0], MAX_DELIVERY_ATTEMPTS), (ids[1], 1)]);

        let out = deliverable(pending, &std::collections::HashSet::new(), &attempts);

        assert_eq!(
            out.iter().map(|m| m.id).collect::<Vec<_>>(),
            vec![ids[1]],
            "an exhausted message stops being retried, but is not dropped"
        );
    }

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
