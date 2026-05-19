# PM fast-mode opt-in Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a persistent wsx setting `pm_fast_mode` (default off) that, when on, launches the Project Manager claude session with Claude Code's fast mode enabled via `--settings '{"fastMode":true}'`.

**Architecture:** New `pm_fast_mode_enabled` helper in `src/pm.rs` reads the setting. A new `fast_mode: bool` field on `SpawnMode::ProjectManager` carries it through to `build_claude_command` in `src/pty/session.rs`, which emits `--settings '{"fastMode":true}'` when both the variant is `ProjectManager` and `fast_mode` is true. `open_pm` is the single wire-up site. Spec: `docs/superpowers/specs/2026-05-19-pm-fast-mode-opt-in-design.md`.

**Tech Stack:** Rust, `portable_pty::CommandBuilder`, `rusqlite`-backed settings store, `cargo test`.

---

## File Structure

- **Modify** `src/cli.rs:98-120` — add `"pm_fast_mode"` to `known_setting_key`. Update test `accepts_pm_enabled_and_pm_custom_instructions` (or add a new test next to it).
- **Modify** `src/pm.rs` — add `pub fn pm_fast_mode_enabled(store) -> bool` and three unit tests. Wire it through `open_pm`'s `SpawnMode::ProjectManager` construction at line 182.
- **Modify** `src/pty/session.rs` — add `fast_mode: bool` field to `SpawnMode::ProjectManager` (declaration at line 225; destructure in `build_claude_command` at line 301; update 5 existing test-construction sites at lines 919, 943, 1028, 1140, 1157). Emit `--settings '{"fastMode":true}'` after permission flags. Add two new `build_claude_command` tests next to the existing PM tests.
- **Modify** `README.md` — add one row to the settings table near line 106 (the `pm_*` rows).

Each task is its own commit. Task 3 is the largest because adding the new field breaks all 6 construction sites until they're updated; that's still one logical change and lands as one commit.

---

### Task 1: Allowlist `pm_fast_mode` in the CLI

**Files:**
- Modify: `src/cli.rs:98-120` (allowlist) and `src/cli.rs:667-672` (test)

- [ ] **Step 1: Write the failing test**

Append a new test in the existing `tests` module in `src/cli.rs` (around line 670, next to `accepts_pm_enabled_and_pm_custom_instructions`):

```rust
    #[test]
    fn accepts_pm_fast_mode() {
        assert!(known_setting_key("pm_fast_mode"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run:
```bash
cargo test -p wsx --lib cli::tests::accepts_pm_fast_mode
```
Expected: FAIL — assertion fails because `pm_fast_mode` is not in `known_setting_key`'s `matches!`.

- [ ] **Step 3: Add the key to the allowlist**

In `src/cli.rs`, modify `known_setting_key` (currently lines 98–120). Add `| "pm_fast_mode"` after `| "pm_custom_instructions"`:

```rust
fn known_setting_key(k: &str) -> bool {
    matches!(
        k,
        "branch_prefix"
            | "custom_instructions"
            | "nerd_fonts"
            | "editor_cmd"
            | "terminal_cmd"
            | "diff_cmd"
            | "lazygit_cmd"
            | "notifications"
            | "theme"
            | "pm_enabled"
            | "pm_custom_instructions"
            | "pm_fast_mode"
            | "mcp_mirror"
            | "remote_control"
            | "remote_control_sandbox"
            | "pinned_commands"
            | "remotes"
            | "dashboard_name_width"
            | "dashboard_branch_width"
    )
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:
```bash
cargo test -p wsx --lib cli::tests::accepts_pm_fast_mode
```
Expected: PASS.

- [ ] **Step 5: Run the full cli test module to confirm no regressions**

Run:
```bash
cargo test -p wsx --lib cli::tests
```
Expected: all existing cli tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): allowlist pm_fast_mode setting key"
```

---

### Task 2: Add `pm_fast_mode_enabled` helper to `pm.rs`

**Files:**
- Modify: `src/pm.rs` (add public function plus three tests in the existing `mod tests`)

- [ ] **Step 1: Write the failing tests**

Append three tests inside the existing `mod tests` block at the bottom of `src/pm.rs` (the module starts at line 253). Place them after the existing test functions, before the closing `}` of the module:

```rust
    #[test]
    fn pm_fast_mode_defaults_false_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert!(!pm_fast_mode_enabled(&store));
    }

    #[test]
    fn pm_fast_mode_true_for_on_values() {
        let store = Store::open_in_memory().unwrap();
        for v in ["true", "on", "1", "yes"] {
            store.set_setting("pm_fast_mode", v).unwrap();
            assert!(pm_fast_mode_enabled(&store), "expected enabled for {v:?}");
        }
    }

    #[test]
    fn pm_fast_mode_false_for_off_or_garbage_values() {
        let store = Store::open_in_memory().unwrap();
        for v in ["false", "off", "0", "no", "", "maybe", "FAST"] {
            store.set_setting("pm_fast_mode", v).unwrap();
            assert!(!pm_fast_mode_enabled(&store), "expected disabled for {v:?}");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail to compile**

Run:
```bash
cargo test -p wsx --lib pm::tests::pm_fast_mode_defaults_false_when_unset
```
Expected: COMPILE ERROR — `pm_fast_mode_enabled` is undefined.

- [ ] **Step 3: Add the helper function**

In `src/pm.rs`, immediately after the existing `pm_system_prompt` function (around line 158), add:

```rust
/// Defaults OFF. On-values: `true` / `on` / `1` / `yes`. Anything else is
/// off. PM-only: workspace sessions never look at this setting.
pub fn pm_fast_mode_enabled(store: &crate::store::Store) -> bool {
    matches!(
        store.get_setting("pm_fast_mode").ok().flatten().as_deref(),
        Some("true" | "on" | "1" | "yes")
    )
}
```

- [ ] **Step 4: Run the new tests to verify they pass**

Run:
```bash
cargo test -p wsx --lib pm::tests::pm_fast_mode
```
Expected: all three `pm_fast_mode_*` tests PASS.

- [ ] **Step 5: Run the full pm test module to confirm no regressions**

Run:
```bash
cargo test -p wsx --lib pm::tests
```
Expected: all pm tests still pass.

- [ ] **Step 6: Commit**

```bash
git add src/pm.rs
git commit -m "feat(pm): add pm_fast_mode_enabled setting helper"
```

---

### Task 3: Add `fast_mode` field to `SpawnMode::ProjectManager` and emit `--settings`

This task changes the enum variant signature, so the existing call sites must be updated in the same commit or the build breaks. Six sites total: one production (`src/pm.rs:182`), one production destructure (`src/pty/session.rs:301`), one variant declaration (`src/pty/session.rs:225`), and five test sites (`src/pty/session.rs:919, 943, 1028, 1140, 1157`). Existing sites get `fast_mode: false`. Wiring through `open_pm` to read the setting happens in Task 4.

**Files:**
- Modify: `src/pty/session.rs` (variant declaration, destructure in `build_claude_command`, 5 test construction sites, 2 new tests)
- Modify: `src/pm.rs:182` (add `fast_mode: false` to the `open_pm` construction; Task 4 will replace it with the setting read)

- [ ] **Step 1: Write the failing tests**

Append two tests to the existing `tests` mod in `src/pty/session.rs`, after the existing PM tests (after `project_manager_mode_resume_adds_continue` ends, around line 957). Note: `WSX_CLAUDE_BIN` is set in these tests because the existing PM tests at lines 914-955 do the same — `build_claude_command` reads the env var to pick the binary path.

```rust
    #[test]
    fn project_manager_mode_emits_settings_when_fast_mode() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: true,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        let idx = argv
            .iter()
            .position(|a| a == std::ffi::OsStr::new("--settings"))
            .expect("expected --settings flag when fast_mode is true");
        let value = argv
            .get(idx + 1)
            .expect("expected JSON value after --settings")
            .to_string_lossy();
        assert_eq!(value, r#"{"fastMode":true}"#);
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[test]
    fn project_manager_mode_omits_settings_when_fast_mode_false() {
        unsafe {
            std::env::set_var("WSX_CLAUDE_BIN", "/usr/bin/cat");
        }
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(
            !argv.iter().any(|a| a == std::ffi::OsStr::new("--settings")),
            "expected no --settings flag when fast_mode is false, argv: {argv:?}"
        );
        unsafe {
            std::env::remove_var("WSX_CLAUDE_BIN");
        }
    }

    #[test]
    fn fresh_mode_never_emits_settings_for_fast_mode() {
        let cwd = PathBuf::from(".");
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: None,
            additional_dirs: vec![],
            yolo: false,
        };
        let cmd = build_claude_command(&cwd, &mode, crate::remote_control::RemoteOpts::disabled());
        let argv = cmd.get_argv();
        assert!(
            !argv.iter().any(|a| a == std::ffi::OsStr::new("--settings")),
            "Fresh mode should never emit --settings, argv: {argv:?}"
        );
    }
```

- [ ] **Step 2: Run the tests and verify they fail to compile**

Run:
```bash
cargo test -p wsx --lib pty::session::tests::project_manager_mode_emits_settings_when_fast_mode
```
Expected: COMPILE ERROR — `ProjectManager` variant has no field `fast_mode`.

- [ ] **Step 3: Add the field to the enum variant**

In `src/pty/session.rs`, modify the `ProjectManager` arm of `enum SpawnMode` (currently lines 225-231):

```rust
    /// Spawn the project-manager session. Embeds the PM system prompt and
    /// a read-only tool allowlist. When `resume` is true, also passes
    /// `--continue` to pick up PM's prior conversation. Always uses
    /// `--dangerously-skip-permissions`. When `fast_mode` is true, also
    /// passes `--settings '{"fastMode":true}'` to enable Claude Code's
    /// fast mode for this session.
    ProjectManager {
        workspaces_json_path: std::path::PathBuf,
        custom_instructions: Option<String>,
        // PM has no owning repo, so always empty. Kept for uniformity.
        additional_dirs: Vec<std::path::PathBuf>,
        resume: bool,
        fast_mode: bool,
    },
```

- [ ] **Step 4: Update the destructure in `build_claude_command`**

In `src/pty/session.rs`, modify the `ProjectManager` arm of the match in `build_claude_command` (currently around lines 301-313). The destructure adds `fast_mode`, but the tuple returned by the match doesn't need to carry it — emit the flag directly in a follow-up `if let` after the match. Update like this:

```rust
            SpawnMode::ProjectManager {
                workspaces_json_path: _,
                custom_instructions,
                additional_dirs,
                resume,
                fast_mode: _, // emitted below, after the match
            } => (
                Some(crate::pm::pm_system_prompt(custom_instructions.as_deref())),
                None,
                false,
                *resume,
                true,
                additional_dirs.clone(),
            ),
```

- [ ] **Step 5: Emit `--settings` after the permission/remote-control flags**

In `src/pty/session.rs`, after the `if remote.enabled { … }` block (around line 337) and before the `combined = match (rename_prompt, custom)` line, add:

```rust
    if let SpawnMode::ProjectManager {
        fast_mode: true, ..
    } = mode
    {
        cmd.arg("--settings");
        cmd.arg(r#"{"fastMode":true}"#);
    }
```

Why here: the spec calls for emitting it on PM mode only, after the permission and remote-control flags. Using a separate `if let` on `mode` keeps this PM-specific concern out of the shared 6-tuple destructure above (the destructure already does too much).

- [ ] **Step 6: Update the five existing PM test construction sites**

Each existing `SpawnMode::ProjectManager { … }` in tests needs `fast_mode: false` added. The sites in `src/pty/session.rs` are at lines 919, 943, 1028, 1140, 1157 (approximate after prior edits; grep `SpawnMode::ProjectManager` to locate them precisely).

For each one, add `fast_mode: false,` as the last field. Example for the first one (currently around line 919):

```rust
        let mode = SpawnMode::ProjectManager {
            workspaces_json_path: PathBuf::from("/tmp/x/workspaces.json"),
            custom_instructions: None,
            additional_dirs: vec![],
            resume: false,
            fast_mode: false,
        };
```

Apply the same one-line addition to the other four sites.

- [ ] **Step 7: Update the production call site in `src/pm.rs`**

In `src/pm.rs:182`, the `open_pm` function constructs `SpawnMode::ProjectManager`. Add `fast_mode: false,` as the last field for now — Task 4 will replace this with the setting read.

```rust
    let mode = crate::pty::session::SpawnMode::ProjectManager {
        workspaces_json_path: workspaces_json,
        custom_instructions,
        additional_dirs: vec![],
        resume,
        fast_mode: false,
    };
```

- [ ] **Step 8: Run the three new tests**

Run:
```bash
cargo test -p wsx --lib pty::session::tests::project_manager_mode_emits_settings_when_fast_mode
cargo test -p wsx --lib pty::session::tests::project_manager_mode_omits_settings_when_fast_mode_false
cargo test -p wsx --lib pty::session::tests::fresh_mode_never_emits_settings_for_fast_mode
```
Expected: all three PASS.

- [ ] **Step 9: Run the full pty::session test module to confirm no regressions**

Run:
```bash
cargo test -p wsx --lib pty::session::tests
```
Expected: all pty::session tests pass, including the existing `project_manager_mode_*` tests.

- [ ] **Step 10: Run the full test suite to catch any other call sites**

Run:
```bash
cargo build && cargo test -p wsx --lib
```
Expected: build succeeds; all unit tests pass. If a missed PM construction site causes a compile error, locate it via `grep -rn "SpawnMode::ProjectManager" --include="*.rs" .` and add `fast_mode: false,` there too before re-running.

- [ ] **Step 11: Commit**

```bash
git add src/pty/session.rs src/pm.rs
git commit -m "feat(pty): emit --settings '{\"fastMode\":true}' for PM fast_mode"
```

---

### Task 4: Wire `pm_fast_mode_enabled` through `open_pm`

The setting helper exists (Task 2) and the `fast_mode` field exists (Task 3). This task replaces the `fast_mode: false` placeholder in `open_pm` with a read from the store. No new tests are needed: both endpoints (the helper and `build_claude_command`'s argv emission) are already covered by Task 2 and Task 3 tests. This is a one-line behavioral change.

**Files:**
- Modify: `src/pm.rs` (one line in `open_pm`)

- [ ] **Step 1: Replace the placeholder in `open_pm`**

In `src/pm.rs`, find the `SpawnMode::ProjectManager` construction in `open_pm` (line 182, where Task 3 left `fast_mode: false`). Change `fast_mode: false,` to:

```rust
        fast_mode: pm_fast_mode_enabled(store),
```

The full block now reads:

```rust
    let mode = crate::pty::session::SpawnMode::ProjectManager {
        workspaces_json_path: workspaces_json,
        custom_instructions,
        additional_dirs: vec![],
        resume,
        fast_mode: pm_fast_mode_enabled(store),
    };
```

- [ ] **Step 2: Run the pm test module**

Run:
```bash
cargo test -p wsx --lib pm::tests
```
Expected: all pm tests still pass. The existing integration tests (`open_pm_spawns_session_and_writes_workspaces_json`, etc.) don't set `pm_fast_mode`, so the default-false behavior is exercised and the PTY spawn argv remains unchanged for them.

- [ ] **Step 3: Run the full library test suite**

Run:
```bash
cargo test -p wsx --lib
```
Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src/pm.rs
git commit -m "feat(pm): wire pm_fast_mode setting into open_pm spawn"
```

---

### Task 5: Document `pm_fast_mode` in the README

**Files:**
- Modify: `README.md` (insert one row in the settings table near line 106)

- [ ] **Step 1: Add the row to the settings table**

In `README.md`, find the existing `pm_custom_instructions` row (currently line 106). Insert a new row immediately after it:

```markdown
| `pm_fast_mode` | Launch the Project Manager session with Claude Code's fast mode enabled (`--settings '{"fastMode":true}'`). PM is a status-summary session, so fast output is usually the right tradeoff. Default OFF; set to `on` / `true` / `1` / `yes` to enable. |
```

- [ ] **Step 2: Verify the markdown table still renders correctly**

Run:
```bash
grep -n "pm_fast_mode\|pm_custom_instructions\|pm_enabled" README.md
```
Expected: three consecutive table rows, with `pm_fast_mode` appearing right after `pm_custom_instructions`.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document pm_fast_mode setting"
```

---

## Verification

After all tasks land, do an end-to-end smoke check:

- [ ] **Build:** `cargo build` — expect a clean build.
- [ ] **Tests:** `cargo test -p wsx --lib` — expect all green.
- [ ] **Clippy:** `cargo clippy --all-targets -- -D warnings` — expect no new warnings.
- [ ] **Formatting:** `cargo fmt --check` — expect clean.
- [ ] **Manual:** in a scratch wsx install: `wsx setting set pm_fast_mode on`, then open the PM pane with `p`. Confirm the PM PTY session indicates fast mode is on (Claude shows this in its prompt area). Then `wsx setting set pm_fast_mode off`, restart wsx, reopen PM, and confirm fast mode is off.
- [ ] **Manual (continue path):** with `pm_fast_mode on`, exit wsx and reopen — PM resumes via `--continue` and fast mode should remain on.
