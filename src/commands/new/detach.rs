//! Running the `wsx new` worker so it outlives the terminal that asked, and
//! following its output from that terminal.

use crate::error::Result;
use std::ffi::{OsStr, OsString};
use std::io::Write;
use std::path::Path;
use std::process::Child;

pub(crate) type Env = Vec<(OsString, OsString)>;

/// `vars` as the worker should see them. `gui`: for a job in the desktop
/// session, which has its own ssh agent.
pub(crate) fn worker_env(vars: impl Iterator<Item = (OsString, OsString)>, gui: bool) -> Env {
    // An agent running `wsx new` must not pass its identity on.
    const AGENT: [&str; 3] = [
        "CLAUDECODE",
        "CLAUDE_CODE_SESSION_ID",
        "CLAUDE_CODE_ENTRYPOINT",
    ];
    let mut env: Env = vars
        .filter(|(k, v)| {
            let key = k.to_string_lossy();
            let bad_term = key == "TERM" && (v.is_empty() || v == "dumb");
            !(bad_term || AGENT.contains(&&*key) || (gui && key.starts_with("SSH_")))
        })
        .collect();
    // The tmux client refuses to start without one.
    if !env.iter().any(|(k, _)| k == "TERM") {
        env.push(("TERM".into(), "xterm-256color".into()));
    }
    env
}

pub(crate) fn spawn(exe: &Path, args: &[OsString], env: &Env, log: &Path) -> Result<Child> {
    use std::os::unix::process::CommandExt;
    let out = private_file(log)?;
    let err = out.try_clone()?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(std::process::Stdio::null())
        .stdout(out)
        .stderr(err);
    // Safety: setsid is async-signal-safe and touches no shared state.
    unsafe {
        cmd.pre_exec(|| match libc::setsid() {
            -1 => Err(std::io::Error::last_os_error()),
            _ => Ok(()),
        });
    }
    Ok(cmd.spawn()?)
}

/// Created readable by this user only: logs and jobs carry the task.
pub(crate) fn private_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?)
}

fn xml(s: &OsStr) -> String {
    s.to_string_lossy()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// A one-off launchd job: `argv` with `env`, output to `log`.
pub(crate) fn plist(label: &str, argv: &[&OsStr], env: &Env, log: &Path) -> String {
    let string = |s: &OsStr| format!("<string>{}</string>", xml(s));
    let args: String = argv.iter().map(|a| string(a)).collect();
    let vars: String = env
        .iter()
        .map(|(k, v)| format!("<key>{}</key>{}", xml(k), string(v)))
        .collect();
    let log = string(log.as_os_str());
    // AbandonProcessGroup: the agent must survive the job's removal.
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <plist version=\"1.0\"><dict>\
         <key>Label</key>{}\
         <key>ProgramArguments</key><array>{args}</array>\
         <key>EnvironmentVariables</key><dict>{vars}</dict>\
         <key>RunAtLoad</key><true/>\
         <key>AbandonProcessGroup</key><true/>\
         <key>StandardOutPath</key>{log}\
         <key>StandardErrorPath</key>{log}\
         </dict></plist>\n",
        string(label.as_ref())
    )
}

fn launchctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("/bin/launchctl")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn gui_domain() -> String {
    format!("gui/{}", unsafe { libc::getuid() })
}

/// Whether this process is outside the desktop session (an ssh login), where
/// macOS keeps Claude's Keychain login out of reach.
pub(crate) fn outside_desktop() -> bool {
    cfg!(target_os = "macos")
        && launchctl(&["managername"]).is_some_and(|name| name.trim() != "Aqua")
}

/// Run `argv` as a one-off job in the desktop session. False when launchd
/// refuses, as it does when nobody is logged in there.
pub(crate) fn spawn_in_gui(label: &str, argv: &[&OsStr], env: &Env, log: &Path) -> Result<bool> {
    private_file(log)?;
    let file = log.with_extension("plist");
    private_file(&file)?.write_all(plist(label, argv, env, log).as_bytes())?;
    let ok = launchctl(&["bootstrap", &gui_domain(), &file.to_string_lossy()]).is_some();
    let _ = std::fs::remove_file(&file);
    Ok(ok)
}

/// Whether the job is still running, not merely loaded.
pub(crate) fn job_running(label: &str) -> bool {
    launchctl(&["list", label]).is_some_and(|s| s.contains("\"PID\""))
}

/// Remove the job. Ends the caller when it is that job.
pub(crate) fn remove_job(label: &str) {
    launchctl(&["bootout", &format!("{}/{label}", gui_domain())]);
}

/// Copy `log` to `out` as it grows; returns the code in `exit_file`, or 1
/// once the worker is gone without writing one.
pub(crate) fn follow(
    log: &Path,
    exit_file: &Path,
    alive: &mut dyn FnMut() -> bool,
    out: &mut dyn Write,
) -> Result<i32> {
    let mut file = std::fs::File::open(log)?;
    loop {
        // Both read before the copy: whatever they saw happened after the
        // worker's last output, so the copy below cannot miss any.
        let running = alive();
        let code = std::fs::read_to_string(exit_file)
            .ok()
            .and_then(|s| s.trim().parse().ok());
        std::io::copy(&mut file, out)?;
        out.flush()?;
        if let Some(code) = code {
            return Ok(code);
        }
        if !running {
            writeln!(out, "wsx new stopped early; its log is {}", log.display())?;
            return Ok(1);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn os(s: &str) -> OsString {
        s.into()
    }

    fn sh(script: &str, log: &Path) -> Child {
        let env = worker_env(std::env::vars_os(), false);
        spawn(Path::new("/bin/sh"), &[os("-c"), os(script)], &env, log).unwrap()
    }

    fn env_of(vars: &[(&str, &str)], gui: bool) -> Vec<(String, String)> {
        let it = vars.iter().map(|(k, v)| (os(k), os(v)));
        worker_env(it, gui)
            .into_iter()
            .map(|(k, v)| (k.into_string().unwrap(), v.into_string().unwrap()))
            .collect()
    }

    #[test]
    fn the_worker_gets_a_usable_term() {
        let term = |vars: &[(&str, &str)]| {
            let got = env_of(vars, false);
            let mut t = got
                .iter()
                .filter(|(k, _)| k == "TERM")
                .map(|(_, v)| v.clone());
            (t.next(), t.next())
        };
        let want = |s: &str| (Some(s.to_string()), None);
        assert_eq!(term(&[]), want("xterm-256color"));
        assert_eq!(term(&[("TERM", "dumb")]), want("xterm-256color"));
        assert_eq!(term(&[("TERM", "")]), want("xterm-256color"));
        assert_eq!(term(&[("TERM", "screen")]), want("screen"));
    }

    #[test]
    fn a_launching_agents_identity_is_dropped() {
        let vars = [
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_SESSION_ID", "x"),
            ("CLAUDE_CODE_ENTRYPOINT", "cli"),
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("PATH", "/bin"),
        ];
        let keys: Vec<_> = env_of(&vars, false).into_iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["CLAUDE_CODE_USE_BEDROCK", "PATH", "TERM"]);
    }

    #[test]
    fn ssh_variables_stay_out_of_the_desktop_session_only() {
        let vars = [
            ("SSH_AUTH_SOCK", "/s"),
            ("SSH_CONNECTION", "c"),
            ("TERM", "x"),
        ];
        assert_eq!(env_of(&vars, true).len(), 1);
        assert_eq!(env_of(&vars, false).len(), 3);
    }

    #[test]
    fn the_worker_leads_its_own_session() {
        let tmp = tempfile::tempdir().unwrap();
        let mut c = sh("sleep 5", &tmp.path().join("log"));
        let sid = unsafe { libc::getsid(c.id() as i32) };
        assert_eq!(sid, c.id() as i32);
        c.kill().unwrap();
        c.wait().unwrap();
    }

    #[test]
    fn follow_copies_everything_and_returns_the_code() {
        for code in [0, 1] {
            let tmp = tempfile::tempdir().unwrap();
            let (log, exit) = (tmp.path().join("log"), tmp.path().join("exit"));
            let mut c = sh(
                &format!(
                    "printf 'a\\377b\\n'; sleep 0.3; echo err >&2; echo {code} > '{}'",
                    exit.display()
                ),
                &log,
            );
            let mut out = vec![];
            let mut alive = || c.try_wait().unwrap().is_none();
            assert_eq!(follow(&log, &exit, &mut alive, &mut out).unwrap(), code);
            assert_eq!(out, b"a\xffb\nerr\n");
        }
    }

    #[test]
    fn follow_returns_while_the_worker_still_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let (log, exit) = (tmp.path().join("log"), tmp.path().join("exit"));
        let mut c = sh(&format!("echo 1 > '{}'; sleep 20", exit.display()), &log);
        let t = Instant::now();
        assert_eq!(follow(&log, &exit, &mut || true, &mut vec![]).unwrap(), 1);
        assert!(t.elapsed() < Duration::from_secs(5));
        c.kill().unwrap();
        c.wait().unwrap();
    }

    #[test]
    fn a_worker_that_dies_silently_is_a_failure_naming_its_log() {
        let tmp = tempfile::tempdir().unwrap();
        let (log, exit) = (tmp.path().join("log"), tmp.path().join("exit"));
        let mut c = sh("echo partial; kill -9 $$", &log);
        let mut out = vec![];
        let mut alive = || c.try_wait().unwrap().is_none();
        assert_eq!(follow(&log, &exit, &mut alive, &mut out).unwrap(), 1);
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("partial\n"), "{out}");
        assert!(out.contains(&log.display().to_string()), "{out}");
    }

    #[test]
    fn plist_escapes_and_holds_the_job() {
        let env = vec![(os("K"), os("a<b>&\"c\""))];
        let argv = [os("/bin/w&x"), os("--worker")];
        let argv: Vec<&OsStr> = argv.iter().map(|a| a.as_os_str()).collect();
        let p = plist("wsx.new.1", &argv, &env, Path::new("/l&g"));
        assert!(p.contains("<string>/bin/w&amp;x</string>"), "{p}");
        assert!(p.contains("a&lt;b&gt;&amp;"), "{p}");
        assert!(p.contains("<key>AbandonProcessGroup</key><true/>"), "{p}");
        assert!(p.contains("<key>RunAtLoad</key><true/>"), "{p}");
        #[cfg(target_os = "macos")]
        {
            let tmp = tempfile::tempdir().unwrap();
            let f = tmp.path().join("j.plist");
            std::fs::write(&f, p).unwrap();
            let ok = std::process::Command::new("plutil")
                .arg("-lint")
                .arg(&f)
                .output();
            assert!(ok.unwrap().status.success());
        }
    }
}
