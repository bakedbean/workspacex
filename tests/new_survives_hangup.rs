//! `wsx new` must finish, and deliver its task once, after the terminal that
//! asked for it hangs up mid-setup.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

struct Sandbox(PathBuf);

impl Sandbox {
    fn cmd(&self, bin: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut c = Command::new(bin);
        c.env("HOME", &self.0)
            .env("XDG_STATE_HOME", self.0.join("state"))
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("TMUX_TMPDIR", &self.0)
            .env("WSX_CLAUDE_BIN", self.0.join("agent.sh"))
            .env_remove("TMUX")
            .env_remove("WSX_WORKSPACE_ID")
            .env_remove("WSX_AGENT_INSTANCE_ID")
            .current_dir(&self.0);
        c
    }
    fn wsx(&self, args: &[&str]) {
        let out = self
            .cmd(env!("CARGO_BIN_EXE_wsx"))
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = self.cmd("tmux").arg("kill-server").output();
    }
}

fn until(what: &str, secs: u64, f: impl Fn() -> bool) {
    for _ in 0..secs * 10 {
        if f() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

fn script(path: &Path, body: &str) {
    std::fs::write(path, body).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_task_arrives_once_after_the_terminal_hangs_up() {
    if Command::new("tmux").arg("-V").output().is_err() {
        eprintln!("tmux not installed; skipping");
        return;
    }
    // Over ssh on macOS this would load a real launchd job.
    if cfg!(target_os = "macos") {
        let name = Command::new("launchctl")
            .arg("managername")
            .output()
            .unwrap();
        if name.stdout.trim_ascii() != b"Aqua" {
            eprintln!("not in the desktop session; skipping");
            return;
        }
    }
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().canonicalize().unwrap();
    let sb = Sandbox(dir.clone());
    let (typed, setting_up) = (dir.join("typed"), dir.join("setting-up"));
    script(
        &dir.join("agent.sh"),
        &format!(
            "#!/bin/sh\nprintf '\\033[?1049h\\033[2J> '\nwhile read -r l; do echo \"$l\" >> '{}'; done\n",
            typed.display()
        ),
    );
    let repo = dir.join("repo");
    std::fs::create_dir(&repo).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "x",
        ][..],
    ] {
        assert!(
            sb.cmd("git")
                .args(args)
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
    }
    sb.wsx(&["repo", "add", repo.to_str().unwrap(), "--name", "r"]);
    sb.wsx(&["config", "set", "remote_control", "off"]);
    let setup = format!("touch '{}'; sleep 2", setting_up.display());
    sb.wsx(&["repo", "set-setup", "r", &setup]);

    let mut cmd = sb.cmd(env!("CARGO_BIN_EXE_wsx"));
    cmd.args(["new", "r", "do", "the", "thing"]);
    // Its own process group, as a login shell's foreground job has.
    cmd.process_group(0);
    let mut follower = cmd.spawn().unwrap();
    until("setup to start", 20, || setting_up.exists());
    assert_eq!(
        unsafe { libc::kill(-(follower.id() as i32), libc::SIGHUP) },
        0
    );
    assert!(
        !follower.wait().unwrap().success(),
        "the hangup ends the follower"
    );
    assert!(!typed.exists(), "still setting up");

    let got = || std::fs::read_to_string(&typed).unwrap_or_default();
    until("the task", 40, || got().contains("do the thing"));
    let logs = dir.join("state/wsx/logs/new");
    let has = |ext: &str| {
        std::fs::read_dir(&logs)
            .unwrap()
            .flatten()
            .any(|e| e.path().extension().is_some_and(|x| x == ext))
    };
    until("the worker to finish", 20, || has("exit"));
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(got().matches("do the thing").count(), 1, "{:?}", got());
    assert!(!has("task"), "a delivered task is not kept");
}
