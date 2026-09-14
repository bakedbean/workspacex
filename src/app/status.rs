//! Turning raw session and reported state into the status the dashboard
//! shows: whether an agent is working, waiting, or stopped, and whether
//! what it last reported is still fresh enough to trust.

use super::*;

impl App {
    /// If the workspace has any pending tool_use that is a real permission
    /// prompt (NOT AskUserQuestion / ExitPlanMode, which are question tools
    /// surfaced separately as AwaitingAnswer, and NOT Agent subagent
    /// dispatches, which run for minutes by design), return the oldest
    /// pending tool's (name, first-seen epoch ms). Returns None otherwise.
    ///
    /// 3 seconds is well past the latency of any auto-approved tool, so a
    /// pending entry that crosses that threshold is usually waiting on a
    /// permission prompt — but the classifier additionally suppresses this
    /// signal when the PTY is still actively streaming (see
    /// `Status::classify`) to avoid false positives from long-running
    /// shell commands.
    pub fn awaiting_permission(
        &self,
        ws_id: crate::data::store::WorkspaceId,
    ) -> Option<(String, i64)> {
        let evt = self.workspace_events.get(&ws_id)?;
        let now = crate::util::time::now_ms();
        evt.pending_permission_tool(now, 3_000)
    }

    /// Assemble the PM digest from caches the dashboard already maintains.
    pub fn build_pm_digest(&self) -> Vec<crate::ui::pm_pane::RepoDigest> {
        // Event time (`evt.latest.timestamp_ms`), NOT `last_log_activity_ms`
        // — the latter is the wall-clock time wsx observed the JSONL grow,
        // which gets stamped to "now" for every workspace on the initial
        // tail pass after a wsx restart. Mirrors the detail bar's own
        // activity-timestamp logic (see `app/render.rs`).
        let last_activity: std::collections::HashMap<_, _> = self
            .workspace_events
            .iter()
            .filter_map(|(id, e)| e.latest.as_ref().map(|latest| (*id, latest.timestamp_ms)))
            .collect();
        crate::ui::pm_pane::build_digest(&crate::ui::pm_pane::DigestInputs {
            repos: &self.repos,
            workspaces: &self.workspaces,
            recaps: &self.recaps,
            pushed_status: &self.pushed_status,
            git: &self.workspace_status,
            pr_lifecycle: &self.pr_lifecycle,
            pr_number: &self.pr_number,
            last_activity_ms: &last_activity,
            filter: self.pm_filter.as_deref(),
        })
    }

    /// Permission prompts still awaiting an answer, keyed by workspace id.
    /// Shared by the updates-panel renderer and key handler so both derive
    /// row text — and the filter's match target — from identical inputs.
    pub fn awaiting_permission_map(
        &self,
    ) -> std::collections::HashMap<crate::data::store::WorkspaceId, (String, i64)> {
        self.workspaces
            .iter()
            .filter_map(|(_, w)| self.awaiting_permission(w.id).map(|a| (w.id, a)))
            .collect()
    }

    /// Classify every workspace into the canonical `Status` vocabulary,
    /// keyed by workspace id. Shared by the updates-panel renderer and key
    /// handler so both derive row order from identical inputs.
    pub fn classified_statuses(
        &self,
    ) -> std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::ui::dashboard::status::Status,
    > {
        self.workspaces
            .iter()
            .map(|(_, w)| (w.id, self.classify_status(w)))
            .collect()
    }

    /// Classify a workspace into the V5 dashboard `Status` vocabulary.
    /// Combines session liveness, JSONL stopped/stalled signals, and
    /// pending tool_use into one canonical state used by the renderer.
    pub fn classify_status(
        &self,
        ws: &crate::data::store::Workspace,
    ) -> crate::ui::dashboard::status::Status {
        let session = self
            .primary_instance(ws.id)
            .and_then(|i| self.sessions.get(i));
        let running = session.as_ref().is_some_and(|s| {
            matches!(
                *s.status.read().unwrap(),
                crate::pty::session::SessionStatus::Running { .. }
            )
        });
        // Returns `None` (not `Some(0)`) when the session is attached but
        // no PTY output has been observed yet, so `Status::classify`'s
        // PTY-active guard treats it as "unknown" rather than "fresh
        // output" — otherwise a permission prompt that fires before the
        // first PTY byte would be misclassified as Thinking.
        let secs = session.as_ref().and_then(|s| s.idle_secs());
        // `has_prior_session` is dead input in V5: `Status::classify`
        // collapses prior-session and no-session to Idle either way, so
        // don't pay `has_prior_session_for`'s filesystem I/O (canonicalize
        // + read_dir) per workspace here — this classifier runs on every
        // render tick and every updates-panel keypress. Spawn-mode
        // detection (`build_spawn_info`) still probes the filesystem itself.
        let has_prior = false;
        let now_ms = crate::util::time::now_ms();
        let stopped_kind = self
            .workspace_events
            .get(&ws.id)
            .and_then(derive_stopped_kind);
        let stalled = self
            .workspace_events
            .get(&ws.id)
            .is_some_and(|e| e.is_stalled(now_ms, 60_000));
        let awaiting = self.awaiting_permission(ws.id).is_some();
        let user_has_prompted = self
            .workspace_events
            .get(&ws.id)
            .is_some_and(|e| e.first_user_text.is_some());
        let last_log_activity = self
            .workspace_events
            .get(&ws.id)
            .map(|e| e.last_log_activity_ms)
            .unwrap_or(0);
        let reported = fresh_reported_state(self.pushed_status.get(&ws.id), last_log_activity);
        crate::ui::dashboard::status::Status::classify(
            awaiting,
            stopped_kind,
            stalled,
            secs,
            running,
            user_has_prompted,
            has_prior,
            reported,
        )
    }

    /// The freshness-gated agent-pushed status for a workspace, or `None` when
    /// there is no fresh push. Same liveness rule as the status classifier, so
    /// the message and the glyph appear/disappear together.
    pub fn fresh_reported_status(
        &self,
        ws_id: crate::data::store::WorkspaceId,
    ) -> Option<&crate::data::store::ReportedStatus> {
        let last_log_activity = self
            .workspace_events
            .get(&ws_id)
            .map(|e| e.last_log_activity_ms)
            .unwrap_or(0);
        fresh_reported(self.pushed_status.get(&ws_id), last_log_activity)
    }
}

/// Derive the StoppedKind for a workspace based on its WorkspaceEvents.
/// Returns Some when the agent is paused waiting on the user (either
/// mid-turn with a pending question tool, or end-of-turn with a
/// trailing question / completion).
pub(crate) fn derive_stopped_kind(
    e: &crate::activity::events::WorkspaceEvents,
) -> Option<StoppedKind> {
    // Question tools fire even without a terminal stop_reason — the model
    // is mid-turn but has explicitly asked the user something.
    if e.pending_question_tool().is_some() {
        return Some(StoppedKind::AwaitingAnswer);
    }
    // A user-initiated interrupt mid-tool-call ends the turn from the
    // agent's perspective: it was told to stop. Claude Code logs this as
    // a synthetic user text block but never emits a follow-up end_turn,
    // so without this branch the session drifts into Stalled after 60s.
    if e.last_user_interrupted {
        return Some(StoppedKind::Complete);
    }
    if !e.is_awaiting_user() {
        return None;
    }
    if e.last_text_ends_with_question() {
        Some(StoppedKind::AwaitingAnswer)
    } else {
        Some(StoppedKind::Complete)
    }
}

/// Decide whether a pushed status is still authoritative, returning the full
/// record. For snapshot states the push wins while no JSONL activity has
/// happened strictly after it; once the log grows past `reported_at`, the agent
/// has acted since reporting and the heuristic re-arms. `last_log_activity_ms`
/// of 0 means "no log activity observed", which never contradicts a push.
///
/// `Busy` is the exception: it is exempt from the gate entirely (it predicts
/// future log growth from background work rather than snapshotting the present)
/// and stays authoritative until the next hook push supersedes it. See the
/// in-body comment for the rationale.
pub(crate) fn fresh_reported(
    reported: Option<&crate::data::store::ReportedStatus>,
    last_log_activity_ms: i64,
) -> Option<&crate::data::store::ReportedStatus> {
    use crate::data::store::ReportedState;
    let r = reported?;
    // `Busy` is exempt from the freshness gate: it explicitly means background
    // work (subagents / shell tasks) is in flight, which legitimately grows the
    // main transcript — a completing subagent writes its result notification
    // back into the session log — *without* the agent being done. Gating it on
    // log growth (as every snapshot state correctly is) would drop the push the
    // instant a sibling subagent finishes, flipping the workspace to ✓ complete
    // mid-work. It is superseded by the next hook push (a `Stop` reporting Done
    // once `background_tasks` empties, or a `UserPromptSubmit` reporting
    // Working), and `classify` falls back to the heuristic if the session dies.
    if r.state == ReportedState::Busy {
        return Some(r);
    }
    if r.reported_at >= last_log_activity_ms {
        Some(r)
    } else {
        None
    }
}

/// The freshness-gated reported *state* (convenience over `fresh_reported`).
pub(crate) fn fresh_reported_state(
    reported: Option<&crate::data::store::ReportedStatus>,
    last_log_activity_ms: i64,
) -> Option<crate::data::store::ReportedState> {
    fresh_reported(reported, last_log_activity_ms).map(|r| r.state)
}

#[cfg(test)]
mod derive_stopped_kind_tests {
    use super::*;
    use crate::activity::events::{StopReason, WorkspaceEvents};

    #[test]
    fn returns_none_when_idle() {
        let evt = WorkspaceEvents::default();
        assert_eq!(derive_stopped_kind(&evt), None);
    }

    #[test]
    fn awaiting_answer_when_question_tool_pending_mid_turn() {
        // AskUserQuestion is in flight: stop_reason is ToolUse (so
        // is_awaiting_user() returns false), but the question tool is in
        // pending_tool_uses. Should still classify as AwaitingAnswer.
        let mut evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            ..Default::default()
        };
        evt.pending_tool_uses
            .insert("t1".into(), ("AskUserQuestion".into(), 0));
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }

    #[test]
    fn awaiting_answer_when_exit_plan_mode_pending_mid_turn() {
        let mut evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            ..Default::default()
        };
        evt.pending_tool_uses
            .insert("t1".into(), ("ExitPlanMode".into(), 0));
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }

    #[test]
    fn complete_when_end_turn_with_no_question_signal() {
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            user_replied_since_stop: false,
            last_assistant_text: Some("Done.".into()),
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::Complete));
    }

    #[test]
    fn awaiting_answer_when_end_turn_with_trailing_question() {
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            user_replied_since_stop: false,
            last_assistant_text: Some("Want me to also handle X?".into()),
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }

    #[test]
    fn none_when_user_has_already_replied() {
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::EndTurn),
            user_replied_since_stop: true,
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), None);
    }

    #[test]
    fn complete_when_user_interrupted_mid_tool_use() {
        // The exact failure case observed in the lively-myrtle session:
        // last assistant emitted a Bash tool_use (stop_reason=tool_use),
        // tool resolved, then the human hit interrupt. Without the
        // interrupt branch wsx falls through to Stalled after 60s; with
        // it, this is Complete (the agent was told to stop).
        let evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            last_user_interrupted: true,
            ..Default::default()
        };
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::Complete));
    }

    #[test]
    fn awaiting_answer_still_wins_over_interrupt_if_question_tool_pending() {
        // Edge case: interrupt fires while an AskUserQuestion is in
        // flight. The pending question tool should take precedence —
        // there's a real question to answer.
        let mut evt = WorkspaceEvents {
            last_stop_reason: Some(StopReason::ToolUse),
            last_user_interrupted: true,
            ..Default::default()
        };
        evt.pending_tool_uses
            .insert("t1".into(), ("AskUserQuestion".into(), 0));
        assert_eq!(derive_stopped_kind(&evt), Some(StoppedKind::AwaitingAnswer));
    }

    #[test]
    fn reset_detail_scroll_zeroes_offsets_on_workspace_change() {
        use crate::data::store::WorkspaceId;
        let mut offsets = [3u16, 7, 0, 2];
        let mut last = Some(WorkspaceId(100));

        super::reset_detail_scroll_on_workspace_change(
            &mut offsets,
            &mut last,
            Some(WorkspaceId(200)),
        );

        assert_eq!(offsets, [0; 4]);
        assert_eq!(last, Some(WorkspaceId(200)));
    }

    #[test]
    fn reset_detail_scroll_preserves_offsets_on_same_workspace() {
        use crate::data::store::WorkspaceId;
        let mut offsets = [3u16, 7, 0, 2];
        let mut last = Some(WorkspaceId(100));

        super::reset_detail_scroll_on_workspace_change(
            &mut offsets,
            &mut last,
            Some(WorkspaceId(100)),
        );

        assert_eq!(offsets, [3, 7, 0, 2]);
        assert_eq!(last, Some(WorkspaceId(100)));
    }

    #[test]
    fn reset_detail_scroll_handles_initial_none_to_some() {
        use crate::data::store::WorkspaceId;
        // App starts with detail_scroll_last_workspace = None and offsets
        // already zero; first draw with a selected workspace should update
        // the sentinel even though the offsets are technically unchanged.
        let mut offsets = [5u16, 0, 0, 0]; // seeded non-zero
        let mut last: Option<WorkspaceId> = None;

        super::reset_detail_scroll_on_workspace_change(
            &mut offsets,
            &mut last,
            Some(WorkspaceId(42)),
        );

        assert_eq!(offsets, [0; 4]);
        assert_eq!(last, Some(WorkspaceId(42)));
    }
}

#[cfg(test)]
mod reported_freshness_tests {
    use super::{fresh_reported, fresh_reported_state};
    use crate::data::store::{ReportedState, ReportedStatus};

    fn status(at: i64) -> ReportedStatus {
        ReportedStatus {
            state: ReportedState::Done,
            message: None,
            source: "model".into(),
            reported_at: at,
        }
    }

    #[test]
    fn push_newer_than_last_log_activity_is_fresh() {
        assert_eq!(
            fresh_reported_state(Some(&status(1000)), 900),
            Some(ReportedState::Done)
        );
        assert_eq!(
            fresh_reported_state(Some(&status(1000)), 1000),
            Some(ReportedState::Done)
        );
    }

    #[test]
    fn jsonl_activity_after_push_re_arms_heuristic() {
        assert_eq!(fresh_reported_state(Some(&status(1000)), 1500), None);
    }

    #[test]
    fn no_push_is_none() {
        assert_eq!(fresh_reported_state(None, 1500), None);
    }

    #[test]
    fn busy_survives_log_growth_from_background_work() {
        // `Busy` means background work (subagents / shell tasks) is in flight.
        // That work legitimately grows the main transcript — a completing
        // subagent writes its result notification back into the session log —
        // *without* the agent being done. So a `Busy` push must NOT be gated
        // out when `last_log_activity_ms` advances past it; otherwise the
        // workspace flips to ✓ complete mid-work in the window before the next
        // `Stop` re-pushes `Busy`. Every other state still re-arms normally.
        let busy = ReportedStatus {
            state: ReportedState::Busy,
            message: None,
            source: "hook".into(),
            reported_at: 1000,
        };
        assert_eq!(
            fresh_reported_state(Some(&busy), 1500),
            Some(ReportedState::Busy),
            "Busy stays authoritative even after the log grows"
        );
        assert!(fresh_reported(Some(&busy), 1500).is_some());
    }

    #[test]
    fn fresh_reported_returns_ref_on_tie_and_none_after() {
        let s = status(1000);
        // tie: reported_at == last_log_activity_ms -> still fresh, returns the ref
        assert!(fresh_reported(Some(&s), 1000).is_some());
        assert!(fresh_reported(Some(&s), 900).is_some());
        // log grew after the push -> stale
        assert!(fresh_reported(Some(&s), 1500).is_none());
        // no push -> none
        assert!(fresh_reported(None, 1500).is_none());
    }
}
