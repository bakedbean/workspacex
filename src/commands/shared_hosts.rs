//! SSH destinations for accessing shared workspaces on remote hosts.
//! Stored as a newline-separated `name=ssh-destination` blob in the
//! `shared_hosts` setting (e.g. `mini=eben@ebenmini.local`).
//! Unlike `remotes`, which are shell commands, shared hosts are
//! ssh destinations to be used for remote workspace browsing.

use crate::data::store::Store;
use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedHost {
    pub name: String,
    pub dest: String,
}

pub fn parse(text: &str) -> Vec<SharedHost> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() {
                return None;
            }
            let (name, dest) = match line.split_once('=') {
                Some((lhs, rhs)) => (lhs.trim().to_string(), rhs.trim().to_string()),
                None => return None, // Lines without '=' are invalid for shared_hosts
            };
            if name.is_empty() || dest.is_empty() {
                return None;
            }
            // Reject a dest that begins with '-': ssh would parse it as an
            // option (e.g. `-oProxyCommand=…`) rather than a destination, so
            // dropping it here keeps a malformed/hostile config from smuggling
            // ssh flags into the fetch/attach argv.
            if dest.starts_with('-') {
                return None;
            }
            Some(SharedHost { name, dest })
        })
        .collect()
}

/// Returns all configured shared hosts, alphabetized by name.
pub fn list(store: &Store) -> Result<Vec<SharedHost>> {
    let raw = store.get_setting("shared_hosts")?.unwrap_or_default();
    let mut out = parse(&raw);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Returns the SharedHost for `name`, or `None` if no shared host with that
/// name is configured. When the blob contains duplicate names, the
/// last one wins (matches the order of the underlying blob).
pub fn lookup(store: &Store, name: &str) -> Result<Option<SharedHost>> {
    let raw = store.get_setting("shared_hosts")?.unwrap_or_default();
    Ok(parse(&raw).into_iter().rev().find(|h| h.name == name))
}

pub fn ssh_bin() -> String {
    std::env::var("WSX_SSH_BIN").unwrap_or_else(|_| "ssh".to_string())
}

pub fn parse_shared_list_output(
    stdout: &str,
) -> crate::error::Result<Vec<crate::commands::shared::SharedWorkspaceRecord>> {
    serde_json::from_str(stdout)
        .map_err(|e| crate::error::Error::UserInput(format!("bad shared-list JSON from host: {e}")))
}

/// What a host's `shared list` returned, and whether the host understood
/// `--all` — i.e. runs a wsx new enough to list unshared workspaces and to
/// take `workspace share --restart`.
#[derive(Debug, Clone)]
pub struct SharedListing {
    pub records: Vec<crate::commands::shared::SharedWorkspaceRecord>,
    pub can_share: bool,
}

/// Run `ssh <dest> "sh -lc 'wsx shared list --json --all'"` and parse the
/// result, falling back to plain `--json` (shared workspaces only, no
/// remote sharing) when the host's wsx predates `--all`. Login shell so PATH
/// resolves wsx on the host. Non-zero exit maps to a user-facing error
/// carrying the captured stderr (spec: failure handling).
pub async fn fetch_shared_list(dest: &str) -> crate::error::Result<SharedListing> {
    match run_remote_wsx(dest, "shared list --json --all").await {
        Ok(stdout) => Ok(SharedListing {
            records: parse_shared_list_output(&stdout)?,
            can_share: true,
        }),
        Err(e) if e.to_string().contains("unknown arg: --all") => {
            let stdout = run_remote_wsx(dest, "shared list --json").await?;
            Ok(SharedListing {
                records: parse_shared_list_output(&stdout)?,
                can_share: false,
            })
        }
        Err(e) => Err(e),
    }
}

/// Run `wsx workspace share <repo> <workspace> --restart --json` on the host:
/// its dashboard shares the workspace and restarts the agents inside tmux,
/// and the command returns once their sessions are live. Returns the
/// workspace's fresh record, carrying the session names to attach to.
pub async fn share_remote(
    dest: &str,
    repo: &str,
    workspace: &str,
) -> crate::error::Result<crate::commands::shared::SharedWorkspaceRecord> {
    for name in [repo, workspace] {
        if !shell_safe(name) {
            return Err(crate::error::Error::UserInput(format!(
                "can't share {name:?} remotely: the name has characters that \
                 would need shell quoting"
            )));
        }
    }
    let stdout = run_remote_wsx(
        dest,
        &format!("workspace share {repo} {workspace} --restart --json"),
    )
    .await?;
    serde_json::from_str(&stdout).map_err(|e| {
        crate::error::Error::UserInput(format!("bad share-result JSON from host: {e}"))
    })
}

/// Names travel inside the nested `sh -lc '…'` quoting, so only characters
/// no shell layer treats specially are let through. Repo and workspace names
/// are slugs in practice; anything else is refused rather than escaped.
fn shell_safe(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+' | '@'))
}

/// Run `ssh <dest> "sh -lc 'wsx <args>'"` and return its stdout.
///
/// The remote command is ONE pre-quoted argument, not several words, because
/// ssh joins the remote-command argv with single spaces and hands the result
/// to the host's login shell as `$SHELL -c "<joined>"`. Passing
/// `[dest, "sh", "-lc", "wsx shared list --json"]` would join to
/// `sh -lc wsx shared list --json`, which the login shell re-parses as
/// `sh -l -c wsx` with `shared`/`list`/`--json` as `$0`/`$1`/`$2` — running a
/// BARE `wsx` (which tries to start the TUI). Keeping the inner
/// `sh -lc 'wsx …'` as a single argument makes the quoting survive the join.
/// `args` must therefore hold no single quotes (callers pass literals and
/// `shell_safe` names).
async fn run_remote_wsx(dest: &str, args: &str) -> crate::error::Result<String> {
    let out = tokio::process::Command::new(ssh_bin())
        // `-o BatchMode=yes` keeps this background call off /dev/tty: a missing
        // key or unknown host fails fast to stderr (→ the error modal) instead
        // of blocking on a password / host-key prompt no one can answer.
        // `-o ConnectTimeout=10` bounds a hung TCP connect. Both apply to these
        // calls ONLY — the interactive attach in `app::attach_remote` omits
        // BatchMode on purpose so it can still prompt in a real terminal.
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            dest,
            &format!("sh -lc 'wsx {args}'"),
        ])
        .output()
        .await
        .map_err(|e| crate::error::Error::UserInput(format!("ssh spawn failed: {e}")))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(crate::error::Error::UserInput(format!(
            "ssh {dest}: {}",
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::Store;

    #[test]
    fn parse_splits_on_first_equals_and_skips_blank_and_invalid() {
        let hosts = parse("mini=eben@ebenmini.local\n\nbad-line\nlab=user@lab=box\n");
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].name, "mini");
        assert_eq!(hosts[0].dest, "eben@ebenmini.local");
        // first '=' splits; the rest stays in dest
        assert_eq!(hosts[1].dest, "user@lab=box");
    }

    #[test]
    fn parse_rejects_option_like_dest() {
        // A dest starting with '-' would be read by ssh as an option, not a
        // host, so such entries are dropped entirely.
        let hosts = parse("evil=-oProxyCommand=touch pwned\nok=eben@mini");
        assert_eq!(hosts.len(), 1, "the option-like dest must be dropped");
        assert_eq!(hosts[0].name, "ok");
        assert_eq!(hosts[0].dest, "eben@mini");
    }

    #[test]
    fn list_reads_setting_sorted_and_lookup_is_last_write_wins() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("shared_hosts", "b=host-b\na=host-a\na=host-a2")
            .unwrap();
        let hosts = list(&store).unwrap();
        assert_eq!(hosts[0].name, "a");
        assert_eq!(lookup(&store, "a").unwrap().unwrap().dest, "host-a2");
        assert!(lookup(&store, "zz").unwrap().is_none());
    }

    #[tokio::test]
    async fn fetch_shared_list_parses_fake_ssh_output_and_surfaces_stderr() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        // Fake ssh logs its full argv so we can pin the wire shape: the remote
        // command must survive ssh's space-join as ONE pre-quoted argument.
        let log = dir.path().join("ssh-args.log");
        let ok = dir.path().join("fake-ssh-ok.sh");
        std::fs::write(
            &ok,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > {}\necho '[{{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"agents\":[]}}]'\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&ok, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        env.set("WSX_SSH_BIN", ok.to_str().unwrap());
        let listing = fetch_shared_list("mini").await.unwrap();
        assert_eq!(listing.records[0].workspace, "w");
        assert!(listing.can_share);

        // Pin the argv shape. `printf '%s\n' "$@"` prints one argument per line,
        // so the remote command being a SINGLE argument means the whole
        // `sh -lc 'wsx shared list --json'` string appears on one line verbatim.
        // ssh joins remote-command words with spaces before handing them to the
        // host login shell, so if this were four words the host would run a bare
        // `wsx`; keeping it one pre-quoted arg preserves the quoting across the
        // join. See `fetch_shared_list`'s doc comment.
        let argv = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(
            lines,
            vec![
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=10",
                "mini",
                "sh -lc 'wsx shared list --json --all'",
            ],
            "argv must be the batch-mode options, then dest, then ONE pre-quoted \
             remote-command arg; 'shared'/'list' must not appear as separate \
             top-level words: {argv:?}"
        );

        let bad = dir.path().join("fake-ssh-bad.sh");
        std::fs::write(&bad, "#!/bin/sh\necho 'connection refused' >&2\nexit 255\n").unwrap();
        std::fs::set_permissions(&bad, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", bad.to_str().unwrap());
        let err = fetch_shared_list("mini").await.unwrap_err().to_string();
        assert!(
            err.contains("connection refused"),
            "stderr must reach the error: {err}"
        );
    }

    #[tokio::test]
    async fn fetch_passes_batchmode_and_connect_timeout() {
        // Fake ssh scans its argv for both hardening options and fails with a
        // recognizable message unless BOTH are present, proving the background
        // fetch never blocks on a /dev/tty prompt and bounds a hung connect.
        let dir = tempfile::tempdir().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        let script = dir.path().join("fake-ssh-optcheck.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\n\
             have_batch=0; have_timeout=0\n\
             for a in \"$@\"; do\n\
             \t[ \"$a\" = 'BatchMode=yes' ] && have_batch=1\n\
             \t[ \"$a\" = 'ConnectTimeout=10' ] && have_timeout=1\n\
             done\n\
             if [ \"$have_batch\" = 1 ] && [ \"$have_timeout\" = 1 ]; then\n\
             \techo '[]'\n\
             else\n\
             \techo 'missing batchmode/connect-timeout hardening' >&2\n\
             \texit 255\n\
             fi\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_SSH_BIN", script.to_str().unwrap());
        let listing = fetch_shared_list("mini").await.unwrap();
        assert!(
            listing.records.is_empty(),
            "fetch should succeed (empty list) once both options are present"
        );
    }

    /// Writes an executable fake ssh at `dir/name` running `body`.
    fn fake_ssh(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}")).unwrap();
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        path
    }

    #[tokio::test]
    async fn fetch_falls_back_for_a_host_without_list_all() {
        // An older host rejects `--all` with a usage error; the fetch retries
        // without it and marks the listing as not shareable.
        let dir = tempfile::tempdir().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        let ssh = fake_ssh(
            dir.path(),
            "old-host.sh",
            "case \"$*\" in\n\
             *--all*) echo 'error: unknown arg: --all' >&2; exit 2;;\n\
             *) echo '[]';;\n\
             esac\n",
        );
        env.set("WSX_SSH_BIN", ssh.to_str().unwrap());
        let listing = fetch_shared_list("mini").await.unwrap();
        assert!(!listing.can_share);
        assert!(listing.records.is_empty());
    }

    #[tokio::test]
    async fn share_remote_runs_share_restart_and_parses_the_record() {
        let dir = tempfile::tempdir().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        let log = dir.path().join("args.log");
        let ssh = fake_ssh(
            dir.path(),
            "share.sh",
            &format!(
                "printf '%s\\n' \"$@\" > {}\n\
                 echo '{{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"shared\":true,\"agents\":[{{\"label\":\"claude\",\"agent\":\"claude\",\"tmux_session\":\"wsx-r-w\",\"alive\":true}}]}}'\n",
                log.display()
            ),
        );
        env.set("WSX_SSH_BIN", ssh.to_str().unwrap());

        let rec = share_remote("mini", "r", "w").await.unwrap();

        assert_eq!(rec.agents[0].tmux_session.as_deref(), Some("wsx-r-w"));
        let argv = std::fs::read_to_string(&log).unwrap();
        assert_eq!(
            argv.lines().last(),
            Some("sh -lc 'wsx workspace share r w --restart --json'"),
            "{argv}"
        );
    }

    #[tokio::test]
    async fn share_remote_refuses_names_that_need_quoting() {
        let mut env = crate::test_support::EnvGuard::new();
        env.set("WSX_SSH_BIN", "/nonexistent/ssh-must-not-run");
        for (repo, ws) in [("r", "it's"), ("r;rm", "w"), ("r", "-w"), ("r", "a b")] {
            let err = share_remote("mini", repo, ws)
                .await
                .unwrap_err()
                .to_string();
            assert!(err.contains("shell quoting"), "{repo}/{ws}: {err}");
        }
    }
}
