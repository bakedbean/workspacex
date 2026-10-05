//! Jump to a workspace from outside the TUI: tell a running TUI to select it
//! over the socket in `crate::app::ipc` and raise that TUI's window (see
//! `desktop::focus`), or launch a fresh TUI on it. Shared by
//! `wsx waybar jump` and the Plasma applet.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::desktop::install_support::shell_quote;
use crate::desktop::terminal::resolve_terminal_template;
use crate::error::{Error, Result};

/// Jump to a workspace: tell a running TUI to select it and focus that
/// window, or launch a fresh TUI on it. `terminal_cmd` is the setting of
/// that name, used for the launch when it carries `{cmd}`.
pub fn jump(repo: &str, slug: &str, terminal_cmd: Option<&str>) -> Result<()> {
    for (path, pid) in crate::app::ipc::live_socket_candidates() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(mut stream) => {
                if writeln!(stream, "select {repo} {slug}").is_ok() {
                    crate::desktop::focus::focus_window_of(pid);
                    return Ok(());
                }
            }
            Err(_) => {
                // Stale socket from a killed TUI.
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    spawn_tui(repo, slug, terminal_cmd)
}

/// How a jump with no TUI running launches one.
#[derive(Debug, PartialEq, Eq)]
enum Launch {
    /// A `terminal_cmd` template with `{cmd}` filled in, run by `sh -c`.
    Shell(String),
    /// A terminal program, given `-e <wsx> --select <repo>/<slug>`.
    Terminal(String),
}

/// The terminal to launch, in order: a `terminal_cmd` template carrying
/// `{cmd}` (filled with `select_cmd`), `$TERMINAL`, konsole on KDE Plasma
/// (its default terminal, and Plasma sets neither of the others), then
/// alacritty. An empty `$TERMINAL` counts as unset.
fn pick_launch(
    terminal_cmd: Option<&str>,
    terminal_env: Option<&str>,
    kde: bool,
    select_cmd: &str,
) -> Launch {
    if let Some(full) = resolve_terminal_template(terminal_cmd, select_cmd) {
        return Launch::Shell(full);
    }
    if let Some(term) = terminal_env.map(str::trim).filter(|t| !t.is_empty()) {
        return Launch::Terminal(term.to_string());
    }
    Launch::Terminal(if kde { "konsole" } else { "alacritty" }.to_string())
}

fn spawn_tui(repo: &str, slug: &str, terminal_cmd: Option<&str>) -> Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wsx"));
    let select = format!("{repo}/{slug}");
    let select_cmd = format!(
        "{} --select {}",
        shell_quote(&exe.display().to_string()),
        shell_quote(&select)
    );
    let launch = pick_launch(
        terminal_cmd,
        std::env::var("TERMINAL").ok().as_deref(),
        crate::desktop::focus::is_kde(std::env::var("XDG_CURRENT_DESKTOP").ok().as_deref()),
        &select_cmd,
    );
    let (term, mut cmd) = match launch {
        Launch::Shell(full) => {
            let mut cmd = Command::new("sh");
            cmd.arg("-c").arg(&full);
            (full, cmd)
        }
        Launch::Terminal(term) => {
            let mut cmd = Command::new(&term);
            cmd.arg("-e").arg(&exe).arg("--select").arg(&select);
            (term, cmd)
        }
    };
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Detach into its own session so it outlives the menu/jump process
    // (same pattern as src/commands/external.rs:262).
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd.spawn()
        .map_err(|e| Error::UserInput(format!("failed to launch terminal '{term}': {e}")))?;
    Ok(())
}

#[cfg(test)]
mod jump_tests {
    use super::*;

    #[test]
    fn a_jump_with_no_tui_picks_its_terminal_in_order() {
        let select = "wsx --select r/s";
        // A terminal_cmd template with {cmd} wins over everything.
        assert_eq!(
            pick_launch(Some("kitty -- {cmd}"), Some("foot"), true, select),
            Launch::Shell("kitty -- wsx --select r/s".into())
        );
        // Without {cmd} it's the dashboard's [t] terminal, not a jump's.
        assert_eq!(
            pick_launch(Some("kitty"), Some("foot"), true, select),
            Launch::Terminal("foot".into())
        );
        assert_eq!(
            pick_launch(None, Some("foot"), true, select),
            Launch::Terminal("foot".into())
        );
        // Plasma sets neither: konsole there, alacritty elsewhere.
        assert_eq!(
            pick_launch(None, Some("  "), true, select),
            Launch::Terminal("konsole".into())
        );
        assert_eq!(
            pick_launch(None, None, false, select),
            Launch::Terminal("alacritty".into())
        );
    }
}
