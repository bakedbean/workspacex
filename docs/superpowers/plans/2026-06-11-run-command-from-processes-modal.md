# Run a background command from the processes modal — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** From the dashboard's processes modal, let the user type a shell command and launch it as a background process in the workspace's worktree, with output captured to a log file.

**Architecture:** Extend `Modal::ProcessList` with an `input` buffer (mode flag) and a `notice` line. A new `spawn_background_command` helper in `src/commands/external.rs` runs the command via `sh -c` with `cwd = worktree_path`, redirecting stdout/stderr to a log file under `~/.local/state/wsx/logs`. Because the process runs in the worktree, the existing `cwd`-based process scan surfaces it in the same modal, where the existing `[K]` verb kills it.

**Tech Stack:** Rust, ratatui (TUI), crossterm (key events), tokio (async input loop).

**Spec:** `docs/superpowers/specs/2026-06-11-run-command-from-processes-modal-design.md`

---

## File structure

- `src/commands/external.rs` — new pure helpers (`shell_argv`, `background_log_path`) and the spawn function (`spawn_background_command`); unit tests for the pure helpers.
- `src/ui/modal.rs` — add `input`/`notice` fields to `Modal::ProcessList`; render the input line, the `[r] run` footer hint, and the notice line in `render_process_list`.
- `src/app/input.rs` — handle the `r` key and input-mode keystrokes in the `Modal::ProcessList` match arm; add `launch_workspace_command` helper.
- `src/app/render.rs` — pass the new modal fields into `render_process_list`.
- `src/app/input_tests.rs` — state-transition tests for the modal input mode.
- `README.md` — note the new `[r] run` action in the process-tracking section.

---

## Task 1: Pure helpers for shell argv and log path

**Files:**
- Modify: `src/commands/external.rs` (add two functions near `spawn_parts`, ~line 197, and tests in the existing `#[cfg(test)] mod tests` block ~line 237)

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests { ... }` block in `src/commands/external.rs`:

```rust
    #[test]
    fn shell_argv_wraps_command_as_single_arg() {
        assert_eq!(
            shell_argv("npm run dev && echo done"),
            vec![
                "sh".to_string(),
                "-c".to_string(),
                "npm run dev && echo done".to_string(),
            ],
        );
    }

    #[test]
    fn background_log_path_uses_workspace_id_and_timestamp() {
        let p = background_log_path(std::path::Path::new("/logs"), 7, 1234);
        assert_eq!(p, std::path::PathBuf::from("/logs/ws7-1234.log"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wsx shell_argv_wraps_command_as_single_arg background_log_path_uses_workspace_id_and_timestamp`
Expected: FAIL — `cannot find function shell_argv` / `background_log_path` in this scope.

(If the crate name isn't `wsx`, use `cargo test shell_argv_wraps`; `grep '^name' Cargo.toml` to confirm.)

- [ ] **Step 3: Implement the helpers**

Add to `src/commands/external.rs`, just above `fn spawn_parts` (~line 184):

```rust
/// Build the argv for running `command` through a POSIX shell. The whole command
/// is passed as a single `-c` argument so pipes, `&&`, and env vars work as typed.
fn shell_argv(command: &str) -> Vec<String> {
    vec!["sh".to_string(), "-c".to_string(), command.to_string()]
}

/// Path for a background command's captured output:
/// `<log_dir>/ws<workspace_id>-<epoch_ms>.log`.
pub fn background_log_path(
    log_dir: &Path,
    workspace_id: i64,
    epoch_ms: u64,
) -> std::path::PathBuf {
    log_dir.join(format!("ws{workspace_id}-{epoch_ms}.log"))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test shell_argv_wraps background_log_path_uses`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src/commands/external.rs
git commit -m "feat(processes): add shell-argv and background log-path helpers"
```

---

## Task 2: `spawn_background_command`

**Files:**
- Modify: `src/commands/external.rs` (add a public function after the helpers from Task 1)

- [ ] **Step 1: Implement the spawn function**

Add to `src/commands/external.rs` (e.g. just below `background_log_path`). Note: actual process spawning is intentionally not unit-tested here, matching how the rest of `external.rs` avoids spawning real processes in tests. The pure parts (`shell_argv`, `background_log_path`) are covered by Task 1.

> **Revised during implementation.** This plan originally spawned the command as a plain
> child of the wsx process. Manual testing showed it never appeared in the modal: wsx's
> process scan hides its own descendants, so a child of wsx is filtered out. The shipped
> version (below) **detaches** the command — it wraps it in a backgrounded subshell
> (`( <command> ) &`) so the parent `sh` exits immediately and the real command reparents to
> init, and calls `setsid` (unix) so it runs in its own session with no controlling
> terminal. wsx reaps the short-lived wrapper `sh` so no zombie accumulates.

```rust
/// Wrap a user command so it runs detached from the wsx process. The backgrounded
/// subshell `( … ) &` lets the parent `sh` exit immediately, reparenting the command
/// to init so it no longer descends from wsx — required because wsx's process scan
/// hides its own descendants. Wrapping (rather than appending `&`) backgrounds compound
/// commands like `a && b` as a unit.
fn detached_command_script(command: &str) -> String {
    format!("( {command} ) &")
}

/// Launch `command` as a background process whose working directory is `worktree`,
/// detached from wsx so it surfaces in the per-workspace process scan and outlives the
/// dashboard — like running it from a fresh terminal in the worktree. The command runs
/// through `sh -c` (so shell features behave as expected), wrapped by
/// `detached_command_script`, and on unix the spawned shell calls `setsid`. stdout and
/// stderr are redirected to `log_path` (created / truncated); stdin is null.
pub fn spawn_background_command(worktree: &Path, command: &str, log_path: &Path) -> Result<()> {
    if command.trim().is_empty() {
        return Err(Error::UserInput("command is empty".into()));
    }
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::UserInput(format!("create log dir: {e}")))?;
    }
    let log = std::fs::File::create(log_path)
        .map_err(|e| Error::UserInput(format!("open log {}: {e}", log_path.display())))?;
    let log_err = log
        .try_clone()
        .map_err(|e| Error::UserInput(format!("clone log handle: {e}")))?;

    let parts = shell_argv(&detached_command_script(command));
    let mut cmd = std::process::Command::new(&parts[0]);
    cmd.args(&parts[1..])
        .current_dir(worktree)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(log))
        .stderr(std::process::Stdio::from(log_err));

    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::UserInput(format!("spawn background command: {e}")))?;
    // The shell backgrounds the command and exits right away; reap it so wsx
    // doesn't accumulate a zombie per launch. The detached command keeps running.
    let _ = child.wait();
    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build`
Expected: builds clean (a `dead_code` warning for `spawn_background_command` is acceptable until Task 3 wires it).

- [ ] **Step 3: Commit**

```bash
git add src/commands/external.rs
git commit -m "feat(processes): spawn background command with output captured to a log file"
```

---

## Task 3: Modal fields, input handling, and launch wiring

**Files:**
- Modify: `src/ui/modal.rs:56-59` (add fields to `Modal::ProcessList`)
- Modify: `src/app/input.rs:635-638` (open site initializes new fields)
- Modify: `src/app/input.rs:1357-1404` (rewrite the `Modal::ProcessList` match arm)
- Modify: `src/app/input.rs` (add `launch_workspace_command` helper near `rescan_processes` usage)
- Modify: `src/app/render.rs:722-725` (destructure with `..` so it still compiles; rendering is wired in Task 4)
- Test: `src/app/input_tests.rs` (new `mod process_command_tests`)

- [ ] **Step 1: Add the fields to the enum variant**

In `src/ui/modal.rs`, change the `ProcessList` variant (lines 56-59):

```rust
    ProcessList {
        workspace_id: crate::data::store::WorkspaceId,
        selected: usize,
        /// `None` = list mode; `Some(buffer)` = the user is typing a command to run.
        input: Option<String>,
        /// Last launch result (success path or error), shown below the list.
        notice: Option<String>,
    },
```

- [ ] **Step 2: Update the open site**

In `src/app/input.rs`, the `Shift+K` handler (lines 635-638):

```rust
                app.modal = Some(Modal::ProcessList {
                    workspace_id: id,
                    selected: 0,
                    input: None,
                    notice: None,
                });
```

- [ ] **Step 3: Keep `render.rs` compiling**

In `src/app/render.rs` (line 722), change the destructure to ignore the new fields for now:

```rust
            crate::ui::modal::Modal::ProcessList {
                workspace_id,
                selected,
                ..
            } => {
```

(Leave the `render_process_list(...)` call unchanged — its signature changes in Task 4.)

- [ ] **Step 4: Add the `launch_workspace_command` helper**

In `src/app/input.rs`, add this free function (place it near `rescan_processes`'s caller context, e.g. just above `handle_key_modal` ~line 1077):

```rust
/// Resolve the worktree for `workspace_id`, build a per-launch log path under the
/// wsx log dir, and spawn `command` there as a background process. Returns a
/// one-line notice (success with the log path, or an error) for the modal.
fn launch_workspace_command(
    app: &App,
    workspace_id: crate::data::store::WorkspaceId,
    command: &str,
) -> String {
    let Some(worktree) = app
        .workspaces
        .iter()
        .find(|(_, w)| w.id == workspace_id)
        .map(|(_, w)| w.worktree_path.clone())
    else {
        return "error: workspace not found".to_string();
    };
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let log_dir = crate::config::Dirs::discover().log_dir();
    let log_path =
        crate::commands::external::background_log_path(&log_dir, workspace_id.0, now_ms);
    match crate::commands::external::spawn_background_command(&worktree, command, &log_path) {
        Ok(()) => format!("\u{25B6} started \u{2192} {}", log_path.display()),
        Err(e) => format!("error: {e}"),
    }
}
```

- [ ] **Step 5: Rewrite the `Modal::ProcessList` match arm**

In `src/app/input.rs`, replace the entire current arm (lines 1357-1404, from `Modal::ProcessList {` through its closing `}` before `Modal::RepoSettings {`) with:

```rust
        Modal::ProcessList {
            workspace_id,
            mut selected,
            input,
            notice,
        } => {
            let procs = app
                .workspace_processes
                .get(&workspace_id)
                .cloned()
                .unwrap_or_default();

            // Input mode: capture keystrokes into the command buffer.
            if let Some(mut buffer) = input {
                match k.code {
                    KeyCode::Esc => {
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: None,
                            notice,
                        });
                    }
                    KeyCode::Enter => {
                        let command = buffer.trim().to_string();
                        if command.is_empty() {
                            // Empty command: stay in input mode, keep the buffer.
                            app.modal = Some(Modal::ProcessList {
                                workspace_id,
                                selected,
                                input: Some(buffer),
                                notice,
                            });
                        } else {
                            let new_notice =
                                launch_workspace_command(app, workspace_id, &command);
                            app.modal = Some(Modal::ProcessList {
                                workspace_id,
                                selected,
                                input: None,
                                notice: Some(new_notice),
                            });
                            rescan_processes(app).await;
                        }
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: Some(buffer),
                            notice,
                        });
                    }
                    KeyCode::Char(c) => {
                        buffer.push(c);
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: Some(buffer),
                            notice,
                        });
                    }
                    _ => {
                        app.modal = Some(Modal::ProcessList {
                            workspace_id,
                            selected,
                            input: Some(buffer),
                            notice,
                        });
                    }
                }
                return Ok(());
            }

            // List mode.
            // ProcessList intentionally does NOT alias j/k to nav like the other
            // list modals: `k` here means SIGTERM and `K` means SIGKILL, so
            // vim-style movement would clash with the kill verbs. Arrow keys are
            // the only navigation; `r` opens the run-command input.
            match k.code {
                KeyCode::Esc => {
                    app.modal = None;
                }
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: None,
                        notice,
                    });
                }
                KeyCode::Down => {
                    if !procs.is_empty() {
                        selected = (selected + 1).min(procs.len() - 1);
                    }
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: None,
                        notice,
                    });
                }
                KeyCode::Char('r') => {
                    app.modal = Some(Modal::ProcessList {
                        workspace_id,
                        selected,
                        input: Some(String::new()),
                        notice,
                    });
                }
                KeyCode::Char('k') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::activity::proc::kill_pid(p.pid, "TERM").await;
                        rescan_processes(app).await;
                    }
                }
                KeyCode::Char('K') => {
                    if let Some(p) = procs.get(selected) {
                        let _ = crate::activity::proc::kill_pid(p.pid, "KILL").await;
                        rescan_processes(app).await;
                    }
                }
                _ => {}
            }
        }
```

Note: the `rescan_processes` selection clamp (`src/app.rs:886`) matches `Modal::ProcessList { selected, .. }` with `..`, so it already tolerates the new fields — no change needed there.

- [ ] **Step 6: Verify the whole crate compiles**

Run: `cargo build`
Expected: builds clean. The `dead_code` warning from Task 2 is now gone (the spawn fn is used).

- [ ] **Step 7: Write the state-transition tests**

Append to `src/app/input_tests.rs` (top level, after the existing `mod` blocks):

```rust
#[cfg(test)]
mod process_command_tests {
    use super::*;
    use crate::data::store::{Store, WorkspaceId};
    use crate::ui::modal::Modal;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn shared() -> SharedApp {
        Arc::new(Mutex::new(
            App::new(Store::open_in_memory().unwrap(), PathBuf::from("/tmp/wsx-test")).unwrap(),
        ))
    }

    fn process_list(input: Option<String>) -> Modal {
        Modal::ProcessList {
            workspace_id: WorkspaceId(1),
            selected: 0,
            input,
            notice: None,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn r_enters_input_mode() {
        let mut app =
            App::new(Store::open_in_memory().unwrap(), PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(process_list(None));
        let shared = shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), .. }) if b.is_empty()
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn typing_appends_and_backspace_pops() {
        let mut app =
            App::new(Store::open_in_memory().unwrap(), PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(process_list(Some(String::new())));
        let shared = shared();
        for c in ['l', 's'] {
            handle_key_modal(
                &mut app,
                &shared,
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
            )
            .await
            .unwrap();
        }
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), .. }) if b == "ls"
        ));
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), .. }) if b == "l"
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn esc_in_input_mode_returns_to_list_mode() {
        let mut app =
            App::new(Store::open_in_memory().unwrap(), PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(process_list(Some("npm".to_string())));
        let shared = shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        // Still open, but back in list mode (input is None).
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: None, .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn enter_with_empty_command_is_a_noop() {
        let mut app =
            App::new(Store::open_in_memory().unwrap(), PathBuf::from("/tmp/wsx-test")).unwrap();
        app.modal = Some(process_list(Some("   ".to_string())));
        let shared = shared();
        handle_key_modal(
            &mut app,
            &shared,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
        )
        .await
        .unwrap();
        // Stays in input mode with the buffer intact; no launch, no notice.
        assert!(matches!(
            app.modal,
            Some(Modal::ProcessList { input: Some(ref b), notice: None, .. }) if b == "   "
        ));
    }
}
```

- [ ] **Step 8: Run the new tests**

Run: `cargo test process_command_tests`
Expected: PASS (4 tests).

- [ ] **Step 9: Commit**

```bash
git add src/ui/modal.rs src/app/input.rs src/app/render.rs src/app/input_tests.rs
git commit -m "feat(processes): run a command from the processes modal"
```

---

## Task 4: Render the input line, footer hint, and notice

**Files:**
- Modify: `src/ui/modal.rs:574-633` (`render_process_list` signature + body)
- Modify: `src/app/render.rs:722-745` (destructure new fields, pass them in)

- [ ] **Step 1: Update `render_process_list` signature and body**

In `src/ui/modal.rs`, change the signature (line 574) to add `input` and `notice`:

```rust
pub fn render_process_list(
    f: &mut Frame,
    area: Rect,
    workspace_name: &str,
    procs: &[crate::activity::proc::ProcInfo],
    selected: usize,
    input: Option<&str>,
    notice: Option<&str>,
    theme: &Theme,
) {
```

Replace the layout/footer section (currently lines 595-632, from `let chunks = Layout::default()` through the final `f.render_widget(...footer_area)` call) with:

```rust
    let has_notice = notice.is_some();
    let constraints = if has_notice {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    let body_area = chunks[0];
    let (notice_area, footer_area) = if has_notice {
        (Some(chunks[1]), chunks[2])
    } else {
        (None, chunks[1])
    };

    if procs.is_empty() {
        f.render_widget(
            Paragraph::new("(no tracked processes)").style(theme.dim_style()),
            body_area,
        );
    } else {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("  {:<7} {:<20} {}", "PID", "COMMAND", "CWD"),
            theme.header_style(),
        )));
        for (i, p) in procs.iter().enumerate() {
            let body = format!(
                "  {:<7} {:<20} {}",
                p.pid,
                truncate(&p.command, 20),
                p.cwd.display()
            );
            if i == selected {
                lines.push(Line::from(Span::styled(body, theme.selected_style())));
            } else {
                lines.push(Line::from(body));
            }
        }
        f.render_widget(Paragraph::new(lines), body_area);
    }

    if let (Some(area), Some(text)) = (notice_area, notice) {
        let style = if text.starts_with("error") {
            theme.err_style()
        } else {
            theme.ok_style()
        };
        f.render_widget(Paragraph::new(text.to_string()).style(style), area);
    }

    if let Some(buf) = input {
        f.render_widget(
            Paragraph::new(format!("run: {buf}\u{2588}   [enter] launch  [esc] cancel"))
                .style(theme.header_style()),
            footer_area,
        );
    } else {
        f.render_widget(
            Paragraph::new(
                "[\u{2191}/\u{2193}] move   [r] run   [k] term   [K] kill   [esc] close",
            )
            .style(theme.dim_style()),
            footer_area,
        );
    }
```

- [ ] **Step 2: Pass the fields from `render.rs`**

In `src/app/render.rs`, update the `ProcessList` arm (lines 722-745):

```rust
            crate::ui::modal::Modal::ProcessList {
                workspace_id,
                selected,
                input,
                notice,
            } => {
                let workspace_name = app
                    .workspaces
                    .iter()
                    .find(|(_, w)| w.id == *workspace_id)
                    .map(|(_, w)| w.name.clone())
                    .unwrap_or_default();
                let procs = app
                    .workspace_processes
                    .get(workspace_id)
                    .cloned()
                    .unwrap_or_default();
                crate::ui::modal::render_process_list(
                    f,
                    area,
                    &workspace_name,
                    &procs,
                    *selected,
                    input.as_deref(),
                    notice.as_deref(),
                    &app.theme,
                );
            }
```

- [ ] **Step 3: Verify it compiles and tests still pass**

Run: `cargo build && cargo test process_command_tests`
Expected: builds clean; 4 tests PASS.

- [ ] **Step 4: Render smoke test (optional but recommended)**

Add to `src/app/input_tests.rs` `process_command_tests` module, mirroring the existing `TestBackend` render tests in that file:

```rust
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn footer_shows_run_hint_in_list_mode() {
        let theme = crate::ui::theme::Theme::default();
        let backend = TestBackend::new(80, 24);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            crate::ui::modal::render_process_list(
                f,
                f.area(),
                "demo",
                &[],
                0,
                None,
                None,
                &theme,
            );
        })
        .unwrap();
        let buf = term.backend().buffer();
        let rendered = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("[r] run"), "{rendered}");
    }
```

If `Theme::default()` or `f.area()` differ from the codebase's conventions, match the existing render tests in `input_tests.rs` (they construct the theme via `App::new(...).theme` and use `draw_for_test`). Prefer copying that file's established pattern over the snippet above if they diverge.

Run: `cargo test process_command_tests`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ui/modal.rs src/app/render.rs src/app/input_tests.rs
git commit -m "feat(processes): render run-command input line, footer hint, and notice"
```

---

## Task 5: Docs and manual verification

**Files:**
- Modify: `README.md` (process-tracking / dashboard keybindings section)

- [ ] **Step 1: Document the new action**

Find the process-tracking section in `README.md` (search for "Process tracking" or the processes-modal keybindings). Add a sentence describing the new action, matching the surrounding prose style, e.g.:

> Press `r` in the processes modal to run a command in the workspace's worktree. It launches as a background process (output captured to a log file under `~/.local/state/wsx/logs/`) and appears in the same list, where `K` kills it.

- [ ] **Step 2: Full test + lint pass**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: all tests pass; no clippy errors. Fix any clippy findings (e.g. needless clones) before continuing.

- [ ] **Step 3: Manual smoke test**

Run wsx, open the dashboard, select a workspace, press `Shift+K` to open the processes modal. Then:
1. Press `r` — the footer becomes a `run:` input line.
2. Type `sleep 300` and press Enter — the notice shows `▶ started → …/logs/wsN-<ts>.log`.
3. Confirm a `sleep` process appears in the list (it may take until the next scan; navigate with arrows).
4. Press `K` on it — it dies and disappears on rescan.
5. `cat` the printed log path to confirm output capture (try `echo hello` as the command and check the log contains `hello`).

- [ ] **Step 4: Commit**

```bash
git add README.md
git commit -m "docs(processes): document running a command from the processes modal"
```

---

## Self-review notes

- **Spec coverage:** §1 interaction → Task 3 (keys) + Task 4 (render). §2 modal state → Task 3 Step 1. §3 spawn → Task 2. §4 logging → Task 1 (`background_log_path`) + Task 3 (`launch_workspace_command` wiring). §5 wiring → Task 3 Step 5. §6 rendering → Task 4. §7 error handling → Task 2 (empty/IO errors) + Task 3 (empty no-op, workspace-not-found notice). §8 testing → Task 1 + Task 3 + Task 4. Known nuance (npm→node) is informational; no task needed.
- **Type consistency:** `WorkspaceId(pub i64)` → `.0` passed as `i64` to `background_log_path`. `Modal::ProcessList` fields `input: Option<String>` / `notice: Option<String>` are consistent across modal.rs, input.rs, render.rs (passed as `Option<&str>` via `.as_deref()`). `handle_key_modal(&mut App, &SharedApp, KeyEvent)` matches the test calls. `spawn_background_command(&Path, &str, &Path) -> Result<()>` matches its caller.
- **No placeholders:** every code step shows complete code; the one conditional ("if Theme::default differs…") points to a concrete existing pattern in the same file.
