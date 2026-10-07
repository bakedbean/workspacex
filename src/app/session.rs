//! Starting, attaching to, and tearing down a workspace's PTY sessions,
//! including the tmux-shared variant and saved split layouts.

use super::*;

pub(crate) fn save_layout_for(app: &mut App, state: crate::ui::AttachedState) {
    let Some(anchor) = state.leaves().first().map(|t| t.workspace_id) else {
        return;
    };
    if let Err(e) = app
        .store
        .set_workspace_layout(anchor, &state.tree, &state.focus)
    {
        tracing::warn!(error = %e, "failed to save workspace layout");
    }
    // Recompute the dashboard indicator cache so the badge updates
    // immediately when the user returns to the dashboard.
    let _ = app.refresh();
}

/// Restore a saved layout for `anchor`, pruning any workspaces that no longer
/// exist. Spawns missing sessions for surviving side panes. Falls back to a
/// single-pane view if no layout is saved or all panes were pruned. Returns
/// `None` only if the anchor has no resolvable primary instance (unreachable
/// in normal use — all callers guard on `primary_instance(...).is_some()`).
pub(crate) fn restore_attached_state(
    app: &mut App,
    anchor: crate::data::store::WorkspaceId,
) -> Option<crate::ui::AttachedState> {
    // Fallback single-pane target: the anchor workspace's primary instance.
    // Matches pre-multi-agent behavior — a single-agent workspace's leaf is
    // its primary instance.
    let single = |app: &App| {
        app.primary_instance(anchor).map(|instance| {
            crate::ui::AttachedState::single(crate::ui::split::AttachTarget {
                workspace_id: anchor,
                instance,
            })
        })
    };
    let Some((mut tree, mut focus)) = app.store.get_workspace_layout(anchor).ok().flatten() else {
        return single(app);
    };
    let valid_ws: std::collections::HashSet<_> = app.workspaces.iter().map(|(_, w)| w.id).collect();
    use crate::ui::split::PruneOutcome;
    // A leaf is stale if its workspace no longer exists OR its agent instance
    // no longer exists in the store.
    let outcome = tree.prune(&|t| {
        valid_ws.contains(&t.workspace_id)
            && app
                .store
                .workspace_agents_by_id(t.instance)
                .ok()
                .flatten()
                .is_some()
    });
    match outcome {
        PruneOutcome::Empty => {
            let _ = app.store.delete_workspace_layout(anchor);
            let _ = app.refresh();
            single(app)
        }
        PruneOutcome::Kept => {
            if tree.leaf_at(&focus).is_none() {
                focus = tree.first_leaf_path();
            }
            // Spawn any missing sessions for the side panes. The focused
            // anchor instance was already spawned by the caller. Skip on
            // failure and continue with remaining panes — partial restore is
            // better than no restore.
            for leaf in tree.leaves() {
                if app.sessions.get(leaf.instance).is_some() {
                    continue;
                }
                let _ = ensure_instance_session(app, leaf.instance, true);
            }
            Some(crate::ui::AttachedState { tree, focus })
        }
    }
}

/// Ensure a workspace has a live PTY session, spawning one in place if
/// missing. Used by `attach_workspace` and by inline-dispatch paths
/// (chip click / chord / reply Enter) so writes from the dashboard
/// don't silently drop on workspaces the user hasn't attached to.
/// No-op when the workspace already has a session, or when
/// `build_spawn_info` returns `None` (e.g., setup hasn't completed).
///
/// This is the single enforcement point for `attach_is_blocked`: every
/// caller — `attach_workspace`, the inline-dispatch paths, and (via
/// `ensure_instance_session`'s delegation) primary-instance retargeting —
/// goes through here or `ensure_primary_session` behind it, so a live
/// archive can't be raced by a respawn from any of them.
pub(crate) fn ensure_workspace_session(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<AttachReady> {
    ensure_primary_session(app, ws_id, true)
}

/// `ensure_workspace_session`, with `surface_missing` as on
/// `ensure_instance_session`, which hands a primary instance here so that a
/// background caller's `false` reaches the modals this path raises.
fn ensure_primary_session(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
    surface_missing: bool,
) -> Result<AttachReady> {
    if attach_is_blocked(app, ws_id) {
        return Ok(AttachReady::Refused);
    }
    if app
        .primary_instance(ws_id)
        .and_then(|i| app.sessions.get(i))
        .is_some()
    {
        return Ok(AttachReady::Ok);
    }
    if let Some((id, path, mode, repo_path, agent)) = build_spawn_info(app, ws_id) {
        // Settle a missing worktree before anything acts on its path. The MCP
        // mirror would otherwise rewrite `~/.claude.json` with an entry for a
        // directory that isn't there, on every retry while a create runs.
        // The spawn's own guard still covers a worktree that goes in between.
        if !path.is_dir() {
            return Ok(missing_worktree_outcome(app, ws_id, &path, surface_missing));
        }
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::agent::remote_control::RemoteOpts::from_store(&app.store);
        // Resolve the primary agent instance for this workspace, defensively
        // seeding one for any row that somehow lacks a primary instance.
        let inst = resolve_primary_instance(app, id)?;
        let instance = app
            .store
            .workspace_agents_by_id(inst)?
            .ok_or_else(|| crate::error::Error::Store(rusqlite::Error::QueryReturnedNoRows))?;
        let tmux = tmux_name_for(app, id, &instance);
        match app.sessions.spawn(
            inst,
            id,
            &path,
            80,
            24,
            mode,
            remote,
            agent,
            tmux.as_deref(),
        ) {
            Ok(_) => {
                if let Some(name) = &tmux {
                    if let Err(e) = app.store.set_instance_session_ref(inst, name) {
                        tracing::warn!(error = %e, "failed to persist tmux session_ref");
                    }
                }
            }
            Err(crate::error::Error::AgentBinaryMissing(binary)) => {
                if surface_missing {
                    app.modal = Some(crate::ui::modal::Modal::AgentMissing {
                        ws_id,
                        agent,
                        binary,
                    });
                }
                return Ok(AttachReady::AgentMissing);
            }
            Err(crate::error::Error::WorktreeMissing(path)) => {
                return Ok(missing_worktree_outcome(app, ws_id, &path, surface_missing));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(AttachReady::Ok)
}

/// Whether `ws_id`'s create has yet to make its worktree, so a missing one is
/// not gone, just not there yet. That is a create in flight here, however
/// long it takes, or a `Pending` row younger than `STALE_PENDING_AFTER` for
/// one in another process: a row leaves `Pending` as soon as `git worktree
/// add` succeeds or fails, or the create is cancelled first. A row that stays
/// `Pending` longer belongs to a create that died (killed mid-fetch, say),
/// the same call `sweep_stale_pending` makes at the next startup. A row that
/// a quit mid-create left `Pending` with setup `Cancelled` (see
/// `confirm_quit`) is settled too. Read from the store rather than
/// `App::workspaces`, which only catches up with another process's writes on
/// its next poll. A store error counts as in progress, so it costs a retry
/// rather than a dropped message.
///
/// Such a spawn is `Refused`, with no modal and a retry from the message
/// drain, rather than treated as a worktree to archive.
fn create_in_progress(app: &App, ws_id: crate::data::store::WorkspaceId) -> bool {
    if app
        .in_flight
        .get(&ws_id)
        .is_some_and(|f| f.kind == crate::data::in_flight::InFlightKind::Create)
    {
        return true;
    }
    let cutoff =
        crate::data::store::now_ms() - crate::data::store::STALE_PENDING_AFTER.as_millis() as i64;
    match app.store.workspace_by_id(ws_id) {
        Err(_) => true,
        Ok(row) => row.is_some_and(|w| {
            w.state == crate::data::store::WorkspaceState::Pending
                && w.setup_status != crate::data::store::SetupStatus::Cancelled
                && w.created_at >= cutoff
        }),
    }
}

/// What an ensure answers when the spawn was refused because `path`, the
/// worktree, does not exist: `Refused` while the create has yet to make it,
/// otherwise `WorktreeMissing`, with its error modal when `surface_missing`.
fn missing_worktree_outcome(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
    path: &std::path::Path,
    surface_missing: bool,
) -> AttachReady {
    if create_in_progress(app, ws_id) {
        return AttachReady::Refused;
    }
    if surface_missing {
        app.modal = Some(worktree_missing_modal(app, ws_id, path));
    }
    AttachReady::WorktreeMissing
}

/// The same answer before anything is added, for a caller that would
/// otherwise insert agent rows an ensure then refuses: `Some` when `ws_id`'s
/// worktree is missing, `None` when there is one to start agents in.
pub(crate) fn refuse_without_worktree(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
    surface_missing: bool,
) -> Option<AttachReady> {
    let path = app.workspace_path(ws_id)?;
    (!path.is_dir()).then(|| missing_worktree_outcome(app, ws_id, &path, surface_missing))
}

/// The error shown when a spawn is refused because the workspace's worktree
/// is gone: a create that failed before git made one, or a worktree deleted
/// by hand. Archive is the way out, and it copes with the missing directory.
fn worktree_missing_modal(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
    path: &std::path::Path,
) -> crate::ui::modal::Modal {
    let name = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == ws_id)
        .map(|(_, w)| w.name.as_str())
        .unwrap_or("this workspace");
    crate::ui::modal::Modal::Error {
        message: format!(
            "The worktree for '{name}' is missing:\n{}\n\n\
             No agent was started. Archive the workspace\n\
             (d on the dashboard) to remove it.",
            path.display()
        ),
    }
}

/// Ensure a specific agent *instance* has a live PTY session, spawning one in
/// place if missing. Primary instances delegate to `ensure_primary_session`,
/// the body of `ensure_workspace_session`, so the primary path is never
/// duplicated. Added (non-primary) instances
/// spawn `Fresh` with an injected handoff note, or resume their own recorded
/// session once they have one (see `build_added_spawn_info`).
/// Mirrors `ensure_workspace_session`'s return/error conventions, including the
/// `AgentMissing` modal for a missing agent binary.
///
/// `surface_missing` controls whether a missing-binary error raises
/// `Modal::AgentMissing`. Pass `true` for interactive callers (keyboard
/// handlers, `switch_focused_pane_to`, `restore_attached_state`) so the user
/// sees the modal. Pass `false` for background callers (e.g. the message drain)
/// so a missing binary doesn't pop a modal over the user's unrelated view.
/// A missing worktree's error modal follows the same rule, and both hold for
/// a primary instance too, since the flag is passed on with it.
///
/// Enforces `attach_is_blocked` for non-primary instances directly (they
/// never reach `ensure_primary_session`). Primary instances delegate
/// above and get the check there instead — do not duplicate it here, or a
/// primary would be guarded twice.
pub(crate) fn ensure_instance_session(
    app: &mut App,
    inst: crate::data::store::AgentInstanceId,
    surface_missing: bool,
) -> Result<AttachReady> {
    // Unknown instance id: treat as a no-op (matches `build_spawn_info`
    // returning `None` for a workspace whose setup hasn't completed).
    let Some(instance) = app.store.workspace_agents_by_id(inst)? else {
        return Ok(AttachReady::Ok);
    };
    if instance.is_primary {
        return ensure_primary_session(app, instance.workspace_id, surface_missing);
    }
    let ws_id = instance.workspace_id;
    if attach_is_blocked(app, ws_id) {
        return Ok(AttachReady::Refused);
    }
    if app.sessions.get(inst).is_some() {
        return Ok(AttachReady::Ok);
    }
    if let Some((path, mode, repo_path)) = build_added_spawn_info(app, &instance) {
        // As on the primary path: no mirroring for a worktree that isn't there.
        if !path.is_dir() {
            return Ok(missing_worktree_outcome(app, ws_id, &path, surface_missing));
        }
        maybe_mirror_mcp(app, &repo_path, &path);
        let remote = crate::agent::remote_control::RemoteOpts::from_store(&app.store);
        let tmux = tmux_name_for(app, ws_id, &instance);
        match app.sessions.spawn(
            inst,
            ws_id,
            &path,
            80,
            24,
            mode,
            remote,
            instance.agent,
            tmux.as_deref(),
        ) {
            Ok(_) => {
                if let Some(name) = &tmux {
                    if let Err(e) = app.store.set_instance_session_ref(inst, name) {
                        tracing::warn!(error = %e, "failed to persist tmux session_ref");
                    }
                }
            }
            Err(crate::error::Error::AgentBinaryMissing(binary)) => {
                if surface_missing {
                    app.modal = Some(crate::ui::modal::Modal::AgentMissing {
                        ws_id,
                        agent: instance.agent,
                        binary,
                    });
                }
                return Ok(AttachReady::AgentMissing);
            }
            Err(crate::error::Error::WorktreeMissing(path)) => {
                return Ok(missing_worktree_outcome(app, ws_id, &path, surface_missing));
            }
            Err(e) => return Err(e),
        }
    }
    Ok(AttachReady::Ok)
}

/// Flip a workspace's `shared` flag and respawn instances per the new flag.
/// `build_spawn_info`/`build_added_spawn_info` already select
/// `SpawnMode::Continue` whenever `has_prior_session_for` finds a prior
/// session, so respawns resume the conversation via `--continue` for free —
/// this function just needs to kill any old backend and re-ensure.
///
/// Sharing spawns eagerly: EVERY instance ends up running inside tmux, not
/// just the ones that happened to be running at toggle time. A stopped agent
/// that only got the flag flip would leave the workspace shared-but-dead —
/// red badge, hidden from the remote picker, nothing to attach to remotely —
/// until the user happened to attach locally.
///
/// Unsharing restarts only instances that were actually running (no spurious
/// spawns of stopped agents). The was-running set is snapshotted *before*
/// flipping the flag and calling `app.refresh()`, so the borrow of
/// `app.store`/`app.sessions` used to compute it is long gone by the time we
/// mutate `app` below.
pub(crate) fn toggle_workspace_shared(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<()> {
    let ws = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == ws_id)
        .map(|(_, w)| w.clone())
        .ok_or_else(|| crate::error::Error::UserInput("workspace not found".into()))?;
    let to_shared = !ws.shared;
    // Guard: sharing spawns agents inside tmux. If tmux is absent, bail BEFORE
    // flipping the flag or killing any running direct agent — otherwise we'd
    // tear down live sessions only to discover we can't respawn them shared.
    // Surface the same AgentMissing modal the spawn path uses.
    if to_shared && !crate::pty::tmux::is_available() {
        app.modal = Some(crate::ui::modal::Modal::AgentMissing {
            ws_id,
            agent: ws.agent,
            binary: crate::pty::tmux::tmux_bin(),
        });
        return Ok(());
    }
    let all_instances = app.store.workspace_agents(ws_id)?;
    // Derived from `all_instances` (not `app.strip_instances`/`agent_roster`):
    // a row inserted on this same tick — e.g. `resolve_primary_instance`
    // backfilling a missing primary during attach — has no `refresh()` on
    // its path, so the cache can be missing it indefinitely. Deriving from
    // the fresh `all_instances` fetch above keeps `running` a subset of it
    // by construction, which the respawn/cleanup loops below depend on.
    let running: Vec<_> = all_instances
        .iter()
        .filter(|inst| app.instance_is_running(inst.id))
        .cloned()
        .collect();
    // The respawns below resume by recorded session; capture omp's current
    // one first (a `/new` since the last poll would otherwise be lost and
    // the respawn would reopen the session before it).
    app.harvest_session_identities();
    app.store.set_workspace_shared(ws_id, to_shared)?;
    app.refresh()?; // reload app.workspaces so spawn sees the new flag
    // `sessions.remove` calls `kill_backend` in both directions: for a direct
    // child it SIGKILLs the agent; for a tmux-backed session (unsharing) it
    // also kills the tmux server session — there's no way to move a live
    // process out of tmux, so losing in-flight output and resuming via
    // `--continue` is the intended design here.
    if to_shared {
        // Eager spawn: respawn running instances inside tmux AND start any
        // stopped ones, so the share is immediately alive (green badge,
        // reachable from the remote picker) — see the doc comment.
        //
        // Best-effort across instances: the shared flag is already flipped,
        // and each instance's spawn is independent, so one failure (PTY or
        // store error — a missing agent binary is NOT an error here, it
        // surfaces via the AgentMissing modal and continues) must not leave
        // the remaining instances stopped. Attempt every instance, then
        // surface the first error.
        let mut first_err = None;
        for inst in &all_instances {
            app.sessions.remove(inst.id);
            let result = if inst.is_primary {
                ensure_workspace_session(app, ws_id)
            } else {
                ensure_instance_session(app, inst.id, false)
            };
            if let Err(e) = result {
                tracing::warn!(error = %e, "failed to spawn an agent instance while sharing");
                first_err.get_or_insert(e);
            }
        }
        return match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        };
    }
    // Unsharing: restart only instances that were actually running.
    for inst in &running {
        app.sessions.remove(inst.id);
        if inst.is_primary {
            ensure_workspace_session(app, ws_id)?;
        } else {
            ensure_instance_session(app, inst.id, false)?;
        }
    }
    // When unsharing, no instance should keep a tmux `session_ref`. Running
    // instances were respawned direct above (their tmux session died inside
    // `sessions.remove`), but their stored ref is now stale. Non-running
    // instances were never touched — a detached-but-alive tmux session would
    // be orphaned, so kill it directly first. Then clear the ref either way so
    // a later archive doesn't try to kill a name that no longer addresses
    // anything. (CLI unshare is intentionally left alone: it flag-flips only,
    // keeping refs so archive can still clean up.)
    let running_ids: std::collections::HashSet<_> = running.iter().map(|i| i.id).collect();
    for inst in &all_instances {
        let Some(name) = &inst.session_ref else {
            continue;
        };
        if !running_ids.contains(&inst.id) {
            crate::pty::tmux::kill_session(name);
        }
        if let Err(e) = app.store.clear_instance_session_ref(inst.id) {
            tracing::warn!(error = %e, "failed to clear session_ref on unshare");
        }
    }
    Ok(())
}

/// The dashboard half of `wsx workspace share --restart`: share `ws_id` if it
/// isn't already (restarting its running agents inside tmux, as `T` does), and
/// make sure every agent instance is running there, so a peer on another
/// machine can attach the moment this returns. Unlike `T`, nothing is raised
/// on this dashboard's screen: the request came from elsewhere, so every
/// failure is returned for the requester to report instead.
pub(crate) fn force_share_workspace(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<()> {
    let ws = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == ws_id)
        .map(|(_, w)| w.clone())
        .ok_or_else(|| crate::error::Error::UserInput("workspace not found".into()))?;
    if attach_is_blocked(app, ws_id) {
        return Err(crate::error::Error::UserInput(
            "workspace is being archived".into(),
        ));
    }
    if !crate::pty::tmux::is_available() {
        return Err(crate::error::Error::UserInput(format!(
            "tmux >= 3.2 ({}) is not available on this host",
            crate::pty::tmux::tmux_bin()
        )));
    }
    // Respawns resume by recorded session; capture omp's current one first,
    // as `T` does.
    app.harvest_session_identities();
    if !ws.shared {
        app.store.set_workspace_shared(ws_id, true)?;
        app.refresh()?; // reload app.workspaces so spawn sees the new flag
    }
    let instances = app.store.workspace_agents(ws_id)?;
    let mut failed = Vec::new();
    for inst in &instances {
        // Decided per session, not by the flag: a plain `workspace share`
        // flips the flag yet leaves running agents outside tmux.
        match app.sessions.get(inst.id) {
            Some(s) if app.instance_is_running(inst.id) && s.tmux_session.is_some() => continue,
            // A direct agent can't move into tmux: kill it and resume its
            // conversation in the respawn, exactly as `T` does.
            Some(_) if app.instance_is_running(inst.id) => app.sessions.remove(inst.id),
            // An exited entry would make the ensure below a no-op. Its client
            // is already gone, so forgetting it kills nothing — and keeps a
            // tmux session that outlived the client alive to reattach to.
            _ => app.sessions.forget(inst.id),
        }
        let ready = if inst.is_primary {
            ensure_primary_session(app, ws_id, false)
        } else {
            ensure_instance_session(app, inst.id, false)
        };
        match ready {
            Ok(AttachReady::Ok) if app.sessions.get(inst.id).is_some() => {}
            Ok(AttachReady::WorktreeMissing) => {
                failed.push(format!("{}: worktree missing", inst.label()))
            }
            Ok(AttachReady::AgentMissing) => failed.push(format!(
                "{}: {} not installed",
                inst.label(),
                inst.agent.display_name()
            )),
            Ok(_) => failed.push(format!("{}: did not start", inst.label())),
            Err(e) => failed.push(format!("{}: {e}", inst.label())),
        }
    }
    if failed.is_empty() {
        Ok(())
    } else {
        Err(crate::error::Error::UserInput(failed.join("; ")))
    }
}

/// Act on every `wsx workspace share --restart` this dashboard should take
/// (see `claim_share_requests`), recording each outcome for the requesting
/// CLI to read back. Sets `share_drain_pending` while requests are left for
/// another dashboard, so the tick looks again once their grace runs out.
pub(crate) fn drain_share_requests(app: &mut App) {
    let claim = {
        let (store, sessions) = (&app.store, &app.sessions);
        // Owning = running a direct agent of the workspace: only this
        // dashboard could kill it.
        let owns = |ws: crate::data::store::WorkspaceId| {
            store.workspace_agents(ws).is_ok_and(|insts| {
                insts.iter().any(|i| {
                    sessions.get(i.id).is_some_and(|s| {
                        s.tmux_session.is_none()
                            && matches!(
                                *s.status.read().unwrap(),
                                crate::pty::session::SessionStatus::Running { .. }
                            )
                    })
                })
            })
        };
        store.claim_share_requests(owns)
    };
    let requests = match claim {
        Ok(c) => {
            app.share_drain_pending = c.deferred;
            c.claimed
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to claim share requests");
            app.share_drain_pending = true;
            return;
        }
    };
    for req in requests {
        let outcome = force_share_workspace(app, req.workspace_id);
        if let Err(e) = &outcome {
            tracing::warn!(error = %e, ws = req.workspace_id.0, "share request failed");
        }
        let error = outcome.err().map(|e| e.to_string());
        if let Err(e) = app.store.finish_share_request(req.id, error.as_deref()) {
            tracing::warn!(error = %e, "failed to record a share request's outcome");
        }
    }
}

/// Whether attaching to `ws_id` must be refused. Only a live archive
/// blocks: its first act is killing the workspace's tmux sessions so a
/// live agent cannot dirty the worktree during teardown, and attaching
/// would respawn one into a directory that is being deleted. A create in
/// flight never blocks — working in a workspace while its setup runs is
/// the point of backgrounding.
pub(crate) fn attach_is_blocked(app: &App, ws_id: crate::data::store::WorkspaceId) -> bool {
    app.in_flight
        .get(&ws_id)
        .is_some_and(|f| f.kind == crate::data::in_flight::InFlightKind::Archive)
}

/// Attach to a workspace: ensure a session, restore layout, and switch
/// to attached view. Shared by the `Enter` / `i` / `l` key handlers.
pub(crate) fn attach_workspace(
    app: &mut App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<()> {
    // `ensure_workspace_session` is the enforcement point for
    // `attach_is_blocked` (a live archive refuses); no need to check here
    // too — see `AttachReady::Refused`.
    match ensure_workspace_session(app, ws_id)? {
        AttachReady::Ok => {}
        // Attach didn't happen (AgentMissing or missing-worktree modal is
        // up, or attach was refused because an archive is tearing this
        // workspace down) — leave the workspace's attention marker alone so
        // a failed open doesn't silently dismiss it.
        AttachReady::AgentMissing | AttachReady::Refused | AttachReady::WorktreeMissing => {
            return Ok(());
        }
    }
    app.workspace_needs_attention.remove(&ws_id);
    if app
        .primary_instance(ws_id)
        .and_then(|i| app.sessions.get(i))
        .is_some()
    {
        if let Some(restored) = restore_attached_state(app, ws_id) {
            app.view = View::Attached(restored);
        }
    }
    Ok(())
}

/// Best-effort MCP server mirror. Logs and continues on any failure.
pub(crate) fn maybe_mirror_mcp(
    app: &App,
    repo_path: &std::path::Path,
    worktree_path: &std::path::Path,
) {
    if !crate::agent::mcp::enabled(&app.store) {
        return;
    }
    if let Err(e) = crate::agent::mcp::mirror_mcp_servers(repo_path, worktree_path) {
        tracing::warn!(error = %e, "failed to mirror MCP servers; continuing");
    }
}

/// Mark `ids` for an immediate out-of-band refresh: clear the per-workspace
/// throttle stamps (so the next periodic poll re-fetches diff/PR right
/// away), reset `last_proc_scan_ms` to 0 so the next tick reruns `lsof`,
/// and queue the workspaces into `pending_workspace_refresh` so `run_loop`
/// spawns an immediate JSONL events tail. Called by detach handlers so
/// the dashboard detail bar reflects work the user just did in the
/// attached session instead of waiting for the next 2s tick.
pub(crate) fn schedule_detach_refresh(app: &mut App, ids: impl IntoIterator<Item = WorkspaceId>) {
    app.last_proc_scan_ms = 0;
    for id in ids {
        app.diff_last_poll_ms.remove(&id);
        app.pr_last_poll_ms.remove(&id);
        app.pending_workspace_refresh.insert(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store error must not read as a settled workspace: that answer drops
    /// the workspace's queued messages for good, where a retry costs nothing.
    #[test]
    fn create_in_progress_assumes_one_when_the_store_errors() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let mut app = App::new(store, std::path::PathBuf::from("/tmp/wsx-test")).unwrap();
        let ws = app.test_workspace("store-error");
        app.store
            .set_workspace_state(ws, crate::data::store::WorkspaceState::Ready)
            .unwrap();
        assert!(
            !create_in_progress(&app, ws),
            "a settled row is not in progress"
        );
        app.store
            .conn()
            .execute("ALTER TABLE workspaces RENAME TO workspaces_gone", [])
            .unwrap();
        assert!(create_in_progress(&app, ws));
    }
}
