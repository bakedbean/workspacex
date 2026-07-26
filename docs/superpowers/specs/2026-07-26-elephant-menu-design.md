# Elephant Lua Menu for the wsx Workspace Picker

Date: 2026-07-26
Status: approved for planning

## Goal

Upgrade the waybar workspace picker from a plain `walker --dmenu` text list to a
rich walker/elephant menu: two-line rows with an agent-status icon, git branch,
PR state + number, and dirty/diff indicators — with rows refreshing live while
the menu is open. Keep the existing dmenu pipe as an automatic fallback.

## Background (verified against sources)

- Walker (2.16) dmenu mode is one plain-text string per stdin line: no
  separators, no per-line icon/subtext, no Pango markup (`walker/src/main.rs`
  copies each line verbatim into `item.text`; labels render via `set_text`).
- Walker is a frontend for **elephant**; elephant's `menus` provider supports
  per-entry `Text`, `Subtext`, `Icon`, `Preview`, multiple `Actions`, and
  dynamic generation from a Lua file in `~/.config/elephant/menus/` with a
  `GetEntries()` function. Omarchy's theme picker
  (`~/.config/elephant/menus/omarchy_themes.lua`, launched as
  `walker -m menus:omarchythemes`) is the working template.
- Elephant's Lua runtime (gopher-lua) registers a `jsonDecode` global —
  the shim can consume JSON from wsx directly.
- Per-entry `async` (elephant `internal/providers/menus/setup.go`): runs the
  command via `sh -c` in a goroutine after entries are served, then replaces
  **only the entry's text line** with trimmed stdout and pushes an in-place
  update to walker. Command failure sets the text to `%DELETE%` (row removed).
  The async output also overwrites the entry's `value`.
- wsx already has PR machinery: `git/forge.rs` (`BranchLifecycle`:
  NoPr/PrDraft/PrOpen/PrConflicted/PrMerged/PrClosed, `fetch_pr_status` via
  `gh pr view --json`), polled per-workspace by the TUI in
  `app/background.rs` — but cached **in memory only** (`pr_lifecycle`,
  `pr_number` maps), invisible to the short-lived menu process.
- `wsx waybar menu` (`src/waybar/menu.rs`) reads the sqlite store, pipes lines
  to the menu command (`WSX_WAYBAR_MENU` override honored), and jumps via
  `waybar::jump`.

## Decisions

| Question | Decision |
|---|---|
| Row content | Branch, PR state + number, agent status message, dirty/diff indicator |
| Freshness | Hybrid: instant from sqlite cache + per-entry `async` live refresh |
| Actions | Jump only (Enter) |
| Install/fallback | `wsx setup waybar` installs the Lua menu; `wsx waybar menu` auto-detects, falls back to the dmenu pipe; `WSX_WAYBAR_MENU` always wins |

## Components

### 1. `scm_cache` table (new migration, `store.rs`)

```
scm_cache(
  workspace_id INTEGER PRIMARY KEY REFERENCES workspaces(id),
  pr_lifecycle TEXT,      -- NULL = never fetched / unknown
  pr_number    INTEGER,
  dirty        INTEGER,   -- NULL = unknown, 0/1 otherwise
  additions    INTEGER,
  deletions    INTEGER,
  fetched_at   INTEGER    -- unix seconds of last successful gh fetch
)
```

- Nullable columns distinguish "unknown" from "known none"; unknown renders as
  no indicator, never as "no PR".
- Row deleted in the existing manual cascade in `remove_workspace`.
- Migration written idempotently — wsx migration blocks re-run on every
  startup (see `wsx-migrations-rerun-every-startup`), so guards must be
  content-based, not version-based.
- Store methods: `upsert_scm_cache`, read-all keyed by workspace id.

### 2. TUI write-through (`app/background.rs`)

Where the existing PR poll updates the in-memory maps, additionally upsert
`pr_lifecycle`/`pr_number`/`fetched_at` into `scm_cache`. The TUI does not
write git-local fields. This keeps the cache warm so most menu opens make no
`gh` calls at all.

### 3. `wsx waybar menu-entries --json` (new subcommand)

Reads store + cache only (no git, no gh — milliseconds). Prints a JSON array
of display-ready entries:

```json
[{ "text": "...", "subtext": "...", "icon": "...",
   "async": "<abs-wsx> waybar entry-refresh 'repo' 'slug'",
   "action": "<abs-wsx> waybar jump 'repo' 'slug'" }]
```

- All composition in Rust (unit-testable); Lua does zero formatting.
- Command strings shell-quoted with `shlex` (repo names may contain spaces).
- Absolute wsx binary path (`std::env::current_exe`) baked in: elephant runs
  as a systemd user service whose PATH may not include `~/.local/bin`.
- Sorted repo name, then workspace name (parity with today).
- Zero workspaces → `[]`.

### 4. `wsx waybar entry-refresh <repo> <slug>` (new subcommand)

The `async` target for one row. Steps:

1. Recompute git-local facts via existing helpers (`git::workspace_status`
   for dirty, `git::workspace_diff_stats` vs resolved base).
2. If `fetched_at` is older than the throttle window (120 s, a module
   constant), call
   `forge::fetch_pr_status`; on success update PR fields + `fetched_at`; on
   failure leave cached PR fields untouched (same don't-clobber rule as the
   TUI poll).
3. Upsert the cache.
4. Print the recomposed text line.

**Contract: always exit 0 with non-empty stdout.** Any failure (unknown
workspace, git error, gh missing) prints the best line composable from cache —
minimum `repo/slug`. Non-zero exit or empty output would delete/blank the row.

### 5. Lua shim `~/.config/elephant/menus/wsx.lua`

~30 lines, embedded as an asset (alongside `src/waybar/assets/`), installed and
overwritten by `wsx setup waybar`:

```lua
Name = "wsx"
NamePretty = "wsx Workspaces"
-- GetEntries(): io.popen("<abs-wsx> waybar menu-entries --json")
--   -> jsonDecode -> map fields 1:1 (Text, Subtext, Icon, Async, Actions.activate)
--   -> {} on popen/decode failure
```

No `Cache = true` — regenerate on every menu open. The absolute wsx path is
substituted at install time.

### 6. Launcher logic (`waybar/menu.rs::run_menu`)

Priority order:

1. `WSX_WAYBAR_MENU` set → current dmenu pipe with that command (unchanged).
2. `~/.config/elephant/menus/wsx.lua` exists **and** `walker` on PATH →
   spawn `walker -m menus:wsx`; selection/jump handled by elephant's action,
   nothing to parse from stdout.
3. Otherwise → current `walker --dmenu` pipe.

Waybar `on-click` stays `wsx waybar menu`. Detection failures degrade
silently.

## Row format

`async` can only replace the text line, so live data lives there; subtext and
icon are cached-at-open.

- **Icon**: agent-status glyph matching the waybar module (`↻` working,
  `…` waiting, `!` blocked, `✓` done, `·` none). Elephant renders non-ASCII
  icon strings as text glyphs.
- **Text (live)**: `repo/slug` + PR segment + dirty/diff, e.g.
  `workspacex/fix-bug   #123 · ●  +45 −12`
  - PR segment by lifecycle: open ` #N`; draft ` #N draft`;
    conflicted ` #N conflict`; merged ` #N`; closed ` #N closed`;
    NoPr/unknown → nothing. (Nerd-font octicons; exact glyphs tunable.)
  - `●` when uncommitted changes; `+N −N` when diff stats vs base nonzero.
- **Subtext (static)**: ` branch — state: message` via existing
  `sanitize()`; just the branch when no reported status.

The async `value`-clobbering is harmless: action strings embed repo/slug
literally and never use `%VALUE%`.

## Error handling

- `entry-refresh` traps everything, exits 0, non-empty output (see §4).
- Unknown vs none distinct end to end (NULL cache → no indicator).
- Lua shim returns `{}` on any popen/jsonDecode failure rather than erroring
  the elephant service.
- Zero workspaces: walker shows its "No Results" placeholder (dmenu path keeps
  its notify-send).
- All command strings shlex-quoted.
- Jump failures already notify via `notify-send` in the existing path.

## Testing

- **Unit (pure, no subprocesses):** text/subtext composer across
  lifecycle × dirty × stats; glyph mapping; action-string quoting incl. spacey
  repo names; JSON serialization shape; refresh-throttle decision with
  injected timestamps; launcher detection (env override / lua present /
  neither) with injected paths; `scm_cache` upsert/read/delete-cascade via
  in-memory store.
- **Not covered on purpose:** live `gh` calls — `parse_gh_pr_status` and
  degrade paths already tested in `forge.rs`.
- **Manual checklist** appended to `docs/manual-tests/waybar.md`: fresh
  install, instant open + visible live row update, Enter jumps, fallback with
  walker absent, `WSX_WAYBAR_MENU` override.
- CI gates: `cargo fmt --check`, clippy, tests.

## Out of scope

- Extra actions (open PR in browser, terminal at worktree, archive).
- Preview pane content.
- Per-row theming/colors (walker themes could style `item-box` later).
- Persisting the TUI's other in-memory state.
