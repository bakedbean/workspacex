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
// `RenameContext` is only constructed by this module's co-located tests.
#[cfg(test)]
use crate::pty::session::RenameContext;
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
fn shell_quote(s: &str) -> String {
    let escaped = s.replace('\'', r"'\''");
    format!("'{escaped}'")
}

fn render_rename_system_prompt(
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
fn render_rename_system_prompt_pi(
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
fn render_rename_system_prompt_hermes(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EnvGuard, cat_path};
    use std::path::PathBuf;

    #[test]
    fn system_prompt_combines_rename_and_custom() {
        let ctx = RenameContext {
            current_branch: "wsx/bold-fern".into(),
            branch_prefix: "wsx".into(),
            repo_name: "myrepo".into(),
            current_slug: "bold-fern".into(),
        };
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions: Some("Use tabs not spaces".into()),
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("--append-system-prompt should be present");
        let prompt = argv
            .get(idx + 1)
            .expect("system prompt value should follow")
            .to_string_lossy();
        assert!(
            prompt.contains("wsx workspace rename 'myrepo' 'bold-fern'"),
            "rename block missing"
        );
        assert!(
            prompt.contains("Use tabs not spaces"),
            "custom instructions missing"
        );
        let rename_pos = prompt.find("wsx workspace rename").unwrap();
        let custom_pos = prompt.find("Use tabs not spaces").unwrap();
        assert!(
            custom_pos > rename_pos,
            "custom instructions must come after rename block"
        );
    }

    #[test]
    fn system_prompt_continue_passes_custom_only() {
        let mode = SpawnMode::Continue {
            custom_instructions: Some("Use ruff".into()),
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("--append-system-prompt should be present");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        assert!(prompt.contains("Use ruff"));
        assert!(
            !prompt.contains("wsx workspace rename"),
            "rename should not appear on Continue"
        );
    }

    #[test]
    fn rename_mode_pre_authorizes_wsx_workspace_rename_tool() {
        let ctx = RenameContext {
            current_branch: "wsx/bold-fern".into(),
            branch_prefix: "wsx".into(),
            repo_name: "myrepo".into(),
            current_slug: "bold-fern".into(),
        };
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--allowedTools"))
            .expect("--allowedTools should be present when rename_ctx is set and yolo=false");
        let value = argv
            .get(idx + 1)
            .expect("value should follow --allowedTools")
            .to_string_lossy();
        assert_eq!(
            value, "Bash(wsx workspace rename:*)",
            "expected wsx-workspace-rename pre-authorization, got: {value}"
        );
    }

    #[test]
    fn system_prompt_omitted_when_nothing_to_say() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(
            !argv
                .iter()
                .any(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
        );
        assert!(!argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
    }

    #[test]
    fn yolo_fresh_emits_skip_permissions() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: true,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--dangerously-skip-permissions")),
            "expected --dangerously-skip-permissions for yolo Fresh"
        );
    }

    #[test]
    fn yolo_continue_emits_skip_permissions() {
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: true,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--dangerously-skip-permissions")),
            "expected --dangerously-skip-permissions for yolo Continue"
        );
    }

    #[test]
    fn non_yolo_fresh_omits_skip_permissions() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(
            !argv
                .iter()
                .any(|a| a == std::ffi::OsStr::new("--dangerously-skip-permissions")),
            "non-yolo Fresh must not emit skip-permissions"
        );
    }

    #[test]
    fn rename_prompt_includes_current_branch_and_prefix() {
        let p = render_rename_system_prompt("wsx/bold-fern", "wsx", "myrepo", "bold-fern");
        assert!(p.contains("`wsx/bold-fern`"));
        assert!(p.contains("wsx workspace rename 'myrepo' 'bold-fern' <slug>"));
        // No "Keep the prefix" constraint — wsx handles that automatically.
        assert!(!p.contains("Keep the `wsx/` prefix"));
    }

    #[test]
    fn rename_prompt_handles_empty_prefix() {
        let p = render_rename_system_prompt("bold-fern", "", "myrepo", "bold-fern");
        assert!(p.contains("`bold-fern`"));
        assert!(p.contains("wsx workspace rename 'myrepo' 'bold-fern' <slug>"));
    }

    #[test]
    fn render_rename_prompt_hermes_includes_branch_and_prefix() {
        let prompt = super::render_rename_system_prompt_hermes(
            "wsx/bold-fern",
            "wsx",
            "myrepo",
            "bold-fern",
        );
        assert!(prompt.contains("wsx workspace rename 'myrepo' 'bold-fern'"));
        // No "Keep the prefix" constraint — wsx handles that automatically.
        assert!(!prompt.contains("Keep the `wsx/` prefix"));
    }

    #[test]
    fn render_rename_prompt_hermes_handles_empty_prefix() {
        let prompt =
            super::render_rename_system_prompt_hermes("bold-fern", "", "myrepo", "bold-fern");
        assert!(prompt.contains("wsx workspace rename 'myrepo' 'bold-fern'"));
        assert!(
            !prompt.contains("//"),
            "prompt should not contain double-slash: {prompt}"
        );
    }

    #[test]
    fn render_rename_prompt_hermes_matches_pi_today() {
        let hermes = super::render_rename_system_prompt_hermes("wsx/x", "wsx", "myrepo", "x");
        let pi = super::render_rename_system_prompt_pi("wsx/x", "wsx", "myrepo", "x");
        assert_eq!(hermes, pi, "drift between hermes and pi rename prompts");
    }

    #[test]
    fn project_manager_mode_adds_skip_permissions_and_system_prompt() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--dangerously-skip-permissions"), "{dbg}");
        assert!(!dbg.contains("--allowedTools"), "{dbg}");
        assert!(dbg.contains("--append-system-prompt"), "{dbg}");
        assert!(dbg.contains("project manager"), "{dbg}");
        assert!(!dbg.contains("--continue"), "should be Fresh-style: {dbg}");
    }

    #[test]
    fn project_manager_mode_resume_adds_continue() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: true,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--continue"), "{dbg}");
    }

    #[test]
    fn project_manager_mode_emits_settings_when_fast_mode() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: true,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--settings"))
            .expect("expected --settings flag when fast_mode is true");
        let value = argv
            .get(idx + 1)
            .expect("expected JSON value after --settings")
            .to_string_lossy();
        assert_eq!(value, r#"{"fastMode":true}"#);
    }

    #[test]
    fn project_manager_mode_omits_settings_when_fast_mode_false() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(
            !argv.iter().any(|a| a == std::ffi::OsStr::new("--settings")),
            "expected no --settings flag when fast_mode is false, argv: {argv:?}"
        );
    }

    #[test]
    fn fresh_mode_emits_status_hooks_via_settings() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--settings"))
            .expect("Fresh mode should emit --settings for status hooks");
        let value = argv
            .get(idx + 1)
            .expect("expected JSON value after --settings")
            .to_string_lossy();
        let v: serde_json::Value =
            serde_json::from_str(&value).expect("--settings value should be valid JSON");
        assert!(
            v["hooks"]["Stop"].is_array(),
            "expected hooks.Stop array, got: {v}"
        );
        assert!(
            v["hooks"]["UserPromptSubmit"].is_array(),
            "expected hooks.UserPromptSubmit array, got: {v}"
        );
        // fastMode must NOT be set for developer-agent spawns
        assert!(
            v.get("fastMode").is_none(),
            "Fresh mode must not set fastMode, got: {v}"
        );
    }

    #[test]
    fn continue_mode_emits_status_hooks_via_settings() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--settings"))
            .expect("Continue mode should emit --settings for status hooks");
        let value = argv
            .get(idx + 1)
            .expect("expected JSON value after --settings")
            .to_string_lossy();
        let v: serde_json::Value =
            serde_json::from_str(&value).expect("--settings value should be valid JSON");
        assert!(
            v["hooks"]["Stop"].is_array(),
            "expected hooks.Stop array, got: {v}"
        );
        assert!(
            v["hooks"]["UserPromptSubmit"].is_array(),
            "expected hooks.UserPromptSubmit array, got: {v}"
        );
        // fastMode must NOT be set for developer-agent spawns
        assert!(
            v.get("fastMode").is_none(),
            "Continue mode must not set fastMode, got: {v}"
        );
    }

    #[test]
    fn build_claude_command_appends_remote_control_when_enabled() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let opts = crate::agent::remote_control::RemoteOpts {
            enabled: true,
            sandbox: false,
        };
        let cmd = build_claude_command(&cwd, &mode, opts);
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control")),
            "expected --remote-control flag, argv: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == std::ffi::OsStr::new("--sandbox")),
            "expected no --sandbox flag, argv: {argv:?}"
        );
    }

    #[test]
    fn build_claude_command_appends_sandbox_when_enabled() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let opts = crate::agent::remote_control::RemoteOpts {
            enabled: true,
            sandbox: true,
        };
        let cmd = build_claude_command(&cwd, &mode, opts);
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control"))
        );
        assert!(argv.iter().any(|a| a == std::ffi::OsStr::new("--sandbox")));
    }

    #[test]
    fn build_claude_command_omits_remote_control_when_disabled() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(
            !argv
                .iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control")),
            "expected no --remote-control flag, argv: {argv:?}"
        );
        assert!(!argv.iter().any(|a| a == std::ffi::OsStr::new("--sandbox")));
    }

    #[test]
    fn build_claude_command_remote_control_applies_to_pm_mode() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let opts = crate::agent::remote_control::RemoteOpts {
            enabled: true,
            sandbox: false,
        };
        let cmd = build_claude_command(&cwd, &mode, opts);
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control")),
            "expected --remote-control in PM argv: {argv:?}"
        );
    }

    #[test]
    fn build_claude_command_emits_add_dir_per_related_path() {
        let cwd = PathBuf::from("/tmp/test");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![
                PathBuf::from("/work/frontend"),
                PathBuf::from("/work/marketing"),
            ],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let args: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        // Two pairs of (--add-dir, <path>) in order.
        let positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--add-dir")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            positions.len(),
            2,
            "expected two --add-dir flags; got: {args:?}"
        );
        assert_eq!(args[positions[0] + 1], "/work/frontend");
        assert_eq!(args[positions[1] + 1], "/work/marketing");
    }

    #[test]
    fn build_claude_command_omits_add_dir_when_no_related() {
        let cwd = PathBuf::from("/tmp/test");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let args: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(!args.iter().any(|a| a == "--add-dir"), "got: {args:?}");
    }

    // All branches in one test: env vars are process-global and the function
    // reads them at call time, so splitting these into separate #[test] fns
    // would only race within ENV_LOCK anyway. EnvGuard restores values on
    // drop, so panicking assertions can't leak state into other tests.
    #[test]
    fn build_pi_command_passes_model_selection() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };

        let argv_of = |env: &mut EnvGuard, mode: &SpawnMode| -> Vec<String> {
            let _ = env;
            let cmd = build_pi_command(
                &cwd,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            cmd.get_argv()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect()
        };

        // 1. Default (no env vars) → --models "deepseek/*"
        {
            let mut env = EnvGuard::new();
            env.remove("WSX_PI_MODEL");
            env.remove("WSX_PI_PROVIDER");
            let argv = argv_of(&mut env, &mode);
            let models_idx = argv
                .iter()
                .position(|a| a == "--models")
                .unwrap_or_else(|| panic!("expected --models in {argv:?}"));
            assert_eq!(argv[models_idx + 1], "deepseek/*");
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
        }

        // 2. WSX_PI_PROVIDER set → --models "<provider>/*"
        {
            let mut env = EnvGuard::new();
            env.remove("WSX_PI_MODEL");
            env.set("WSX_PI_PROVIDER", "anthropic");
            let argv = argv_of(&mut env, &mode);
            let models_idx = argv.iter().position(|a| a == "--models").unwrap();
            assert_eq!(argv[models_idx + 1], "anthropic/*");
        }

        // 3. WSX_PI_MODEL set → --model <value>, with --provider also forwarded
        {
            let mut env = EnvGuard::new();
            env.set("WSX_PI_PROVIDER", "anthropic");
            env.set("WSX_PI_MODEL", "deepseek/deepseek-v4-pro");
            let argv = argv_of(&mut env, &mode);
            let model_idx = argv.iter().position(|a| a == "--model").unwrap();
            assert_eq!(argv[model_idx + 1], "deepseek/deepseek-v4-pro");
            let provider_idx = argv.iter().position(|a| a == "--provider").unwrap();
            assert_eq!(argv[provider_idx + 1], "anthropic");
            assert!(!argv.iter().any(|a| a == "--models"), "argv: {argv:?}");
        }

        // 4. Empty/whitespace env values → treated as unset, fall back to default
        {
            let mut env = EnvGuard::new();
            env.set("WSX_PI_MODEL", "   ");
            env.set("WSX_PI_PROVIDER", "");
            let argv = argv_of(&mut env, &mode);
            let models_idx = argv.iter().position(|a| a == "--models").unwrap();
            assert_eq!(argv[models_idx + 1], "deepseek/*");
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
        }

        // 5. Continue mode → no model/provider flags at all (pi reuses session)
        {
            let mut env = EnvGuard::new();
            env.set("WSX_PI_PROVIDER", "anthropic");
            env.set("WSX_PI_MODEL", "claude-opus-4-7");
            let cont_mode = SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let argv = argv_of(&mut env, &cont_mode);
            assert!(argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--models"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
        }
    }

    mod hermes_compose {
        fn rename_ctx() -> super::RenameContext {
            super::RenameContext {
                current_branch: "wsx/bold-fern".into(),
                branch_prefix: "wsx".into(),
                repo_name: "myrepo".into(),
                current_slug: "bold-fern".into(),
            }
        }

        #[test]
        fn fresh_with_rename_returns_rename_text() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: Some(rename_ctx()),
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert!(result.contains("wsx workspace rename 'myrepo' 'bold-fern'"));
        }

        #[test]
        fn fresh_with_rename_and_custom_combines_both() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: Some(rename_ctx()),
                custom_instructions: Some("Use ruff.".into()),
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert!(result.contains("wsx workspace rename"));
            assert!(result.contains("Use ruff."));
            let rename_pos = result.find("wsx workspace rename").unwrap();
            let custom_pos = result.find("Use ruff.").unwrap();
            assert!(
                custom_pos > rename_pos,
                "custom should come after rename block"
            );
        }

        #[test]
        fn fresh_without_rename_returns_custom_only() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: Some("Use ruff.".into()),
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert_eq!(result, "Use ruff.");
        }

        #[test]
        fn fresh_with_nothing_returns_none() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            assert!(super::compose_injected_prompt(&mode).is_none());
        }

        #[test]
        fn continue_with_custom_returns_custom() {
            let mode = super::SpawnMode::Continue {
                custom_instructions: Some("Be terse.".into()),
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert_eq!(result, "Be terse.");
        }

        #[test]
        fn continue_without_custom_returns_none() {
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            assert!(super::compose_injected_prompt(&mode).is_none());
        }

        #[test]
        fn project_manager_returns_pm_prompt() {
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: false,
                fast_mode: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert!(!result.is_empty());
        }

        #[test]
        fn hermes_prepends_doctrine_before_custom() {
            let mode = super::SpawnMode::Continue {
                custom_instructions: Some("CUSTOM_MARK".to_string()),
                doctrine: Some("DOCTRINE_MARK".to_string()),
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            let dpos = result.find("DOCTRINE_MARK").expect("doctrine present");
            let cpos = result.find("CUSTOM_MARK").expect("custom present");
            assert!(dpos < cpos, "doctrine must precede custom: {result}");
            assert!(
                result.starts_with("DOCTRINE_MARK"),
                "doctrine must lead: {result}"
            );
        }

        #[test]
        fn hermes_pm_mode_has_no_doctrine() {
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/x/workspaces.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: false,
                fast_mode: false,
            };
            let result =
                super::compose_injected_prompt(&mode).expect("PM still injects its prompt");
            assert!(
                !result.contains("DOCTRINE_MARK"),
                "PM must not get doctrine: {result}"
            );
        }
    }

    mod hermes_build_command {
        use std::ffi::OsStr;

        fn argv_strings(cmd: &portable_pty::CommandBuilder) -> Vec<String> {
            // Skip argv[0] (the binary name); callers assert on subcommand/flags.
            cmd.get_argv()
                .iter()
                .skip(1)
                .map(|s| s.to_string_lossy().into_owned())
                .collect()
        }

        fn fresh_no_rename() -> super::SpawnMode {
            super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            }
        }

        #[test]
        fn fresh_emits_chat_subcommand_only_no_source_flag() {
            // --source is never emitted: Hermes ignores it for session creation.
            let tmp = tempfile::tempdir().unwrap();
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert_eq!(
                argv.first().map(|s| s.as_str()),
                Some("chat"),
                "argv: {argv:?}"
            );
            assert!(
                !argv.iter().any(|a| a == "--source"),
                "--source must not be emitted; argv: {argv:?}"
            );
        }

        #[test]
        fn fresh_omits_continue_resume_and_yolo() {
            let tmp = tempfile::tempdir().unwrap();
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--resume"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--yolo"), "argv: {argv:?}");
        }

        #[test]
        fn yolo_fresh_emits_yolo_flag() {
            let tmp = tempfile::tempdir().unwrap();
            let mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: true,
            };
            let cmd = super::build_hermes_command(
                tmp.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
        }

        #[test]
        fn yolo_continue_emits_yolo_flag() {
            let tmp = tempfile::tempdir().unwrap();
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: true,
            };
            let cmd = super::build_hermes_command(
                tmp.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
        }

        #[test]
        fn project_manager_mode_is_always_yolo() {
            let tmp = tempfile::tempdir().unwrap();
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: false,
                fast_mode: false,
            };
            let cmd = super::build_hermes_command(
                tmp.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
        }

        #[test]
        fn project_manager_mode_emits_yolo_and_resume_if_set() {
            let home = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            // Seed .git/info structure and spawn marker for cwd.
            std::fs::create_dir_all(cwd.path().join(".git/info")).unwrap();
            std::fs::write(cwd.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();
            // Seed ~/.hermes/state.db with a session after spawn_ts.
            let hermes_dir = home.path().join(".hermes");
            std::fs::create_dir_all(&hermes_dir).unwrap();
            let db_path = hermes_dir.join("state.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, started_at REAL NOT NULL);",
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('pm-sess', 'cli', 1234.5);",
                [],
            )
            .unwrap();
            drop(conn);

            let mut env = super::EnvGuard::new();
            env.set("HOME", home.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: true,
                fast_mode: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let resume_idx = argv
                .iter()
                .position(|a| a == "--resume")
                .expect("expected --resume");
            assert_eq!(argv[resume_idx + 1], "pm-sess");
            assert!(argv.iter().any(|a| a == "--yolo"), "argv: {argv:?}");
        }

        #[test]
        fn no_worktree_flag_ever_emitted() {
            let tmp = tempfile::tempdir().unwrap();
            for mode in &[
                fresh_no_rename(),
                super::SpawnMode::Continue {
                    custom_instructions: None,
                    doctrine: None,
                    additional_dirs: vec![],
                    yolo: true,
                },
                super::SpawnMode::ProjectManager {
                    workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                    custom_instructions: None,
                    additional_dirs: vec![],
                    resume: true,
                    fast_mode: false,
                },
            ] {
                let cmd = super::build_hermes_command(
                    tmp.path(),
                    mode,
                    crate::agent::remote_control::RemoteOpts::disabled(),
                );
                let argv = argv_strings(&cmd);
                assert!(
                    !argv.iter().any(|a| a == "--worktree" || a == "-w"),
                    "should never emit --worktree; argv: {argv:?}"
                );
            }
        }

        #[test]
        fn source_never_emitted_regardless_of_path() {
            // --source is never emitted, even for paths that would previously have
            // triggered source tag emission. Session detection uses the marker file.
            let bogus = std::path::Path::new("/nonexistent/path/for/canonicalize");
            let cmd = super::build_hermes_command(
                bogus,
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert!(
                !argv.iter().any(|a| a == "--source"),
                "expected --source absent; argv: {argv:?}"
            );
            assert_eq!(argv.first().map(|s| s.as_str()), Some("chat"));
        }

        #[test]
        fn continue_without_prior_session_omits_resume() {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.set("HOME", tmp.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--resume"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
        }

        #[test]
        fn continue_with_prior_session_passes_resume_id() {
            let home = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            // Seed .git/info structure and a marker file for cwd.
            std::fs::create_dir_all(cwd.path().join(".git/info")).unwrap();
            // Write marker with timestamp 1000.0
            std::fs::write(cwd.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();

            let hermes_dir = home.path().join(".hermes");
            std::fs::create_dir_all(&hermes_dir).unwrap();
            let db_path = hermes_dir.join("state.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, started_at REAL NOT NULL);",
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('session-abc', 'cli', 1234.5);",
                [],
            ).unwrap();
            drop(conn);

            let mut env = super::EnvGuard::new();
            env.set("HOME", home.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let idx = argv
                .iter()
                .position(|a| a == "--resume")
                .expect("expected --resume");
            assert_eq!(argv[idx + 1], "session-abc");
        }

        #[test]
        fn continue_with_cached_session_id_uses_cached_value() {
            // Marker file has session_id="session-cached". DB has two sessions:
            // "session-cached" (older, started_at=1100.0) and "session-newer"
            // (newer, started_at=1500.0). The cached id must win over the newer
            // time-based result.
            let home = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(cwd.path().join(".git/info")).unwrap();
            // Write marker with start_ts=1000.0 AND cached session_id.
            std::fs::write(
                cwd.path().join(".git/info/wsx-hermes-spawn-at"),
                "1000.0\nsession-cached\n",
            )
            .unwrap();

            let hermes_dir = home.path().join(".hermes");
            std::fs::create_dir_all(&hermes_dir).unwrap();
            let db_path = hermes_dir.join("state.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, started_at REAL NOT NULL);",
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('session-cached', 'cli', 1100.0);",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('session-newer', 'cli', 1500.0);",
                [],
            ).unwrap();
            drop(conn);

            let mut env = super::EnvGuard::new();
            env.set("HOME", home.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let idx = argv
                .iter()
                .position(|a| a == "--resume")
                .expect("expected --resume");
            assert_eq!(
                argv[idx + 1],
                "session-cached",
                "cached id must win over time-based newer session; argv: {argv:?}"
            );
        }

        fn env_of(cmd: &portable_pty::CommandBuilder, key: &str) -> Option<String> {
            cmd.get_env(OsStr::new(key))
                .map(|v| v.to_string_lossy().into_owned())
        }

        #[test]
        fn wsx_hermes_model_env_sets_inference_model_env_on_child() {
            let tmp = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.remove("HERMES_INFERENCE_MODEL");
            env.set("WSX_HERMES_MODEL", "deepseek/deepseek-v4-pro");
            env.remove("WSX_HERMES_PROVIDER");
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert_eq!(
                env_of(&cmd, "HERMES_INFERENCE_MODEL"),
                Some("deepseek/deepseek-v4-pro".to_string())
            );
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
        }

        #[test]
        fn wsx_hermes_provider_env_passes_provider_flag() {
            let tmp = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.remove("WSX_HERMES_MODEL");
            env.set("WSX_HERMES_PROVIDER", "openrouter");
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let idx = argv
                .iter()
                .position(|a| a == "--provider")
                .expect("expected --provider");
            assert_eq!(argv[idx + 1], "openrouter");
        }

        #[test]
        fn empty_model_env_treated_as_unset() {
            let tmp = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.remove("HERMES_INFERENCE_MODEL");
            env.set("WSX_HERMES_MODEL", "   ");
            env.set("WSX_HERMES_PROVIDER", "");
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(env_of(&cmd, "HERMES_INFERENCE_MODEL").is_none());
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
        }
    }

    // ── Batch B: shell_quote helper and rename prompt quoting ────────────────

    #[test]
    fn shell_quote_handles_internal_single_quote() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn render_rename_prompt_claude_shell_quotes_repo_name_with_space() {
        let prompt = render_rename_system_prompt("wsx/bold-fern", "wsx", "my repo", "bold-fern");
        assert!(
            prompt.contains("wsx workspace rename 'my repo'"),
            "expected single-quoted repo name with space; prompt: {prompt}"
        );
    }

    #[test]
    fn render_rename_prompt_pi_shell_quotes_repo_name_with_metacharacter() {
        let prompt = render_rename_system_prompt_pi("wsx/bold-fern", "wsx", "foo;bar", "bold-fern");
        assert!(
            prompt.contains("'foo;bar'"),
            "expected single-quoted repo name with metachar; prompt: {prompt}"
        );
    }

    // ── Batch C: rename prompt uses stored ws.name, not derived slug ─────────

    #[test]
    fn rename_prompt_uses_ws_name_not_derived_slug() {
        let ctx = RenameContext {
            current_branch: "OLD-PREFIX/bold-fern".into(),
            branch_prefix: "wsx".into(),
            repo_name: "myrepo".into(),
            current_slug: "actual-stored-name".into(),
        };
        let prompt = render_rename_system_prompt(
            &ctx.current_branch,
            &ctx.branch_prefix,
            &ctx.repo_name,
            &ctx.current_slug,
        );
        assert!(
            prompt.contains("wsx workspace rename 'myrepo' 'actual-stored-name' <slug>"),
            "expected stored slug in rename command; prompt: {prompt}"
        );
        assert!(
            !prompt.contains("'bold-fern'"),
            "prompt must not contain derived 'bold-fern'; prompt: {prompt}"
        );
    }

    #[test]
    fn claude_prepends_doctrine_before_custom_instructions() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some("CUSTOM_MARK".to_string()),
            doctrine: Some("DOCTRINE_MARK".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("expected --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        let dpos = prompt.find("DOCTRINE_MARK").expect("doctrine present");
        let cpos = prompt.find("CUSTOM_MARK").expect("custom present");
        assert!(
            dpos < cpos,
            "doctrine must precede custom instructions: {prompt}"
        );
        assert!(
            prompt.starts_with("DOCTRINE_MARK"),
            "doctrine must lead: {prompt}"
        );
    }

    #[test]
    fn pi_prepends_doctrine_before_custom_instructions() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Continue {
            custom_instructions: Some("CUSTOM_MARK".to_string()),
            doctrine: Some("DOCTRINE_MARK".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_pi_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("expected --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        let dpos = prompt.find("DOCTRINE_MARK").expect("doctrine present");
        let cpos = prompt.find("CUSTOM_MARK").expect("custom present");
        assert!(
            dpos < cpos,
            "doctrine must precede custom instructions: {prompt}"
        );
        assert!(
            prompt.starts_with("DOCTRINE_MARK"),
            "doctrine must lead: {prompt}"
        );
    }

    #[test]
    fn pi_pm_mode_has_no_doctrine_marker() {
        // PM variant has no doctrine field; ensure nothing leaks one in.
        // Give PM custom instructions so it definitely emits an
        // --append-system-prompt, making the no-doctrine assertion non-vacuous.
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: Some("PM_CUSTOM_MARK".to_string()),
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_pi_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("PM with custom instructions must emit --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        assert!(
            prompt.contains("PM_CUSTOM_MARK"),
            "PM prompt should be present: {prompt}"
        );
        assert!(
            !prompt.contains("DOCTRINE_MARK"),
            "PM must not get doctrine: {prompt}"
        );
    }

    #[test]
    fn claude_pm_mode_has_no_doctrine_marker() {
        // PM variant has no doctrine field; ensure nothing leaks one in.
        let cwd = PathBuf::from(".");
        // Give PM custom instructions so it definitely emits an
        // --append-system-prompt, making the no-doctrine assertion non-vacuous.
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: Some("PM_CUSTOM_MARK".to_string()),
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("PM with custom instructions must emit --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        assert!(
            prompt.contains("PM_CUSTOM_MARK"),
            "PM prompt should be present: {prompt}"
        );
        assert!(
            !prompt.contains("DOCTRINE_MARK"),
            "PM must not get doctrine: {prompt}"
        );
    }

    /// Build a Codex command for `mode` and return its argv as lossy Strings.
    fn codex_argv(mode: &SpawnMode) -> Vec<String> {
        let cmd = build_codex_command(
            Path::new("/tmp/wt"),
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        cmd.get_argv()
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn codex_fresh_is_bare_codex_with_no_approval_flags() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.remove("WSX_CODEX_MODEL");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(
            !argv.iter().any(|a| a == "resume"),
            "fresh must not resume: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a.starts_with("--dangerously-bypass")),
            "non-yolo must not bypass: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "--ask-for-approval"),
            "dev session uses codex defaults: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "-m"),
            "no model env set: {argv:?}"
        );
    }

    #[test]
    fn codex_fresh_yolo_bypasses_approvals() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: true,
        });
        assert!(
            argv.iter()
                .any(|a| a == "--dangerously-bypass-approvals-and-sandbox"),
            "yolo must bypass: {argv:?}"
        );
    }

    #[test]
    fn codex_continue_uses_resume_last() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::Continue {
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(
            argv.iter().any(|a| a == "resume"),
            "continue must resume: {argv:?}"
        );
        assert!(
            argv.iter().any(|a| a == "--last"),
            "continue must use --last: {argv:?}"
        );
    }

    #[test]
    fn codex_pm_is_read_only_and_never_asks() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::ProjectManager {
            workspaces_json_path: std::path::PathBuf::from("/tmp/pm/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        });
        assert!(
            argv.windows(2)
                .any(|w| w[0] == "--ask-for-approval" && w[1] == "never"),
            "pm must never ask: {argv:?}"
        );
        assert!(
            argv.windows(2)
                .any(|w| w[0] == "--sandbox" && w[1] == "read-only"),
            "pm must be read-only: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "resume"),
            "pm fresh must not resume: {argv:?}"
        );
    }

    #[test]
    fn codex_model_env_adds_dash_m() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.set("WSX_CODEX_MODEL", "gpt-5.4");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(
            argv.windows(2).any(|w| w[0] == "-m" && w[1] == "gpt-5.4"),
            "model must be passed via -m: {argv:?}"
        );
    }

    #[test]
    fn codex_fresh_injects_notify_status_wiring() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.remove("WSX_CODEX_MODEL");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(argv.iter().any(|a| a == "-c"), "argv: {argv:?}");
        assert!(
            argv.iter()
                .any(|a| a.starts_with("notify=[") && a.contains("from-notify")),
            "argv: {argv:?}"
        );
    }

    #[test]
    fn codex_pm_omits_notify_status_wiring() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::ProjectManager {
            workspaces_json_path: std::path::PathBuf::from("/tmp/pm/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        });
        assert!(
            !argv.iter().any(|a| a.starts_with("notify=[")),
            "PM should not get status wiring; argv: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "-c"),
            "PM should not inject the -c flag; argv: {argv:?}"
        );
    }
}
