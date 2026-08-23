//! The event loop.
//!
//! Frame pacing lives here too: the loop redraws on a dirty flag rather
//! than a fixed tick, and [`frame_delay`] decides how long to wait.

use super::*;
use crossterm::event::EventStream;
use futures::StreamExt;
use ratatui::Terminal;
use ratatui::backend::Backend;
use std::time::Duration;

/// Cadence of the housekeeping/animation tick.
///
/// This used to be 16ms, but not because anything needed 62.5Hz: the PTY reader
/// thread had no way to tell the render loop that output arrived, so the tick
/// doubled as the poll rate and the loop rebuilt the whole frame ~62 times a
/// second forever. `pty::wake` now carries that edge, leaving this to drive
/// only the spinner and periodic bookkeeping — 8Hz is the spinner's spec rate,
/// so one tick is exactly one spinner frame.
pub(crate) const TICK: Duration = Duration::from_millis(125);

/// Build the housekeeping/animation interval.
///
/// `tokio::time::interval` defaults to `MissedTickBehavior::Burst`, which
/// replays every deadline missed during a stall back-to-back. The loop does
/// stall: `do_pending_edit` hands the terminal to `$EDITOR` and awaits it, so a
/// minutes-long edit banks hundreds of ticks that would then fire in a rush —
/// whirling the spinner and re-running the bookkeeping block many times over.
///
/// `Delay` instead guarantees a full `TICK` between ticks no matter how long
/// the loop was blocked. Nothing in the tick arm wants replaying: it advances
/// an animation counter and does deadline-guarded bookkeeping, all of which is
/// idempotent and all of which only cares about *now*.
fn housekeeping_interval() -> tokio::time::Interval {
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tick
}

/// Floor on the gap between two frames — 30fps.
///
/// A busy agent emits output continuously (Claude Code animates its own
/// spinner), so the wake fires continuously and this floor, not the tick, is
/// what sets CPU cost while an agent works. 30fps is the cheapest rate that
/// still reads as smooth: the resulting ~33ms of echo latency sits under the
/// ~50ms threshold where typing starts to feel detached, and it halves the
/// per-frame cost against the 62.5Hz the old fixed tick ran at.
const MIN_FRAME: Duration = Duration::from_millis(33);

/// The cadence the loop ran at before repaints were wake-driven. Retained as
/// the yardstick for `MIN_FRAME`: peak redraw rate must never rise above it.
#[cfg(test)]
const LEGACY_FRAME: Duration = Duration::from_millis(16);

/// How long to wait before the next frame, given how long ago the last one was.
/// `None` means the budget is already spent — draw now. A remainder of exactly
/// zero is `None` too: scheduling a zero-length sleep would burn a whole loop
/// iteration to learn what is already known.
fn frame_delay(since_last_frame: Duration) -> Option<Duration> {
    MIN_FRAME
        .checked_sub(since_last_frame)
        .filter(|remaining| !remaining.is_zero())
}

/// What the render loop should do about drawing on this iteration.
#[derive(Debug, PartialEq, Eq)]
enum FrameAction {
    /// A repaint is owed and allowed — draw now.
    Draw,
    /// A repaint is owed but the floor has not expired. Wake after this long,
    /// while still servicing input and output in the meantime.
    WaitFor(Duration),
    /// Nothing is owed. Park on the event arms with no frame timer at all —
    /// an idle TUI does no work until something actually happens.
    Park,
}

/// Decide the iteration's drawing behaviour.
///
/// The floor paces *frames*, never the loop. Sleeping before the select instead
/// would cap event intake at one event per frame, so key repeats and wheel
/// bursts would drain at 30/sec and accumulate latency — far worse than the
/// single-frame delay the floor is meant to cost.
fn next_frame_action(dirty: bool, since_last_frame: Option<Duration>) -> FrameAction {
    if !dirty {
        return FrameAction::Park;
    }
    match since_last_frame.and_then(frame_delay) {
        Some(remaining) => FrameAction::WaitFor(remaining),
        None => FrameAction::Draw,
    }
}

async fn do_pending_edit<B>(
    terminal: &mut ratatui::Terminal<B>,
    app: &SharedApp,
    edit: PendingEdit,
) -> Result<()>
where
    B: ratatui::backend::Backend + std::io::Write,
{
    // Read current value + extension hint under the lock.
    let (current, ext) = {
        let g = app.lock().await;
        let Some(repo) = g.repos.iter().find(|r| r.id == edit.repo_id) else {
            return Ok(());
        };
        match edit.field {
            RepoSettingField::RepoName => (repo.name.clone(), "txt"),
            RepoSettingField::BranchPrefix => (repo.branch_prefix.clone(), "txt"),
            RepoSettingField::BaseBranch => (repo.base_branch.clone().unwrap_or_default(), "txt"),
            RepoSettingField::CustomInstructions => {
                (repo.custom_instructions.clone().unwrap_or_default(), "md")
            }
            RepoSettingField::SetupScript => {
                (repo.setup_script.clone().unwrap_or_default(), "bash")
            }
            RepoSettingField::ArchiveScript => {
                (repo.archive_script.clone().unwrap_or_default(), "bash")
            }
            RepoSettingField::PinnedCommands => {
                (repo.pinned_commands.clone().unwrap_or_default(), "txt")
            }
            RepoSettingField::RelatedRepos => {
                (repo.related_repos.clone().unwrap_or_default(), "txt")
            }
            RepoSettingField::DetailBarConfig => {
                let raw = repo
                    .detail_bar_config
                    .clone()
                    .unwrap_or_else(|| "{}\n".to_string());
                (raw, "json")
            }
        }
    };

    // Suspend the TUI, handing the terminal to the editor.
    crossterm::terminal::disable_raw_mode()?;
    crate::ui::term_modes::leave_tui_modes(terminal.backend_mut())?;

    let result = crate::commands::external::edit_in_editor(&current, ext);

    // Resume. This must re-assert EVERY mode, not just the alternate screen:
    // the editor resets bracketed paste and mouse reporting as it exits (vim
    // emits `ESC[?2004l`), and those modes are global tty state that nothing
    // else restores. See `crate::ui::term_modes`.
    crate::ui::term_modes::enter_tui_modes(terminal.backend_mut())?;
    crossterm::terminal::enable_raw_mode()?;
    terminal.clear()?;

    if let Ok(Some(new)) = result {
        if new.trim() != current.trim() {
            let mut g = app.lock().await;
            if let Err(e) = apply_repo_setting(&mut g, edit.repo_id, edit.field, &new) {
                g.modal = Some(crate::ui::modal::Modal::Error {
                    message: e.to_string(),
                });
            } else {
                let _ = g.refresh();
            }
        }
    }
    Ok(())
}

pub async fn run<B: Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: SharedApp,
) -> Result<()> {
    let mut events = EventStream::new();
    let mut tick = housekeeping_interval();
    let mut last_frame: Option<std::time::Instant> = None;
    // Whether the screen owes a repaint. Set by anything that can change what
    // is displayed; cleared by the draw. Lets input be consumed at full speed
    // while frames stay coalesced to `MIN_FRAME`.
    let mut dirty = true;

    loop {
        // Handle any pending edit BEFORE drawing — the editor takes
        // over the terminal and we need a clean redraw after it exits.
        let pending = {
            let mut g = app.lock().await;
            g.pending_edit.take()
        };
        if let Some(edit) = pending {
            do_pending_edit(terminal, &app, edit).await?;
            // The editor owned the terminal; the screen needs rebuilding.
            dirty = true;
        }

        // Draw only when something changed AND the floor has expired. The floor
        // paces *frames*, never the loop itself: sleeping here instead would
        // throttle event intake to one event per frame, so a burst of key
        // repeats or wheel events would queue up and drain at 30/sec with
        // cumulative latency. Input is handled at full speed below and merely
        // marks the screen dirty; redraws coalesce.
        if next_frame_action(dirty, last_frame.map(|t| t.elapsed())) == FrameAction::Draw {
            let mut g = app.lock().await;
            terminal.draw(|f| crate::app::render::draw(f, &mut g))?;
            // Drain bells queued during draw and fire them OUTSIDE the draw
            // closure so writes to stdout don't interleave with ratatui's
            // frame flush (mid-escape `\x07` is undefined per VT spec).
            let bells = std::mem::take(&mut g.pending_bells);
            for state in bells {
                fire_bell(state, &g.store);
            }
            dirty = false;
            last_frame = Some(std::time::Instant::now());
            if g.quit {
                break;
            }
        }

        // A redraw owed but not yet allowed needs its own wakeup, or the loop
        // would park on the other arms and the frame would never land. When
        // nothing is owed this arm is `pending` and never fires.
        let frame_due = match next_frame_action(dirty, last_frame.map(|t| t.elapsed())) {
            FrameAction::WaitFor(d) => Some(d),
            // `Draw` cannot occur — the block above would have consumed it.
            FrameAction::Draw | FrameAction::Park => None,
        };

        tokio::select! {
            _ = async move {
                match frame_due {
                    Some(d) => tokio::time::sleep(d).await,
                    None => std::future::pending::<()>().await,
                }
            } => {}
            // An attached pane's contents changed. The redraw happens at the
            // top of the next iteration, subject to the frame floor — this is
            // what lets `TICK` be slow without laggy panes.
            _ = crate::pty::wake::output_wake().wait() => { dirty = true; }
            _ = tick.tick() => {
                dirty = true;
                let mut g = app.lock().await;
                g.tick = g.tick.wrapping_add(1);
                // Expire any ephemeral chip-dispatch echo in the reply
                // input. Set by `fire_chip` so the user briefly sees
                // which command was sent; wiped here once the deadline
                // is reached.
                let now_ms = crate::util::time::now_ms_u64();
                if matches!(g.dashboard.reply_draft_clear_at_ms, Some(t) if now_ms >= t) {
                    g.dashboard.reply_draft.clear();
                    g.dashboard.reply_draft_clear_at_ms = None;
                }
                // Apply a settled terminal resize to backgrounded sessions so
                // re-attaching doesn't show a vt100 frame clipped to the old
                // size. Visible panes are sized by the render path above.
                if let Some((cols, rows)) = g.resize_debounce.take_due(now_ms) {
                    g.apply_backgrounded_resize(cols, rows);
                }
                // Pick up workspaces/repos written by sibling `wsx` CLI
                // processes (e.g. `wsx workspace create` invoked by Claude
                // during a related-repos flow). Cheap: PRAGMA data_version
                // is in-process and only triggers refresh on external commits.
                // Only scan the inbox when a sibling commit was detected
                // (e.g. a `wsx agent send`), avoiding a per-frame DB query.
                // `apply_delivery_outcomes` must run unconditionally: an
                // injection that finished (landed, or gave up waiting for the
                // agent to be ready) reports back through shared state, not
                // through the DB, so `poll_external_changes` never sees it. It
                // is a lock + is_empty when nothing is in flight.
                let redeliver = g.apply_delivery_outcomes();
                // The heartbeat covers what neither of the other two triggers
                // can: mail already queued at startup, and mail left pending by
                // a failure that produced no outcome (agent spawn failed, no
                // session). Both would otherwise wait for an unrelated sibling
                // commit that may never come.
                let mail_due = g.mail_drain_due(now_ms);
                if g.poll_external_changes() || redeliver || mail_due {
                    g.drain_agent_messages();
                }
                let now_secs = crate::util::time::now_secs();
                let now_hour = now_secs - (now_secs % 3600);
                let live = g
                    .workspaces
                    .iter()
                    .filter(|(_rid, ws)| {
                        let s = g.classify_status(ws);
                        matches!(s,
                            crate::ui::dashboard::status::Status::Thinking
                            | crate::ui::dashboard::status::Status::Waiting)
                    })
                    .count() as u32;
                match g.activity_history.back().copied() {
                    Some((h, prev_max)) if h == now_hour => {
                        if live > prev_max {
                            g.activity_history.pop_back();
                            g.activity_history.push_back((h, live));
                        }
                    }
                    Some(_) | None => {
                        if let Some((h, m)) = g.activity_history.back().copied() {
                            let _ = g.store.set_activity_bucket(h, m);
                        }
                        g.activity_history.push_back((now_hour, live));
                        while g.activity_history.len() > MAX_ACTIVITY_HOURS as usize {
                            g.activity_history.pop_front();
                        }
                        let _ = g.store.prune_activity_buckets_before(
                            now_hour.saturating_sub(MAX_ACTIVITY_HOURS * 3600),
                        );
                    }
                }
            }
            maybe_evt = events.next() => {
                dirty = true;
                let Some(Ok(evt)) = maybe_evt else { break; };
                // Drain any refreshes scheduled by detach handlers while
                // we held the lock; resolve each id to its (path, agent)
                // pair under the same lock so the spawned tail doesn't
                // need to walk `App::workspaces`. Then spawn outside the
                // lock so the tails don't serialize event handling.
                let pending: Vec<(
                    WorkspaceId,
                    std::path::PathBuf,
                    crate::pty::session::AgentKind,
                )> = {
                    let mut g = app.lock().await;
                    crate::app::input::handle_event(&mut g, &app, evt).await?;
                    let ids: Vec<WorkspaceId> =
                        g.pending_workspace_refresh.drain().collect();
                    ids.into_iter()
                        .filter_map(|id| {
                            g.workspaces
                                .iter()
                                .find(|(_, w)| w.id == id)
                                .map(|(_, w)| (id, w.worktree_path.clone(), w.agent))
                        })
                        .collect()
                };
                for (id, path, agent) in pending {
                    let app_clone = app.clone();
                    tokio::spawn(async move {
                        tail_workspace_events(app_clone, id, path, agent).await;
                    });
                }
            }
        }
    }
    Ok(())
}

/// Immediately re-run `proc::scan` and re-bucket. Used after a kill
/// so the modal reflects the new state without waiting for the
/// next 10s poll tick.
pub(crate) async fn rescan_processes(app: &mut App) {
    let procs = crate::activity::proc::scan().await;
    let worktrees: Vec<(crate::data::store::WorkspaceId, std::path::PathBuf)> = app
        .workspaces
        .iter()
        .map(|(_, w)| (w.id, w.worktree_path.clone()))
        .collect();
    let worktree_refs: Vec<(crate::data::store::WorkspaceId, &std::path::Path)> = worktrees
        .iter()
        .map(|(id, path)| (*id, path.as_path()))
        .collect();
    app.workspace_processes = crate::activity::proc::bucket_by_worktree(&procs, &worktree_refs);
    app.last_proc_scan_ms = crate::util::time::now_ms();
    // Clamp the modal's `selected` index after the list size changes.
    // Read workspace_id out first (Copy) to avoid a simultaneous
    // borrow of `app.workspace_processes` and `app.modal`.
    let modal_ws_id = match &app.modal {
        Some(Modal::ProcessList { workspace_id, .. }) => Some(*workspace_id),
        _ => None,
    };
    if let Some(ws_id) = modal_ws_id {
        let len = app
            .workspace_processes
            .get(&ws_id)
            .map(|v| v.len())
            .unwrap_or(0);
        if let Some(Modal::ProcessList { selected, .. }) = &mut app.modal {
            *selected = if len == 0 {
                0
            } else {
                (*selected).min(len - 1)
            };
        }
    }
}

#[cfg(test)]
mod pacing_tests {
    use super::*;

    #[test]
    fn a_tick_driven_frame_never_waits() {
        // `TICK` is far longer than `MIN_FRAME`, so ordinary idle repaints must
        // pass straight through the floor rather than adding latency.
        assert!(TICK > MIN_FRAME);
        assert_eq!(frame_delay(TICK), None);
    }

    #[test]
    fn back_to_back_output_is_held_to_the_frame_floor() {
        // The streaming-agent case: output arriving 1ms after the last frame
        // waits out the remaining budget instead of spinning the loop.
        assert_eq!(
            frame_delay(Duration::from_millis(1)),
            Some(Duration::from_millis(32))
        );
    }

    #[test]
    fn an_exhausted_budget_draws_immediately() {
        // Exactly at the floor counts as spent, not as a zero-length wait.
        assert_eq!(frame_delay(MIN_FRAME), None);
        assert_eq!(frame_delay(MIN_FRAME + Duration::from_millis(1)), None);
    }

    #[test]
    fn the_floor_never_beats_the_old_fixed_cadence() {
        // The old loop drew at most every 16ms. Wake-driven redraws must not be
        // more frequent than that, or this would raise peak CPU instead of
        // cutting it. `MIN_FRAME` is deliberately slower still.
        assert!(
            MIN_FRAME >= LEGACY_FRAME,
            "peak redraw rate must not exceed the pre-wake loop"
        );
    }

    /// Drain the tick that is already overdue after a stall, then report
    /// whether *another* one is also immediately ready. Under `Burst` every
    /// missed deadline is queued up and the answer is yes.
    async fn extra_tick_ready_after_stall(tick: &mut tokio::time::Interval) -> bool {
        tick.tick().await; // completes immediately at t=0
        tokio::time::advance(TICK * 10).await;
        tick.tick().await; // the overdue one; legitimate under any behaviour
        tokio::time::timeout(Duration::from_millis(1), tick.tick())
            .await
            .is_ok()
    }

    #[tokio::test(start_paused = true)]
    async fn the_tick_does_not_replay_deadlines_missed_during_a_stall() {
        // `do_pending_edit` blocks the loop for as long as $EDITOR is open. On
        // return the tick must resume, not fire hundreds of banked ticks that
        // whirl the spinner and re-run the bookkeeping block.
        let mut tick = housekeeping_interval();
        assert!(
            !extra_tick_ready_after_stall(&mut tick).await,
            "a stalled tick must not have extra ticks queued up behind it"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn the_default_interval_would_replay_them() {
        // Proves the test above discriminates: tokio's default `Burst` is
        // exactly the behaviour being guarded against.
        let mut tick = tokio::time::interval(TICK);
        assert!(
            extra_tick_ready_after_stall(&mut tick).await,
            "tokio's default is Burst; if this ever changes the guard above is moot"
        );
    }

    #[test]
    fn an_idle_loop_parks_instead_of_scheduling_a_frame() {
        // Nothing owed: no frame timer at all, so an idle TUI waits purely on
        // real events rather than waking itself 30 times a second.
        assert_eq!(next_frame_action(false, None), FrameAction::Park);
        assert_eq!(
            next_frame_action(false, Some(Duration::from_secs(9))),
            FrameAction::Park
        );
    }

    #[test]
    fn the_first_frame_draws_without_waiting() {
        assert_eq!(next_frame_action(true, None), FrameAction::Draw);
    }

    #[test]
    fn a_repaint_owed_past_the_floor_draws_immediately() {
        assert_eq!(next_frame_action(true, Some(MIN_FRAME)), FrameAction::Draw);
        assert_eq!(
            next_frame_action(true, Some(MIN_FRAME + Duration::from_millis(5))),
            FrameAction::Draw
        );
    }

    #[test]
    fn a_repaint_owed_inside_the_floor_waits_out_the_remainder() {
        assert_eq!(
            next_frame_action(true, Some(Duration::from_millis(1))),
            FrameAction::WaitFor(Duration::from_millis(32))
        );
    }

    #[test]
    fn input_is_never_gated_by_the_frame_floor() {
        // The regression this replaced: the floor used to sleep at the top of
        // the loop, so every event waited a frame and bursts drained at 30/sec
        // with cumulative latency. Pacing must never resolve to a decision that
        // blocks the select, only ever to one that schedules a *frame* — so
        // however recently we drew, the loop still parks on the event arms as
        // soon as the repaint is satisfied.
        for elapsed_ms in [0, 1, 10, 32, 33, 100] {
            let elapsed = Some(Duration::from_millis(elapsed_ms));
            assert_eq!(
                next_frame_action(false, elapsed),
                FrameAction::Park,
                "a satisfied screen must never hold up event intake ({elapsed_ms}ms)"
            );
            assert_ne!(
                next_frame_action(true, elapsed),
                FrameAction::Park,
                "an owed repaint must still be scheduled ({elapsed_ms}ms)"
            );
        }
    }

    #[test]
    fn the_floor_stays_within_typing_latency_budget() {
        // Echo latency is bounded by the floor. Past ~50ms typing reads as
        // detached, so this is the ceiling on trading responsiveness for CPU.
        assert!(
            MIN_FRAME <= Duration::from_millis(50),
            "frame floor doubles as worst-case keystroke echo latency"
        );
    }
}
