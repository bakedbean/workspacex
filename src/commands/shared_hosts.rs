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

/// Run `ssh <dest> sh -lc 'wsx shared list --json'` and parse the result.
/// Login shell so PATH resolves wsx on the host. Non-zero exit maps to a
/// user-facing error carrying the captured stderr (spec: failure handling).
pub async fn fetch_shared_list(
    dest: &str,
) -> crate::error::Result<Vec<crate::commands::shared::SharedWorkspaceRecord>> {
    let out = tokio::process::Command::new(ssh_bin())
        .args([dest, "sh", "-lc", "wsx shared list --json"])
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
        let ok = dir.path().join("fake-ssh-ok.sh");
        std::fs::write(&ok, "#!/bin/sh\necho '[{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"agents\":[]}]'\n").unwrap();
        std::fs::set_permissions(&ok, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        env.set("WSX_SSH_BIN", ok.to_str().unwrap());
        let recs = fetch_shared_list("mini").await.unwrap();
        assert_eq!(recs[0].workspace, "w");

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
