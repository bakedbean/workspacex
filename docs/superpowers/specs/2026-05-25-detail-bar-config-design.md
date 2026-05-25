# Configurable workspace detail bar

The workspace detail bar (introduced in
[2026-05-24-dashboard-workspace-detail-design.md](2026-05-24-dashboard-workspace-detail-design.md))
is currently hard-coded: visible whenever a workspace is selected, 30%
of the dashboard height (clamped 8–18 rows), three columns in fixed
proportions. This spec makes its display configurable at both the
global and per-repo level so users and teams can tune it to their
workflow.

## Goals

- Let users turn the detail bar off entirely.
- Let users tune the bar's height (percent + min/max row clamps).
- Let users independently toggle the three body columns (SESSION
  SUMMARY, RECENT CHAT, PROCESSES + RECENT FILES).
- Provide both a global default and a per-repo override that wins
  per-field (matching the existing `branch_prefix` resolution pattern
  in `src/repo.rs:24-29`).
- Surface the config through both the existing CLI
  (`wsx config edit <key>`) and the existing repo-settings modal.

## Non-goals

- Per-field configurability for the header strip (workspace name,
  branch, PR state, diff, procs count, status, ago). Header stays
  fixed; the user explicitly opted out of header-strip toggles.
- Toggling the reply input row. Reply is the dashboard's primary call
  to action; if the user hides the whole bar (`visible = false`)
  they've already opted out of replying from the dashboard.
- Reordering columns. Display order stays SESSION SUMMARY → RECENT
  CHAT → PROCESSES + RECENT FILES.
- Per-workspace overrides. Per-repo is the deepest scope.
- A new file-on-disk format (`~/.config/wsx/detail_bar.toml`,
  `<repo>/.wsx.toml`). All settings live in SQLite, matching the
  existing model.
- A live-preview editor inside wsx. The user edits JSON in `$EDITOR`;
  the result is reflected on the next draw.
- Configurable color/theming of the bar's chrome. Out of scope.

## Configurable knobs

| field | type | default | constraints |
|---|---|---|---|
| `visible` | bool | `true` | — |
| `height.percent` | u8 | `30` | clamped to `[5, 80]` on save and at resolve |
| `height.min_rows` | u16 | `8` | clamped to `[4, 40]` |
| `height.max_rows` | u16 | `18` | clamped to `[min_rows, 60]` |
| `sections.session_summary` | bool | `true` | — |
| `sections.recent_chat` | bool | `true` | — |
| `sections.procs_and_files` | bool | `true` | — |

Defaults match today's hard-coded behavior exactly. A user who never
touches the setting sees zero change.

## Resolution semantics

Per-repo overrides global on a **per-field** basis: each field of the
override is `Option<T>`, and `None` means "inherit from the global
value." Either falls back to the built-in default if unset. Matches
the existing `resolve_branch_prefix` pattern, generalized to a struct.

Example: global says `visible = true`, `height.percent = 30`, all
columns on. Repo override sets `sections.recent_chat = false` only.
Result for workspaces in this repo: visible, 30% height, summary + procs
columns only. Other repos are unaffected.

## Architecture overview

Four touch points — one new file, three localized extensions:

1. **`src/detail_bar_config.rs` (new)** — owns the `DetailBarConfig`
   struct, the `DetailBarOverride` struct, default values, JSON
   serialization, validation/clamping, and the `resolve(repo, store)`
   function. Pure data; no I/O beyond the SQLite reads delegated to
   `Store`.
2. **`src/store.rs`** — schema migration adding `detail_bar_config
   TEXT` to the `repos` table; `Repo` struct gains the field; new
   `set_repo_detail_bar_config(id, Option<&str>)` helper.
3. **`src/ui/dashboard/detail.rs`** — `preferred_height` moves from
   free function to method on `DetailBarConfig`; `DetailInputs` gains
   a `&DetailBarConfig` field; the body builder skips disabled
   columns and redistributes widths.
4. **`src/app.rs`** — `RepoSettingField` gains a `DetailBarConfig`
   variant; the modal's editor flow routes that variant to `$EDITOR`
   with JSON validation on save; `dashboard_regions` consults
   `cfg.visible`; `PaneFocus::DetailBarReply` becomes unreachable
   when `cfg.visible = false`.

No new modules outside `src/detail_bar_config.rs`. No new threads. No
new background work. The config is re-resolved on every draw — at
most two SQLite KV reads + two JSON parses per frame, consistent with
the existing per-frame `get_setting` calls (`src/app.rs:1313`, `1320`,
`1330`).

## Data model

### `src/detail_bar_config.rs`

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailBarConfig {
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub height: Height,
    #[serde(default)]
    pub sections: Sections,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Height {
    #[serde(default = "default_percent")]
    pub percent: u8,
    #[serde(default = "default_min_rows")]
    pub min_rows: u16,
    #[serde(default = "default_max_rows")]
    pub max_rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sections {
    #[serde(default = "default_true")]
    pub session_summary: bool,
    #[serde(default = "default_true")]
    pub recent_chat: bool,
    #[serde(default = "default_true")]
    pub procs_and_files: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetailBarOverride {
    pub visible: Option<bool>,
    pub height: Option<HeightOverride>,
    pub sections: Option<SectionsOverride>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeightOverride {
    pub percent: Option<u8>,
    pub min_rows: Option<u16>,
    pub max_rows: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SectionsOverride {
    pub session_summary: Option<bool>,
    pub recent_chat: Option<bool>,
    pub procs_and_files: Option<bool>,
}

impl Default for DetailBarConfig {
    fn default() -> Self { /* baked defaults from the table above */ }
}

impl DetailBarConfig {
    /// Apply an Override on top of self (repo wins per-field).
    pub fn with_override(self, ovr: &DetailBarOverride) -> Self;

    /// Clamp percent/min/max into legal ranges. Swap min/max if
    /// inverted. Called after parse and before returning from
    /// `resolve`.
    pub fn sanitize(&mut self);

    /// Number of always-on chrome rows (header + 2 rules + reply).
    pub const CHROME_ROWS: u16 = 4;

    /// True when at least one body column is enabled.
    pub fn has_body(&self) -> bool {
        self.sections.session_summary
            || self.sections.recent_chat
            || self.sections.procs_and_files
    }

    /// Compute the desired bar height for the current terminal.
    /// When no sections are enabled, the bar shrinks to its chrome.
    pub fn preferred_height(&self, total: u16) -> u16 {
        if !self.has_body() {
            return Self::CHROME_ROWS;
        }
        let target = (u32::from(total) * u32::from(self.height.percent) / 100) as u16;
        target.clamp(self.height.min_rows, self.height.max_rows)
    }
}

pub fn resolve(repo: &Repo, store: &Store) -> DetailBarConfig {
    let mut cfg = match store.get_setting("detail_bar_config") {
        Ok(Some(s)) => serde_json::from_str::<DetailBarConfig>(&s).unwrap_or_else(|e| {
            tracing::warn!(err = %e, "detail_bar_config global parse failed; using defaults");
            DetailBarConfig::default()
        }),
        _ => DetailBarConfig::default(),
    };
    if let Some(raw) = repo.detail_bar_config.as_deref() {
        match serde_json::from_str::<DetailBarOverride>(raw) {
            Ok(ovr) => cfg = cfg.with_override(&ovr),
            Err(e) => tracing::warn!(err = %e, repo = %repo.name,
                                     "detail_bar_config repo override parse failed; ignoring"),
        }
    }
    cfg.sanitize();
    cfg
}
```

`#[serde(default)]` on every field means partial JSON parses cleanly:
`{"visible": false}` becomes a fully-populated config with all other
fields at default. This is the forward-compat lever — future knobs
can be added without breaking saved blobs.

### `src/store.rs`

Migration in `Store::open`, matching the existing pattern at
`src/store.rs:106-184`:

```rust
let has_detail_bar = conn.query_row(
    "SELECT count(*) FROM pragma_table_info('repos')
     WHERE name = 'detail_bar_config'",
    [], |r| r.get::<_, i64>(0))? > 0;
if !has_detail_bar {
    conn.execute(
        "ALTER TABLE repos ADD COLUMN detail_bar_config TEXT",
        [],
    )?;
}
```

`Repo` struct gains:

```rust
pub detail_bar_config: Option<String>,
```

The `SELECT` and `INSERT` queries that touch `repos` (around
`src/store.rs:231` and `src/store.rs:212`) gain the new column.

New helper:

```rust
pub fn set_repo_detail_bar_config(
    &self,
    id: RepoId,
    value: Option<&str>,
) -> Result<()>;
```

## Edit surfaces

### CLI: `wsx config edit detail_bar_config`

The existing `CliAction::ConfigEdit { key }` flow in
`src/cli.rs:734-744` already covers this — it reads
`store.get_setting(key)`, opens `$EDITOR`, and writes back via
`set_setting`. Two enhancements:

1. **Seed empty buffer with pretty-printed defaults.** When `key ==
   "detail_bar_config"` *and* the stored value is empty, write the
   `serde_json::to_string_pretty(&DetailBarConfig::default())` into
   the editor buffer before launching `$EDITOR`. Other keys keep
   their existing empty-buffer behavior.
2. **Parse + clamp on save.** When `key == "detail_bar_config"`,
   parse with `serde_json::from_str::<DetailBarConfig>`. On error,
   print the error message, do not save. On success, call
   `sanitize()` and persist the re-serialized pretty JSON (so what
   the user sees on the next edit matches what's in effect).

The `wsx config get detail_bar_config` and `wsx config set
detail_bar_config <file>` paths get the same parse-validate treatment.

### Repo-settings modal

`RepoSettingField` in `src/app.rs:40-49` gains a 9th variant:

```rust
pub enum RepoSettingField {
    RepoName,
    BranchPrefix,
    BaseBranch,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
    PinnedCommands,
    RelatedRepos,
    DetailBarConfig,
}
```

Updates:

- `RepoSettingField::ALL` grows to 9 entries.
- `label()` returns `"detail_bar_config"`.
- The `rows: [(field, value); 8]` array in `src/ui/modal.rs:535`
  becomes `[…; 9]`; the new row reads from `repo.detail_bar_config`.
- The editor-launch arm (around `src/app.rs:572`) gets:

  ```rust
  RepoSettingField::DetailBarConfig =>
      (repo.detail_bar_config.clone().unwrap_or_default(), "json"),
  ```

  Empty-buffer seed for this variant: literal `"{}\n"` (an empty
  override means "inherit everything from global"). The full schema
  with all override fields lives in the docs, not the seed — keeping
  the buffer small encourages users to write minimal overrides.

- The save-handler arm (around `src/app.rs:2168`) parses the buffer
  with `serde_json::from_str::<DetailBarOverride>`. Parse failure →
  error modal, prior value preserved. Success → re-serialize via
  `to_string_pretty` and call `store.set_repo_detail_bar_config(id,
  Some(&json))`. Empty `{}` is still saved (so the row displays as
  set rather than `(unset)`).

- `[d] clear` semantics (existing modal keybind) calls
  `set_repo_detail_bar_config(id, None)`, restoring full global
  inheritance.

## Layout & rendering integration

### `src/ui/dashboard/detail.rs`

Three changes:

1. **`preferred_height` moves to `DetailBarConfig`.** The free
   function in `detail.rs` is removed; callers use
   `cfg.preferred_height(area.height)`. `MIN_HEIGHT` constant is
   removed in favor of `cfg.height.min_rows`.

2. **`DetailInputs<'a>` gains one field:**

   ```rust
   pub struct DetailInputs<'a> {
       // ...existing fields...
       pub config: &'a DetailBarConfig,
   }
   ```

3. **Body builder skips disabled columns and redistributes widths.**

   ```rust
   enum Column { SessionSummary, RecentChat, ProcsAndFiles }

   let enabled: Vec<Column> = [
       cfg.sections.session_summary.then_some(Column::SessionSummary),
       cfg.sections.recent_chat.then_some(Column::RecentChat),
       cfg.sections.procs_and_files.then_some(Column::ProcsAndFiles),
   ].into_iter().flatten().collect();
   ```

   Width distribution preserves the existing 30/40/30 ratios across
   whichever columns survive:

   | visible cols                  | constraints |
   |-------------------------------|-------------|
   | 3 (all)                       | 30 / 40 / 30 |
   | 2 (summary + chat)            | 43 / 57 |
   | 2 (summary + procs)           | 50 / 50 |
   | 2 (chat + procs)              | 57 / 43 |
   | 1                             | 100 |
   | 0                             | body region sized `Length(0)` |

   Header strip, top rule, bottom rule, and reply input row always
   render (when `visible = true`) — call these four 1-row pieces the
   bar's **chrome** (4 rows total). When zero columns are enabled,
   `preferred_height` returns just the chrome height (4); the body
   region is sized `Length(0)` and the bar is a tight 4 rows. When at
   least one column is enabled, `preferred_height` returns the
   user's configured percent-clamped target and the body fills
   `target - 4`.

### Narrow-terminal collapse (`area.width < 80`)

Today drops to SESSION SUMMARY only. With user toggles, the renderer
picks **the first enabled column in display order** (summary → chat
→ procs). If no columns are enabled, behaves identically to the
empty-body case above.

### `src/app.rs::dashboard_regions`

The `detail_visible` check changes from

```rust
let detail_visible = matches!(app.selected_target(), Some(SelectionTarget::Workspace(_)));
```

to

```rust
let detail_visible = cfg.visible
    && matches!(app.selected_target(), Some(SelectionTarget::Workspace(_)));
```

`cfg` is resolved once per `App::draw` (after the selected workspace
is known so the right repo's override applies) and threaded into
`dashboard_regions`, `DetailInputs`, and the focus-cycle logic below.

### Focus model when `cfg.visible = false`

The detail bar's reply input is unreachable when the bar isn't drawn:

- Tab cycle when a workspace is selected: PM hidden → no-op; PM
  visible → `Dashboard ↔ ProjectManager`, skipping `DetailBarReply`.
- If `focus == DetailBarReply` and the resolved config flips to
  `visible = false` (CLI edit mid-session, or selection moves to a
  workspace whose repo overrides off), next draw observes the
  invariant violation and calls `app.return_focus_to_dashboard()`,
  which discards the draft.
- The reply-input keybind hint in the footer is suppressed.

## Edge cases

- **Corrupt JSON in global blob:** `from_str` returns `Err`. Log
  `tracing::warn!` (matching `src/store.rs:621` style) and fall back
  to `DetailBarConfig::default()`. Dashboard remains renderable.
- **Corrupt JSON in per-repo override:** same — log + ignore the
  override (the global config still applies in full).
- **`min_rows > max_rows`:** `sanitize()` swaps them. Cheaper than
  refusing the save.
- **`percent` outside `[5, 80]`:** clamped by `sanitize()` on save
  and on resolve. Persisted value reflects what's in effect.
- **All sections disabled but `visible = true`:** bar still renders
  at a tight 4 rows (chrome only — header + 2 rules + reply).
  `preferred_height` short-circuits to `CHROME_ROWS` so the user
  doesn't get a tall bar with a blank middle. User opted in.
- **`visible = false` plus repo-override toggles set:** override
  fields for `height`/`sections` are still parsed and merged but
  have no observable effect while `visible = false`. Re-enabling
  `visible` later (globally or per-repo) brings the configured
  height/sections back as-is. No surprise resets.
- **Repo override sets `visible = true` but global is `false`:**
  repo wins. Bar renders for this repo only.
- **By-attention dashboard view:** the bar is selection-driven, so
  only the selected workspace's repo config applies at any moment.
  As selection moves between workspaces in different repos, the
  bar's height and visible columns change accordingly. This is
  intentional — per-repo settings follow the selection.
- **Mid-session CLI edit flips `visible` off while focus is on the
  reply input:** next draw observes `cfg.visible = false`, focus
  auto-returns to Dashboard, draft cleared. Standard defensive
  pattern.
- **Migration on existing databases:** the `ALTER TABLE` adds the
  column with `NULL` default, so existing repos see no override and
  inherit global defaults. Zero observable change.
- **Empty `{}` per-repo override:** parses as `DetailBarOverride::default()`
  (all fields `None`). Equivalent to inheriting global in full, but the
  row appears "set" in the modal so the user can find it again.

## Public surface of `src/detail_bar_config.rs`

```rust
pub struct DetailBarConfig { /* ... */ }
pub struct Height { /* ... */ }
pub struct Sections { /* ... */ }
pub struct DetailBarOverride { /* ... */ }
pub struct HeightOverride { /* ... */ }
pub struct SectionsOverride { /* ... */ }

impl Default for DetailBarConfig { /* baked defaults */ }
impl DetailBarConfig {
    pub const CHROME_ROWS: u16 = 4;
    pub fn with_override(self, ovr: &DetailBarOverride) -> Self;
    pub fn sanitize(&mut self);
    pub fn has_body(&self) -> bool;
    pub fn preferred_height(&self, total: u16) -> u16;
}

pub fn resolve(repo: &Repo, store: &Store) -> DetailBarConfig;
```

## Testing

### `src/detail_bar_config.rs` (new test module)

- `DetailBarConfig::default()` matches the documented defaults.
- `serde_json` round-trip of `DetailBarConfig::default()` is lossless.
- Parsing `{}` yields `DetailBarConfig::default()`.
- Parsing `{"visible": false}` fills other fields with defaults.
- Parsing `{"unknown_field": 123}` succeeds (forward-compat).
- `with_override` returns global field when override is `None`.
- `with_override` returns override field when `Some`.
- `with_override` merges nested `HeightOverride` per-field
  (override only `percent`; `min_rows`/`max_rows` keep global).
- `with_override` merges nested `SectionsOverride` per-field.
- `sanitize` clamps `percent` to `[5, 80]`.
- `sanitize` clamps `min_rows` to `[4, 40]` and `max_rows` to
  `[min_rows, 60]`.
- `sanitize` swaps `min_rows` and `max_rows` when inverted.
- `preferred_height` returns `min_rows` on a short terminal,
  `max_rows` on a tall one, percent-target in between.
- `preferred_height` after `sanitize` with originally inverted
  bounds returns a sensible (swapped) value.
- `preferred_height` returns `CHROME_ROWS` (4) when no sections
  are enabled, regardless of `total` and configured percent.
- `has_body` returns `true` when at least one section is enabled,
  `false` when all three are off.
- `resolve(repo, store)` returns defaults when neither global nor
  repo has a value.
- `resolve` returns global when only global is set.
- `resolve` returns global-with-override-applied when both are set.
- `resolve` logs and falls back when global JSON is malformed.
- `resolve` logs and ignores override when override JSON is
  malformed (global still applies).

### `src/ui/dashboard/detail.rs` (extend existing test module)

- Renders 3-col body when all sections enabled (back-compat with
  existing snapshot).
- Renders 2-col body with redistributed widths — one test per
  two-col combination.
- Renders 1-col body at 100% width — one test per single-col case.
- Body region collapses to 0 rows when all three sections disabled;
  chrome (header + 2 rules + reply) still renders, total height
  is exactly `DetailBarConfig::CHROME_ROWS`.
- Narrow-terminal collapse (`area.width < 80`) picks the first
  enabled column in display order. Test cases:
  - All enabled → renders SESSION SUMMARY.
  - Summary disabled, chat + procs enabled → renders RECENT CHAT.
  - Only procs enabled → renders PROCESSES + RECENT FILES.
- `preferred_height(DetailBarConfig::default(), total)` matches the
  existing test cases for `total ∈ {20, 50, 100, 0}` — back-compat
  contract.
- `preferred_height` with `height.percent = 50` returns `50%` of
  the area clamped by the default min/max.

### `src/store.rs` (extend existing test module)

- Fresh-DB schema has the `detail_bar_config` column.
- Migration on an old-schema DB (column absent) adds the column.
- `set_repo_detail_bar_config(id, Some("{}"))` followed by
  `repos()` returns `Some("{}")` for that repo.
- `set_repo_detail_bar_config(id, None)` followed by `repos()`
  returns `None`.

### `src/app.rs` (extend existing tests)

- Tab cycle when `cfg.visible = false` and PM hidden: no-op.
- Tab cycle when `cfg.visible = false` and PM visible: `Dashboard
  ↔ ProjectManager`, skipping `DetailBarReply`.
- Tab cycle when `cfg.visible = true` and PM hidden: `Dashboard ↔
  DetailBarReply` (existing behavior).
- Selection change to a workspace whose repo has `visible = false`
  overridden: focus on `DetailBarReply` is dropped, draft cleared.
- `RepoSettingField::DetailBarConfig` is in `RepoSettingField::ALL`
  and its `label()` returns `"detail_bar_config"`.

### `src/cli.rs` (extend existing tests)

- `ConfigEdit { key: "detail_bar_config" }` seeds a fresh editor
  buffer with the pretty-printed default config when the existing
  value is empty.
- `ConfigEdit { key: "detail_bar_config" }` with malformed JSON
  saved by the user: prior value preserved, error printed.
- `ConfigEdit { key: "detail_bar_config" }` with valid JSON
  containing out-of-range `percent`: saves the clamped value
  (verified by re-reading via `get_setting`).
- `ConfigGet { key: "detail_bar_config" }` returns the pretty
  JSON when set, or empty when unset.

### Manual verification

Add a walkthrough to `docs/manual-tests/` matching the existing
files there:

1. Launch wsx with the test fixture; select a workspace.
2. Run `wsx config edit detail_bar_config` — observe pretty-printed
   default JSON in `$EDITOR`.
3. Change `sections.recent_chat` to `false`, save. Observe the
   middle column disappears; summary/procs columns redistribute to
   50/50.
4. Open the repo settings modal (`R`), navigate to
   `detail_bar_config`, press Enter. Set `{"visible": false}`. Save.
   Observe the bar collapses entirely for this repo's workspaces;
   workspaces in other repos still show it.
5. Press `[d]` on the repo's `detail_bar_config` row → observe the
   bar returns for this repo (global config back in effect).
6. Edit `detail_bar_config` globally, set `height.percent: 50` —
   observe the bar takes ~half the dashboard.
7. Resize the terminal narrower than 80 cols — observe the first
   enabled column renders alone.
8. Set `{"sections": {"session_summary": false, "recent_chat":
   false, "procs_and_files": false}}` globally — observe the bar
   shrinks to header + reply only.

## Rollout

Single PR. No feature flag — the default config matches today's
hard-coded behavior exactly, so users who never edit the setting see
zero change. The schema migration is additive (a nullable column on
`repos`), with no data migration needed.

If the JSON shape needs to evolve later, the `#[serde(default)]`
attribute on each field means partial blobs from older versions
continue to parse; new fields silently take their default until the
user opts in.
