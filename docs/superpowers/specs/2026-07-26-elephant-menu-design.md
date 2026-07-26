# Elephant Lua Menu for the wsx Workspace Picker

Date: 2026-07-26
Status: approved for planning

## Goal

Upgrade the waybar workspace picker from a plain `walker --dmenu` text list to a
rich walker/elephant menu: two-line rows with an agent-status icon, git branch,
PR state + number, and dirty/diff indicators — git-local indicators computed
fresh at every menu open, PR state served from a cache kept warm by the TUI
poll and a detached sweep (in-place refresh while the menu is open is not
achievable; see Background). Keep the existing dmenu pipe as an automatic
fallback.

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
- Elephant's per-entry `async` field (replaces an entry's text line in place
  after display) is **TOML-only**: the Lua entry parser
  (`pkg/common/menucfg.go`) reads Text/Subtext/Icon/Value/Actions/Keywords/
  SubMenu/Preview/State — not Async. Lua-generated entries cannot
  self-refresh after display, and static TOML menus cannot enumerate a
  dynamic workspace list, so live in-place row updates are not achievable.
  Freshness is instead delivered at menu open (see §3/§4).
- Elephant's Lua runtime registers `jsonDecode`, which returns `nil, err` on
  invalid JSON.
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
| Freshness | Git-local facts computed fresh at every menu open (parallel, local); PR state from sqlite cache, kept warm by TUI write-through + a detached throttled sweep spawned at menu open |
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

Prints a JSON array of display-ready entries:

```json
[{ "text": "...", "subtext": "...", "icon": "...",
   "action": "<abs-wsx> waybar jump 'repo' 'slug'" }]
```

- Git-local facts (dirty, diff stats vs base) are computed here at menu open,
  concurrently across workspaces (~3 git subprocesses each, local disk;
  ~100–300 ms total). A workspace whose git commands fail renders without
  those indicators. Fresh git facts are also upserted into `scm_cache`.
- PR fields come from `scm_cache` only — never a `gh` call on the open path.
- After printing, spawns a detached `wsx waybar refresh-prs` sweep (§4) so PR
  data self-heals by the next open even without a running TUI.
- All composition in Rust (unit-testable); Lua does zero formatting.
- Command strings shell-quoted with `shlex` (repo names may contain spaces).
- Absolute wsx binary path (`std::env::current_exe`) baked in: elephant runs
  as a systemd user service whose PATH may not include `~/.local/bin`.
- Sorted repo name, then workspace name (parity with today).
- Zero workspaces → `[]`.

### 4. `wsx waybar refresh-prs` (new subcommand)

Detached PR-cache sweep, spawned fire-and-forget (stdio null) by
`menu-entries`. For every workspace whose `fetched_at` is older than the
throttle window (120 s, a module constant), call `forge::fetch_pr_status`;
on success update PR fields + `fetched_at`; on failure leave cached PR fields
untouched (same don't-clobber rule as the TUI poll). Workspaces inside the
window are skipped, so back-to-back menu opens cost no `gh` calls. Failures
are silent — the sweep improves the cache or does nothing. Known trade-off:
a PR state change lands on the *next* menu open, not while the menu is
already showing.

### 5. Lua shim `~/.config/elephant/menus/wsx.lua`

~30 lines, embedded as an asset (alongside `src/waybar/assets/`), installed and
overwritten by `wsx setup waybar`:

```lua
Name = "wsx"
NamePretty = "wsx Workspaces"
-- GetEntries(): io.popen("<abs-wsx> waybar menu-entries --json")
--   -> jsonDecode -> map fields 1:1 (Text, Subtext, Icon, Actions.activate)
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

All fields are computed at menu open: git-local indicators freshly, PR
indicators from cache.

- **Icon**: agent-status glyph matching the waybar module (`↻` working,
  `…` waiting, `!` blocked, `✓` done, `·` none). Elephant renders non-ASCII
  icon strings as text glyphs.
- **Text**: `repo/slug` + PR segment + dirty/diff, e.g.
  `workspacex/fix-bug   #123 · ●  +45 −12`
  - PR segment by lifecycle: open ` #N`; draft ` #N draft`;
    conflicted ` #N conflict`; merged ` #N`; closed ` #N closed`;
    NoPr/unknown → nothing. (Nerd-font octicons; exact glyphs tunable.)
  - `●` when uncommitted changes; `+N −N` when diff stats vs base nonzero.
- **Subtext**: ` branch — state: message` via existing
  `sanitize()`; just the branch when no reported status.

Action strings embed repo/slug literally and never use `%VALUE%`.

## Error handling

- `menu-entries` never fails a row: per-workspace git errors degrade to
  cached/absent indicators; `refresh-prs` failures are silent (§3, §4).
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
  install, instant open with fresh dirty/diff indicators, PR indicator
  appearing by the next open after the sweep, Enter jumps, fallback with
  walker absent, `WSX_WAYBAR_MENU` override.
- CI gates: `cargo fmt --check`, clippy, tests.

## Out of scope

- Extra actions (open PR in browser, terminal at worktree, archive).
- Preview pane content.
- Per-row theming/colors (walker themes could style `item-box` later).
- Persisting the TUI's other in-memory state.
