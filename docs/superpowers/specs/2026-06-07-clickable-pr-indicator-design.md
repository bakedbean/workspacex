# Clickable PR indicator in the workspace detail bar

**Date:** 2026-06-07

## Problem

When a workspace is associated with a GitHub pull request, the user wants
a visual indicator in the workspace detail bar and the ability to click it
to open the PR in the browser.

The detail bar header already renders a PR **lifecycle chip** (e.g.
`⏺ open`, `⏷ draft`, `⏺ merged`, `⏸ closed`, `⏺ conflict`), colored by
state, driven by the `pr_lifecycle` cache (`src/app.rs`) which is populated
in the background via `gh pr view` (`src/git/forge.rs`). Two things are
missing:

1. The chip isn't clickable — there is no browser-open code anywhere in the
   repo, and the chip's screen rect isn't tracked for hit-testing.
2. The fetched PR data is only *state* (`state`, `isDraft`, `mergeable`) —
   it has no PR number or URL, so there's nothing to display or open.

## Goal

Extend the existing lifecycle chip so it:

- shows the PR number (`⏺ #152 open`), and
- opens the PR in the browser when clicked, via `gh pr view <branch> --web`.

Scope is the **dashboard detail bar header** (not the attached-view footer).

## Non-goals

- No new standalone indicator element; we reuse and enhance the existing chip.
- No URL storage / `xdg-open` path — opening goes through `gh`, consistent
  with the existing `gh` usage and respecting the user's `gh` config.
- No change to attached-view rendering.

## Design

### 1. Data — fetch & store the PR number

`src/git/forge.rs`:

- Add `number` to the `gh pr view <branch> --json …` field list and to the
  `GhPrView` struct (`#[serde(default)] number: Option<u64>`).
- Introduce `pub struct PrStatus { pub lifecycle: BranchLifecycle, pub number: Option<u64> }`.
- Rename `fetch_branch_lifecycle` → `fetch_pr_status`, returning
  `Result<Option<PrStatus>>`. The existing parse logic produces a `PrStatus`.
  For the `NoPr` (stderr heuristic) path, `number` is `None`.
- `BranchLifecycle` is unchanged and remains the value used for styling.

`src/app.rs`:

- Add a parallel map `pr_number: HashMap<WorkspaceId, u64>` alongside
  `pr_lifecycle`. Parallel per-workspace maps are the established idiom
  (`pr_last_poll_ms`, `workspace_diff*`, etc.). Keeping `BranchLifecycle`
  as the stored lifecycle value means `row.rs` / `updates_bar.rs` / modal
  signatures stay untouched.

`src/app/background.rs`:

- The PR poll calls `fetch_pr_status`; on success it writes `pr_lifecycle`
  and (when `number` is `Some`) `pr_number` together.
- The workspace-removal path (~line 278) clears both maps.

### 2. Rendering — show the number + report the chip's rect

`src/ui/dashboard/detail.rs`:

- `build_header_strip` gains a `pr_number: Option<u64>` parameter and, when
  building the lifecycle chip, renders `#<n>` between the glyph and label:
  `format!("{glyph} #{n} {label}")` when a number is present, falling back
  to the current `{glyph} {label}` when it isn't.
- While assembling spans, `build_header_strip` accumulates a running
  char-offset (matching the existing chip-row width idiom of
  `chars().count()`) and, at the lifecycle chip, records
  `Option<HeaderChip { start: usize, width: usize }>`. It returns
  `(Line, Option<HeaderChip>)`.
- `detail::render` converts the `HeaderChip` into a screen `Rect`
  (`x = header_area.x + start`, `y = header_area.y`, `width`, `height = 1`),
  clamped to `header_area`, and includes it in `DetailDrawOutput` as
  `pr_link_rect: Option<Rect>`.

### 3. Hit-testing — store & read the rect

`src/app.rs`:

- Add `pr_link_rect: Option<(WorkspaceId, Rect)>`, cleared at the top of
  `draw` like the other rect fields.

`src/app/render.rs`:

- When the detail bar draws for the selected workspace, set
  `app.pr_link_rect = out.pr_link_rect.map(|r| (ws_id, r))`.

`src/app/input.rs`:

- Add a branch to the existing `MouseEventKind::Down(MouseButton::Left)`
  `else if` chain (next to `chip_rects` / `attention_rects` /
  `agent_chip_rects`) that hit-tests `pr_link_rect` and calls
  `open_pr_for_workspace(app, ws_id)`.

### 4. Open action

`src/git/forge.rs`:

- `pub fn open_pr_in_browser(worktree: &Path, branch: &str)` spawns
  `gh pr view <branch> --web` detached (fire-and-forget). Spawn failures are
  logged via `tracing::warn!`, consistent with the other click handlers.

`src/app/input.rs`:

- `open_pr_for_workspace(app, ws_id)` looks up the workspace's worktree path
  and branch by id and calls `open_pr_in_browser`.

## Edge cases

- **No PR (`NoPr`)**: no chip is drawn → no rect → not clickable. Unchanged.
- **Merged / closed PRs**: still show a number and remain clickable (opens
  the PR page), which is useful.
- **`gh` missing or offline**: the poll already degrades to leaving the
  cache alone (`Ok(None)`); a click simply warns on spawn failure.
- **Number missing but state known** (unexpected `gh` output): chip renders
  without `#<n>` and is still clickable.

## Testing

- Unit: `parse_gh_pr_status` extracts `number` across states (open, draft,
  merged, closed, conflicted) and tolerates a missing `number`.
- Unit: `build_header_strip` includes `#<n>` in the chip when a number is
  present, omits it when absent, and returns the correct chip offset/width.
- Existing `parse_gh_pr_view` / header-strip tests are adapted to the new
  function/parameter shapes (assert on `.lifecycle` where they previously
  compared the bare enum).
- The browser spawn is a thin fire-and-forget shell-out (not unit-tested);
  the hit-test → workspace-resolution seam is the testable boundary.

## Files touched

- `src/git/forge.rs` — `PrStatus`, `fetch_pr_status`, `number` field,
  `open_pr_in_browser`, tests.
- `src/app.rs` — `pr_number` map, `pr_link_rect` field + clear in `draw`.
- `src/app/background.rs` — write/clear both maps.
- `src/ui/dashboard/detail.rs` — `build_header_strip` number param + chip
  rect, `DetailDrawOutput.pr_link_rect`, tests.
- `src/app/render.rs` — populate `app.pr_link_rect`.
- `src/app/input.rs` — mouse hit-test branch + `open_pr_for_workspace`.
