//! tmux integration for shared workspaces.
//!
//! All tmux knowledge lives here: session-name derivation, wrapping an agent
//! `CommandBuilder` so the agent runs inside a tmux server (wsx's PTY child
//! becomes the attach client), and subprocess helpers for session lifecycle.
//! `WSX_TMUX_BIN` overrides the binary, mirroring `WSX_CLAUDE_BIN`.

use crate::pty::AgentKind;
use portable_pty::CommandBuilder;

pub fn tmux_bin() -> String {
    std::env::var("WSX_TMUX_BIN").unwrap_or_else(|_| "tmux".to_string())
}

/// `tmux -V` succeeds — used to gate shared spawns with a friendly error.
pub fn is_available() -> bool {
    std::process::Command::new(tmux_bin())
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Replace anything outside [A-Za-z0-9_-] with '-'. tmux rejects '.' and ':'
/// in session names; the rest is defensive.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Deterministic tmux session name for one agent instance. Primary instances
/// get the bare `wsx-<repo>-<workspace>`; added instances append
/// `-<agent><ordinal>` (matching `instance_label`'s vocabulary, '#' replaced
/// by the ordinal suffix since '#' is a tmux format character).
pub fn session_name(
    repo: &str,
    workspace: &str,
    agent: AgentKind,
    ordinal: i64,
    is_primary: bool,
) -> String {
    let base = format!("wsx-{}-{}", sanitize(repo), sanitize(workspace));
    if is_primary {
        base
    } else {
        format!("{base}-{}{ordinal}", sanitize(agent.display_name()))
    }
}

/// Wrap a built agent command so it runs inside `tmux new-session -A`.
/// The returned builder spawns the tmux *client*; the agent process lives in
/// the tmux *server*. The inner command's env is forwarded with repeated `-e`
/// flags (session environment) because a pre-existing tmux server would not
/// otherwise inherit wsx's environment. TMUX/TMUX_PANE are stripped from both
/// the client env and the forwarded set so nesting under the user's own tmux
/// works.
pub fn wrap_in_tmux(inner: &CommandBuilder, session_name: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(tmux_bin());
    if let Some(cwd) = inner.get_cwd() {
        cmd.cwd(cwd);
    }
    for (k, v) in std::env::vars() {
        if k != "TMUX" && k != "TMUX_PANE" {
            cmd.env(k, v);
        }
    }
    cmd.args(["new-session", "-A", "-s", session_name]);
    if let Some(cwd) = inner.get_cwd().and_then(|c| c.to_str()) {
        cmd.args(["-c", cwd]);
    }
    for (k, v) in inner.iter_extra_env_as_str() {
        if k == "TMUX" || k == "TMUX_PANE" {
            continue;
        }
        cmd.arg("-e");
        cmd.arg(format!("{k}={v}"));
    }
    cmd.arg("--");
    for a in inner.get_argv() {
        cmd.arg(a);
    }
    cmd
}

/// Exact-match (`=name`) session existence check.
pub fn has_session(name: &str) -> bool {
    std::process::Command::new(tmux_bin())
        .args(["has-session", "-t", &format!("={name}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Exact-match kill. Returns true when a session was actually killed.
pub fn kill_session(name: &str) -> bool {
    std::process::Command::new(tmux_bin())
        .args(["kill-session", "-t", &format!("={name}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `window-size latest` stops simultaneously attached clients (desk + laptop)
/// from letterboxing each other to the smallest screen. Must run after the
/// session exists; the client spawn is asynchronous, so retry briefly in a
/// detached thread. Best-effort — a failure only degrades multi-client UX.
pub fn spawn_window_size_fixup(name: String) {
    std::thread::spawn(move || {
        for _ in 0..20 {
            let ok = std::process::Command::new(tmux_bin())
                .args([
                    "set-option",
                    "-t",
                    &format!("={name}"),
                    "window-size",
                    "latest",
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if ok {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::AgentKind;

    #[test]
    fn session_name_primary_and_added() {
        assert_eq!(
            session_name("workspacex", "big-fix", AgentKind::Claude, 1, true),
            "wsx-workspacex-big-fix"
        );
        assert_eq!(
            session_name("workspacex", "big-fix", AgentKind::Codex, 2, false),
            "wsx-workspacex-big-fix-codex2"
        );
    }

    #[test]
    fn session_name_sanitizes_tmux_hostile_chars() {
        // tmux rejects '.' and ':' in session names; spaces are just hostile.
        assert_eq!(
            session_name("my.repo", "fix: thing", AgentKind::Claude, 1, true),
            "wsx-my-repo-fix--thing"
        );
    }

    #[test]
    fn wrap_preserves_argv_env_and_strips_tmux_vars() {
        let mut inner = portable_pty::CommandBuilder::new("claude");
        inner.cwd("/tmp/wt");
        inner.arg("--continue");
        inner.env("WSX_WORKSPACE_ID", "7");
        inner.env("TMUX", "/private/socket,123,0"); // must NOT propagate
        let wrapped = wrap_in_tmux(&inner, "wsx-r-w");
        let argv: Vec<String> = wrapped
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        // head: tmux new-session -A -s <name> -c <cwd>
        assert_eq!(
            argv[1..7],
            ["new-session", "-A", "-s", "wsx-r-w", "-c", "/tmp/wt"]
        );
        // env forwarded via -e, minus TMUX*
        assert!(argv.iter().any(|a| a == "WSX_WORKSPACE_ID=7"));
        assert!(!argv.iter().any(|a| a.starts_with("TMUX=")));
        // tail: -- <inner argv verbatim>
        let sep = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(argv[sep + 1..], ["claude", "--continue"]);
    }
}
