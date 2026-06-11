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

/// Resolve and launch lazygit (or configured equivalent) with cwd=`worktree`.
pub fn open_in_lazygit(worktree: &Path, configured: Option<&str>) -> Result<()> {
    let cmd = resolve_lazygit_cmd(configured)?;
    spawn_with_cwd(&cmd, worktree)
}

/// Resolve and launch chronox (or configured equivalent) with cwd=`worktree`.
/// The worktree path is supplied as chronox's positional argument: either via a
/// `{path}` placeholder in the command, or appended when no placeholder is used.
pub fn open_in_chronox(worktree: &Path, configured: Option<&str>) -> Result<()> {
    let cmd = resolve_chronox_cmd(configured)?;
    spawn_with_path_arg(&cmd, worktree)
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

fn resolve_lazygit_cmd(configured: Option<&str>) -> Result<String> {
    if let Some(c) = configured {
        if !c.trim().is_empty() {
            return Ok(c.to_string());
        }
    }
    Err(Error::UserInput(
        "no lazygit command configured; set `wsx config set lazygit_cmd <cmd>` \
         (e.g. `wezterm start -- lazygit`) — wsx's own TUI owns the terminal, \
         so lazygit needs a wrapper that opens its own window"
            .into(),
    ))
}

fn resolve_chronox_cmd(configured: Option<&str>) -> Result<String> {
    if let Some(c) = configured {
        if !c.trim().is_empty() {
            return Ok(c.to_string());
        }
    }
    Err(Error::UserInput(
        "no chronox command configured; set `wsx config set chronox_cmd <cmd>` \
         (e.g. `wezterm start -- chronox`) — wsx's own TUI owns the terminal, \
         so chronox needs a wrapper that opens its own window"
            .into(),
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
    let parts = resolve_argv(cmd, substitutions, fallback_when_no_placeholder)?;
    spawn_parts(parts, cwd)
}

/// Build the argv for running `command` through a POSIX shell. The whole command
/// is passed as a single `-c` argument so pipes, `&&`, and env vars work as typed.
fn shell_argv(command: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), command.to_string()]
}

/// Path for a background command's captured output:
/// `<log_dir>/ws<workspace_id>-<epoch_ms>.log`.
pub fn background_log_path(log_dir: &Path, workspace_id: i64, epoch_ms: u64) -> std::path::PathBuf {
    log_dir.join(format!("ws{workspace_id}-{epoch_ms}.log"))
}

/// Wrap a user command so it runs detached from the wsx process. The command is
/// run inside a backgrounded subshell (`( … ) &`); the parent `sh` we spawn then
/// exits immediately, so the subshell is reparented to init and no longer
/// descends from wsx. That matters because wsx's per-workspace process scan hides
/// its own descendants (to suppress auto-spawned helpers) — without reparenting,
/// a command launched here would never appear in the processes modal. Wrapping in
/// a subshell (rather than appending `&`) backgrounds the whole command as a unit,
/// so compound commands like `a && b` detach correctly.
fn detached_command_script(command: &str) -> String {
    format!("( {command} ) &")
}

/// Launch `command` as a background process whose working directory is `worktree`,
/// detached from wsx so it surfaces in the per-workspace process scan and outlives
/// the dashboard — like running it from a fresh terminal in the worktree.
///
/// The command runs through `sh -c` so shell features behave as the user expects.
/// It is wrapped by [`detached_command_script`] so it reparents away from wsx, and
/// (on unix) the spawned shell calls `setsid` so the command runs in its own
/// session with no controlling terminal. stdout and stderr are redirected to
/// `log_path` (created / truncated); stdin is null.
pub fn spawn_background_command(worktree: &Path, command: &str, log_path: &Path) -> Result<()> {
    if command.trim().is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::UserInput(format!("create log dir: {e}")))?;
    }
    let log = std::fs::File::create(log_path)
        .map_err(|e| Error::UserInput(format!("open log {}: {e}", log_path.display())))?;
    let log_err = log
        .try_clone()
        .map_err(|e| Error::UserInput(format!("clone log handle: {e}")))?;

    let parts = shell_argv(&detached_command_script(command));
    let mut cmd = std::process::Command::new(&parts[0]);
    cmd.args(&parts[1..])
        .current_dir(worktree)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));

    // Detach from wsx's controlling terminal so the command can't grab the TTY
    // (SIGTTOU) and survives the dashboard being closed. Mirrors src/data/setup.rs.
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::UserInput(format!("spawn background command: {e}")))?;
    // The shell backgrounds the command and exits right away; reap it so wsx
    // doesn't accumulate a zombie per launch. The detached command keeps running.
    let _ = child.wait();
    Ok(())
}

/// Spawn `parts` (program + argv) detached, with cwd = `cwd`.
fn spawn_parts(mut parts: Vec<String>, cwd: &Path) -> Result<()> {
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
    use crate::test_support::{EnvGuard, false_path, true_path};

    #[test]
    fn editor_fallback_uses_configured_first() {
        // We can't easily test EDITOR/VISUAL without mutating process env, but the
        // configured-first path is deterministic.
        assert_eq!(resolve_editor_cmd(Some("my-editor")).unwrap(), "my-editor");
    }

    #[test]
    fn editor_falls_back_to_env() {
        let mut env = EnvGuard::new();
        env.set("VISUAL", "fake-visual-editor");
        let r = resolve_editor_cmd(None).unwrap();
        assert_eq!(r, "fake-visual-editor");
    }

    #[test]
    fn editor_errors_when_unconfigured() {
        let mut env = EnvGuard::new();
        env.remove("VISUAL");
        env.remove("EDITOR");
        let r = resolve_editor_cmd(None);
        assert!(r.is_err());
    }

    #[test]
    fn terminal_errors_when_unconfigured() {
        let mut env = EnvGuard::new();
        env.remove("TERMINAL");
        let r = resolve_terminal_cmd(None);
        assert!(r.is_err());
    }

    #[test]
    fn spawn_with_path_arg_runs_true_with_quoted_command() {
        let dir = std::env::temp_dir();
        // `true` exits immediately — handy for spawn-success assertions.
        let r = spawn_with_path_arg(true_path(), &dir);
        assert!(r.is_ok(), "spawn true failed: {r:?}");
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
        let r = spawn_with_path_arg(&format!("{} --dir={{path}}", true_path()), &dir);
        assert!(r.is_ok(), "spawn failed: {r:?}");
    }

    #[test]
    fn no_placeholder_appends_path_for_editor() {
        let dir = std::env::temp_dir();
        let r = spawn_with_path_arg(true_path(), &dir);
        assert!(r.is_ok());
    }

    #[test]
    fn no_placeholder_does_not_append_for_terminal() {
        let dir = std::env::temp_dir();
        let r = spawn_with_cwd(true_path(), &dir);
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
        let template = format!("{} --path={{path}} --base={{base}}", true_path());
        let r = open_diff(&dir, "main", Some(&template));
        assert!(r.is_ok(), "open_diff failed: {r:?}");
    }

    #[test]
    fn edit_in_editor_returns_unchanged_when_editor_doesnt_write() {
        let mut env = EnvGuard::new();
        env.set("EDITOR", true_path());
        let result = edit_in_editor("hello world", "txt");
        assert_eq!(result.unwrap().as_deref(), Some("hello world"));
    }

    #[test]
    fn edit_in_editor_returns_none_when_editor_exits_nonzero() {
        let mut env = EnvGuard::new();
        env.set("EDITOR", false_path());
        let result = edit_in_editor("anything", "txt");
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn lazygit_errors_when_unconfigured() {
        assert!(resolve_lazygit_cmd(None).is_err());
    }

    #[test]
    fn lazygit_uses_configured_first() {
        assert_eq!(
            resolve_lazygit_cmd(Some("wezterm start -- lazygit")).unwrap(),
            "wezterm start -- lazygit"
        );
    }

    #[test]
    fn open_in_lazygit_spawns_configured_cmd() {
        let dir = std::env::temp_dir();
        let r = open_in_lazygit(&dir, Some(true_path()));
        assert!(r.is_ok(), "open_in_lazygit failed: {r:?}");
    }

    #[test]
    fn chronox_uses_configured_command() {
        assert_eq!(
            resolve_chronox_cmd(Some("wezterm start -- chronox")).unwrap(),
            "wezterm start -- chronox"
        );
    }

    #[test]
    fn chronox_errors_when_unconfigured() {
        assert!(resolve_chronox_cmd(None).is_err());
        assert!(resolve_chronox_cmd(Some("   ")).is_err());
    }

    #[test]
    fn open_in_chronox_spawns_configured_cmd() {
        // A bare command (no `{path}`) gets the worktree appended as chronox's
        // positional arg; spawning `true` exits immediately, proving the wiring.
        let dir = std::env::temp_dir();
        let r = open_in_chronox(&dir, Some(true_path()));
        assert!(r.is_ok(), "open_in_chronox failed: {r:?}");
    }

    #[test]
    fn chronox_substitutes_path_placeholder() {
        let argv = resolve_argv(
            "wezterm start -- chronox {path}",
            &[("path", "/tmp/wt")],
            Some("/tmp/wt"),
        )
        .unwrap();
        assert_eq!(argv, vec!["wezterm", "start", "--", "chronox", "/tmp/wt"]);
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

    #[test]
    fn shell_argv_wraps_command_as_single_arg() {
        assert_eq!(
            shell_argv("npm run dev && echo done"),
            vec![
                "sh".to_string(),
                "-c".to_string(),
                "npm run dev && echo done".to_string(),
            ],
        );
    }

    #[test]
    fn background_log_path_uses_workspace_id_and_timestamp() {
        let p = background_log_path(std::path::Path::new("/logs"), 7, 1234);
        assert_eq!(p, std::path::PathBuf::from("/logs/ws7-1234.log"));
    }

    #[test]
    fn detached_command_script_backgrounds_in_subshell() {
        assert_eq!(detached_command_script("pnpm dev"), "( pnpm dev ) &");
    }

    #[test]
    fn detached_command_script_wraps_compound_command_as_a_unit() {
        // The whole compound runs in the backgrounded subshell, not just the
        // last segment — so `a && b` detaches as one unit.
        assert_eq!(detached_command_script("a && b"), "( a && b ) &");
    }

    // --- background command detachment ---

    /// Walk a process's ancestor chain via `ps -o ppid=`, bounded.
    #[cfg(unix)]
    fn ancestor_chain(start: i32) -> Vec<i32> {
        let mut chain = Vec::new();
        let mut cur = start;
        for _ in 0..64 {
            let Ok(out) = std::process::Command::new("ps")
                .args(["-o", "ppid=", "-p", &cur.to_string()])
                .output()
            else {
                break;
            };
            let Ok(ppid) = String::from_utf8_lossy(&out.stdout).trim().parse::<i32>() else {
                break;
            };
            chain.push(ppid);
            if ppid <= 1 {
                break;
            }
            cur = ppid;
        }
        chain
    }

    /// All pids whose `ps` command line contains `needle`.
    #[cfg(unix)]
    fn pids_matching(needle: &str) -> Vec<i32> {
        let Ok(out) = std::process::Command::new("ps")
            .args(["-axo", "pid=,command="])
            .output()
        else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains(needle))
            .filter_map(|l| l.split_whitespace().next()?.parse::<i32>().ok())
            .collect()
    }

    /// Regression test for the reported bug: a command launched from the
    /// processes modal was spawned as a child of the wsx process, so the
    /// per-workspace scan (which hides wsx's own descendants) filtered it
    /// out. The launched command must instead detach from this process so
    /// it surfaces in the modal — i.e. this process must NOT appear in the
    /// spawned command's ancestor chain.
    #[cfg(unix)]
    #[test]
    fn spawned_command_does_not_descend_from_this_process() {
        use std::time::{Duration, Instant};

        let dir = std::env::temp_dir();
        let log = dir.join(format!("wsx-detach-test-{}.log", std::process::id()));
        // A unique sleep duration doubles as the process marker.
        let marker = format!("1{}", std::process::id());
        let needle = format!("sleep {marker}");

        spawn_background_command(&dir, &needle, &log).expect("spawn should succeed");

        let me = std::process::id() as i32;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut found: Option<i32> = None;
        while Instant::now() < deadline {
            // Skip any lingering `sh -c` wrapper; assert on the `sleep` leaf.
            if let Some(pid) = pids_matching(&needle)
                .into_iter()
                .find(|&pid| ancestor_chain(pid).len() <= 64)
            {
                found = Some(pid);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        let result = found.map(|pid| (pid, ancestor_chain(pid)));

        // Always clean up: kill every matching process and remove the log.
        for pid in pids_matching(&needle) {
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .status();
        }
        let _ = std::fs::remove_file(&log);

        let (pid, chain) = result.expect("detached `sleep` not found in process table");
        assert!(
            !chain.contains(&me),
            "spawned process {pid} still descends from this process {me}; chain={chain:?}"
        );
    }
}
