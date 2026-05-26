# Configurable detail bar via modules and containers

Tracks [issue #98](https://github.com/bakedbean/workspacex/issues/98). Builds
on the bar introduced in
[2026-05-24-dashboard-workspace-detail-design.md](2026-05-24-dashboard-workspace-detail-design.md)
and replaces the per-section toggle scheme from
[2026-05-25-detail-bar-config-design.md](2026-05-25-detail-bar-config-design.md).

Today the bar's body is three hard-coded columns:

1. session summary
2. recent chat
3. processes + recent files

This spec turns that body into a configurable sequence of **containers**, each
holding one or more **modules** drawn from a registry. Containers divide the
body's width; modules within a container stack vertically. Chrome (header
strip, rules, reply input) stays fixed.

## Goals

- Let the user configure 1–4 containers; the bar's body width is divided
  equally among them.
- Let each container hold one or more modules, stacked vertically in the
  order listed.
- Ship four built-in modules: `session_summary`, `recent_chat`, `processes`,
  `recent_files`. Splits today's combined procs+files column into two
  independent modules.
- Modules live in their own crate-root directory (`src/detail_modules/`) and
  implement a `DetailModule` trait, registered into a `Registry` owned by
  `App`.
- Per-repo overrides replace `containers` wholesale; scalar fields (`visible`,
  `height`) still merge per-field.
- Surface the config through the existing `wsx config edit detail_bar_config`
  CLI and the existing repo-settings modal.

## Non-goals

- External/plugin module loading (WASM, dlopen, separate repos). The trait +
  registry boundary is designed to permit this later; no host code for it
  ships now.
- Per-container width control. All N containers get `100/N` width. A `weight`
  field could be added later without breaking existing blobs.
- A discovery CLI (`wsx detail-modules list`). Only four built-ins exist;
  documenting them in the README is enough until external modules arrive.
- Header strip configurability. The header stays a fixed chrome layer.
- Reply input as a module. Reply stays chrome — it's the dashboard's primary
  call to action and gets `visible: false` opt-out via the existing
  whole-bar toggle.
- Per-workspace overrides. Per-repo is the deepest scope.
- Backwards compatibility with the previous `{sections: {...}}` schema. wsx
  has no users besides the author; legacy blobs fail to parse and fall back
  to the new default (which reproduces today's content). One-time visible
  regression on first launch with a custom config; user re-edits.
- SQLite schema changes. The `detail_bar_config TEXT` column on `repos`
  already exists.

## Architecture overview

Five touch points — one new directory, four localized changes:

1. **`src/detail_modules/` (new)** — owns the `DetailModule` trait,
   `DetailContext` push-bundle, `Registry`, and one file per built-in:
   `session_summary.rs`, `recent_chat.rs`, `processes.rs`,
   `recent_files.rs`. `mod.rs` re-exports the trait + a
   `register_builtins(&mut Registry)` helper. Optional `util.rs` for
   formatting helpers used only by modules.
2. **`src/detail_bar_config.rs`** — existing file, **replaced wholesale**. New
   top-level shape with `visible`, `height`, `containers: Vec<Vec<String>>`.
   The old `Sections` struct goes away.
3. **`src/app.rs`** — owns a `Registry` built once at startup. Threaded into
   `DetailInputs`. The repo-settings modal's `RepoSettingField::DetailBarConfig`
   flow stays — only the seed buffer and the parser change.
4. **`src/ui/dashboard/detail.rs`** — chrome rendering stays (header strip,
   rules, reply input, height math). Body rendering replaced with a generic
   "build N containers, each with stacked modules" routine that dispatches
   via the registry. The `Column` enum, `enabled_columns`, `column_widths`,
   and per-column render functions are deleted (the per-column code moves
   into the corresponding module files).
5. **`src/store.rs` / `src/cli.rs`** — unchanged at the schema level. The
   CLI's seed JSON for `wsx config edit detail_bar_config` updates to the
   new default; the parser swaps to the new struct.

**Lifetime of a draw** (workspace selected on dashboard):

1. `App::draw` calls `detail_bar_config::resolve(repo, store)` →
   `DetailBarConfig`.
2. If `cfg.visible == false`, the bar isn't drawn (existing chrome flow).
3. Else: `cfg.preferred_height(area.height)` is computed — depends only on
   `height` and on whether any container is non-empty.
4. The bar region is laid out into chrome rows + body region.
5. The body region is split horizontally into `cfg.containers.len()` equal-
   width columns.
6. Each container's column is split vertically by collecting
   `module.height_hint()` for each module ID (after registry lookup) and
   feeding the constraints to `Layout::vertical(...)`.
7. For each module, `registry.get(id)` resolves to either `Some(module)` →
   `module.render(area, &ctx, frame)`, or `None` → render a 1-row
   `[unknown: id]` placeholder in the dim style.

Single PR, no rollout flag. Default `DetailBarConfig` reproduces today's
content (three containers: `[["session_summary"], ["recent_chat"],
["processes", "recent_files"]]`); the only visible default-state change is
the column widths shifting from `30/40/30` to `33/33/34` because the new
system isn't content-aware about which slot deserves more room.

## Module trait and built-in modules

### Trait

```rust
// src/detail_modules/mod.rs

use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub trait DetailModule: Send + Sync {
    /// Stable identifier used in config JSON. Lowercase snake_case.
    fn id(&self) -> &'static str;

    /// Heading drawn above the module's body. Rendered by the host;
    /// modules don't draw their own title.
    fn title(&self) -> &'static str;

    /// Vertical sizing hint used when multiple modules stack in one
    /// container. Fed directly to `Layout::vertical(...)`. Receives the
    /// context so data-dependent modules (e.g. `Processes`) can size to
    /// their current contents.
    fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint;

    /// Render the module's body into `area`. The host has already
    /// drawn the title row above `area` and reserved a 1-row gap below
    /// (when not last in container).
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>);
}
```

### DetailContext

The push-bundle. A struct of borrowed references; zero allocations per draw.
Effectively today's `DetailInputs` minus the reply-input fields (those stay
in the chrome layer).

```rust
pub struct DetailContext<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub diff_per_file: Option<&'a HashMap<String, DiffStats>>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<&'a str>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub events_scanned: bool,
    pub theme: &'a Theme,
}
```

Every built-in module reads only the fields it needs. New modules that need
new data sources grow the struct.

### Registry

```rust
pub struct Registry {
    modules: HashMap<&'static str, Box<dyn DetailModule>>,
}

impl Registry {
    pub fn new() -> Self;
    pub fn register(&mut self, m: Box<dyn DetailModule>);
    pub fn get(&self, id: &str) -> Option<&dyn DetailModule>;
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_;
}

pub fn register_builtins(reg: &mut Registry) {
    reg.register(Box::new(session_summary::SessionSummary));
    reg.register(Box::new(recent_chat::RecentChat));
    reg.register(Box::new(processes::Processes));
    reg.register(Box::new(recent_files::RecentFiles));
}
```

`App` owns a single `Registry`, built once at startup. Tests can build their
own with mock modules — no global state.

### Built-ins

| File | Struct | `id()` | `title()` | `height_hint()` | Reads from `DetailContext` |
|---|---|---|---|---|---|
| `session_summary.rs` | `SessionSummary` | `"session_summary"` | `"SESSION SUMMARY"` | `Min(3)` | `events`, `events_scanned` |
| `recent_chat.rs` | `RecentChat` | `"recent_chat"` | `"RECENT CHAT"` | `Min(3)` | `events`, `events_scanned` |
| `processes.rs` | `Processes` | `"processes"` | `"PROCESSES"` | `Length(ctx.procs.len().clamp(1, 6) as u16)` | `procs` |
| `recent_files.rs` | `RecentFiles` | `"recent_files"` | `"RECENT FILES"` | `Min(3)` | `events`, `diff_per_file` |

`Min(3)` for summary/chat/files = "give me as much as you can, but at least
3 rows." When stacked together, `Layout::vertical` distributes leftover
space evenly among `Min`-constrained children. `Processes` returns a
`Length` sized to the actual process count (1 row per proc, capped at 6)
so a workspace with 1 process doesn't reserve a tall slot when stacked
with `recent_files`.

Each module's `render(...)` body is extracted mechanically from the
corresponding `render_*_column` function in today's `src/ui/dashboard/detail.rs`:
move the body half (the title row is now drawn by the host) into the module's
`render` impl, replace `inputs.field` references with `ctx.field`. The
combined `render_procs_and_files_column` splits in two — one impl per module
file.

## Config schema and resolution

### Schema

```rust
// src/detail_bar_config.rs

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailBarConfig {
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub height: Height,
    #[serde(default = "default_containers")]
    pub containers: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Height {
    #[serde(default = "default_percent")]   pub percent: u8,    // [5, 80]
    #[serde(default = "default_min_rows")]  pub min_rows: u16,  // [4, 40]
    #[serde(default = "default_max_rows")]  pub max_rows: u16,  // [min_rows, 60]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DetailBarOverride {
    pub visible: Option<bool>,
    pub height: Option<HeightOverride>,
    pub containers: Option<Vec<Vec<String>>>,  // whole-replace, not per-index
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HeightOverride {
    pub percent: Option<u8>,
    pub min_rows: Option<u16>,
    pub max_rows: Option<u16>,
}
```

`#[serde(default)]` on every field means partial JSON parses cleanly
(`{"visible": false}` round-trips with everything else at default). This is
the forward-compat lever for future schema additions.

### Defaults

```json
{
  "visible": true,
  "height": { "percent": 30, "min_rows": 8, "max_rows": 18 },
  "containers": [
    ["session_summary"],
    ["recent_chat"],
    ["processes", "recent_files"]
  ]
}
```

Reproduces today's content; only column widths shift (33/33/34 vs 30/40/30).

### Validation and sanitization

```rust
impl DetailBarConfig {
    pub fn sanitize(&mut self) {
        self.height.percent  = self.height.percent.clamp(5, 80);
        self.height.min_rows = self.height.min_rows.clamp(4, 40);
        if self.height.max_rows < self.height.min_rows {
            std::mem::swap(&mut self.height.min_rows, &mut self.height.max_rows);
        }
        self.height.max_rows = self.height.max_rows.clamp(self.height.min_rows, 60);

        if self.containers.is_empty() {
            tracing::warn!("detail_bar_config.containers was empty; using default layout");
            *self = Self::default();
            return;
        }
        if self.containers.len() > 4 {
            tracing::warn!(
                len = self.containers.len(),
                "detail_bar_config.containers > 4; truncating to first 4"
            );
            self.containers.truncate(4);
        }
        // Empty inner lists (spacer containers) are kept — the user opted in.
        // Module-ID validity is checked at render time; see Edge cases.
    }

    pub const CHROME_ROWS: u16 = 4;

    pub fn has_body(&self) -> bool {
        self.containers.iter().any(|c| !c.is_empty())
    }

    pub fn preferred_height(&self, total: u16) -> u16 {
        if !self.has_body() {
            return Self::CHROME_ROWS;
        }
        let target = (u32::from(total) * u32::from(self.height.percent) / 100) as u16;
        target.clamp(self.height.min_rows, self.height.max_rows)
    }

    pub fn with_override(mut self, ovr: &DetailBarOverride) -> Self {
        if let Some(v) = ovr.visible { self.visible = v; }
        if let Some(h) = &ovr.height {
            if let Some(p) = h.percent  { self.height.percent  = p; }
            if let Some(m) = h.min_rows { self.height.min_rows = m; }
            if let Some(x) = h.max_rows { self.height.max_rows = x; }
        }
        if let Some(c) = &ovr.containers {
            self.containers = c.clone();
        }
        self
    }
}
```

### Resolution

```rust
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
            Err(e)  => tracing::warn!(
                err = %e, repo = %repo.name,
                "detail_bar_config repo override parse failed; ignoring"
            ),
        }
    }

    cfg.sanitize();
    cfg
}
```

Computed once per `App::draw`, after the selected workspace is known.

### Edit surfaces (no plumbing change)

- **`wsx config edit detail_bar_config`** — existing flow in `src/cli.rs`.
  Updates: seed the empty buffer with
  `serde_json::to_string_pretty(&DetailBarConfig::default())`; on save,
  parse with the new struct, call `sanitize()`, persist the re-serialized
  pretty JSON. Same pattern as today, pointed at the new schema.
- **Repo-settings modal** (`R` keybind) — `RepoSettingField::DetailBarConfig`
  row stays. Empty-buffer seed stays `"{}\n"` (an empty override means
  "inherit global"). Save parses with `DetailBarOverride`; error modal on
  failure, otherwise persists.
- **`[d] clear`** on the row calls `set_repo_detail_bar_config(id, None)`.
- **`wsx config get detail_bar_config`** — unchanged; returns whatever's
  stored (empty string if unset).
- **`wsx config set detail_bar_config <file>`** — parses the file content
  with the new struct, calls `sanitize()`, persists the re-serialized
  pretty JSON. Parse failure prints the error and exits non-zero without
  touching the store. Matches the `config edit` validation behavior.

### Worked example

Global is default (3 containers, all four built-in modules). Repo `foo`'s
`detail_bar_config`:

```json
{ "containers": [["recent_chat"], ["processes", "recent_files"]] }
```

Result for workspaces in `foo`: 2 containers, 50/50 width. Container 1:
recent_chat. Container 2: processes stacked above recent_files. `visible`
and `height` inherit from global. Other repos unaffected.

## Layout and rendering integration

### File layout after the change

```
src/
  detail_bar_config.rs       (new schema + resolve)
  detail_modules/
    mod.rs                   (trait, registry, DetailContext, register_builtins)
    session_summary.rs
    recent_chat.rs
    processes.rs
    recent_files.rs
    util.rs                  (optional; module-only formatting helpers)
  ui/dashboard/
    detail.rs                (chrome + generic body splitter, ~400 lines after extraction)
```

### `src/ui/dashboard/detail.rs` after extraction

Keeps: header strip, top/bottom rules, reply input row, the body splitter
that turns `cfg.containers` into rendered columns of stacked modules.

`DetailInputs<'a>` gains two fields:

```rust
pub config: &'a DetailBarConfig,
pub registry: &'a Registry,
```

Body builder, roughly:

```rust
fn render_body(frame: &mut Frame, area: Rect, inputs: &DetailInputs<'_>) {
    let cfg = inputs.config;
    if !cfg.has_body() || area.height == 0 {
        return;
    }

    // Narrow-terminal collapse: < 80 cols → first non-empty container only.
    let containers: Vec<&Vec<String>> = if area.width < 80 {
        cfg.containers.iter().find(|c| !c.is_empty()).into_iter().collect()
    } else {
        cfg.containers.iter().collect()
    };

    let widths = equal_widths(containers.len());
    let column_areas = Layout::horizontal(
        widths.iter().map(|&w| Constraint::Percentage(w)).collect::<Vec<_>>()
    ).split(area);

    let ctx = DetailContext {
        repo: inputs.repo,
        workspace: inputs.workspace,
        events: inputs.events,
        procs: inputs.procs,
        diff: inputs.diff,
        diff_per_file: inputs.diff_per_file,
        lifecycle: inputs.lifecycle,
        pr_title: inputs.pr_title.as_deref(),
        pr_number: inputs.pr_number,
        status: inputs.status,
        ago_secs: inputs.ago_secs,
        events_scanned: inputs.events_scanned,
        theme: inputs.theme,
    };

    for (col_area, ids) in column_areas.iter().zip(containers.iter()) {
        render_container(frame, *col_area, ids, &ctx, inputs.registry);
    }
}

fn render_container(
    frame: &mut Frame,
    area: Rect,
    module_ids: &[String],
    ctx: &DetailContext<'_>,
    reg: &Registry,
) {
    if module_ids.is_empty() || area.height == 0 { return; }

    enum Slot<'a> { Found(&'a dyn DetailModule), Unknown(&'a str) }
    let slots: Vec<Slot> = module_ids.iter()
        .map(|id| match reg.get(id) {
            Some(m) => Slot::Found(m),
            None    => Slot::Unknown(id.as_str()),
        })
        .collect();

    // Each slot: 1 title row + body (per height_hint) + 1 gap row (omit gap on last).
    // Unknown placeholder body = Length(0); only the title row renders.
    let constraints: Vec<Constraint> = slots.iter().enumerate().flat_map(|(i, slot)| {
        let last = i == slots.len() - 1;
        let body = match slot {
            Slot::Found(m)   => m.height_hint(ctx),
            Slot::Unknown(_) => Constraint::Length(0),
        };
        let title = Constraint::Length(1);
        let gap   = if last { Constraint::Length(0) } else { Constraint::Length(1) };
        [title, body, gap]
    }).collect();

    let chunks = Layout::vertical(constraints).split(area);

    for (i, slot) in slots.iter().enumerate() {
        let title_area = chunks[i * 3];
        let body_area  = chunks[i * 3 + 1];
        match slot {
            Slot::Found(m) => {
                render_title(frame, title_area, m.title(), ctx.theme);
                m.render(body_area, ctx, frame);
            }
            Slot::Unknown(id) => {
                render_unknown_placeholder(frame, title_area, id, ctx.theme);
            }
        }
    }
}

fn equal_widths(n: usize) -> Vec<u16> {
    match n {
        0 => vec![],
        1 => vec![100],
        2 => vec![50, 50],
        3 => vec![33, 33, 34],
        4 => vec![25, 25, 25, 25],
        _ => unreachable!("sanitize() guarantees containers.len() in 1..=4"),
    }
}
```

The title-row + body + gap split keeps the chrome layer in charge of headings
(consistent typography, no module can muck it up). Modules receive only the
body `Rect`; the title is already drawn above.

### Narrow-terminal collapse

`area.width < 80` → render only the first *non-empty* container at 100%
width. The user picks which container survives by ordering — whichever
appears first in `containers` wins. Today's collapse always picks SESSION
SUMMARY; the new behavior gives the user the lever.

### `src/app.rs` changes

1. **Registry initialization** in `App::new`:
   ```rust
   let mut registry = Registry::new();
   detail_modules::register_builtins(&mut registry);
   ```
   Stored as `pub registry: detail_modules::Registry`. Built once, immutable
   for the rest of the process.
2. **Threading into the draw path** — `DetailInputs` construction adds
   `config: &cfg` and `registry: &self.registry`.
3. **`dashboard_regions`** — `detail_visible` check already keys off
   `cfg.visible`. No change.
4. **Focus model** — reply input is still chrome; existing
   `PaneFocus::DetailBarReply` flow stays. `cfg.visible = false` already
   drops focus correctly. No change.
5. **Repo-settings modal** — `RepoSettingField::DetailBarConfig` already
   exists. The save handler parses with the new `DetailBarOverride`
   (containers field added); the editor seed for the global edit uses the
   new default JSON. Both are one-line swaps in existing arms.

### Deletions in `detail.rs`

- The `Column` enum, `enabled_columns()`, `column_widths()`.
- `render_session_summary_column`, `render_recent_chat_column`,
  `render_procs_and_files_column` — body code moves to the corresponding
  module files. The combined procs+files renderer splits into two impls.
- Per-column helpers used only by one column move into that module's file.
- Shared helpers (text wrapping, time formatting) used by both chrome and
  modules either stay in `detail.rs` (and become `pub(crate)`) or move
  into `src/detail_modules/util.rs` if they're module-only.

## Edge cases

### Bad config (parse / range / IDs)

- **Corrupt JSON in global blob** → `from_str` returns `Err`; logged;
  default used. Dashboard renders default layout.
- **Corrupt JSON in per-repo override** → logged with repo name; override
  ignored; global still applies.
- **`percent` outside `[5, 80]`** → `sanitize()` clamps; the clamped value
  is what `wsx config edit` writes back on save.
- **`min_rows > max_rows`** → `sanitize()` swaps.
- **`containers` is empty (`[]`)** → `sanitize()` logs and resets to
  default. (Functionally equivalent to `visible: false` but harder to
  reason about; treat as user error and recover.)
- **`containers.len() > 4`** → `sanitize()` logs and truncates to first 4.
- **Empty inner list (e.g. `[["session_summary"], [], ["recent_chat"]]`)** →
  kept; renders as an empty column at its equal-width slot. The user opted
  into the spacer.
- **Unknown module ID** → registry returns `None`; the container reserves
  a 1-row slot rendering `[unknown: <id>]` in dim style;
  `tracing::warn!` logged. Other modules in the container render normally.
- **Same module ID twice in same container** → both render. Idempotent
  registry lookup; rendering twice is harmless.
- **Same module ID across containers** → both render.

### Degenerate layouts

- **All containers empty (`[[], [], []]`)** → `has_body()` returns false;
  `preferred_height` returns `CHROME_ROWS` (4); bar is 4 rows of chrome.
- **All modules unknown** → `has_body()` returns true (containers
  non-empty); bar takes normal height; each container shows a 1-row
  placeholder, rest blank. Loud enough to notice.
- **Narrow terminal (`area.width < 80`)** → first non-empty container at
  100% width. Multi-module stacks inside it render normally. All-empty
  case behaves like the above.
- **`visible = true` + all-empty containers** → chrome only (4 rows).
- **`visible = false` + populated `containers`** → bar hidden; list
  preserved; reappears when `visible` flips back.

### Runtime drift

- **Mid-session CLI edit flips `visible = false` while focus is on reply
  input** → next draw observes `cfg.visible == false`, focus auto-returns
  to Dashboard, draft cleared. Existing pattern.
- **Selection moves to a workspace whose repo overrides `visible = false`**
  → same focus-drop pattern. Existing.
- **Selection moves between repos with different overrides** → bar's
  column count and modules change per selection. By design — per-repo
  settings follow the selection.
- **Mid-session edit changes module composition** → next draw picks up
  new config via per-frame `resolve(...)`. No focus implications (focus
  lives only in chrome).

### Migration / schema drift

- **Existing global blob in old shape on first launch** → parse fails;
  logged; default used. One-time visible regression for the author; re-edit
  to the new shape.
- **Existing per-repo override in old shape** → parse fails; logged;
  override ignored; repo inherits global.
- **Future schema additions** — `#[serde(default)]` on every field keeps
  old blobs parsing as new fields appear.
- **No SQLite migration needed** — `detail_bar_config TEXT` column on
  `repos` already exists from the previous spec.

## Testing

### `src/detail_bar_config.rs` (replaces existing test module)

- `default()` returns documented defaults.
- Round-trip via `to_string_pretty` + `from_str` is lossless.
- Parsing `{}` yields `default()`.
- Parsing `{"visible": false}` fills other fields with defaults.
- Parsing `{"unknown_field": 123}` succeeds (forward-compat).
- Parsing `{"containers": [["a", "b"], ["c"]]}` yields expected list-of-lists.
- `sanitize` clamps `percent` to `[5, 80]`.
- `sanitize` clamps `min_rows`/`max_rows` and swaps inverted.
- `sanitize` truncates `containers` to 4 when longer.
- `sanitize` resets to default when `containers` is empty.
- `sanitize` leaves empty inner lists alone.
- `has_body` returns true iff at least one container is non-empty.
- `preferred_height` returns `CHROME_ROWS` when `!has_body`; else
  percent-clamped target. Cases for `total ∈ {20, 50, 100, 0}`.
- `with_override` merges scalars per-field.
- `with_override` whole-replaces `containers` when `Some`, leaves alone
  when `None`.
- `resolve` returns default when neither global nor repo is set.
- `resolve` returns global when only global is set.
- `resolve` returns global-with-override-applied when both are set.
- `resolve` falls back to default + logs when global JSON malformed.
- `resolve` ignores override + logs when override JSON malformed.

### `src/detail_modules/mod.rs`

- `Registry::new()` is empty; `get("anything") == None`.
- After `register_builtins`, `get("session_summary")`, `get("recent_chat")`,
  `get("processes")`, `get("recent_files")` all return `Some`.
- `get("nonexistent")` returns `None`.
- `ids()` returns all four built-in IDs (collected into a set; order not
  asserted).
- A test-only `MockModule { id, height, marker }` is provided for layout
  tests in other modules.

### Per-module tests (`src/detail_modules/{session_summary,recent_chat,processes,recent_files}.rs`)

For each built-in:

- `id()` returns expected string.
- `title()` returns expected heading.
- `height_hint(ctx)` returns the documented constraint. For `Processes`,
  test with `ctx.procs` of length 0, 1, 3, 6, 10 and verify the returned
  `Length` is `1, 1, 3, 6, 6` respectively.
- `render` into a known-size buffer matches today's per-column output
  (snapshot comparison against fixtures extracted from existing
  `detail.rs` tests). Verifies the extraction is mechanical, not a rewrite.
- `render` with empty `events`/`procs`/etc. produces today's "empty" output
  (no panics, expected placeholder text).

### `src/ui/dashboard/detail.rs` (extend existing test module)

- 3-container body (default) renders at 33/33/34 (new snapshot; old
  30/40/30 retired).
- 2-container body renders at 50/50.
- 1-container body renders at 100%.
- 4-container body renders at 25/25/25/25.
- Body collapses to 0 rows when all containers empty; chrome renders;
  total height is `CHROME_ROWS`.
- Container with two stacked modules: title rows, body areas, gap row land
  at rows predicted by `height_hint()` + chrome allocation.
- Container with an unknown module ID renders 1-row `[unknown: <id>]`
  placeholder; siblings render normally.
- Two unknown IDs in one container render two placeholders.
- Narrow terminal (`area.width = 60`) renders first non-empty container
  at 100%.
- Narrow terminal with first container empty renders the second container.
- Narrow terminal with all containers empty renders nothing in body region.

### `src/app.rs` (extend existing tests)

- `App::new` populates `registry` with the four built-ins.
- Repo-settings modal save with valid `DetailBarOverride` JSON updates
  `repo.detail_bar_config`.
- Repo-settings modal save with invalid JSON preserves prior value, shows
  error.
- `[d] clear` on `detail_bar_config` row clears the override.
- Tab cycle when `cfg.visible = false` skips `DetailBarReply` (re-verified).
- Focus on `DetailBarReply` is dropped when resolved config flips to
  `visible = false` mid-session.

### `src/cli.rs` (extend existing tests)

- `ConfigEdit { key: "detail_bar_config" }` seeds editor buffer with
  pretty-printed new default when stored value is empty.
- `ConfigEdit` with malformed JSON saved by user: prior value preserved,
  error printed.
- `ConfigEdit` with `containers.len() > 4`: saves truncated value.
- `ConfigEdit` with `containers = []`: saves default (sanitize resets).
- `ConfigGet { key: "detail_bar_config" }` returns pretty JSON when set,
  empty when unset.

### Manual verification (`docs/manual-tests/`)

New walkthrough file:

1. Launch wsx with the test fixture; select a workspace; observe default
   3-container layout with equal-width columns.
2. `wsx config edit detail_bar_config` → observe pretty-printed default JSON.
3. Change `containers` to `[["recent_chat"]]`. Save. Observe single
   full-width chat column.
4. Change `containers` to `[["session_summary"], ["recent_chat"],
   ["processes"], ["recent_files"]]`. Save. Observe 4 equal-width columns.
5. Change one container to stack two modules: `["processes", "recent_files"]`.
   Save. Observe procs above files, sized by their `height_hint`.
6. Introduce a typo (`"seshun_summary"`). Save. Observe
   `[unknown: seshun_summary]` placeholder in that slot.
7. Open repo-settings modal (`R`), set override to
   `{"containers": [["recent_chat"]]}`. Save. Observe only this repo's
   workspaces show the single chat column.
8. Press `[d]` on the override row → bar returns to global layout for
   this repo.
9. Set `{"visible": false}` globally. Observe bar disappears entirely.
10. Resize terminal to < 80 cols. Observe only first non-empty container
    renders.

## Public surface

```rust
// src/detail_bar_config.rs
pub struct DetailBarConfig { /* ... */ }
pub struct Height { /* ... */ }
pub struct DetailBarOverride { /* ... */ }
pub struct HeightOverride { /* ... */ }

impl Default for DetailBarConfig { /* documented defaults */ }
impl DetailBarConfig {
    pub const CHROME_ROWS: u16 = 4;
    pub fn with_override(self, ovr: &DetailBarOverride) -> Self;
    pub fn sanitize(&mut self);
    pub fn has_body(&self) -> bool;
    pub fn preferred_height(&self, total: u16) -> u16;
}

pub fn resolve(repo: &Repo, store: &Store) -> DetailBarConfig;

// src/detail_modules/mod.rs
pub trait DetailModule: Send + Sync {
    fn id(&self) -> &'static str;
    fn title(&self) -> &'static str;
    fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint;
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>);
}
pub struct DetailContext<'a> { /* borrowed fields, including pr_title: Option<&'a str> */ }
pub struct Registry { /* ... */ }
impl Registry {
    pub fn new() -> Self;
    pub fn register(&mut self, m: Box<dyn DetailModule>);
    pub fn get(&self, id: &str) -> Option<&dyn DetailModule>;
    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_;
}
pub fn register_builtins(reg: &mut Registry);
```

## Rollout

Single PR. No feature flag. Default `DetailBarConfig` reproduces today's
content; only column widths shift (33/33/34 vs 30/40/30). Users with custom
config blobs (just the author) see a one-time fallback to default on first
launch and re-edit to the new shape.

The trait + registry boundary leaves room for future external module loading
without further architecture work — adding a plugin host means new
`Registry::register` calls (from a loader module) and possibly an ABI shim
around `DetailModule`. No schema change required.
