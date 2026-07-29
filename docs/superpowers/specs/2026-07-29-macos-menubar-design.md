# wsx macOS menubar indicator — design

Date: 2026-07-29
Status: approved pending spec review

## Goal

A macOS menubar item, hosted by SwiftBar, that mirrors the Linux waybar
indicator: all registered wsx repos and their workspaces at a glance, with a
dropdown menu to jump to any workspace. Richer than the Linux menu in one
deliberate way: each workspace row carries a submenu of secondary actions
(open PR, copy path, reveal in Finder). Shipped as part of wsx, strictly
isolated as a macOS/SwiftBar add-on: no menubar code may bleed into core
modules.

## Non-goals

- No native NSStatusItem app or daemon owned by wsx; SwiftBar is the
  long-lived host, wsx remains a short-lived data provider (same split as
  waybar/walker on Linux).
- No support for xbar or sketchybar.
- No push/streaming updates; SwiftBar polls on a fixed interval.
- No fresh `gh` or git subprocess work on the render path (see Freshness).

## Architecture

Four pieces inside one macOS-only module tree, mirroring `src/waybar/`:

| Piece | Command | Role |
|---|---|---|
| Plugin renderer | `wsx menubar plugin` | Print the full SwiftBar document (header + menu); run by SwiftBar on every refresh |
| Refresh sweep | `wsx menubar refresh` | Detached, throttled recompute of git facts + PR status into `scm_cache` |
| Jump | `wsx menubar jump <repo> <slug>` | Select workspace in a running TUI + focus its window, or launch one |
| Installer | `wsx setup menubar` | Install the SwiftBar plugin shim |

The installed SwiftBar plugin is a two-line shell shim
(`wsx-menubar.10s.sh`) that execs `<abs wsx path> menubar plugin`. All logic
stays in Rust where it is unit-testable; the shim exists only because
SwiftBar discovers executables in a plugin folder and encodes the refresh
interval in the filename.

### Isolation (hard requirement)

- All menubar logic lives in a new top-level `src/menubar/` module, declared
  `#[cfg(target_os = "macos")] pub mod menubar;` beside `waybar` in
  `lib.rs`. Suggested files: `mod.rs`, `plugin.rs` (SwiftBar document),
  `jump.rs`, `refresh.rs`, `install.rs`.
- `cli.rs` parses the `menubar` group on all platforms (uniform help/usage)
  but dispatch on non-macOS returns:
  `wsx menubar is only available on macOS (SwiftBar integration)` —
  the mirror of the existing `waybar_linux_only()` arm.
- `src/menubar/` depends on core; core never depends on `src/menubar/`.

### Refactors of existing code (targeted, no behavior change on Linux)

1. **Un-gate the IPC module.** `src/waybar/ipc.rs` is pure-Unix (unix
   sockets, `$XDG_RUNTIME_DIR` with `~/.local/state/wsx/run` fallback).
   Move it to a shared location (e.g. `src/ipc.rs` or `src/tui_ipc.rs`),
   gated `#[cfg(unix)]`, and start the listener at TUI startup on both
   Linux and macOS (`main.rs` currently gates the spawn to Linux). The
   socket wire protocol (`select <repo…> <slug>\n`) is unchanged.
2. **Split row data from walker formatting.** `src/waybar/entries.rs`
   couples platform-neutral data collection (`collect_rows`: store +
   `scm_cache` + git facts) with walker-specific rendering
   (`compose_text` fixed-byte columns, `state` CSS classes, non-ASCII
   icon workaround). Extract the data side — the row struct and
   `collect_rows`, plus the `refresh-prs` sweep — into a shared
   platform-neutral module (e.g. `src/workspace_rows.rs`, `#[cfg(unix)]`
   or ungated); `waybar/entries.rs` keeps only walker formatting,
   `menubar/plugin.rs` adds SwiftBar formatting.
3. **`collect_rows` gains a cache-only mode.** Today it always computes
   git facts fresh (fine for click-time menus). Add a mode that reads
   `scm_cache` only, used by the menubar render path. The existing waybar
   menu keeps its fresh-at-open behavior.
4. **`scm_cache` freshness for git facts.** The table already stores
   `dirty`, `additions`, `deletions`; add a `git_fetched_at` column (the
   existing `fetched_at` tracks the PR fetch) so the git sweep can
   throttle independently of the PR sweep.

## `wsx menubar plugin`

Prints the complete SwiftBar document to stdout on every poll:

### Header (the menubar item itself)

- SF Symbol via `sfimage=` + workspace count across all repos, e.g.
  `4 | sfimage=arrow.triangle.branch`.
- Tinted by the same attention ranking as waybar
  (`blocked > done > waiting > working/busy > idle`) using `sfcolor=`:
  blocked red, done green, waiting yellow, working blue, idle default
  (no color param). Colors specified as hex with a light,dark pair where
  the palettes differ.
- No registered repos → print nothing meaningful (SwiftBar hides an item
  whose output is empty); any error → bare symbol with no count, exit 0.
  Never a visible error string in the menubar.

### Menu body

After the `---` separator, per repo (sorted by name), all registered repos
including empty ones:

- A disabled section-header line: the repo name (`| disabled=true`), and
  `(no workspaces) | disabled=true` beneath empty repos — same content as
  the waybar tooltip tree.
- One row per workspace (sorted by slug). Row text renders with
  `font=SFMono-Regular` (monospaced, so columns align without the walker
  fixed-byte-offset machinery) and shows, space-separated:
  - status glyph (same set as waybar: blocked `!`, done `✓`, waiting `…`,
    working/busy `↻`, none `·`),
  - `slug`,
  - PR indicator when cached: `#N` plus a lifecycle word for
    `draft`/`conflict`/`closed`/`merged`; open carries the bare `#N`.
    Plain text only — nerd-font glyphs are not in SF Mono, and SwiftBar's
    `sfimage` is per-line, not inline. `NoPr` and unknown render
    identically as nothing, matching the Linux rule,
  - dirty marker `●` when the worktree has uncommitted changes,
  - `+adds −dels` vs the resolved base branch when cached.
- Each row opens a **submenu** (indented `--` lines in SwiftBar syntax):
  1. `Jump` — `bash=<wsx> param1=menubar param2=jump param3=<repo>
     param4=<slug> terminal=false` (first item, the primary action).
  2. `Open PR #N in browser` — only present when a PR number is cached;
     opens the PR URL via `href=`. The PR URL is stored in `scm_cache`
     alongside the number (small additive column) so no `gh` call is
     needed at render time.
  3. `Copy worktree path` — pipes the path to `pbcopy`.
  4. `Reveal in Finder` — `open -R <worktree>`.
  A second `-- <branch> — <status message>` disabled subtitle line at the
  top of the submenu carries the info the Linux menu shows as subtext.
- Footer after a separator: a single `Refresh` item (SwiftBar's built-in
  `refresh=true` action). No other footer actions — YAGNI.

All user-controlled strings (repo, slug, branch, status message) are
sanitized: newlines/control chars collapsed, `|` escaped, length-capped —
one workspace can never inject extra menu lines or params.

### Freshness

The render path reads sqlite only (store + `scm_cache`); a 10s refresh
interval is therefore cheap regardless of workspace count. After printing,
the plugin spawns `wsx menubar refresh` detached (double-fork/`setsid`,
stdio null — the same fire-and-forget contract as `waybar refresh-prs`),
which:

- recomputes git facts (dirty, diff stats) for workspaces whose
  `git_fetched_at` is older than 60s, with the same bounded concurrency
  (`buffered(8)`) as the waybar path, writing through to `scm_cache`;
- runs the existing PR sweep logic (`gh pr view`) for rows whose PR
  `fetched_at` is older than the existing 120s throttle.

Sweep failures are silent and never clobber cached state. Net effect:
indicators lag at most ~70s behind reality; the menu itself opens
instantly. The TUI's background PR poll already write-throughs to
`scm_cache`, so a running TUI keeps the menubar fresher for free.

## Jump

`wsx menubar jump <repo> <slug>`:

1. **Running TUI**: identical to Linux — iterate live sockets
   (most-recently-modified first), send `select <repo> <slug>\n`; the
   listener locks the shared app and calls `App::open_workspace_by_name`
   (select + attach). Stale sockets unlinked on connect failure.
2. **Focus** (best-effort, silently skipped on any failure): the socket
   filename encodes the TUI pid. Walk the pid's ancestor chain via
   `ps -o ppid=`; for each ancestor ask `lsappinfo` for an owning app
   bundle; on a hit, activate it with `open -b <bundle-id>`. This is the
   macOS analogue of the `/proc` + `hyprctl` walk.
3. **No running TUI**: spawn a terminal running
   `<wsx> --select <repo>/<slug>`, resolved in order:
   1. `terminal_cmd` config key if set — reused with a `{cmd}` placeholder
      convention: if the template contains `{cmd}`, substitute the
      shell-quoted command; otherwise fall back to step 2 (a bare
      app-open command like `open -a iTerm` cannot carry a command, and
      guessing would misfire).
   2. iTerm2 installed (`open -b com.googlecode.iterm2` resolvable) →
      osascript: `create window with default profile command "<cmd>"`.
   3. Terminal.app → osascript `do script "<cmd>"` + `activate`.

Errors that prevent the jump entirely surface via
`osascript -e 'display notification … with title "wsx"'` plus stderr —
the `notify-send` analogue.

## `wsx setup menubar`

Follows the `wsx setup waybar` precedent:

1. Resolve the SwiftBar plugin directory: read the `PluginDirectory` key
   from SwiftBar's defaults domain (`defaults read com.ameba.SwiftBar
   PluginDirectory`); if unreadable, fall back to prompting with a
   paste-ready shim and instructions rather than guessing a location.
2. Write `wsx-menubar.10s.sh` into it atomically (write-temp + rename),
   `0755`, with the absolute wsx path baked in via the existing
   `preferred_wsx_bin` logic (prefers `~/.local/bin/wsx` over transient
   dev-build paths). Re-running overwrites the shim (refreshes the baked
   path) — idempotent.
3. SwiftBar not installed (defaults domain missing and app not found) →
   print `brew install swiftbar` hint and the manual steps; exit 0.
4. Ask SwiftBar to reload the plugin via its URL scheme
   (`open -g "swiftbar://refreshallplugins"`), best-effort.

No config keys are added; the only knobs are the shim filename (refresh
interval) and the installed file itself, matching the waybar precedent.

## Testing

- Unit (in `src/menubar/`, macOS-gated like the module): SwiftBar document
  rendering (header states, color mapping, row composition, submenu
  actions, param escaping/sanitization), freshness threshold logic,
  terminal-command template resolution (`{cmd}` present/absent), jump
  argument formatting.
- Shared-module tests move with the code: `collect_rows` cache-only mode,
  `scm_cache` `git_fetched_at` column migration + accessors, IPC parse
  tests now run on macOS too.
- Installer: plugin-directory resolution against fixture `defaults`
  output; shim content golden test.
- CI note: run `cargo fmt --check`, clippy, and tests; Linux builds must
  stay green after the entries/ipc refactor (CI matrix already covers
  `macos-latest` and Linux).
- Manual: `docs/manual-tests/menubar.md` mirroring the waybar doc —
  install on this machine, verify live item, state colors, menu rows,
  submenu actions, jump-to-running-TUI (with focus), jump-with-no-TUI
  (iTerm2 spawn), PR indicator after a sweep.

## Open items deliberately deferred

- No SwiftBar streaming mode or refresh-on-open; fixed 10s poll with
  ≤ ~70s staleness on git/PR indicators is accepted.
- No xbar/sketchybar packaging.
- No workspace-management actions (archive, create) in the menu.
- No focus support for terminals whose app can't be resolved via
  `lsappinfo` (selection still happens; focus is best-effort).
