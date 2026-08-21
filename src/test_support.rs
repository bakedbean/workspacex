//! Path helpers and env-var serialization for tests.
//!
//! **Path helpers.** macOS and Linux disagree on where `cat`/`true`/`false`
//! live — `cat` is in `/bin` on macOS but `/usr/bin` on Linux, and
//! `true`/`false` are mirrored. `cat_path()` etc. probe both layouts and
//! fall through to the bare command name when neither exists.
//!
//! **`ENV_LOCK` + `EnvGuard`.** Several tests across the crate mutate
//! process-global env vars (`WSX_CLAUDE_BIN`, `HOME`, `EDITOR`). Without
//! synchronization they race when cargo runs test modules in parallel.
//! `EnvGuard` is an RAII guard: it acquires the single process-wide
//! `ENV_LOCK`, stashes the previous value of every var it touches, and
//! restores them on drop (even on panic).
//!
//! Public so `tests/smoke.rs` (built as a separate crate) can see it.

use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard};

pub static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Probe absolute paths in order for existence. Returns the first one
/// that exists; otherwise returns the final candidate verbatim (intended
/// to be a bare command name resolvable via PATH). Non-absolute
/// candidates other than the final fallback are not filesystem-probed —
/// otherwise a stray `./cat` in CWD would shadow the real binary.
fn resolve_util(candidates: &[&'static str]) -> &'static str {
    for path in &candidates[..candidates.len().saturating_sub(1)] {
        if std::path::Path::new(path).is_absolute() && std::path::Path::new(path).exists() {
            return path;
        }
    }
    candidates.last().copied().unwrap_or("")
}

pub fn cat_path() -> &'static str {
    resolve_util(&["/bin/cat", "/usr/bin/cat", "cat"])
}

pub fn true_path() -> &'static str {
    resolve_util(&["/usr/bin/true", "/bin/true", "true"])
}

pub fn false_path() -> &'static str {
    resolve_util(&["/usr/bin/false", "/bin/false", "false"])
}

/// Path to an executable wrapper that ignores all CLI arguments and cats
/// stdin. Use in place of `cat_path()` for agent spawns that now inject flags
/// the bare `cat` would reject (e.g. Codex `-c notify=...`).
///
/// The wrapper `exec`s the absolute path resolved by `cat_path()` rather than a
/// bare `cat`, so it doesn't depend on `PATH` (the same macOS/Linux-layout and
/// PATH-mutation concerns `cat_path()` was built to avoid).
///
/// # Why this is written exactly once, to a pid-scoped path
///
/// This used to rewrite a fixed path on every call, on the reasoning that
/// `ENV_LOCK` serialized the writers. That reasoning was wrong, and it made CI
/// fail intermittently on Linux with
/// `Os { code: 26, kind: ExecutableFileBusy }`.
///
/// The race is not writer-versus-writer, which the lock does cover. It is
/// writer-versus-**`execve`**: a test that has already called this spawns a PTY
/// child that executes the script, and that child outlives the `EnvGuard` its
/// spawner dropped. Linux refuses to open a file for writing while it is being
/// executed, so the next test to acquire the lock and rewrite the path can hit
/// `ETXTBSY`. The lock has nothing to say about it — the racing party is a
/// process, not a lock holder. macOS does not enforce this, which is why the
/// failure only ever appeared on the Linux runner.
///
/// Writing once via `OnceLock` removes the rewrite entirely: the single write
/// happens before this process can have exec'd the path. The pid in the file
/// name covers the other half, `cargo test --all-targets` running several test
/// binaries at once — without it, one binary could rewrite the script another
/// was mid-`execve` on.
///
/// The cost is one small file per test-binary run left in the temp dir instead
/// of one file overall. Deleting it would need to outlive the last spawned
/// child, which is exactly the thing that cannot be known here, so it is left
/// for normal temp reaping.
#[cfg(unix)]
pub fn cat_ignore_args_path() -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    use std::sync::OnceLock;
    static WRAPPER: OnceLock<std::path::PathBuf> = OnceLock::new();
    WRAPPER
        .get_or_init(|| {
            let p = std::env::temp_dir().join(format!(
                "wsx_test_cat_ignore_args_{}.sh",
                std::process::id()
            ));
            let script = format!("#!/bin/sh\nexec {}\n", cat_path());
            std::fs::write(&p, script).expect("write wrapper script");
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755))
                .expect("chmod wrapper script");
            p
        })
        .clone()
}

/// RAII guard for env-mutating tests: acquires `ENV_LOCK`, stashes the
/// original value of any env var it sets/removes, and restores them on
/// drop — even on panic — so a failed assertion can't leak stale env
/// into subsequent tests.
pub struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    saved: Vec<(String, Option<OsString>)>,
}

impl EnvGuard {
    pub fn new() -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        Self {
            _lock: lock,
            saved: Vec::new(),
        }
    }

    pub fn set(&mut self, key: &str, value: impl AsRef<OsStr>) {
        self.saved.push((key.to_string(), std::env::var_os(key)));
        unsafe {
            std::env::set_var(key, value);
        }
    }

    pub fn remove(&mut self, key: &str) {
        self.saved.push((key.to_string(), std::env::var_os(key)));
        unsafe {
            std::env::remove_var(key);
        }
    }
}

impl Default for EnvGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, prior) in self.saved.drain(..).rev() {
            unsafe {
                match prior {
                    Some(v) => std::env::set_var(&key, v),
                    None => std::env::remove_var(&key),
                }
            }
        }
    }
}
