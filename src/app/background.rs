// background — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)

#[cfg(test)]
use crate::app::App;
use crate::app::SharedApp;
use crate::data::store::WorkspaceId;

/// Tail the agent session JSONL for `id` and merge any new events into
/// `App::workspace_events`. Shared by `branch_drift_poll` (the periodic
/// 2s poll) and the detach handlers (which spawn this immediately on
/// return-to-dashboard so the detail bar reflects work done in the
/// just-detached session without waiting for the next tick).
///
/// Callers pass `worktree_path` + `ws_agent` directly so this helper
/// doesn't have to walk `App::workspaces` (O(n) lookup that would make
/// the poll's per-tick work O(n²) over the workspace list).
///
/// Lock-ordering: snapshot path/offset under brief locks, do the file
/// I/O without the lock held, then re-acquire to commit the update.
///
/// Concurrent-tail safety: this helper can race with itself (the periodic
/// poll's awaited call and a detach-driven spawn for the same workspace
/// can interleave). The commit block re-checks `evt.file_path` and
/// `evt.byte_offset` against the values seen at snapshot time and skips
/// the update if either has changed — the winning tail already absorbed
/// the bytes we read, so applying our update on top would double-count
/// events and tool-use metrics. The next periodic tick picks up anything
/// past the newer offset.
pub async fn tail_workspace_events(
    app: SharedApp,
    id: crate::data::store::WorkspaceId,
    worktree_path: std::path::PathBuf,
    ws_agent: crate::pty::session::AgentKind,
) {
    if !worktree_path.exists() {
        return;
    }
    let current_file = match ws_agent {
        crate::pty::session::AgentKind::Claude => {
            crate::activity::events::locate_session_file(&worktree_path)
        }
        crate::pty::session::AgentKind::Pi => {
            crate::activity::pi_events::locate_session_file(&worktree_path)
        }
        crate::pty::session::AgentKind::Hermes => {
            crate::activity::hermes_events::locate_session_file(&worktree_path)
        }
        crate::pty::session::AgentKind::Codex => {
            crate::activity::codex_events::locate_session_file(&worktree_path)
        }
    };
    // Snapshot the FULL (file_path, byte_offset) pair so the commit can
    // detect a concurrent tail that landed between our snapshot and now.
    let (snapshot_file, snapshot_offset) = {
        let g = app.lock().await;
        match g.workspace_events.get(&id) {
            Some(evt) => (evt.file_path.clone(), evt.byte_offset),
            None => (None, 0),
        }
    };
    // The byte we actually tail from: reuse the prior offset only when
    // the snapshot's file matches the current session file; otherwise
    // start fresh (file rotated, first observation, etc.).
    let tail_from = match (snapshot_file.as_ref(), current_file.as_ref()) {
        (Some(p), Some(c)) if p == c => snapshot_offset,
        _ => 0,
    };
    let Some(file) = current_file else {
        return;
    };
    let tail_result = match ws_agent {
        crate::pty::session::AgentKind::Claude => {
            crate::activity::events::tail_session(&file, tail_from).map_err(Into::into)
        }
        crate::pty::session::AgentKind::Pi => {
            crate::activity::pi_events::tail_session(&file, tail_from).map_err(Into::into)
        }
        crate::pty::session::AgentKind::Hermes => {
            crate::activity::hermes_events::tail_session(&file, tail_from)
        }
        crate::pty::session::AgentKind::Codex => {
            crate::activity::codex_events::tail_session(&file, tail_from).map_err(Into::into)
        }
    };
    let Ok(update) = tail_result else {
        return;
    };
    let crate::activity::events::TailUpdate {
        new_offset,
        events,
        tool_use_starts,
        tool_use_resolves,
        last_stop_reason,
        human_replied_after_last_stop,
        reset_from_zero,
        last_assistant_text,
        longest_assistant_text_in_batch,
        last_user_interrupted,
        first_user_text,
        tool_use_counts,
        edited_file_paths,
        context_tokens,
        model_id,
        current_action,
        pending_question_text,
    } = update;
    let mut g = app.lock().await;
    // Concurrent-tail guard. If another `tail_workspace_events` call
    // for the same workspace committed between our snapshot and now,
    // its commit already advanced `byte_offset` (and may have replaced
    // `file_path`). Our `update` was computed against the snapshot —
    // applying it on top would re-add the same events and double-count
    // tool-use metrics. Skip; the next periodic tick re-snapshots and
    // catches any bytes past the newer offset.
    let still_at_snapshot = match g.workspace_events.get(&id) {
        Some(evt) => evt.file_path == snapshot_file && evt.byte_offset == snapshot_offset,
        None => snapshot_file.is_none() && snapshot_offset == 0,
    };
    if !still_at_snapshot {
        return;
    }
    let evt = g.workspace_events.entry(id).or_default();
    // If the session file was replaced (different path) or
    // truncated/rewound (reset_from_zero), discard all
    // session-derived state before applying the new batch.
    // Otherwise stale tool_uses or stop_reasons from the
    // prior session keep the dashboard stuck on "awaiting".
    let file_changed = evt.file_path.as_deref() != Some(file.as_path());
    if file_changed || reset_from_zero {
        evt.reset_session_state();
    }
    if new_offset != tail_from {
        // The log grew this iteration — stamp the activity marker so
        // is_stalled can compute time-since-last-write.
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        evt.last_log_activity_ms = now_ms;
    }
    evt.file_path = Some(file);
    evt.byte_offset = new_offset;
    for (tu_id, tu_name, ts) in tool_use_starts {
        evt.pending_tool_uses.insert(tu_id, (tu_name, ts));
    }
    for tu_id in tool_use_resolves {
        evt.pending_tool_uses.remove(&tu_id);
    }
    // Update the "agent is waiting on user" tracking.
    // - A fresh assistant stop_reason replaces the prior one
    //   and resets the user-replied latch (the agent just
    //   produced a new stopping point).
    // - `human_replied_after_last_stop` from this batch
    //   already accounts for within-batch ordering: it's set
    //   only if real user text appears AFTER the last
    //   stop_reason in the batch (or anywhere in the batch
    //   if there's no new stop_reason).
    // Recap pipeline: record this batch's longest assistant text into
    // the per-turn accumulator BEFORE handling stop_reason, so that if
    // the same batch contains both the recap text and the end_turn
    // marker, the snapshot sees the latest accumulator.
    if let Some(text) = &longest_assistant_text_in_batch {
        evt.record_batch_longest_text(text);
    }
    if let Some(sr) = last_stop_reason {
        let terminal = sr.is_awaiting_user();
        evt.last_stop_reason = Some(sr);
        evt.user_replied_since_stop = false;
        if terminal {
            // Snapshot the just-completed turn's recap through
            // clean_recap; the SESSION SUMMARY column reads this field.
            evt.snapshot_recap_at_turn_end();
            // The live action label is turn-scoped: once the agent finishes a
            // turn (awaiting user), drop it so the next turn's Thinking phase
            // doesn't surface the previous turn's "now …"/command as if live.
            evt.current_action = None;
        }
    }
    if human_replied_after_last_stop {
        evt.user_replied_since_stop = true;
    }
    if let Some(text) = last_assistant_text {
        evt.last_assistant_text = Some(text);
    }
    if evt.first_user_text.is_none() {
        if let Some(t) = first_user_text {
            evt.first_user_text = Some(t);
        }
    }
    evt.tool_use_counts.read = evt
        .tool_use_counts
        .read
        .saturating_add(tool_use_counts.read);
    evt.tool_use_counts.edit = evt
        .tool_use_counts
        .edit
        .saturating_add(tool_use_counts.edit);
    evt.tool_use_counts.write = evt
        .tool_use_counts
        .write
        .saturating_add(tool_use_counts.write);
    evt.tool_use_counts.bash = evt
        .tool_use_counts
        .bash
        .saturating_add(tool_use_counts.bash);
    evt.tool_use_counts.other = evt
        .tool_use_counts
        .other
        .saturating_add(tool_use_counts.other);
    for path in edited_file_paths {
        evt.push_recent_edited_file(path);
    }
    if let Some(t) = context_tokens {
        evt.context_tokens = Some(t);
    }
    if let Some(m) = model_id {
        evt.model_id = Some(m);
    }
    if let Some(a) = current_action {
        evt.current_action = Some(a);
    }
    // Question topic: adopt a freshly-seen one, then clear it once the
    // question is no longer pending. `pending_tool_uses` was already
    // maintained above (tool_use_starts/resolves), so
    // `pending_question_tool()` reflects this batch.
    if let Some(q) = pending_question_text {
        evt.pending_question_text = Some(q);
    }
    if evt.pending_question_tool().is_none() {
        evt.pending_question_text = None;
    }
    // Sticky between batches: only overwrite when the batch
    // had a definitive signal. Some(true) = batch ended on
    // the interrupt sentinel; Some(false) = batch had a
    // newer assistant message or real user text overriding
    // it; None = batch was silent on this axis.
    if let Some(v) = last_user_interrupted {
        evt.last_user_interrupted = v;
    }
    for e in events {
        crate::activity::events::push_event(evt, e);
    }
    // First successful tail of this workspace's JSONL.
    // After this point the classifier sees the agent's
    // real stop_reason, so the bell loop can start
    // trusting activity transitions for this workspace.
    g.workspace_events_scanned.insert(id);
}

/// Periodically check each live workspace's current git branch against
/// the DB; if claude (or a user) renamed it, update name + branch in the
/// store. Runs forever; cheap when nothing has drifted.
pub async fn branch_drift_poll(app: SharedApp) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        let snapshot: Vec<(
            WorkspaceId,
            std::path::PathBuf,
            String,
            String,
            Option<String>,
            crate::pty::session::AgentKind,
        )> = {
            let g = app.lock().await;
            g.workspaces
                .iter()
                .filter_map(|(_, w)| {
                    let repo = g.repos.iter().find(|r| r.id == w.repo_id)?;
                    let prefix = crate::data::repo::resolve_branch_prefix(repo, &g.store)
                        .unwrap_or_default();
                    Some((
                        w.id,
                        w.worktree_path.clone(),
                        w.branch.clone(),
                        prefix,
                        repo.base_branch.clone(),
                        w.agent,
                    ))
                })
                .collect()
        };

        for (id, path, db_branch, prefix, base_branch, ws_agent) in snapshot {
            if !path.exists() {
                continue;
            }

            // 1) Branch drift (existing logic).
            if let Ok(current) = crate::git::current_branch(&path).await {
                if current != db_branch && current != "HEAD" {
                    let new_name = if prefix.is_empty() {
                        current.clone()
                    } else {
                        let strip = format!("{}/", prefix.trim_end_matches('/'));
                        current.strip_prefix(&strip).unwrap_or(&current).to_string()
                    };
                    let mut g = app.lock().await;
                    let _ = g.store.rename_workspace(id, &new_name);
                    let _ = g.store.set_workspace_branch(id, &current);
                    let _ = g.refresh();
                    // Invalidate cached PR state — the new branch may have a
                    // different (or no) PR. Clearing the throttle stamp
                    // makes the next tick poll immediately.
                    g.pr_lifecycle.remove(&id);
                    g.pr_number.remove(&id);
                    g.pr_last_poll_ms.remove(&id);
                    // New branch → different ancestry from `base_branch`,
                    // so the cached diff and its throttle stamp are
                    // stale. Drop them to force a fresh poll.
                    g.workspace_diff.remove(&id);
                    g.workspace_diff_per_file.remove(&id);
                    g.diff_last_poll_ms.remove(&id);
                }
            }

            // 2) Workspace status — refresh the cache for this workspace.
            if let Ok(status) = crate::git::workspace_status(&path).await {
                let mut g = app.lock().await;
                g.workspace_status.insert(id, status);
            }

            // 2b) Diff stats vs. base branch (for dashboard +N/-M column).
            //     Throttled to once per 10s per workspace: running
            //     `git diff --shortstat <base>...HEAD` on every 2s tick
            //     is wasteful on large repos and the column doesn't need
            //     sub-10s freshness.
            if let Some(base) = base_branch.as_deref() {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                let should_poll = {
                    let g = app.lock().await;
                    g.diff_last_poll_ms
                        .get(&id)
                        .map(|t| now_ms.saturating_sub(*t) >= 10_000)
                        .unwrap_or(true)
                };
                if should_poll {
                    {
                        let mut g = app.lock().await;
                        g.diff_last_poll_ms.insert(id, now_ms);
                    }
                    if let Some(diff) = crate::git::workspace_diff_stats(&path, base).await {
                        let mut g = app.lock().await;
                        g.workspace_diff.insert(id, diff);
                    }
                    if let Some(per_file) = crate::git::workspace_diff_per_file(&path, base).await {
                        let mut g = app.lock().await;
                        g.workspace_diff_per_file.insert(id, per_file);
                    }
                }
            }

            // 3) PR lifecycle — throttled to once per 30s per workspace.
            //    gh is a network call, so we don't run it every tick.
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let should_poll_pr = {
                let g = app.lock().await;
                g.pr_last_poll_ms
                    .get(&id)
                    .map(|t| now_ms.saturating_sub(*t) >= 30_000)
                    .unwrap_or(true)
            };
            if should_poll_pr {
                // Mark the attempt before awaiting the fetch, so concurrent
                // ticks don't queue up multiple gh processes.
                {
                    let mut g = app.lock().await;
                    g.pr_last_poll_ms.insert(id, now_ms);
                }
                if let Ok(Some(status)) =
                    crate::git::forge::fetch_pr_status(&path, &db_branch).await
                {
                    let mut g = app.lock().await;
                    g.pr_lifecycle.insert(id, status.lifecycle);
                    match status.number {
                        Some(n) => {
                            g.pr_number.insert(id, n);
                        }
                        None => {
                            g.pr_number.remove(&id);
                        }
                    }
                }
                // Ok(None) → leave any existing cached value alone; better
                // than clobbering a previously-known state on a transient
                // network error.
            }

            // 4) Tail agent session JSONL for events.
            //    Extracted into `tail_workspace_events` so detach handlers
            //    can trigger an immediate refresh on return-to-dashboard
            //    without waiting for the next poll tick. Path/agent are
            //    passed from the snapshot above so the helper doesn't
            //    re-walk `App::workspaces` (would make this loop O(n²)).
            tail_workspace_events(app.clone(), id, path.clone(), ws_agent).await;
        }

        // 5) Per-workspace process scan. Throttled to once per 10 s globally —
        //    lsof returns everything in a single call, so we don't pay per-workspace.
        let should_scan = {
            let g = app.lock().await;
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            now_ms.saturating_sub(g.last_proc_scan_ms) >= 10_000
        };
        if should_scan {
            let procs = crate::activity::proc::scan().await;
            let worktrees: Vec<(crate::data::store::WorkspaceId, std::path::PathBuf)> = {
                let g = app.lock().await;
                g.workspaces
                    .iter()
                    .map(|(_, w)| (w.id, w.worktree_path.clone()))
                    .collect()
            };
            let worktree_refs: Vec<(crate::data::store::WorkspaceId, &std::path::Path)> = worktrees
                .iter()
                .map(|(id, path)| (*id, path.as_path()))
                .collect();
            let bucketed = crate::activity::proc::bucket_by_worktree(&procs, &worktree_refs);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let mut g = app.lock().await;
            g.workspace_processes = bucketed;
            g.last_proc_scan_ms = now_ms;
        }
    }
}

#[cfg(test)]
mod external_change_polling_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, Store};

    /// Simulates the bug from issue #70: the dashboard process is holding a
    /// snapshot of workspaces; a separate process (e.g. `wsx workspace
    /// create` driven by Claude during a related-repos flow) writes a new
    /// workspace to the same DB. `poll_external_changes` must pick it up.
    #[test]
    fn poll_external_changes_pulls_in_workspace_added_by_other_process() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = dir.path().join("wsx.db");

        // The "TUI" process: opens the store, starts the App.
        let store_tui = Store::open(&db).unwrap();
        let repo_id = store_tui
            .add_repo(std::path::Path::new("/work/backend"), "backend", "")
            .unwrap();
        let mut app = App::new(store_tui, std::path::PathBuf::from("/tmp/wsx-poll-test")).unwrap();
        assert!(app.workspaces.is_empty(), "no workspaces at startup");

        // The "CLI" process: separate connection, writes a new workspace.
        let store_cli = Store::open(&db).unwrap();
        store_cli
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "from-cli",
                branch: "backend/from-cli",
                worktree_path: std::path::Path::new("/wt/from-cli"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();

        // Back in the TUI: the next tick polls and must pick the new row up.
        let changed = app.poll_external_changes();
        assert!(changed, "external commit should trigger a refresh");
        assert_eq!(app.workspaces.len(), 1);
        assert_eq!(app.workspaces[0].1.name, "from-cli");

        // And a second poll with no further writes must be a no-op so we
        // don't churn refresh every tick.
        assert!(
            !app.poll_external_changes(),
            "idle poll must not trigger refresh"
        );
    }
}
