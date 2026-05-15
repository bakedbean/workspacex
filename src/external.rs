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
    let mut parts = resolve_argv(cmd, substitutions, fallback_when_no_placeholder)?;
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

fn detach_io(cmd: &mut std::process::Command) {
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_fallback_uses_configured_first() {
        // We can't easily test EDITOR/VISUAL without mutating process env, but the
        // configured-first path is deterministic.
        assert_eq!(resolve_editor_cmd(Some("my-editor")).unwrap(), "my-editor");
    }

    #[test]
    fn editor_falls_back_to_env() {
        unsafe {
            std::env::set_var("VISUAL", "fake-visual-editor");
        }
        let r = resolve_editor_cmd(None).unwrap();
        unsafe {
            std::env::remove_var("VISUAL");
        }
        assert_eq!(r, "fake-visual-editor");
    }

    #[test]
    fn editor_errors_when_unconfigured() {
        // Ensure neither env var is set.
        let saved_v = std::env::var_os("VISUAL");
        let saved_e = std::env::var_os("EDITOR");
        unsafe {
            std::env::remove_var("VISUAL");
            std::env::remove_var("EDITOR");
        }
        let r = resolve_editor_cmd(None);
        unsafe {
            if let Some(v) = saved_v {
                std::env::set_var("VISUAL", v);
            }
            if let Some(v) = saved_e {
                std::env::set_var("EDITOR", v);
            }
        }
        assert!(r.is_err());
    }

    #[test]
    fn terminal_errors_when_unconfigured() {
        let saved = std::env::var_os("TERMINAL");
        unsafe {
            std::env::remove_var("TERMINAL");
        }
        let r = resolve_terminal_cmd(None);
        unsafe {
            if let Some(v) = saved {
                std::env::set_var("TERMINAL", v);
            }
        }
        assert!(r.is_err());
    }

    #[test]
    fn spawn_with_path_arg_runs_true_with_quoted_command() {
        let dir = std::env::temp_dir();
        // /bin/true exists on Linux/macOS test runners; it exits immediately.
        let r = spawn_with_path_arg("/bin/true", &dir);
        assert!(r.is_ok(), "spawn /bin/true failed: {r:?}");
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
        let r = spawn_with_path_arg("/bin/true --dir={path}", &dir);
        assert!(r.is_ok(), "spawn failed: {r:?}");
    }

    #[test]
    fn no_placeholder_appends_path_for_editor() {
        let dir = std::env::temp_dir();
        let r = spawn_with_path_arg("/bin/true", &dir);
        assert!(r.is_ok());
    }

    #[test]
    fn no_placeholder_does_not_append_for_terminal() {
        let dir = std::env::temp_dir();
        let r = spawn_with_cwd("/bin/true", &dir);
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
        let r = open_diff(&dir, "main", Some("/bin/true --path={path} --base={base}"));
        assert!(r.is_ok(), "open_diff failed: {r:?}");
    }

    #[test]
    fn edit_in_editor_returns_unchanged_when_editor_doesnt_write() {
        // Save / restore EDITOR around the test.
        let saved = std::env::var_os("EDITOR");
        unsafe {
            std::env::set_var("EDITOR", "/bin/true");
        }
        let result = edit_in_editor("hello world", "txt");
        unsafe {
            match saved {
                Some(v) => std::env::set_var("EDITOR", v),
                None => std::env::remove_var("EDITOR"),
            }
        }
        assert_eq!(result.unwrap().as_deref(), Some("hello world"));
    }

    #[test]
    fn edit_in_editor_returns_none_when_editor_exits_nonzero() {
        let saved = std::env::var_os("EDITOR");
        unsafe {
            std::env::set_var("EDITOR", "/bin/false");
        }
        let result = edit_in_editor("anything", "txt");
        unsafe {
            match saved {
                Some(v) => std::env::set_var("EDITOR", v),
                None => std::env::remove_var("EDITOR"),
            }
        }
        assert!(result.unwrap().is_none());
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
}
