#![allow(clippy::collapsible_if, clippy::arc_with_non_send_sync)]

use crate::error::{Error, Result};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
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

// Per-agent command construction now lives in `command`; the builders are
// called by `spawn_session` below.
pub use crate::pty::command::{
    build_claude_command, build_codex_command, build_hermes_command, build_pi_command,
};

// AGENTS.md / git-exclude / spawn-prep plumbing now lives in `workspace_prep`;
// `prepare_*_workspace` are called by `spawn_session` below.
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
    /// When set, this session's child is a tmux attach client and the agent
    /// lives in the tmux server under this session name. `kill()`/`Drop` kill
    /// only the client (agent survives — the shared-workspace persistence
    /// contract); `kill_backend()` also kills the server session.
    pub tmux_session: Option<String>,
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
        // Floor both dimensions to >=1. A pane area can collapse to 0 on a tiny
        // terminal (ratatui's `Min(1)` yields 0 when chrome eats all the rows),
        // and `vt100::Grid::set_size` computes `size.rows - 1`, which underflows
        // and panics at 0. Guards both the background resize sweep and the
        // attached render path, which can hit the same case.
        let cols = cols.max(1);
        let rows = rows.max(1);
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

    /// Kill the child (attach client) AND, for tmux-backed sessions, the tmux
    /// session holding the agent. Explicit user intent — "kill this agent".
    pub fn kill_backend(&self) {
        self.kill();
        if let Some(name) = &self.tmux_session {
            crate::pty::tmux::kill_session(name);
        }
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

// Threading the tmux session name into the spawn path adds an eighth input
// past clippy's default; bundling these into a params struct would not improve
// clarity, matching the same allowance on `SessionManager::spawn`.
#[allow(clippy::too_many_arguments)]
pub fn spawn_session(
    cwd: &Path,
    cols: u16,
    rows: u16,
    mode: SpawnMode,
    remote: crate::agent::remote_control::RemoteOpts,
    agent: AgentKind,
    identity: Option<SpawnIdentity>,
    tmux: Option<&str>,
) -> Result<Session> {
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
    let reportable = resolved_binary(agent);
    spawn_command_session(child_cmd, cols, rows, agent, reportable, tmux)
}

/// Agent-agnostic spawn path: opens a PTY, optionally wraps the command in
/// tmux, spawns it, and wires up the parser / reader thread / writer task into
/// a [`Session`]. `spawn_session` builds a per-agent command and delegates
/// here; other callers (e.g. remote `ssh -t … tmux attach`) can pass an
/// arbitrary [`CommandBuilder`] directly.
///
/// `agent` is inert plumbing: it only tags the returned `Session`, driving
/// render/paste quirks via [`submit_writes`]. Remote sessions never paste
/// through `submit_writes`, so such callers pass a benign default
/// (`AgentKind::Claude`).
///
/// `reportable_binary` is the binary name surfaced in an
/// [`Error::AgentBinaryMissing`] when the spawn fails because the command is
/// not found. It is decoupled from `agent` on purpose: `spawn_session` passes
/// the resolved agent binary, while the remote attach passes `ssh` (the binary
/// it actually execs), so a missing *local* `ssh` isn't misreported as the
/// agent it would eventually reach.
pub fn spawn_command_session(
    child_cmd: CommandBuilder,
    cols: u16,
    rows: u16,
    agent: AgentKind,
    reportable_binary: String,
    tmux: Option<&str>,
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

    let child_cmd = match tmux {
        Some(name) => {
            if !crate::pty::tmux::is_available() {
                return Err(Error::AgentBinaryMissing(crate::pty::tmux::tmux_bin()));
            }
            crate::pty::tmux::spawn_window_size_fixup(name.to_string());
            crate::pty::tmux::wrap_in_tmux(&child_cmd, name)
        }
        None => child_cmd,
    };
    let mut child = pair.slave.spawn_command(child_cmd).map_err(|e| {
        if is_binary_not_found(&e) {
            Error::AgentBinaryMissing(reportable_binary)
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
        tmux_session: tmux.map(str::to_string),
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
        tmux: Option<&str>,
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
            cwd, cols, rows, mode, remote, agent, identity, tmux,
        )?);
        self.sessions.insert(id, session.clone());
        Ok(session)
    }

    pub fn get(&self, id: crate::data::store::AgentInstanceId) -> Option<Arc<Session>> {
        self.sessions.get(&id).cloned()
    }

    pub fn remove(&mut self, id: crate::data::store::AgentInstanceId) {
        if let Some(s) = self.sessions.remove(&id) {
            s.kill_backend();
        }
    }

    /// Resize every backgrounded running session to `cols × rows` (the
    /// projected single-pane size). Sessions in `visible` are skipped: the
    /// attached render path already keeps those sized every frame, and resizing
    /// one would clip the frame the user is looking at. The PM session lives in
    /// `app.pm`, not here, and is synced separately by
    /// `App::apply_backgrounded_resize`. See `crate::app::resize_sync` for why
    /// this sweep exists.
    pub fn resize_backgrounded(
        &self,
        cols: u16,
        rows: u16,
        visible: &std::collections::HashSet<crate::data::store::AgentInstanceId>,
    ) {
        for (id, session) in &self.sessions {
            let running = matches!(
                *session.status.read().unwrap(),
                SessionStatus::Running { .. }
            );
            // Resize only running, non-visible sessions: the render path keeps
            // visible panes sized, and resizing one would clip the frame in view.
            if running && !visible.contains(id) {
                let _ = session.resize(cols, rows);
            }
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
        let session = Arc::new(spawn_session(
            cwd, cols, rows, mode, remote, agent, None, None,
        )?);
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

    /// Kills a private tmux server on scope exit, so a mid-test panic (a failed
    /// assert) can't leak a running server inside the isolated `TMUX_TMPDIR`.
    /// Holds the tmpdir path directly and passes it explicitly, so it works
    /// regardless of `EnvGuard`'s env-restoration order. Best-effort:
    /// `kill-server` errors (e.g. no server ever started) are ignored.
    struct TmuxServerGuard {
        tmpdir: PathBuf,
    }

    impl Drop for TmuxServerGuard {
        fn drop(&mut self) {
            let _ = std::process::Command::new(crate::pty::tmux::tmux_bin())
                .env("TMUX_TMPDIR", &self.tmpdir)
                .env_remove("TMUX")
                .env_remove("TMUX_PANE")
                .args(["kill-server"])
                .output();
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_command_session_runs_arbitrary_command_through_pty() {
        let mut cmd = portable_pty::CommandBuilder::new("/bin/sh");
        cmd.args(["-c", "printf remote-hello; sleep 5"]);
        cmd.cwd("/tmp");
        let session =
            spawn_command_session(cmd, 80, 24, AgentKind::Claude, "claude".to_string(), None)
                .unwrap();
        let mut seen = false;
        for _ in 0..50 {
            if session
                .parser
                .lock()
                .unwrap()
                .screen()
                .contents()
                .contains("remote-hello")
            {
                seen = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        session.kill();
        assert!(seen, "PTY must deliver the command's output");
        assert!(session.tmux_session.is_none());
    }

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
            None,
        )
        .unwrap();
        s.writer.send(b"hello\n".to_vec()).await.unwrap();
        // Give cat a moment to echo and the reader to process.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("hello"), "screen contents: {screen:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resize_to_zero_rows_is_floored_and_does_not_panic() {
        // A terminal short enough that the projected pane height collapses to 0
        // (≤3 rows) must not crash the vt100 parser: `Grid::set_size` computes
        // `size.rows - 1`, which underflows and panics in debug at rows=0.
        // `Session::resize` floors both dimensions to >=1 to guard this — for
        // both the background sweep and the pre-existing attached render path.
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
            None,
        )
        .unwrap();
        s.resize(80, 0).unwrap();
        let (rows, cols) = s.parser.lock().unwrap().screen().size();
        assert_eq!((rows, cols), (1, 80), "rows floored to 1, cols preserved");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn resize_backgrounded_resizes_hidden_sessions_and_skips_visible() {
        let mut env = EnvGuard::new();
        env.set("WSX_CODEX_BIN", crate::test_support::cat_ignore_args_path());
        let cwd = PathBuf::from(".");
        let fresh = || SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let mut sm = SessionManager::new();
        let hidden = sm
            .spawn(
                crate::data::store::AgentInstanceId(1),
                crate::data::store::WorkspaceId(1),
                &cwd,
                80,
                24,
                fresh(),
                crate::agent::remote_control::RemoteOpts::disabled(),
                AgentKind::Codex,
                None,
            )
            .unwrap();
        let visible_session = sm
            .spawn(
                crate::data::store::AgentInstanceId(2),
                crate::data::store::WorkspaceId(2),
                &cwd,
                80,
                24,
                fresh(),
                crate::agent::remote_control::RemoteOpts::disabled(),
                AgentKind::Codex,
                None,
            )
            .unwrap();

        let visible: std::collections::HashSet<crate::data::store::AgentInstanceId> =
            [crate::data::store::AgentInstanceId(2)]
                .into_iter()
                .collect();
        sm.resize_backgrounded(100, 40, &visible);

        assert_eq!(
            hidden.parser.lock().unwrap().screen().size(),
            (40, 100),
            "hidden running session resized to the projected size"
        );
        assert_eq!(
            visible_session.parser.lock().unwrap().screen().size(),
            (24, 80),
            "visible session left untouched — the render path owns it"
        );
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
                None,
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
    fn spawn_command_session_reports_the_passed_binary_name_on_missing_command() {
        // `attach_remote` runs ssh via this path and passes `ssh_bin()`, so a
        // missing *local* ssh must report "ssh", not the agent it would reach.
        // Drive that decoupling directly: a nonexistent command with an explicit
        // reportable name must surface that exact name in AgentBinaryMissing.
        let cmd = CommandBuilder::new("/no/such/dir/ssh-test-bin-missing");
        let result =
            spawn_command_session(cmd, 80, 24, AgentKind::Claude, "ssh-test-bin".into(), None);
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("spawn should fail when the command is missing"),
        };
        match err {
            Error::AgentBinaryMissing(binary) => assert_eq!(
                binary, "ssh-test-bin",
                "must report the reportable_binary, not the AgentKind default"
            ),
            other => panic!("expected AgentBinaryMissing(\"ssh-test-bin\"), got {other:?}"),
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
            None,
        )
        .unwrap();
        s.writer.send(b"hello-codex\n".to_vec()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let screen = s.parser.lock().unwrap().screen().contents();
        assert!(screen.contains("hello-codex"), "screen: {screen:?}");
    }

    /// Shared-session persistence semantics against a real, private tmux server.
    /// Skips when tmux is absent. TMUX_TMPDIR isolation keeps the user's tmux
    /// server untouched.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_session_survives_client_kill_and_dies_on_kill_backend() {
        if !crate::pty::tmux::is_available() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        let tmpdir = tempfile::tempdir().unwrap();
        let _server_guard = TmuxServerGuard {
            tmpdir: tmpdir.path().to_path_buf(),
        };
        let mut env = EnvGuard::new();
        env.set("TMUX_TMPDIR", tmpdir.path().to_str().unwrap());
        // The tmux client refuses to start without a usable TERM, and CI
        // runners (GitHub ubuntu-latest) leave it unset or "dumb".
        env.set("TERM", "xterm-256color");
        // WSX_CLAUDE_BIN must point at a real script: `/bin/sh` would receive the
        // claude CLI args and reject them. Write a wrapper that ignores args and
        // sleeps so the tmux window keeps a live child.
        let script = tmpdir.path().join("fake-agent.sh");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_CLAUDE_BIN", script.to_str().unwrap());

        let name = "wsx-test-shared";
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let session = spawn_session(
            tmpdir.path(),
            80,
            24,
            mode,
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Claude,
            None,
            Some(name),
        )
        .unwrap();
        // Server-side session appears (client connect is async; poll briefly).
        let mut alive = false;
        for _ in 0..50 {
            if crate::pty::tmux::has_session(name) {
                alive = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(alive, "tmux session was never created");

        // Kill the CLIENT (quit-wsx semantics): backend must survive.
        session.kill();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            crate::pty::tmux::has_session(name),
            "agent died with the client"
        );

        // kill_backend (explicit-kill semantics): backend must die.
        session.kill_backend();
        let mut gone = false;
        for _ in 0..50 {
            if !crate::pty::tmux::has_session(name) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(gone, "kill_backend left the tmux session running");
    }

    /// Pins the `-A` attach-not-duplicate contract: respawning against a
    /// tmux session name that's already alive on the server must reattach
    /// the new client to the existing agent, never spin up a second one.
    /// Same TMUX_TMPDIR isolation as the survive/kill test above.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shared_session_respawn_reattaches_instead_of_duplicating() {
        if !crate::pty::tmux::is_available() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        let tmpdir = tempfile::tempdir().unwrap();
        let _server_guard = TmuxServerGuard {
            tmpdir: tmpdir.path().to_path_buf(),
        };
        let mut env = EnvGuard::new();
        env.set("TMUX_TMPDIR", tmpdir.path().to_str().unwrap());
        // The tmux client refuses to start without a usable TERM, and CI
        // runners (GitHub ubuntu-latest) leave it unset or "dumb".
        env.set("TERM", "xterm-256color");
        // Heartbeat script (instead of a bare `sleep`) so we can prove the
        // *reattached* client actually receives bytes from the still-running
        // agent, not just that the tmux session survives. Bounded to ~120
        // beats so a leaked child can't run forever if the guard is bypassed.
        let script = tmpdir.path().join("fake-agent.sh");
        std::fs::write(
            &script,
            "#!/bin/sh\nfor i in $(seq 1 120); do echo beat; sleep 1; done\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_CLAUDE_BIN", script.to_str().unwrap());

        let name = "wsx-test-reattach";
        let mode = || SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };

        let s1 = spawn_session(
            tmpdir.path(),
            80,
            24,
            mode(),
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Claude,
            None,
            Some(name),
        )
        .unwrap();
        let mut alive = false;
        for _ in 0..50 {
            if crate::pty::tmux::has_session(name) {
                alive = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(alive, "tmux session was never created");

        // Kill only the client (quit-wsx semantics) — the agent keeps running
        // in the tmux server, same as a user quitting wsx on a shared session.
        s1.kill();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            crate::pty::tmux::has_session(name),
            "agent died with the client"
        );

        // Respawn against the SAME session name: `-A` must attach to the
        // still-running server session rather than creating a second one.
        let s2 = spawn_session(
            tmpdir.path(),
            80,
            24,
            mode(),
            crate::agent::remote_control::RemoteOpts::disabled(),
            AgentKind::Claude,
            None,
            Some(name),
        )
        .unwrap();

        // Poll: the reattached client's parser eventually receives bytes from
        // the pre-existing agent (its heartbeat), proving it attached to the
        // live server session instead of a fresh, silent one.
        let mut saw_beat = false;
        for _ in 0..50 {
            let screen = s2.parser.lock().unwrap().screen().contents();
            if screen.contains("beat") {
                saw_beat = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(saw_beat, "reattached client never saw agent output");

        // Exactly one tmux session named `name` exists — no duplicate was
        // spun up by the second spawn. Scrub TMUX/TMUX_PANE like
        // `tmux::tmux_cmd()` does, so this targets the isolated test server
        // even if the test happens to run inside a tmux session itself.
        let ls = std::process::Command::new(crate::pty::tmux::tmux_bin())
            .env_remove("TMUX")
            .env_remove("TMUX_PANE")
            .args(["ls", "-F", "#{session_name}"])
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&ls.stdout);
        let matches = listing.lines().filter(|l| *l == name).count();
        assert_eq!(
            matches, 1,
            "expected exactly one session {name:?}, got:\n{listing}"
        );

        s2.kill_backend();
        let mut gone = false;
        for _ in 0..50 {
            if !crate::pty::tmux::has_session(name) {
                gone = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(gone, "kill_backend left the tmux session running");
    }
}
