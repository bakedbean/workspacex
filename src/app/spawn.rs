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
    let mode = if crate::pty::session::has_prior_session_for(&worktree, agent) {
        crate::pty::session::SpawnMode::Continue {
            custom_instructions: custom,
            doctrine: doctrine.clone(),
            additional_dirs,
            yolo,
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

/// Build spawn parameters for an *added* (non-primary) instance. Added agents
/// always spawn `Fresh` with an injected handoff note so they re-orient from
/// the shared worktree + git diff (added agents never resume a session).
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
    let mode = crate::pty::session::SpawnMode::Fresh {
        rename_ctx: None,
        custom_instructions: Some(custom_instructions),
        doctrine,
        additional_dirs: ctx.additional_dirs,
        yolo: ctx.yolo,
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
}
