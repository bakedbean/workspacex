// background — extracted from src/app.rs (see docs/superpowers/specs/2026-05-25-app-rs-refactor-design.md)

#[cfg(test)]
use crate::app::App;
use crate::app::SharedApp;
use crate::data::store::WorkspaceId;

/// Tail each recorded agent instance for `id`, keeping primary events in
/// `App::workspace_events` and peers in `App::agent_events`. Shared by
/// `branch_drift_poll` (the periodic 2s poll) and the detach handlers
/// (which spawn this immediately on return-to-dashboard so the detail bar
/// reflects the just-detached session without waiting for the next tick).
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
    // Read the store, not the render cache: hooks can record a new session
    // identity before the next App::refresh.
    let roster = {
        let g = app.lock().await;
        let Ok(roster) = g.store.workspace_agents(id) else {
            return;
        };
        roster
    };
    if roster.is_empty() {
        // Retain the legacy entrypoint for workspaces without instance rows.
        tail_instance_events(&app, id, &worktree_path, ws_agent, None, &roster).await;
    } else {
        for instance in &roster {
            tail_instance_events(
                &app,
                id,
                &worktree_path,
                instance.agent,
                Some(instance),
                &roster,
            )
            .await;
        }
    }
}

async fn tail_instance_events(
    app: &SharedApp,
    id: WorkspaceId,
    worktree_path: &std::path::Path,
    agent: crate::pty::session::AgentKind,
    instance: Option<&crate::data::agents::AgentInstance>,
    roster: &[crate::data::agents::AgentInstance],
) {
    let peer_id = instance.filter(|i| !i.is_primary).map(|i| i.id);
    // Snapshot before locating/reading the file. Never clone event histories.
    let (snapshot_file, snapshot_offset) = {
        let g = app.lock().await;
        let evt = match peer_id {
            Some(peer) => g.agent_events.get(&peer),
            None => g.workspace_events.get(&id),
        };
        evt.map(|evt| (evt.file_path.clone(), evt.byte_offset))
            .unwrap_or((None, 0))
    };
    let current_file = if !worktree_path.exists() {
        None
    } else if let Some(instance) = instance {
        let same_kind_count = roster.iter().filter(|i| i.agent == agent).count();
        crate::activity::locate_instance_session_file(instance, worktree_path, same_kind_count)
    } else {
        crate::activity::locate_session_file_for(agent, worktree_path)
    };
    // The byte we actually tail from: reuse the prior offset only when
    // the snapshot's file matches the current session file; otherwise
    // start fresh (file rotated, first observation, etc.).
    let tail_from = match (snapshot_file.as_ref(), current_file.as_ref()) {
        (Some(p), Some(c)) if p == c => snapshot_offset,
        _ => 0,
    };
    let tail_result = current_file
        .as_ref()
        .map(|file| crate::activity::tail_session_for(agent, file, tail_from));
    let mut g = app.lock().await;
    // Session hooks and roster changes can land while file I/O is running.
    // Recheck identity AND singleton eligibility before committing or clearing.
    // A newer recorded identity must never receive an older session's stats.
    if !g
        .store
        .workspace_agents(id)
        .is_ok_and(|current| current == roster)
    {
        return;
    }
    let cached = match peer_id {
        Some(peer) => g.agent_events.get(&peer),
        None => g.workspace_events.get(&id),
    };
    let still_at_snapshot = match cached {
        Some(evt) => evt.file_path == snapshot_file && evt.byte_offset == snapshot_offset,
        None => snapshot_file.is_none() && snapshot_offset == 0,
    };
    if !still_at_snapshot {
        return;
    }
    let Some(file) = current_file else {
        // Missing pinned files and ambiguous unpinned instances must not keep
        // model/usage from the session they used to resolve.
        // Keep primary initialization sticky so PTY/permission activity can
        // still advance after a previously scanned transcript disappears.
        if let Some(peer) = peer_id {
            g.agent_events.remove(&peer);
        } else {
            g.workspace_events.remove(&id);
        }
        return;
    };
    let Some(Ok(update)) = tail_result else {
        // Preserve an unchanged session on transient read errors (including
        // Hermes virtual paths), but never show the previous identity's usage.
        if snapshot_file.as_ref() != Some(&file) {
            if let Some(peer) = peer_id {
                g.agent_events.remove(&peer);
            } else {
                g.workspace_events.remove(&id);
            }
        }
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
    let evt = match peer_id {
        Some(peer) => g.agent_events.entry(peer).or_default(),
        None => g.workspace_events.entry(id).or_default(),
    };
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
        let now_ms = crate::util::time::now_ms();
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
    // Only a successful primary scan opens the cold-start activity gate.
    // Once initialized, keep tracking transitions even if this transcript
    // later disappears, becomes ambiguous, or fails to read after replacement.
    if peer_id.is_none() {
        g.workspace_events_scanned.insert(id);
    }
}

#[cfg(test)]
mod instance_event_tests {
    use super::*;
    use crate::app::activity::ActivityState;
    use crate::data::store::{AgentInstanceId, NewWorkspace, Store};
    use crate::pty::session::{AgentKind, SessionStatus};
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    const TRANSCRIPT: &str = include_str!("../../tests/fixtures/omp-session.jsonl");

    fn setup(
        worktree: &Path,
    ) -> (
        SharedApp,
        WorkspaceId,
        AgentInstanceId,
        AgentInstanceId,
        PathBuf,
        PathBuf,
    ) {
        let store = Store::open_in_memory().unwrap();
        let repo_id = store.add_repo(worktree, "events", "").unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "events",
                branch: "events",
                worktree_path: worktree,
                yolo: false,
                agent: AgentKind::Omp,
                shared: false,
            })
            .unwrap();
        let primary = store.add_primary_agent(ws, AgentKind::Omp, 0).unwrap();
        let peer = store.add_workspace_agent(ws, AgentKind::Omp).unwrap();
        let primary_file = worktree.join("primary.jsonl");
        let peer_file = worktree.join("peer.jsonl");
        std::fs::write(&primary_file, TRANSCRIPT).unwrap();
        std::fs::write(
            &peer_file,
            TRANSCRIPT
                .replace("gpt-5.6-sol", "peer-model")
                .replace("\"input\":357", "\"input\":1000"),
        )
        .unwrap();
        store
            .set_instance_agent_session(primary.id, primary_file.to_str().unwrap())
            .unwrap();
        store
            .set_instance_agent_session(peer.id, peer_file.to_str().unwrap())
            .unwrap();
        let app = App::new(store, worktree.to_path_buf()).unwrap();
        (
            Arc::new(Mutex::new(app)),
            ws,
            primary.id,
            peer.id,
            primary_file,
            peer_file,
        )
    }

    fn assistant_record(model: &str, tokens: u64, tool: &str) -> String {
        // Keep the real harness envelope and usage schema, varying only the
        // observable signals needed to distinguish sessions and new batches.
        let mut record = TRANSCRIPT
            .lines()
            .rev()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .find(|record| record["message"]["role"] == "assistant")
            .unwrap();
        record["message"]["model"] = model.into();
        record["message"]["usage"]["input"] = tokens.into();
        record["message"]["usage"]["cacheRead"] = 0.into();
        record["message"]["usage"]["cacheWrite"] = 0.into();
        record["message"]["content"] = serde_json::json!([{
            "type": "toolCall",
            "id": format!("{model}-{tool}"),
            "name": tool,
            "arguments": {}
        }]);
        format!("{record}\n")
    }

    fn append(path: &Path, record: &str) {
        std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .unwrap()
            .write_all(record.as_bytes())
            .unwrap();
    }

    fn draw_once(app: &mut App) {
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| crate::app::render::draw_for_test(frame, app))
            .unwrap();
    }

    #[tokio::test]
    async fn same_kind_instances_accumulate_independently_without_duplicate_polls() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, ws, _, peer, primary_file, peer_file) = setup(dir.path());
        // The passed workspace kind must not override the actual instance roster.
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Claude).await;
        {
            let g = app.lock().await;
            let primary_events = &g.workspace_events[&ws];
            let peer_events = &g.agent_events[&peer];
            assert_eq!(primary_events.model_id.as_deref(), Some("gpt-5.6-sol"));
            assert_eq!(primary_events.context_tokens, Some(16_485));
            assert_eq!(peer_events.model_id.as_deref(), Some("peer-model"));
            assert_eq!(peer_events.context_tokens, Some(17_128));
            assert_eq!(primary_events.log.len(), 4);
            assert_eq!(peer_events.log.len(), 4);
            assert!(g.workspace_events_scanned.contains(&ws));
        }
        append(
            &primary_file,
            &assistant_record("primary-next", 33_000, "bash"),
        );
        append(&peer_file, &assistant_record("peer-next", 44_000, "read"));
        tokio::join!(
            tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp),
            tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp),
        );
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        let g = app.lock().await;
        let primary_events = &g.workspace_events[&ws];
        let peer_events = &g.agent_events[&peer];
        assert_eq!(primary_events.log.len(), 5);
        assert_eq!(peer_events.log.len(), 5);
        assert_eq!(primary_events.model_id.as_deref(), Some("primary-next"));
        assert_eq!(primary_events.context_tokens, Some(33_000));
        assert_eq!(peer_events.model_id.as_deref(), Some("peer-next"));
        assert_eq!(peer_events.context_tokens, Some(44_000));
    }

    #[tokio::test]
    async fn peer_rollover_and_missing_identity_reset_only_that_instance() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, ws, _, peer, _, _) = setup(dir.path());
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        let replacement = dir.path().join("replacement.jsonl");
        std::fs::write(
            &replacement,
            assistant_record("replacement", 55_000, "read"),
        )
        .unwrap();
        {
            let g = app.lock().await;
            g.store
                .set_instance_agent_session(peer, replacement.to_str().unwrap())
                .unwrap();
            // Deliberately do not refresh: the recorded identity leads the
            // dashboard's cached agent_roster.
        }
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        {
            let g = app.lock().await;
            let events = &g.agent_events[&peer];
            assert_eq!(events.model_id.as_deref(), Some("replacement"));
            assert_eq!(events.context_tokens, Some(55_000));
            assert!(events.first_user_text.is_none());
        }
        // Rewind the same path: old context and model must reset too.
        std::fs::write(&replacement, "{}\n").unwrap();
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        {
            let g = app.lock().await;
            let events = &g.agent_events[&peer];
            assert_eq!(events.context_tokens, None);
            assert_eq!(events.model_id, None);
        }
        std::fs::write(
            &replacement,
            assistant_record("replacement", 55_000, "read"),
        )
        .unwrap();
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        std::fs::remove_file(&replacement).unwrap();
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        let g = app.lock().await;
        assert!(!g.agent_events.contains_key(&peer));
        assert_eq!(
            g.workspace_events[&ws].model_id.as_deref(),
            Some("gpt-5.6-sol")
        );
        assert_eq!(g.workspace_events[&ws].context_tokens, Some(16_485));
        assert_eq!(g.workspace_events[&ws].log.len(), 4);
        assert!(g.workspace_events_scanned.contains(&ws));
    }

    #[tokio::test]
    async fn missing_primary_does_not_mark_workspace_scanned_from_peer_events() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, ws, primary, peer, _, _) = setup(dir.path());
        app.lock()
            .await
            .store
            .set_instance_agent_session(primary, dir.path().join("missing.jsonl").to_str().unwrap())
            .unwrap();
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        let mut g = app.lock().await;
        assert!(!g.workspace_events.contains_key(&ws));
        g.test_spawn_session(primary, SessionStatus::Running { pid: 1 });
        draw_once(&mut g);
        assert!(!g.workspace_activity.contains_key(&ws));
        assert!(!g.workspace_needs_attention.contains(&ws));
        assert!(g.pending_bells.is_empty());
        assert_eq!(
            g.agent_events[&peer].model_id.as_deref(),
            Some("peer-model")
        );
        assert_eq!(g.agent_events[&peer].context_tokens, Some(17_128));
        assert_eq!(g.agent_events[&peer].log.len(), 4);
    }

    #[tokio::test]
    async fn missing_primary_after_scan_keeps_activity_and_attention_live() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, ws, primary, _, _, _) = setup(dir.path());
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        {
            let mut g = app.lock().await;
            g.test_spawn_session(primary, SessionStatus::Running { pid: 1 });
            draw_once(&mut g);
            assert_eq!(g.workspace_events[&ws].context_tokens, Some(16_485));
            assert_eq!(g.workspace_activity[&ws], ActivityState::Complete);
            assert!(g.workspace_needs_attention.remove(&ws));
            assert!(
                g.pending_bells.is_empty(),
                "cold-start completion is silent"
            );
            g.store
                .set_instance_agent_session(
                    primary,
                    dir.path().join("missing.jsonl").to_str().unwrap(),
                )
                .unwrap();
        }
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        let mut g = app.lock().await;
        assert!(
            g.workspace_events
                .get(&ws)
                .and_then(|events| events.context_tokens)
                .is_none(),
            "missing primary must not retain old usage or borrow peer usage"
        );
        let session = g.sessions.get(primary).unwrap();
        session.activity_ms.store(
            crate::util::time::now_ms() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        draw_once(&mut g);
        assert_eq!(g.workspace_activity[&ws], ActivityState::Active);
        assert!(!g.workspace_needs_attention.contains(&ws));

        let session = g.sessions.get(primary).unwrap();
        session.activity_ms.store(
            (crate::util::time::now_ms() - 10_000) as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
        draw_once(&mut g);
        assert_eq!(g.workspace_activity[&ws], ActivityState::Idle);

        // A fresh primary permission signal must still reach the alert loop,
        // without another successful transcript scan reopening its gate.
        g.workspace_events
            .entry(ws)
            .or_default()
            .pending_tool_uses
            .insert("permission".into(), ("bash".into(), 0));
        draw_once(&mut g);
        assert_eq!(g.workspace_activity[&ws], ActivityState::Awaiting);
        assert!(g.workspace_needs_attention.contains(&ws));
        assert_eq!(g.pending_bells, vec![ActivityState::Awaiting]);
        draw_once(&mut g);
        assert_eq!(g.pending_bells, vec![ActivityState::Awaiting]);
    }

    #[tokio::test]
    async fn unreadable_primary_replacement_keeps_activity_live_without_stale_usage() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, ws, primary, _, _, _) = setup(dir.path());
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        let replacement = dir.path().join("unreadable.jsonl");
        std::fs::write(&replacement, [0xff, b'\n']).unwrap();
        {
            let mut g = app.lock().await;
            draw_once(&mut g);
            assert_eq!(g.workspace_activity[&ws], ActivityState::Complete);
            g.store
                .set_instance_agent_session(primary, replacement.to_str().unwrap())
                .unwrap();
        }
        tail_workspace_events(app.clone(), ws, dir.path().into(), AgentKind::Omp).await;
        let mut g = app.lock().await;
        assert!(
            g.workspace_events
                .get(&ws)
                .and_then(|events| events.context_tokens)
                .is_none()
        );
        draw_once(&mut g);
        assert_eq!(g.workspace_activity[&ws], ActivityState::Off);
    }

    #[tokio::test]
    async fn stale_roster_cannot_commit_after_recorded_identity_changes() {
        let dir = tempfile::TempDir::new().unwrap();
        let (app, ws, _, peer, _, _) = setup(dir.path());
        let roster = app.lock().await.store.workspace_agents(ws).unwrap();
        let instance = roster.iter().find(|i| i.id == peer).unwrap();
        app.lock()
            .await
            .store
            .set_instance_agent_session(peer, dir.path().join("missing.jsonl").to_str().unwrap())
            .unwrap();
        // Models a tail whose identity snapshot predates a session hook.
        tail_instance_events(
            &app,
            ws,
            dir.path(),
            instance.agent,
            Some(instance),
            &roster,
        )
        .await;
        assert!(!app.lock().await.agent_events.contains_key(&peer));
    }

    #[tokio::test]
    async fn unchanged_hermes_identity_keeps_summary_when_database_tail_fails() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        std::fs::create_dir_all(home.path().join(".hermes")).unwrap();
        let db = rusqlite::Connection::open(home.path().join(".hermes/state.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE sessions (id TEXT PRIMARY KEY, started_at REAL);
             INSERT INTO sessions VALUES ('current', 100);
             CREATE TABLE messages (
                 id INTEGER, session_id TEXT, role TEXT, content TEXT,
                 tool_call_id TEXT, tool_calls TEXT, tool_name TEXT,
                 timestamp REAL, finish_reason TEXT
             );
             INSERT INTO messages VALUES
                 (1, 'current', 'assistant', 'Completed work', NULL, NULL, NULL, 100, 'stop');",
        )
        .unwrap();
        std::fs::create_dir_all(work.path().join(".git/info")).unwrap();
        std::fs::write(
            work.path().join(".git/info/wsx-hermes-spawn-at"),
            "100\ncurrent\n",
        )
        .unwrap();
        let store = Store::open_in_memory().unwrap();
        let repo_id = store.add_repo(work.path(), "hermes", "").unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "hermes",
                branch: "hermes",
                worktree_path: work.path(),
                yolo: false,
                agent: AgentKind::Hermes,
                shared: false,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Hermes, 0).unwrap();
        let app = Arc::new(Mutex::new(App::new(store, work.path().into()).unwrap()));
        tail_workspace_events(app.clone(), ws, work.path().into(), AgentKind::Hermes).await;
        assert_eq!(
            app.lock().await.workspace_events[&ws]
                .last_assistant_text
                .as_deref(),
            Some("Completed work"),
        );

        // Discovery still resolves the same virtual session, but its message
        // query fails. An I/O error must not erase the last successful summary.
        db.execute_batch("DROP TABLE messages").unwrap();
        tail_workspace_events(app.clone(), ws, work.path().into(), AgentKind::Hermes).await;
        let g = app.lock().await;
        assert_eq!(
            g.workspace_events
                .get(&ws)
                .and_then(|e| e.last_assistant_text.as_deref()),
            Some("Completed work"),
        );
        assert!(g.workspace_events_scanned.contains(&ws));
    }
}

/// Periodically check each live workspace's current git branch against
/// the DB; if claude (or a user) renamed it, update name + branch in the
/// store. Runs forever; cheap when nothing has drifted.
pub async fn branch_drift_poll(app: SharedApp) {
    branch_drift_poll_with(app, |path, branch| async move {
        crate::git::forge::fetch_pr_status(&path, &branch).await
    })
    .await
}

/// [`branch_drift_poll`] with the PR fetch injected, so tests can drive the
/// loop without a remote and without `gh`. Modelled on the liveness
/// injection in `commands::shared::shared_list_records`: production passes
/// `crate::git::forge::fetch_pr_status`, tests pass a closure that answers
/// from a table keyed by branch.
///
/// The closure takes owned `(worktree, branch)` rather than references so
/// the returned future doesn't have to borrow from the loop body.
pub async fn branch_drift_poll_with<F, Fut>(app: SharedApp, fetch_pr: F)
where
    F: Fn(std::path::PathBuf, String) -> Fut,
    Fut: std::future::Future<Output = crate::error::Result<Option<crate::git::forge::PrStatus>>>,
{
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
                    g.pr_review.remove(&id);
                    g.pr_unresolved.remove(&id);
                    g.pr_last_poll_ms.remove(&id);
                    // The persisted row too, not just these in-memory maps:
                    // `wsx waybar menu-entries` and `wsx menubar plugin` are
                    // separate short-lived processes that render from
                    // scm_cache alone, so a verdict left behind there keeps
                    // claiming the OLD branch's PR is approved — under the
                    // new branch's name — until a later poll happens to
                    // overwrite it.
                    let _ = g.store.clear_scm_pr(id);
                    // New branch → different ancestry from `base_branch`,
                    // so the cached diff and its throttle stamp are
                    // stale. Drop them to force a fresh poll.
                    g.workspace_diff.remove(&id);
                    g.workspace_diff_per_file.remove(&id);
                    g.diff_last_poll_ms.remove(&id);
                    // Everything below works from `snapshot`, taken before
                    // the rename — `db_branch` now names the branch we just
                    // superseded. Polling PR status with it would re-file
                    // the old branch's PR under the new branch's name and
                    // undo the invalidation we just did, in the same pass.
                    // Skip the rest of this workspace's iteration; the
                    // throttle stamps were cleared above, so the next tick
                    // (2s) re-snapshots and refreshes everything for real.
                    continue;
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
                let now_ms = crate::util::time::now_ms();
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
            let now_ms = crate::util::time::now_ms();
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
                if let Ok(Some(status)) = fetch_pr(path.clone(), db_branch).await {
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
                    // Removed, not left alone, when the verdict is gone: a
                    // new commit on a protected branch dismisses an
                    // approval, and a stale tick would claim the PR is
                    // still ready to merge.
                    match status.review {
                        Some(d) => {
                            g.pr_review.insert(id, d);
                        }
                        None => {
                            g.pr_review.remove(&id);
                        }
                    }
                    // Same removal rule as the verdict: a probe that
                    // couldn't answer must not leave a stale count behind.
                    match status.unresolved {
                        Some(n) => {
                            g.pr_unresolved.insert(id, n);
                        }
                        None => {
                            g.pr_unresolved.remove(&id);
                        }
                    }
                    // Write-through so `wsx waybar menu-entries` (a separate
                    // short-lived process) sees PR state without calling gh.
                    let _ = g.store.upsert_scm_pr(id, &status, now_ms / 1000);
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
            let now_ms = crate::util::time::now_ms();
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
            let now_ms = crate::util::time::now_ms();
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
                shared: false,
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
