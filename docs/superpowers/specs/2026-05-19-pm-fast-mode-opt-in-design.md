# Project Manager fast-mode opt-in — Design

## Goal

Let the user opt into running the wsx Project Manager (PM) session with Claude
Code's "fast mode" enabled. PM is a status-summary session — short answers,
read-only tools — and benefits more from output latency than from any other
quality dimension. The user should be able to flip this on once and have it
apply to every PM spawn going forward.

## Approach

Claude Code's fast mode is normally toggled in-session via the `/fast` slash
command. Two facts shape the design:

1. `/fast` is a **toggle**, not an idempotent setter. Sending it on every PM
   spawn would break the `--continue` resume path: PM that's already in fast
   mode from the prior wsx run would get flipped OFF.
2. Claude Code's `settings.json` accepts a `"fastMode": true` key, and the
   `--settings` CLI flag accepts an inline JSON string. `"fastMode": true` is
   idempotent: applying it when fast mode is already on is a no-op.

So the safe mechanism is to pass `--settings '{"fastMode":true}'` to claude on
every PM spawn when the wsx-side opt-in is on. Single chokepoint:
`build_claude_command` in `src/pty/session.rs`, which is where the
existing PM-only flags (`--dangerously-skip-permissions`, the PM system
prompt) are already emitted.

## Decisions

- **Setting name `pm_fast_mode`** — matches the existing `pm_*` namespace
  (`pm_enabled`, `pm_custom_instructions`). Defaults OFF. On-values
  `true`/`on`/`1`/`yes` (same convention as `remote_control_sandbox`).
- **Scope: PM only.** Workspace sessions are explicitly not affected. The
  rationale for fast mode (terse, status-style summaries) is PM-specific.
  Workspace sessions need full thinking quality for code work.
- **Inline `--settings '{"fastMode":true}'`, not a `pm_dir/.claude/settings.json`
  file.** The wsx settings DB stays the single source of truth. Flipping
  `pm_fast_mode` off in wsx immediately stops emitting the flag on the next
  PM spawn — no on-disk cleanup, no risk of stale state.
- **Re-apply on every spawn, fresh or `--continue`.** `"fastMode": true` is
  idempotent in Claude's settings layer, and fast mode persists across
  `--continue` resumes anyway, so the flag is harmless on resume and ensures
  the wsx setting is authoritative.
- **No new keybinding to toggle at runtime.** Persistent setting is enough
  for v1. If runtime toggling becomes a frequent ask, a future `Ctrl-x f` or
  similar can call `/fast` against the live PM session.
- **No UI surfacing.** The user sees fast-mode status inside the PM PTY (the
  claude session indicates it). wsx doesn't need to render it.

## Scope

### In

1. New helper `pm_fast_mode_enabled(&Store) -> bool` in `src/pm.rs`,
   matching the shape of `remote_control::sandbox_enabled` (defaults false,
   true for on-values).
2. New field `fast_mode: bool` on `SpawnMode::ProjectManager` in
   `src/pty/session.rs`. Other `SpawnMode` variants are untouched.
3. `open_pm` in `src/pm.rs` reads the setting once and passes it through into
   the `SpawnMode::ProjectManager` it constructs.
4. `build_claude_command` emits `--settings '{"fastMode":true}'` when
   `SpawnMode::ProjectManager { fast_mode: true, .. }`. Emitted for both
   fresh and `--continue` PM spawns.
5. Register `pm_fast_mode` in `cli::known_setting_key`.
6. One bullet in `README.md`'s setting list documenting `pm_fast_mode`.
7. Tests:
   - `known_setting_key("pm_fast_mode")` accepted (extends the existing
     allow-list test).
   - `pm_fast_mode_enabled` defaults false; true for `true`/`on`/`1`/`yes`;
     false for `false`/`off`/`0`/`no` and other values.
   - `build_claude_command` argv assertion: contains `--settings` with
     `{"fastMode":true}` when `ProjectManager { fast_mode: true, .. }`; does
     not contain it when `fast_mode: false`; does not contain it for `Fresh`
     or `Continue` modes regardless.

### Out

- Applying fast mode to workspace sessions. Out of scope for this design and
  not currently desired.
- A runtime keybinding to flip PM fast mode without restarting PM.
- Per-repo override. PM is global; per-repo doesn't apply.
- Detecting whether the installed Claude Code binary supports `fastMode` in
  settings. If the user is on an old version that ignores the key, the
  setting is a silent no-op — acceptable for an opt-in feature.
- Surfacing fast-mode status in the wsx PM pane title or footer.

## Implementation notes

### Helper

```rust
// src/pm.rs (new free function near the existing PM helpers)

/// Defaults OFF. On-values: `true` / `on` / `1` / `yes`.
pub fn pm_fast_mode_enabled(store: &crate::store::Store) -> bool {
    matches!(
        store.get_setting("pm_fast_mode").ok().flatten().as_deref(),
        Some("true" | "on" | "1" | "yes")
    )
}
```

### `SpawnMode::ProjectManager` field

```rust
// src/pty/session.rs
SpawnMode::ProjectManager {
    workspaces_json_path: PathBuf,
    custom_instructions: Option<String>,
    additional_dirs: Vec<PathBuf>,
    resume: bool,
    fast_mode: bool, // NEW
}
```

### Argv emission in `build_claude_command`

The existing PM-mode match arm in `build_claude_command` (currently at
`src/pty/session.rs:301`) destructures `ProjectManager { … }`. Add
`fast_mode` to the destructure and, after the existing flag emission for
`--dangerously-skip-permissions` and `--continue`, emit:

```rust
if matches!(mode, SpawnMode::ProjectManager { fast_mode: true, .. }) {
    cmd.arg("--settings");
    cmd.arg(r#"{"fastMode":true}"#);
}
```

(Exact code placement chosen during implementation; the spirit is "PM-only,
guarded by the field".)

### Wiring in `open_pm`

`open_pm` already reads the store to write `workspaces.json`. Add one line
to read `pm_fast_mode_enabled(store)` and pass it into the
`SpawnMode::ProjectManager` it builds.

`open_pm_with_auto_summary` and `open_pm_with_refresh` delegate to `open_pm`,
so no changes there.

### README

One bullet under the existing setting list:

> `pm_fast_mode` (default off) — when on, the Project Manager session
> launches with Claude Code's fast mode enabled. PM is a status-summary
> session, so fast output is usually the right tradeoff. `wsx config set
> pm_fast_mode on`.

## Risks

- **Claude `fastMode` settings key renamed or removed.** Low probability,
  but if Anthropic renames the key, the setting silently stops working.
  Mitigation: opt-in default means there's no breakage for non-opted-in
  users, and the failure mode is "fast mode doesn't engage" rather than a
  crash.
- **`--settings` inline-JSON parsing.** Claude Code accepts a JSON string
  per `claude --help`. If a future version restricts `--settings` to file
  paths only, we'd need to write a temp file. Mitigation: same as above —
  silent degrade, easy fix later.
- **User toggles fast mode off mid-PM-session with `/fast`.** That's
  expected behavior. wsx's setting governs the *next* spawn, not the
  running PTY. Same model as every other claude setting in wsx.
- **PM caches conversation context across runs.** Switching `pm_fast_mode`
  mid-stream could feel inconsistent (some replies pre-toggle, some
  post-toggle). Acceptable — the user opted in.

## Out-of-scope follow-ups

- Runtime keybinding to toggle PM fast mode without restarting PM.
- Per-workspace fast-mode opt-in (analogous to `yolo`).
- Detecting old claude-code binaries that don't recognize `fastMode` and
  warning the user.
- A wsx dashboard or PM-pane glyph indicating "PM is running in fast mode".
