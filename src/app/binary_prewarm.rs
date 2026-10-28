//! Run a newly installed `claude` once in the background, so macOS's
//! first-launch scan of the ~270 MB binary (2s+) happens before the user
//! attaches, not while the pane sits blank. Claude Code updates almost
//! daily, swapping the `claude` symlink to a new version file.

use super::App;
use crate::pty::AgentKind;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Ticks between checks: 480 × 125 ms = 60 s. Checked on tick 1 too, so a
/// version installed while wsx was closed is warmed at startup.
const CHECK_EVERY_TICKS: u32 = 480;
/// Longest a warm-up run may take before it is killed: generous for a scan
/// on a loaded machine, finite for a wrapper that ignores `--version`.
const RUN_TIMEOUT: Duration = Duration::from_secs(60);

/// Warm-up state shared with the worker thread, which does all the
/// filesystem and process work so a slow `PATH` entry never stalls the UI.
#[derive(Default)]
pub struct ClaudePrewarm {
    /// The resolved binary last launched successfully.
    warmed: Mutex<Option<PathBuf>>,
    /// A worker is running; later checks skip rather than queue.
    busy: AtomicBool,
}

impl ClaudePrewarm {
    fn run(&self, located: Option<PathBuf>) {
        let mut warmed = self.warmed.lock().unwrap();
        let Some(target) = prewarm_target(located, warmed.as_deref()) else {
            return;
        };
        // Recorded only once launched, so a failed spawn is retried next check.
        let Ok(child) = Command::new(&target)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return;
        };
        *warmed = Some(target);
        drop(warmed);
        wait_or_kill(child, RUN_TIMEOUT);
    }
}

/// The binary to warm: `located` with symlinks resolved, unless it is the
/// one already warmed. Resolving matters because the update only repoints
/// the symlink; the name on `PATH` stays the same.
fn prewarm_target(located: Option<PathBuf>, warmed: Option<&Path>) -> Option<PathBuf> {
    let real = std::fs::canonicalize(located?).ok()?;
    (warmed != Some(real.as_path())).then_some(real)
}

/// Reap `child`, killing it first if it outlives `timeout`.
fn wait_or_kill(mut child: Child, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Ok(Some(_)) | Err(_) => return,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

impl App {
    /// Called every tick; once a minute, hands a warm-up check to a worker
    /// thread. macOS only: that is where the scan is.
    pub fn maybe_prewarm_claude(&mut self) {
        if !cfg!(target_os = "macos") || self.tick % CHECK_EVERY_TICKS != 1 {
            return;
        }
        if self.prewarmed_claude.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let state = self.prewarmed_claude.clone();
        std::thread::spawn(move || {
            state.run(crate::pty::session::locate_agent_binary(AgentKind::Claude));
            state.busy.store(false, Ordering::Release);
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

    /// An executable script that appends a line to `log` per run.
    fn recorder(dir: &Path, log: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("claude");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\necho \"$1\" >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    #[test]
    fn runs_the_binary_with_version_once_per_target() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let bin = recorder(dir.path(), &log);
        let state = ClaudePrewarm::default();

        state.run(Some(bin.clone()));
        state.run(Some(bin));

        assert_eq!(std::fs::read_to_string(&log).unwrap(), "--version\n");
    }

    #[test]
    fn a_failed_launch_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let bin = dir.path().join("claude");
        std::fs::write(&bin, "").unwrap(); // not executable: spawn fails
        let state = ClaudePrewarm::default();

        state.run(Some(bin.clone()));
        assert!(state.warmed.lock().unwrap().is_none());

        recorder(dir.path(), &log);
        state.run(Some(bin));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "--version\n");
    }

    #[test]
    fn a_hung_run_is_killed_at_the_timeout() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let started = Instant::now();
        wait_or_kill(child, Duration::from_millis(200));
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
