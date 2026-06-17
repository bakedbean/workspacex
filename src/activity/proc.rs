//! Per-workspace process detection.
//!
//! wsx never spawns these processes; it observes the system via `lsof`
//! and offers a kill hook. See `docs/superpowers/specs/2026-05-15-process-tracking-design.md`.

use crate::data::store::WorkspaceId;
use crate::error::{Error, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub ppid: i32,
    /// Short process name (`comm`), e.g. `node`. Refined from
    /// `ps -axo comm=` and used for denylist matching — see `scan`.
    pub command: String,
    /// Full command line with arguments, e.g.
    /// `node /path/server.js --port 3000`. Populated from
    /// `ps -axo command=` for display only; never used for filtering.
    /// Empty when the cwd-only `lsof` parser builds the proc and `ps`
    /// hasn't refined it yet.
    pub cmdline: String,
    pub cwd: PathBuf,
    /// True if the process holds at least one listening TCP socket.
    /// Used to rescue genuine servers (e.g. a `pnpm dev` on :3000) from
    /// the ancestor denylist when they were spawned under `claude` via
    /// Claude Code's background runner — MCP stdio helpers, which never
    /// listen, stay hidden. Populated by `scan`; the cwd-only parser
    /// leaves it `false`.
    pub listening: bool,
}

/// Process names that should never count as user processes for a
/// workspace, even when their cwd matches. Covers shells and
/// multiplexers (which host user work but aren't themselves
/// interesting), wsx-spawned things (claude), and editors launched
/// via `[e]`.
pub const PROC_DENYLIST: &[&str] = &[
    // shells + multiplexers — host the user's work, don't propagate
    "bash", "zsh", "fish", "sh", "dash", "ash", "tmux", "screen",
    // self-and-descendants set, mirrored below in PROC_DENYLIST_PROPAGATING
    "wsx", "claude", "nvim", "vim", "emacs", "code", "cursor",
];

/// Subset of `PROC_DENYLIST` whose **descendants** are also hidden.
///
/// `wsx` and `claude` actively spawn helper processes (MCP servers
/// launched via `npm exec`, language servers, `caffeinate`) that
/// inherit the worktree cwd. Editors spawn LSP and formatter children
/// the same way. Without ancestor propagation those would dominate
/// the per-workspace list without representing user-launched work.
///
/// Shells/multiplexers are deliberately excluded — `npm run dev` from
/// a zsh inside a worktree is real user work and should appear.
///
/// Invariant: every entry here must also be in `PROC_DENYLIST` (the
/// process itself is always hidden too). Enforced by
/// `propagating_is_subset_of_denylist` test.
pub const PROC_DENYLIST_PROPAGATING: &[&str] =
    &["wsx", "claude", "nvim", "vim", "emacs", "code", "cursor"];

/// Cap on ancestor chain traversal. Real process trees are shallow
/// (<20); a higher bound just protects against malformed input that
/// would otherwise cycle.
const MAX_ANCESTOR_DEPTH: usize = 64;

/// Parse `lsof -d cwd -F pcRn` output into a list of `ProcInfo`.
///
/// Each process is a block of lines beginning with single-char field
/// indicators: `p` (pid), `R` (ppid), `c` (command), `n` (cwd path).
/// Blocks are not separated by blank lines — the next `p` starts a
/// new block. Unknown tags (e.g. `f` for fd type, which lsof emits
/// unsolicited) are ignored.
pub fn parse_lsof_output(raw: &str) -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let mut pid: Option<i32> = None;
    let mut ppid: Option<i32> = None;
    let mut command: Option<String> = None;
    let mut cwd: Option<String> = None;

    let flush = |pid: &mut Option<i32>,
                 ppid: &mut Option<i32>,
                 command: &mut Option<String>,
                 cwd: &mut Option<String>,
                 out: &mut Vec<ProcInfo>| {
        if let (Some(p), Some(c), Some(n)) = (pid.take(), command.take(), cwd.take()) {
            out.push(ProcInfo {
                pid: p,
                ppid: ppid.take().unwrap_or(0),
                command: c,
                // lsof gives no argv; `scan` fills this from `ps` later.
                cmdline: String::new(),
                cwd: PathBuf::from(n),
                listening: false,
            });
        } else {
            // Discard partial block fields so they don't bleed into
            // the next process.
            ppid.take();
        }
    };

    for line in raw.lines() {
        let Some((tag, rest)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => {
                // Starting a new block — flush the previous one.
                flush(&mut pid, &mut ppid, &mut command, &mut cwd, &mut out);
                pid = rest.parse::<i32>().ok();
            }
            "R" => ppid = rest.parse::<i32>().ok(),
            "c" => command = Some(rest.to_string()),
            "n" => cwd = Some(rest.to_string()),
            _ => {}
        }
    }
    flush(&mut pid, &mut ppid, &mut command, &mut cwd, &mut out);
    out
}

/// Parse `lsof -nP -iTCP -sTCP:LISTEN -F pn` output into the set of
/// pids holding at least one listening TCP socket.
///
/// Only the `p<pid>` lines matter; the `n<addr>` socket lines (one or
/// more per pid) are ignored. A pid with several listening sockets
/// collapses to a single set entry.
pub fn parse_listening_pids(raw: &str) -> std::collections::HashSet<i32> {
    let mut out = std::collections::HashSet::new();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix('p')
            && let Ok(pid) = rest.parse::<i32>()
        {
            out.insert(pid);
        }
    }
    out
}

/// Return true if any ancestor in the process chain has a command in
/// `PROC_DENYLIST_PROPAGATING`. Walks up to `MAX_ANCESTOR_DEPTH` to
/// bound any pathological cycle. Shells and multiplexers are
/// intentionally NOT propagating — a `node` whose ancestor chain is
/// just `npm → zsh` is real user work and should be kept.
fn ancestor_denied(start_ppid: i32, by_pid: &HashMap<i32, &ProcInfo>) -> bool {
    let mut current = start_ppid;
    for _ in 0..MAX_ANCESTOR_DEPTH {
        if current <= 1 {
            return false;
        }
        let Some(parent) = by_pid.get(&current) else {
            return false;
        };
        if PROC_DENYLIST_PROPAGATING.contains(&parent.command.as_str()) {
            return true;
        }
        current = parent.ppid;
    }
    false
}

/// Bucket processes by which workspace's worktree their cwd falls under,
/// dropping any process whose own command is on `PROC_DENYLIST` or
/// whose ancestor chain includes a `PROC_DENYLIST_PROPAGATING` entry.
/// The ancestor check is what hides Claude Code's MCP server children
/// (npm exec wrapper + node) and editor language servers, which
/// inherit cwd from their denylisted parent — while still showing
/// `npm run dev` launched from a shell.
///
/// A claude-descended process that holds a listening socket
/// (`p.listening`) is exempt from the ancestor check: it's a genuine
/// server (a dev server started via Claude Code's background runner)
/// the user wants to see and kill, not stdio MCP noise. The
/// self-denylist still applies unconditionally, so a listening editor
/// or `claude` itself never reappears.
pub fn bucket_by_worktree(
    procs: &[ProcInfo],
    worktrees: &[(WorkspaceId, &Path)],
) -> HashMap<WorkspaceId, Vec<ProcInfo>> {
    let by_pid: HashMap<i32, &ProcInfo> = procs.iter().map(|p| (p.pid, p)).collect();
    let mut out: HashMap<WorkspaceId, Vec<ProcInfo>> = HashMap::new();
    for p in procs {
        if PROC_DENYLIST.contains(&p.command.as_str()) {
            continue;
        }
        if !p.listening && ancestor_denied(p.ppid, &by_pid) {
            continue;
        }
        for (id, wt) in worktrees {
            if p.cwd.starts_with(wt) {
                out.entry(*id).or_default().push(p.clone());
                break;
            }
        }
    }
    out
}

/// Parse `ps -axo pid=,comm=` output into a `pid → command` map.
///
/// macOS quirk: `lsof -F c` reports the kernel's `p_comm` (set at exec
/// time, 15-char cap, basename-of-executable). For Claude Code this is
/// `"2.1.145"` (the version segment of its install path), not
/// `"claude"`. `ps -o comm` reads `process.title` at observation time,
/// which Claude Code sets to `"claude"` — so it's the authoritative
/// source for the denylist check.
///
/// We keep only the first whitespace-separated token to strip any
/// argv-tail (`npm` sets `process.title` to `"npm exec @foo/bar"`),
/// and take the basename to handle full-path entries like
/// `"/sbin/launchd"`.
pub fn parse_ps_comm(raw: &str) -> HashMap<i32, String> {
    let mut out = HashMap::new();
    for line in raw.lines() {
        let line = line.trim_start();
        let mut split = line.splitn(2, char::is_whitespace);
        let Some(pid_str) = split.next() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<i32>() else {
            continue;
        };
        let rest = split.next().unwrap_or("").trim_start();
        let first_token = rest.split_whitespace().next().unwrap_or("");
        if first_token.is_empty() {
            continue;
        }
        let comm = Path::new(first_token)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(first_token)
            .to_string();
        out.insert(pid, comm);
    }
    out
}

/// Parse `ps -axo pid=,command=` output into a `pid → full command line`
/// map for display.
///
/// Unlike `parse_ps_comm`, this keeps the entire argv tail — everything
/// after the leading pid token — so the process modal can show
/// `node /path/server.js --port 3000` rather than just `node`. The value
/// is display-only and never feeds the denylist, so we don't basename or
/// tokenize it. Lines with a non-numeric pid (the header) or an empty
/// command are dropped.
pub fn parse_ps_command(raw: &str) -> HashMap<i32, String> {
    let mut out = HashMap::new();
    for line in raw.lines() {
        let line = line.trim_start();
        let mut split = line.splitn(2, char::is_whitespace);
        let Some(pid_str) = split.next() else {
            continue;
        };
        let Ok(pid) = pid_str.parse::<i32>() else {
            continue;
        };
        let cmdline = split.next().unwrap_or("").trim_start();
        if cmdline.is_empty() {
            continue;
        }
        out.insert(pid, cmdline.to_string());
    }
    out
}

/// Run `lsof -d cwd -F pcRn` (pid, ppid, cwd) in parallel with
/// `ps -axo pid=,comm=` (authoritative command names) and
/// `ps -axo pid=,command=` (full command lines for display), merge the
/// results, and return the parsed process list.
///
/// Failure handling is asymmetric and intentional. `lsof` provides the
/// pid/ppid/cwd that the bucketer needs to function at all — if it's
/// missing or fails, we return an empty list. The `comm` `ps` only
/// refines the `comm` field; if it fails, we keep lsof's `comm` (the
/// macOS-quirky `p_comm`) so the dashboard still works with slightly
/// degraded denylist matching. The `command` `ps` only fills the
/// display-only `cmdline`; if it fails, `cmdline` stays empty and the
/// modal falls back to the short `command`.
///
/// A third lsof, for listening TCP sockets, runs alongside and marks
/// `listening` on each matching proc. Like `ps`, it only refines the
/// result: if it fails, every proc keeps `listening = false` and the
/// dashboard degrades to the pre-listening behavior (claude-descended
/// servers stay hidden) rather than breaking.
pub async fn scan() -> Vec<ProcInfo> {
    let lsof_fut = tokio::process::Command::new("lsof")
        .args(["-d", "cwd", "-F", "pcRn"])
        .output();
    let ps_fut = tokio::process::Command::new("ps")
        .args(["-axo", "pid=,comm="])
        .output();
    let cmd_fut = tokio::process::Command::new("ps")
        .args(["-axo", "pid=,command="])
        .output();
    let listen_fut = tokio::process::Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pn"])
        .output();
    let (lsof_out, ps_out, cmd_out, listen_out) =
        tokio::join!(lsof_fut, ps_fut, cmd_fut, listen_fut);

    let mut procs = match lsof_out {
        // lsof exits 1 when some processes can't be inspected; the
        // stdout it does produce is still valid. Only treat fully
        // empty + nonzero as "missing/broken."
        Ok(o) if o.status.success() || !o.stdout.is_empty() => {
            parse_lsof_output(&String::from_utf8_lossy(&o.stdout))
        }
        _ => return Vec::new(),
    };

    if let Ok(o) = ps_out
        && (o.status.success() || !o.stdout.is_empty())
    {
        let comm_map = parse_ps_comm(&String::from_utf8_lossy(&o.stdout));
        for p in &mut procs {
            if let Some(comm) = comm_map.get(&p.pid) {
                p.command = comm.clone();
            }
        }
    }

    if let Ok(o) = cmd_out
        && (o.status.success() || !o.stdout.is_empty())
    {
        let cmd_map = parse_ps_command(&String::from_utf8_lossy(&o.stdout));
        for p in &mut procs {
            if let Some(cmdline) = cmd_map.get(&p.pid) {
                p.cmdline = cmdline.clone();
            }
        }
    }

    if let Ok(o) = listen_out
        && (o.status.success() || !o.stdout.is_empty())
    {
        let listening = parse_listening_pids(&String::from_utf8_lossy(&o.stdout));
        for p in &mut procs {
            if listening.contains(&p.pid) {
                p.listening = true;
            }
        }
    }
    procs
}

/// Send a signal to a process. `signal` is the `kill -<signal>` arg
/// ("TERM" or "KILL"). Returns Ok on success and on ESRCH ("No such
/// process") — the latter is treated as success because it means the
/// process exited between scan and kill. Other kill failures
/// (permission denied, invalid signal) propagate as `Error::UserInput`.
pub async fn kill_pid(pid: i32, signal: &str) -> Result<()> {
    let output = tokio::process::Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(pid.to_string())
        .output()
        .await
        .map_err(|e| Error::Io(std::io::Error::other(format!("spawn kill: {e}"))))?;
    if output.status.success() {
        return Ok(());
    }
    // kill returns exit code 1 for various reasons; ESRCH is the
    // only one we silently absorb (the process exited between scan
    // and our kill — equivalent to success from the user's POV).
    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("No such process") {
        return Ok(());
    }
    Err(Error::UserInput(format!(
        "kill pid {pid} ({signal}): {}",
        stderr.trim()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proc(pid: i32, ppid: i32, command: &str, cwd: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid,
            command: command.into(),
            cmdline: String::new(),
            cwd: PathBuf::from(cwd),
            listening: false,
        }
    }

    #[test]
    fn parse_lsof_output_handles_three_processes() {
        let raw = "p1234\nR1\ncnpm\nn/home/u/wt/a\np5678\nR1234\ncnode\nn/home/u/wt/a\np9012\nR1\ncbash\nn/home/u/wt/b\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 3);
        assert_eq!(procs[0].pid, 1234);
        assert_eq!(procs[0].ppid, 1);
        assert_eq!(procs[0].command, "npm");
        assert_eq!(procs[0].cwd, PathBuf::from("/home/u/wt/a"));
        assert_eq!(procs[1].ppid, 1234);
        assert_eq!(procs[2].command, "bash");
    }

    #[test]
    fn parse_lsof_output_ignores_unknown_tags() {
        // lsof emits `f` (fd type) lines unsolicited; they must not
        // affect the parse.
        let raw = "p1\nR0\ncfoo\nfcwd\nn/x\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].command, "foo");
    }

    #[test]
    fn parse_lsof_output_defaults_ppid_to_zero_when_missing() {
        let raw = "p1\ncfoo\nn/x\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].ppid, 0);
    }

    #[test]
    fn parse_lsof_output_handles_empty() {
        assert!(parse_lsof_output("").is_empty());
    }

    #[test]
    fn parse_lsof_output_skips_block_missing_pid() {
        // A block with c and n but no p is dropped (malformed) and
        // its R/c/n fields don't bleed into the next block.
        let raw = "R99\ncstray\nn/tmp\np1\nR2\ncgood\nn/x\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].pid, 1);
        assert_eq!(procs[0].ppid, 2);
        assert_eq!(procs[0].command, "good");
    }

    #[test]
    fn bucket_groups_by_descendant_match() {
        let procs = vec![
            proc(1, 0, "npm", "/wt/a"),
            proc(2, 0, "node", "/wt/a/sub/dir"),
            proc(3, 0, "pytest", "/wt/b"),
            proc(4, 0, "elsewhere", "/other"),
        ];
        let worktrees: Vec<(WorkspaceId, &Path)> = vec![
            (WorkspaceId(10), Path::new("/wt/a")),
            (WorkspaceId(20), Path::new("/wt/b")),
        ];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert_eq!(bucketed.get(&WorkspaceId(10)).unwrap().len(), 2);
        assert_eq!(bucketed.get(&WorkspaceId(20)).unwrap().len(), 1);
        assert!(!bucketed.contains_key(&WorkspaceId(30)));
    }

    #[test]
    fn bucket_filters_out_denylist_commands() {
        let procs = vec![
            proc(1, 0, "bash", "/wt/a"),
            proc(2, 0, "npm", "/wt/a"),
            proc(3, 0, "claude", "/wt/a"),
            proc(4, 0, "nvim", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        let list = bucketed.get(&WorkspaceId(10)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].command, "npm");
    }

    #[test]
    fn bucket_excludes_non_matching_cwd() {
        let procs = vec![proc(1, 0, "npm", "/somewhere/else")];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn bucket_excludes_child_of_denylisted_ancestor() {
        // node forked directly by claude: claude itself is dropped by
        // its own denylist hit, and node is dropped because its parent
        // is on the denylist.
        let procs = vec![
            proc(100, 1, "claude", "/wt/a"),
            proc(200, 100, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn bucket_excludes_transitive_descendant_of_denylisted_ancestor() {
        // node -> npm -> claude. Neither npm nor node should be
        // attributed: claude is two hops up but still on the chain.
        let procs = vec![
            proc(100, 1, "claude", "/wt/a"),
            proc(200, 100, "npm", "/wt/a"),
            proc(300, 200, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn bucket_keeps_npm_run_dev_under_shell() {
        // node -> npm -> zsh. The shell is on PROC_DENYLIST (self
        // hidden) but NOT propagating — so the npm wrapper and the
        // dev-server node that descend from it are kept. This is the
        // critical "user runs `npm run dev` from a terminal" path.
        let procs = vec![
            proc(100, 1, "zsh", "/wt/a"),
            proc(200, 100, "npm", "/wt/a"),
            proc(300, 200, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        let list = bucketed.get(&WorkspaceId(10)).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|p| p.command == "npm"));
        assert!(list.iter().any(|p| p.command == "node"));
    }

    #[test]
    fn bucket_keeps_work_launched_inside_tmux() {
        // node -> npm -> zsh -> tmux. tmux is self-hidden but
        // non-propagating, same as a shell. Real work inside a tmux
        // session must remain visible.
        let procs = vec![
            proc(100, 1, "tmux", "/wt/a"),
            proc(200, 100, "zsh", "/wt/a"),
            proc(300, 200, "npm", "/wt/a"),
            proc(400, 300, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        let list = bucketed.get(&WorkspaceId(10)).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn bucket_excludes_lsp_under_editor() {
        // node tsserver -> nvim. nvim is on PROC_DENYLIST_PROPAGATING,
        // so its LSP child node is hidden. Editors spawn language
        // servers we don't want to count as user work.
        let procs = vec![
            proc(100, 1, "nvim", "/wt/a"),
            proc(200, 100, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn propagating_is_subset_of_denylist() {
        // Invariant documented on PROC_DENYLIST_PROPAGATING: anything
        // that propagates must also be self-denied. Otherwise an
        // editor could appear in its own workspace's process list.
        for &cmd in PROC_DENYLIST_PROPAGATING {
            assert!(
                PROC_DENYLIST.contains(&cmd),
                "{cmd} is in PROC_DENYLIST_PROPAGATING but missing from PROC_DENYLIST"
            );
        }
    }

    #[test]
    fn bucket_keeps_process_with_unknown_ancestor() {
        // ppid points at a pid not in our snapshot (parent exited or
        // never had cwd visible). The chain bottoms out cleanly — the
        // process should be kept.
        let procs = vec![proc(200, 999, "node", "/wt/a")];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert_eq!(bucketed.get(&WorkspaceId(10)).unwrap().len(), 1);
    }

    #[test]
    fn bucket_keeps_process_with_non_denylisted_ancestor_chain() {
        // node -> make -> python. None denylisted; node should be
        // kept. This is the "real user work" case.
        let procs = vec![
            proc(100, 1, "python", "/wt/a"),
            proc(200, 100, "make", "/wt/a"),
            proc(300, 200, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert_eq!(bucketed.get(&WorkspaceId(10)).unwrap().len(), 3);
    }

    #[test]
    fn parse_ps_comm_handles_simple_basename() {
        // Single short comm — typical for processes that set
        // process.title (claude, node, zsh).
        let raw = "  1275 zsh\n77875 claude\n78287 node\n";
        let map = parse_ps_comm(raw);
        assert_eq!(map.get(&1275).map(String::as_str), Some("zsh"));
        assert_eq!(map.get(&77875).map(String::as_str), Some("claude"));
        assert_eq!(map.get(&78287).map(String::as_str), Some("node"));
    }

    #[test]
    fn parse_ps_comm_strips_argv_tail() {
        // npm sets process.title to "npm exec <pkg>"; we want just "npm".
        let raw = "77885 npm exec @playwright/mcp@latest\n";
        let map = parse_ps_comm(raw);
        assert_eq!(map.get(&77885).map(String::as_str), Some("npm"));
    }

    #[test]
    fn parse_ps_comm_strips_path_to_basename() {
        // System binaries appear as full paths in ps -o comm output.
        let raw = "    1 /sbin/launchd\n  287 /Applications/Firefox.app/Contents/MacOS/plugin-container\n";
        let map = parse_ps_comm(raw);
        assert_eq!(map.get(&1).map(String::as_str), Some("launchd"));
        assert_eq!(map.get(&287).map(String::as_str), Some("plugin-container"));
    }

    #[test]
    fn parse_ps_comm_skips_malformed_lines() {
        // Non-numeric pid token or empty comm — silently dropped.
        let raw = "PID COMM\n12345 \n\nfoo bar\n42 ok\n";
        let map = parse_ps_comm(raw);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&42).map(String::as_str), Some("ok"));
    }

    #[test]
    fn parse_ps_command_extracts_full_command_line() {
        // `ps -axo pid=,command=` emits the full argv after the pid. We
        // keep everything past the first whitespace run, preserving
        // internal argument spacing.
        let raw = "  41203 node /Users/eben/proj/server.js --port 3000\n 1275 -zsh\n";
        let map = parse_ps_command(raw);
        assert_eq!(
            map.get(&41203).map(String::as_str),
            Some("node /Users/eben/proj/server.js --port 3000")
        );
        assert_eq!(map.get(&1275).map(String::as_str), Some("-zsh"));
    }

    #[test]
    fn parse_ps_command_skips_malformed_lines() {
        // Header row (non-numeric pid) and a pid with an empty command
        // are both dropped; a clean row survives.
        let raw = "PID COMMAND\n12345 \n42 python -m http.server 8080\n";
        let map = parse_ps_command(raw);
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get(&42).map(String::as_str),
            Some("python -m http.server 8080")
        );
    }

    #[test]
    fn parse_ps_command_handles_empty() {
        assert!(parse_ps_command("").is_empty());
    }

    #[test]
    fn ancestor_walk_terminates_on_cycle() {
        // Pathological: pid 100 claims pid 200 as parent and vice
        // versa. The walk must not loop forever. Neither is
        // denylisted, so neither is excluded by the ancestor check.
        let procs = vec![
            proc(100, 200, "node", "/wt/a"),
            proc(200, 100, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert_eq!(bucketed.get(&WorkspaceId(10)).unwrap().len(), 2);
    }

    #[test]
    fn bucket_keeps_listening_server_under_claude() {
        // The motivating case: a dev server (`node` on :3000) launched
        // by Claude Code via run_in_background, so `claude` is in its
        // ancestor chain. The ancestor denylist would normally hide it,
        // but a process holding a listening socket is real user work the
        // user wants to see and be able to kill — so it's kept.
        let procs = vec![
            proc(100, 1, "claude", "/wt/a"),
            ProcInfo {
                listening: true,
                ..proc(200, 100, "node", "/wt/a")
            },
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        let list = bucketed.get(&WorkspaceId(10)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].command, "node");
    }

    #[test]
    fn bucket_still_drops_non_listening_node_under_claude() {
        // An MCP stdio helper (`node`, no listening socket) descended
        // from claude must stay hidden — the listening exception is the
        // only thing that rescues a claude descendant.
        let procs = vec![
            proc(100, 1, "claude", "/wt/a"),
            proc(200, 100, "node", "/wt/a"),
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn bucket_drops_self_denylisted_process_even_when_listening() {
        // The self-denylist is absolute: a listening socket must NOT
        // rescue a process whose own command is denylisted (an editor
        // or claude with a debug port shouldn't appear in its own
        // workspace's list).
        let procs = vec![ProcInfo {
            listening: true,
            ..proc(100, 1, "claude", "/wt/a")
        }];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }

    #[test]
    fn parse_listening_pids_collects_pids() {
        // `lsof -nP -iTCP -sTCP:LISTEN -F pn` emits a `p<pid>` line per
        // process followed by one or more `n<addr>` socket lines. We
        // only need the pid set; multiple sockets for one pid collapse.
        let raw = "p1234\nn*:3000\np5678\nn127.0.0.1:8080\nn*:8081\n";
        let pids = parse_listening_pids(raw);
        assert_eq!(pids.len(), 2);
        assert!(pids.contains(&1234));
        assert!(pids.contains(&5678));
    }

    #[test]
    fn parse_listening_pids_handles_empty() {
        assert!(parse_listening_pids("").is_empty());
    }
}
