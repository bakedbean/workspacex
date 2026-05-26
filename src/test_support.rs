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
