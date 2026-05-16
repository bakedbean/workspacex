#![allow(clippy::collapsible_if, clippy::arc_with_non_send_sync)]

use crate::error::{Error, Result};
use crate::store::WorkspaceId;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc;
use vt100::Parser;

/// True if Claude Code has a persisted session JSONL for this worktree.
/// Claude Code stores sessions at `~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`,
/// where the encoding replaces `/` and `.` with `-`.
pub fn has_prior_session(worktree: &Path) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let abs = match std::fs::canonicalize(worktree) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let encoded = abs.to_string_lossy().replace(['/', '.'], "-");
    let session_dir = home.join(".claude/projects").join(encoded);
    if !session_dir.is_dir() {
        return false;
    }
    std::fs::read_dir(&session_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .any(|e| e.path().extension().map(|x| x == "jsonl").unwrap_or(false))
        })
        .unwrap_or(false)
}

#[derive(Default)]
pub struct PromptCapture {
    buffer: String,
    pub done: bool,
}

#[derive(Debug, Clone)]
pub enum SessionStatus {
    Running { pid: u32 },
    Exited { code: i32 },
}

pub struct Session {
    pub parser: Arc<Mutex<Parser>>,
    pub writer: mpsc::Sender<Vec<u8>>,
    pub status: Arc<RwLock<SessionStatus>>,
    pub activity_ms: Arc<AtomicU64>,
    /// Rows back from live tail. 0 = live. The render path calls
    /// `parser.set_scrollback(offset)` before reading `parser.screen()`,
    /// so vt100 clamps to whatever scrollback actually exists.
    pub scrollback_offset: std::sync::atomic::AtomicUsize,
    // Wrapped in Mutex so Session is Sync — required because App is held in
    // an Arc<tokio::sync::Mutex<App>> that gets passed to `tokio::spawn` for
    // the branch-drift poller.
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
    pub prompt: Arc<Mutex<PromptCapture>>,
}

impl Session {
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .lock()
            .unwrap()
            .resize(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| Error::Pty(format!("resize: {e}")))?;
        self.parser.lock().unwrap().set_size(rows, cols);
        Ok(())
    }

    /// Send SIGKILL (or platform equivalent) to the child process.
    /// Idempotent; safe to call multiple times.
    pub fn kill(&self) {
        let _ = self.killer.lock().unwrap().kill();
    }

    pub fn scroll_up(&self, rows: usize) {
        use std::sync::atomic::Ordering;
        let cur = self.scrollback_offset.load(Ordering::Relaxed);
        self.scrollback_offset
            .store(cur.saturating_add(rows), Ordering::Relaxed);
    }

    pub fn scroll_down(&self, rows: usize) {
        use std::sync::atomic::Ordering;
        let cur = self.scrollback_offset.load(Ordering::Relaxed);
        self.scrollback_offset
            .store(cur.saturating_sub(rows), Ordering::Relaxed);
    }

    pub fn scroll_to_live(&self) {
        self.scrollback_offset
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn is_scrolled(&self) -> bool {
        self.scrollback_offset
            .load(std::sync::atomic::Ordering::Relaxed)
            > 0
    }

    pub fn capture_char(&self, c: char) {
        let mut p = self.prompt.lock().unwrap();
        if !p.done && p.buffer.chars().count() < 200 {
            p.buffer.push(c);
        }
    }

    pub fn capture_backspace(&self) {
        let mut p = self.prompt.lock().unwrap();
        if !p.done {
            p.buffer.pop();
        }
    }

    /// Take the captured prompt and mark capture as done. Returns None if
    /// already taken or buffer is empty/whitespace.
    pub fn take_first_prompt(&self) -> Option<String> {
        let mut p = self.prompt.lock().unwrap();
        if p.done {
            return None;
        }
        let text = std::mem::take(&mut p.buffer);
        if text.trim().is_empty() {
            None // don't latch — let next Enter try again
        } else {
            p.done = true;
            Some(text)
        }
    }

    /// Write `text` (with a trailing `\r`) to the PTY after the activity
    /// stream has been quiet for `quiet_ms` milliseconds following some
    /// output. If the overall window of `timeout_ms` elapses without ever
    /// seeing both output AND a quiet window, log a warning and return
    /// without writing.
    ///
    /// Used to gate the PM auto-summary message on claude having finished
    /// rendering its banner + input prompt.
    pub async fn send_text_when_settled(&self, text: &str, quiet_ms: u64, timeout_ms: u64) {
        use std::sync::atomic::Ordering;
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_millis(timeout_ms);
        loop {
            if start.elapsed() >= timeout {
                tracing::warn!(
                    text = %text,
                    "send_text_when_settled: timed out waiting for PTY to settle"
                );
                return;
            }
            let last = self.activity_ms.load(Ordering::Relaxed);
            if last > 0 {
                let now_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let since_last = now_ms.saturating_sub(last);
                if since_last >= quiet_ms {
                    // Two writes so claude's TUI sees the text as typed input
                    // and the trailing CR as a separate Enter (submit). A
                    // single payload "<text>\r" can look like a bracketed
                    // paste and not auto-submit.
                    let _ = self.writer.send(text.as_bytes().to_vec()).await;
                    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                    let _ = self.writer.send(b"\r".to_vec()).await;
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.killer.lock().unwrap().kill();
    }
}

/// Context the claude-mode auto-rename system prompt needs to address the
/// worktree by branch name. Passed into `build_claude_command` only when
/// the workspace name is still a generated slug.
#[derive(Debug, Clone)]
pub struct RenameContext {
    pub current_branch: String,
    pub branch_prefix: String, // empty if no prefix
}

/// How to spawn the claude process for a workspace.
#[derive(Debug, Clone)]
pub enum SpawnMode {
    /// Brand-new session. Apply rename system prompt if context provided.
    /// `yolo` adds `--dangerously-skip-permissions`.
    Fresh {
        rename_ctx: Option<RenameContext>,
        custom_instructions: Option<String>,
        yolo: bool,
    },
    /// Resume the most recent prior session in this worktree via `--continue`.
    /// `yolo` adds `--dangerously-skip-permissions`.
    Continue {
        custom_instructions: Option<String>,
        yolo: bool,
    },
    /// Spawn the project-manager session. Embeds the PM system prompt and
    /// a read-only tool allowlist. When `resume` is true, also passes
    /// `--continue` to pick up PM's prior conversation. Always uses
    /// `--dangerously-skip-permissions`.
    ProjectManager {
        workspaces_json_path: std::path::PathBuf,
        custom_instructions: Option<String>,
        resume: bool,
    },
}

/// Build a `CommandBuilder` for `claude` (or whatever `WSX_CLAUDE_BIN`
/// points to) inside `cwd`. Inherits the current process env.
///
/// When `mode` is `Fresh { rename_ctx: Some(_) }` and `WSX_RENAME_MODE` is
/// `claude` (the default), appends a system-prompt instruction directing
/// claude to rename the branch based on the user's first message, plus
/// pre-authorizes `Bash(git branch:*)` so the rename runs without a
/// permission prompt. When `mode` is `Continue`, passes `--continue` so
/// claude resumes the most recent persisted session for this worktree.
pub fn build_claude_command(
    cwd: &Path,
    mode: &SpawnMode,
    remote: crate::remote::RemoteOpts,
) -> CommandBuilder {
    let bin = std::env::var("WSX_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    let (rename_prompt, custom, allow_git_branch, add_continue, skip_permissions) = match mode {
        SpawnMode::Continue {
            custom_instructions,
            yolo,
        } => (None, custom_instructions.clone(), false, true, *yolo),
        SpawnMode::Fresh {
            rename_ctx,
            custom_instructions,
            yolo,
        } => {
            let rename_mode =
                std::env::var("WSX_RENAME_MODE").unwrap_or_else(|_| "claude".to_string());
            let (rp, allow) = if let Some(ctx) = rename_ctx {
                if rename_mode == "claude" {
                    (
                        Some(render_rename_system_prompt(
                            &ctx.current_branch,
                            &ctx.branch_prefix,
                        )),
                        true,
                    )
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };
            (rp, custom_instructions.clone(), allow, false, *yolo)
        }
        SpawnMode::ProjectManager {
            workspaces_json_path: _,
            custom_instructions,
            resume,
        } => (
            Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
            None,
            false,
            *resume,
            true,
        ),
    };

    if add_continue {
        cmd.arg("--continue");
    }

    if skip_permissions {
        cmd.arg("--dangerously-skip-permissions");
    } else if allow_git_branch {
        cmd.arg("--allowedTools");
        cmd.arg("Bash(git branch:*)");
    }

    if remote.enabled {
        cmd.arg("--remote-control");
        if remote.sandbox {
            cmd.arg("--sandbox");
        }
    }

    let combined = match (rename_prompt, custom) {
        (None, None) => None,
        (Some(r), None) => Some(r),
        (None, Some(c)) => Some(c),
        (Some(r), Some(c)) => Some(format!("{r}\n\n{c}")),
    };

    if let Some(prompt) = combined {
        cmd.arg("--append-system-prompt");
        cmd.arg(prompt);
    }

    cmd
}

fn render_rename_system_prompt(current_branch: &str, branch_prefix: &str) -> String {
    let prefix = if branch_prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", branch_prefix.trim_end_matches('/'))
    };
    format!(
        "This is a wsx-managed worktree currently checked out on a placeholder branch \
         named `{current_branch}`. The placeholder is a randomly-generated \
         adjective-plant slug from the wsx workspace manager.\n\n\
         BEFORE doing the work the user asks about, on their first message: \
         run `git branch -m {current_branch} {prefix}<slug>` where `<slug>` is a \
         2-4 word lowercase kebab-case summary of what the user is asking for. \
         Then briefly tell the user \"renamed branch to {prefix}<slug>\" on one line \
         and proceed with their actual request.\n\n\
         Constraints:\n\
         - Keep the `{prefix}` prefix exactly as shown.\n\
         - Slug: lowercase, 2-4 words, hyphen-separated, max ~32 chars.\n\
         - Don't ask for confirmation; don't add extra explanation.\n\
         - Only do this once per worktree. If the current branch is no longer \
         the placeholder `{current_branch}`, skip the rename — it's already done.\n"
    )
}

pub fn spawn_session(
    cwd: &Path,
    cols: u16,
    rows: u16,
    mode: SpawnMode,
    remote: crate::remote::RemoteOpts,
) -> Result<Session> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| Error::Pty(format!("openpty: {e}")))?;

    let mut child = pair
        .slave
        .spawn_command(build_claude_command(cwd, &mode, remote))
        .map_err(|e| Error::Pty(format!("spawn: {e}")))?;
    drop(pair.slave);

    let killer = child.clone_killer();
    let pid = child.process_id().unwrap_or(0);
    let parser = Arc::new(Mutex::new(Parser::new(rows, cols, 1000)));
    let status = Arc::new(RwLock::new(SessionStatus::Running { pid }));
    let activity_ms = Arc::new(AtomicU64::new(0));

    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(64);

    // Reader thread (blocking I/O on PTY master clone).
    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| Error::Pty(format!("clone reader: {e}")))?;
    let parser_r = parser.clone();
    let activity_r = activity_ms.clone();
    let status_r = status.clone();
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    parser_r.lock().unwrap().process(&buf[..n]);
                    activity_r.store(now_ms(), Ordering::Relaxed);
                }
                Err(_) => break,
            }
        }
        // Wait for child exit so we can capture the exit code.
        if let Ok(exit) = child.wait() {
            let code = exit.exit_code() as i32;
            *status_r.write().unwrap() = SessionStatus::Exited { code };
        } else {
            *status_r.write().unwrap() = SessionStatus::Exited { code: -1 };
        }
    });

    // Writer task on tokio.
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|e| Error::Pty(format!("take writer: {e}")))?;
    tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if writer.write_all(&bytes).is_err() {
                break;
            }
            let _ = writer.flush();
        }
    });

    let prompt = Arc::new(Mutex::new(PromptCapture::default()));

    Ok(Session {
        parser,
        writer: tx,
        status,
        activity_ms,
        scrollback_offset: std::sync::atomic::AtomicUsize::new(0),
        master: Mutex::new(pair.master),
        killer: Mutex::new(killer),
        prompt,
    })
}

fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct SessionManager {
    sessions: HashMap<WorkspaceId, Arc<Session>>,
    pm: Option<Arc<Session>>,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            pm: None,
        }
    }

    pub fn spawn(
        &mut self,
        id: WorkspaceId,
        cwd: &Path,
        cols: u16,
        rows: u16,
        mode: SpawnMode,
        remote: crate::remote::RemoteOpts,
    ) -> Result<Arc<Session>> {
        if let Some(s) = self.sessions.get(&id) {
            if matches!(*s.status.read().unwrap(), SessionStatus::Running { .. }) {
                return Ok(s.clone());
            }
            // Otherwise fall through and respawn.
        }
        let session = Arc::new(spawn_session(cwd, cols, rows, mode, remote)?);
        self.sessions.insert(id, session.clone());
        Ok(session)
    }

    pub fn get(&self, id: WorkspaceId) -> Option<Arc<Session>> {
        self.sessions.get(&id).cloned()
    }

    pub fn spawn_pm(
        &mut self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        mode: SpawnMode,
        remote: crate::remote::RemoteOpts,
    ) -> Result<Arc<Session>> {
        if let Some(existing) = &self.pm {
            if matches!(
                *existing.status.read().unwrap(),
                SessionStatus::Running { .. }
            ) {
                return Ok(existing.clone());
            }
        }
        let session = Arc::new(spawn_session(cwd, cols, rows, mode, remote)?);
        self.pm = Some(session.clone());
        Ok(session)
    }

    pub fn pm(&self) -> Option<Arc<Session>> {
        self.pm.clone()
    }

    pub fn kill_all(&mut self) {
        for s in self.sessions.values() {
            s.kill();
        }
        self.sessions.clear();
        if let Some(pm) = &self.pm {
            pm.kill();
        }
        self.pm = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn echo_bin() -> &'static str {
        if std::path::Path::new("/usr/bin/cat").exists() {
            "/usr/bin/cat"
        } else {
            "cat"
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_and_echo() {
        // Substitute claude with `cat` via the env-var seam.
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", echo_bin());
        }
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                yolo: false,
            },
            crate::remote::RemoteOpts::disabled(),
        )
        .unwrap();
        s.writer.send(b"hello\n".to_vec()).await.unwrap();
        // Give cat a moment to echo and the reader to process.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("hello"), "screen contents: {screen:?}");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_binary_returns_pty_error() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/no/such/binary/wsx-test");
        }
        let cwd = PathBuf::from(".");
        let result = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                yolo: false,
            },
            crate::remote::RemoteOpts::disabled(),
        );
        assert!(result.is_err());
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn kill_all_terminates_child() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "sleep");
        }
        // sleep needs an arg; we use sh as a wrapper instead.
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/bin/sh");
        }
        let cwd = std::path::PathBuf::from(".");
        let mut mgr = SessionManager::new();
        let id = crate::store::WorkspaceId(1);
        let session = mgr
            .spawn(
                id,
                &cwd,
                80,
                24,
                SpawnMode::Fresh {
                    rename_ctx: None,
                    custom_instructions: None,
                    yolo: false,
                },
                crate::remote::RemoteOpts::disabled(),
            )
            .unwrap();
        // sh -i would run forever; we just check the session was Running.
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(matches!(
            *session.status.read().unwrap(),
            SessionStatus::Running { .. }
        ));
        mgr.kill_all();
        // Give the reader thread time to observe the kill and update status.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            matches!(
                *session.status.read().unwrap(),
                SessionStatus::Exited { .. }
            ),
            "expected Exited after kill_all, got {:?}",
            *session.status.read().unwrap()
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_enter_does_not_latch_prompt_capture() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = std::path::PathBuf::from(".");
        let session = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                yolo: false,
            },
            crate::remote::RemoteOpts::disabled(),
        )
        .unwrap();

        // First "Enter" before typing — must NOT latch.
        assert!(session.take_first_prompt().is_none());

        // Now type and submit — must capture and return.
        for c in "hello!".chars() {
            session.capture_char(c);
        }
        assert_eq!(session.take_first_prompt().as_deref(), Some("hello!"));

        // After a successful take, further calls latch correctly.
        session.capture_char('x');
        assert!(session.take_first_prompt().is_none());

        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[test]
    fn system_prompt_combines_rename_and_custom() {
        let ctx = RenameContext {
            current_branch: "wsx/bold-fern".into(),
            branch_prefix: "wsx".into(),
        };
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions: Some("Use tabs not spaces".into()),
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("--append-system-prompt should be present");
        let prompt = argv
            .get(idx + 1)
            .expect("system prompt value should follow")
            .to_string_lossy();
        assert!(
            prompt.contains("git branch -m wsx/bold-fern"),
            "rename block missing"
        );
        assert!(
            prompt.contains("Use tabs not spaces"),
            "custom instructions missing"
        );
        let rename_pos = prompt.find("git branch -m").unwrap();
        let custom_pos = prompt.find("Use tabs not spaces").unwrap();
        assert!(
            custom_pos > rename_pos,
            "custom instructions must come after rename block"
        );
    }

    #[test]
    fn system_prompt_continue_passes_custom_only() {
        let mode = SpawnMode::Continue {
            custom_instructions: Some("Use ruff".into()),
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("--append-system-prompt should be present");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        assert!(prompt.contains("Use ruff"));
        assert!(
            !prompt.contains("git branch -m"),
            "rename should not appear on Continue"
        );
    }

    #[test]
    fn system_prompt_omitted_when_nothing_to_say() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(
            !argv
                .iter()
                .any(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
        );
        assert!(!argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
    }

    #[test]
    fn yolo_fresh_emits_skip_permissions() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: true,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--dangerously-skip-permissions")),
            "expected --dangerously-skip-permissions for yolo Fresh"
        );
    }

    #[test]
    fn yolo_continue_emits_skip_permissions() {
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            yolo: true,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--dangerously-skip-permissions")),
            "expected --dangerously-skip-permissions for yolo Continue"
        );
    }

    #[test]
    fn non_yolo_fresh_omits_skip_permissions() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(
            !argv
                .iter()
                .any(|a| a == std::ffi::OsStr::new("--dangerously-skip-permissions")),
            "non-yolo Fresh must not emit skip-permissions"
        );
    }

    #[test]
    fn rename_prompt_includes_current_branch_and_prefix() {
        let p = render_rename_system_prompt("wsx/bold-fern", "wsx");
        assert!(p.contains("`wsx/bold-fern`"));
        assert!(p.contains("git branch -m wsx/bold-fern wsx/<slug>"));
        assert!(p.contains("Keep the `wsx/` prefix"));
    }

    #[test]
    fn rename_prompt_handles_empty_prefix() {
        let p = render_rename_system_prompt("bold-fern", "");
        assert!(p.contains("`bold-fern`"));
        assert!(p.contains("git branch -m bold-fern <slug>"));
    }

    #[test]
    fn has_prior_session_finds_jsonl() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let encoded = abs.to_string_lossy().replace(['/', '.'], "-");
        let session_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("abc.jsonl"), "{}").unwrap();

        // Override HOME for the duration of this test.
        let original = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", home.path());
        }
        let result = has_prior_session(work.path());
        if let Some(h) = original {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
        assert!(
            result,
            "expected to find prior session at {}",
            session_dir.display()
        );
    }

    #[test]
    fn has_prior_session_returns_false_for_empty_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let original = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", home.path());
        }
        let result = has_prior_session(work.path());
        if let Some(h) = original {
            unsafe {
                std::env::set_var("HOME", h);
            }
        } else {
            unsafe {
                std::env::remove_var("HOME");
            }
        }
        assert!(!result);
    }

    #[test]
    fn project_manager_mode_adds_skip_permissions_and_system_prompt() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            resume: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--dangerously-skip-permissions"), "{dbg}");
        assert!(!dbg.contains("--allowedTools"), "{dbg}");
        assert!(dbg.contains("--append-system-prompt"), "{dbg}");
        assert!(dbg.contains("project manager"), "{dbg}");
        assert!(!dbg.contains("--continue"), "should be Fresh-style: {dbg}");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[test]
    fn project_manager_mode_resume_adds_continue() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            resume: true,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--continue"), "{dbg}");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[test]
    fn build_claude_command_appends_remote_control_when_enabled() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        let opts = crate::remote::RemoteOpts {
            enabled: true,
            sandbox: false,
        };
        let cmd = build_claude_command(&cwd, &mode, opts);
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control")),
            "expected --remote-control flag, argv: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == std::ffi::OsStr::new("--sandbox")),
            "expected no --sandbox flag, argv: {argv:?}"
        );
    }

    #[test]
    fn build_claude_command_appends_sandbox_when_enabled() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        let opts = crate::remote::RemoteOpts {
            enabled: true,
            sandbox: true,
        };
        let cmd = build_claude_command(&cwd, &mode, opts);
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control"))
        );
        assert!(argv.iter().any(|a| a == std::ffi::OsStr::new("--sandbox")));
    }

    #[test]
    fn build_claude_command_omits_remote_control_when_disabled() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            yolo: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(
            !argv
                .iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control")),
            "expected no --remote-control flag, argv: {argv:?}"
        );
        assert!(!argv.iter().any(|a| a == std::ffi::OsStr::new("--sandbox")));
    }

    #[test]
    fn build_claude_command_remote_control_applies_to_pm_mode() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            resume: false,
        };
        let opts = crate::remote::RemoteOpts {
            enabled: true,
            sandbox: false,
        };
        let cmd = build_claude_command(&cwd, &mode, opts);
        let argv = cmd.get_argv();
        assert!(
            argv.iter()
                .any(|a| a == std::ffi::OsStr::new("--remote-control")),
            "expected --remote-control in PM argv: {argv:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_text_when_settled_writes_after_quiet_window() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                yolo: false,
            },
            crate::remote::RemoteOpts::disabled(),
        )
        .unwrap();
        // Prime cat with some output so activity_ms is populated, then let it settle.
        s.writer.send(b"prime\n".to_vec()).await.unwrap();
        // The helper waits for the quiet window, then writes the payload.
        // With cat, the payload echoes back into the screen buffer.
        s.send_text_when_settled("AUTO_MSG", 200, 3_000).await;
        // Allow cat to echo.
        tokio::time::sleep(Duration::from_millis(300)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("AUTO_MSG"), "screen contents: {screen:?}");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_manager_pm_spawn_get_kill() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mut mgr = SessionManager::new();
        assert!(mgr.pm().is_none());
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/wsx-test-pm/workspaces.json"),
            custom_instructions: None,
            resume: false,
        };
        let s = mgr
            .spawn_pm(&cwd, 80, 24, mode, crate::remote::RemoteOpts::disabled())
            .unwrap();
        assert!(mgr.pm().is_some());
        // Second spawn while running is a no-op (returns existing).
        let mode2 = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/wsx-test-pm/workspaces.json"),
            custom_instructions: None,
            resume: false,
        };
        let s2 = mgr
            .spawn_pm(&cwd, 80, 24, mode2, crate::remote::RemoteOpts::disabled())
            .unwrap();
        assert!(Arc::ptr_eq(&s, &s2));
        // kill_all also kills PM.
        mgr.kill_all();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(matches!(
            *s.status.read().unwrap(),
            SessionStatus::Exited { .. }
        ));
        assert!(mgr.pm().is_none(), "kill_all should clear pm slot");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_text_when_settled_times_out_when_no_output() {
        unsafe {
            // cat with no input produces no spontaneous output, so activity_ms
            // stays 0 and the quiet-window condition is never met.
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                yolo: false,
            },
            crate::remote::RemoteOpts::disabled(),
        )
        .unwrap();
        // Do NOT send any input — cat stays silent, activity_ms never gets set.
        let start = std::time::Instant::now();
        s.send_text_when_settled("NEVER_SENT", 200, 500).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(450), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(1500), "{elapsed:?}");
        s.kill();
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    /// Construct a real PTY-backed Session for scrollback unit tests. Uses
    /// `cat` as the child so spawn succeeds without claude on the path.
    fn spawn_for_test() -> Session {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", echo_bin());
        }
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                yolo: false,
            },
            crate::remote::RemoteOpts::disabled(),
        )
        .expect("spawn_session for scrollback test");
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
        s
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_scroll_offset_starts_at_zero() {
        let s = spawn_for_test();
        assert_eq!(
            s.scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert!(!s.is_scrolled());
        s.kill();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_scroll_up_advances_offset() {
        let s = spawn_for_test();
        s.scroll_up(5);
        assert_eq!(
            s.scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            5
        );
        assert!(s.is_scrolled());
        s.kill();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_scroll_down_is_saturating() {
        let s = spawn_for_test();
        s.scroll_up(3);
        s.scroll_down(10);
        assert_eq!(
            s.scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        s.kill();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_scroll_to_live_zeroes_offset() {
        let s = spawn_for_test();
        s.scroll_up(42);
        s.scroll_to_live();
        assert_eq!(
            s.scrollback_offset
                .load(std::sync::atomic::Ordering::Relaxed),
            0
        );
        assert!(!s.is_scrolled());
        s.kill();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scrollback_offset_reveals_older_content_via_set_scrollback() {
        let s = spawn_for_test();
        // Feed enough output to overflow the 24-row screen so vt100 moves
        // rows into the scrollback buffer.
        {
            let mut p = s.parser.lock().unwrap();
            for i in 0..200 {
                p.process(format!("line {i}\r\n").as_bytes());
            }
        }
        // Live view shows the latest line.
        {
            let mut p = s.parser.lock().unwrap();
            p.set_scrollback(0);
            let live = p.screen().contents();
            assert!(live.contains("line 199"), "live should show latest: {live}");
        }
        // After scrolling back, set_scrollback should reveal older lines.
        s.scroll_up(150);
        {
            let mut p = s.parser.lock().unwrap();
            p.set_scrollback(
                s.scrollback_offset
                    .load(std::sync::atomic::Ordering::Relaxed),
            );
            let scrolled = p.screen().contents();
            assert!(
                !scrolled.contains("line 199"),
                "scrolled view must not include latest: {scrolled}"
            );
        }
        s.kill();
    }
}
