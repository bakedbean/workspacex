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
    let mut parts = shlex::split(cmd)
        .ok_or_else(|| Error::UserInput(format!("could not parse command: {cmd}")))?;
    if parts.is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    let program = parts.remove(0);
    let mut command = std::process::Command::new(&program);
    command.args(&parts).arg(path);
    detach_io(&mut command);
    command
        .spawn()
        .map_err(|e| Error::UserInput(format!("spawn {program}: {e}")))?;
    Ok(())
}

fn spawn_with_cwd(cmd: &str, cwd: &Path) -> Result<()> {
    let mut parts = shlex::split(cmd)
        .ok_or_else(|| Error::UserInput(format!("could not parse command: {cmd}")))?;
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
}
