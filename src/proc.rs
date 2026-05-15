//! Per-workspace process detection.
//!
//! wsx never spawns these processes; it observes the system via `lsof`
//! and offers a kill hook. See `docs/superpowers/specs/2026-05-15-process-tracking-design.md`.

use crate::error::{Error, Result};
use crate::store::WorkspaceId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: i32,
    pub command: String,
    pub cwd: PathBuf,
}

/// Process names that should never count as user processes for a
/// workspace, even when their cwd matches. Covers shells (which host
/// the interesting children but aren't themselves interesting),
/// wsx-spawned things (claude), and editors launched via `[e]`.
pub const PROC_DENYLIST: &[&str] = &[
    "bash", "zsh", "fish", "sh", "dash", "ash", "wsx", "claude", "nvim", "vim", "emacs", "code",
    "cursor", "tmux", "screen",
];

/// Parse `lsof -d cwd -F pcn` output into a list of `ProcInfo`.
///
/// Each process is a block of lines beginning with single-char field
/// indicators: `p` (pid), `c` (command), `n` (cwd path). Blocks are
/// not separated by blank lines — the next `p` starts a new block.
pub fn parse_lsof_output(raw: &str) -> Vec<ProcInfo> {
    let mut out = Vec::new();
    let mut pid: Option<i32> = None;
    let mut command: Option<String> = None;
    let mut cwd: Option<String> = None;

    let flush = |pid: &mut Option<i32>,
                 command: &mut Option<String>,
                 cwd: &mut Option<String>,
                 out: &mut Vec<ProcInfo>| {
        if let (Some(p), Some(c), Some(n)) = (pid.take(), command.take(), cwd.take()) {
            out.push(ProcInfo {
                pid: p,
                command: c,
                cwd: PathBuf::from(n),
            });
        }
    };

    for line in raw.lines() {
        let Some((tag, rest)) = line.split_at_checked(1) else {
            continue;
        };
        match tag {
            "p" => {
                // Starting a new block — flush the previous one.
                flush(&mut pid, &mut command, &mut cwd, &mut out);
                pid = rest.parse::<i32>().ok();
            }
            "c" => command = Some(rest.to_string()),
            "n" => cwd = Some(rest.to_string()),
            _ => {}
        }
    }
    flush(&mut pid, &mut command, &mut cwd, &mut out);
    out
}

/// Bucket processes by which workspace's worktree their cwd falls under,
/// applying the deny-list filter on command name.
pub fn bucket_by_worktree(
    procs: &[ProcInfo],
    worktrees: &[(WorkspaceId, &Path)],
) -> HashMap<WorkspaceId, Vec<ProcInfo>> {
    let mut out: HashMap<WorkspaceId, Vec<ProcInfo>> = HashMap::new();
    for p in procs {
        if PROC_DENYLIST.contains(&p.command.as_str()) {
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

/// Run `lsof -d cwd -F pcn` and return the parsed process list.
/// Returns an empty list (not an error) when `lsof` is missing or
/// fails, so the rest of the dashboard keeps working.
pub async fn scan() -> Vec<ProcInfo> {
    let output = tokio::process::Command::new("lsof")
        .args(["-d", "cwd", "-F", "pcn"])
        .output()
        .await;
    match output {
        Ok(o) if o.status.success() || !o.stdout.is_empty() => {
            // lsof exits 1 when some processes can't be inspected; the
            // stdout it does produce is still valid. Only treat fully
            // empty + nonzero as "missing/broken."
            parse_lsof_output(&String::from_utf8_lossy(&o.stdout))
        }
        _ => Vec::new(),
    }
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

    #[test]
    fn parse_lsof_output_handles_three_processes() {
        let raw = "p1234\ncnpm\nn/home/u/wt/a\np5678\ncnode\nn/home/u/wt/a\np9012\ncbash\nn/home/u/wt/b\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 3);
        assert_eq!(procs[0].pid, 1234);
        assert_eq!(procs[0].command, "npm");
        assert_eq!(procs[0].cwd, PathBuf::from("/home/u/wt/a"));
        assert_eq!(procs[2].command, "bash");
    }

    #[test]
    fn parse_lsof_output_handles_empty() {
        assert!(parse_lsof_output("").is_empty());
    }

    #[test]
    fn parse_lsof_output_skips_block_missing_pid() {
        // A block with c and n but no p is dropped (malformed).
        let raw = "cstray\nn/tmp\np1\ncgood\nn/x\n";
        let procs = parse_lsof_output(raw);
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].pid, 1);
    }

    #[test]
    fn bucket_groups_by_descendant_match() {
        let procs = vec![
            ProcInfo {
                pid: 1,
                command: "npm".into(),
                cwd: PathBuf::from("/wt/a"),
            },
            ProcInfo {
                pid: 2,
                command: "node".into(),
                cwd: PathBuf::from("/wt/a/sub/dir"),
            },
            ProcInfo {
                pid: 3,
                command: "pytest".into(),
                cwd: PathBuf::from("/wt/b"),
            },
            ProcInfo {
                pid: 4,
                command: "elsewhere".into(),
                cwd: PathBuf::from("/other"),
            },
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
            ProcInfo {
                pid: 1,
                command: "bash".into(),
                cwd: PathBuf::from("/wt/a"),
            },
            ProcInfo {
                pid: 2,
                command: "npm".into(),
                cwd: PathBuf::from("/wt/a"),
            },
            ProcInfo {
                pid: 3,
                command: "claude".into(),
                cwd: PathBuf::from("/wt/a"),
            },
            ProcInfo {
                pid: 4,
                command: "nvim".into(),
                cwd: PathBuf::from("/wt/a"),
            },
        ];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        let list = bucketed.get(&WorkspaceId(10)).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].command, "npm");
    }

    #[test]
    fn bucket_excludes_non_matching_cwd() {
        let procs = vec![ProcInfo {
            pid: 1,
            command: "npm".into(),
            cwd: PathBuf::from("/somewhere/else"),
        }];
        let worktrees = vec![(WorkspaceId(10), Path::new("/wt/a") as &Path)];
        let bucketed = bucket_by_worktree(&procs, &worktrees);
        assert!(bucketed.get(&WorkspaceId(10)).is_none_or(|v| v.is_empty()));
    }
}
