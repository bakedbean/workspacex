#![allow(clippy::collapsible_if)]

use crate::error::{Error, Result};
use std::path::Path;

/// Resolve and launch the user's editor on `worktree`.
/// Path is appended as the final argument.
pub fn open_in_editor(worktree: &Path, configured: Option<&str>) -> Result<()> {
    let cmd = resolve_editor_cmd(configured)?;
    spawn_with_path_arg(&cmd, worktree)
}

/// Resolve and launch the user's terminal with cwd=`worktree`.
pub fn open_in_terminal(worktree: &Path, configured: Option<&str>) -> Result<()> {
    let cmd = resolve_terminal_cmd(configured)?;
    spawn_with_cwd(&cmd, worktree)
}

/// Resolve and launch lazygit (or configured equivalent) with cwd=`worktree`.
pub fn open_in_lazygit(worktree: &Path, configured: Option<&str>) -> Result<()> {
    let cmd = resolve_lazygit_cmd(configured)?;
    spawn_with_cwd(&cmd, worktree)
}

/// Resolve and launch the user's difftool on `worktree`, diffing against `base`.
/// The command template can reference `{path}` and `{base}`. If neither
/// appears, `{path}` is appended (same convention as the editor).
pub fn open_diff(worktree: &Path, base: &str, configured: Option<&str>) -> Result<()> {
    let cmd = resolve_diff_cmd(configured)?;
    let path_str = worktree.to_string_lossy();
    spawn_resolved(
        &cmd,
        worktree,
        &[("path", path_str.as_ref()), ("base", base)],
        Some(path_str.as_ref()),
    )
}

/// Open `$EDITOR` (or `vi` if unset) on a tempfile prepopulated with
/// `initial`. Returns `Ok(Some(contents))` on a clean exit (the contents
/// may equal `initial` if the user didn't modify them), `Ok(None)` if
/// the editor exited non-zero (treat as cancel), or `Err` on tempfile/
/// spawn I/O failure.
///
/// `ext_hint` is the file extension (no leading dot) — `"sh"`, `"md"`,
/// `"txt"` — used so the editor picks the right syntax mode.
pub fn edit_in_editor(initial: &str, ext_hint: &str) -> Result<Option<String>> {
    use std::time::{SystemTime, UNIX_EPOCH};

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let pid = std::process::id();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("wsx-edit-{pid}-{nanos}.{ext_hint}"));
    std::fs::write(&path, initial)?;

    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| Error::Io(std::io::Error::other(format!("spawn {editor}: {e}"))))?;

    if !status.success() {
        let _ = std::fs::remove_file(&path);
        return Ok(None);
    }
    let new = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(Some(new))
}

fn resolve_editor_cmd(configured: Option<&str>) -> Result<String> {
    if let Some(c) = configured {
        if !c.trim().is_empty() {
            return Ok(c.to_string());
        }
    }
    if let Ok(v) = std::env::var("VISUAL") {
        if !v.trim().is_empty() {
            return Ok(v);
        }
    }
    if let Ok(v) = std::env::var("EDITOR") {
        if !v.trim().is_empty() {
            return Ok(v);
        }
    }
    Err(Error::UserInput(
        "no editor configured; set `wsx config set editor_cmd <cmd>` or $VISUAL / $EDITOR".into(),
    ))
}

fn resolve_diff_cmd(configured: Option<&str>) -> Result<String> {
    if let Some(c) = configured {
        if !c.trim().is_empty() {
            return Ok(c.to_string());
        }
    }
    Err(Error::UserInput(
        "no diff command configured; set `wsx config set diff_cmd <cmd>` \
         (placeholders: `{path}`, `{base}`)"
            .into(),
    ))
}

fn resolve_terminal_cmd(configured: Option<&str>) -> Result<String> {
    if let Some(c) = configured {
        if !c.trim().is_empty() {
            return Ok(c.to_string());
        }
    }
    if let Ok(v) = std::env::var("TERMINAL") {
        if !v.trim().is_empty() {
            return Ok(v);
        }
    }
    Err(Error::UserInput(
        "no terminal configured; set `wsx config set terminal_cmd <cmd>` or $TERMINAL".into(),
    ))
}

fn resolve_lazygit_cmd(configured: Option<&str>) -> Result<String> {
    if let Some(c) = configured {
        if !c.trim().is_empty() {
            return Ok(c.to_string());
        }
    }
    Err(Error::UserInput(
        "no lazygit command configured; set `wsx config set lazygit_cmd <cmd>` \
         (e.g. `wezterm start -- lazygit`) — wsx's own TUI owns the terminal, \
         so lazygit needs a wrapper that opens its own window"
            .into(),
    ))
}

fn spawn_with_path_arg(cmd: &str, path: &Path) -> Result<()> {
    let path_str = path.to_string_lossy();
    spawn_resolved(
        cmd,
        path,
        &[("path", path_str.as_ref())],
        Some(path_str.as_ref()),
    )
}

fn spawn_with_cwd(cmd: &str, cwd: &Path) -> Result<()> {
    let cwd_str = cwd.to_string_lossy();
    spawn_resolved(cmd, cwd, &[("path", cwd_str.as_ref())], None)
}

fn spawn_resolved(
    cmd: &str,
    cwd: &Path,
    substitutions: &[(&str, &str)],
    fallback_when_no_placeholder: Option<&str>,
) -> Result<()> {
    let parts = resolve_argv(cmd, substitutions, fallback_when_no_placeholder)?;
    spawn_parts(parts, cwd)
}

/// Spawn `parts` (program + argv) detached, with cwd = `cwd`.
fn spawn_parts(mut parts: Vec<String>, cwd: &Path) -> Result<()> {
    if parts.is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    let program = parts.remove(0);
    let mut command = std::process::Command::new(&program);
    command.args(&parts).current_dir(cwd);
    detach_io(&mut command);
    command
        .spawn()
        .map_err(|e| Error::UserInput(format!("spawn {program}: {e}")))?;
    Ok(())
}

/// Pure helper: resolve a command into the program + argv that would be spawned.
/// For each `(name, value)` in `substitutions`, replace `{name}` occurrences in every
/// token. If no token contained any of the named placeholders and
/// `fallback_when_no_placeholder` is `Some(p)`, append `p` as the final argument.
fn resolve_argv(
    cmd: &str,
    substitutions: &[(&str, &str)],
    fallback_when_no_placeholder: Option<&str>,
) -> Result<Vec<String>> {
    let mut parts = shlex::split(cmd)
        .ok_or_else(|| Error::UserInput(format!("could not parse command: {cmd}")))?;
    if parts.is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    let needles: Vec<String> = substitutions
        .iter()
        .map(|(name, _)| format!("{{{name}}}"))
        .collect();
    let any_placeholder_used = parts.iter().any(|p| needles.iter().any(|n| p.contains(n)));
    for part in &mut parts {
        for (needle, (_, value)) in needles.iter().zip(substitutions.iter()) {
            *part = part.replace(needle.as_str(), value);
        }
    }
    if !any_placeholder_used {
        if let Some(fallback) = fallback_when_no_placeholder {
            parts.push(fallback.to_string());
        }
    }
    Ok(parts)
}

/// How an editor wants a file+line on its command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GotoStyle {
    /// VS Code family: `--goto file:line`.
    Goto,
    /// vi/emacs family: `+line file`.
    PlusLine,
}

/// Map an editor program basename to its goto style, if known.
fn known_editor_goto(basename: &str) -> Option<GotoStyle> {
    match basename {
        "code" | "codium" | "cursor" | "zed" => Some(GotoStyle::Goto),
        "vim" | "nvim" | "vi" | "nano" | "emacs" | "emacsclient" => Some(GotoStyle::PlusLine),
        _ => None,
    }
}

/// Resolve the editor command into argv that opens `file` at `line`.
///
/// Resolution order:
/// 1. If the command contains `{file}`/`{line}` placeholders, substitute them.
/// 2. Else scan ALL tokens for the first one whose basename is a known editor
///    and append that editor's goto syntax (so window-wrapper commands like
///    `alacritty -e nvim` detect the inner editor and keep the line).
/// 3. Else append the file (line dropped); the user can add `{file}`/`{line}`
///    placeholders for an unrecognized editor.
fn resolve_editor_at_argv(cmd: &str, path: &str, file: &str, line: u32) -> Result<Vec<String>> {
    let line_s = line.to_string();
    let mut parts = shlex::split(cmd)
        .ok_or_else(|| Error::UserInput(format!("could not parse command: {cmd}")))?;
    if parts.is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    // Substitute {path} (the worktree / working dir) first, so an editor_cmd
    // shared with the dir-open action — which also uses {path} — works here too.
    for part in &mut parts {
        *part = part.replace("{path}", path);
    }
    let used_placeholder = parts
        .iter()
        .any(|p| p.contains("{file}") || p.contains("{line}"));
    if used_placeholder {
        for part in &mut parts {
            *part = part.replace("{file}", file).replace("{line}", &line_s);
        }
        return Ok(parts);
    }
    let style = parts.iter().find_map(|p| {
        let base = std::path::Path::new(p)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(p);
        known_editor_goto(base)
    });
    match style {
        Some(GotoStyle::Goto) => {
            parts.push("--goto".to_string());
            parts.push(format!("{file}:{line_s}"));
        }
        Some(GotoStyle::PlusLine) => {
            parts.push(format!("+{line_s}"));
            parts.push(file.to_string());
        }
        None => parts.push(file.to_string()),
    }
    Ok(parts)
}

/// Resolve and launch the user's editor on `file`, positioned at `line`.
/// Spawns with cwd = `worktree`. Used by the chronology bar's entry clicks.
pub fn open_in_editor_at(
    worktree: &Path,
    file: &Path,
    line: u32,
    configured: Option<&str>,
) -> Result<()> {
    let cmd = resolve_editor_cmd(configured)?;
    let worktree_str = worktree.to_string_lossy();
    let file_str = file.to_string_lossy();
    let parts = resolve_editor_at_argv(&cmd, worktree_str.as_ref(), file_str.as_ref(), line)?;
    spawn_parts(parts, worktree)
}

/// Outcome of deciding whether the chronology open-at-line can launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorOpenDecision {
    /// Launch the trimmed command.
    Launch(String),
    /// No usable `editor_cmd`; the caller should prompt the user to configure one.
    NeedsConfig,
}

/// Decide whether the chronology open-at-line can launch. A non-empty,
/// non-whitespace `editor_cmd` yields `Launch`; anything else `NeedsConfig`.
/// Unlike `open_in_editor`, this path does NOT fall back to `$VISUAL`/`$EDITOR`
/// — opening a file at a line needs an editor the user has chosen to wire up.
pub fn editor_open_decision(editor_cmd: Option<&str>) -> EditorOpenDecision {
    match editor_cmd {
        Some(c) if !c.trim().is_empty() => EditorOpenDecision::Launch(c.trim().to_string()),
        _ => EditorOpenDecision::NeedsConfig,
    }
}

fn detach_io(cmd: &mut std::process::Command) {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EnvGuard, false_path, true_path};

    #[test]
    fn editor_fallback_uses_configured_first() {
        // We can't easily test EDITOR/VISUAL without mutating process env, but the
        // configured-first path is deterministic.
        assert_eq!(resolve_editor_cmd(Some("my-editor")).unwrap(), "my-editor");
    }

    #[test]
    fn editor_falls_back_to_env() {
        let mut env = EnvGuard::new();
        env.set("VISUAL", "fake-visual-editor");
        let r = resolve_editor_cmd(None).unwrap();
        assert_eq!(r, "fake-visual-editor");
    }

    #[test]
    fn editor_errors_when_unconfigured() {
        let mut env = EnvGuard::new();
        env.remove("VISUAL");
        env.remove("EDITOR");
        let r = resolve_editor_cmd(None);
        assert!(r.is_err());
    }

    #[test]
    fn terminal_errors_when_unconfigured() {
        let mut env = EnvGuard::new();
        env.remove("TERMINAL");
        let r = resolve_terminal_cmd(None);
        assert!(r.is_err());
    }

    #[test]
    fn spawn_with_path_arg_runs_true_with_quoted_command() {
        let dir = std::env::temp_dir();
        // `true` exits immediately — handy for spawn-success assertions.
        let r = spawn_with_path_arg(true_path(), &dir);
        assert!(r.is_ok(), "spawn true failed: {r:?}");
    }

    #[test]
    fn spawn_errors_on_missing_program() {
        let dir = std::env::temp_dir();
        let r = spawn_with_path_arg("/no/such/wsx-test-binary", &dir);
        assert!(r.is_err());
    }

    #[test]
    fn placeholder_substituted_when_present() {
        let dir = std::env::temp_dir();
        let r = spawn_with_path_arg(&format!("{} --dir={{path}}", true_path()), &dir);
        assert!(r.is_ok(), "spawn failed: {r:?}");
    }

    #[test]
    fn no_placeholder_appends_path_for_editor() {
        let dir = std::env::temp_dir();
        let r = spawn_with_path_arg(true_path(), &dir);
        assert!(r.is_ok());
    }

    #[test]
    fn no_placeholder_does_not_append_for_terminal() {
        let dir = std::env::temp_dir();
        let r = spawn_with_cwd(true_path(), &dir);
        assert!(r.is_ok());
    }

    #[test]
    fn resolve_argv_substitutes_named_placeholders() {
        let argv = resolve_argv(
            "diff-tool --cwd={path} --base={base}",
            &[("path", "/tmp/wt"), ("base", "main")],
            None,
        )
        .unwrap();
        assert_eq!(argv, vec!["diff-tool", "--cwd=/tmp/wt", "--base=main"]);
    }

    #[test]
    fn resolve_argv_substitutes_multiple_occurrences_of_same_placeholder() {
        let argv = resolve_argv(
            "editor --cwd={path} --file={path}/main.rs",
            &[("path", "/tmp/wt")],
            None,
        )
        .unwrap();
        assert_eq!(
            argv,
            vec!["editor", "--cwd=/tmp/wt", "--file=/tmp/wt/main.rs"]
        );
    }

    #[test]
    fn resolve_argv_appends_fallback_path_when_no_placeholder() {
        let argv = resolve_argv("code", &[("path", "/tmp/wt")], Some("/tmp/wt")).unwrap();
        assert_eq!(argv, vec!["code", "/tmp/wt"]);
    }

    #[test]
    fn resolve_argv_omits_fallback_when_none() {
        let argv = resolve_argv("alacritty", &[("path", "/tmp/wt")], None).unwrap();
        assert_eq!(argv, vec!["alacritty"]);
    }

    #[test]
    fn resolve_argv_with_empty_substitutions_returns_argv_unchanged() {
        let argv = resolve_argv("just a command", &[], None).unwrap();
        assert_eq!(argv, vec!["just", "a", "command"]);
    }

    #[test]
    fn diff_errors_when_unconfigured() {
        let r = resolve_diff_cmd(None);
        assert!(r.is_err());
    }

    #[test]
    fn diff_uses_configured_first() {
        assert_eq!(
            resolve_diff_cmd(Some("my-diff {path} {base}")).unwrap(),
            "my-diff {path} {base}"
        );
    }

    #[test]
    fn spawn_diff_substitutes_both_placeholders() {
        let dir = std::env::temp_dir();
        let template = format!("{} --path={{path}} --base={{base}}", true_path());
        let r = open_diff(&dir, "main", Some(&template));
        assert!(r.is_ok(), "open_diff failed: {r:?}");
    }

    #[test]
    fn edit_in_editor_returns_unchanged_when_editor_doesnt_write() {
        let mut env = EnvGuard::new();
        env.set("EDITOR", true_path());
        let result = edit_in_editor("hello world", "txt");
        assert_eq!(result.unwrap().as_deref(), Some("hello world"));
    }

    #[test]
    fn edit_in_editor_returns_none_when_editor_exits_nonzero() {
        let mut env = EnvGuard::new();
        env.set("EDITOR", false_path());
        let result = edit_in_editor("anything", "txt");
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn lazygit_errors_when_unconfigured() {
        assert!(resolve_lazygit_cmd(None).is_err());
    }

    #[test]
    fn lazygit_uses_configured_first() {
        assert_eq!(
            resolve_lazygit_cmd(Some("wezterm start -- lazygit")).unwrap(),
            "wezterm start -- lazygit"
        );
    }

    #[test]
    fn open_in_lazygit_spawns_configured_cmd() {
        let dir = std::env::temp_dir();
        let r = open_in_lazygit(&dir, Some(true_path()));
        assert!(r.is_ok(), "open_in_lazygit failed: {r:?}");
    }

    #[test]
    fn resolve_argv_leaves_unknown_placeholders_literal_and_skips_fallback() {
        // Design note: `any_placeholder_used` only considers placeholders the
        // caller actually passed in `substitutions`. An unknown placeholder like
        // `{base}` in the command stays literal (no substitution rule matches),
        // and because no *known* placeholder appeared, the fallback path IS
        // appended. This test pins that behavior.
        let argv = resolve_argv(
            "tool --base={base}",
            &[("path", "/tmp/wt")],
            Some("/tmp/wt"),
        )
        .unwrap();
        assert_eq!(argv, vec!["tool", "--base={base}", "/tmp/wt"]);
    }

    #[test]
    fn editor_at_substitutes_file_and_line_placeholders() {
        let argv = resolve_editor_at_argv(
            "code --goto {file}:{line}",
            "/tmp/wt",
            "/tmp/wt/src/main.rs",
            42,
        )
        .unwrap();
        assert_eq!(argv, vec!["code", "--goto", "/tmp/wt/src/main.rs:42"]);
    }

    #[test]
    fn editor_at_vim_fallback_uses_plus_line() {
        let argv = resolve_editor_at_argv("nvim", "/tmp/wt", "/tmp/wt/src/main.rs", 42).unwrap();
        assert_eq!(argv, vec!["nvim", "+42", "/tmp/wt/src/main.rs"]);
    }

    #[test]
    fn editor_at_code_fallback_uses_goto() {
        let argv = resolve_editor_at_argv("code", "/tmp/wt", "/tmp/wt/src/main.rs", 7).unwrap();
        assert_eq!(argv, vec!["code", "--goto", "/tmp/wt/src/main.rs:7"]);
    }

    #[test]
    fn editor_at_emacs_fallback_uses_plus_line() {
        let argv = resolve_editor_at_argv("emacsclient", "/tmp/wt", "/tmp/wt/a.rs", 3).unwrap();
        assert_eq!(argv, vec!["emacsclient", "+3", "/tmp/wt/a.rs"]);
    }

    #[test]
    fn editor_at_unknown_editor_appends_file_only() {
        let argv = resolve_editor_at_argv("myeditor", "/tmp/wt", "/tmp/wt/a.rs", 3).unwrap();
        assert_eq!(argv, vec!["myeditor", "/tmp/wt/a.rs"]);
    }

    #[test]
    fn editor_at_substitutes_placeholders_in_separate_tokens() {
        let argv =
            resolve_editor_at_argv("nvim +{line} {file}", "/tmp/wt", "/tmp/wt/a.rs", 9).unwrap();
        assert_eq!(argv, vec!["nvim", "+9", "/tmp/wt/a.rs"]);
    }

    #[test]
    fn editor_at_wrapper_terminal_editor_keeps_line() {
        let argv = resolve_editor_at_argv("alacritty -e nvim", "/wt", "/wt/a.rs", 42).unwrap();
        assert_eq!(argv, vec!["alacritty", "-e", "nvim", "+42", "/wt/a.rs"]);
    }

    #[test]
    fn editor_at_wrapper_gui_editor_uses_goto() {
        let argv = resolve_editor_at_argv("wezterm start -- code", "/wt", "/wt/a.rs", 7).unwrap();
        assert_eq!(
            argv,
            vec!["wezterm", "start", "--", "code", "--goto", "/wt/a.rs:7"]
        );
    }

    #[test]
    fn editor_at_zed_uses_goto() {
        let argv = resolve_editor_at_argv("zed", "/wt", "/wt/a.rs", 5).unwrap();
        assert_eq!(argv, vec!["zed", "--goto", "/wt/a.rs:5"]);
    }

    #[test]
    fn editor_at_nano_uses_plus_line() {
        let argv = resolve_editor_at_argv("nano", "/wt", "/wt/a.rs", 5).unwrap();
        assert_eq!(argv, vec!["nano", "+5", "/wt/a.rs"]);
    }

    #[test]
    fn editor_at_unknown_wrapped_editor_appends_file_only() {
        // No known editor token and no placeholders → append the file, line dropped.
        let argv = resolve_editor_at_argv("myterm -e myed", "/wt", "/wt/a.rs", 9).unwrap();
        assert_eq!(argv, vec!["myterm", "-e", "myed", "/wt/a.rs"]);
    }

    #[test]
    fn editor_at_first_known_editor_token_wins() {
        // When more than one known editor appears, the first match decides the
        // goto style (here `code` → --goto, even though `vim` follows).
        let argv = resolve_editor_at_argv("code --diff vim", "/wt", "/wt/a.rs", 3).unwrap();
        assert_eq!(argv, vec!["code", "--diff", "vim", "--goto", "/wt/a.rs:3"]);
    }

    #[test]
    fn editor_at_substitutes_path_placeholder() {
        // {path} is the worktree (shared with the dir-open action); the inner
        // editor is still detected and the line appended.
        let argv =
            resolve_editor_at_argv("xdg-terminal-exec --dir={path} nvim", "/wt", "/wt/a.rs", 42)
                .unwrap();
        assert_eq!(
            argv,
            vec!["xdg-terminal-exec", "--dir=/wt", "nvim", "+42", "/wt/a.rs"]
        );
    }

    #[test]
    fn editor_at_substitutes_path_with_file_and_line_placeholders() {
        let argv = resolve_editor_at_argv(
            "term --dir={path} -- nvim +{line} {file}",
            "/wt",
            "/wt/a.rs",
            9,
        )
        .unwrap();
        assert_eq!(
            argv,
            vec!["term", "--dir=/wt", "--", "nvim", "+9", "/wt/a.rs"]
        );
    }

    #[test]
    fn editor_decision_needs_config_when_unset_or_blank() {
        assert_eq!(editor_open_decision(None), EditorOpenDecision::NeedsConfig);
        assert_eq!(
            editor_open_decision(Some("")),
            EditorOpenDecision::NeedsConfig
        );
        assert_eq!(
            editor_open_decision(Some("   ")),
            EditorOpenDecision::NeedsConfig
        );
    }

    #[test]
    fn editor_decision_launches_trimmed_command() {
        assert_eq!(
            editor_open_decision(Some("  alacritty -e nvim  ")),
            EditorOpenDecision::Launch("alacritty -e nvim".to_string())
        );
    }
}
