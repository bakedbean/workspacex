When a workspace is selected on the dashboard, wsx renders a multi-column
detail bar across the bottom. The body is divided into 1–4 equal-width
**containers**; each container holds one or more **modules** stacked
vertically. Four built-in modules ship today: `session_summary`,
`recent_chat`, `processes`, `recent_files`. The bar's appearance is
controlled by the `detail_bar_config` setting — globally via `wsx config`,
with optional per-repo overrides.

### Schema and defaults

The global value is a full `DetailBarConfig` JSON blob. Every field is
optional; missing fields fall back to defaults. Out-of-range values are
clamped on save (see below).

```json
{
  "visible": true,
  "height": {
    "percent": 30,
    "min_rows": 8,
    "max_rows": 18
  },
  "containers": [
    ["session_summary"],
    ["recent_chat"],
    ["processes", "recent_files"]
  ]
}
```

| Field             | Type          | Default             | Effect                                                                                                                                                                                                                                         |
| ----------------- | ------------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `visible`         | bool          | `true`              | Master toggle. When `false`, the bar is hidden entirely and `Tab` skips the reply input.                                                                                                                                                       |
| `height.percent`  | u8            | `30`                | Target height as a percent of the terminal's rows. Clamped to `[5, 80]`.                                                                                                                                                                       |
| `height.min_rows` | u16           | `8`                 | Floor on the bar's height. Clamped to `[4, 40]`.                                                                                                                                                                                               |
| `height.max_rows` | u16           | `18`                | Ceiling on the bar's height. Clamped to `[4, 60]`. If `min_rows > max_rows`, the two are swapped on save.                                                                                                                                      |
| `containers`      | list of lists | (see default above) | Outer length 1–4: one entry per equal-width column. Inner is a list of module IDs stacked vertically within the column. An empty inner list `[]` reserves an empty column. Empty outer list resets to default. Lengths > 4 are truncated to 4. |

**Built-in module IDs:** `session_summary`, `recent_chat`, `processes`,
`recent_files`. Unknown IDs render a `[unknown: <id>]` placeholder and
log a warning, so typos are visible but don't break the dashboard.

When every container is empty (`[[], [], []]`), the bar shrinks to its
4-row chrome (header + two rules + reply input) regardless of
`height.percent`. That's how you trim the bar to just the reply input.

### Setting the global value

```bash
wsx config edit detail_bar_config     # opens $EDITOR; seeded with the pretty-printed default
wsx config set  detail_bar_config '{"height": {"percent": 50}}'
wsx config get  detail_bar_config
wsx config set  detail_bar_config ""  # clear (reverts to baked-in defaults)
```

Partial JSON is fine — `{"visible": false}` is a complete, valid value.
Missing fields are filled in from defaults. Malformed JSON is rejected
with a non-zero exit and the previous value is preserved.

Examples:

```bash
# Make the bar taller on big monitors.
wsx config set detail_bar_config '{"height": {"percent": 45, "max_rows": 24}}'

# Single full-width chat column.
wsx config set detail_bar_config '{"containers": [["recent_chat"]]}'

# Four columns, processes and files in separate slots.
wsx config set detail_bar_config '{"containers": [["session_summary"], ["recent_chat"], ["processes"], ["recent_files"]]}'

# Hide the bar entirely.
wsx config set detail_bar_config '{"visible": false}'
```

### Per-repo override

Each repo can override any subset of the global config. The per-repo
value is a `DetailBarOverride` — `visible` and `height.*` merge
per-field; `containers` is whole-replace when present, fully-inherited
when absent. An empty `{}` inherits everything; you only specify what
you want to change.

Open the repo settings modal with `s` on the dashboard, select the
`detail_bar_config` row, and press Enter. `$EDITOR` opens on `{}\n`
(or the current override). Save to apply; press `d` on the row to
clear the override and fall back to the global value.

Override examples:

Hide the bar entirely for this repo (global value can stay on):

```json
{ "visible": false }
```

Single chat column for this repo; keep `visible` and `height` inherited from global:

```json
{ "containers": [["recent_chat"]] }
```

Taller bar for a repo where the session-summary text is usually long
(CLI tools with verbose tool-call traces):

```json
{ "height": { "percent": 45, "max_rows": 28 } }
```

Merge precedence: bake-in defaults → global `detail_bar_config` →
per-repo override. `visible` and `height.*` apply per-field; `containers`
whole-replaces when the override sets it. So a repo override that only
sets `containers` still picks up any global `height` changes you make
later.

### Behavior on bad input

- Malformed JSON at the global level — falls back to baked-in defaults at runtime, logged at `warn`.
- Malformed JSON in a repo override — the override is ignored; the global value applies, logged at `warn` with the repo name.
- Out-of-range `height.percent` / `min_rows` / `max_rows` — clamped to legal ranges on save (`wsx config set/edit`) and again at runtime as a defense-in-depth.
- `min_rows > max_rows` — swapped on save so the lower bound is the floor and the higher is the ceiling.
