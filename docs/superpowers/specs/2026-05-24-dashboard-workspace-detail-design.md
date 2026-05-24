# Dashboard workspace detail bar

When the user selects a workspace on the dashboard, the bottom of the
screen sheds the keybind footer down by one band and reveals a detail
bar: per-workspace context, a derived chat summary, and an inline reply
input. The bar gives the user enough information to decide whether to
attach, reply, ignore, or move on — without leaving the dashboard.

Tracks issue
[bakedbean/workspacex#91](https://github.com/bakedbean/workspacex/issues/91).

## Goals

- A detail panel pinned to the bottom of the dashboard, above the
  keybind footer, that surfaces per-workspace context for the currently
  selected workspace.
- Targets ~22% of the dashboard's vertical space (clamped to 8–14 rows)
  whenever a workspace is selected; otherwise collapses entirely and
  the workspace list reclaims the space.
- Coexists with the Project Manager pane by stacking vertically: list /
  detail / pm / footer.
- Chat-session summary derived purely from the JSONL we already tail —
  no per-workspace LLM calls.
- Inline reply input that sends typed text to the selected workspace's
  PTY without entering the attached view.
- The workspace list above the bar remains independently scrollable
  via the existing `List` selection model (selection auto-scrolls to
  stay visible).

## Non-goals

- A new always-on-screen panel. The bar is selection-driven and
  workspace-only; it does not render for repo headers or when the
  workspace list is empty.
- A per-workspace summarizer agent. All summary content derives from
  existing JSONL state.
- Multi-line / mid-string editing in the reply input. v1 supports
  append-only typing, backspace, Enter to send, Esc to cancel.
- Reply-draft persistence across selection changes or wsx restarts.
  Drafts are tied to the focused-at-the-time workspace and are
  discarded the moment selection moves elsewhere.
- Mouse activation of the reply input (Tab is the only entry point).
- Mid-string editing in the reply input. Left/right arrow keys,
  Home/End, Ctrl-W (delete word), and similar editor chords are
  swallowed in v1.
- Pinned-command chips in the bar. Pinned commands stay on the
  attached view only.
- PR title and PR number rendering. The detail bar takes optional
  `pr_title` / `pr_number` inputs and skips the PR row when either
  is missing; v1 wires both to `None`. Fetching them is a follow-up
  spec on the existing `forge.rs` PR poller.

## Architecture overview

Four touch points — one new file, three localized extensions:

1. **`src/events.rs`** — extend `WorkspaceEvents` with three derived
   fields populated by the existing tail-loop parsing path:
   `first_user_text`, `tool_use_counts`, `recent_edited_files`. All
   three are cleared by `reset_session_state`.
2. **`src/ui/dashboard/detail.rs` (new)** — owns rendering of the bar.
   Pure function over input data, no I/O. Peer of `layout.rs` and
   `row.rs`.
3. **`src/ui/mod.rs`** — extend `PaneFocus` with a `DetailBarReply`
   variant.
4. **`src/app.rs`** — three sub-changes:
   - `DashboardState` gains `pub reply_draft: String`.
   - `draw()` carves the dashboard area into list / detail / pm
     regions based on `(pm_visible, selection_is_workspace)`.
   - `handle_key_dashboard` handles Tab into the new focus and routes
     keystrokes to the draft while focused there.

PR title/number plumbing is **deferred** — the renderer accepts
`pr_title` / `pr_number` as optional inputs and skips the PR row when
either is `None`. v1 wires them to `None` because the existing
`forge.rs` PR poller only captures `BranchLifecycle` state, not the
PR's title or number. A follow-up spec will extend the poller; that
work is explicitly out of scope here so this spec stays single-PR-sized.

No new modules outside `ui/dashboard/`, no new threads, no new
background work. The summary is recomputed at draw time from
already-parsed JSONL state.

## Data model

### `WorkspaceEvents` extensions (`src/events.rs`)

```rust
pub struct WorkspaceEvents {
    // ...existing fields...

    /// First plain-text user content block observed since the most
    /// recent session reset. Set once per session; preserved across
    /// log rotation past MAX_LOG.
    pub first_user_text: Option<String>,

    /// Running tallies of tool_use blocks by category. Categorization
    /// is by tool name (see `ToolUseCounts::increment`).
    pub tool_use_counts: ToolUseCounts,

    /// Most-recent-first ring of edited file paths, bounded to 7.
    /// Populated from Read/Edit/Write tool_use blocks. Consecutive
    /// same-path entries deduplicate.
    pub recent_edited_files: VecDeque<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ToolUseCounts {
    pub read: u32,
    pub edit: u32,
    pub write: u32,
    pub bash: u32,
    pub other: u32,
}
```

`reset_session_state` clears all three. Categorization:

- `Read` → `read`
- `Edit`, `MultiEdit` → `edit`
- `Write`, `NotebookEdit` → `write`
- `Bash` → `bash`
- everything else (including `Task`, `Glob`, `Grep`, `WebFetch`, …) → `other`

Path extraction for `recent_edited_files`: when a tool_use is one of
Read/Edit/MultiEdit/Write/NotebookEdit, parse the `file_path` field
from its input JSON and push-front the path, dropping duplicates and
trimming to a max length of 7.

## Detail bar contents

The bar is a fixed-position widget. Internal layout, top to bottom:

| rows | content |
|---|---|
| 1 | Header strip |
| 1 | `─` rule (dim, full width) |
| flex | 3-column body |
| 1 | `─` rule (dim, full width) |
| 1 | Reply input row |

### Header strip

Single line, left to right:

```
▍ <name>  ⎇ <branch>  <lifecycle-glyph> <pr-state>  +X −Y  ● Np procs  <status-glyph> <status-label> · <ago>
```

- `▍` in the workspace's status color.
- `<name>` bold; `<branch>` faint.
- Lifecycle glyph / state visible only when `pr_lifecycle` is `Some(_)`.
- Diff `+X −Y` visible only when `workspace_diff` is `Some(_)` and at
  least one of added/removed is nonzero.
- Procs `● Np` faint when zero, status-color when nonzero.
- `<ago>` mirrors the dashboard row's `format_ago`.

### Body — 3 columns

Column proportions: SESSION SUMMARY 30% · RECENT CHAT 40% · PROCESSES
+ RECENT FILES 30%. Each column has a small uppercase label header in
muted color.

**SESSION SUMMARY:** lines prefixed with `▸` in status color.

1. `▸ "<first_user_text>"` — italicized initial prompt, truncated to
   column width minus prefix.
2. `▸ <tool-trace>` — single line synthesized from `tool_use_counts`,
   e.g. *"read 14 files, edited 3 files, wrote 1 file, ran 2 commands"*.
   Fragments render in fixed order (read, edit, write, bash, other),
   pluralize on count, and separate with commas. Zero-count categories
   are omitted. `other` fragment renders as *"+N other actions"* when
   nonzero. Empty `tool_use_counts` shows a faint `—`.
3. `▸ <where-we-are-now>` — picks the strongest signal in this order:
   pending-question tool text → pending-permission tool name → first
   line of `last_assistant_text` → faint `—`.
4. `▸ #<pr_number> <pr_state> · <pr_title>` — only when both PR number
   and title are available.
5. `▸ <worktree-path> · created <ago>` — path is faint and
   left-truncated to keep the basename visible; created-ago uses
   `format_ago` on `workspace.created_at`.

**RECENT CHAT:** last ~6 wrapped lines of recent assistant text.
Drawn from `last_assistant_text` (most recent message) plus the
preceding assistant text if log space allows. Wraps inside column
width; no ANSI / styling beyond a faint color for everything. Empty
state shows a single faint `—`.

**PROCESSES + RECENT FILES:** stacked, separated by a faint
`RECENT FILES` sub-label.

- Top: up to 5 procs, each `● <cmd> <ago>` with cmd left-truncated to
  fit. If `workspace_processes.len() > 5`, last line shows `+N more`.
- Bottom: up to 5 entries from `recent_edited_files`, paths
  left-truncated so the basename stays readable.

### Reply input row

```
┃ Reply to agent  ┃ <draft>                                  ↵ send · Esc cancel
```

- Left chip `┃ Reply to agent ┃` (18 cells, dim/border style).
- Input field draws `reply_draft` text. When `focus ==
  PaneFocus::DetailBarReply`, `f.set_cursor_position` is called at the
  byte-after-draft cell so the user gets a terminal cursor.
- Drafts longer than the field width scroll horizontally so the last
  ~field-width-minus-1 characters and the cursor stay visible.
- Right side: `↵ send · Esc cancel` keybind hints in dim, only shown
  when focused; hidden otherwise.

## Layout integration

Replaces the current `if app.pm_visible` block in `app.rs::draw` with
a small helper that produces up to three regions:

```rust
fn dashboard_regions(
    area: Rect,
    pm_visible: bool,
    detail_visible: bool,
) -> (Rect, Option<Rect>, Option<Rect>) // (list, detail, pm)
```

Cases:

| pm_visible | detail_visible | layout (top → bottom)                          |
|------------|----------------|------------------------------------------------|
| false      | false          | list (full area)                               |
| false      | true           | list (`Min(0)`) / detail (`Length(detail_h)`)  |
| true       | false          | list (60%) / pm (40%)  *(today's behavior)*    |
| true       | true           | list (`Min(0)`) / detail (`Length(detail_h)`) / pm (`Length(pm_h)`) |

Where `detail_h = detail::preferred_height(area.height)` returns
`(area.height * 22 / 100).clamp(MIN_HEIGHT, 14)` with
`MIN_HEIGHT = 8`. `pm_h` when stacking with detail: `(area.height * 33
/ 100).max(6)` so PM always has its title row + ≥4 PTY rows.

`detail_visible` is `matches!(app.selected_target(), Some(SelectionTarget::Workspace(_)))`.

If `area.height < MIN_HEIGHT + 10` (list needs at least 10 rows), the
bar is suppressed regardless of selection and a one-line condensed
banner — `▍ <name> · <status-label> · <ago>` — overlays the bottom of
the list area instead. This keeps short terminals usable.

If `area.width < 80`, the body collapses to a single column (SESSION
SUMMARY only). Header strip and reply row stay; RECENT CHAT and
PROCESSES + RECENT FILES are dropped.

## Focus model & input handling

`PaneFocus` gains a third variant:

```rust
pub enum PaneFocus { Dashboard, ProjectManager, DetailBarReply }
```

Tab cycle when a workspace is selected:

- PM hidden: `Dashboard → DetailBarReply → Dashboard`
- PM visible: `Dashboard → DetailBarReply → ProjectManager → Dashboard`

Tab cycle when a repo header (or nothing) is selected: identical to
today (`Dashboard → ProjectManager` if PM visible, no-op otherwise).
DetailBarReply is never entered without a workspace selection.

While `focus == DetailBarReply`:

| key | behavior |
|---|---|
| `Tab` | focus → Dashboard, draft preserved |
| `Esc` | focus → Dashboard, draft cleared |
| `Enter` | `session.writer.send(draft.into_bytes() + b"\r")`, draft cleared, focus → Dashboard |
| `Backspace` | pop last char from `reply_draft` |
| `Char(c)` (no modifier or Shift only) | append `c` to `reply_draft` |
| `Char(c)` with Ctrl/Alt | swallowed in v1 |
| arrows, Home/End, F-keys, etc. | swallowed in v1 |

The PTY send reuses the existing `session.writer.send` channel used
by paste handling (`app.rs:1370`) and pinned-command dispatch
(`app.rs:1399`). Text is sent raw (no bracketed-paste wrapping) so
claude treats it as typed input and submits it on the trailing `\r`.

Selection changes while focused (arrow keys, filter input,
external-change refresh that drops the workspace) auto-return focus
to Dashboard and discard the draft. The draft is tied to a specific
workspace; preserving it across selection would risk sending the
wrong message to the wrong agent.

If the focused workspace is archived from elsewhere mid-edit (rare),
the next draw observes `selected_target()` is no longer a workspace
and the focus auto-returns to Dashboard.

## Edge cases

- **Events not yet scanned for the selected workspace** (id missing
  from `workspace_events_scanned`): header strip renders normally;
  SESSION SUMMARY and RECENT CHAT columns show a faint `loading…`
  placeholder. PROCESSES and RECENT FILES render from their
  independent state sources.
- **PR lifecycle present but no title fetched yet:** PR row omitted
  from SESSION SUMMARY. Header strip still shows the lifecycle glyph
  + state.
- **No diff data:** header skips the `+X −Y` slot entirely. No
  `+0 −0` placeholder.
- **Empty procs and empty recent files:** column shows `—` faint for
  each section.
- **Reply draft longer than field width:** field scrolls horizontally
  so the cursor + last ~(field_width - 1) chars stay visible.
- **By-attention view selection:** identical rendering. The bar reads
  per-workspace state, not per-group state.
- **Mouse scroll while reply is focused:** still scrolls the
  workspace's PTY scrollback (existing `scroll_active` behavior). The
  reply draft is unaffected. Mouse activation of the input is not
  supported in v1.

## Public surface of `detail.rs`

```rust
pub const MIN_HEIGHT: u16 = 8;

pub fn preferred_height(total_height: u16) -> u16;

pub struct DetailInputs<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<String>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub reply_draft: &'a str,
    pub reply_focused: bool,
    pub events_scanned: bool,
}

pub fn render(f: &mut Frame, area: Rect, inputs: &DetailInputs, theme: &Theme);
```

`pr_title` / `pr_number` are wired to `None` in v1 — see Non-goals.
The fields are plumbed in now so a future `forge.rs` extension can
fill them without touching the renderer.

## Testing

### `src/events.rs` (extend existing test module)

- `first_user_text` set on first user content block.
- `first_user_text` preserved when the 50-entry log rotates past it.
- `first_user_text` cleared by `reset_session_state`.
- `tool_use_counts` increments correctly for each tool name; unknown
  tools count as `other`.
- `recent_edited_files` push-front behavior, bound at 7, deduplicates
  consecutive same-path entries.
- All three fields cleared by `reset_session_state`.

### `src/ui/dashboard/detail.rs` (new test module)

- Renders header strip with all chips in order.
- SESSION SUMMARY emits the four lines (initial prompt, tool trace,
  where-we-are, worktree+age) and omits the PR row when
  `pr_lifecycle` is `None`.
- RECENT CHAT renders `—` faint when no assistant text exists.
- PROCESSES section shows `+N more` when `procs.len() > 5`.
- RECENT FILES section truncates paths from the left (basename
  retained).
- Reply input renders cursor position spec when `reply_focused =
  true`, hides cursor + hint when false.
- Responsive: `area.width < 80` collapses body to single column.
- `preferred_height` clamps correctly for `total_height = 20`
  (returns 8) and `total_height = 100` (returns 14).
- `loading…` placeholder when `events_scanned = false`.

### `src/ui/dashboard/tests.rs` (extend existing module)

- Frame snapshot with a workspace selected — bar appears above the
  footer; list area shrinks accordingly.
- Frame snapshot with a repo header selected — bar is absent; list
  area fills.
- Frame snapshot with PM visible + workspace selected — three-region
  stack (list / detail / pm) renders in order.

### `src/app.rs` (extend or add focused tests)

- `Tab` on workspace selection moves focus to `DetailBarReply`.
- `Tab` cycles `Dashboard → DetailBarReply → Dashboard` when PM
  hidden.
- `Esc` while focused returns to Dashboard and clears `reply_draft`.
- `Enter` while focused dispatches bytes ending in `\r` to the
  session writer (mock).
- `Char(c)` while focused appends to `reply_draft` and does not fire
  dashboard hotkeys.
- Selection change while focused returns focus to Dashboard and
  clears `reply_draft`.

### Manual verification

A short walkthrough script in `docs/manual-tests/` matching the
existing files there:

1. Launch wsx with the test fixture.
2. Select a workspace; observe the bar.
3. Tab; observe the cursor in the reply input.
4. Type "ping"; Enter; observe the message appear in the attached
   view.
5. Move selection to a repo header; observe the bar collapses.
6. Toggle PM; with a workspace selected, observe list / detail / pm
   stack.

## Rollout

Single PR. No feature flag — the bar is a visible-on-selection
behavior with a clean inert state (selection on repo header) that
matches today's UX. No data migration: the new `WorkspaceEvents`
fields are populated lazily from the existing tail loop and survive a
fresh `wsx` start the moment the JSONL is re-scanned.
