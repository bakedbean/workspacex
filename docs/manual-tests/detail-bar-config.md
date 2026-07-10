# Manual test — configurable detail bar

Spec: `docs/superpowers/specs/2026-05-25-detail-bar-config-design.md`

## Setup

Start wsx with at least one repo registered and a workspace selected.

## 1. Global config CLI

```bash
wsx config edit detail_bar_config
```

Expected: `$EDITOR` opens with pretty-printed default JSON:

```json
{
  "visible": true,
  "height": {
    "percent": 30,
    "min_rows": 8,
    "max_rows": 18
  },
  "sections": {
    "session_summary": true,
    "recent_chat": true,
    "procs_and_files": true
  }
}
```

Change `sections.recent_chat` to `false`, save and exit.

In wsx, select a workspace. Expected: detail bar's middle column is
gone; left + right columns redistribute to 50/50.

## 2. Per-repo override via modal

In the dashboard, press `R` to open the repo-settings modal. Navigate
down to `detail_bar_config` (last row). Press Enter.

Expected: editor opens with `{}\n` (empty override = inherit all).

Type:

```json
{"visible": false}
```

Save and exit. Expected: the detail bar is gone for workspaces in
this repo; workspaces in other repos still show the bar (with the
config from step 1 in effect).

## 3. Clear the override

Reopen the repo-settings modal, navigate to `detail_bar_config`,
press `d`. Expected: row shows `(unset)`. The detail bar returns
when selecting workspaces in this repo.

## 4. Height tuning

```bash
wsx config edit detail_bar_config
```

Set `height.percent` to `50`, save. Select a workspace.

Expected: detail bar takes roughly half the dashboard vertically
(clamped by `max_rows`).

## 5. Out-of-range clamp

```bash
wsx config edit detail_bar_config
```

Set `height.percent` to `200`, save.

Expected: the message "set detail_bar_config (… chars)" is printed.
Re-run `wsx config get detail_bar_config`. Expected: `percent` is
clamped to `80`.

## 6. Empty body

Set globally:

```json
{
  "sections": {
    "session_summary": false,
    "recent_chat": false,
    "procs_and_files": false
  }
}
```

Save. Select a workspace. Expected: bar shrinks to a tight 4 rows —
header strip, two rules, and the reply input row. No empty body
region.

## 7. Narrow terminal

Resize the terminal to under 80 columns wide with a workspace
selected and the default config restored (`wsx config edit
detail_bar_config` → `{}\n` → save). Expected: the body collapses to
the first enabled column (SESSION SUMMARY by default).

Then disable SESSION SUMMARY:

```json
{"sections": {"session_summary": false}}
```

Resize narrow again. Expected: the body collapses to RECENT CHAT.

## 8. Bar hidden + Tab cycle

Set globally:

```json
{"visible": false}
```

Select a workspace and press Tab. Expected: focus stays on Dashboard
if the digest is hidden; cycles between Dashboard and the digest pane
(ProjectManager focus) if the digest is visible. Never enters the
reply input.

Set `visible` back to true. Expected: Tab cycles Dashboard ↔
DetailBarReply (digest hidden) or Dashboard → DetailBarReply →
ProjectManager (digest) → Dashboard (digest visible).

## 9. Malformed JSON

```bash
echo "{not json" | wsx config set detail_bar_config -
```

Expected: command exits non-zero with "detail_bar_config: invalid
JSON: …". The previous valid value is preserved (`wsx config get
detail_bar_config` shows it unchanged).
