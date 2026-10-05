//! Jump to a workspace from outside the TUI: tell a running TUI to select it
//! over the socket in `crate::app::ipc` and raise that TUI's window (see
//! `desktop::focus`), or launch a fresh TUI on it. Shared by
//! `wsx waybar jump` and the Plasma applet.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Jump to a workspace: tell a running TUI to select it and focus that
/// window, or launch a fresh TUI on it.
pub fn jump(repo: &str, slug: &str) -> Result<()> {
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
    spawn_tui(repo, slug)
}

fn spawn_tui(repo: &str, slug: &str) -> Result<()> {
    let term = std::env::var("TERMINAL").unwrap_or_else(|_| "alacritty".into());
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wsx"));
    let mut cmd = Command::new(&term);
    cmd.arg("-e")
        .arg(exe)
        .arg("--select")
        .arg(format!("{repo}/{slug}"))
        .stdin(Stdio::null())
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
