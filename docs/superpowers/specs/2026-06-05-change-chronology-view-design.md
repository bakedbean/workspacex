# Change Chronology View — Design

**Date:** 2026-06-05
**Status:** Approved for planning

## Problem

Agentic coding erodes the developer's "muscle memory" of the codebase. When you
aren't typing the edits yourself, you lose the felt sense of where code lives,
what each file does, and *why* an implementation took the shape it did. Reading
diffs after the fact doesn't rebuild that memory — a diff shows *what* changed
but not the lived, ordered narrative of the agent working through the change.

wsx is uniquely positioned to help: it already observes agent sessions
read-only by tailing their JSONL logs, so it can surface a faithful, moment-by-
moment record of what the agent touched and when.

## Goal

A toggleable vertical bar, docked left or right inside the **attached** view,
showing a newest-first, time-ordered series of individual file changes the agent
made — **one entry per edit**, a literal chronology rather than a commit list.
Each entry shows the file, the change magnitude, and a one-line "what"; an entry
can be expanded to a short inline diff peek, and clicking it opens the file in
the user's editor **at the changed line**.

The bar is toggleable and left/right alignable, configurable both globally and
per repo.

## Non-goals

- Not a commit list and not derived from git history or the reflog.
- No new agent UI — wsx continues to delegate the agent TUI to the PTY; the bar
  is wsx chrome carved out of the attach layout.
- No new persistent storage of change events in this iteration. The on-disk
  session logs are the source of truth (Approach 1, below). An mtime-keyed
  on-disk cache is a possible later optimization, explicitly out of scope now.

## Decisions (from brainstorming)

| Decision | Choice |
| --- | --- |
| Entry fidelity | **B (medium)** default: `time · file · +adds/-dels` + one-line summary. **C (inline diff peek)** on expand. |
| Click behavior | Open in editor at **`file:line`**, reusing today's editor-open mechanism plus a line capability. |
| Default width | **Wider (~32% of attach width)**; **min width configurable**. |
| Side | Left/right, configurable; default **right**. Toggleable on/off. |
| Settings scope | **Global + per-repo**, mirroring `detail_bar_config`. |
| History scope | **Whole workspace history** — all sessions on the branch/worktree, rebuilt from on-disk logs. |
| Agent coverage | **All agents** (Claude, Codex, Pi, Hermes). |
| Grouping | **One entry per edit** (literal time series), newest on top. |

## Architecture (Approach 1: logs as source of truth)

```
session JSONL logs ──▶ ChangeEvent extraction ──▶ ChronologyTimeline ──▶ ChronologyBar
 (Claude/Codex/Pi/      (per-agent parsers,        (merge across all      (carved column
  Hermes, on disk)       mutating tools only)       sessions, cached)       in attach view)
```

- The **active** session's log is followed live by the tail loop that already
  runs in `src/activity/`.
- **Historical** sessions for the workspace are located and scanned once on
  attach, then cached by file `(size, mtime)` so re-attach and periodic refresh
  only re-read the active (growing) file.
- No SQLite table for events. Disabling the feature removes the bar with no
  residue.

## Components

### 1. `ChangeEvent` extraction (parser layer)

Extend the existing tool_use parsing in `src/activity/events.rs` (Claude) and
the `codex_events.rs` / `pi_events.rs` / `hermes_events.rs` variants to emit a
structured event for each **mutating** tool call. `Read` and other non-mutating
tools are excluded.

```rust
pub struct ChangeEvent {
    pub timestamp: u64,          // parsed from the JSONL line (existing ms-precision parser)
    pub tool: ChangeTool,        // Edit | MultiEdit | Write | NotebookEdit (+ per-agent equivalents)
    pub file_path: PathBuf,      // stored absolute; displayed workspace-relative
    pub summary: String,         // one-line "what" (B fidelity)
    pub detail: ChangeDetail,    // bounded old/new text or content snippet (C expand)
}

pub enum ChangeTool { Edit, MultiEdit, Write, NotebookEdit }

pub enum ChangeDetail {
    Edit { old: String, new: String },   // bounded slices for the peek
    Write { head: String },              // leading lines of new content
    None,                                // agent exposed no change text
}
```

**Summary heuristic** (`summary`):
- `Edit`/`MultiEdit`: the most salient changed line — prefer a line matching a
  declaration pattern (`fn`/`def`/`class`/`pub`/`struct`/`impl`/assignment),
  else the first non-blank changed line; trimmed and truncated to fit.
- `Write`: `"new file"` or the first declaration in the content.
- `NotebookEdit`: the cell's summary/first line.

**Per-agent mapping:** each non-Claude parser maps its own tool vocabulary
(e.g. Codex `apply_patch`) into the same `ChangeTool` / `ChangeDetail`. Agents
that do not expose the changed text degrade to **B-only** entries
(`ChangeDetail::None`, no C-expand).

This is additive to the existing parsers, which already extract `file_path` and
parse timestamps; the new work is retaining the change text and synthesizing the
summary.

### 2. `ChronologyTimeline` (`src/activity/chronology.rs`, new)

Responsibilities:
- **Locate** all session JSONL files for the workspace's worktree path, using
  the same encoded-cwd convention as `pty::session::has_prior_session`, across
  each agent's log directory.
- **Build** the merged timeline by parsing each file's `ChangeEvent`s and
  merging by `timestamp`, stable, newest-first.
- **Cache** per file by `(size, mtime)`; only re-read changed/active files. The
  active session file is normally the only one that grows between refreshes.
- Expose a view to the renderer that keeps all events but is rendered lazily by
  scroll offset (no hard cap on retained history; the renderer paints a window).

### 3. `ChronologyConfig` (`src/config/chronology.rs`, new)

Mirror `src/config/detail_bar_config.rs` exactly: a global JSON blob in the
`settings` table plus a per-repo JSON override on a new `repos.chronology_config`
TEXT column (added via the established `ALTER TABLE repos ADD COLUMN` migration
pattern in `src/data/store.rs`). Scalar fields merge per-field; the per-repo
override wins per-field.

```rust
pub struct ChronologyConfig {
    pub visible: bool,        // default true
    pub side: Side,           // Left | Right, default Right
    pub width: WidthSpec,
}
pub struct WidthSpec {
    pub percent: u8,          // default ~32, of attach area width
    pub min_cols: u16,        // user-configurable minimum (the requested knob)
    pub max_cols: u16,
}
pub enum Side { Left, Right }
```

Provide `Default`, `with_override`, and `sanitize` (clamp `percent`/`min_cols`/
`max_cols` into legal ranges; swap inverted min/max), matching the
`detail_bar_config` and `usage_window` precedents. Resolution happens at render
time so CLI and in-app toggles agree live. Resolved width = `percent` of attach
width, clamped to `[min_cols, max_cols]`.

### 4. `ChronologyBar` renderer (in `src/ui/attached.rs`)

When the chronology is visible, before computing pane rects:
1. Split the attach pane area horizontally: carve a `width`-column strip on the
   configured side; the remainder feeds the existing pane/split layout
   unchanged.
2. Draw a 1-column divider reusing the `src/ui/split.rs` divider style.
3. Paint the bar:
   - Header: `CHANGE CHRONOLOGY` with a side indicator.
   - Newest-first entries: line 1 `time · file · +adds/-dels`; line 2 the dim
     one-line summary (B). The focused/expanded entry additionally renders the
     short inline diff peek (C).
   - Long file paths middle-truncated; scrollable via an offset.

**Auto-hide:** if the attach area is too narrow to host `min_cols` plus a usable
agent width, the bar is skipped for that frame without breaking the agent pane.

Hit-testing returns per-entry clickable rects via the existing
`PanesDrawOutput` pattern (alongside `chip_rects` / `pane_rects`).

### 5. Editor `file:line` extension (`src/commands/external.rs`)

Add `{file}` and `{line}` placeholders to the editor command template
substitution (alongside today's `{path}`), and a new entry point:

```rust
pub fn open_in_editor_at(worktree: &Path, file: &Path, line: u32, configured: Option<&str>) -> Result<()>;
```

When the configured template contains no placeholders, fall back by detecting
common editors and formatting their goto syntax:
- `code` / VS Code: `code --goto {file}:{line}`
- `vim` / `nvim` / `vi`: `+{line} {file}`
- `emacs` / `emacsclient`: `+{line} {file}`
- otherwise: append `{file}` (line omitted).

Today's `e` (dashboard) and `Ctrl-x e` (attached) whole-worktree open are
unchanged; this is a new, separate call path used only by chronology entry
clicks.

**Line resolution** (computed lazily on click, not stored):
- `Edit`/`MultiEdit`: locate the first line of `old_string` in the file's
  *current* contents → 1-based line number.
- `Write` / new file: line 1.
- Not found / file missing: fall back to line 1; a spawn failure surfaces via
  the existing editor spawn-error path.

## Interaction

| Action | Binding |
| --- | --- |
| Toggle bar on/off | `Ctrl-x c` (attached leader; also via config) |
| Swap side (L/R) | `Ctrl-x C` |
| Scroll | mouse wheel over the bar; keys when the bar holds focus |
| Expand entry to C (diff peek) | click entry (or key) toggles inline peek |
| Open in editor at line | click an entry's file → `open_in_editor_at(file, line)` |

`Ctrl-x` is the existing attached-mode leader, which already accepts letter
follow-ups (`d`, `e`, `u`, `a`, `x`, arrows); `c`/`C` are free.

## CLI surface

Following the `usage_graph_window` and detail-bar precedents:
- `wsx config set chronology <json>` (global) and the per-repo equivalent.
- Discrete conveniences if they fit the existing config CLI shape:
  `chronology.visible`, `chronology.side`, `chronology.width.min_cols`.

Reads resolve live at render time so CLI changes and the in-app toggle stay in
sync.

## Error handling & edge cases

- Missing/unreadable log dir → empty timeline; bar shows an em-dash placeholder
  (like `recent_files`).
- Malformed JSONL lines are skipped (existing parser behavior).
- Non-Claude agents lacking change text → B-only entries (no expand).
- Deleted/renamed files → click attempts open at line 1; if the file is gone,
  the editor spawn error surfaces via the existing path.
- Terminal too narrow → bar auto-hides without disturbing the agent pane.
- Huge histories → bounded per-file cache + lazy windowed render. If cold scan
  is ever too slow, an mtime-keyed on-disk cache is a later add (out of scope).

## Testing

- `ChangeEvent` extraction per tool and per agent — table-driven over sample
  JSONL lines, mirroring the existing `events.rs` tests.
- `summary` heuristic across Edit/Write/NotebookEdit inputs.
- `ChronologyTimeline` merge ordering and cache invalidation on `(size, mtime)`
  change.
- `ChronologyConfig` default/override/sanitize round-trips, mirroring the
  `usage_window` tests.
- Editor `{file}`/`{line}` argv resolution including each fallback editor —
  extending the existing `resolve_argv` tests.
- Line-number resolution from `old_string` (found, not-found, new-file cases).
- Renderer width/auto-hide math as pure functions where feasible.

## Files touched

- `src/activity/events.rs`, `codex_events.rs`, `pi_events.rs`, `hermes_events.rs`
  — emit `ChangeEvent`s.
- `src/activity/chronology.rs` (new) — timeline build/merge/cache.
- `src/config/chronology.rs` (new) + `src/config/mod.rs` — config struct.
- `src/data/store.rs` — `repos.chronology_config` column + accessors.
- `src/ui/attached.rs` — carve column, render bar, hit-testing.
- `src/app/input.rs` — `Ctrl-x c` / `Ctrl-x C`, scroll, click → editor.
- `src/commands/external.rs` — `open_in_editor_at` + `{file}`/`{line}`.
- `src/cli.rs` — `chronology` config subcommands.
- `README.md` — document the feature, keybindings, and config.
