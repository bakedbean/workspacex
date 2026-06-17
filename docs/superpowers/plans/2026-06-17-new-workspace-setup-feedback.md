# New-workspace Setup Feedback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the bare braille spinner in the new-workspace modal with a coarse phase label, a live tail of the setup script's output, and an elapsed timer.

**Architecture:** A `std::sync::Mutex`-guarded `SetupProgress` sink lives in the data layer. `create_with_app` sets a coarse phase before each slow step and pushes the setup script's captured output lines into it (currently discarded by a `|_| {}` callback). The `SetupRunning` modal reads the sink each frame and renders phase + tail + timer. Output is segmented on `\r` and `\n` (not `\n` alone) so in-place progress bars surface.

**Tech Stack:** Rust, tokio (async), ratatui (TUI), `strip-ansi-escapes` crate (new dependency).

## Global Constraints

- The progress sink uses `std::sync::Mutex`, never `tokio::sync::Mutex`. Both the writer (`on_line` callback) and reader (`render`) are synchronous; the lock must never be held across an `.await`.
- ANSI stripping uses the `strip-ansi-escapes` crate (`strip_str`). Do not hand-roll an escape parser.
- Output is segmented on both `\r` and `\n`; empty segments are skipped.
- Stdout and stderr merge into one tail with no prefix.
- Esc-cancels and all existing create/reconcile behavior must be preserved unchanged.
- The modal box is `centered(area, 60, 14)` — inner content area is 58×12. Tail lines are truncated to fit.
- Spec: `docs/superpowers/specs/2026-06-17-new-workspace-setup-feedback-design.md`.

---

## File Structure

- `src/data/setup.rs` (modify) — replace the `BufReader::lines()` readers in `run_script` with a new `SegmentReader` that splits on `\r`/`\n`. Shared by setup and archive.
- `src/data/progress.rs` (create) — the `SetupPhase` enum, `SetupProgress` sink, and `SharedProgress` type alias.
- `src/data/mod.rs` (modify) — register `pub mod progress;`.
- `src/data/workspace.rs` (modify) — `create_with_app` gains a `progress` param; sets phases and pushes setup output.
- `src/app/input.rs` (modify) — Enter handler constructs the sink, stores it in the modal, passes it to `create_with_app`; the `SetupRunning` key handler destructure gains `..`.
- `src/ui/modal/mod.rs` (modify) — `SetupRunning` carries `progress` + `started`; render the phase/tail/timer; add a `truncate_to` helper and render tests.
- `Cargo.toml` (modify) — add `strip-ansi-escapes`.

---

## Task 1: Segment setup output on CR and LF

**Files:**
- Modify: `src/data/setup.rs` (imports at line 3; readers at lines 103–104 and the `tokio::select!` / drain loops at lines 105–131; add `SegmentReader` + tests)

**Interfaces:**
- Produces: `struct SegmentReader<R: AsyncRead + Unpin>` with `fn new(inner: R) -> Self` and `async fn next_segment(&mut self) -> std::io::Result<Option<String>>`. Internal to `setup.rs` (not `pub`). `run_setup` / `run_archive` public signatures and the `SetupLine` callback contract are unchanged.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `src/data/setup.rs`:

```rust
async fn collect_segments(input: &[u8]) -> Vec<String> {
    let mut r = SegmentReader::new(std::io::Cursor::new(input.to_vec()));
    let mut out = Vec::new();
    while let Some(seg) = r.next_segment().await.unwrap() {
        out.push(seg);
    }
    out
}

#[tokio::test]
async fn segments_split_on_newline() {
    assert_eq!(collect_segments(b"a\nb\nc\n").await, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn segments_split_on_carriage_return() {
    // No trailing delimiter: the final "c" is flushed at EOF.
    assert_eq!(collect_segments(b"a\rb\rc").await, vec!["a", "b", "c"]);
}

#[tokio::test]
async fn crlf_pair_yields_single_segment() {
    assert_eq!(collect_segments(b"x\r\ny\r\n").await, vec!["x", "y"]);
}

#[tokio::test]
async fn empty_segments_are_skipped_and_trailing_flushed() {
    assert_eq!(collect_segments(b"line\n\nblank").await, vec!["line", "blank"]);
}

#[tokio::test]
async fn empty_input_yields_nothing() {
    assert_eq!(collect_segments(b"").await, Vec::<String>::new());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test segments_ crlf_pair_ empty_input 2>&1 | tail -20`
Expected: FAIL — `cannot find type SegmentReader in this scope`.

- [ ] **Step 3: Add `SegmentReader` and switch the imports**

In `src/data/setup.rs`, replace the line-3 import:

```rust
use tokio::io::{AsyncBufReadExt, BufReader};
```

with:

```rust
use tokio::io::{AsyncRead, AsyncReadExt};
```

Then add this `SegmentReader` definition immediately after the `use` block (above `run_setup`):

```rust
/// Reads an async byte stream and yields text segments delimited by `\r`
/// or `\n`. Empty segments — the gap inside a `\r\n` pair, and blank lines —
/// are skipped; a trailing unterminated segment is flushed at EOF. Splitting
/// on `\r` lets in-place progress bars (pnpm/mise carriage-return redraws)
/// surface as individual segments instead of buffering until the next
/// newline. The logic is stateless across reads, so chunk boundaries — e.g. a
/// `\r\n` split across two reads — do not matter.
struct SegmentReader<R> {
    inner: R,
    pending: Vec<u8>,
    eof: bool,
}

impl<R: AsyncRead + Unpin> SegmentReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            pending: Vec::new(),
            eof: false,
        }
    }

    /// The next non-empty segment, or `None` at end of stream.
    async fn next_segment(&mut self) -> std::io::Result<Option<String>> {
        loop {
            // Carve a segment up to the first delimiter, if one is buffered.
            if let Some(idx) = self.pending.iter().position(|&b| b == b'\r' || b == b'\n') {
                let seg: Vec<u8> = self.pending.drain(..=idx).collect();
                // `seg` ends with the delimiter byte; drop it.
                let text = String::from_utf8_lossy(&seg[..seg.len() - 1]).into_owned();
                if text.is_empty() {
                    continue;
                }
                return Ok(Some(text));
            }
            if self.eof {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let text = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                if text.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(text));
            }
            let mut chunk = [0u8; 1024];
            let n = self.inner.read(&mut chunk).await?;
            if n == 0 {
                self.eof = true;
            } else {
                self.pending.extend_from_slice(&chunk[..n]);
            }
        }
    }
}
```

- [ ] **Step 4: Run the new tests to verify they pass**

Run: `cargo test segments_ crlf_pair_ empty_ 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 5: Switch `run_script` to use `SegmentReader`**

In `run_script`, replace the two reader bindings (lines 103–104):

```rust
    let mut out_reader = BufReader::new(stdout).lines();
    let mut err_reader = BufReader::new(stderr).lines();
```

with:

```rust
    let mut out_reader = SegmentReader::new(stdout);
    let mut err_reader = SegmentReader::new(stderr);
```

Then, in the `tokio::select!` loop and the post-loop drain, replace every `.next_line()` call with `.next_segment()`. The four sites are:

```rust
            line = out_reader.next_segment() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stdout(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stdout read: {e}"))),
            },
            line = err_reader.next_segment() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stderr(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stderr read: {e}"))),
            },
```

and the drain loops:

```rust
    while let Ok(Some(l)) = out_reader.next_segment().await {
        on_line(SetupLine::Stdout(l));
    }
    while let Ok(Some(l)) = err_reader.next_segment().await {
        on_line(SetupLine::Stderr(l));
    }
```

- [ ] **Step 6: Run the full setup test module to verify nothing regressed**

Run: `cargo test --lib data::setup 2>&1 | tail -25`
Expected: PASS — all existing `run_setup`/`run_script` tests plus the 5 new segmenter tests.

- [ ] **Step 7: Commit**

```bash
git add src/data/setup.rs
git commit -m "feat(data): segment setup output on CR and LF"
```

---

## Task 2: SetupProgress sink

**Files:**
- Create: `src/data/progress.rs`
- Modify: `src/data/mod.rs` (add `pub mod progress;` after the other `pub mod` lines)
- Modify: `Cargo.toml` (add `strip-ansi-escapes`)

**Interfaces:**
- Produces:
  - `pub enum SetupPhase { Fetching, CreatingWorktree, RunningSetup }` (derives `Debug, Clone, Copy, PartialEq, Eq`) with `pub fn label(self) -> &'static str`.
  - `pub struct SetupProgress` (derives `Debug`) with `pub fn shared() -> SharedProgress`, `pub fn set_phase(&mut self, SetupPhase)`, `pub fn phase(&self) -> SetupPhase`, `pub fn push_line(&mut self, raw: &str)`, `pub fn recent(&self, n: usize) -> Vec<String>`.
  - `pub type SharedProgress = std::sync::Arc<std::sync::Mutex<SetupProgress>>;`

- [ ] **Step 1: Add the dependency**

Run: `cargo add strip-ansi-escapes`
Expected: adds `strip-ansi-escapes = "0.2"` (or newer 0.2.x) under `[dependencies]` in `Cargo.toml`.

- [ ] **Step 2: Write the failing tests**

Create `src/data/progress.rs` with ONLY the test module for now (the impl comes in Step 4):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_round_trips_and_labels() {
        let p = SetupProgress::shared();
        assert_eq!(p.lock().unwrap().phase(), SetupPhase::Fetching);
        p.lock().unwrap().set_phase(SetupPhase::RunningSetup);
        assert_eq!(p.lock().unwrap().phase(), SetupPhase::RunningSetup);
        assert_eq!(SetupPhase::Fetching.label(), "Fetching base");
        assert_eq!(SetupPhase::CreatingWorktree.label(), "Creating worktree");
        assert_eq!(SetupPhase::RunningSetup.label(), "Running setup");
    }

    #[test]
    fn push_line_strips_ansi_and_trims() {
        let p = SetupProgress::shared();
        p.lock().unwrap().push_line("\x1b[32mgreen text\x1b[0m   ");
        assert_eq!(p.lock().unwrap().recent(5), vec!["green text"]);
    }

    #[test]
    fn push_line_skips_blank() {
        let p = SetupProgress::shared();
        p.lock().unwrap().push_line("   ");
        p.lock().unwrap().push_line("");
        assert!(p.lock().unwrap().recent(5).is_empty());
    }

    #[test]
    fn ring_buffer_evicts_oldest_at_cap() {
        let p = SetupProgress::shared();
        for i in 0..(CAP + 5) {
            p.lock().unwrap().push_line(&format!("line {i}"));
        }
        let g = p.lock().unwrap();
        let all = g.recent(CAP + 100);
        assert_eq!(all.len(), CAP, "buffer should be capped");
        assert_eq!(all[0], format!("line {}", 5), "oldest 5 evicted");
        assert_eq!(all[CAP - 1], format!("line {}", CAP + 4));
    }

    #[test]
    fn recent_returns_last_n_oldest_first() {
        let p = SetupProgress::shared();
        for i in 0..10 {
            p.lock().unwrap().push_line(&format!("l{i}"));
        }
        assert_eq!(p.lock().unwrap().recent(3), vec!["l7", "l8", "l9"]);
    }
}
```

- [ ] **Step 3: Register the module and run tests to verify they fail**

In `src/data/mod.rs`, add after the existing `pub mod` lines (after `pub mod store;` / before or after `pub mod workspace;`):

```rust
pub mod progress;
```

Run: `cargo test --lib data::progress 2>&1 | tail -20`
Expected: FAIL — `cannot find ... SetupProgress` / `CAP` not found.

- [ ] **Step 4: Write the implementation**

Prepend the implementation above the test module in `src/data/progress.rs`:

```rust
//! Live progress sink for workspace creation. `workspace::create_with_app`
//! writes the current phase and the setup script's output lines here; the TUI
//! `SetupRunning` modal reads it each frame to render a phase label and a live
//! tail. A plain `std::sync::Mutex` (not tokio) is used deliberately: both the
//! writer (the synchronous `on_line` callback) and the reader (`render`) are
//! synchronous and hold the lock only for microseconds, never across `.await`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Coarse phase of `create_with_app`, shown in the modal header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupPhase {
    /// `git fetch` of the base branch.
    Fetching,
    /// `git worktree add`.
    CreatingWorktree,
    /// The repo's setup script.
    RunningSetup,
}

impl SetupPhase {
    /// Header label for the modal (no trailing ellipsis; the renderer adds it).
    pub fn label(self) -> &'static str {
        match self {
            SetupPhase::Fetching => "Fetching base",
            SetupPhase::CreatingWorktree => "Creating worktree",
            SetupPhase::RunningSetup => "Running setup",
        }
    }
}

/// Max output lines retained in the ring buffer.
const CAP: usize = 64;

/// Progress state shared between the create task and the modal renderer.
#[derive(Debug)]
pub struct SetupProgress {
    phase: SetupPhase,
    lines: VecDeque<String>,
}

/// Shared handle. Clone to hand one copy to the modal and one to the create task.
pub type SharedProgress = Arc<Mutex<SetupProgress>>;

impl SetupProgress {
    /// A new handle, starting in the `Fetching` phase with no output.
    pub fn shared() -> SharedProgress {
        Arc::new(Mutex::new(SetupProgress {
            phase: SetupPhase::Fetching,
            lines: VecDeque::new(),
        }))
    }

    pub fn set_phase(&mut self, phase: SetupPhase) {
        self.phase = phase;
    }

    pub fn phase(&self) -> SetupPhase {
        self.phase
    }

    /// Strip ANSI escapes, trim trailing whitespace, and append. Drops the
    /// oldest line once at capacity. Blank results are ignored.
    pub fn push_line(&mut self, raw: &str) {
        let clean = strip_ansi_escapes::strip_str(raw);
        let clean = clean.trim_end();
        if clean.is_empty() {
            return;
        }
        if self.lines.len() == CAP {
            self.lines.pop_front();
        }
        self.lines.push_back(clean.to_string());
    }

    /// The last `n` lines, oldest-first, for the modal tail.
    pub fn recent(&self, n: usize) -> Vec<String> {
        let start = self.lines.len().saturating_sub(n);
        self.lines.iter().skip(start).cloned().collect()
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib data::progress 2>&1 | tail -20`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/data/mod.rs src/data/progress.rs
git commit -m "feat(data): add SetupProgress sink for workspace creation"
```

---

## Task 3: Report phases and setup output from create_with_app

**Files:**
- Modify: `src/data/workspace.rs` (`create_with_app` signature ~line 144; phase 2/4/5 bodies ~lines 170–232; the in-file test at ~line 1045)
- Modify: `src/app/input.rs` (Enter handler ~lines 1129–1156)

**Interfaces:**
- Consumes: `crate::data::progress::{SetupProgress, SetupPhase, SharedProgress}` from Task 2; `SetupLine` (already imported in `workspace.rs`).
- Produces: `create_with_app(app, repo, name, worktree_base, yolo, agent, progress: SharedProgress, cancel)` — the new `progress` parameter sits immediately before `cancel`. The `Modal::SetupRunning` struct is NOT changed in this task; the sink is written but not yet read.

- [ ] **Step 1: Add the import to workspace.rs**

In `src/data/workspace.rs`, below the existing `use crate::data::setup::...;` line, add:

```rust
use crate::data::progress::{SetupPhase, SharedProgress};
```

- [ ] **Step 2: Add the `progress` parameter to `create_with_app`**

Change the signature (currently ending `agent: AgentKind,` then `cancel: ...,`) to insert `progress` before `cancel`:

```rust
pub async fn create_with_app(
    app: crate::app::SharedApp,
    repo: Repo,
    name: Option<String>,
    worktree_base: PathBuf,
    yolo: bool,
    agent: AgentKind,
    progress: SharedProgress,
    cancel: tokio_util::sync::CancellationToken,
) -> Result<CreatedWorkspace> {
```

- [ ] **Step 3: Set phases and wire the setup callback**

In `create_with_app`, set the phase before each slow step. Add immediately before the `crate::git::fetch_for_base(...)` call (phase 2):

```rust
    if let Ok(mut p) = progress.lock() {
        p.set_phase(SetupPhase::Fetching);
    }
```

Add immediately before the `crate::git::create_worktree(...)` call (phase 4):

```rust
    if let Ok(mut p) = progress.lock() {
        p.set_phase(SetupPhase::CreatingWorktree);
    }
```

Then replace the phase-5 setup block. Change:

```rust
    let setup_result = setup::run_setup(
        repo.setup_script.as_deref(),
        &repo.path,
        &worktree_path,
        cancel.clone(),
        |_| {},
    )
    .await;
```

to:

```rust
    if let Ok(mut p) = progress.lock() {
        p.set_phase(SetupPhase::RunningSetup);
    }
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

- [ ] **Step 4: Update the in-file create_with_app test**

In `src/data/workspace.rs`, the test `create_with_app_works_end_to_end_without_holding_lock` (~line 1045) calls `create_with_app(...)`. Add a progress argument before `cancel`:

```rust
        let cancel = CancellationToken::new();
        let progress = crate::data::progress::SetupProgress::shared();
        let created = create_with_app(
            app.clone(),
            repo,
            Some("alpha".to_string()),
            base.path().to_path_buf(),
            false,
            crate::pty::session::AgentKind::Claude,
            progress,
            cancel,
        )
        .await
        .unwrap();
```

- [ ] **Step 5: Update the input.rs Enter handler to construct and pass the sink**

In `src/app/input.rs`, the `NewWorkspace` `KeyCode::Enter` arm (~lines 1129–1156). After `let cancel = tokio_util::sync::CancellationToken::new();` and before the `tokio::spawn`, construct the sink. The modal still carries only `cancel` in this task. Change the spawn call to pass `progress`:

```rust
                let cancel = tokio_util::sync::CancellationToken::new();
                let create_gen = app.alloc_create_gen();
                let progress = crate::data::progress::SetupProgress::shared();
                app.modal = Some(Modal::SetupRunning {
                    cancel: cancel.clone(),
                });
                let shared_clone = shared.clone();
                tokio::spawn(async move {
                    let result = crate::data::workspace::create_with_app(
                        shared_clone.clone(),
                        repo,
                        name,
                        base,
                        yolo,
                        agent,
                        progress,
                        cancel,
                    )
                    .await;
                    reconcile_create_result(shared_clone, create_gen, result).await;
                });
```

- [ ] **Step 6: Build and run the affected tests**

Run: `cargo test --lib data::workspace 2>&1 | tail -25`
Expected: PASS — including `create_with_app_works_end_to_end_without_holding_lock`.

Run: `cargo build 2>&1 | tail -15`
Expected: builds clean (the sink is written but not yet read — no unused-warning because `progress` is consumed by `create_with_app`).

- [ ] **Step 7: Commit**

```bash
git add src/data/workspace.rs src/app/input.rs
git commit -m "feat(data): report create phases and setup output to progress sink"
```

---

## Task 4: Render phase, tail, and timer in the modal

**Files:**
- Modify: `src/ui/modal/mod.rs` (`Modal::SetupRunning` variant ~lines 54–56; its render arm ~lines 193–197; add `truncate_to` helper; add render tests)
- Modify: `src/app/input.rs` (Enter-handler modal construction ~line 1139; `SetupRunning` key handler destructure ~line 1229)

**Interfaces:**
- Consumes: `crate::data::progress::SharedProgress` and `SetupProgress::recent/phase`, `SetupPhase::label` from Tasks 2–3; the sink constructed in `input.rs` from Task 3.
- Produces: `Modal::SetupRunning { cancel, progress: SharedProgress, started: std::time::Instant }`.

- [ ] **Step 1: Add the modal fields**

In `src/ui/modal/mod.rs`, change the `SetupRunning` variant:

```rust
    SetupRunning {
        cancel: tokio_util::sync::CancellationToken,
        progress: crate::data::progress::SharedProgress,
        started: std::time::Instant,
    },
```

- [ ] **Step 2: Add the truncation helper**

Near the other free functions in `src/ui/modal/mod.rs` (e.g. above `capitalize_first`), add:

```rust
/// Truncate `s` to at most `max` characters, appending '…' when cut. Used to
/// keep setup-output tail lines inside the modal's inner width.
fn truncate_to(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}
```

- [ ] **Step 3: Write the failing render tests**

Add to the `#[cfg(test)] mod tests` block in `src/ui/modal/mod.rs`:

```rust
    fn render_to_text(modal: &Modal) -> String {
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
        term.draw(|f| render(f, f.area(), modal, 0, &theme)).unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn setup_running_shows_phase_and_recent_lines() {
        use crate::data::progress::{SetupPhase, SetupProgress};
        let progress = SetupProgress::shared();
        {
            let mut p = progress.lock().unwrap();
            p.set_phase(SetupPhase::RunningSetup);
            p.push_line("mise install");
            p.push_line("Installing dependencies");
        }
        let modal = Modal::SetupRunning {
            cancel: tokio_util::sync::CancellationToken::new(),
            progress,
            started: std::time::Instant::now(),
        };
        let text = render_to_text(&modal);
        assert!(text.contains("Running setup"), "missing phase:\n{text}");
        assert!(text.contains("Installing dependencies"), "missing line:\n{text}");
        assert!(text.contains("[esc] cancel"), "missing footer:\n{text}");
    }

    #[test]
    fn setup_running_truncates_overwide_line() {
        use crate::data::progress::SetupProgress;
        let progress = SetupProgress::shared();
        progress.lock().unwrap().push_line(&"x".repeat(200));
        let modal = Modal::SetupRunning {
            cancel: tokio_util::sync::CancellationToken::new(),
            progress,
            started: std::time::Instant::now(),
        };
        let text = render_to_text(&modal);
        assert!(text.contains('…'), "over-wide line should be truncated:\n{text}");
    }
```

- [ ] **Step 4: Run the tests to verify they fail**

Run: `cargo test --lib ui::modal 2>&1 | tail -25`
Expected: FAIL — `Modal::SetupRunning` arm still ignores `progress`/`started` (compile error on missing fields in the existing render arm, or assertion failures).

- [ ] **Step 5: Update the render arm**

In `src/ui/modal/mod.rs`, replace the `Modal::SetupRunning` render arm (currently lines ~193–197):

```rust
        Modal::SetupRunning {
            progress, started, ..
        } => {
            let frame = crate::ui::dashboard::spinner::frame(tick);
            let (phase_label, tail) = match progress.lock() {
                Ok(p) => (p.phase().label(), p.recent(6)),
                Err(_) => ("Working", Vec::new()),
            };
            let secs = started.elapsed().as_secs();
            let elapsed = format!("{:02}:{:02}", secs / 60, secs % 60);
            let mut body = format!("  {frame} {phase_label}…   ({elapsed})\n\n");
            if tail.is_empty() {
                body.push_str("  (waiting for output…)\n");
            } else {
                for line in &tail {
                    body.push_str(&format!("  {}\n", truncate_to(line, 54)));
                }
            }
            body.push_str("\n  [esc] cancel");
            ("new workspace", body)
        }
```

- [ ] **Step 6: Run the modal tests to verify they pass**

Run: `cargo test --lib ui::modal 2>&1 | tail -25`
Expected: PASS — including the two new tests and the existing `workspace_actions_overlay_lists_all_actions`.

- [ ] **Step 7: Update input.rs to populate the new fields and fix the handler destructure**

In `src/app/input.rs`, change the modal construction in the Enter handler so it stores the sink and a start instant (and clones `progress` since the spawn still needs it):

```rust
                let cancel = tokio_util::sync::CancellationToken::new();
                let create_gen = app.alloc_create_gen();
                let progress = crate::data::progress::SetupProgress::shared();
                app.modal = Some(Modal::SetupRunning {
                    cancel: cancel.clone(),
                    progress: progress.clone(),
                    started: std::time::Instant::now(),
                });
```

The `tokio::spawn` block is unchanged — it still passes the (now-cloned-from) `progress` into `create_with_app`.

Then update the `SetupRunning` key handler (~line 1229) destructure to ignore the new fields:

```rust
        Modal::SetupRunning { cancel, .. } => {
```

- [ ] **Step 8: Build and run the full test suite**

Run: `cargo build 2>&1 | tail -15`
Expected: builds clean.

Run: `cargo test 2>&1 | tail -25`
Expected: PASS — whole suite green.

- [ ] **Step 9: Lint and format**

Run: `cargo clippy --all-targets 2>&1 | tail -20 && cargo fmt`
Expected: no clippy warnings on the new code; `cargo fmt` leaves a clean tree (re-stage if it reformats).

- [ ] **Step 10: Commit**

```bash
git add src/ui/modal/mod.rs src/app/input.rs
git commit -m "feat(dashboard): show phase + live setup output in new-workspace modal"
```

---

## Self-Review Notes

- **Spec coverage:** phase label (Task 4 render + Task 3 `set_phase`), live tail (Task 3 callback + Task 4 render), elapsed timer (Task 4 `started.elapsed()`), CR/LF segmentation (Task 1), `strip-ansi-escapes` (Task 2), std-Mutex sink (Task 2), merged stdout/stderr (Task 3 callback), Esc-cancel preserved (Task 4 Step 7 `{ cancel, .. }`). All covered.
- **Non-goals respected:** failed-setup surfacing untouched (phase-6 finalize logic unchanged); no persistence/scrollback added.
- **Type consistency:** `SharedProgress`, `SetupProgress::{shared,set_phase,phase,push_line,recent}`, `SetupPhase::{Fetching,CreatingWorktree,RunningSetup,label}` used identically across Tasks 2–4. `create_with_app`'s `progress` parameter position (before `cancel`) matches the call sites in Task 3 (test + input.rs).
