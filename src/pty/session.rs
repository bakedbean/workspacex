#![allow(clippy::collapsible_if, clippy::arc_with_non_send_sync)]

use crate::error::{Error, Result};
use portable_pty::{MasterPty, PtySize, native_pty_system};
// The command builders moved to `pty::command`, but this file's test module
// still names `CommandBuilder` when inspecting their output.
#[cfg(test)]
use portable_pty::CommandBuilder;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::mpsc;
use vt100::Parser;

// `AgentKind` now lives in its own leaf module; re-export it so the many
// `crate::pty::session::AgentKind` call sites across the codebase keep working.
pub use crate::pty::agent_kind::AgentKind;

// Prior-session detection + Hermes marker/sqlite plumbing now live in
// `session_detect`. Re-export the public surface so external callers
// (`crate::pty::session::has_prior_session_for`, …) and this file's spawn /
// command builders keep resolving the names unqualified.
pub use crate::pty::session_detect::{
    has_prior_codex_session, has_prior_hermes_session, has_prior_pi_session, has_prior_session,
    has_prior_session_for, latest_hermes_session_id_default,
};
// The marker/sqlite internals are now exercised only by this file's test
// module (via `super::`) — their production callers moved to `command` /
// `workspace_prep` — so re-export them under cfg(test) to avoid unused imports.
#[cfg(test)]
pub(crate) use crate::pty::session_detect::{
    cache_hermes_session_id_in_marker, latest_hermes_session_id, read_hermes_spawn_marker,
    write_hermes_spawn_marker,
};

// Per-agent command construction now lives in `command`. The builders are
// called by `spawn_session` below; `compose_injected_prompt` and the
// rename-prompt / shell-quote helpers are reached only by this file's test
// module, so gate those re-exports under cfg(test).
pub use crate::pty::command::{
    build_claude_command, build_codex_command, build_hermes_command, build_pi_command,
};
#[cfg(test)]
pub(crate) use crate::pty::command::{
    compose_injected_prompt, render_rename_system_prompt, render_rename_system_prompt_hermes,
    render_rename_system_prompt_pi, shell_quote,
};

// AGENTS.md / git-exclude / spawn-prep plumbing now lives in `workspace_prep`.
// `prepare_*_workspace` are called by `spawn_session` below; the AGENTS.md
// helpers and wsx-managed-block consts are reached only by this file's test
// module, so gate those re-exports under cfg(test).
#[cfg(test)]
pub(crate) use crate::pty::workspace_prep::{
    CLAUDE_PROVENANCE_COMMENT, HERMES_BLOCK_BEGIN, HERMES_BLOCK_END, ensure_git_exclude,
    write_agents_md_section,
};
pub(crate) use crate::pty::workspace_prep::{prepare_codex_workspace, prepare_hermes_workspace};

/// True if `err`'s `Display` output looks like portable-pty's
/// "binary not found on PATH" error.
///
/// Why string-matching: portable-pty constructs these errors with
/// `anyhow::bail!` and plain strings; there is no `io::Error` in the
/// chain to detect via `io::ErrorKind::NotFound`. We match against
/// the three message patterns portable-pty 0.9.0 produces in
/// `src/cmdbuilder.rs::CommandBuilder::search_path`:
///
/// - `"because it does not exist"` — cwd-relative path missing
/// - `"doesn't exist on the filesystem"` — absolute path missing
/// - `"No viable candidates found in PATH"` — PATH search exhausted
///
/// A fourth path — `"Unable to resolve the PATH"`, fired when the
/// `PATH` env var is entirely missing — is INTENTIONALLY excluded:
/// that is a system misconfiguration, not a "binary not found"
/// situation, and should surface as `Error::Pty` so the user sees
/// the real cause.
///
/// If portable-pty is bumped past 0.9.0, re-verify these patterns.
/// The `spawn_session_returns_agent_binary_missing_for_unknown_path`
/// test guards the cwd-relative branch.
fn is_binary_not_found(err: &dyn std::fmt::Display) -> bool {
    let msg = err.to_string();
    msg.contains("because it does not exist")
        || msg.contains("doesn't exist on the filesystem")
        || msg.contains("No viable candidates found in PATH")
}

/// Resolve the binary name we will attempt to spawn for `agent`, honoring
/// the `WSX_<AGENT>_BIN` env-var seam used by tests.
fn resolved_binary(agent: AgentKind) -> String {
    let env_var = match agent {
        AgentKind::Claude => "WSX_CLAUDE_BIN",
        AgentKind::Pi => "WSX_PI_BIN",
        AgentKind::Hermes => "WSX_HERMES_BIN",
        AgentKind::Codex => "WSX_CODEX_BIN",
    };
    std::env::var(env_var).unwrap_or_else(|_| agent.default_binary().to_string())
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
    /// Which agent backs this session. Drives input quirks like how an
    /// injected message is submitted (see `submit_writes`).
    pub agent: AgentKind,
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
    /// Whole seconds since this session last produced PTY output, or `None`
    /// when no output has been observed yet (`activity_ms == 0`). Callers that
    /// treat "idle-unknown" the same as "idle 0s" can `.unwrap_or(0)`; callers
    /// that must distinguish unknown from fresh output keep the `None`.
    pub fn idle_secs(&self) -> Option<u64> {
        let last = self.activity_ms.load(Ordering::Relaxed);
        if last == 0 {
            return None;
        }
        Some(crate::time::now_ms_u64().saturating_sub(last) / 1000)
    }

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

    /// Encode a wheel event for the inner program when it has mouse reporting
    /// enabled. Returns `None` when mouse mode is off, in which case the caller
    /// should fall back to wsx's own scrollback. `col`/`row` are 1-based cell
    /// coordinates relative to the pane the cursor is over.
    pub fn wheel_report_bytes(&self, up: bool, col: u16, row: u16) -> Option<Vec<u8>> {
        let p = self.parser.lock().unwrap();
        let screen = p.screen();
        if matches!(screen.mouse_protocol_mode(), vt100::MouseProtocolMode::None) {
            return None;
        }
        // Wheel-up = button 64, wheel-down = 65 (press-only -> trailing `M`).
        let cb: u16 = if up { 64 } else { 65 };
        match screen.mouse_protocol_encoding() {
            vt100::MouseProtocolEncoding::Sgr => {
                Some(format!("\x1b[<{cb};{col};{row}M").into_bytes())
            }
            // Default + Utf8: fall back to the legacy X10 single-byte triplet.
            // Proper Utf8 mode would wrap coords as UTF-8 codepoints, but no
            // agent in practice requests Utf8 (they use SGR), so that complexity
            // isn't worth it. Clamp to 223 so `32 + coord` fits in a byte; a
            // cursor past column 223 on a Utf8-mode terminal yields a slightly
            // wrong position, which beats a malformed escape sequence.
            _ => {
                let c = col.min(223) as u8;
                let r = row.min(223) as u8;
                Some(vec![0x1b, b'[', b'M', 32 + cb as u8, 32 + c, 32 + r])
            }
        }
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
                let now_ms = now_ms();
                let since_last = now_ms.saturating_sub(last);
                if since_last >= quiet_ms {
                    // Inject the text and the submitting CR as two writes (see
                    // `submit_writes` for the per-agent byte shapes). The CR is
                    // a separate write so the agent's TUI sees it as a distinct
                    // Enter rather than part of the typed/pasted text.
                    let (body, enter) = submit_writes(self.agent, text);
                    let _ = self.writer.send(body).await;
                    tokio::time::sleep(std::time::Duration::from_millis(80)).await;
                    let _ = self.writer.send(enter).await;
                    return;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

/// Build the two writes used to inject `text` into an agent and submit it:
/// `(body, enter)`, sent as separate writes by `send_text_when_settled`.
///
/// Codex's input parser does paste-burst detection: a multi-byte chunk that
/// arrives in one `read()` is treated as a paste, and a trailing CR folded into
/// that same chunk becomes a literal newline in the composer rather than an
/// Enter — so the message lands as an unsubmitted multi-line draft. This bites
/// the first message to a freshly-spawned Codex (and any time the body and CR
/// writes coalesce under load), which is exactly the "sat there until I pressed
/// Enter myself" symptom. Wrapping the body in a bracketed paste
/// (`ESC[200~ … ESC[201~`) makes the paste boundary explicit in the byte
/// stream, so the following CR is an unambiguous Enter even when the two writes
/// arrive in a single read. Other agents (Claude/Pi/Hermes) submit fine on a
/// plain `text` + CR, so they keep the simpler form and are untouched.
pub(crate) fn submit_writes(agent: AgentKind, text: &str) -> (Vec<u8>, Vec<u8>) {
    let enter = b"\r".to_vec();
    match agent {
        AgentKind::Codex => {
            let mut body = Vec::with_capacity(text.len() + 12);
            body.extend_from_slice(b"\x1b[200~");
            body.extend_from_slice(text.as_bytes());
            body.extend_from_slice(b"\x1b[201~");
            (body, enter)
        }
        AgentKind::Claude | AgentKind::Pi | AgentKind::Hermes => (text.as_bytes().to_vec(), enter),
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
    pub repo_name: String,     // wsx repo name (used by `wsx workspace rename <repo> ...`)
    pub current_slug: String,  // wsx workspace name (the stored slug, e.g., "patient-larkspur")
}

/// How to spawn the claude process for a workspace.
#[derive(Debug, Clone)]
pub enum SpawnMode {
    /// Brand-new session. Apply rename system prompt if context provided.
    /// `yolo` adds `--dangerously-skip-permissions`.
    Fresh {
        rename_ctx: Option<RenameContext>,
        custom_instructions: Option<String>,
        /// Process doctrine to inject ahead of rename/custom content.
        /// `build_spawn_info` populates this in production via
        /// `crate::agent::doctrine::resolve_effective_doctrine`. `None` means "inject
        /// no doctrine" — a real production state when the operator disables it
        /// (`process_doctrine` set to `off`/`none`/`disabled`), as well as the
        /// default in tests. It is never a placeholder to be filled in later.
        doctrine: Option<String>,
        additional_dirs: Vec<std::path::PathBuf>,
        yolo: bool,
    },
    /// Resume the most recent prior session in this worktree via `--continue`.
    /// `yolo` adds `--dangerously-skip-permissions`.
    Continue {
        custom_instructions: Option<String>,
        doctrine: Option<String>,
        additional_dirs: Vec<std::path::PathBuf>,
        yolo: bool,
    },
    /// Spawn the project-manager session. Embeds the PM system prompt and
    /// a read-only tool allowlist. When `resume` is true, also passes
    /// `--continue` to pick up PM's prior conversation. Always uses
    /// `--dangerously-skip-permissions`. When `fast_mode` is true, also
    /// passes `--settings '{"fastMode":true}'` to enable Claude Code's
    /// fast mode for this session.
    ProjectManager {
        workspaces_json_path: std::path::PathBuf,
        custom_instructions: Option<String>,
        // PM has no owning repo, so always empty. Kept for uniformity.
        additional_dirs: Vec<std::path::PathBuf>,
        resume: bool,
        fast_mode: bool,
    },
}

/// Resolve `<worktree>/.git` to the real gitdir, following a worktree-style
/// `.git` file if necessary. Returns None on missing or unparseable input.
///
/// Shared hub helper: `pty::session_detect` (Hermes spawn markers) and the
/// AGENTS.md / git-exclude plumbing below both resolve the gitdir through this.
pub(crate) fn resolve_gitdir(dot_git: &Path, worktree: &Path) -> Option<std::path::PathBuf> {
    let meta = std::fs::metadata(dot_git).ok()?;
    if meta.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    if !meta.is_file() {
        return None;
    }
    let contents = std::fs::read_to_string(dot_git).ok()?;
    let line = contents.lines().next()?;
    let rest = line.strip_prefix("gitdir:")?.trim();
    let path = std::path::PathBuf::from(rest);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(worktree.join(path))
    }
}

/// Identity of the workspace+instance a spawned agent belongs to, surfaced to
/// the child process as `WSX_WORKSPACE_ID` / `WSX_AGENT_INSTANCE_ID` so the
/// agent's `wsx agent send` can address peers and identify itself. `None` for
/// the project-manager session (which is not a workspace agent).
#[derive(Debug, Clone, Copy)]
pub struct SpawnIdentity {
    pub workspace_id: i64,
    pub instance_id: i64,
}

pub fn spawn_session(
    cwd: &Path,
    cols: u16,
    rows: u16,
    mode: SpawnMode,
    remote: crate::agent::remote_control::RemoteOpts,
    agent: AgentKind,
    identity: Option<SpawnIdentity>,
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

    let mut child_cmd = match agent {
        AgentKind::Claude => build_claude_command(cwd, &mode, remote),
        AgentKind::Pi => build_pi_command(cwd, &mode, remote),
        AgentKind::Hermes => {
            prepare_hermes_workspace(cwd, &mode);
            build_hermes_command(cwd, &mode, remote)
        }
        AgentKind::Codex => {
            prepare_codex_workspace(cwd, &mode);
            build_codex_command(cwd, &mode, remote)
        }
    };
    if let Some(id) = identity {
        child_cmd.env("WSX_WORKSPACE_ID", id.workspace_id.to_string());
        child_cmd.env("WSX_AGENT_INSTANCE_ID", id.instance_id.to_string());
    }
    let mut child = pair.slave.spawn_command(child_cmd).map_err(|e| {
        if is_binary_not_found(&e) {
            Error::AgentBinaryMissing(resolved_binary(agent))
        } else {
            Error::Pty(format!("spawn: {e}"))
        }
    })?;
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
        agent,
        scrollback_offset: std::sync::atomic::AtomicUsize::new(0),
        master: Mutex::new(pair.master),
        killer: Mutex::new(killer),
        prompt,
    })
}

fn now_ms() -> u64 {
    crate::time::now_ms_u64()
}

pub struct SessionManager {
    sessions: HashMap<crate::data::store::AgentInstanceId, Arc<Session>>,
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

    // Spawning a session genuinely needs all these inputs; bundling them into a
    // params struct would not improve clarity here.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        &mut self,
        id: crate::data::store::AgentInstanceId,
        workspace_id: crate::data::store::WorkspaceId,
        cwd: &Path,
        cols: u16,
        rows: u16,
        mode: SpawnMode,
        remote: crate::agent::remote_control::RemoteOpts,
        agent: AgentKind,
    ) -> Result<Arc<Session>> {
        if let Some(s) = self.sessions.get(&id) {
            if matches!(*s.status.read().unwrap(), SessionStatus::Running { .. }) {
                return Ok(s.clone());
            }
            // Otherwise fall through and respawn.
        }
        let identity = Some(SpawnIdentity {
            workspace_id: workspace_id.0,
            instance_id: id.0,
        });
        let session = Arc::new(spawn_session(
            cwd, cols, rows, mode, remote, agent, identity,
        )?);
        self.sessions.insert(id, session.clone());
        Ok(session)
    }

    pub fn get(&self, id: crate::data::store::AgentInstanceId) -> Option<Arc<Session>> {
        self.sessions.get(&id).cloned()
    }

    pub fn remove(&mut self, id: crate::data::store::AgentInstanceId) {
        if let Some(s) = self.sessions.remove(&id) {
            s.kill();
        }
    }

    pub fn spawn_pm(
        &mut self,
        cwd: &Path,
        cols: u16,
        rows: u16,
        mode: SpawnMode,
        remote: crate::agent::remote_control::RemoteOpts,
        agent: AgentKind,
    ) -> Result<Arc<Session>> {
        if let Some(existing) = &self.pm {
            if matches!(
                *existing.status.read().unwrap(),
                SessionStatus::Running { .. }
            ) {
                return Ok(existing.clone());
            }
        }
        let session = Arc::new(spawn_session(cwd, cols, rows, mode, remote, agent, None)?);
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
    use crate::test_support::{EnvGuard, cat_path};
    use std::path::PathBuf;
    use std::time::Duration;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_and_echo() {
        // Substitute the agent binary with a wrapper that ignores args and cats
        // stdin. Codex Fresh now injects `-c notify=...` for status reporting,
        // which bare `cat` would reject, so we can't use `cat_path()` directly.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Codex,
            None,
        )
        .unwrap();
        s.writer.send(b"hello\n".to_vec()).await.unwrap();
        // Give cat a moment to echo and the reader to process.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("hello"), "screen contents: {screen:?}");
    }

    // Validates the contract under test (portable-pty rejects a missing
    // binary at spawn time) directly, without driving the env-var seam.
    // Env-var-driven tests in this file use `EnvGuard` from
    // `test_support` to serialize against sibling tests across the crate
    // that mutate the same process-global vars; see
    // `spawn_session_returns_agent_binary_missing_for_unknown_path` for an
    // example.
    #[test]
    fn missing_binary_returns_pty_error() {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                cols: 80,
                rows: 24,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");
        let cmd = CommandBuilder::new("/no/such/binary/wsx-test");
        let result = pair.slave.spawn_command(cmd);
        assert!(
            result.is_err(),
            "spawn_command must error when the binary doesn't exist"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn kill_all_terminates_child() {
        // Use AgentKind::Codex with an arg-ignoring wrapper that execs cat,
        // because Codex Fresh/Continue now injects `-c notify=...` for status
        // reporting which bare `cat` would reject. The wrapper preserves the
        // behavior we rely on: cat stays alive reading stdin so we can verify
        // kill_all actually terminates it.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = std::path::PathBuf::from(".");
        let mut mgr = SessionManager::new();
        let id = crate::data::store::AgentInstanceId(1);
        let ws_id = crate::data::store::WorkspaceId(1);
        let session = mgr
            .spawn(
                id,
                ws_id,
                &cwd,
                80,
                24,
                SpawnMode::Fresh {
                    rename_ctx: None,
                    custom_instructions: None,
                    doctrine: None,
                    additional_dirs: vec![],
                    yolo: false,
                },
                crate::agent::remote_control::RemoteOpts::disabled(),
                AgentKind::Codex,
            )
            .unwrap();
        // cat reads stdin forever — the spawn stays alive so we can verify
        // kill_all actually terminates it.
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
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn empty_enter_does_not_latch_prompt_capture() {
        // Codex stub: use an arg-ignoring wrapper that execs cat, because
        // Codex Fresh/Continue now injects `-c notify=...` for status reporting
        // which bare `cat` would reject. The wrapper starts cat cleanly.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = std::path::PathBuf::from(".");
        let session = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Codex,
            None,
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
    }

    #[test]
    fn system_prompt_combines_rename_and_custom() {
        let ctx = RenameContext {
            current_branch: "wsx/bold-fern".into(),
            branch_prefix: "wsx".into(),
            repo_name: "myrepo".into(),
            current_slug: "bold-fern".into(),
        };
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions: Some("Use tabs not spaces".into()),
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
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
            prompt.contains("wsx workspace rename 'myrepo' 'bold-fern'"),
            "rename block missing"
        );
        assert!(
            prompt.contains("Use tabs not spaces"),
            "custom instructions missing"
        );
        let rename_pos = prompt.find("wsx workspace rename").unwrap();
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
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(argv.iter().any(|a| a == std::ffi::OsStr::new("--continue")));
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("--append-system-prompt should be present");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        assert!(prompt.contains("Use ruff"));
        assert!(
            !prompt.contains("wsx workspace rename"),
            "rename should not appear on Continue"
        );
    }

    #[test]
    fn rename_mode_pre_authorizes_wsx_workspace_rename_tool() {
        let ctx = RenameContext {
            current_branch: "wsx/bold-fern".into(),
            branch_prefix: "wsx".into(),
            repo_name: "myrepo".into(),
            current_slug: "bold-fern".into(),
        };
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(ctx),
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--allowedTools"))
            .expect("--allowedTools should be present when rename_ctx is set and yolo=false");
        let value = argv
            .get(idx + 1)
            .expect("value should follow --allowedTools")
            .to_string_lossy();
        assert_eq!(
            value, "Bash(wsx workspace rename:*)",
            "expected wsx-workspace-rename pre-authorization, got: {value}"
        );
    }

    #[test]
    fn system_prompt_omitted_when_nothing_to_say() {
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
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
            doctrine: None,
            additional_dirs: vec![],
            yolo: true,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
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
            doctrine: None,
            additional_dirs: vec![],
            yolo: true,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
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
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cwd = std::path::PathBuf::from(".");
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
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
        let p = render_rename_system_prompt("wsx/bold-fern", "wsx", "myrepo", "bold-fern");
        assert!(p.contains("`wsx/bold-fern`"));
        assert!(p.contains("wsx workspace rename 'myrepo' 'bold-fern' <slug>"));
        // No "Keep the prefix" constraint — wsx handles that automatically.
        assert!(!p.contains("Keep the `wsx/` prefix"));
    }

    #[test]
    fn rename_prompt_handles_empty_prefix() {
        let p = render_rename_system_prompt("bold-fern", "", "myrepo", "bold-fern");
        assert!(p.contains("`bold-fern`"));
        assert!(p.contains("wsx workspace rename 'myrepo' 'bold-fern' <slug>"));
    }

    #[test]
    fn render_rename_prompt_hermes_includes_branch_and_prefix() {
        let prompt = super::render_rename_system_prompt_hermes(
            "wsx/bold-fern",
            "wsx",
            "myrepo",
            "bold-fern",
        );
        assert!(prompt.contains("wsx workspace rename 'myrepo' 'bold-fern'"));
        // No "Keep the prefix" constraint — wsx handles that automatically.
        assert!(!prompt.contains("Keep the `wsx/` prefix"));
    }

    #[test]
    fn render_rename_prompt_hermes_handles_empty_prefix() {
        let prompt =
            super::render_rename_system_prompt_hermes("bold-fern", "", "myrepo", "bold-fern");
        assert!(prompt.contains("wsx workspace rename 'myrepo' 'bold-fern'"));
        assert!(
            !prompt.contains("//"),
            "prompt should not contain double-slash: {prompt}"
        );
    }

    #[test]
    fn render_rename_prompt_hermes_matches_pi_today() {
        let hermes = super::render_rename_system_prompt_hermes("wsx/x", "wsx", "myrepo", "x");
        let pi = super::render_rename_system_prompt_pi("wsx/x", "wsx", "myrepo", "x");
        assert_eq!(hermes, pi, "drift between hermes and pi rename prompts");
    }

    #[test]
    fn has_prior_session_finds_jsonl() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let encoded = crate::activity::events::encode_cwd(&abs);
        let session_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("abc.jsonl"), "{}").unwrap();

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let result = has_prior_session(work.path());
        assert!(
            result,
            "expected to find prior session at {}",
            session_dir.display()
        );
    }

    #[test]
    fn has_prior_session_finds_jsonl_for_path_with_space() {
        // Regression: a repo whose name contains a space (e.g. "meals backend")
        // yields a worktree path with a space. The encoder must map it to '-'
        // to match the real ~/.claude/projects directory Claude writes.
        let home = tempfile::TempDir::new().unwrap();
        let parent = tempfile::TempDir::new().unwrap();
        let work = parent.path().join("meals backend");
        std::fs::create_dir_all(&work).unwrap();
        let abs = std::fs::canonicalize(&work).unwrap();
        let encoded = crate::activity::events::encode_cwd(&abs);
        let session_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&session_dir).unwrap();
        std::fs::write(session_dir.join("abc.jsonl"), "{}").unwrap();

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        assert!(
            has_prior_session(&work),
            "expected to find prior session at {}",
            session_dir.display()
        );
    }

    #[test]
    fn has_prior_session_returns_false_for_empty_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let result = has_prior_session(work.path());
        assert!(!result);
    }

    #[test]
    fn project_manager_mode_adds_skip_permissions_and_system_prompt() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--dangerously-skip-permissions"), "{dbg}");
        assert!(!dbg.contains("--allowedTools"), "{dbg}");
        assert!(dbg.contains("--append-system-prompt"), "{dbg}");
        assert!(dbg.contains("project manager"), "{dbg}");
        assert!(!dbg.contains("--continue"), "should be Fresh-style: {dbg}");
    }

    #[test]
    fn project_manager_mode_resume_adds_continue() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: true,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let dbg = format!("{cmd:?}");
        assert!(dbg.contains("--continue"), "{dbg}");
    }

    #[test]
    fn project_manager_mode_emits_settings_when_fast_mode() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: true,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--settings"))
            .expect("expected --settings flag when fast_mode is true");
        let value = argv
            .get(idx + 1)
            .expect("expected JSON value after --settings")
            .to_string_lossy();
        assert_eq!(value, r#"{"fastMode":true}"#);
    }

    #[test]
    fn project_manager_mode_omits_settings_when_fast_mode_false() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        assert!(
            !argv.iter().any(|a| a == std::ffi::OsStr::new("--settings")),
            "expected no --settings flag when fast_mode is false, argv: {argv:?}"
        );
    }

    #[test]
    fn fresh_mode_emits_status_hooks_via_settings() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--settings"))
            .expect("Fresh mode should emit --settings for status hooks");
        let value = argv
            .get(idx + 1)
            .expect("expected JSON value after --settings")
            .to_string_lossy();
        let v: serde_json::Value =
            serde_json::from_str(&value).expect("--settings value should be valid JSON");
        assert!(
            v["hooks"]["Stop"].is_array(),
            "expected hooks.Stop array, got: {v}"
        );
        assert!(
            v["hooks"]["UserPromptSubmit"].is_array(),
            "expected hooks.UserPromptSubmit array, got: {v}"
        );
        // fastMode must NOT be set for developer-agent spawns
        assert!(
            v.get("fastMode").is_none(),
            "Fresh mode must not set fastMode, got: {v}"
        );
    }

    #[test]
    fn continue_mode_emits_status_hooks_via_settings() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Continue {
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--settings"))
            .expect("Continue mode should emit --settings for status hooks");
        let value = argv
            .get(idx + 1)
            .expect("expected JSON value after --settings")
            .to_string_lossy();
        let v: serde_json::Value =
            serde_json::from_str(&value).expect("--settings value should be valid JSON");
        assert!(
            v["hooks"]["Stop"].is_array(),
            "expected hooks.Stop array, got: {v}"
        );
        assert!(
            v["hooks"]["UserPromptSubmit"].is_array(),
            "expected hooks.UserPromptSubmit array, got: {v}"
        );
        // fastMode must NOT be set for developer-agent spawns
        assert!(
            v.get("fastMode").is_none(),
            "Continue mode must not set fastMode, got: {v}"
        );
    }

    #[test]
    fn build_claude_command_appends_remote_control_when_enabled() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let opts = crate::agent::remote_control::RemoteOpts {
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
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let opts = crate::agent::remote_control::RemoteOpts {
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
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
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
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let opts = crate::agent::remote_control::RemoteOpts {
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

    #[test]
    fn build_claude_command_emits_add_dir_per_related_path() {
        let cwd = PathBuf::from("/tmp/test");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![
                PathBuf::from("/work/frontend"),
                PathBuf::from("/work/marketing"),
            ],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let args: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        // Two pairs of (--add-dir, <path>) in order.
        let positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "--add-dir")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            positions.len(),
            2,
            "expected two --add-dir flags; got: {args:?}"
        );
        assert_eq!(args[positions[0] + 1], "/work/frontend");
        assert_eq!(args[positions[1] + 1], "/work/marketing");
    }

    #[test]
    fn build_claude_command_omits_add_dir_when_no_related() {
        let cwd = PathBuf::from("/tmp/test");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let args: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert!(!args.iter().any(|a| a == "--add-dir"), "got: {args:?}");
    }

    #[test]
    fn submit_writes_wraps_codex_in_bracketed_paste() {
        // Codex folds a coalesced "<text>\r" into its composer (paste-burst
        // detection), so the body must be wrapped in a bracketed paste and the
        // CR kept as a separate, unambiguous Enter.
        let (body, enter) = submit_writes(AgentKind::Codex, "[message from claude]\nreview pls");
        assert_eq!(
            body,
            b"\x1b[200~[message from claude]\nreview pls\x1b[201~".to_vec()
        );
        assert_eq!(enter, b"\r".to_vec());
    }

    #[test]
    fn submit_writes_keeps_other_agents_plain() {
        // Claude/Pi/Hermes submit on a plain text + CR; no bracketed paste so
        // their proven-working behavior is untouched.
        for agent in [AgentKind::Claude, AgentKind::Pi, AgentKind::Hermes] {
            let (body, enter) = submit_writes(agent, "hello\nworld");
            assert_eq!(body, b"hello\nworld".to_vec(), "agent {agent:?}");
            assert_eq!(enter, b"\r".to_vec(), "agent {agent:?}");
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_text_when_settled_writes_after_quiet_window() {
        // Use AgentKind::Codex with an arg-ignoring wrapper that execs cat,
        // because Codex Fresh/Continue now injects `-c notify=...` for status
        // reporting which bare `cat` would reject. The wrapper preserves the
        // behavior this timing test requires: cat stays alive and echoes stdin
        // cleanly.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Codex,
            None,
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
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn session_manager_pm_spawn_get_kill() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let cwd = PathBuf::from(".");
        let mut mgr = SessionManager::new();
        assert!(mgr.pm().is_none());
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/wsx-test-pm/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let s = mgr
            .spawn_pm(
                &cwd,
                80,
                24,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
                AgentKind::Claude,
            )
            .unwrap();
        assert!(mgr.pm().is_some());
        // Second spawn while running is a no-op (returns existing).
        let mode2 = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/wsx-test-pm/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let s2 = mgr
            .spawn_pm(
                &cwd,
                80,
                24,
                mode2,
                crate::agent::remote_control::RemoteOpts::disabled(),
                AgentKind::Claude,
            )
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
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn send_text_when_settled_times_out_when_no_output() {
        // cat with no input produces no spontaneous output, so activity_ms
        // stays 0 and the quiet-window condition is never met.
        // Use AgentKind::Codex with an arg-ignoring wrapper that execs cat,
        // because Codex Fresh/Continue now injects `-c notify=...` for status
        // reporting which bare `cat` would reject. The wrapper preserves the
        // behavior this timing test requires: cat stays alive and fully silent.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Codex,
            None,
        )
        .unwrap();
        // Do NOT send any input — cat stays silent, activity_ms never gets set.
        let start = std::time::Instant::now();
        s.send_text_when_settled("NEVER_SENT", 200, 500).await;
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(450), "{elapsed:?}");
        assert!(elapsed < Duration::from_millis(1500), "{elapsed:?}");
        s.kill();
    }

    /// Construct a real PTY-backed Session for scrollback unit tests. Uses
    /// an arg-ignoring wrapper that execs `cat` as the child so spawn succeeds
    /// without the agent on the path. The wrapper is needed because Codex
    /// Fresh/Continue now injects `-c notify=...` for status reporting which
    /// bare `cat` would reject. The `EnvGuard` is only needed for the spawn
    /// syscall itself — `WSX_CODEX_BIN` is read by the parent at
    /// command-build time, not by the spawned cat — so dropping it before the
    /// test body returns is safe.
    fn spawn_for_test() -> Session {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = PathBuf::from(".");
        spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Codex,
            None,
        )
        .expect("spawn_session for scrollback test")
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_report_none_when_mouse_mode_off() {
        let s = spawn_for_test();
        assert!(s.wheel_report_bytes(true, 5, 10).is_none());
        s.kill();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_report_sgr_when_sgr_mode() {
        let s = spawn_for_test();
        {
            let mut p = s.parser.lock().unwrap();
            p.process(b"\x1b[?1000h\x1b[?1006h"); // mouse on + SGR encoding
        }
        assert_eq!(
            s.wheel_report_bytes(true, 5, 10),
            Some(b"\x1b[<64;5;10M".to_vec())
        );
        assert_eq!(
            s.wheel_report_bytes(false, 5, 10),
            Some(b"\x1b[<65;5;10M".to_vec())
        );
        s.kill();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn wheel_report_x10_when_default_encoding() {
        let s = spawn_for_test();
        {
            let mut p = s.parser.lock().unwrap();
            p.process(b"\x1b[?1000h"); // mouse on, default (non-SGR) encoding
        }
        // up=64 -> 32+64=96; col 1 -> 33; row 1 -> 33
        assert_eq!(
            s.wheel_report_bytes(true, 1, 1),
            Some(vec![0x1b, b'[', b'M', 96, 33, 33])
        );
        s.kill();
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

    // All branches in one test: env vars are process-global and the function
    // reads them at call time, so splitting these into separate #[test] fns
    // would only race within ENV_LOCK anyway. EnvGuard restores values on
    // drop, so panicking assertions can't leak state into other tests.
    #[test]
    fn build_pi_command_passes_model_selection() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };

        let argv_of = |env: &mut EnvGuard, mode: &SpawnMode| -> Vec<String> {
            let _ = env;
            let cmd = build_pi_command(
                &cwd,
                mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            cmd.get_argv()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect()
        };

        // 1. Default (no env vars) → --models "deepseek/*"
        {
            let mut env = EnvGuard::new();
            env.remove("WSX_PI_MODEL");
            env.remove("WSX_PI_PROVIDER");
            let argv = argv_of(&mut env, &mode);
            let models_idx = argv
                .iter()
                .position(|a| a == "--models")
                .unwrap_or_else(|| panic!("expected --models in {argv:?}"));
            assert_eq!(argv[models_idx + 1], "deepseek/*");
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
        }

        // 2. WSX_PI_PROVIDER set → --models "<provider>/*"
        {
            let mut env = EnvGuard::new();
            env.remove("WSX_PI_MODEL");
            env.set("WSX_PI_PROVIDER", "anthropic");
            let argv = argv_of(&mut env, &mode);
            let models_idx = argv.iter().position(|a| a == "--models").unwrap();
            assert_eq!(argv[models_idx + 1], "anthropic/*");
        }

        // 3. WSX_PI_MODEL set → --model <value>, with --provider also forwarded
        {
            let mut env = EnvGuard::new();
            env.set("WSX_PI_PROVIDER", "anthropic");
            env.set("WSX_PI_MODEL", "deepseek/deepseek-v4-pro");
            let argv = argv_of(&mut env, &mode);
            let model_idx = argv.iter().position(|a| a == "--model").unwrap();
            assert_eq!(argv[model_idx + 1], "deepseek/deepseek-v4-pro");
            let provider_idx = argv.iter().position(|a| a == "--provider").unwrap();
            assert_eq!(argv[provider_idx + 1], "anthropic");
            assert!(!argv.iter().any(|a| a == "--models"), "argv: {argv:?}");
        }

        // 4. Empty/whitespace env values → treated as unset, fall back to default
        {
            let mut env = EnvGuard::new();
            env.set("WSX_PI_MODEL", "   ");
            env.set("WSX_PI_PROVIDER", "");
            let argv = argv_of(&mut env, &mode);
            let models_idx = argv.iter().position(|a| a == "--models").unwrap();
            assert_eq!(argv[models_idx + 1], "deepseek/*");
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
        }

        // 5. Continue mode → no model/provider flags at all (pi reuses session)
        {
            let mut env = EnvGuard::new();
            env.set("WSX_PI_PROVIDER", "anthropic");
            env.set("WSX_PI_MODEL", "claude-opus-4-7");
            let cont_mode = SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let argv = argv_of(&mut env, &cont_mode);
            assert!(argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--models"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
        }
    }

    #[test]
    fn has_prior_hermes_session_false_when_no_marker() {
        // A brand-new tempdir has no spawn marker → no session detected.
        let tmp = tempfile::tempdir().unwrap();
        assert!(!super::has_prior_hermes_session(tmp.path()));
    }

    mod hermes_session_lookup {
        use super::latest_hermes_session_id;

        fn make_db(path: &std::path::Path) -> rusqlite::Connection {
            let conn = rusqlite::Connection::open(path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    started_at REAL NOT NULL
                );",
            )
            .unwrap();
            conn
        }

        fn insert(conn: &rusqlite::Connection, id: &str, source: &str, started_at: f64) {
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, source, started_at],
            )
            .unwrap();
        }

        #[test]
        fn missing_db_returns_none() {
            let tmp = tempfile::tempdir().unwrap();
            let bogus = tmp.path().join("nope.db");
            assert!(latest_hermes_session_id(&bogus, 1000.0).is_none());
        }

        #[test]
        fn empty_sessions_returns_none() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let _ = make_db(&db_path);
            assert!(latest_hermes_session_id(&db_path, 1000.0).is_none());
        }

        #[test]
        fn session_before_spawn_ts_returns_none() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "old", "cli", 100.0);
            // Spawn was way later; even with -2s buffer, this row is too old.
            assert!(latest_hermes_session_id(&db_path, 1000.0).is_none());
        }

        #[test]
        fn session_after_spawn_ts_returns_id() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "new", "cli", 1500.0);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("new")
            );
        }

        #[test]
        fn buffer_absorbs_small_clock_skew() {
            // Session row created 1.5s before our marker — buffer covers it.
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "racy", "cli", 998.5);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("racy")
            );
        }

        #[test]
        fn returns_most_recent_when_multiple_match() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "first", "cli", 1100.0);
            insert(&conn, "second", "cli", 1200.0);
            insert(&conn, "third", "cli", 1150.0);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("second")
            );
        }

        #[test]
        fn source_irrelevant_to_lookup() {
            // No source filtering; any row in the time range counts.
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let conn = make_db(&db_path);
            insert(&conn, "telegram-sess", "telegram", 1500.0);
            assert_eq!(
                latest_hermes_session_id(&db_path, 1000.0).as_deref(),
                Some("telegram-sess")
            );
        }

        #[test]
        fn concurrent_writer_does_not_block_read_in_wal_mode() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let writer = make_db(&db_path);
            // Switch to WAL mode (matches Hermes's real-world configuration).
            writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
            insert(&writer, "committed", "cli", 1000.0);
            // Start an explicit transaction that writes but doesn't commit yet.
            writer.execute_batch("BEGIN IMMEDIATE; INSERT INTO sessions (id, source, started_at) VALUES ('uncommitted', 'cli', 2000.0);").unwrap();

            // Our reader should see the committed row (the WAL pages from earlier commits
            // are visible) but NOT the uncommitted one. spawn_ts=0 sweeps everything.
            let result = latest_hermes_session_id(&db_path, 0.0);
            assert_eq!(
                result.as_deref(),
                Some("committed"),
                "expected to see committed row, not uncommitted; got: {result:?}"
            );

            writer.execute_batch("ROLLBACK;").unwrap();
        }

        #[test]
        fn reader_sees_wal_committed_writes() {
            let tmp = tempfile::tempdir().unwrap();
            let db_path = tmp.path().join("state.db");
            let writer = make_db(&db_path);
            writer.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
            // First commit goes through normal checkpoint behavior.
            insert(&writer, "first", "cli", 1000.0);
            // Subsequent commits land in WAL before checkpoint.
            insert(&writer, "second", "cli", 2000.0);
            insert(&writer, "third", "cli", 3000.0);
            // Without a manual checkpoint, "second" and "third" are WAL-pending.
            // The reader must still see them all.
            let result = latest_hermes_session_id(&db_path, 0.0);
            assert_eq!(
                result.as_deref(),
                Some("third"),
                "expected newest WAL-committed row; got: {result:?}"
            );
        }
    }

    mod hermes_spawn_marker {
        #[test]
        fn write_then_read_roundtrip() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            super::write_hermes_spawn_marker(tmp.path());
            let marker =
                super::read_hermes_spawn_marker(tmp.path()).expect("marker should be present");
            // Within 60s of now (sanity check).
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            assert!(
                (now - marker.start_ts).abs() < 60.0,
                "marker ts {} too far from now {now}",
                marker.start_ts
            );
            assert!(
                marker.session_id.is_none(),
                "fresh marker should have no session_id"
            );
        }

        #[test]
        fn read_returns_none_when_absent() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            assert!(super::read_hermes_spawn_marker(tmp.path()).is_none());
        }

        #[test]
        fn read_returns_none_when_unparseable() {
            let tmp = tempfile::tempdir().unwrap();
            let info = tmp.path().join(".git/info");
            std::fs::create_dir_all(&info).unwrap();
            std::fs::write(info.join("wsx-hermes-spawn-at"), "not a float\n").unwrap();
            assert!(super::read_hermes_spawn_marker(tmp.path()).is_none());
        }

        #[test]
        fn write_handles_worktree_style_git_file() {
            // `.git` is a file pointing to an external gitdir (real wsx worktree shape).
            let tmp = tempfile::tempdir().unwrap();
            let external = tempfile::tempdir().unwrap();
            let gitdir = external.path().join("worktrees/feature-x");
            std::fs::create_dir_all(&gitdir).unwrap();
            std::fs::write(
                tmp.path().join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .unwrap();
            super::write_hermes_spawn_marker(tmp.path());
            let marker = gitdir.join("info/wsx-hermes-spawn-at");
            assert!(marker.exists(), "expected marker at {}", marker.display());
        }

        #[test]
        fn read_tolerates_old_format() {
            // Old single-line format (no trailing newline, no second line) must parse
            // correctly with session_id=None.
            let tmp = tempfile::tempdir().unwrap();
            let info = tmp.path().join(".git/info");
            std::fs::create_dir_all(&info).unwrap();
            std::fs::write(info.join("wsx-hermes-spawn-at"), "1780002798.96").unwrap();
            let marker = super::read_hermes_spawn_marker(tmp.path())
                .expect("old-format marker should parse");
            assert!(
                (marker.start_ts - 1780002798.96).abs() < 0.001,
                "start_ts mismatch: {}",
                marker.start_ts
            );
            assert!(
                marker.session_id.is_none(),
                "old format should yield session_id=None"
            );
        }

        #[test]
        fn cache_session_id_preserves_start_ts() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            // Write a marker with a specific timestamp.
            std::fs::write(tmp.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();
            // Cache a session id.
            super::cache_hermes_session_id_in_marker(tmp.path(), "abc");
            let marker = super::read_hermes_spawn_marker(tmp.path())
                .expect("marker should exist after cache");
            assert!(
                (marker.start_ts - 1000.0).abs() < 0.001,
                "start_ts should be preserved; got {}",
                marker.start_ts
            );
            assert_eq!(
                marker.session_id.as_deref(),
                Some("abc"),
                "session_id should be cached"
            );
        }

        #[test]
        fn cache_session_id_no_op_when_marker_absent() {
            // tempdir with .git/info set up but no marker file.
            let tmp = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(tmp.path().join(".git/info")).unwrap();
            // Call cache — must not create the marker file.
            super::cache_hermes_session_id_in_marker(tmp.path(), "abc");
            assert!(
                !tmp.path().join(".git/info/wsx-hermes-spawn-at").exists(),
                "cache should not create marker when none exists"
            );
        }
    }

    mod hermes_git_exclude {
        use std::fs;
        use std::io::Read;

        fn init_gitdir(dir: &std::path::Path) {
            fs::create_dir_all(dir.join(".git/info")).unwrap();
        }

        fn read(path: &std::path::Path) -> String {
            let mut s = String::new();
            fs::File::open(path)
                .unwrap()
                .read_to_string(&mut s)
                .unwrap();
            s
        }

        #[test]
        fn creates_exclude_line_when_absent() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            let contents = read(&tmp.path().join(".git/info/exclude"));
            assert!(
                contents.lines().any(|l| l == "AGENTS.md"),
                "expected AGENTS.md line in {contents:?}"
            );
        }

        #[test]
        fn idempotent_when_entry_already_present() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            let exclude = tmp.path().join(".git/info/exclude");
            fs::write(&exclude, "AGENTS.md\n").unwrap();
            let before = read(&exclude);
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            let after = read(&exclude);
            assert_eq!(before, after);
        }

        #[test]
        fn handles_missing_info_dir() {
            let tmp = tempfile::tempdir().unwrap();
            fs::create_dir_all(tmp.path().join(".git")).unwrap();
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            let contents = read(&tmp.path().join(".git/info/exclude"));
            assert!(contents.contains("AGENTS.md"));
        }

        #[test]
        fn no_op_when_gitdir_absent() {
            let tmp = tempfile::tempdir().unwrap();
            // No .git/ at all. Must not panic.
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            assert!(!tmp.path().join(".git").exists());
        }

        #[test]
        fn follows_worktree_style_git_file_with_absolute_gitdir() {
            let tmp = tempfile::tempdir().unwrap();
            // External gitdir lives outside the worktree
            let external = tempfile::tempdir().unwrap();
            let gitdir = external.path().join("worktrees/feature-x");
            fs::create_dir_all(&gitdir).unwrap();
            // worktree/.git is a FILE pointing at the external gitdir
            fs::write(
                tmp.path().join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .unwrap();

            super::ensure_git_exclude(tmp.path(), "AGENTS.md");

            let exclude = gitdir.join("info/exclude");
            let contents = read(&exclude);
            assert!(
                contents.contains("AGENTS.md"),
                "expected AGENTS.md in {}: {contents:?}",
                exclude.display()
            );
        }

        #[test]
        fn follows_worktree_style_git_file_with_relative_gitdir() {
            let tmp = tempfile::tempdir().unwrap();
            // Relative gitdir resolved against the worktree path
            let rel = "external-gitdir";
            fs::create_dir_all(tmp.path().join(rel)).unwrap();
            fs::write(tmp.path().join(".git"), format!("gitdir: {rel}\n")).unwrap();

            super::ensure_git_exclude(tmp.path(), "AGENTS.md");

            let exclude = tmp.path().join(rel).join("info/exclude");
            let contents = read(&exclude);
            assert!(contents.contains("AGENTS.md"));
        }

        #[test]
        fn returns_silently_when_git_file_unparseable() {
            let tmp = tempfile::tempdir().unwrap();
            fs::write(tmp.path().join(".git"), "not a valid git pointer\n").unwrap();
            // Must not panic and must not create any files
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
        }
    }

    mod hermes_agents_md {
        use std::fs;

        #[test]
        fn creates_file_with_fenced_block_when_absent() {
            let tmp = tempfile::tempdir().unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                contents.contains(super::HERMES_BLOCK_BEGIN),
                "missing BEGIN marker: {contents:?}"
            );
            assert!(
                contents.contains(super::HERMES_BLOCK_END),
                "missing END marker: {contents:?}"
            );
            assert!(contents.contains("inject me"));
        }

        #[test]
        fn preserves_user_content_outside_wsx_block() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "# User notes\n\nKeep me.\n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(&path).unwrap();
            assert!(contents.contains("# User notes"));
            assert!(contents.contains("Keep me."));
            assert!(contents.contains("inject me"));
        }

        #[test]
        fn replaces_existing_wsx_block_idempotently() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            super::write_agents_md_section(tmp.path(), Some("first"));
            let after_first = fs::read_to_string(&path).unwrap();
            super::write_agents_md_section(tmp.path(), Some("first"));
            let after_second = fs::read_to_string(&path).unwrap();
            assert_eq!(
                after_first, after_second,
                "second write should be byte-identical"
            );
        }

        #[test]
        fn replacing_block_with_new_content_replaces_in_place() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            super::write_agents_md_section(tmp.path(), Some("first"));
            super::write_agents_md_section(tmp.path(), Some("second"));
            let contents = fs::read_to_string(&path).unwrap();
            assert!(contents.contains("second"));
            assert!(!contents.contains("first"), "old content should be removed");
        }

        #[test]
        fn strips_block_when_content_is_none() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "user content\n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("temp"));
            super::write_agents_md_section(tmp.path(), None);
            let contents = fs::read_to_string(&path).unwrap();
            assert!(contents.contains("user content"));
            assert!(!contents.contains(super::HERMES_BLOCK_BEGIN));
            assert!(!contents.contains("temp"));
        }

        #[test]
        fn no_write_when_content_is_none_and_no_existing_block() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            // Don't create the file at all.
            super::write_agents_md_section(tmp.path(), None);
            assert!(
                !path.exists(),
                "should not create AGENTS.md just to strip nothing"
            );
        }

        #[test]
        fn survives_unreadable_agents_md() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "untouchable\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            // Must not panic.
            super::write_agents_md_section(tmp.path(), Some("inject"));
            // Restore perms so tempdir cleanup works.
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        #[test]
        fn copies_claude_md_after_block_on_fresh_create() {
            let tmp = tempfile::tempdir().unwrap();
            fs::write(
                tmp.path().join("CLAUDE.md"),
                "# Project rules\n\nBe nice.\n",
            )
            .unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "missing provenance comment: {contents:?}"
            );
            assert!(
                contents.contains("Be nice."),
                "missing CLAUDE.md content: {contents:?}"
            );
            // CLAUDE.md content must come AFTER the wsx-managed block.
            let end_idx = contents.find(super::HERMES_BLOCK_END).unwrap();
            let prov_idx = contents.find(super::CLAUDE_PROVENANCE_COMMENT).unwrap();
            assert!(
                prov_idx > end_idx,
                "CLAUDE.md content must follow the wsx block: {contents:?}"
            );
        }

        #[test]
        fn no_claude_md_means_no_copy() {
            let tmp = tempfile::tempdir().unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                !contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "should not add provenance comment when no CLAUDE.md: {contents:?}"
            );
        }

        #[test]
        fn does_not_copy_claude_md_when_agents_md_already_exists() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "# Existing notes\n").unwrap();
            fs::write(tmp.path().join("CLAUDE.md"), "Be nice.\n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(&path).unwrap();
            assert!(
                !contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "must not copy CLAUDE.md when AGENTS.md pre-existed: {contents:?}"
            );
            assert!(
                !contents.contains("Be nice."),
                "must not copy CLAUDE.md content: {contents:?}"
            );
        }

        #[test]
        fn blank_claude_md_is_not_copied() {
            let tmp = tempfile::tempdir().unwrap();
            fs::write(tmp.path().join("CLAUDE.md"), "   \n\n  \n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                !contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "blank CLAUDE.md should not be copied: {contents:?}"
            );
        }
    }

    mod hermes_compose {
        fn rename_ctx() -> super::RenameContext {
            super::RenameContext {
                current_branch: "wsx/bold-fern".into(),
                branch_prefix: "wsx".into(),
                repo_name: "myrepo".into(),
                current_slug: "bold-fern".into(),
            }
        }

        #[test]
        fn fresh_with_rename_returns_rename_text() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: Some(rename_ctx()),
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert!(result.contains("wsx workspace rename 'myrepo' 'bold-fern'"));
        }

        #[test]
        fn fresh_with_rename_and_custom_combines_both() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: Some(rename_ctx()),
                custom_instructions: Some("Use ruff.".into()),
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert!(result.contains("wsx workspace rename"));
            assert!(result.contains("Use ruff."));
            let rename_pos = result.find("wsx workspace rename").unwrap();
            let custom_pos = result.find("Use ruff.").unwrap();
            assert!(
                custom_pos > rename_pos,
                "custom should come after rename block"
            );
        }

        #[test]
        fn fresh_without_rename_returns_custom_only() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: Some("Use ruff.".into()),
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert_eq!(result, "Use ruff.");
        }

        #[test]
        fn fresh_with_nothing_returns_none() {
            let mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            assert!(super::compose_injected_prompt(&mode).is_none());
        }

        #[test]
        fn continue_with_custom_returns_custom() {
            let mode = super::SpawnMode::Continue {
                custom_instructions: Some("Be terse.".into()),
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert_eq!(result, "Be terse.");
        }

        #[test]
        fn continue_without_custom_returns_none() {
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            assert!(super::compose_injected_prompt(&mode).is_none());
        }

        #[test]
        fn project_manager_returns_pm_prompt() {
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: false,
                fast_mode: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            assert!(!result.is_empty());
        }

        #[test]
        fn hermes_prepends_doctrine_before_custom() {
            let mode = super::SpawnMode::Continue {
                custom_instructions: Some("CUSTOM_MARK".to_string()),
                doctrine: Some("DOCTRINE_MARK".to_string()),
                additional_dirs: vec![],
                yolo: false,
            };
            let result = super::compose_injected_prompt(&mode).expect("expected Some");
            let dpos = result.find("DOCTRINE_MARK").expect("doctrine present");
            let cpos = result.find("CUSTOM_MARK").expect("custom present");
            assert!(dpos < cpos, "doctrine must precede custom: {result}");
            assert!(
                result.starts_with("DOCTRINE_MARK"),
                "doctrine must lead: {result}"
            );
        }

        #[test]
        fn hermes_pm_mode_has_no_doctrine() {
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/x/workspaces.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: false,
                fast_mode: false,
            };
            let result =
                super::compose_injected_prompt(&mode).expect("PM still injects its prompt");
            assert!(
                !result.contains("DOCTRINE_MARK"),
                "PM must not get doctrine: {result}"
            );
        }
    }

    mod hermes_prepare_workspace {
        use std::fs;

        fn init_gitdir(dir: &std::path::Path) {
            fs::create_dir_all(dir.join(".git/info")).unwrap();
        }

        #[test]
        fn fresh_with_rename_writes_agents_md_and_exclude() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            let mode = super::SpawnMode::Fresh {
                rename_ctx: Some(super::RenameContext {
                    current_branch: "wsx/bold-fern".into(),
                    branch_prefix: "wsx".into(),
                    repo_name: "myrepo".into(),
                    current_slug: "bold-fern".into(),
                }),
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &mode);

            let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(agents.contains("<!-- BEGIN wsx-managed -->"));
            assert!(agents.contains("wsx workspace rename 'myrepo' 'bold-fern'"));

            let exclude = fs::read_to_string(tmp.path().join(".git/info/exclude")).unwrap();
            assert!(exclude.lines().any(|l| l == "AGENTS.md"));
        }

        #[test]
        fn continue_without_custom_instructions_strips_block() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            // First prepare a Fresh+rename state.
            let fresh = super::SpawnMode::Fresh {
                rename_ctx: Some(super::RenameContext {
                    current_branch: "wsx/bold-fern".into(),
                    branch_prefix: "wsx".into(),
                    repo_name: "myrepo".into(),
                    current_slug: "bold-fern".into(),
                }),
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &fresh);
            // Now spawn Continue with nothing to inject.
            let cont = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &cont);
            let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap_or_default();
            assert!(
                !agents.contains("<!-- BEGIN wsx-managed -->"),
                "wsx block should be removed; got: {agents}"
            );
            assert!(
                !agents.contains("wsx workspace rename"),
                "rename text should be gone; got: {agents}"
            );
        }

        #[test]
        fn no_op_when_continue_no_custom_and_no_existing_agents_md() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            let cont = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &cont);
            assert!(!tmp.path().join("AGENTS.md").exists());
        }

        #[test]
        fn does_not_overwrite_existing_marker() {
            // Write a marker with a known timestamp, then call prepare_hermes_workspace
            // in Fresh mode. The marker must NOT be overwritten — start_ts stays 1000.0.
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            // Manually write a marker with a specific (old) timestamp.
            std::fs::write(tmp.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();
            let fresh_mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &fresh_mode);
            let marker = super::read_hermes_spawn_marker(tmp.path())
                .expect("marker should still exist after prepare");
            assert!(
                (marker.start_ts - 1000.0).abs() < 0.001,
                "start_ts must be preserved; got {}",
                marker.start_ts
            );
        }
    }

    mod hermes_build_command {
        use std::ffi::OsStr;

        fn argv_strings(cmd: &portable_pty::CommandBuilder) -> Vec<String> {
            // Skip argv[0] (the binary name); callers assert on subcommand/flags.
            cmd.get_argv()
                .iter()
                .skip(1)
                .map(|s| s.to_string_lossy().into_owned())
                .collect()
        }

        fn fresh_no_rename() -> super::SpawnMode {
            super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            }
        }

        #[test]
        fn fresh_emits_chat_subcommand_only_no_source_flag() {
            // --source is never emitted: Hermes ignores it for session creation.
            let tmp = tempfile::tempdir().unwrap();
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert_eq!(
                argv.first().map(|s| s.as_str()),
                Some("chat"),
                "argv: {argv:?}"
            );
            assert!(
                !argv.iter().any(|a| a == "--source"),
                "--source must not be emitted; argv: {argv:?}"
            );
        }

        #[test]
        fn fresh_omits_continue_resume_and_yolo() {
            let tmp = tempfile::tempdir().unwrap();
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--resume"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--yolo"), "argv: {argv:?}");
        }

        #[test]
        fn yolo_fresh_emits_yolo_flag() {
            let tmp = tempfile::tempdir().unwrap();
            let mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: true,
            };
            let cmd = super::build_hermes_command(
                tmp.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
        }

        #[test]
        fn yolo_continue_emits_yolo_flag() {
            let tmp = tempfile::tempdir().unwrap();
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: true,
            };
            let cmd = super::build_hermes_command(
                tmp.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
        }

        #[test]
        fn project_manager_mode_is_always_yolo() {
            let tmp = tempfile::tempdir().unwrap();
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: false,
                fast_mode: false,
            };
            let cmd = super::build_hermes_command(
                tmp.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(argv_strings(&cmd).iter().any(|a| a == "--yolo"));
        }

        #[test]
        fn project_manager_mode_emits_yolo_and_resume_if_set() {
            let home = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            // Seed .git/info structure and spawn marker for cwd.
            std::fs::create_dir_all(cwd.path().join(".git/info")).unwrap();
            std::fs::write(cwd.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();
            // Seed ~/.hermes/state.db with a session after spawn_ts.
            let hermes_dir = home.path().join(".hermes");
            std::fs::create_dir_all(&hermes_dir).unwrap();
            let db_path = hermes_dir.join("state.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, started_at REAL NOT NULL);",
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('pm-sess', 'cli', 1234.5);",
                [],
            )
            .unwrap();
            drop(conn);

            let mut env = super::EnvGuard::new();
            env.set("HOME", home.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::ProjectManager {
                workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                custom_instructions: None,
                additional_dirs: vec![],
                resume: true,
                fast_mode: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let resume_idx = argv
                .iter()
                .position(|a| a == "--resume")
                .expect("expected --resume");
            assert_eq!(argv[resume_idx + 1], "pm-sess");
            assert!(argv.iter().any(|a| a == "--yolo"), "argv: {argv:?}");
        }

        #[test]
        fn no_worktree_flag_ever_emitted() {
            let tmp = tempfile::tempdir().unwrap();
            for mode in &[
                fresh_no_rename(),
                super::SpawnMode::Continue {
                    custom_instructions: None,
                    doctrine: None,
                    additional_dirs: vec![],
                    yolo: true,
                },
                super::SpawnMode::ProjectManager {
                    workspaces_json_path: std::path::PathBuf::from("/tmp/ws.json"),
                    custom_instructions: None,
                    additional_dirs: vec![],
                    resume: true,
                    fast_mode: false,
                },
            ] {
                let cmd = super::build_hermes_command(
                    tmp.path(),
                    mode,
                    crate::agent::remote_control::RemoteOpts::disabled(),
                );
                let argv = argv_strings(&cmd);
                assert!(
                    !argv.iter().any(|a| a == "--worktree" || a == "-w"),
                    "should never emit --worktree; argv: {argv:?}"
                );
            }
        }

        #[test]
        fn source_never_emitted_regardless_of_path() {
            // --source is never emitted, even for paths that would previously have
            // triggered source tag emission. Session detection uses the marker file.
            let bogus = std::path::Path::new("/nonexistent/path/for/canonicalize");
            let cmd = super::build_hermes_command(
                bogus,
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert!(
                !argv.iter().any(|a| a == "--source"),
                "expected --source absent; argv: {argv:?}"
            );
            assert_eq!(argv.first().map(|s| s.as_str()), Some("chat"));
        }

        #[test]
        fn continue_without_prior_session_omits_resume() {
            let tmp = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.set("HOME", tmp.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--resume"), "argv: {argv:?}");
            assert!(!argv.iter().any(|a| a == "--continue"), "argv: {argv:?}");
        }

        #[test]
        fn continue_with_prior_session_passes_resume_id() {
            let home = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            // Seed .git/info structure and a marker file for cwd.
            std::fs::create_dir_all(cwd.path().join(".git/info")).unwrap();
            // Write marker with timestamp 1000.0
            std::fs::write(cwd.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();

            let hermes_dir = home.path().join(".hermes");
            std::fs::create_dir_all(&hermes_dir).unwrap();
            let db_path = hermes_dir.join("state.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, started_at REAL NOT NULL);",
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('session-abc', 'cli', 1234.5);",
                [],
            ).unwrap();
            drop(conn);

            let mut env = super::EnvGuard::new();
            env.set("HOME", home.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let idx = argv
                .iter()
                .position(|a| a == "--resume")
                .expect("expected --resume");
            assert_eq!(argv[idx + 1], "session-abc");
        }

        #[test]
        fn continue_with_cached_session_id_uses_cached_value() {
            // Marker file has session_id="session-cached". DB has two sessions:
            // "session-cached" (older, started_at=1100.0) and "session-newer"
            // (newer, started_at=1500.0). The cached id must win over the newer
            // time-based result.
            let home = tempfile::tempdir().unwrap();
            let cwd = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(cwd.path().join(".git/info")).unwrap();
            // Write marker with start_ts=1000.0 AND cached session_id.
            std::fs::write(
                cwd.path().join(".git/info/wsx-hermes-spawn-at"),
                "1000.0\nsession-cached\n",
            )
            .unwrap();

            let hermes_dir = home.path().join(".hermes");
            std::fs::create_dir_all(&hermes_dir).unwrap();
            let db_path = hermes_dir.join("state.db");
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (id TEXT PRIMARY KEY, source TEXT NOT NULL, started_at REAL NOT NULL);",
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('session-cached', 'cli', 1100.0);",
                [],
            ).unwrap();
            conn.execute(
                "INSERT INTO sessions (id, source, started_at) VALUES ('session-newer', 'cli', 1500.0);",
                [],
            ).unwrap();
            drop(conn);

            let mut env = super::EnvGuard::new();
            env.set("HOME", home.path().to_string_lossy().as_ref());
            let mode = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            let cmd = super::build_hermes_command(
                cwd.path(),
                &mode,
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let idx = argv
                .iter()
                .position(|a| a == "--resume")
                .expect("expected --resume");
            assert_eq!(
                argv[idx + 1],
                "session-cached",
                "cached id must win over time-based newer session; argv: {argv:?}"
            );
        }

        fn env_of(cmd: &portable_pty::CommandBuilder, key: &str) -> Option<String> {
            cmd.get_env(OsStr::new(key))
                .map(|v| v.to_string_lossy().into_owned())
        }

        #[test]
        fn wsx_hermes_model_env_sets_inference_model_env_on_child() {
            let tmp = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.remove("HERMES_INFERENCE_MODEL");
            env.set("WSX_HERMES_MODEL", "deepseek/deepseek-v4-pro");
            env.remove("WSX_HERMES_PROVIDER");
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert_eq!(
                env_of(&cmd, "HERMES_INFERENCE_MODEL"),
                Some("deepseek/deepseek-v4-pro".to_string())
            );
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--model"), "argv: {argv:?}");
        }

        #[test]
        fn wsx_hermes_provider_env_passes_provider_flag() {
            let tmp = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.remove("WSX_HERMES_MODEL");
            env.set("WSX_HERMES_PROVIDER", "openrouter");
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            let argv = argv_strings(&cmd);
            let idx = argv
                .iter()
                .position(|a| a == "--provider")
                .expect("expected --provider");
            assert_eq!(argv[idx + 1], "openrouter");
        }

        #[test]
        fn empty_model_env_treated_as_unset() {
            let tmp = tempfile::tempdir().unwrap();
            let mut env = super::EnvGuard::new();
            env.remove("HERMES_INFERENCE_MODEL");
            env.set("WSX_HERMES_MODEL", "   ");
            env.set("WSX_HERMES_PROVIDER", "");
            let cmd = super::build_hermes_command(
                tmp.path(),
                &fresh_no_rename(),
                crate::agent::remote_control::RemoteOpts::disabled(),
            );
            assert!(env_of(&cmd, "HERMES_INFERENCE_MODEL").is_none());
            let argv = argv_strings(&cmd);
            assert!(!argv.iter().any(|a| a == "--provider"), "argv: {argv:?}");
        }
    }

    // ── Batch B: shell_quote helper and rename prompt quoting ────────────────

    #[test]
    fn shell_quote_handles_internal_single_quote() {
        assert_eq!(shell_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn render_rename_prompt_claude_shell_quotes_repo_name_with_space() {
        let prompt = render_rename_system_prompt("wsx/bold-fern", "wsx", "my repo", "bold-fern");
        assert!(
            prompt.contains("wsx workspace rename 'my repo'"),
            "expected single-quoted repo name with space; prompt: {prompt}"
        );
    }

    #[test]
    fn render_rename_prompt_pi_shell_quotes_repo_name_with_metacharacter() {
        let prompt = render_rename_system_prompt_pi("wsx/bold-fern", "wsx", "foo;bar", "bold-fern");
        assert!(
            prompt.contains("'foo;bar'"),
            "expected single-quoted repo name with metachar; prompt: {prompt}"
        );
    }

    // ── Batch C: rename prompt uses stored ws.name, not derived slug ─────────

    #[test]
    fn rename_prompt_uses_ws_name_not_derived_slug() {
        let ctx = RenameContext {
            current_branch: "OLD-PREFIX/bold-fern".into(),
            branch_prefix: "wsx".into(),
            repo_name: "myrepo".into(),
            current_slug: "actual-stored-name".into(),
        };
        let prompt = render_rename_system_prompt(
            &ctx.current_branch,
            &ctx.branch_prefix,
            &ctx.repo_name,
            &ctx.current_slug,
        );
        assert!(
            prompt.contains("wsx workspace rename 'myrepo' 'actual-stored-name' <slug>"),
            "expected stored slug in rename command; prompt: {prompt}"
        );
        assert!(
            !prompt.contains("'bold-fern'"),
            "prompt must not contain derived 'bold-fern'; prompt: {prompt}"
        );
    }

    #[test]
    fn agent_kind_helpers_match_existing_strings() {
        use super::AgentKind;
        assert_eq!(AgentKind::ALL.len(), 4);
        assert!(AgentKind::ALL.contains(&AgentKind::Claude));
        assert!(AgentKind::ALL.contains(&AgentKind::Pi));
        assert!(AgentKind::ALL.contains(&AgentKind::Hermes));
        assert!(AgentKind::ALL.contains(&AgentKind::Codex));

        assert_eq!(AgentKind::Claude.display_name(), "claude");
        assert_eq!(AgentKind::Pi.display_name(), "pi");
        assert_eq!(AgentKind::Hermes.display_name(), "hermes");
        assert_eq!(AgentKind::Codex.display_name(), "codex");

        assert_eq!(AgentKind::Claude.default_binary(), "claude");
        assert_eq!(AgentKind::Pi.default_binary(), "pi");
        assert_eq!(AgentKind::Hermes.default_binary(), "hermes");
        assert_eq!(AgentKind::Codex.default_binary(), "codex");

        for k in AgentKind::ALL {
            assert_eq!(AgentKind::from_str_or_default(Some(k.store_value())), k);
        }

        assert_eq!(
            AgentKind::from_str_or_default(None),
            AgentKind::Claude,
            "None input must default to Claude — store.rs relies on this"
        );
    }

    #[test]
    fn spawn_session_returns_agent_binary_missing_for_unknown_path() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", "/nonexistent/wsx-test-bin-does-not-exist");
        let cwd = PathBuf::from(".");
        let result = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Claude,
            None,
        );
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("spawn should fail when binary is missing"),
        };
        match err {
            Error::AgentBinaryMissing(binary) => {
                assert_eq!(binary, "/nonexistent/wsx-test-bin-does-not-exist");
            }
            other => panic!("expected AgentBinaryMissing, got {other:?}"),
        }
    }

    #[test]
    fn spawn_identity_env_vars_set_on_command_when_present() {
        let mut cmd = CommandBuilder::new("dummy");
        let identity = Some(SpawnIdentity {
            workspace_id: 42,
            instance_id: 7,
        });
        if let Some(id) = identity {
            cmd.env("WSX_WORKSPACE_ID", id.workspace_id.to_string());
            cmd.env("WSX_AGENT_INSTANCE_ID", id.instance_id.to_string());
        }
        assert_eq!(
            cmd.get_env("WSX_WORKSPACE_ID").and_then(|v| v.to_str()),
            Some("42"),
        );
        assert_eq!(
            cmd.get_env("WSX_AGENT_INSTANCE_ID")
                .and_then(|v| v.to_str()),
            Some("7"),
        );
    }

    #[test]
    fn spawn_identity_env_vars_absent_when_none() {
        let mut env = EnvGuard::new();
        env.remove("WSX_WORKSPACE_ID");
        env.remove("WSX_AGENT_INSTANCE_ID");

        let mut cmd = CommandBuilder::new("dummy");
        let identity: Option<SpawnIdentity> = None;
        if let Some(id) = identity {
            cmd.env("WSX_WORKSPACE_ID", id.workspace_id.to_string());
            cmd.env("WSX_AGENT_INSTANCE_ID", id.instance_id.to_string());
        }
        assert!(
            cmd.get_env("WSX_WORKSPACE_ID").is_none(),
            "WSX_WORKSPACE_ID must not be set for PM session"
        );
        assert!(
            cmd.get_env("WSX_AGENT_INSTANCE_ID").is_none(),
            "WSX_AGENT_INSTANCE_ID must not be set for PM session"
        );
    }

    #[test]
    fn claude_prepends_doctrine_before_custom_instructions() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some("CUSTOM_MARK".to_string()),
            doctrine: Some("DOCTRINE_MARK".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("expected --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        let dpos = prompt.find("DOCTRINE_MARK").expect("doctrine present");
        let cpos = prompt.find("CUSTOM_MARK").expect("custom present");
        assert!(
            dpos < cpos,
            "doctrine must precede custom instructions: {prompt}"
        );
        assert!(
            prompt.starts_with("DOCTRINE_MARK"),
            "doctrine must lead: {prompt}"
        );
    }

    #[test]
    fn pi_prepends_doctrine_before_custom_instructions() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Continue {
            custom_instructions: Some("CUSTOM_MARK".to_string()),
            doctrine: Some("DOCTRINE_MARK".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_pi_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("expected --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        let dpos = prompt.find("DOCTRINE_MARK").expect("doctrine present");
        let cpos = prompt.find("CUSTOM_MARK").expect("custom present");
        assert!(
            dpos < cpos,
            "doctrine must precede custom instructions: {prompt}"
        );
        assert!(
            prompt.starts_with("DOCTRINE_MARK"),
            "doctrine must lead: {prompt}"
        );
    }

    #[test]
    fn pi_pm_mode_has_no_doctrine_marker() {
        // PM variant has no doctrine field; ensure nothing leaks one in.
        // Give PM custom instructions so it definitely emits an
        // --append-system-prompt, making the no-doctrine assertion non-vacuous.
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: Some("PM_CUSTOM_MARK".to_string()),
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_pi_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("PM with custom instructions must emit --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        assert!(
            prompt.contains("PM_CUSTOM_MARK"),
            "PM prompt should be present: {prompt}"
        );
        assert!(
            !prompt.contains("DOCTRINE_MARK"),
            "PM must not get doctrine: {prompt}"
        );
    }

    #[test]
    fn claude_pm_mode_has_no_doctrine_marker() {
        // PM variant has no doctrine field; ensure nothing leaks one in.
        let cwd = PathBuf::from(".");
        // Give PM custom instructions so it definitely emits an
        // --append-system-prompt, making the no-doctrine assertion non-vacuous.
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: Some("PM_CUSTOM_MARK".to_string()),
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(
            &cwd,
            &mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--append-system-prompt"))
            .expect("PM with custom instructions must emit --append-system-prompt");
        let prompt = argv.get(idx + 1).unwrap().to_string_lossy();
        assert!(
            prompt.contains("PM_CUSTOM_MARK"),
            "PM prompt should be present: {prompt}"
        );
        assert!(
            !prompt.contains("DOCTRINE_MARK"),
            "PM must not get doctrine: {prompt}"
        );
    }

    /// Build a Codex command for `mode` and return its argv as lossy Strings.
    fn codex_argv(mode: &SpawnMode) -> Vec<String> {
        let cmd = build_codex_command(
            Path::new("/tmp/wt"),
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
        );
        cmd.get_argv()
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn codex_fresh_is_bare_codex_with_no_approval_flags() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.remove("WSX_CODEX_MODEL");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(
            !argv.iter().any(|a| a == "resume"),
            "fresh must not resume: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a.starts_with("--dangerously-bypass")),
            "non-yolo must not bypass: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "--ask-for-approval"),
            "dev session uses codex defaults: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "-m"),
            "no model env set: {argv:?}"
        );
    }

    #[test]
    fn codex_fresh_yolo_bypasses_approvals() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: true,
        });
        assert!(
            argv.iter()
                .any(|a| a == "--dangerously-bypass-approvals-and-sandbox"),
            "yolo must bypass: {argv:?}"
        );
    }

    #[test]
    fn codex_continue_uses_resume_last() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::Continue {
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(
            argv.iter().any(|a| a == "resume"),
            "continue must resume: {argv:?}"
        );
        assert!(
            argv.iter().any(|a| a == "--last"),
            "continue must use --last: {argv:?}"
        );
    }

    #[test]
    fn codex_pm_is_read_only_and_never_asks() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::ProjectManager {
            workspaces_json_path: std::path::PathBuf::from("/tmp/pm/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        });
        assert!(
            argv.windows(2)
                .any(|w| w[0] == "--ask-for-approval" && w[1] == "never"),
            "pm must never ask: {argv:?}"
        );
        assert!(
            argv.windows(2)
                .any(|w| w[0] == "--sandbox" && w[1] == "read-only"),
            "pm must be read-only: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "resume"),
            "pm fresh must not resume: {argv:?}"
        );
    }

    #[test]
    fn codex_model_env_adds_dash_m() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.set("WSX_CODEX_MODEL", "gpt-5.4");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(
            argv.windows(2).any(|w| w[0] == "-m" && w[1] == "gpt-5.4"),
            "model must be passed via -m: {argv:?}"
        );
    }

    #[test]
    fn codex_fresh_injects_notify_status_wiring() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        env.remove("WSX_CODEX_MODEL");
        let argv = codex_argv(&SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        });
        assert!(argv.iter().any(|a| a == "-c"), "argv: {argv:?}");
        assert!(
            argv.iter()
                .any(|a| a.starts_with("notify=[") && a.contains("from-notify")),
            "argv: {argv:?}"
        );
    }

    #[test]
    fn codex_pm_omits_notify_status_wiring() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", "codex");
        let argv = codex_argv(&SpawnMode::ProjectManager {
            workspaces_json_path: std::path::PathBuf::from("/tmp/pm/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        });
        assert!(
            !argv.iter().any(|a| a.starts_with("notify=[")),
            "PM should not get status wiring; argv: {argv:?}"
        );
        assert!(
            !argv.iter().any(|a| a == "-c"),
            "PM should not inject the -c flag; argv: {argv:?}"
        );
    }

    #[test]
    fn prepare_codex_workspace_injects_rename_block_into_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(RenameContext {
                current_branch: "prefix/my-slug".to_string(),
                branch_prefix: "prefix".to_string(),
                repo_name: "myrepo".to_string(),
                current_slug: "my-slug".to_string(),
            }),
            custom_instructions: None,
            doctrine: Some("DOCTRINE-MARKER".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        prepare_codex_workspace(cwd, &mode);
        let agents = std::fs::read_to_string(cwd.join("AGENTS.md")).unwrap();
        assert!(
            agents.contains("BEGIN wsx-managed"),
            "block markers: {agents}"
        );
        assert!(
            agents.contains("DOCTRINE-MARKER"),
            "doctrine injected: {agents}"
        );
        assert!(
            agents.contains("wsx workspace rename"),
            "rename hint: {agents}"
        );
    }

    #[test]
    fn prepare_codex_workspace_writes_no_hermes_marker() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some("CUSTOM".to_string()),
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        prepare_codex_workspace(cwd, &mode);
        // Codex uses cwd-in-file detection, not the Hermes spawn marker.
        assert!(
            !cwd.join(".git/info/wsx-hermes-spawn-at").exists(),
            "codex must not write the hermes spawn marker"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn codex_spawn_and_echo() {
        // Use an arg-ignoring wrapper that execs cat, because Codex
        // Fresh/Continue now injects `-c notify=...` for status reporting
        // which bare `cat` would reject. The wrapper preserves the echo
        // behavior this test relies on.
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = PathBuf::from(".");
        let s = spawn_session(
            &cwd,
            80,
            24,
            SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            },
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Codex,
            None,
        )
        .unwrap();
        s.writer.send(b"hello-codex\n".to_vec()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("hello-codex"), "screen: {screen:?}");
    }
}
