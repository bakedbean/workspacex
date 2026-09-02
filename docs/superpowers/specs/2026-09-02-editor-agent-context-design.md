# Editor-agent context digest (`wsx context`) — Design

## Problem

The user drops into neovim from the wsx dashboard (`e` / `v`) to read diffs
and make light edits, and uses an editor-hosted AI agent there
(magenta.nvim). That agent knows nothing about wsx: not the workspace's goal,
not the status the primary agent last reported, not which peers exist, and
not what the primary agent was just doing. The reverse is also true: the wsx
agent does not know a second agent may be editing the same worktree.

Two facts make a cheap bridge possible:

- magenta.nvim re-reads every tracked context file before each provider
  request (`file-context-supervisor.onBeforeRequest`), so a file that wsx
  keeps fresh is live context, not a snapshot. Adding the same path twice is
  a no-op (`ContextManager.addFileContext` returns early on a known path).
- The wsx CLI resolves the current workspace from cwd when
  `WSX_WORKSPACE_ID` is absent (`src/cli/resolve.rs`). The editor is launched
  in a fresh iTerm tab via osascript, so env vars never reach it; cwd
  resolution is the only path and it already works.

Everything wsx knows lives in `~/.local/state/wsx/state.db` (recap, status,
agents) and in the agents' session transcripts (parsed by the TUI's tail
loop). Nothing today renders that to a file or to stdout in one piece.

## Goal

1. `wsx context show` prints a markdown digest of the current workspace.
2. `wsx context write` renders the same digest to a stable per-workspace path
   under the wsx state dir and prints that path.
3. The digest ends with an **External instructions** block that tells an
   editor-hosted agent how to behave in a wsx workspace and how to report
   back to the primary agent.
4. A new doctrine clause tells wsx-spawned agents that an editor-hosted agent
   may edit the worktree and will message them via `wsx agent send`.
5. A docs page documents the command and carries a reference nvim snippet
   that keeps the file fresh and hands it to magenta.nvim.

Editor-agnostic on purpose: the wsx side knows nothing about neovim or
magenta. Cursor / VS Code support later is a new docs section, not a code
change.

## Non-goals

- JSON output for `context`, recap, or status.
- PR lifecycle, undelivered peer messages, or diff contents in the digest.
- Pushing the file from inside `recap set` / `status set` / `agent send`.
  The editor pulls on focus (decided in design review); the primary agent's
  last message changes without any wsx event anyway.
- Plumbing `WSX_WORKSPACE_ID` into the editor / terminal / diff spawn.
- An MCP server. The instructions block plus the already-installed wsx skill
  give the editor agent the CLI vocabulary.
- A `wsx setup install-nvim`. The snippet lives in docs and in the user's
  own config.

## CLI surface

New group `context`, registered in `src/cli/groups.rs` and dispatched in
`src/cli/parse/mod.rs` → `src/cli/parse/reporting.rs` beside `recap`:

```
wsx context show     Print the workspace context digest (markdown)
wsx context write    Write the digest to <state>/wsx/context/<repo>/<slug>.md and print the path
```

- Both resolve the workspace with `resolve_current_workspace` (env var, else
  cwd). No `--workspace` flag; the editor is always inside the worktree.
- No other flags. Extra arguments are a usage error, matching `recap show`.
- `write` creates parent dirs, writes to a sibling temp file, then renames
  over the target so a concurrent reader never sees a partial file. It prints
  the absolute path on stdout and nothing else.
- Exit non-zero only when the workspace cannot be resolved or the file
  cannot be written. Missing transcript, git failure, missing recap, and
  missing status are all rendered as absent sections or `-`, never errors.

`src/config/mod.rs` gains `context_dir() -> <app_dir>/context`. The
`storage-and-config-files.md` reference page lists it.

## Digest format

Same bytes from `show` and `write`. Markdown, sections in this order. A
section whose data is absent is omitted entirely except where a `-` is
shown below.

```
# wsx workspace: workspacex/magenta-nvim-context

- branch: bakedbean/magenta-nvim-context (base: origin/main)
- worktree: /Users/eben/.local/state/wsx/worktrees/workspacex/magenta-nvim-context
- agents: claude (primary), claude#2
- status: working — "writing the spec" (claude, 4m ago)

## Recap

- goal: Add 'wsx context show/write' digest …
- state: design approved; writing spec
- next: user reviews spec

## Recent commits (origin/main..HEAD)

- ff2a9ad fix(input): swallow Ctrl-D in attached views …
- 801794a fix(status): stop active pi/omp workspaces reading Idle …

Uncommitted: 3 modified, 1 untracked

## Primary agent's last message

<last assistant text, verbatim markdown, truncated>

## External instructions

You are an editor-hosted agent working inside a wsx-managed worktree. The
agents listed above share this branch and this working tree with you, and
one of them (the primary) owns this workspace's status and recap.

- Before editing, run `git status` and `git diff`; the primary agent may
  have changed files since this digest was written.
- Keep edits small and scoped. Do not create branches, rename the
  workspace, or run `wsx status set` / `wsx recap set`; those belong to the
  primary agent.
- When you finish a change, or when you need a decision the primary agent
  should make, report it with:
  `wsx agent send <primary label> "<one-paragraph summary>"`
  Run it from this worktree; the workspace is resolved from cwd.
- This file is regenerated by `wsx context write`; do not edit it.
```

Rules:

- **Header.** `<repo name>/<workspace name>`. Base ref from
  `git::resolve_base_branch` (falls back to `main`).
- **agents.** Every `AgentInstance` for the workspace, labels from
  `AgentInstance::label()`, primary first, `(primary)` suffix. If the
  roster is empty the line reads `agents: -`.
- **status.** From `workspace_status`: `state — "message" (source, age)`.
  Message omitted when empty, age from `reported_at` in the coarse
  `Nm ago` / `Nh ago` / `Nd ago` style. `-` when no status.
- **Recap.** Full fields only (goal / state / next); short forms are
  dashboard-only. Missing fields render `-`. Section omitted when there is
  no recap row.
- **Recent commits.** `git log --oneline --no-decorate <base>..HEAD`
  capped at 20 lines. Section omitted when the log is empty or git fails.
  The `Uncommitted:` line uses the existing porcelain parser
  (`git::workspace_status`); it reads `Uncommitted: clean` when both
  counts are zero and is omitted when git fails.
- **Primary agent's last message.** Locate the primary instance's agent
  kind (falls back to the workspace's `agent` when no instance exists),
  locate its session file for this worktree, tail it from offset 0, and
  take `TailUpdate::last_assistant_text`. Truncate to 2000 chars on a char
  boundary and append `… [truncated]`. Section omitted when no transcript
  or no assistant text.
- **External instructions.** Constant text. `<primary label>` is
  substituted with the real label (e.g. `claude`); when there is no
  primary, `primary` is used, which `wsx agent send` also accepts.

The section title says "Primary agent's" even when the workspace is
single-agent; that is accurate and keeps the format stable.

## Code layout

### `src/commands/context.rs` (new)

```rust
pub struct ContextDigest {
    pub repo_name: String,
    pub workspace_name: String,
    pub branch: String,
    pub base_ref: String,
    pub worktree: PathBuf,
    pub agents: Vec<AgentInstance>,      // primary first
    pub status: Option<ReportedStatus>,
    pub recap: Option<WorkspaceRecap>,
    pub commits: Vec<String>,            // already "sha subject" lines
    pub uncommitted: Option<git::WorkspaceStatus>,
    pub last_assistant_text: Option<String>,
    pub now_ms: i64,                     // for age rendering; injectable in tests
}

pub async fn gather(store: &Store, ws: &Workspace) -> Result<ContextDigest>;
pub fn render(d: &ContextDigest) -> String;
pub fn digest_path(dirs: &Dirs, repo_name: &str, workspace_name: &str) -> PathBuf;
pub fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()>;
```

`render` is pure and fully unit-tested against hand-built structs: every
section present, every section absent, truncation boundary, `-`
fallbacks, primary-label substitution. `gather` is exercised by one
integration-style test using `Store::open` on a temp dir and a temp git
repo (the existing CLI tests already do this for `recap`).

### `src/activity/mod.rs` (extract)

`src/app/background.rs::tail_workspace_events` has two `match ws_agent`
blocks choosing the locate and tail function per `AgentKind`. Extract them:

```rust
pub fn locate_session_file_for(kind: AgentKind, worktree: &Path) -> Option<PathBuf>;
pub fn tail_session_for(kind: AgentKind, path: &Path, offset: u64) -> Result<events::TailUpdate>;
```

`background.rs` calls these; behavior unchanged. This is the only edit to
existing runtime code beyond CLI wiring. `TailUpdate` is the sessionx type
already re-exported through `crate::activity::events`.

### `src/git/mod.rs` (add)

```rust
pub async fn log_oneline(worktree: &Path, base: &str, limit: usize) -> Result<Vec<String>>;
```

Runs `git log --oneline --no-decorate -n <limit> <base>..HEAD` via the
module's existing `run` helper. Unit-tested with a temp repo, including the
empty-range case.

### CLI wiring

- `src/cli/action.rs`: `CliAction::ContextShow`, `CliAction::ContextWrite`.
- `src/cli/parse/reporting.rs`: `parse_context`, same shape as
  `parse_recap` (`show` / `write` / unknown → usage error).
- `src/cli/parse/mod.rs`: `"context" => parse_context(...)`.
- `src/cli/groups.rs`: `GroupInfo { name: "context", … }` with both
  commands. The `registry_matches_dispatched_groups` test enforces this.
- `src/cli/run.rs`: `ContextShow` prints `render(&gather(..)?)`;
  `ContextWrite` renders, writes atomically to `digest_path`, prints the
  path.
- `src/cli/tests.rs`: parser cases for both subcommands and the usage
  error.

## Reverse direction

### Doctrine clause (`src/agent/doctrine.rs`)

New constant, appended to the clause list for every agent kind:

```
- An editor-hosted agent (for example magenta.nvim inside neovim) may work
  in this worktree alongside you. It reads a digest of your recap and status
  produced by `wsx context write`, shares this branch and working tree, and
  reports back to you with `wsx agent send` (its messages arrive with a
  `[message]` banner and no sender label). Before assuming the tree matches
  your last edit, check `git status` and `git diff`, and treat those
  messages as the user's own follow-up instructions.
```

Existing doctrine tests that snapshot clause order / count are updated;
the `process_doctrine` override and `process_doctrine_extra` append paths
are untouched.

### wsx skill (`skills/wsx/SKILL.md`)

New `## External editor agents` section, two short paragraphs: what the
digest is, that editor-side messages arrive unlabelled, and the same
`git status` advice. Kept short because the doctrine already carries the
rule; the skill is the reference the agent consults on demand.

## Editor glue (docs only)

New page `docs/book/src/integrations/editor-agent-context.md`, linked from
`SUMMARY.md` under "Integrations and remote access", and cross-linked from
`editor-terminal-diff.md`. It covers:

1. What `wsx context show` / `write` produce and where the file lives.
2. The External instructions block, verbatim, so users of other editors
   know what the file asks of their agent.
3. The reverse-direction doctrine clause, so readers know why the wsx
   agent mentions an editor agent.
4. A reference neovim + magenta.nvim snippet:

```lua
-- wsx: keep the workspace context digest fresh and hand it to magenta.nvim
local worktrees = vim.fn.expand("~/.local/state/wsx/worktrees/")
local added = false

local function magenta_sidebar_visible()
  for _, win in ipairs(vim.api.nvim_list_wins()) do
    local name = vim.api.nvim_buf_get_name(vim.api.nvim_win_get_buf(win))
    if name:find("Magenta Input", 1, true) then return true end
  end
  return false
end

local function wsx_context()
  if not vim.startswith(vim.fn.getcwd(), worktrees) then return end
  vim.system({ "wsx", "context", "write" }, { text = true }, function(out)
    if out.code ~= 0 then return end
    local path = vim.trim(out.stdout)
    if added or path == "" then return end
    vim.schedule(function()
      if magenta_sidebar_visible() then
        vim.cmd("Magenta context-files " .. vim.fn.fnameescape(path))
        added = true
      end
    end)
  end)
end

vim.api.nvim_create_autocmd({ "VimEnter", "FocusGained" }, { callback = wsx_context })
vim.api.nvim_create_user_command("WsxContext", function()
  added = false
  wsx_context()
end, {})
```

Behavior: the file is rewritten on every `VimEnter` and `FocusGained`
(cheap: one sqlite read, two git commands, one transcript scan). It is
added to magenta only once, and only when the sidebar is already visible,
because `:Magenta context-files` toggles the sidebar open if it is hidden.
`:WsxContext` forces a rewrite and re-add. Because magenta diffs tracked
files before each request, later rewrites reach the agent without any
further nvim action.

The same snippet is added to the user's `~/.config/nvim/lua/polish.lua`
as part of this work (outside the repo; noted in the final report).

## Error handling

| Situation | `show` / `write` behavior |
|---|---|
| cwd not inside a wsx worktree and no env var | usage-style error, exit 1 (same as `recap show`) |
| worktree path missing on disk | digest still renders from DB; commits / uncommitted / transcript sections omitted |
| git not installed or command fails | those sections omitted, exit 0 |
| no transcript for the primary agent kind | last-message section omitted |
| transcript parse error | last-message section omitted; error logged at debug |
| `write` cannot create dir / rename | error, exit 1 |

## Testing

- `render` unit tests (pure): full digest, minimal digest, each optional
  section absent, truncation exactly at the boundary, primary label
  substitution with and without a primary.
- `log_oneline` test with a temp repo: two commits ahead of base, and
  zero.
- `parse_context` tests: `show`, `write`, missing subcommand, unknown
  subcommand, trailing garbage.
- `registry_matches_dispatched_groups` passes with the new group.
- Doctrine tests updated for the new clause; `process_doctrine` override
  still suppresses it.
- One end-to-end CLI test: temp store + temp worktree with a recap, a
  status, and one agent instance; `context write` produces a file at the
  expected path whose contents equal `context show` stdout.
- Manual: from this worktree, `wsx context show`, then open nvim with the
  snippet, open the magenta sidebar, `:WsxContext`, and confirm the file
  appears in magenta's context list and the agent can answer "what is this
  workspace's goal?".

## Commits

1. `refactor(activity): extract per-agent locate/tail helpers from background tail loop`
2. `feat(git): add log_oneline helper`
3. `feat(context): add ContextDigest gather/render with tests`
4. `feat(cli): add wsx context show/write`
5. `feat(doctrine): tell agents an editor-hosted agent may share the worktree`
6. `docs: editor agent context page, nvim snippet, storage reference`
