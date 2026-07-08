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

/// Run `ssh <dest> "sh -lc 'wsx shared list --json'"` and parse the result.
/// Login shell so PATH resolves wsx on the host. Non-zero exit maps to a
/// user-facing error carrying the captured stderr (spec: failure handling).
///
/// The remote command is ONE pre-quoted argument, not four words, because ssh
/// joins the remote-command argv with single spaces and hands the result to the
/// host's login shell as `$SHELL -c "<joined>"`. Passing
/// `[dest, "sh", "-lc", "wsx shared list --json"]` would join to
/// `sh -lc wsx shared list --json`, which the login shell re-parses as
/// `sh -l -c wsx` with `shared`/`list`/`--json` as `$0`/`$1`/`$2` — running a
/// BARE `wsx` (which tries to start the TUI). Keeping the inner
/// `sh -lc 'wsx shared list --json'` as a single argument makes the quoting
/// survive the join so the host runs `wsx shared list --json`.
pub async fn fetch_shared_list(
    dest: &str,
) -> crate::error::Result<Vec<crate::commands::shared::SharedWorkspaceRecord>> {
    let out = tokio::process::Command::new(ssh_bin())
        .args([dest, "sh -lc 'wsx shared list --json'"])
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
    parse_shared_list_output(&String::from_utf8_lossy(&out.stdout))
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
        let recs = fetch_shared_list("mini").await.unwrap();
        assert_eq!(recs[0].workspace, "w");

        // Pin the argv shape. `printf '%s\n' "$@"` prints one argument per line,
        // so the remote command being a SINGLE argument means the whole
        // `sh -lc 'wsx shared list --json'` string appears on one line verbatim.
        // ssh joins remote-command words with spaces before handing them to the
        // host login shell, so if this were four words the host would run a bare
        // `wsx`; keeping it one pre-quoted arg preserves the quoting across the
        // join. See `fetch_shared_list`'s doc comment.
        let argv = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<&str> = argv.lines().collect();
        assert_eq!(lines[0], "mini", "dest is the first argument");
        assert_eq!(
            lines[1], "sh -lc 'wsx shared list --json'",
            "remote command must be ONE pre-quoted argument, got argv: {argv:?}"
        );
        assert_eq!(
            lines.len(),
            2,
            "exactly dest + one remote-command arg; 'shared'/'list' must not \
             appear as separate top-level words: {argv:?}"
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
}
