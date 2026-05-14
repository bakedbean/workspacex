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
    Fresh {
        rename_ctx: Option<RenameContext>,
        custom_instructions: Option<String>,
    },
    /// Resume the most recent prior session in this worktree via `--continue`.
    Continue { custom_instructions: Option<String> },
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
pub fn build_claude_command(cwd: &Path, mode: &SpawnMode) -> CommandBuilder {
    let bin = std::env::var("WSX_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
    let mut cmd = CommandBuilder::new(bin);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    let (rename_prompt, custom, allow_git_branch) = match mode {
        SpawnMode::Continue {
            custom_instructions,
        } => {
            cmd.arg("--continue");
            (None, custom_instructions.clone(), false)
        }
        SpawnMode::Fresh {
            rename_ctx,
            custom_instructions,
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
            (rp, custom_instructions.clone(), allow)
        }
    };

    if allow_git_branch {
        cmd.arg("--allowedTools");
        cmd.arg("Bash(git branch:*)");
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

pub fn spawn_session(cwd: &Path, cols: u16, rows: u16, mode: SpawnMode) -> Result<Session> {
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
        .spawn_command(build_claude_command(cwd, &mode))
        .map_err(|e| Error::Pty(format!("spawn: {e}")))?;
    drop(pair.slave);

    let killer = child.clone_killer();
    let pid = child.process_id().unwrap_or(0);
    let parser = Arc::new(Mutex::new(Parser::new(rows, cols, 1000)));
    let status = Arc::new(RwLock::new(SessionStatus::Running { pid }));
    let activity_ms = Arc::new(AtomicU64::new(now_ms()));

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
        }
    }

    pub fn spawn(
        &mut self,
        id: WorkspaceId,
        cwd: &Path,
        cols: u16,
        rows: u16,
        mode: SpawnMode,
    ) -> Result<Arc<Session>> {
        if let Some(s) = self.sessions.get(&id) {
            if matches!(*s.status.read().unwrap(), SessionStatus::Running { .. }) {
                return Ok(s.clone());
            }
            // Otherwise fall through and respawn.
        }
        let session = Arc::new(spawn_session(cwd, cols, rows, mode)?);
        self.sessions.insert(id, session.clone());
        Ok(session)
    }

    pub fn get(&self, id: WorkspaceId) -> Option<Arc<Session>> {
        self.sessions.get(&id).cloned()
    }

    pub fn kill_all(&mut self) {
        for s in self.sessions.values() {
            s.kill();
        }
        self.sessions.clear();
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
            },
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
            },
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
                },
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
            },
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
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode);
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
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode);
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
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(&cwd, &mode);
        let argv = cmd.get_argv();
        assert!(
            !argv
                .iter()
                .any(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
        );
        assert!(!argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
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
}
