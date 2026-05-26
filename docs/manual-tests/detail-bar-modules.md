# Detail bar modules — manual walkthrough

Verifies the container/module system end-to-end per
[`docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`](../superpowers/specs/2026-05-25-detail-bar-modules-design.md).

## Setup

Launch wsx with a workspace that has at least one running process and
recent agent activity:

```bash
cargo run --release
```

Select a workspace on the dashboard so the detail bar appears.

## Steps

1. **Default layout** — observe three equal-width columns: SESSION SUMMARY
   (left), RECENT CHAT (middle), PROCESSES stacked above RECENT FILES (right).
   Widths are 33/33/34.

2. **Edit the global config:**

   ```bash
   wsx config edit detail_bar_config
   ```

   The editor opens with the pretty-printed default config. Confirm it
   contains a `containers` array of length 3.

3. **Single-container layout** — change `containers` to:

   ```json
   "containers": [["recent_chat"]]
   ```

   Save and exit. The detail bar collapses to a single full-width chat
   column.

4. **Four-container layout** — `wsx config edit detail_bar_config` again,
   change `containers` to:

   ```json
   "containers": [
     ["session_summary"],
     ["recent_chat"],
     ["processes"],
     ["recent_files"]
   ]
   ```

   Save. Observe four equal-width columns (25% each), processes and files
   separated.

5. **Stacked modules** — change one container to stack two modules:

   ```json
   "containers": [
     ["session_summary"],
     ["recent_chat", "recent_files"],
     ["processes"]
   ]
   ```

   Save. Observe the middle column has RECENT CHAT on top and RECENT
   FILES below, sized by their height hints (chat grows; files takes
   its minimum).

6. **Unknown module ID** — change one entry to a typo:

   ```json
   "containers": [["seshun_summary"], ["recent_chat"], ["processes"]]
   ```

   Save. Observe `[unknown: seshun_summary]` placeholder in the left
   column; other columns render normally.

7. **Per-repo override** — open the repo settings modal (`R`), navigate
   to `detail_bar_config`, press Enter. Set:

   ```json
   {"containers": [["recent_chat"]]}
   ```

   Save. Workspaces in this repo show only the single chat column;
   workspaces in other repos still show the global layout.

8. **Clear override** — press `[d]` on the `detail_bar_config` row. The
   bar reverts to the global layout for this repo.

9. **Hide the bar entirely** — `wsx config edit detail_bar_config`,
   set `"visible": false`. Save. The detail bar disappears from the
   dashboard.

10. **Narrow terminal** — resize the terminal below 80 columns. Observe
    only the first non-empty container renders, at 100% width.

## Restore

```bash
wsx config edit detail_bar_config
```

Replace contents with `{}` and save — restores defaults.
