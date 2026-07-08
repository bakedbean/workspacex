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

/// A `std::process::Command` for the tmux binary with TMUX/TMUX_PANE
/// scrubbed, so invocations target the default server even when wsx
/// itself runs inside a tmux session.
fn tmux_cmd() -> std::process::Command {
    let mut cmd = std::process::Command::new(tmux_bin());
    cmd.env_remove("TMUX").env_remove("TMUX_PANE");
    cmd
}

/// `tmux -V` succeeds AND reports a version >= 3.2 (`new-session -e` floor) —
/// used to gate shared spawns with a friendly error upfront instead of a
/// cryptic in-PTY failure later.
pub fn is_available() -> bool {
    tmux_cmd()
        .arg("-V")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .is_some_and(|o| version_supported(&String::from_utf8_lossy(&o.stdout)))
}

/// Parse `tmux -V` output ("tmux 3.6b", "tmux next-3.4", "tmux master") and
/// require >= 3.2. Unversioned dev builds ("master") and unparseable output
/// fail OPEN: they are newer than any release, and refusing to run on them
/// would be a worse failure mode than the late error this check prevents.
fn version_supported(version_output: &str) -> bool {
    let mut nums = Vec::with_capacity(2);
    let mut cur: Option<u32> = None;
    for c in version_output.chars() {
        match (c.to_digit(10), cur) {
            (Some(d), Some(n)) => cur = Some(n.saturating_mul(10) + d),
            (Some(d), None) => cur = Some(d),
            (None, Some(n)) => {
                nums.push(n);
                cur = None;
                if nums.len() == 2 {
                    break;
                }
            }
            (None, None) => {}
        }
    }
    if let (Some(n), true) = (cur, nums.len() < 2) {
        nums.push(n);
    }
    match nums[..] {
        [major, minor, ..] => (major, minor) >= (3, 2),
        [major] => major >= 4,
        [] => true, // "tmux master" etc. — fail open
    }
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
    // `CommandBuilder::new` pre-seeds its env map from the full process
    // environment, so the skip above only prevents *re-adding* TMUX/TMUX_PANE
    // — it doesn't remove the base-env copies. Scrub them explicitly.
    cmd.env_remove("TMUX");
    cmd.env_remove("TMUX_PANE");
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
    tmux_cmd()
        .args(["has-session", "-t", &format!("={name}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Exact-match kill. Returns true when a session was actually killed.
pub fn kill_session(name: &str) -> bool {
    tmux_cmd()
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
            let ok = tmux_cmd()
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
    use crate::test_support::EnvGuard;

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
    fn version_floor_is_3_2_and_dev_builds_fail_open() {
        // Released versions, with and without patch letters.
        assert!(version_supported("tmux 3.6b\n"));
        assert!(version_supported("tmux 3.2a\n"));
        assert!(version_supported("tmux 3.2\n"));
        assert!(!version_supported("tmux 3.1c\n"));
        assert!(!version_supported("tmux 2.9\n"));
        // Pre-release naming carries the target version.
        assert!(version_supported("tmux next-3.4\n"));
        assert!(!version_supported("tmux next-3.1\n"));
        // Unversioned dev builds are newer than any release: fail open.
        assert!(version_supported("tmux master\n"));
        assert!(version_supported(""));
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

    #[test]
    fn wrap_scrubs_tmux_vars_from_actual_child_env() {
        // Regression: `CommandBuilder::new()` pre-seeds its env map from the
        // full process environment, so merely skipping TMUX/TMUX_PANE while
        // forwarding process vars leaves the base-env copies intact. If the
        // real process env has TMUX set (wsx running inside tmux, as in this
        // test), the wrapped command must not carry it through regardless.
        let mut env = EnvGuard::new();
        env.set("TMUX", "/private/socket,123,0");
        env.set("TMUX_PANE", "%42");

        let mut inner = portable_pty::CommandBuilder::new("claude");
        inner.cwd("/tmp/wt");
        let wrapped = wrap_in_tmux(&inner, "wsx-r-w");

        assert!(wrapped.get_env("TMUX").is_none());
        assert!(wrapped.get_env("TMUX_PANE").is_none());
    }
}
