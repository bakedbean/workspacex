//! `wsx menubar jump`: select the workspace in a running TUI over the
//! shared unix socket (focusing its terminal app), or spawn a fresh
//! terminal running `wsx --select repo/slug`. Also `copy-path`, the
//! pbcopy action for the SwiftBar submenu.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::data::store::Store;
use crate::error::{Error, Result};

pub fn jump(repo: &str, slug: &str, terminal_cmd: Option<&str>) -> Result<()> {
    for (path, pid) in crate::app::ipc::live_socket_candidates() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(mut stream) => {
                if writeln!(stream, "select {repo} {slug}").is_ok() {
                    focus_app_of(pid);
                    return Ok(());
                }
            }
            Err(_) => {
                // Stale socket from a killed TUI.
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    let res = spawn_tui(repo, slug, terminal_cmd);
    if let Err(e) = &res {
        notify(&format!("jump failed: {e}"));
    }
    res
}

/// osascript notification + stderr — the macOS notify-send analogue.
pub(crate) fn notify(msg: &str) {
    eprintln!("wsx: {msg}");
    let _ = Command::new("/usr/bin/osascript")
        .args([
            "-e",
            &format!(
                "display notification {} with title \"wsx\"",
                applescript_str(msg)
            ),
        ])
        .status();
}

fn applescript_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn ppid_of(pid: u32) -> Option<u32> {
    let out = Command::new("/bin/ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// self → parent → … , capped at 32 like the Linux /proc walk.
fn ancestor_pids(pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut current = pid;
    while chain.len() < 32 {
        let Some(ppid) = ppid_of(current) else { break };
        if ppid <= 1 {
            break;
        }
        chain.push(ppid);
        current = ppid;
    }
    chain
}

/// Parse `lsappinfo info -only bundleid <pid>` output:
/// `"CFBundleIdentifier"="com.googlecode.iterm2"`. Empty / `[ NULL ]`
/// means the pid isn't an app process.
fn parse_bundle_id(s: &str) -> Option<String> {
    let (_, rhs) = s.split_once('=')?;
    let v = rhs.trim().trim_matches('"').trim();
    if v.is_empty() || v == "[ NULL ]" {
        return None;
    }
    Some(v.to_string())
}

/// Best-effort: walk the TUI's ancestor chain, find the first pid that is
/// a real app (lsappinfo knows it), activate it. Any failure → silent
/// skip; the selection already happened (mirror of the hyprctl path).
fn focus_app_of(tui_pid: u32) {
    for pid in ancestor_pids(tui_pid) {
        let Ok(out) = Command::new("/usr/bin/lsappinfo")
            .args(["info", "-only", "bundleid", &pid.to_string()])
            .output()
        else {
            return;
        };
        if let Some(bundle) = parse_bundle_id(&String::from_utf8_lossy(&out.stdout)) {
            let _ = Command::new("/usr/bin/open").args(["-b", &bundle]).status();
            return;
        }
    }
}

fn shquote(s: &str) -> String {
    shlex::try_quote(s)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| format!("'{}'", s.replace(['\'', '\0'], "")))
}

/// terminal_cmd is honored only when it carries a `{cmd}` placeholder —
/// a bare app-open command (`open -a iTerm`) cannot run a command, and
/// guessing an argv position would misfire.
fn resolve_terminal_template(configured: Option<&str>, cmd: &str) -> Option<String> {
    let t = configured?.trim();
    if t.is_empty() || !t.contains("{cmd}") {
        return None;
    }
    Some(t.replace("{cmd}", cmd))
}

fn iterm_installed() -> bool {
    std::path::Path::new("/Applications/iTerm.app").exists()
        || dirs::home_dir().is_some_and(|h| h.join("Applications/iTerm.app").exists())
}

fn spawn_detached(prog: &str, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Own session so it outlives the SwiftBar action process (same
    // pattern as waybar::jump::spawn_tui).
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
        .map_err(|e| Error::UserInput(format!("failed to launch '{prog}': {e}")))?;
    Ok(())
}

fn spawn_tui(repo: &str, slug: &str, terminal_cmd: Option<&str>) -> Result<()> {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wsx"));
    let cmd = format!(
        "{} --select {}",
        shquote(&exe.display().to_string()),
        shquote(&format!("{repo}/{slug}"))
    );
    if let Some(full) = resolve_terminal_template(terminal_cmd, &cmd) {
        return spawn_detached("/bin/sh", &["-c", &full]);
    }
    if iterm_installed() {
        return spawn_detached(
            "/usr/bin/osascript",
            &[
                "-e",
                &format!(
                    "tell application \"iTerm2\" to create window with default profile command {}",
                    applescript_str(&cmd)
                ),
                "-e",
                "tell application \"iTerm2\" to activate",
            ],
        );
    }
    spawn_detached(
        "/usr/bin/osascript",
        &[
            "-e",
            &format!(
                "tell application \"Terminal\" to do script {}",
                applescript_str(&cmd)
            ),
            "-e",
            "tell application \"Terminal\" to activate",
        ],
    )
}

/// Resolve the worktree path from the store (fresh — paths can move) and
/// pipe it to pbcopy.
pub fn copy_path(store: &Store, repo: &str, slug: &str) -> Result<()> {
    for r in crate::data::repo::list(store)? {
        if r.name != repo {
            continue;
        }
        for ws in store.workspaces(r.id)? {
            if ws.name == slug {
                let mut child = Command::new("/usr/bin/pbcopy")
                    .stdin(Stdio::piped())
                    .spawn()
                    .map_err(|e| Error::UserInput(format!("pbcopy failed: {e}")))?;
                child
                    .stdin
                    .take()
                    .expect("piped stdin")
                    .write_all(ws.worktree_path.display().to_string().as_bytes())?;
                child.wait()?;
                return Ok(());
            }
        }
    }
    Err(Error::UserInput(format!("unknown workspace {repo}/{slug}")))
}

#[cfg(test)]
mod jump_tests {
    use super::*;

    #[test]
    fn applescript_str_escapes_quotes_and_backslashes() {
        assert_eq!(applescript_str(r#"say "hi" \now"#), r#""say \"hi\" \\now""#);
    }

    #[test]
    fn parse_bundle_id_variants() {
        assert_eq!(
            parse_bundle_id("\"CFBundleIdentifier\"=\"com.googlecode.iterm2\"\n"),
            Some("com.googlecode.iterm2".into())
        );
        assert_eq!(parse_bundle_id(""), None);
        assert_eq!(parse_bundle_id("\"CFBundleIdentifier\"=\"[ NULL ]\""), None);
        assert_eq!(parse_bundle_id("garbage without equals"), None);
    }

    #[test]
    fn ancestor_pids_walks_ps() {
        let chain = ancestor_pids(std::process::id());
        assert_eq!(chain.first(), Some(&std::process::id()));
        assert!(chain.len() >= 2, "expected self + parent, got {chain:?}");
        assert!(chain.len() <= 32);
    }

    #[test]
    fn terminal_template_requires_cmd_placeholder() {
        // With {cmd}: substituted. Without: None → caller falls through to
        // the osascript paths (a bare `open -a iTerm` can't carry a command).
        assert_eq!(
            resolve_terminal_template(Some("alacritty -e {cmd}"), "wsx --select r/s"),
            Some("alacritty -e wsx --select r/s".into())
        );
        assert_eq!(resolve_terminal_template(Some("open -a iTerm"), "x"), None);
        assert_eq!(resolve_terminal_template(None, "x"), None);
        assert_eq!(resolve_terminal_template(Some("  "), "x"), None);
    }

    #[test]
    fn copy_path_unknown_workspace_errors() {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        assert!(copy_path(&store, "nope", "missing").is_err());
    }
}
