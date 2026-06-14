//! Per-agent command construction.
//!
//! Builds the `CommandBuilder` for each [`AgentKind`] (claude/pi/hermes/codex)
//! from a worktree path + [`SpawnMode`], including rename system-prompt
//! rendering and the AGENTS.md-injected prompt composition. Pure functions over
//! paths/modes — no `Session` state. Re-exported from `pty::session` so the
//! spawn path and the existing call sites keep resolving the builders
//! unqualified.

use crate::pty::AgentKind;
use crate::pty::session::{SpawnMode, latest_hermes_session_id_default};
use portable_pty::CommandBuilder;
use std::path::Path;

/// Build a `CommandBuilder` for `claude` (or whatever `WSX_CLAUDE_BIN`
/// points to) inside `cwd`. Inherits the current process env.
///
/// When `mode` is `Fresh { rename_ctx: Some(_) }` and `WSX_RENAME_MODE` is
/// `claude` (the default), appends a system-prompt instruction directing
/// claude to rename the workspace based on the user's first message, plus
/// pre-authorizes `Bash(wsx workspace rename:*)` so the rename runs without a
/// permission prompt. When `mode` is `Continue`, passes `--continue` so
/// claude resumes the most recent persisted session for this worktree.
pub fn build_claude_command(
    cwd: &Path,
    mode: &SpawnMode,
    remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    let (
        doctrine,
        rename_prompt,
        custom,
        allow_wsx_rename,
        add_continue,
        skip_permissions,
        add_dirs,
    ) = match mode {
        SpawnMode::Continue {
            custom_instructions,
            doctrine,
            additional_dirs,
            yolo,
        } => (
            doctrine.clone(),
            None,
            custom_instructions.clone(),
            false,
            true,
            *yolo,
            additional_dirs.clone(),
        ),
        SpawnMode::Fresh {
            rename_ctx,
            custom_instructions,
            doctrine,
            additional_dirs,
            yolo,
        } => {
            let rename_mode =
                std::env::var("WSX_RENAME_MODE").unwrap_or_else(|_| "claude".to_string());
            let (rp, allow) = if let Some(ctx) = rename_ctx {
                if rename_mode == "claude" {
                    (
                        Some(render_rename_system_prompt(
                            &ctx.current_branch,
                            &ctx.branch_prefix,
                            &ctx.repo_name,
                            &ctx.current_slug,
                        )),
                        true,
                    )
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };
            (
                doctrine.clone(),
                rp,
                custom_instructions.clone(),
                allow,
                false,
                *yolo,
                additional_dirs.clone(),
            )
        }
        SpawnMode::ProjectManager {
            workspaces_json_path: _,
            custom_instructions,
            additional_dirs,
            resume,
            fast_mode: _, // emitted below, after the match
        } => (
            None,
            Some(crate::agent::pm::pm_system_prompt(
                custom_instructions.as_deref(),
            )),
            None,
            false,
            *resume,
            true,
            additional_dirs.clone(),
        ),
    };

    for dir in &add_dirs {
        cmd.arg("--add-dir");
        cmd.arg(dir);
    }

    if add_continue {
        cmd.arg("--continue");
    }

    if skip_permissions {
        cmd.arg("--dangerously-skip-permissions");
    } else if allow_wsx_rename {
        cmd.arg("--allowedTools");
        cmd.arg("Bash(wsx workspace rename:*)");
    }

    if remote.enabled {
        cmd.arg("--remote-control");
        if remote.sandbox {
            cmd.arg("--sandbox");
        }
    }

    // Status-reporting wiring goes to the developer agents (Fresh/Continue) via
    // the harness-agnostic spawn_wiring() entry point; the PM pane keeps just
    // its fastMode flag. The wiring points at the running wsx binary by
    // absolute path so PATH differences can't break the callback.
    let pm_fast = matches!(
        mode,
        SpawnMode::ProjectManager {
            fast_mode: true,
            ..
        }
    );
    let inject_status = matches!(mode, SpawnMode::Fresh { .. } | SpawnMode::Continue { .. });
    if inject_status {
        let wsx_bin = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("wsx"));
        if let Some(wiring) =
            crate::agent::status::for_agent(AgentKind::Claude).spawn_wiring(&wsx_bin, false)
        {
            for arg in wiring.args {
                cmd.arg(arg);
            }
        }
    } else if pm_fast {
        cmd.arg("--settings");
        cmd.arg(r#"{"fastMode":true}"#);
    }

    let parts: Vec<String> = [doctrine, rename_prompt, custom]
        .into_iter()
        .flatten()
        .collect();
    let combined = if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    };

    if let Some(prompt) = combined {
        cmd.arg("--append-system-prompt");
        cmd.arg(prompt);
    }

    cmd
}

/// Single-quote a string for embedding in a shell command shown to the
/// agent. Handles internal single quotes via the `'\''` escape so the
/// agent renders a valid `wsx workspace rename` invocation even when
/// repo names contain spaces or shell metacharacters.
pub(crate) fn shell_quote(s: &str) -> String {
    let escaped = s.replace('\'', r"'\''");
    format!("'{escaped}'")
}

pub(crate) fn render_rename_system_prompt(
    current_branch: &str,
    _branch_prefix: &str,
    repo_name: &str,
    current_slug: &str,
) -> String {
    let quoted_repo = shell_quote(repo_name);
    let quoted_slug = shell_quote(current_slug);
    format!(
        "This is a wsx-managed worktree currently checked out on a placeholder branch \
         named `{current_branch}`. The placeholder slug is `{current_slug}` (auto-generated \
         adjective+plant from the wsx workspace manager).\n\n\
         BEFORE doing the work the user asks about, on their first message: \
         run `wsx workspace rename {quoted_repo} {quoted_slug} <slug>` where `<slug>` is a \
         2-4 word lowercase kebab-case summary of what the user is asking for. \
         This command updates both the git branch and the wsx workspace registry — do \
         NOT run `git branch -m` directly, since that leaves wsx's database stale. \
         After renaming, briefly tell the user \"renamed workspace to <slug>\" on one line \
         and proceed with their actual request.\n\n\
         Constraints:\n\
         - Slug: lowercase, 2-4 words, hyphen-separated, max ~32 chars. Do NOT include the \
         branch prefix — wsx prepends it automatically.\n\
         - Don't ask for confirmation; don't add extra explanation.\n\
         - Only do this once per worktree. If the current branch is no longer \
         the placeholder `{current_branch}`, skip the rename — it's already done.\n"
    )
}

/// Build a `CommandBuilder` for `pi` (or whatever `WSX_PI_BIN`
/// points to) inside `cwd`. Inherits the current process env.
///
/// Maps wsx spawn modes to pi CLI flags:
/// - `Fresh` with `rename_ctx` → system prompt for auto-rename
/// - `Continue` → `--continue`
/// - `ProjectManager` → system prompt + `--continue` if resuming
///
/// Pi has no permission system, so yolo/--dangerously-skip-permissions
/// and --allowedTools are no-ops. Pi has no --add-dir or --remote-control
/// equivalents. Pi can read from any path directly.
pub fn build_pi_command(
    cwd: &Path,
    mode: &SpawnMode,
    _remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_PI_BIN").unwrap_or_else(|_| "pi".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    // Suppress pi's startup npm chatter and update checks.
    cmd.env("PI_OFFLINE", "1");
    cmd.env("npm_config_loglevel", "error");

    let (doctrine, rename_prompt, custom, add_continue) = match mode {
        SpawnMode::Continue {
            custom_instructions,
            doctrine,
            additional_dirs: _,
            yolo: _,
        } => (doctrine.clone(), None, custom_instructions.clone(), true),
        SpawnMode::Fresh {
            rename_ctx,
            custom_instructions,
            doctrine,
            additional_dirs: _,
            yolo: _,
        } => {
            let rename_mode =
                std::env::var("WSX_RENAME_MODE").unwrap_or_else(|_| "claude".to_string());
            let rp = if let Some(ctx) = rename_ctx {
                if rename_mode == "claude" {
                    Some(render_rename_system_prompt_pi(
                        &ctx.current_branch,
                        &ctx.branch_prefix,
                        &ctx.repo_name,
                        &ctx.current_slug,
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            (doctrine.clone(), rp, custom_instructions.clone(), false)
        }
        SpawnMode::ProjectManager {
            workspaces_json_path: _,
            custom_instructions,
            additional_dirs: _,
            resume,
            fast_mode: _, // pi has no fast mode
        } => (
            None,
            Some(crate::agent::pm::pm_system_prompt(
                custom_instructions.as_deref(),
            )),
            None,
            *resume,
        ),
    };

    if add_continue {
        cmd.arg("--continue");
    } else {
        // Model selection for new pi sessions.
        //
        // Pi silently ignores `--provider` unless `--model` is also passed
        // (see pi's resolveCliModel: it short-circuits when cliModel is empty),
        // so we always pass a model selector. Precedence:
        //   1. WSX_PI_MODEL — explicit model pattern, e.g. "claude-sonnet-4-5"
        //      or "deepseek/deepseek-v4-pro". Pi resolves via substring/exact.
        //   2. WSX_PI_PROVIDER — scope to that provider via `--models "<p>/*"`
        //      (plural `--models` accepts globs; singular `--model` does not).
        //   3. Default to the deepseek provider.
        //
        // Empty/whitespace env var values are treated as unset — shells expand
        // `export FOO=$BAR` to "" when $BAR is unset, and we don't want to
        // emit `--model ""` (re-triggers the pi short-circuit) or `--models
        // "/*"` (malformed glob).
        let model = std::env::var("WSX_PI_MODEL")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let provider = std::env::var("WSX_PI_PROVIDER")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        if let Some(model) = model {
            cmd.arg("--model");
            cmd.arg(&model);
            if let Some(provider) = provider {
                cmd.arg("--provider");
                cmd.arg(&provider);
            }
        } else {
            let provider = provider.unwrap_or_else(|| "deepseek".to_string());
            cmd.arg("--models");
            cmd.arg(format!("{provider}/*"));
        }
    }

    let parts: Vec<String> = [doctrine, rename_prompt, custom]
        .into_iter()
        .flatten()
        .collect();
    let combined = if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    };

    if let Some(prompt) = combined {
        cmd.arg("--append-system-prompt");
        cmd.arg(prompt);
    }

    cmd
}

/// Build a `CommandBuilder` for `hermes chat` (or whatever `WSX_HERMES_BIN`
/// points to) inside `cwd`. Inherits the current process env.
///
/// Maps wsx spawn modes to Hermes CLI flags:
/// - `Fresh` → bare `hermes chat`, no continue/resume.
/// - `Continue` → `--resume <id>` if a prior wsx session exists for this cwd,
///   otherwise silently launches fresh (better than bare `--continue` which
///   would resume the globally-most-recent Hermes session regardless of cwd).
/// - `ProjectManager` → `--resume <id>` if `resume`, always `--yolo`.
///
/// Model selection uses env-var precedence:
///   1. `WSX_HERMES_MODEL` → set `HERMES_INFERENCE_MODEL` env var on the child
///      (works in all Hermes modes, unlike `--model` which is `-z/--tui` only).
///   2. `WSX_HERMES_PROVIDER` → forward as `--provider <value>` (may be a no-op
///      in classic REPL per Hermes docs; persistent provider lives in
///      `~/.hermes/config.yaml`).
///
/// `--worktree` is never emitted — wsx manages worktrees itself; passing it
/// would double-isolate.
///
/// Prompt injection (rename / custom_instructions / PM prompt) is handled
/// separately by `prepare_hermes_workspace`, which writes a wsx-managed
/// block into `AGENTS.md`.
pub fn build_hermes_command(
    cwd: &Path,
    mode: &SpawnMode,
    _remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_HERMES_BIN").unwrap_or_else(|_| "hermes".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    cmd.arg("chat");

    // Note: we deliberately do NOT pass `--source`. Hermes's interactive chat
    // hardcodes platform="cli" at session creation, preempting both the
    // --source flag (which only affects `sessions list` filtering) and the
    // HERMES_SESSION_SOURCE env var. Per-cwd session detection is achieved
    // via the spawn-timestamp marker (see write_hermes_spawn_marker /
    // latest_hermes_session_id_default) instead.

    let (add_continue, add_yolo) = match mode {
        SpawnMode::Continue { yolo, .. } => (true, *yolo),
        SpawnMode::Fresh { yolo, .. } => (false, *yolo),
        SpawnMode::ProjectManager { resume, .. } => (*resume, true),
    };

    if add_continue {
        if let Some(id) = latest_hermes_session_id_default(cwd) {
            cmd.arg("--resume");
            cmd.arg(&id);
        }
        // No prior wsx session → silently launch fresh.
    }
    if add_yolo {
        cmd.arg("--yolo");
    }

    let model = std::env::var("WSX_HERMES_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let provider = std::env::var("WSX_HERMES_PROVIDER")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(m) = &model {
        cmd.env("HERMES_INFERENCE_MODEL", m);
    }
    if let Some(p) = &provider {
        cmd.arg("--provider");
        cmd.arg(p);
    }

    cmd
}

/// Pi version of the rename system prompt. Pi uses `bash` (lowercase) as its
/// tool name and has no permission system, so we don't need to
/// pre-authorize the wsx workspace rename command.
pub(crate) fn render_rename_system_prompt_pi(
    current_branch: &str,
    _branch_prefix: &str,
    repo_name: &str,
    current_slug: &str,
) -> String {
    let quoted_repo = shell_quote(repo_name);
    let quoted_slug = shell_quote(current_slug);
    format!(
        "This is a wsx-managed worktree currently checked out on a placeholder branch \
         named `{current_branch}`. The placeholder slug is `{current_slug}` (auto-generated \
         adjective+plant from the wsx workspace manager).\n\n\
         BEFORE doing the work the user asks about, on their first message: \
         run `wsx workspace rename {quoted_repo} {quoted_slug} <slug>` where `<slug>` is a \
         2-4 word lowercase kebab-case summary of what the user is asking for. \
         This command updates both the git branch and the wsx workspace registry — do \
         NOT run `git branch -m` directly, since that leaves wsx's database stale. \
         After renaming, briefly tell the user \"renamed workspace to <slug>\" on one line \
         and proceed with their actual request.\n\n\
         Constraints:\n\
         - Slug: lowercase, 2-4 words, hyphen-separated, max ~32 chars. Do NOT include the \
         branch prefix — wsx prepends it automatically.\n\
         - Don't ask for confirmation; don't add extra explanation.\n\
         - Only do this once per worktree. If the current branch is no longer \
         the placeholder `{current_branch}`, skip the rename — it's already done.\n"
    )
}

/// Hermes version of the rename system prompt. Today the text is identical to
/// the Pi version — Hermes has no permission system and uses plain bash, same
/// as Pi. Keep this function distinct from the Pi helper so future divergence
/// (e.g., a Hermes-specific tool naming convention) is a one-place change.
pub(crate) fn render_rename_system_prompt_hermes(
    current_branch: &str,
    branch_prefix: &str,
    repo_name: &str,
    current_slug: &str,
) -> String {
    render_rename_system_prompt_pi(current_branch, branch_prefix, repo_name, current_slug)
}

/// Decide what text to inject into the wsx-managed block of AGENTS.md for a
/// given Hermes spawn mode. Returns None when nothing needs injecting.
pub(crate) fn compose_injected_prompt(mode: &SpawnMode) -> Option<String> {
    let (doctrine, rename, custom) = match mode {
        SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions,
            doctrine,
            ..
        } => (
            doctrine.clone(),
            Some(render_rename_system_prompt_hermes(
                &ctx.current_branch,
                &ctx.branch_prefix,
                &ctx.repo_name,
                &ctx.current_slug,
            )),
            custom_instructions.clone(),
        ),
        SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions,
            doctrine,
            ..
        }
        | SpawnMode::Continue {
            custom_instructions,
            doctrine,
            ..
        } => (doctrine.clone(), None, custom_instructions.clone()),
        SpawnMode::ProjectManager {
            custom_instructions,
            ..
        } => (
            None,
            Some(crate::agent::pm::pm_system_prompt(
                custom_instructions.as_deref(),
            )),
            None,
        ),
    };

    let parts: Vec<String> = [doctrine, rename, custom].into_iter().flatten().collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Build a `CommandBuilder` for `codex` (or whatever `WSX_CODEX_BIN` points to)
/// inside `cwd`. Inherits the current process env.
///
/// Spawn-mode mapping:
/// - `Fresh`            → `codex`
/// - `Continue`         → `codex resume --last` (cwd-filtered by Codex itself)
/// - `ProjectManager`   → `codex [resume --last]` + `--ask-for-approval never
///                         --sandbox read-only` (PM reads only, never prompts)
///
/// `yolo` adds `--dangerously-bypass-approvals-and-sandbox`. Non-yolo dev
/// sessions pass no approval flags, inheriting Codex's interactive defaults.
/// `WSX_CODEX_MODEL` (trimmed, non-empty) adds `-m <model>`.
///
/// Codex has no `--append-system-prompt`; instruction injection (doctrine /
/// rename / custom / PM prompt) is handled by `prepare_codex_workspace` via
/// AGENTS.md. The `remote` arg is unused — wsx's RemoteOpts targets Claude's
/// `--remote-control`, which is unrelated to Codex's `--remote`.
pub fn build_codex_command(
    cwd: &Path,
    mode: &SpawnMode,
    _remote: crate::agent::remote_control::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    // Status reporting: developer sessions (Fresh/Continue) get `-c notify=...`
    // so Codex calls back into `wsx status from-notify` on agent-turn-complete.
    // The PM pane is excluded, matching the Claude spawn. `-c` is a global flag
    // and is accepted before any subcommand (`resume`).
    if matches!(mode, SpawnMode::Fresh { .. } | SpawnMode::Continue { .. }) {
        let wsx_bin = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("wsx"));
        if let Some(wiring) =
            crate::agent::status::for_agent(AgentKind::Codex).spawn_wiring(&wsx_bin, false)
        {
            for arg in wiring.args {
                cmd.arg(arg);
            }
        }
    }

    let (resume, yolo, pm) = match mode {
        SpawnMode::Fresh { yolo, .. } => (false, *yolo, false),
        SpawnMode::Continue { yolo, .. } => (true, *yolo, false),
        SpawnMode::ProjectManager { resume, .. } => (*resume, false, true),
    };

    if resume {
        cmd.arg("resume");
        cmd.arg("--last");
    }

    if yolo {
        cmd.arg("--dangerously-bypass-approvals-and-sandbox");
    } else if pm {
        cmd.arg("--ask-for-approval");
        cmd.arg("never");
        cmd.arg("--sandbox");
        cmd.arg("read-only");
    }

    let model = std::env::var("WSX_CODEX_MODEL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(m) = model {
        cmd.arg("-m");
        cmd.arg(&m);
    }

    cmd
}
