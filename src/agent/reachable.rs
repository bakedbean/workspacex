//! Whether a Claude session can be reached from the Claude app, read from
//! the (undocumented) session files Claude Code keeps per process.

use std::path::{Path, PathBuf};

pub fn sessions_dir() -> PathBuf {
    let config = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude"),
    };
    config.join("sessions")
}

pub fn pid_alive(pid: i32) -> bool {
    // 0 and negatives address process groups, not a process.
    pid > 0
        && (unsafe { libc::kill(pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
}

/// A live session in `worktree` has registered with the Claude app.
pub fn bridge_up(dir: &Path, worktree: &Path, pid_alive: impl Fn(i32) -> bool) -> bool {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let worktree = canonical(worktree);
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .filter_map(|p| std::fs::read(p).ok())
        .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .any(|s| {
            let text = |key: &str| s[key].as_str().filter(|v| !v.is_empty());
            text("bridgeSessionId").is_some()
                && text("cwd").is_some_and(|c| canonical(Path::new(c)) == worktree)
                && s["pid"]
                    .as_i64()
                    .and_then(|p| i32::try_from(p).ok())
                    .is_some_and(&pid_alive)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EnvGuard;
    use serde_json::{Value, json};

    struct Fix {
        tmp: tempfile::TempDir,
        dir: PathBuf,
        wt: PathBuf,
    }

    fn fix() -> Fix {
        let tmp = tempfile::tempdir().unwrap();
        let (dir, wt) = (tmp.path().join("sessions"), tmp.path().join("wt"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        Fix { tmp, dir, wt }
    }

    impl Fix {
        fn put(&self, name: &str, v: &Value) {
            std::fs::write(self.dir.join(name), v.to_string()).unwrap();
        }
        fn good(&self) -> Value {
            json!({"pid": 7, "cwd": self.wt, "bridgeSessionId": "session_x"})
        }
        fn up(&self) -> bool {
            bridge_up(&self.dir, &self.wt, |pid| pid == 7)
        }
    }

    #[test]
    fn a_live_session_with_a_bridge_id_is_up() {
        let f = fix();
        assert!(!f.up(), "no files yet");
        f.put("7.json", &f.good());
        assert!(f.up());
    }

    #[test]
    fn anything_short_of_that_is_not() {
        let f = fix();
        for (key, v) in [
            ("bridgeSessionId", Value::Null),
            ("bridgeSessionId", json!("")),
            ("pid", json!(8)),
            ("cwd", json!(f.tmp.path())),
        ] {
            let mut s = f.good();
            s[key] = v.clone();
            f.put("7.json", &s);
            assert!(!f.up(), "{key}={v}");
        }
        let mut s = f.good();
        s.as_object_mut().unwrap().remove("bridgeSessionId");
        f.put("7.json", &s);
        assert!(!f.up(), "key missing");
        std::fs::write(f.dir.join("7.json"), "{oops").unwrap();
        assert!(!f.up(), "malformed");
        assert!(!bridge_up(&f.dir.join("none"), &f.wt, |_| true));
    }

    #[test]
    fn a_malformed_or_unrelated_file_does_not_hide_a_good_one() {
        let f = fix();
        std::fs::write(f.dir.join("1.json"), "{oops").unwrap();
        f.put("2.json", &json!({"pid": 7, "cwd": "/elsewhere"}));
        f.put("7.json", &f.good());
        assert!(f.up());
    }

    #[test]
    fn only_json_files_count() {
        let f = fix();
        f.put("7.key", &f.good());
        assert!(!f.up());
    }

    #[test]
    fn a_symlinked_cwd_matches() {
        let f = fix();
        let link = f.tmp.path().join("link");
        std::os::unix::fs::symlink(&f.wt, &link).unwrap();
        let mut s = f.good();
        s["cwd"] = json!(link);
        f.put("7.json", &s);
        assert!(f.up());
    }

    #[test]
    fn pid_alive_knows_self_and_rejects_nonsense() {
        assert!(pid_alive(std::process::id() as i32));
        assert!(pid_alive(1), "EPERM counts as alive");
        assert!(!pid_alive(0));
        assert!(!pid_alive(-1));
        assert!(!pid_alive(i32::MAX));
    }

    #[test]
    fn sessions_dir_follows_the_config_dir() {
        let mut env = EnvGuard::new();
        env.set("CLAUDE_CONFIG_DIR", "/cfg");
        assert_eq!(sessions_dir(), Path::new("/cfg/sessions"));
        env.remove("CLAUDE_CONFIG_DIR");
        assert!(sessions_dir().ends_with(".claude/sessions"));
    }
}
