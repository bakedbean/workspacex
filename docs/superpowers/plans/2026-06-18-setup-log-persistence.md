# Setup Log Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist each workspace-creation's setup-script output to a predictable log file so a failed setup is inspectable after the modal auto-closes.

**Architecture:** A new `src/data/setup_log.rs` module owns the log path and the
`Write`-generic line/header/footer formatting (filesystem-free, unit-testable). A
new private `run_setup_logged` helper in `workspace.rs` composes "open file →
tee each line to the progress sink *and* the log → write footer", and
`create_with_app` calls it in place of its inline `run_setup` block. All log I/O
is best-effort and never affects the create flow. The modal is untouched.

**Tech Stack:** Rust, tokio, `strip-ansi-escapes` (already a dependency),
`tempfile` (already a dev-dependency).

## Global Constraints

- Log location: `~/.local/state/wsx/logs/setup-<repo>-<name>.log`, via the
  existing `crate::config::Dirs::log_dir()`. One file per workspace, truncated
  each run (latest only).
- Filename sanitization: any char outside `[A-Za-z0-9._-]` → `-`, applied to
  both repo and workspace name.
- All log I/O is best-effort: opening, every write, and flush are `let _ = …`.
  A failure to log must never change or abort workspace creation.
- No modal/UX change. The modal still auto-closes on completion. No CLI-help or
  detail-bar surfacing.
- Only the setup-script phase is logged; only when `repo.setup_script` is
  present and non-blank. No setup script → no file.
- Timestamp is epoch seconds via `crate::time::now_secs()` (no date library is a
  dependency); the header labels it `(unix seconds)`.
- Verification before any commit runs the project's full gate (the fmt toolchain
  is pinned): `cargo test`, `cargo clippy --all-targets -- -D warnings`, and
  `mise exec rust@1.95.0 -- cargo fmt --all --check`.

---

### Task 1: `setup_log` module — path + formatting

**Files:**
- Create: `src/data/setup_log.rs`
- Modify: `src/data/mod.rs` (register the module)
- Test: inline `#[cfg(test)] mod tests` in `src/data/setup_log.rs`

**Interfaces:**
- Consumes: `crate::data::setup::{SetupLine, SetupResult}` (existing enums:
  `SetupLine::Stdout(String)` / `SetupLine::Stderr(String)`; `SetupResult::Ok` /
  `SetupResult::Failed { exit_code: i32 }` / `SetupResult::Skipped`).
- Produces (used by Task 2):
  - `pub fn setup_log_path(log_dir: &Path, repo: &str, name: &str) -> PathBuf`
  - `pub fn create(log_dir: &Path, repo: &str, name: &str, worktree: &Path, started_secs: u64) -> Option<BufWriter<File>>`
  - `pub fn write_line(w: &mut impl Write, line: &SetupLine) -> io::Result<()>`
  - `pub fn write_footer(w: &mut impl Write, result: &SetupResult) -> io::Result<()>`

- [ ] **Step 1: Register the module**

In `src/data/mod.rs`, add `setup_log` to the public module list. After the
existing `pub mod setup;` line, insert:

```rust
pub mod setup_log;
```

(Keep the list alphabetical: `setup` then `setup_log` then `store`.)

- [ ] **Step 2: Write the module with its failing tests**

Create `src/data/setup_log.rs`:

```rust
//! Persists a workspace's setup-script output to a log file so a failed setup
//! is inspectable after the creation modal auto-closes. All writing is
//! best-effort — callers ignore I/O errors so logging can never break the
//! create flow. See `data::workspace::run_setup_logged` for the call site.

use crate::data::setup::{SetupLine, SetupResult};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

/// `<log_dir>/setup-<repo>-<name>.log`, with repo/name sanitized to a safe
/// filename. Stable (no timestamp) so a workspace's log is always at the same
/// path; each run truncates it.
pub fn setup_log_path(log_dir: &Path, repo: &str, name: &str) -> PathBuf {
    log_dir.join(format!("setup-{}-{}.log", sanitize(repo), sanitize(name)))
}

/// Replace anything outside `[A-Za-z0-9._-]` with `-` so repo/workspace names
/// (which can contain `/`, spaces, etc.) form a safe single path segment.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Best-effort: create (truncating) the log file under `log_dir` and write the
/// header. Returns `None` if the directory or file can't be created — the
/// caller then simply skips logging.
pub fn create(
    log_dir: &Path,
    repo: &str,
    name: &str,
    worktree: &Path,
    started_secs: u64,
) -> Option<BufWriter<File>> {
    let path = setup_log_path(log_dir, repo, name);
    std::fs::create_dir_all(path.parent()?).ok()?;
    let mut w = BufWriter::new(File::create(&path).ok()?);
    write_header(&mut w, repo, name, worktree, started_secs).ok()?;
    Some(w)
}

fn write_header(
    w: &mut impl Write,
    repo: &str,
    name: &str,
    worktree: &Path,
    started_secs: u64,
) -> io::Result<()> {
    writeln!(w, "=== setup: {repo}/{name} ===")?;
    writeln!(w, "worktree: {}", worktree.display())?;
    writeln!(w, "started:  {started_secs} (unix seconds)")?;
    writeln!(w)
}

/// Write one captured line: ANSI escapes stripped, trailing whitespace trimmed,
/// blank lines skipped (matching the on-screen buffer). `Stderr` lines are
/// prefixed `! ` so a reader can tell the two streams apart.
pub fn write_line(w: &mut impl Write, line: &SetupLine) -> io::Result<()> {
    let (raw, is_err) = match line {
        SetupLine::Stdout(s) => (s, false),
        SetupLine::Stderr(s) => (s, true),
    };
    let clean = strip_ansi_escapes::strip_str(raw);
    let clean = clean.trim_end();
    if clean.is_empty() {
        return Ok(());
    }
    if is_err {
        writeln!(w, "! {clean}")
    } else {
        writeln!(w, "{clean}")
    }
}

/// Write the outcome footer.
pub fn write_footer(w: &mut impl Write, result: &SetupResult) -> io::Result<()> {
    match result {
        SetupResult::Ok => writeln!(w, "\n=== OK ==="),
        SetupResult::Failed { exit_code } => writeln!(w, "\n=== FAILED (exit {exit_code}) ==="),
        SetupResult::Skipped => writeln!(w, "\n=== SKIPPED ==="),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn path_is_stable_and_sanitized() {
        let dir = Path::new("/logs");
        assert_eq!(
            setup_log_path(dir, "myrepo", "foo"),
            PathBuf::from("/logs/setup-myrepo-foo.log")
        );
        // Slashes and spaces in repo/name become `-`.
        assert_eq!(
            setup_log_path(dir, "org/repo", "feat branch"),
            PathBuf::from("/logs/setup-org-repo-feat-branch.log")
        );
    }

    #[test]
    fn write_line_strips_ansi_prefixes_stderr_and_skips_blank() {
        let mut buf = Vec::new();
        write_line(&mut buf, &SetupLine::Stdout("\x1b[32mok\x1b[0m".into())).unwrap();
        write_line(&mut buf, &SetupLine::Stderr("boom".into())).unwrap();
        write_line(&mut buf, &SetupLine::Stdout("   ".into())).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(out, "ok\n! boom\n");
    }

    #[test]
    fn footer_renders_each_outcome() {
        let render = |r: &SetupResult| {
            let mut buf = Vec::new();
            write_footer(&mut buf, r).unwrap();
            String::from_utf8(buf).unwrap()
        };
        assert_eq!(render(&SetupResult::Ok), "\n=== OK ===\n");
        assert_eq!(
            render(&SetupResult::Failed { exit_code: 2 }),
            "\n=== FAILED (exit 2) ===\n"
        );
        assert_eq!(render(&SetupResult::Skipped), "\n=== SKIPPED ===\n");
    }

    #[test]
    fn create_writes_a_file_with_header() {
        let logs = TempDir::new().unwrap();
        let mut w = create(logs.path(), "myrepo", "foo", Path::new("/wt/foo"), 1718722921)
            .expect("log file should be created under a writable temp dir");
        write_line(&mut w, &SetupLine::Stdout("hello".into())).unwrap();
        drop(w); // flush
        let body =
            std::fs::read_to_string(setup_log_path(logs.path(), "myrepo", "foo")).unwrap();
        assert!(body.contains("=== setup: myrepo/foo ==="), "{body}");
        assert!(body.contains("worktree: /wt/foo"), "{body}");
        assert!(body.contains("1718722921 (unix seconds)"), "{body}");
        assert!(body.contains("hello"), "{body}");
    }
}
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wsx setup_log`
Expected: PASS for `path_is_stable_and_sanitized`,
`write_line_strips_ansi_prefixes_stderr_and_skips_blank`,
`footer_renders_each_outcome`, `create_writes_a_file_with_header`.

(If the crate is not named `wsx`, drop `-p wsx`: `cargo test setup_log`.)

- [ ] **Step 4: Run the full gate**

Run:
```bash
cargo test
cargo clippy --all-targets -- -D warnings
mise exec rust@1.95.0 -- cargo fmt --all --check
```
Expected: all pass, no warnings, no formatting diff.

- [ ] **Step 5: Commit**

```bash
git add src/data/setup_log.rs src/data/mod.rs
git commit -m "feat(data): add setup_log module for setup-output persistence

Claude-Session: https://claude.ai/code/session_01BbT6YyjYeydENc9YwCeKif"
```

---

### Task 2: tee setup output to the log during workspace creation

**Files:**
- Modify: `src/data/workspace.rs` (add `run_setup_logged`; rewire the Phase 5
  block in `create_with_app`, currently `src/data/workspace.rs:239-254`)
- Test: extend the existing `#[cfg(test)] mod tests` in
  `src/data/workspace.rs` (starts at line 461; already imports
  `tempfile::TempDir`)

**Interfaces:**
- Consumes (from Task 1): `crate::data::setup_log::{setup_log_path, create, write_line, write_footer}`.
- Produces: `async fn run_setup_logged(...) -> Result<SetupResult>` (private to
  `workspace.rs`); no public surface change. `create_with_app`'s signature and
  return type are unchanged.

- [ ] **Step 1: Write the failing tests**

Add these two tests inside the existing `mod tests` in
`src/data/workspace.rs` (the module already has `use tempfile::TempDir;`; add
the other `use`s shown at the top of the test fns):

```rust
    #[tokio::test]
    async fn run_setup_logged_writes_failure_log() {
        use crate::data::progress::SetupProgress;
        use crate::data::setup::SetupResult;
        use crate::data::setup_log::setup_log_path;
        use tokio_util::sync::CancellationToken;

        let work = TempDir::new().unwrap(); // stands in for repo_root + worktree
        let logs = TempDir::new().unwrap();
        let progress = SetupProgress::shared();
        let script = "echo hello-stdout; echo oops-stderr 1>&2; exit 3";

        let result = run_setup_logged(
            Some(script),
            work.path(),
            work.path(),
            "myrepo",
            "foo",
            logs.path(),
            &progress,
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(matches!(result, SetupResult::Failed { exit_code: 3 }), "{result:?}");
        let body =
            std::fs::read_to_string(setup_log_path(logs.path(), "myrepo", "foo")).unwrap();
        assert!(body.contains("=== setup: myrepo/foo ==="), "{body}");
        assert!(body.contains("hello-stdout"), "{body}");
        assert!(body.contains("! oops-stderr"), "{body}");
        assert!(body.contains("=== FAILED (exit 3) ==="), "{body}");

        // The progress sink is still fed (the modal behavior is unchanged).
        let recent = progress.lock().unwrap().recent(10);
        assert!(recent.iter().any(|l| l.contains("hello-stdout")), "{recent:?}");
    }

    #[tokio::test]
    async fn run_setup_logged_writes_no_file_without_script() {
        use crate::data::progress::SetupProgress;
        use crate::data::setup::SetupResult;
        use crate::data::setup_log::setup_log_path;
        use tokio_util::sync::CancellationToken;

        let work = TempDir::new().unwrap();
        let logs = TempDir::new().unwrap();
        let progress = SetupProgress::shared();

        let result = run_setup_logged(
            None,
            work.path(),
            work.path(),
            "myrepo",
            "bar",
            logs.path(),
            &progress,
            CancellationToken::new(),
        )
        .await
        .unwrap();

        assert!(matches!(result, SetupResult::Skipped), "{result:?}");
        assert!(!setup_log_path(logs.path(), "myrepo", "bar").exists());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p wsx run_setup_logged`
Expected: FAIL to **compile** with "cannot find function `run_setup_logged`".

- [ ] **Step 3: Add the `run_setup_logged` helper**

In `src/data/workspace.rs`, add this private async fn (place it just above
`pub async fn create_with_app`):

```rust
/// Run the setup script while teeing each captured line to two consumers: the
/// live `progress` sink (drives the creation modal) and a best-effort per-
/// workspace log file under `log_dir` (so a failed setup is inspectable after
/// the modal closes). The log is opened only when a setup script is present;
/// all log I/O is best-effort and never affects the returned result. Returns
/// the same `Result<SetupResult>` as `setup::run_setup`.
async fn run_setup_logged(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    repo_name: &str,
    ws_name: &str,
    log_dir: &Path,
    progress: &SharedProgress,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<SetupResult> {
    let mut log = match script {
        Some(s) if !s.trim().is_empty() => crate::data::setup_log::create(
            log_dir,
            repo_name,
            ws_name,
            worktree,
            crate::time::now_secs(),
        ),
        _ => None,
    };
    let log_ref = &mut log;
    let result = setup::run_setup(script, repo_root, worktree, cancel, |line| {
        let text = match &line {
            SetupLine::Stdout(s) | SetupLine::Stderr(s) => s.as_str(),
        };
        if let Ok(mut p) = progress.lock() {
            p.push_line(text);
        }
        if let Some(w) = log_ref.as_mut() {
            let _ = crate::data::setup_log::write_line(w, &line);
        }
    })
    .await?;
    if let Some(mut w) = log {
        let _ = crate::data::setup_log::write_footer(&mut w, &result);
    }
    Ok(result)
}
```

Note: the `?` after `run_setup` propagates `Err(Cancelled)` exactly as the old
inline code did — on cancel the log keeps its header + partial body (flushed
when the `BufWriter` drops) and gets no footer, which is fine.

- [ ] **Step 4: Rewire `create_with_app` to call the helper**

In `src/data/workspace.rs`, replace the Phase 5 block (the
`let progress_lines = progress.clone();` statement through the `.await;` that
ends the `setup::run_setup(...)` call — currently lines 239-254):

```rust
    let progress_lines = progress.clone();
    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        cancel.clone(),
        move |line| {
            let text = match line {
                SetupLine::Stdout(s) | SetupLine::Stderr(s) => s,
            };
            if let Ok(mut p) = progress_lines.lock() {
                p.push_line(&text);
            }
        },
    )
    .await;
```

with:

```rust
    let log_dir = crate::config::Dirs::discover().log_dir();
    let setup_result = run_setup_logged(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        &repo.name,
        &final_name,
        &log_dir,
        &progress,
        cancel.clone(),
    )
    .await;
```

The following `let setup_result = match setup_result { Ok(r) => r, Err(Error::Cancelled) => …, Err(e) => return Err(e) };`
block is unchanged — `run_setup_logged` returns the same `Result<SetupResult>`.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test -p wsx run_setup_logged`
Expected: PASS for `run_setup_logged_writes_failure_log` and
`run_setup_logged_writes_no_file_without_script`.

- [ ] **Step 6: Run the full gate**

Run:
```bash
cargo test
cargo clippy --all-targets -- -D warnings
mise exec rust@1.95.0 -- cargo fmt --all --check
```
Expected: all pass. (Watch for an unused-import warning on `SetupLine` — it is
still used by `run_setup_logged`, so it should remain imported and warning-free.)

- [ ] **Step 7: Commit**

```bash
git add src/data/workspace.rs
git commit -m "feat(data): persist setup output to a log file during workspace creation

Claude-Session: https://claude.ai/code/session_01BbT6YyjYeydENc9YwCeKif"
```

---

### Task 3: document the setup log location

**Files:**
- Modify: `README.md` (the "Per-repo setup scripts" section, the paragraph
  ending in the `[setup-failed]` badge sentence — currently line 906)

**Interfaces:**
- Consumes: nothing. Pure docs.
- Produces: nothing.

- [ ] **Step 1: Add the log-location note**

In `README.md`, find this sentence (end of the `$SHELL -ilc` paragraph in
"Per-repo setup scripts"):

```
Setup failure does not block the workspace from being usable; it's surfaced as a `[setup-failed]` badge on the dashboard. Passing an empty value clears the script.
```

Replace it with:

```
Setup failure does not block the workspace from being usable; it's surfaced as a `[setup-failed]` badge on the dashboard. The script's output for each workspace creation is captured to `~/.local/state/wsx/logs/setup-<repo>-<name>.log` (overwritten on each run) — check it when a workspace shows `[setup-failed]` to see what went wrong. Passing an empty value clears the script.
```

- [ ] **Step 2: Verify the note renders and the path matches the code**

Run: `grep -n "setup-<repo>-<name>.log" README.md`
Expected: one match, in the "Per-repo setup scripts" section. Confirm the path
string matches `setup_log_path`'s format from Task 1.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document the workspace setup log location

Claude-Session: https://claude.ai/code/session_01BbT6YyjYeydENc9YwCeKif"
```

---

## Self-Review

**Spec coverage:**
- Log location / stable overwrite name → Task 1 (`setup_log_path`), Global Constraints.
- Captured contents (header / ANSI-stripped body / stderr prefix / footer) → Task 1 (`write_header`/`write_line`/`write_footer`) + tests.
- Only setup phase, only when script present → Task 2 (`run_setup_logged` gates on `script`), test `run_setup_logged_writes_no_file_without_script`.
- Tee from the existing on_line closure, `setup.rs` untouched → Task 2 Step 3/4.
- Best-effort I/O never breaks create → `create` returns `Option`, all writes `let _ = …` (Task 1/2).
- No modal/UX change → no modal files touched; progress sink still fed (asserted in Task 2 test).
- README docs, no CLI/detail-bar → Task 3 only.

**Placeholder scan:** none — every code and test block is complete.

**Type consistency:** `setup_log_path`, `create`, `write_line`, `write_footer`
signatures in Task 1 match their call sites in Task 2. `run_setup_logged`'s
parameter order in the helper (Task 2 Step 3) matches both test call sites
(Task 2 Step 1) and the `create_with_app` call (Task 2 Step 4). `SetupResult`
variants (`Ok` / `Failed { exit_code }` / `Skipped`) are used consistently.
