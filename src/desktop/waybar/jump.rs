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
                    focus_window_of(pid);
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

fn ppid_from_stat(stat: &str) -> Option<u32> {
    // comm is parenthesized and may itself contain ')' — split on the LAST ')'.
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().nth(1)?.parse().ok() // state, ppid, ...
}

fn ancestor_pids(pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut current = pid;
    while chain.len() < 32 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{current}/stat")) else {
            break;
        };
        let Some(ppid) = ppid_from_stat(&stat) else {
            break;
        };
        if ppid <= 1 {
            break;
        }
        chain.push(ppid);
        current = ppid;
    }
    chain
}

fn client_pid_for_chain(clients_json: &str, chain: &[u32]) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(clients_json).ok()?;
    let clients = v.as_array()?;
    // chain is self→ancestors; the first chain pid with a window wins.
    chain.iter().copied().find(|pid| {
        clients
            .iter()
            .any(|c| c.get("pid").and_then(|p| p.as_u64()) == Some(u64::from(*pid)))
    })
}

fn focus_window_of(tui_pid: u32) {
    let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
        return; // not Hyprland — selection still happened
    };
    let chain = ancestor_pids(tui_pid);
    if let Some(pid) = client_pid_for_chain(&String::from_utf8_lossy(&out.stdout), &chain) {
        let _ = Command::new("hyprctl")
            .args(["dispatch", "focuswindow", &format!("pid:{pid}")])
            .status();
    }
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

#[cfg(test)]
mod jump_tests {
    use super::*;

    #[test]
    fn client_pid_prefers_closest_ancestor() {
        let clients = r#"[
            {"address":"0x1","pid":900,"class":"Alacritty"},
            {"address":"0x2","pid":300,"class":"ghostty"}
        ]"#;
        // chain is ordered self → parent → grandparent
        assert_eq!(client_pid_for_chain(clients, &[100, 300, 900]), Some(300));
        assert_eq!(client_pid_for_chain(clients, &[100, 200]), None);
        assert_eq!(client_pid_for_chain("not json", &[100]), None);
        assert_eq!(client_pid_for_chain("[]", &[100]), None);
    }

    #[test]
    fn ancestor_pids_walks_proc() {
        let chain = ancestor_pids(std::process::id());
        assert_eq!(chain.first(), Some(&std::process::id()));
        assert!(
            chain.len() >= 2,
            "expected at least self + parent, got {chain:?}"
        );
        assert!(chain.len() <= 32);
    }

    #[test]
    fn stat_ppid_parses_despite_parens_in_comm() {
        assert_eq!(ppid_from_stat("123 (weird) name) S 77 123 123 0"), Some(77));
        assert_eq!(ppid_from_stat("123 (simple) S 1 123"), Some(1));
        assert_eq!(ppid_from_stat("garbage"), None);
    }
}
