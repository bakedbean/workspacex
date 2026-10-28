//! Run a newly installed `claude` once in the background, so macOS's
//! first-launch scan of the ~270 MB binary (2s+) happens before the user
//! attaches, not while the pane sits blank. Claude Code updates almost
//! daily, swapping the `claude` symlink to a new version file.

use super::App;
use crate::pty::AgentKind;
use std::path::{Path, PathBuf};

/// Ticks between checks: 480 × 125 ms = 60 s. Checked on tick 1 too, so a
/// version installed while wsx was closed is warmed at startup.
const CHECK_EVERY_TICKS: u32 = 480;

/// The binary to warm: `located` with symlinks resolved, unless it is the
/// one already warmed. Resolving matters because the update only repoints
/// the symlink; the name on `PATH` stays the same.
fn prewarm_target(located: Option<PathBuf>, warmed: Option<&Path>) -> Option<PathBuf> {
    let real = std::fs::canonicalize(located?).ok()?;
    (warmed != Some(real.as_path())).then_some(real)
}

impl App {
    /// Called every tick; once a minute, warms the claude binary if it has
    /// changed since the last warm. macOS only: that is where the scan is.
    pub fn maybe_prewarm_claude(&mut self) {
        if !cfg!(target_os = "macos") || self.tick % CHECK_EVERY_TICKS != 1 {
            return;
        }
        let located = crate::pty::session::locate_agent_binary(AgentKind::Claude);
        let Some(target) = prewarm_target(located, self.prewarmed_claude.as_deref()) else {
            return;
        };
        self.prewarmed_claude = Some(target.clone());
        // `--version` exits before any session, config or network work. The
        // thread waits on it so the child is reaped.
        std::thread::spawn(move || {
            let _ = std::process::Command::new(target)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warms_the_symlink_target_and_not_again_until_it_changes() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = dir.path().join("1.0.0");
        let v2 = dir.path().join("1.0.1");
        std::fs::write(&v1, "").unwrap();
        std::fs::write(&v2, "").unwrap();
        let link = dir.path().join("claude");
        std::os::unix::fs::symlink(&v1, &link).unwrap();
        let v1 = std::fs::canonicalize(&v1).unwrap();
        let v2 = std::fs::canonicalize(&v2).unwrap();

        assert_eq!(prewarm_target(Some(link.clone()), None), Some(v1.clone()));
        assert_eq!(prewarm_target(Some(link.clone()), Some(&v1)), None);

        std::fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&v2, &link).unwrap();
        assert_eq!(prewarm_target(Some(link), Some(&v1)), Some(v2));
    }

    #[test]
    fn nothing_to_warm_when_the_binary_is_missing() {
        assert_eq!(prewarm_target(None, None), None);
        let gone = PathBuf::from("/nonexistent/wsx-prewarm/claude");
        assert_eq!(prewarm_target(Some(gone), None), None);
    }
}
