//! Lookups and value validation shared by more than one `run` arm.
//!
//! Resolving "which workspace am I in?", turning a `<repo>/<slug>` spec
//! into ids, and normalizing setting values before they reach the store.

use crate::error::{Error, Result};

/// Resolve the workspace the current `wsx` invocation is acting within:
/// prefer the `WSX_WORKSPACE_ID` env var (set when wsx spawns an agent), else
/// fall back to matching the current directory against known worktree paths.
pub(in crate::cli) fn resolve_current_workspace(
    store: &crate::data::store::Store,
) -> Result<crate::data::store::Workspace> {
    use crate::data::store::WorkspaceId;
    // 1. WSX_WORKSPACE_ID (reliable for agent-initiated calls)
    if let Ok(s) = std::env::var("WSX_WORKSPACE_ID") {
        if let Ok(id) = s.parse::<i64>() {
            if let Some(ws) = store.workspace_by_id(WorkspaceId(id))? {
                return Ok(ws);
            }
        }
    }
    // 2. cwd: find the workspace whose worktree_path is an ancestor-or-equal of cwd
    // Note: this is a raw path-prefix match. If the user `cd`'d into the
    // worktree through a symlink (e.g. macOS /var -> /private/var), cwd may not
    // prefix the stored worktree_path and the match will miss. Setting
    // WSX_WORKSPACE_ID (the agent-spawn path) avoids this entirely.
    let cwd = std::env::current_dir()
        .map_err(|e| Error::UserInput(format!("cannot determine current directory: {e}")))?;
    let ws = store
        .all_workspaces()?
        .into_iter()
        .filter(|w| cwd.starts_with(&w.worktree_path))
        .max_by_key(|w| w.worktree_path.as_os_str().len())
        .ok_or_else(|| {
            Error::UserInput(
                "not inside a wsx workspace (set WSX_WORKSPACE_ID or run from a worktree)".into(),
            )
        })?;
    Ok(ws)
}

/// Effective yolo + agent for a new workspace: explicit flags win, then the
/// parent workspace (the one this `wsx` invocation runs inside, if any), then
/// `default_agent` (the `coding_agent` setting — the same default the TUI's
/// create modal uses). Inheritance means an agent handing work to a sibling
/// workspace doesn't need to know — and can't reliably know — its own
/// workspace's yolo state or agent kind. Pure so it can be unit-tested
/// without the process-global env/cwd that `resolve_current_workspace` reads.
pub(in crate::cli) fn effective_create_flags(
    explicit_yolo: bool,
    explicit_agent: Option<&str>,
    parent: Option<&crate::data::store::Workspace>,
    default_agent: crate::pty::session::AgentKind,
) -> (bool, crate::pty::session::AgentKind) {
    let yolo = explicit_yolo || parent.is_some_and(|p| p.yolo);
    let agent = match explicit_agent {
        Some(_) => crate::pty::session::AgentKind::from_str_or_default(explicit_agent),
        None => match parent {
            Some(p) => p.agent,
            None => default_agent,
        },
    };
    (yolo, agent)
}

pub(in crate::cli) fn lookup_repo(
    store: &crate::data::store::Store,
    name: &str,
) -> Result<crate::data::store::Repo> {
    crate::data::repo::list(store)?
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))
}

pub(in crate::cli) fn lookup_workspace(
    store: &crate::data::store::Store,
    repo: &crate::data::store::Repo,
    name: &str,
) -> Result<crate::data::store::Workspace> {
    store
        .workspaces(repo.id)?
        .into_iter()
        .find(|w| w.name == name)
        .ok_or_else(|| Error::UserInput(format!("no workspace named {name} in repo {}", repo.name)))
}

/// Resolve a `--workspace <repo>/<slug>` spec to a workspace.
///
/// Splits on the LAST `/`: repo names may contain spaces and other
/// characters, but a workspace slug never contains `/` (the same assumption
/// `app::ipc::parse_line` makes). Errors list the valid alternatives, because
/// the caller is usually an agent that cannot enumerate them itself.
pub(in crate::cli) fn resolve_workspace_spec(
    store: &crate::data::store::Store,
    spec: &str,
) -> Result<crate::data::store::Workspace> {
    let malformed = || Error::UserInput(format!("--workspace expects <repo>/<slug>, got '{spec}'"));
    let (repo_name, slug) = spec.rsplit_once('/').ok_or_else(malformed)?;
    if repo_name.is_empty() || slug.is_empty() {
        return Err(malformed());
    }
    let repos = crate::data::repo::list(store)?;
    let repo = repos.iter().find(|r| r.name == repo_name).ok_or_else(|| {
        Error::UserInput(format!(
            "--workspace: no repo named '{repo_name}'; known repos: {}",
            join_or_none(repos.iter().map(|r| r.name.as_str()))
        ))
    })?;
    let workspaces = store.workspaces(repo.id)?;
    workspaces
        .iter()
        .find(|w| w.name == slug)
        .cloned()
        .ok_or_else(|| {
            Error::UserInput(format!(
                "--workspace: no workspace '{slug}' in repo '{repo_name}'; known: {}",
                join_or_none(workspaces.iter().map(|w| w.name.as_str()))
            ))
        })
}

/// Comma-join names for an error hint, or `(none)` when the list is empty.
pub(in crate::cli) fn join_or_none<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let v: Vec<&str> = names.collect();
    if v.is_empty() {
        "(none)".to_string()
    } else {
        v.join(", ")
    }
}

/// The `wsx agent send` invocation that resends a starter prompt whose
/// enqueue failed after the workspace was already created.
///
/// Every dynamic part is shell-quoted: repo names may contain spaces (the
/// `<repo>/<slug>` spec splits on the LAST slash precisely because of that)
/// and a prompt is arbitrary text. An unquoted hint would be a command the
/// user cannot actually paste.
pub(in crate::cli) fn retry_send_hint(repo: &str, slug: &str, prompt: &str) -> String {
    fn shquote(s: &str) -> String {
        shlex::try_quote(s)
            .map(|c| c.into_owned())
            // Only fails on interior NUL, which cannot reach here through
            // sqlite TEXT or a CLI arg; drop the byte rather than emit an
            // unquoted arg.
            .unwrap_or_else(|_| format!("'{}'", s.replace(['\'', '\0'], "")))
    }
    format!(
        "wsx agent send --workspace {} primary {}",
        shquote(&format!("{repo}/{slug}")),
        shquote(prompt)
    )
}

/// Queue `body` for `target` and warn when nothing will deliver it.
///
/// The CLI only ever writes to the store; the dashboard is the sole thing
/// that injects queued messages into an agent PTY (`App::drain_agent_messages`
/// spawns the target on demand). So without a live TUI the enqueue is a no-op
/// the sender would never notice — not an error, since the row is queued
/// rather than lost, but worth saying out loud.
///
/// Shared by `agent send` and `workspace create --prompt` so the two can't
/// drift apart on sender attribution or on that warning.
pub(in crate::cli) fn enqueue_for_agent(
    store: &crate::data::store::Store,
    workspace: crate::data::store::WorkspaceId,
    target: crate::data::store::AgentInstanceId,
    body: &str,
) -> Result<()> {
    let from = std::env::var("WSX_AGENT_INSTANCE_ID")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .map(crate::data::store::AgentInstanceId);
    store.enqueue_message(workspace, target, from, body)?;
    if !crate::app::ipc::any_live_tui() {
        eprintln!(
            "warning: no wsx dashboard is running — this message is queued and \
             will not be delivered until one starts. Tell the user to open `wsx`."
        );
    }
    Ok(())
}

pub(in crate::cli) fn open_in_editor(key: &str, initial: &str) -> Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let dir = std::env::temp_dir();
    let path = dir.join(format!("wsx-{key}-{}.txt", std::process::id()));
    std::fs::write(&path, initial)?;
    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| Error::UserInput(format!("spawn editor {editor}: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_file(&path);
        return Err(Error::UserInput(format!(
            "editor {editor} exited with {status}"
        )));
    }
    let value = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(value)
}

/// Seed text for the editor when the global `detail_bar_config`
/// setting is empty — the pretty-printed default config.
pub(in crate::cli) fn detail_bar_config_seed_for_empty() -> String {
    serde_json::to_string_pretty(&crate::config::detail_bar_config::DetailBarConfig::default())
        .unwrap_or_else(|_| "{}".to_string())
}

/// Parse, sanitize, and re-serialize a global `detail_bar_config`
/// blob. Returns the pretty-printed normalized JSON.
pub(in crate::cli) fn detail_bar_config_validate_and_normalize(raw: &str) -> Result<String> {
    let mut cfg: crate::config::detail_bar_config::DetailBarConfig = serde_json::from_str(raw)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: invalid JSON: {e}")))?;
    cfg.sanitize();
    serde_json::to_string_pretty(&cfg)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: serialize failed: {e}")))
}

/// Validate a `usage_graph_window` value: accept only the canonical tokens
/// (`24h`/`1w`/`1mo`), ignoring surrounding whitespace, and store the trimmed
/// canonical form. Rejects anything else so a CLI typo fails loudly instead of
/// silently falling back to `24h` at render time.
pub(in crate::cli) fn usage_window_validate_and_normalize(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if crate::config::usage_window::UsageWindow::ALL
        .iter()
        .any(|w| w.as_setting() == trimmed)
    {
        Ok(trimmed.to_string())
    } else {
        Err(Error::UserInput(format!(
            "usage_graph_window: expected one of 24h, 1w, 1mo (got {trimmed:?})"
        )))
    }
}
