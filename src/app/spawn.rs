//! Working out what to launch for a workspace -- which agent, which tmux
//! session name, which instance is primary -- before a PTY is started.

use super::*;

/// Resolve the primary agent instance id for a workspace, defensively seeding
/// a primary instance row for any (pre-migration / freshly created) workspace
/// that somehow lacks one. Used by the spawn paths to key sessions.
pub(crate) fn resolve_primary_instance(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
) -> Result<crate::data::store::AgentInstanceId> {
    match app.store.primary_instance_id(ws_id)? {
        Some(i) => Ok(i),
        None => {
            let (_, ws) = app
                .workspaces
                .iter()
                .find(|(_, w)| w.id == ws_id)
                .ok_or_else(|| crate::error::Error::Store(rusqlite::Error::QueryReturnedNoRows))?;
            Ok(app
                .store
                .add_primary_agent(ws_id, ws.agent, ws.created_at)?
                .id)
        }
    }
}

/// Shared spawn context for a workspace: the bits common to spawning the
/// primary agent or any added instance. Keeping this in one place avoids
/// duplicating the custom-instructions / related-repo / doctrine /
/// additional-dirs computation between `build_spawn_info` and
/// `build_added_spawn_info`.
pub(crate) struct SpawnContext {
    repo_path: std::path::PathBuf,
    worktree: std::path::PathBuf,
    /// Repo custom instructions merged with the related-repo read-only prompt.
    custom: Option<String>,
    additional_dirs: Vec<std::path::PathBuf>,
    yolo: bool,
}

pub(crate) fn resolve_spawn_context(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
) -> Option<SpawnContext> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let custom = crate::data::repo::resolve_custom_instructions(repo, &app.store)
        .ok()
        .flatten();
    // Resolve related repos (per-repo names → source paths), filter out
    // the spawning repo itself, build the read-only system-prompt
    // fragment, and fold it into custom_instructions before the agent sees it.
    let resolved = crate::agent::related::resolve(repo.related_repos.as_deref(), &app.repos);
    let resolved: Vec<(String, std::path::PathBuf)> = resolved
        .into_iter()
        .filter(|(_, p)| p != &repo.path)
        .collect();
    let additional_dirs: Vec<std::path::PathBuf> =
        resolved.iter().map(|(_, p)| p.clone()).collect();
    let related_prompt = crate::agent::related::build_read_only_prompt(&resolved);
    let custom = match (custom, related_prompt) {
        (None, None) => None,
        (Some(c), None) => Some(c),
        (None, Some(r)) => Some(r),
        (Some(c), Some(r)) => Some(format!("{c}\n\n{r}")),
    };
    Some(SpawnContext {
        repo_path: repo.path.clone(),
        worktree: ws.worktree_path.clone(),
        custom,
        additional_dirs,
        yolo: ws.yolo,
    })
}

/// The session this instance should `--resume` on respawn: its recorded
/// harness session id, provided the harness still has that session on disk
/// for `worktree`. `None` means "no exact identity" and callers fall back to
/// the kind's cwd-wide behaviour (`--continue` for a primary, fresh for an
/// added agent).
///
/// Only Claude reports an id today (via its hooks, see
/// `cli::run` `StatusFromHook`), so only Claude instances ever resolve here.
/// The existence check matters: Claude refuses to start on an unknown id,
/// which would leave the pane dead instead of merely un-resumed.
pub(crate) fn recorded_resume_id(
    instance: &crate::data::agents::AgentInstance,
    worktree: &std::path::Path,
) -> Option<String> {
    if instance.agent != crate::pty::session::AgentKind::Claude {
        return None;
    }
    let id = instance.agent_session_id.as_deref()?;
    crate::pty::session::claude_session_exists(worktree, id).then(|| id.to_string())
}

pub(crate) fn build_spawn_info(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
) -> Option<(
    crate::data::store::WorkspaceId,
    std::path::PathBuf,
    crate::pty::session::SpawnMode,
    std::path::PathBuf,
    crate::pty::session::AgentKind,
)> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let agent = ws.agent;
    let doctrine = crate::agent::doctrine::resolve_effective_doctrine(&app.store, agent);
    let ctx = resolve_spawn_context(app, ws_id)?;
    let SpawnContext {
        custom,
        additional_dirs,
        yolo,
        worktree,
        repo_path,
        ..
    } = ctx;
    // An exact recorded session wins over the cwd-wide `has_prior_session_for`
    // probe: once a peer shares this worktree, "most recent session here" may
    // be the peer's, and the snapshot gate is moot for an id this workspace's
    // own instance reported.
    let resume_session_id = app
        .store
        .primary_instance_id(ws_id)
        .ok()
        .flatten()
        .and_then(|id| app.store.workspace_agents_by_id(id).ok().flatten())
        .and_then(|inst| recorded_resume_id(&inst, &worktree));
    let mode = if resume_session_id.is_some()
        || crate::pty::session::has_prior_session_for(&worktree, agent)
    {
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
            doctrine: doctrine.clone(),
            additional_dirs,
            yolo,
            resume_session_id,
        }
    } else {
        let rename_ctx = if crate::util::names::is_generated_slug(&ws.name) {
            let resolved_prefix =
                crate::data::repo::resolve_branch_prefix(repo, &app.store).unwrap_or_default();
            Some(crate::pty::session::RenameContext {
                current_branch: ws.branch.clone(),
                branch_prefix: resolved_prefix,
                repo_name: repo.name.clone(),
                current_slug: ws.name.clone(),
            })
        } else {
            None
        };
        crate::pty::session::SpawnMode::Fresh {
            rename_ctx,
            custom_instructions: custom,
            doctrine,
            additional_dirs,
            yolo,
        }
    };
    Some((ws_id, worktree, mode, repo_path, agent))
}

/// The tmux session name for an instance of a *shared* workspace, or None
/// for direct workspaces.
///
/// `session_ref` is the source of truth for lookup/kill: once an instance has
/// a stored name, that name is returned verbatim and NEVER re-derived. This
/// matters because workspaces are renamed routinely (auto-rename), and a
/// re-derived name would no longer match the live tmux session — `-A` would
/// spin up a SECOND session and orphan the original agent forever.
///
/// A name is derived (and persisted after a successful spawn by the caller)
/// only when `session_ref` is None. At derivation time, if another instance
/// already claims the derived name (a sanitization collision — see
/// `Store::session_ref_in_use`), the workspace id is appended so `-A` can't
/// attach to the wrong agent. Combined with the stored-ref reuse above, the
/// disambiguated name is then stable for the life of the instance.
pub(crate) fn tmux_name_for(
    app: &App,
    ws_id: crate::data::store::WorkspaceId,
    instance: &crate::data::agents::AgentInstance,
) -> Option<String> {
    let (rid, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    if !ws.shared {
        return None;
    }
    // Stored name wins: never re-derive after creation.
    if let Some(existing) = &instance.session_ref {
        return Some(existing.clone());
    }
    let repo = app.repos.iter().find(|r| r.id == *rid)?;
    let derived = crate::pty::tmux::session_name(
        &repo.name,
        &ws.name,
        instance.agent,
        instance.ordinal,
        instance.is_primary,
    );
    // Disambiguate a first-spawn collision with another instance's stored ref.
    match app.store.session_ref_in_use(&derived, instance.id) {
        Ok(true) => Some(format!("{derived}-{}", ws_id.0)),
        _ => Some(derived),
    }
}

/// Build spawn parameters for an *added* (non-primary) instance.
///
/// First spawn is `Fresh` with an injected handoff note so the agent
/// re-orients from the shared worktree + git diff. Once the instance has
/// reported its own harness session id (`recorded_resume_id`), a respawn —
/// typically wsx being quit and reopened — is `Continue` with that exact id,
/// so the peer gets its conversation back rather than a blank chat. The
/// handoff note rides along on resume too: it is the peer's only
/// system-prompt statement of who it is and how to reach the others, and
/// Claude does not persist system prompts across resumes.
///
/// The cwd-wide resume (`--continue`) is never used here: it would reopen
/// whichever agent in the worktree spoke last, usually the primary.
/// Returns `(worktree, SpawnMode, repo_path)`.
pub(crate) fn build_added_spawn_info(
    app: &App,
    instance: &crate::data::agents::AgentInstance,
) -> Option<(
    std::path::PathBuf,
    crate::pty::session::SpawnMode,
    std::path::PathBuf,
)> {
    let ws_id = instance.workspace_id;
    let (_, ws) = app.workspaces.iter().find(|(_, w)| w.id == ws_id)?;
    let repo = app.repos.iter().find(|r| r.id == ws.repo_id)?;
    let base_ref = repo.base_branch.as_deref().unwrap_or("main");
    // The primary instance's label, for the handoff note's "alongside `X`" line.
    let primary_label = app
        .store
        .workspace_agents(ws_id)
        .ok()
        .and_then(|agents| agents.into_iter().find(|a| a.is_primary).map(|a| a.label()))
        .unwrap_or_else(|| "the primary agent".to_string());
    let note = crate::agent::handoff::context_note(
        instance.agent,
        &crate::agent::handoff::HandoffContext {
            primary_label: &primary_label,
            branch: &ws.branch,
            base_ref,
            workspace_name: &ws.name,
        },
    );
    let ctx = resolve_spawn_context(app, ws_id)?;
    // Put the handoff note LAST so repo/related context precedes it.
    let custom_instructions = match ctx.custom {
        Some(c) => format!("{c}\n\n{note}"),
        None => note,
    };
    let doctrine = crate::agent::doctrine::resolve_effective_doctrine(&app.store, instance.agent);
    let mode = match recorded_resume_id(instance, &ctx.worktree) {
        Some(id) => crate::pty::session::SpawnMode::Continue {
            custom_instructions: Some(custom_instructions),
            doctrine,
            additional_dirs: ctx.additional_dirs,
            yolo: ctx.yolo,
            resume_session_id: Some(id),
        },
        None => crate::pty::session::SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some(custom_instructions),
            doctrine,
            additional_dirs: ctx.additional_dirs,
            yolo: ctx.yolo,
        },
    };
    Some((ctx.worktree, mode, ctx.repo_path))
}

#[cfg(test)]
mod added_spawn_tests {
    use super::*;
    use crate::data::store::NewWorkspace;
    use crate::pty::session::{AgentKind, SpawnMode};
    use tempfile::TempDir;

    #[test]
    fn build_added_spawn_info_is_fresh_with_handoff_note() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "feat",
                branch: "wsx/feat",
                worktree_path: std::path::Path::new("/tmp/r/feat"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        let added = store.add_workspace_agent(ws, AgentKind::Codex).unwrap();

        let tmp = TempDir::new().unwrap();
        let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
        app.refresh().unwrap();

        let (_worktree, mode, _repo_path) =
            build_added_spawn_info(&app, &added).expect("spawn info");
        match mode {
            SpawnMode::Fresh {
                rename_ctx,
                custom_instructions,
                ..
            } => {
                assert!(rename_ctx.is_none(), "added agents never rename");
                let note = custom_instructions.expect("handoff note present");
                // References the primary's label, the branch, and the
                // base-ref-driven git diff hint (default "main").
                assert!(note.contains("claude"), "note mentions primary: {note}");
                assert!(note.contains("wsx/feat"), "note mentions branch: {note}");
                assert!(
                    note.contains("git diff main...HEAD"),
                    "note mentions base ref: {note}"
                );
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    /// A workspace on a real (canonicalizable) worktree, with a fake `$HOME`
    /// holding one Claude session file for it. Returns the app, the primary
    /// and the added instance, and the seeded session id. The `EnvGuard`
    /// must outlive the assertions — it pins `HOME` to the fake dir.
    fn app_with_claude_session(
        added_kind: AgentKind,
    ) -> (
        App,
        crate::data::agents::AgentInstance,
        crate::data::agents::AgentInstance,
        String,
        TempDir,
        TempDir,
        crate::test_support::EnvGuard,
    ) {
        let home = TempDir::new().unwrap();
        let worktree = TempDir::new().unwrap();
        let abs = std::fs::canonicalize(worktree.path()).unwrap();
        let encoded = crate::activity::events::encode_cwd(&abs);
        let dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&dir).unwrap();
        let sid = "656c166a-911b-4375-9db9-007b8456f3e3".to_string();
        std::fs::write(dir.join(format!("{sid}.jsonl")), "{}").unwrap();

        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "feat",
                branch: "wsx/feat",
                worktree_path: worktree.path(),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let primary = store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        let added = store.add_workspace_agent(ws, added_kind).unwrap();

        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        let state_dir = TempDir::new().unwrap();
        let mut app = App::new(store, state_dir.path().to_path_buf()).unwrap();
        app.refresh().unwrap();
        (app, primary, added, sid, home, worktree, env)
    }

    #[test]
    fn added_claude_with_recorded_live_session_resumes_it_with_the_note() {
        let (app, _primary, added, sid, _home, _wt, _env) =
            app_with_claude_session(AgentKind::Claude);
        app.store
            .set_instance_agent_session(added.id, &sid)
            .unwrap();
        let added = app.store.workspace_agents_by_id(added.id).unwrap().unwrap();

        let (_wt, mode, _repo) = build_added_spawn_info(&app, &added).expect("spawn info");
        match mode {
            SpawnMode::Continue {
                resume_session_id,
                custom_instructions,
                ..
            } => {
                assert_eq!(resume_session_id.as_deref(), Some(sid.as_str()));
                let note = custom_instructions.expect("handoff note still injected");
                assert!(note.contains("wsx agent send"), "{note}");
            }
            other => panic!("expected Continue by id, got {other:?}"),
        }
    }

    #[test]
    fn added_claude_with_recorded_but_missing_session_spawns_fresh() {
        let (app, _primary, added, _sid, _home, _wt, _env) =
            app_with_claude_session(AgentKind::Claude);
        app.store
            .set_instance_agent_session(added.id, "0000-gone")
            .unwrap();
        let added = app.store.workspace_agents_by_id(added.id).unwrap().unwrap();

        let (_wt, mode, _repo) = build_added_spawn_info(&app, &added).expect("spawn info");
        assert!(
            matches!(mode, SpawnMode::Fresh { .. }),
            "claude would refuse an unknown id; got {mode:?}"
        );
    }

    #[test]
    fn added_claude_without_a_recorded_session_never_uses_cwd_wide_continue() {
        // The primary's session is on disk in this worktree; an unreported
        // peer must not `--continue` into it.
        let (app, _primary, added, _sid, _home, _wt, _env) =
            app_with_claude_session(AgentKind::Claude);
        let (_wt, mode, _repo) = build_added_spawn_info(&app, &added).expect("spawn info");
        assert!(matches!(mode, SpawnMode::Fresh { .. }), "got {mode:?}");
    }

    #[test]
    fn added_non_claude_ignores_a_recorded_session_id() {
        let (app, _primary, added, sid, _home, _wt, _env) =
            app_with_claude_session(AgentKind::Codex);
        app.store
            .set_instance_agent_session(added.id, &sid)
            .unwrap();
        let added = app.store.workspace_agents_by_id(added.id).unwrap().unwrap();
        let (_wt, mode, _repo) = build_added_spawn_info(&app, &added).expect("spawn info");
        assert!(matches!(mode, SpawnMode::Fresh { .. }), "got {mode:?}");
    }

    #[test]
    fn primary_claude_with_recorded_live_session_resumes_by_id() {
        let (app, primary, _added, sid, _home, _wt, _env) =
            app_with_claude_session(AgentKind::Claude);
        app.store
            .set_instance_agent_session(primary.id, &sid)
            .unwrap();
        let ws_id = primary.workspace_id;

        let (_id, _wt, mode, _repo, _agent) = build_spawn_info(&app, ws_id).expect("spawn info");
        match mode {
            SpawnMode::Continue {
                resume_session_id, ..
            } => assert_eq!(resume_session_id.as_deref(), Some(sid.as_str())),
            other => panic!("expected Continue by id, got {other:?}"),
        }
    }

    #[test]
    fn primary_claude_without_a_recorded_session_keeps_cwd_wide_continue() {
        // Pre-existing workspaces have no recorded id; the old `--continue`
        // path (a session file present, no snapshot gate) must still apply.
        let (app, primary, _added, _sid, _home, _wt, _env) =
            app_with_claude_session(AgentKind::Claude);
        let (_id, _wt, mode, _repo, _agent) =
            build_spawn_info(&app, primary.workspace_id).expect("spawn info");
        match mode {
            SpawnMode::Continue {
                resume_session_id, ..
            } => assert_eq!(resume_session_id, None),
            other => panic!("expected plain Continue, got {other:?}"),
        }
    }
}
