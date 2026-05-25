//! Path helpers for tests that spawn small POSIX utilities (`cat`, `true`,
//! `false`). macOS and Linux disagree on where these live — `cat` is in
//! `/bin` on macOS but `/usr/bin` on Linux, and `true`/`false` are mirrored.
//! Tests previously hardcoded one layout and failed on the other.
//!
//! Also exposes a single process-wide `ENV_LOCK` so any test in any module
//! that mutates process-global env vars (notably `WSX_CLAUDE_BIN`) can
//! serialize against every other env-mutating test. A per-module mutex
//! isn't enough — cargo runs tests across modules in parallel, and one
//! module's `unsafe { set_var }` will clobber another module's setup.

use std::sync::Mutex;

pub static ENV_LOCK: Mutex<()> = Mutex::new(());

fn first_existing(candidates: &[&'static str]) -> &'static str {
    for path in candidates {
        if std::path::Path::new(path).exists() {
            return path;
        }
    }
    candidates.last().copied().unwrap_or("")
}

pub fn cat_path() -> &'static str {
    first_existing(&["/bin/cat", "/usr/bin/cat", "cat"])
}

pub fn true_path() -> &'static str {
    first_existing(&["/usr/bin/true", "/bin/true", "true"])
}

pub fn false_path() -> &'static str {
    first_existing(&["/usr/bin/false", "/bin/false", "false"])
}
