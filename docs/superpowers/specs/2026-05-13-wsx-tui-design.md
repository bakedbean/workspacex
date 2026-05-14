# wsx — TUI for managing Claude Code sessions in git worktrees

**Status:** approved design
**Date:** 2026-05-13
**Working name:** `wsx` (placeholder; rename freely)

## Background and motivation

Claudette (`/home/eben/Claudette`) is a cross-platform Tauri desktop app that manages parallel Claude Code sessions in isolated git worktrees. Its conversational features depend on the Anthropic Claude SDK, which Anthropic is no longer permitting under paid subscription plans. Most of Claudette's value — multi-workspace orchestration, setup scripts, agent dashboards — is independent of how the agent itself runs.

`wsx` is a terminal UI that preserves the **workspace management** value of Claudette but delegates the entire conversation to the standard `claude` CLI running inside a PTY. The TUI never parses Claude's output; it multiplexes terminals.

## Goals (v1)

1. Registry of git repositories the user works in.
2. CRUD over git worktrees attached to those repos ("workspaces").
3. Run the repo's `.claudette.json` setup script on workspace creation and archive script on removal — format-compatible with Claudette so existing repos work unchanged.
4. Spawn `claude` as a child process inside each workspace's worktree, multiplexed inside the TUI so several can run concurrently.
5. Dashboard view across all workspaces showing branch, dirty/ahead/behind, agent status, last activity.

## Non-goals (v1)

- Env-provider stack (direnv, mise, dotenv, nix-devshell). The user's shell env at TUI launch is what `claude` inherits. Defer.
- Daemon / IPC / remote workspaces. Sessions die when the TUI exits.
- Parsing Claude's TUI output for plan-mode, AskUserQuestion handling, checkpoints, or any other in-band protocol. Claude Code handles those itself.
- MCP supervision, SCM/PR integration, voice input, alternative providers — all deferred or dropped.
- Sharing state with Claudette. wsx owns its own SQLite database.

## Locked-in decisions

| Decision | Choice |
|---|---|
| Language / TUI framework | Rust + Ratatui |
| PTY library | `portable-pty` |
| Terminal emulator | `vt100` crate (parses ANSI into a screen model) |
| Async runtime | Tokio (multi-thread) |
| State store | SQLite via `rusqlite`, WAL mode, two tables |
| Session model | TUI owns PTYs in-process; sessions die on TUI exit |
| Detach key | `Ctrl-a d` (tmux-style) |
| Claudette code reuse | Vendor a thin slice (`git`, `names`); no crate dependency on Claudette |
| `.claudette.json` compatibility | Read same file shape; ignore fields we don't implement |

## Architecture

### Module layout

Single binary, internal modules. Each module has one responsibility and depends only on layers below it: `ui` → `app` → (`workspace`, `pty`, `store`) → (`git`, `setup`, `names`).

```
wsx/
├── Cargo.toml
└── src/
    ├── main.rs             # tokio runtime entry, crossterm init, panic hook
    ├── app.rs              # App state, event loop, key dispatch
    ├── config.rs           # XDG dirs, settings.toml load/save
    ├── store.rs            # repo + workspace metadata persistence
    ├── git/                # vendored thin slice of claudette::git
    │   ├── mod.rs
    │   └── worktree.rs
    ├── names.rs            # vendored from claudette::names
    ├── repo.rs             # repo registry domain logic
    ├── workspace.rs        # workspace lifecycle: create/archive/import/discover
    ├── setup.rs            # .claudette.json parsing + setup/archive script runner
    ├── pty/                # PTY layer
    │   ├── mod.rs          # SessionManager, public API
    │   ├── session.rs      # one session = one claude PTY + vt100::Parser
    │   └── render.rs       # vt100 -> Ratatui Buffer translation
    └── ui/                 # Ratatui views
        ├── mod.rs
        ├── dashboard.rs    # workspace list view
        ├── attached.rs     # full-screen attached PTY view
        ├── modal.rs        # create/confirm/error modals
        └── theme.rs
```

### Process model

Single binary. Tokio multi-thread runtime. The main task runs the Ratatui event loop reading crossterm events; `SessionManager` owns per-session reader/writer tokio tasks. Setup/archive scripts run as one-shot `tokio::process::Command` invocations whose output is streamed into a transient modal via a bounded `mpsc::channel(256)`.

## Components

### `store` — durable metadata

```rust
pub struct Store { conn: rusqlite::Connection }

impl Store {
    pub fn open(path: &Path) -> Result<Self>;          // runs migrations idempotently
    pub fn repos(&self) -> Result<Vec<Repo>>;
    pub fn add_repo(&self, path: &Path, name: &str) -> Result<RepoId>;
    pub fn remove_repo(&self, id: RepoId) -> Result<()>;
    pub fn workspaces(&self, repo: RepoId) -> Result<Vec<Workspace>>;
    pub fn insert_workspace(&self, w: &NewWorkspace) -> Result<WorkspaceId>;
    pub fn delete_workspace(&self, id: WorkspaceId) -> Result<()>;
    pub fn rename_workspace(&self, id: WorkspaceId, name: &str) -> Result<()>;
    pub fn set_workspace_state(&self, id: WorkspaceId, state: WorkspaceState) -> Result<()>;
    pub fn set_setup_status(&self, id: WorkspaceId, status: SetupStatus) -> Result<()>;
    pub fn sweep_stale_pending(&self, older_than: Duration) -> Result<usize>;
}
```

Schema (two tables):

```sql
CREATE TABLE repos (
    id            INTEGER PRIMARY KEY,
    name          TEXT NOT NULL,
    path          TEXT NOT NULL UNIQUE,
    branch_prefix TEXT NOT NULL DEFAULT '',
    created_at    INTEGER NOT NULL
);

CREATE TABLE workspaces (
    id             INTEGER PRIMARY KEY,
    repo_id        INTEGER NOT NULL REFERENCES repos(id),
    name           TEXT NOT NULL,
    branch         TEXT NOT NULL,
    worktree_path  TEXT NOT NULL UNIQUE,
    state          TEXT NOT NULL,   -- 'Pending' | 'Ready' | 'Failed' | 'Orphaned'
    setup_status   TEXT NOT NULL,   -- 'NotRun' | 'Skipped' | 'Ok' | 'Failed'
    created_at     INTEGER NOT NULL
);
```

Located at `$XDG_STATE_HOME/wsx/state.db` (fallback `~/.local/state/wsx/state.db`). WAL journal mode so a slow workspace-import scan doesn't block the UI thread.

### `git` — vendored worktree helpers

Public surface: `create_worktree`, `remove_worktree`, `restore_worktree`, `current_branch`, `head_commit`, `commits_since`, `WorktreeInfo`. Every function shells out to the `git` binary via `tokio::process::Command`; no `git2`/libgit2 dependency. Errors bubble as a `GitError` enum with stderr captured.

Preflight check at startup: verify `git --version` succeeds; otherwise refuse to start with a clear message.

### `setup` — `.claudette.json` runner

```rust
pub struct RepoConfig { setup: Option<ScriptSpec>, archive: Option<ScriptSpec> }
pub struct ScriptSpec { command: String, args: Vec<String>, env: HashMap<String,String> }

pub async fn run_setup(
    repo_root: &Path,
    worktree: &Path,
    on_line: impl FnMut(SetupLine),
) -> Result<SetupResult>;

pub async fn run_archive(
    repo_root: &Path,
    worktree: &Path,
    on_line: impl FnMut(SetupLine),
) -> Result<SetupResult>;
```

Reads `.claudette.json` from the **source repo root**, spawns the script with `cwd = <worktree>` and env vars `WSX_WORKTREE` and `WSX_REPO_ROOT`. Streams stdout/stderr line-by-line into the caller's closure. Missing file or no setup block → `SetupResult::Skipped`. Fields wsx doesn't implement (env-providers, MCP hooks) are ignored with a single info-level log line, not an error.

### `workspace` — lifecycle orchestration

```rust
pub async fn create(store: &Store, repo: &Repo, name: Option<&str>) -> Result<CreatedWorkspace>;
pub async fn archive(store: &Store, ws: &Workspace, opts: ArchiveOpts) -> Result<()>;
pub async fn import_existing(store: &Store, repo: &Repo) -> Result<Vec<Workspace>>;
pub async fn discover(repo: &Repo) -> Result<Vec<WorktreeInfo>>;
```

`create`: resolve worktree base dir → generate branch suffix via `names` if no name given → insert DB row as `Pending` → `git worktree add` → mark `Ready` → run setup script (caller drives modal) → mark `setup_status`.

`archive`: optional `git worktree remove` → branch delete if no unmerged commits or `--force` → row delete → run archive script.

Atomicity: DB row inserted **before** the git operation so there's never an orphaned worktree the registry doesn't know about. The reverse (worktree without row) is what `discover`/`import` is for. Setup script run is **not** atomic with the DB row — failure leaves the workspace `Ready` but `setup_status = Failed`.

Startup sweep: `Pending` rows older than 5 minutes are marked `Orphaned`; surfaced in dashboard for manual cleanup.

### `pty::SessionManager` — owns all live `claude` PTYs

```rust
pub struct SessionManager { sessions: HashMap<WorkspaceId, Arc<Session>> }

pub struct Session {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: mpsc::Sender<Vec<u8>>,         // stdin bytes
    status: Arc<RwLock<SessionStatus>>,    // Running { pid } | Exited { code }
    activity: Arc<AtomicU64>,              // last-output timestamp (ms epoch)
}

impl SessionManager {
    pub fn spawn(&mut self, ws: &Workspace) -> Result<Arc<Session>>;
    pub fn get(&self, id: WorkspaceId) -> Option<Arc<Session>>;
    pub fn kill(&mut self, id: WorkspaceId) -> Result<()>;
    pub fn kill_all(&mut self);
}
```

Spawning a session: `portable_pty::native_pty_system().openpty(...)`; launch `claude` (or `WSX_CLAUDE_BIN` if set) with `cwd = <worktree>` and the current process's env; fork two tokio tasks:

- **Reader**: loops `master.read()` → `parser.lock().process(&buf)` → `activity.store(now_ms, Ordering::Relaxed)`. EOF → `SessionStatus::Exited { code }`, task ends.
- **Writer**: drains the mpsc into `master.write()`.

PTY size is whatever the UI most recently reported via `resize(cols, rows)`. Resize events are coalesced so window-drag doesn't lock-thrash the parser.

### `ui` — Ratatui views

Three views, exactly one active at a time:

- **Dashboard**: tree of repos and their workspaces. Per-row columns: name, branch, dirty-file count, ahead/behind counts, session-status dot (off / idle / active / waiting), last-activity age.
- **Attached**: full-screen render of one session's `vt100::Parser` screen. Keys forward to the session writer except the `Ctrl-a` prefix:
  - `Ctrl-a d` → detach to dashboard
  - `Ctrl-a a` → forward literal `Ctrl-a` to claude
- **Modal**: stacked overlay used for create-workspace, confirm-archive, setup-script-running (with streaming log), and error display.

Activity detection: derived from time-since-last-PTY-byte-received, not from parsing Claude's output. `<2s` = active, `2–30s` = idle, `>30s with no user input` = waiting. We never match strings against Claude's TUI output.

UI re-renders on a fixed tick (60Hz via `tokio::time::interval`) rather than change-driven. vt100 doesn't notify on screen change; the tick keeps the loop simple at trivial cost.

## Data flow

### Flow A — Create a new workspace

```
User on Dashboard presses [n]
  ui::dashboard           --> AppEvent::NewWorkspace(repo_id)
  app::handle_event       --> open Modal::NewWorkspace { repo_id, name_buffer }
User types name, presses Enter
  ui::modal               --> AppEvent::ConfirmCreate { repo_id, name }
  app::handle_event       --> tokio::spawn(workspace::create(...))
  workspace::create
    1. store.insert_workspace(state=Pending, setup_status=NotRun)
    2. git::create_worktree(repo.path, branch, path)
    3. store.set_workspace_state(Ready)
    4. setup::run_setup(repo, worktree, on_line)
         on_line --> mpsc --> Modal::SetupRunning appends scrollback
    5. store.set_setup_status(Ok|Failed|Skipped)
       mpsc --> AppEvent::WorkspaceCreated { ws }
  app::handle_event       --> close modal, refresh dashboard row
```

Properties:
- Each `await` is a possible cancellation point; `Pending` rows are recovered by the startup sweep.
- mpsc is bounded (256) so a runaway script doesn't balloon memory.
- DB write precedes git op — no orphan-worktree-without-row state.

### Flow B — Attach to a session, type, detach

```
User on Dashboard, cursor on workspace W, presses [enter]
  ui::dashboard           --> AppEvent::Attach(W.id)
  app::handle_event
    if !sm.has(W.id):
        sm.spawn(W) ::= openpty + spawn `claude` + start reader/writer tasks
    app.view = View::Attached(W.id)

Each frame (60Hz):
  ui::attached.render
    snapshot = parser.lock().screen().clone()
    walk cells -> ratatui::Buffer
    set cursor to snapshot.cursor_position()

Each crossterm key event in Attached view:
  if key == Ctrl-a:
      await next key:
          'd' -> AppEvent::Detach  (back to dashboard, session keeps running)
          'a' -> forward literal Ctrl-a
  else:
      encode key to bytes, session.writer.send(bytes)
  on Resize:
      master.resize(cols, rows); parser.set_size(rows, cols)
```

Detach does not kill the session — reader task keeps consuming output; on re-attach the screen is up-to-date.

### Flow C — TUI shutdown

```
Clean exit ([q] from dashboard):
  app::run loop breaks
  drop(SessionManager)
    for each Session: child.kill(); abort reader/writer tasks
  crossterm: leave alternate screen, disable raw mode
  store: WAL checkpoint

Panic anywhere:
  std::panic::set_hook installed at main():
    1. crossterm restore (alt-screen off, raw mode off, cursor on)
    2. SessionManager (in OnceCell) -> kill_all()
    3. eprintln! panic payload to the cooked terminal
    4. exit(1)
```

The panic-hook → SessionManager path is the one place we deliberately use global state. Acceptable cost: alternative is leaking child processes on every crash during development.

## Error handling

Principle: **fail loud at the boundary, never leave the user guessing**. No swallowing, no silent retries.

### Taxonomy

```rust
pub enum Error {
    Git(GitError),
    Store(rusqlite::Error),
    Pty(PtyError),
    Setup(SetupError),
    Io(io::Error),
    UserInput(String),  // recoverable; shown inline in modal
}
```

Only `UserInput` is expected in normal flow. Everything else → red error modal dismissible with Esc.

### Per-failure handling

| Failure | Where | Handling |
|---|---|---|
| `git worktree add` fails | `workspace::create` step 2 | Row stays `Pending`, marked `Failed` with stderr; dashboard shows red; `[d]` removes the row. |
| Setup script exits non-zero | `setup::run_setup` | Worktree exists, row `Ready` but `setup_status = Failed`. Yellow badge; `[r]` re-runs; `[enter]` still attaches a session — setup failure does not block usage. |
| `claude` binary not on PATH | `pty::SessionManager::spawn` | `PtyError::ClaudeNotFound`. Modal explains how to install Claude Code. Preflight check at startup too. |
| PTY EOF (claude exited) | reader task | `SessionStatus::Exited { code }`. Dashboard shows exit code; `[enter]` re-spawns. |
| SQLite locked / corrupt | `Store::open` | Fatal at startup; stderr + exit 1. No recovery attempt. |
| Crossterm event channel broken | main loop | Fatal — TUI can't function without input. |
| `.claudette.json` malformed | `setup::run_setup` | `Setup(InvalidConfig)` — non-fatal; warning badge on repo; workspaces still usable, setup doesn't run. |
| Disk full mid-write | `Store` writes | Surfaces as `Store(rusqlite::Error)`; modal; SQLite rolls back the transaction. |
| Panic anywhere | global hook | Restore terminal, kill children, print panic, exit 1. |

### Deliberate non-choices

- **No retry loops.** Failed git ops are shown, not retried.
- **No fallbacks for missing tools.** No `git` → tell the user.
- **No partial-state rollback inside failing ops.** Startup sweep handles `Pending` orphans.
- **No catching panics inside session tasks.** A panicking reader = session dead = user re-attaches to respawn.

### Logging

`tracing` to `$XDG_STATE_HOME/wsx/logs/wsx-<date>.log`. Default `info`; `RUST_LOG=wsx=debug` for verbose. Logs **never** go to stderr while the TUI is in alternate-screen mode.

## Testing

Strategy: unit-test the pure pieces, integration-test the impure pieces against a real filesystem and real `git` binary, single smoke test for the event loop.

| Layer | Style | What we test | Mocking |
|---|---|---|---|
| `git` | Integration vs. real `git` in tempdir | Worktree create/remove/restore, branch listing, stderr-pattern error mapping | None — `git` must be on PATH (matches runtime) |
| `store` | Integration vs. `:memory:` and on-disk SQLite | Idempotent migrations, CRUD round-trips, WAL, FK behavior | None |
| `names` | Unit | Seed → expected suffix, collision rejection | None |
| `setup` | Integration, real subprocess | `.claudette.json` parsing (valid + malformed), script stdout/stderr/exit-code variations, env-var injection, missing-file = skipped | `sh -c '…'` test scripts |
| `workspace` | Integration | End-to-end `create` → `archive` lifecycle in tempdir; `Pending` sweep; setup-fail → workspace `Ready` w/ `setup_failed` | Real Store, real git |
| `pty` | Integration | Spawn `/bin/cat`, write bytes, assert parser screen state; `/bin/false` → `Exited { code: 1 }`; resize round-trip | `cat`/`false`/`sh` — NOT `claude` |
| `pty::render` | Unit | vt100::Screen → ratatui::Buffer translation: SGR colors, cursor pos, wide chars, cursor visibility | Canned ANSI byte sequences |
| `ui::*` | Snapshot via `ratatui::backend::TestBackend` | Dashboard layout, modal stack rendering, attached view from canned screen | TestBackend → 2D char grid diffed against fixtures |
| `app` | One smoke test | Boot event loop with `TestBackend`, scripted key sequence, assert final view | `WSX_CLAUDE_BIN=cat`, in-memory Store, git tempdir |

### Explicitly not tested

- Real `claude` interaction (Claude's UI changes; our tests would break).
- `vt100` internals (trust the upstream crate).
- Slow-filesystem behavior (disk full, read-only) — we test the modal that surfaces those errors.

### The `WSX_CLAUDE_BIN` escape hatch

The PTY layer reads `WSX_CLAUDE_BIN` (default: `claude`) before spawning. This is the single seam that makes the app smoke-testable: tests set it to `cat` or a bash script. Production leaves it unset. No `cfg(test)` branching — same code path, different env.

### CI

- `cargo test --workspace`
- `cargo clippy --all-targets -- -D warnings`
- `cargo fmt --check`
- `cargo build --release`

No GUI snapshot-image testing, no fuzzing, no property-based testing in v1.

## Open questions / deferred

- **Project name.** `wsx` is a placeholder.
- **Branch-prefix UX.** Claudette has user/repo/custom prefix modes. v1 ships with a single repo-level prefix field; revisit if users need more.
- **Env-provider integration.** Deferred — bring back per-provider plugins (direnv/mise/dotenv) once core is stable, behind an explicit per-repo opt-in.
- **Future daemon split.** If session-survives-TUI becomes important, the clean split is `SessionManager` → its own binary speaking a Unix-socket protocol; the rest of the TUI doesn't need to change.
