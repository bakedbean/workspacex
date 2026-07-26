# wsx Waybar Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Linux-only waybar module showing all wsx repos/workspaces with click-to-jump, shipped inside wsx as `wsx waybar status|menu|jump` + `wsx setup waybar`, strictly isolated in `src/waybar/`.

**Architecture:** All waybar logic lives in a new `#[cfg(target_os = "linux")] pub mod waybar` with submodules `status`, `menu`, `jump`, `ipc`, `install`. Core gains exactly two platform-neutral seams (`App::select_workspace_by_name`, the `--select` launch flag) plus one Linux-gated listener spawn in `main.rs`. The TUI has **no event channel** — background tasks lock `SharedApp = Arc<tokio::sync::Mutex<App>>` and mutate directly; the IPC listener follows that pattern.

**Tech Stack:** Rust (edition 2024), tokio (`net` is included via feature "full"), rusqlite store, serde/serde_json, hand-rolled CLI in `src/cli.rs` (no clap). Dev: tempfile. Spec: `docs/superpowers/specs/2026-07-26-waybar-indicator-design.md`.

## Global Constraints

- Isolation: no waybar types/names in core modules; `src/waybar/` depends on core, never the reverse. Non-Linux `wsx waybar`/`wsx setup waybar` must error with exactly: `wsx waybar is only available on Linux (waybar integration)`.
- Whole module gated `#[cfg(target_os = "linux")]`; `CliAction` variants and parsing are UNGATED (help/usage uniform on all platforms); only `run_cli` execution arms are gated in `#[cfg]` pairs. There is no `target_os` usage in the tree today — this introduces it; do not use bare `#[cfg(unix)]` for waybar code.
- `main.rs` is a separate bin crate: anything it calls must be `pub` (not `pub(crate)`).
- CI gates rustfmt, clippy, tests separately: before every commit run `cargo fmt`, and in the final task `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`.
- Commit messages: conventional commits. Never push to main; branch `wsx-waybar-indicator` is already checked out in this worktree.
- House test style: inline `#[cfg(test)] mod <concern>_tests` at file bottom; seed with `Store::open_in_memory()`; env mutation only via `wsx::test_support::EnvGuard`.
- Status vocabulary is `ReportedState { Working, Waiting, Blocked, Done, Busy }` (`src/data/store.rs:38`); `Busy` is hook-internal — map it to `working` everywhere user-visible. Class priority: `blocked > done > waiting > working > idle`.
- IPC wire protocol: one line `select <repo...> <slug>\n` — repo names may contain spaces (e.g. "meals backend"), slugs never do, so the LAST whitespace token is the slug. `repo/slug` strings split on the FIRST `/`.

---

### Task 1: CLI surface + module skeleton

**Files:**
- Modify: `src/lib.rs` (module list, currently flat/alphabetical, lines 1-17)
- Modify: `src/cli.rs` — `GROUPS` (~line 16-217), `CliAction` enum (~line 296), `parse_args` dispatch (~line 528-545), new `parse_waybar`, `parse_setup` (~line 1048), `run_cli` (~line 1167), tests incl. `registry_matches_dispatched_groups` (~line 2663)
- Create: `src/waybar/mod.rs`, `src/waybar/status.rs`, `src/waybar/menu.rs`, `src/waybar/jump.rs`, `src/waybar/ipc.rs`, `src/waybar/install.rs` (stubs)

**Interfaces:**
- Produces: `CliAction::{WaybarStatus, WaybarMenu, WaybarJump { repo: String, slug: String }, SetupWaybar}` (ungated); `pub mod waybar` gated `#[cfg(target_os = "linux")]`; group help for `waybar`; setup group gains `waybar` command. Later tasks fill the stub arms marked `todo` below.

- [ ] **Step 1: Write failing parse tests** in `src/cli.rs`'s existing `mod tests` (helper `fn parse(args: &[&str]) -> Result<CliAction>` exists at ~line 1886):

```rust
#[test]
fn parses_waybar_commands() {
    assert!(matches!(parse(&["waybar", "status"]), Ok(CliAction::WaybarStatus)));
    assert!(matches!(parse(&["waybar", "menu"]), Ok(CliAction::WaybarMenu)));
    match parse(&["waybar", "jump", "meals backend", "api-fix"]) {
        Ok(CliAction::WaybarJump { repo, slug }) => {
            assert_eq!(repo, "meals backend");
            assert_eq!(slug, "api-fix");
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(parse(&["waybar", "jump", "onlyrepo"]).is_err());
    assert!(parse(&["waybar", "bogus"]).is_err());
    assert!(parse(&["waybar"]).is_err());
}

#[test]
fn parses_setup_waybar() {
    assert!(matches!(parse(&["setup", "waybar"]), Ok(CliAction::SetupWaybar)));
}

#[test]
fn waybar_group_help_renders() {
    let h = render_group_help("waybar");
    assert!(h.contains("wsx waybar —"));
    assert!(h.contains("status"));
    assert!(h.contains("jump <repo> <slug>"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib cli::tests::parses_waybar 2>&1 | tail -20`
Expected: compile error (`WaybarStatus` not found) — that counts as the failing state.

- [ ] **Step 3: Implement CLI surface.**

In `GROUPS` (append after the `recap` entry, ~line 200):

```rust
GroupInfo {
    name: "waybar",
    blurb: "Linux waybar status module and workspace jumper",
    commands: &[
        CmdInfo { usage: "status", blurb: "Print waybar JSON for the custom module" },
        CmdInfo { usage: "menu", blurb: "Pick a workspace in a menu and jump to it" },
        CmdInfo { usage: "jump <repo> <slug>", blurb: "Select the workspace in a running TUI, or launch one" },
    ],
},
```

Add to the `setup` group's `commands` (~line 174):

```rust
CmdInfo { usage: "waybar", blurb: "Install the waybar module into ~/.config/waybar" },
```

`CliAction` variants (near `SetupInstallSkill`, ~line 401):

```rust
SetupWaybar,
WaybarStatus,
WaybarMenu,
WaybarJump { repo: String, slug: String },
```

Dispatch arm in `parse_args` (~line 530 match): `"waybar" => parse_waybar(&mut it).map_err(|e| tag_group(e, group)),`

New parser (next to `parse_setup`):

```rust
fn parse_waybar(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("status") => Ok(CliAction::WaybarStatus),
        Some("menu") => Ok(CliAction::WaybarMenu),
        Some("jump") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage { group: None, msg: "jump needs <repo> <slug>".into() });
            };
            Ok(CliAction::WaybarJump { repo, slug })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown waybar command: {cmd}"),
                None => "missing waybar command".into(),
            },
        }),
    }
}
```

In `parse_setup` add `Some("waybar") => Ok(CliAction::SetupWaybar),`.

Update the `dispatched` array in `registry_matches_dispatched_groups` (~line 2663) to include `"waybar"`.

In `run_cli`: add gated arms. `WaybarStatus` and `SetupWaybar` go in the PRE-STORE section (next to the `SetupInstallSkill` block at ~line 1185 — `WaybarStatus` must control its own store-open error handling; `SetupWaybar` doesn't need the store):

```rust
if matches!(action, CliAction::WaybarStatus) {
    #[cfg(target_os = "linux")]
    {
        crate::waybar::status::print_status(&dirs.db_path());
        return Ok(());
    }
    #[cfg(not(target_os = "linux"))]
    return Err(waybar_linux_only());
}
if matches!(action, CliAction::SetupWaybar) {
    #[cfg(target_os = "linux")]
    {
        for line in crate::waybar::install::run()? {
            println!("{line}");
        }
        return Ok(());
    }
    #[cfg(not(target_os = "linux"))]
    return Err(waybar_linux_only());
}
```

In the MAIN match (post-store):

```rust
#[cfg(target_os = "linux")]
CliAction::WaybarMenu => crate::waybar::menu::run_menu(&store)?,
#[cfg(target_os = "linux")]
CliAction::WaybarJump { repo, slug } => crate::waybar::jump::jump(&repo, &slug)?,
#[cfg(not(target_os = "linux"))]
CliAction::WaybarMenu | CliAction::WaybarJump { .. } => return Err(waybar_linux_only()),
```

And add `CliAction::WaybarStatus | CliAction::SetupWaybar => unreachable!("handled before store open"),` to the trailing unreachable arm (join the existing one).

Helper at the bottom of cli.rs:

```rust
#[cfg(not(target_os = "linux"))]
fn waybar_linux_only() -> Error {
    Error::UserInput("wsx waybar is only available on Linux (waybar integration)".into())
}
```

`src/lib.rs`: add (alphabetical position, after `ui`):

```rust
#[cfg(target_os = "linux")]
pub mod waybar;
```

`src/waybar/mod.rs`:

```rust
pub mod install;
pub mod ipc;
pub mod jump;
pub mod menu;
pub mod status;
```

Stub bodies so Task 1 compiles (replaced by later tasks):

```rust
// status.rs
pub fn print_status(_db_path: &std::path::Path) { println!(r#"{{"text":""}}"#); }
// menu.rs
pub fn run_menu(_store: &crate::data::store::Store) -> crate::error::Result<()> { Ok(()) }
// jump.rs
pub fn jump(_repo: &str, _slug: &str) -> crate::error::Result<()> { Ok(()) }
// ipc.rs — empty for now
// install.rs
pub fn run() -> crate::error::Result<Vec<String>> { Ok(vec![]) }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib cli:: 2>&1 | tail -5`
Expected: PASS, including `registry_matches_dispatched_groups`, `root_help` test (it derives from GROUPS so it passes automatically), and the three new tests.

- [ ] **Step 5: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(waybar): add waybar CLI group and Linux-gated module skeleton"
```

---

### Task 2: `wsx waybar status` payload

**Files:**
- Modify: `src/waybar/status.rs` (replace stub)
- Test: inline `mod status_tests` in same file

**Interfaces:**
- Consumes: `Store::open(&Path)` (`src/data/store.rs:160`), `crate::data::repo::list(&Store) -> Result<Vec<Repo>>` (`src/data/repo.rs:15`), `Store::workspaces(RepoId)` (`store.rs:275`), `Store::all_workspace_status() -> Result<HashMap<WorkspaceId, ReportedStatus>>` (`src/data/status.rs:47`), `ReportedState` (`store.rs:38`).
- Produces: `pub struct StatusPayload { text, class, tooltip }` (Serialize), `pub fn status_payload(store: &Store) -> Result<StatusPayload>`, `pub fn print_status(db_path: &Path)` (never fails: any error → `{"text":""}`), `pub(crate) fn escape_pango(&str) -> String`.

- [ ] **Step 1: Write failing tests** (bottom of `status.rs`):

```rust
#[cfg(test)]
mod status_tests {
    use super::*;
    use crate::data::store::{NewWorkspace, ReportedState, Store, WorkspaceState};
    use crate::data::AgentKind; // adjust path to wherever AgentKind lives (used by NewWorkspace)

    fn seed() -> Store {
        let store = Store::open_in_memory().unwrap();
        let r1 = store.add_repo(std::path::Path::new("/tmp/alpha"), "alpha", "feat").unwrap();
        let r2 = store.add_repo(std::path::Path::new("/tmp/empty"), "empty", "feat").unwrap();
        let _ = r2;
        for name in ["one", "two"] {
            let id = store
                .insert_workspace(&NewWorkspace {
                    repo_id: r1,
                    name: name.into(),
                    branch: format!("feat/{name}"),
                    worktree_path: format!("/tmp/wt-{name}").into(),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap();
            store.set_workspace_state(id, WorkspaceState::Ready).unwrap();
        }
        store
    }
    // NOTE: copy the exact seeding calls from src/commands/shared.rs `fn seed` (~line 146)
    // if the signatures above drift — that test is the canonical model.

    #[test]
    fn counts_workspaces_and_defaults_to_idle() {
        let store = seed();
        let p = status_payload(&store).unwrap();
        assert!(p.text.ends_with(" 2"), "text was {:?}", p.text);
        assert_eq!(p.class, "idle");
        assert!(p.tooltip.contains("alpha"));
        assert!(p.tooltip.contains("one"));
        assert!(p.tooltip.contains("empty")); // repos with no workspaces still listed
        assert!(p.tooltip.contains("(no workspaces)"));
    }

    #[test]
    fn class_uses_priority_blocked_over_done_over_waiting_over_working() {
        let store = seed();
        let ws = store.all_workspaces().unwrap();
        store.set_workspace_status(ws[0].id, ReportedState::Working, Some("hacking"), "model").unwrap();
        assert_eq!(status_payload(&store).unwrap().class, "working");
        store.set_workspace_status(ws[1].id, ReportedState::Waiting, None, "model").unwrap();
        assert_eq!(status_payload(&store).unwrap().class, "waiting");
        store.set_workspace_status(ws[0].id, ReportedState::Done, None, "model").unwrap();
        assert_eq!(status_payload(&store).unwrap().class, "done");
        store.set_workspace_status(ws[1].id, ReportedState::Blocked, Some("need input"), "model").unwrap();
        let p = status_payload(&store).unwrap();
        assert_eq!(p.class, "blocked");
        assert!(p.tooltip.contains("need input"));
    }

    #[test]
    fn busy_maps_to_working_and_pango_is_escaped() {
        let store = seed();
        let ws = store.all_workspaces().unwrap();
        store.set_workspace_status(ws[0].id, ReportedState::Busy, Some("a <b> & c"), "hook").unwrap();
        let p = status_payload(&store).unwrap();
        assert_eq!(p.class, "working");
        assert!(p.tooltip.contains("a &lt;b&gt; &amp; c"));
        assert!(!p.tooltip.contains("<b>"));
    }

    #[test]
    fn no_repos_hides_module() {
        let store = Store::open_in_memory().unwrap();
        let p = status_payload(&store).unwrap();
        assert_eq!(p.text, "");
    }

    #[test]
    fn json_shape() {
        let store = seed();
        let json = serde_json::to_string(&status_payload(&store).unwrap()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("text").is_some() && v.get("class").is_some() && v.get("tooltip").is_some());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib waybar::status 2>&1 | tail -20`
Expected: compile error — `status_payload` doesn't exist yet. If the seeding helper signatures don't match the real store API, fix the TEST to match `src/commands/shared.rs`'s seed fn, not the other way round.

- [ ] **Step 3: Implement:**

```rust
use std::path::Path;

use serde::Serialize;

use crate::data::store::{ReportedState, Store};
use crate::error::Result;

#[derive(Serialize, Debug, PartialEq)]
pub struct StatusPayload {
    pub text: String,
    pub class: String,
    pub tooltip: String,
}

fn rank(state: ReportedState) -> u8 {
    match state {
        ReportedState::Blocked => 4,
        ReportedState::Done => 3,
        ReportedState::Waiting => 2,
        ReportedState::Working | ReportedState::Busy => 1,
    }
}

fn class_name(state: ReportedState) -> &'static str {
    match state {
        ReportedState::Blocked => "blocked",
        ReportedState::Done => "done",
        ReportedState::Waiting => "waiting",
        ReportedState::Working | ReportedState::Busy => "working",
    }
}

fn glyph(state: Option<ReportedState>) -> &'static str {
    match state {
        Some(ReportedState::Blocked) => "!",
        Some(ReportedState::Done) => "✓",
        Some(ReportedState::Waiting) => "…",
        Some(ReportedState::Working | ReportedState::Busy) => "↻",
        None => "·",
    }
}

pub(crate) fn escape_pango(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn status_payload(store: &Store) -> Result<StatusPayload> {
    let repos = crate::data::repo::list(store)?;
    if repos.is_empty() {
        return Ok(StatusPayload {
            text: String::new(),
            class: "idle".into(),
            tooltip: String::new(),
        });
    }
    let statuses = store.all_workspace_status()?;
    let mut count = 0usize;
    let mut best: Option<ReportedState> = None;
    let mut lines = Vec::new();
    for repo in &repos {
        lines.push(escape_pango(&repo.name));
        let workspaces = store.workspaces(repo.id)?;
        if workspaces.is_empty() {
            lines.push("  (no workspaces)".into());
        }
        for ws in &workspaces {
            count += 1;
            let st = statuses.get(&ws.id);
            if let Some(st) = st
                && best.map_or(true, |b| rank(st.state) > rank(b))
            {
                best = Some(st.state);
            }
            let mut line = format!("  {} {}", glyph(st.map(|s| s.state)), escape_pango(&ws.name));
            if let Some(msg) = st.and_then(|s| s.message.as_deref()) {
                line.push_str(" — ");
                line.push_str(&escape_pango(msg));
            }
            lines.push(line);
        }
    }
    Ok(StatusPayload {
        text: format!("\u{e725} {count}"), //  nf-dev-git_branch
        class: best.map(class_name).unwrap_or("idle").to_string(),
        tooltip: lines.join("\n"),
    })
}

/// Never fails: waybar runs this every 5s; on any error emit an empty payload
/// so the module hides instead of flashing errors in the bar.
pub fn print_status(db_path: &Path) {
    let json = Store::open(db_path)
        .and_then(|store| status_payload(&store))
        .and_then(|p| Ok(serde_json::to_string(&p)?));
    match json {
        Ok(j) => println!("{j}"),
        Err(_) => println!(r#"{{"text":""}}"#),
    }
}
```

(If `let ... && let`-chain syntax displeases clippy/edition, use a nested `if`.)

- [ ] **Step 4: Run tests**

Run: `cargo test --lib waybar::status 2>&1 | tail -5`
Expected: 5 passed.

- [ ] **Step 5: Sanity-check the real binary**

Run: `cargo run --quiet -- waybar status`
Expected: one JSON line with your real repos in the tooltip; `echo $?` is 0.

- [ ] **Step 6: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(waybar): implement status payload with class priority and pango escaping"
```

---

### Task 3: Core seam — `App::select_workspace_by_name`

**Files:**
- Modify: `src/app.rs` — add method near `select_index` (~line 858); add tests near `mod selection_helper_tests` (~line 2697)

**Interfaces:**
- Consumes: `App` fields `repos: Vec<Repo>` (:422), `workspaces: Vec<(RepoId, Workspace)>` (:423), `selectable: Vec<SelectionTarget>` (:424), `dashboard.folded: HashMap<u64, bool>`, `select_index(idx)` (:858), `SelectionTarget` (:36).
- Produces: `pub fn App::select_workspace_by_name(&mut self, repo_name: &str, slug: &str) -> bool` — MUST be `pub` (called from the bin crate `main.rs` and from `src/waybar/ipc.rs`). Platform-neutral; no waybar naming.

- [ ] **Step 1: Write failing test.** Reuse the existing fixture — `fn app_with_one_workspace()` exists inside a test mod at `src/app.rs:2703`; read it and either reuse (if visible to your new mod) or copy its body. Add a new inline mod near it:

```rust
#[cfg(test)]
mod select_by_name_tests {
    use super::*;

    #[test]
    fn selects_existing_workspace_and_unfolds_repo() {
        // build exactly like app_with_one_workspace() at src/app.rs:2703
        let mut app = /* fixture */;
        let repo_name = app.repos[0].name.clone();
        let ws = app.workspaces[0].1.clone();
        app.dashboard.folded.insert(app.repos[0].id.0 as u64, true); // folded → must unfold
        assert!(app.select_workspace_by_name(&repo_name, &ws.name));
        assert_eq!(app.dashboard.folded.get(&(app.repos[0].id.0 as u64)), Some(&false));
        assert_eq!(app.selected_target(), Some(SelectionTarget::Workspace(ws.id)));
    }

    #[test]
    fn unknown_names_return_false_and_leave_selection_alone() {
        let mut app = /* fixture */;
        let before = app.selected_target();
        assert!(!app.select_workspace_by_name("nope", "nothing"));
        assert!(!app.select_workspace_by_name(&app.repos[0].name.clone(), "nothing"));
        assert_eq!(app.selected_target(), before);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib select_by_name 2>&1 | tail -10`
Expected: compile error (method missing).

- [ ] **Step 3: Implement** (place right after `select_index`, ~line 864). The unfold-before-position sequence is load-bearing — copied from `reconcile_create_result` (`src/app.rs:2071-2087`): a workspace row inside a folded repo is absent from `selectable`, and selection would get parked.

```rust
/// Select a workspace by repo name + workspace slug, unfolding its repo.
/// Returns false if the pair doesn't exist or isn't currently selectable.
/// Used by automation surfaces (waybar jump, `wsx --select`).
pub fn select_workspace_by_name(&mut self, repo_name: &str, slug: &str) -> bool {
    let Some(repo_id) = self.repos.iter().find(|r| r.name == repo_name).map(|r| r.id) else {
        return false;
    };
    let Some(ws_id) = self
        .workspaces
        .iter()
        .find(|(rid, w)| *rid == repo_id && w.name == slug)
        .map(|(_, w)| w.id)
    else {
        return false;
    };
    self.dashboard.folded.insert(repo_id.0 as u64, false);
    match self
        .selectable
        .iter()
        .position(|t| *t == SelectionTarget::Workspace(ws_id))
    {
        Some(idx) => {
            self.select_index(idx);
            true
        }
        None => false,
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib select_by_name 2>&1 | tail -5`
Expected: 2 passed.

- [ ] **Step 5: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(app): add select_workspace_by_name selection seam"
```

---

### Task 4: `--select <repo>/<slug>` launch flag

**Files:**
- Modify: `src/cli.rs` — `CliAction::Tui` (~line 297), `parse_args` (~line 499), the `Tui => unreachable!` arm in `run_cli`, plus every `matches!(action, CliAction::Tui)`/pattern site (grep `CliAction::Tui`)
- Modify: `src/main.rs` — the gate at ~line 71, selection application after `App::new` (~line 92)

**Interfaces:**
- Consumes: `App::select_workspace_by_name` (Task 3).
- Produces: `CliAction::Tui { select: Option<(String, String)> }` — later tasks (jump's fallback spawn) rely on `wsx --select <repo>/<slug>` working from a cold start.

- [ ] **Step 1: Write failing parse tests** in cli.rs `mod tests`:

```rust
#[test]
fn parses_select_launch_flag() {
    match parse(&["--select", "meals backend/api-fix"]) {
        Ok(CliAction::Tui { select: Some((repo, slug)) }) => {
            assert_eq!(repo, "meals backend");
            assert_eq!(slug, "api-fix");
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(matches!(parse(&[]), Ok(CliAction::Tui { select: None })));
    assert!(parse(&["--select"]).is_err());
    assert!(parse(&["--select", "no-slash"]).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib parses_select_launch_flag 2>&1 | tail -10`
Expected: compile error (`Tui` is a unit variant).

- [ ] **Step 3: Implement.** Change the variant to `Tui { select: Option<(String, String)> }`. In `parse_args`, the bare-launch return (~line 499-500) becomes `Ok(CliAction::Tui { select: None })`, and add a first-token arm:

```rust
Some("--select") => {
    let Some(target) = rest.get(1).cloned() else {
        return Err(Error::Usage { group: None, msg: "--select needs <repo>/<slug>".into() });
    };
    let Some((repo, slug)) = target.split_once('/') else {
        return Err(Error::Usage { group: None, msg: "--select target must be <repo>/<slug>".into() });
    };
    return Ok(CliAction::Tui { select: Some((repo.to_string(), slug.to_string())) });
}
```

(Adapt to `parse_args`' actual first/rest handling — mirror how `help` peeks `rest.first()` at ~line 504.) Fix all other `CliAction::Tui` pattern sites to `CliAction::Tui { .. }`.

In `main.rs`, replace the gate (~line 71):

```rust
let select = match action {
    cli::CliAction::Tui { select } => select,
    other => {
        cli::run_cli(other, &dirs).await?;
        return Ok(());
    }
};
```

and after the app is constructed (~line 92):

```rust
if let Some((repo, slug)) = &select {
    app.lock().await.select_workspace_by_name(repo, slug);
}
```

(Result deliberately ignored — a bad target just leaves default selection; the TUI is about to render either way.)

- [ ] **Step 4: Run tests + build**

Run: `cargo test --lib cli:: 2>&1 | tail -5 && cargo build 2>&1 | tail -3`
Expected: all pass; bin compiles (proves the `pub` visibility of the seam).

- [ ] **Step 5: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(cli): add --select launch flag to open the TUI on a workspace"
```

---

### Task 5: IPC listener (`src/waybar/ipc.rs` + main.rs wiring)

**Files:**
- Modify: `src/waybar/ipc.rs`
- Modify: `src/main.rs` — spawn next to `branch_drift_poll` (~line 96), unlink right after `app::run` returns (~line 111), BEFORE the `?`-bearing terminal-restore lines (they can early-return)
- Test: inline `mod ipc_tests`

**Interfaces:**
- Consumes: `SharedApp` (`src/app.rs:1152`), `App::refresh()` (:727, needed when the workspace was created after the TUI loaded), `App::select_workspace_by_name` (Task 3).
- Produces (all `pub` — used by main.rs bin crate and by jump.rs):
  - `pub fn socket_dir() -> PathBuf` — `$XDG_RUNTIME_DIR/wsx`, else `dirs::state_dir()/wsx/run`, else `env::temp_dir()/wsx-run`
  - `pub fn socket_path_for(pid: u32) -> PathBuf` — `socket_dir()/tui-<pid>.sock`
  - `pub fn live_socket_candidates() -> Vec<(PathBuf, u32)>` — matching entries sorted newest-mtime-first
  - `pub fn parse_line(line: &str) -> Option<(String, String)>` — `select <repo...> <slug>`; last token = slug, rest joined by single spaces = repo
  - `pub async fn handle_line(app: &SharedApp, line: &str) -> bool`
  - `pub async fn listen(app: SharedApp, path: PathBuf)` — never panics; logs and returns on bind failure

- [ ] **Step 1: Write failing tests:**

```rust
#[cfg(test)]
mod ipc_tests {
    use super::*;

    #[test]
    fn parse_line_handles_spaces_in_repo_names() {
        assert_eq!(
            parse_line("select meals backend api-fix\n"),
            Some(("meals backend".into(), "api-fix".into()))
        );
        assert_eq!(parse_line("select alpha one"), Some(("alpha".into(), "one".into())));
        assert_eq!(parse_line("select onlyslug"), None);
        assert_eq!(parse_line("nonsense alpha one"), None);
        assert_eq!(parse_line(""), None);
    }

    #[test]
    fn socket_path_shape() {
        let p = socket_path_for(4242);
        assert!(p.to_string_lossy().ends_with("tui-4242.sock"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn handle_line_selects_workspace() {
        // Build a SharedApp exactly like src/app.rs:2703 app_with_one_workspace()
        // (in-memory store + App::new), wrapped in Arc<tokio::sync::Mutex<_>>.
        let app: crate::app::SharedApp = /* fixture */;
        let (repo, slug) = {
            let g = app.lock().await;
            (g.repos[0].name.clone(), g.workspaces[0].1.name.clone())
        };
        assert!(handle_line(&app, &format!("select {repo} {slug}")).await);
        assert!(!handle_line(&app, "select nope nothing").await);
        assert!(!handle_line(&app, "garbage").await);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib waybar::ipc 2>&1 | tail -10`
Expected: compile error.

- [ ] **Step 3: Implement:**

```rust
use std::path::PathBuf;

use tokio::io::AsyncBufReadExt;

use crate::app::SharedApp;

pub fn socket_dir() -> PathBuf {
    if let Some(rt) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(rt).join("wsx");
    }
    dirs::state_dir()
        .map(|d| d.join("wsx/run"))
        .unwrap_or_else(|| std::env::temp_dir().join("wsx-run"))
}

pub fn socket_path_for(pid: u32) -> PathBuf {
    socket_dir().join(format!("tui-{pid}.sock"))
}

pub fn live_socket_candidates() -> Vec<(PathBuf, u32)> {
    let mut found: Vec<(PathBuf, u32, std::time::SystemTime)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(socket_dir()) {
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(pid) = name
                .strip_prefix("tui-")
                .and_then(|s| s.strip_suffix(".sock"))
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            found.push((entry.path(), pid, mtime));
        }
    }
    found.sort_by(|a, b| b.2.cmp(&a.2));
    found.into_iter().map(|(p, pid, _)| (p, pid)).collect()
}

/// Wire protocol: `select <repo...> <slug>` — repo names may contain spaces,
/// slugs never do, so the last token is the slug.
pub fn parse_line(line: &str) -> Option<(String, String)> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "select" {
        return None;
    }
    let rest: Vec<&str> = tokens.collect();
    let (slug, repo_parts) = rest.split_last()?;
    if repo_parts.is_empty() {
        return None;
    }
    Some((repo_parts.join(" "), (*slug).to_string()))
}

pub async fn handle_line(app: &SharedApp, line: &str) -> bool {
    let Some((repo, slug)) = parse_line(line) else {
        return false;
    };
    let mut g = app.lock().await;
    // The workspace may have been created after the TUI last refreshed.
    let _ = g.refresh();
    g.select_workspace_by_name(&repo, &slug)
}

pub async fn listen(app: SharedApp, path: PathBuf) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::remove_file(&path);
    let listener = match tokio::net::UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("waybar ipc: bind {} failed: {e}", path.display());
            return;
        }
    };
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let ok = handle_line(&app, &line).await;
                tracing::debug!("waybar ipc: {line:?} -> {ok}");
            }
        });
    }
}
```

`main.rs` wiring (TUI path only). Next to the `branch_drift_poll` spawn (~line 96):

```rust
#[cfg(target_os = "linux")]
let ipc_socket = {
    let path = wsx::waybar::ipc::socket_path_for(std::process::id());
    tokio::spawn(wsx::waybar::ipc::listen(app.clone(), path.clone()));
    path
};
```

Immediately after `let result = app::run(&mut terminal, app.clone()).await;` (~line 111), before any `?`:

```rust
#[cfg(target_os = "linux")]
let _ = std::fs::remove_file(&ipc_socket);
```

(Match main.rs's actual import style — it may `use wsx::...` or fully qualify; keep consistent. Stale sockets from a panic/kill are tolerated: jump unlinks any socket that refuses connections.)

- [ ] **Step 4: Run tests**

Run: `cargo test --lib waybar::ipc 2>&1 | tail -5`
Expected: 3 passed.

- [ ] **Step 5: Manual smoke of the socket (this machine runs Hyprland/Linux):**

```bash
cargo build && ./target/debug/wsx &  # in a real terminal, or use an existing wsx
ls "$XDG_RUNTIME_DIR/wsx/"          # expect tui-<pid>.sock
printf 'select workspacex wsx-waybar-indicator\n' | socat - UNIX-CONNECT:"$XDG_RUNTIME_DIR/wsx/tui-<pid>.sock"
```
Expected: the running TUI's selection moves to this workspace. (If `socat` is missing, defer to Task 6's jump test.)

- [ ] **Step 6: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(waybar): add TUI unix-socket listener for workspace selection"
```

---

### Task 6: Jump (`src/waybar/jump.rs`)

**Files:**
- Modify: `src/waybar/jump.rs` (replace stub)
- Test: inline `mod jump_tests`

**Interfaces:**
- Consumes: `ipc::live_socket_candidates()`, `ipc::socket_path_for` (Task 5); `libc::setsid` (already a `cfg(unix)` dependency); detach precedent at `src/commands/external.rs:262`.
- Produces: `pub fn jump(repo: &str, slug: &str) -> Result<()>`; internal pure fns `fn client_pid_for_chain(clients_json: &str, chain: &[u32]) -> Option<u32>` and `fn ancestor_pids(pid: u32) -> Vec<u32>` (reads `/proc/<pid>/stat`; parse ppid as the token after the LAST `)` — comm can contain spaces/parens — field 4).

- [ ] **Step 1: Write failing tests** (pure parts only — hyprctl/terminal spawning is not unit-tested):

```rust
#[cfg(test)]
mod jump_tests {
    use super::*;

    #[test]
    fn client_pid_prefers_closest_ancestor() {
        let clients = r#"[
            {"address":"0x1","pid":900,"class":"Alacritty"},
            {"address":"0x2","pid":300,"class":"ghostty"}
        ]"#;
        // chain is ordered self → parent → grandparent
        assert_eq!(client_pid_for_chain(clients, &[100, 300, 900]), Some(300));
        assert_eq!(client_pid_for_chain(clients, &[100, 200]), None);
        assert_eq!(client_pid_for_chain("not json", &[100]), None);
        assert_eq!(client_pid_for_chain("[]", &[100]), None);
    }

    #[test]
    fn ancestor_pids_walks_proc() {
        let chain = ancestor_pids(std::process::id());
        assert_eq!(chain.first(), Some(&std::process::id()));
        assert!(chain.len() >= 2, "expected at least self + parent, got {chain:?}");
        assert!(chain.len() <= 32);
    }

    #[test]
    fn stat_ppid_parses_despite_parens_in_comm() {
        assert_eq!(ppid_from_stat("123 (weird) name) S 77 123 123 0"), Some(77));
        assert_eq!(ppid_from_stat("123 (simple) S 1 123"), Some(1));
        assert_eq!(ppid_from_stat("garbage"), None);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib waybar::jump 2>&1 | tail -10`
Expected: compile error.

- [ ] **Step 3: Implement:**

```rust
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Jump to a workspace: tell a running TUI to select it and focus that
/// window, or launch a fresh TUI on it.
pub fn jump(repo: &str, slug: &str) -> Result<()> {
    for (path, pid) in crate::waybar::ipc::live_socket_candidates() {
        match std::os::unix::net::UnixStream::connect(&path) {
            Ok(mut stream) => {
                if writeln!(stream, "select {repo} {slug}").is_ok() {
                    focus_window_of(pid);
                    return Ok(());
                }
            }
            Err(_) => {
                // Stale socket from a killed TUI.
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    spawn_tui(repo, slug)
}

fn ppid_from_stat(stat: &str) -> Option<u32> {
    // comm is parenthesized and may itself contain ')' — split on the LAST ')'.
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().nth(1)?.parse().ok() // state, ppid, ...
}

fn ancestor_pids(pid: u32) -> Vec<u32> {
    let mut chain = vec![pid];
    let mut current = pid;
    while chain.len() < 32 {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{current}/stat")) else {
            break;
        };
        let Some(ppid) = ppid_from_stat(&stat) else { break };
        if ppid <= 1 {
            break;
        }
        chain.push(ppid);
        current = ppid;
    }
    chain
}

fn client_pid_for_chain(clients_json: &str, chain: &[u32]) -> Option<u32> {
    let v: serde_json::Value = serde_json::from_str(clients_json).ok()?;
    let clients = v.as_array()?;
    // chain is self→ancestors; the first chain pid with a window wins.
    chain.iter().copied().find(|pid| {
        clients
            .iter()
            .any(|c| c.get("pid").and_then(|p| p.as_u64()) == Some(u64::from(*pid)))
    })
}

fn focus_window_of(tui_pid: u32) {
    let Ok(out) = Command::new("hyprctl").args(["clients", "-j"]).output() else {
        return; // not Hyprland — selection still happened
    };
    let chain = ancestor_pids(tui_pid);
    if let Some(pid) = client_pid_for_chain(&String::from_utf8_lossy(&out.stdout), &chain) {
        let _ = Command::new("hyprctl")
            .args(["dispatch", "focuswindow", &format!("pid:{pid}")])
            .status();
    }
}

fn spawn_tui(repo: &str, slug: &str) -> Result<()> {
    let term = std::env::var("TERMINAL").unwrap_or_else(|_| "alacritty".into());
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("wsx"));
    let mut cmd = Command::new(&term);
    cmd.arg("-e")
        .arg(exe)
        .arg("--select")
        .arg(format!("{repo}/{slug}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // Detach into its own session so it outlives the menu/jump process
    // (same pattern as src/commands/external.rs:262).
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(&mut cmd, || {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn()
        .map_err(|e| Error::UserInput(format!("failed to launch terminal '{term}': {e}")))?;
    Ok(())
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib waybar::jump 2>&1 | tail -5`
Expected: 3 passed.

- [ ] **Step 5: Manual smoke:** with a wsx TUI running in another window: `cargo run --quiet -- waybar jump workspacex wsx-waybar-indicator` → TUI selects it and its window gets focus. Then quit the TUI and re-run → a new terminal opens with wsx focused on the workspace.

- [ ] **Step 6: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(waybar): implement jump via TUI socket, hyprctl focus, and terminal fallback"
```

---

### Task 7: Menu (`src/waybar/menu.rs`)

**Files:**
- Modify: `src/waybar/menu.rs` (replace stub)
- Test: inline `mod menu_tests`

**Interfaces:**
- Consumes: `crate::data::repo::list`, `Store::workspaces`, `Store::all_workspace_status`, `jump::jump` (Task 6), `shlex::split` (existing dep).
- Produces: `pub fn run_menu(store: &Store) -> Result<()>` (called by cli.rs arm from Task 1); pure `pub fn menu_line(repo, slug, message: Option<&str>) -> String`, `pub fn parse_menu_line(&str) -> Option<(String, String)>`, `pub fn menu_command() -> Vec<String>`.

- [ ] **Step 1: Write failing tests:**

```rust
#[cfg(test)]
mod menu_tests {
    use super::*;

    #[test]
    fn menu_line_round_trips() {
        for (repo, slug, msg) in [
            ("alpha", "one", Some("fixing the bug")),
            ("meals backend", "api-fix", Some("has — a dash")),
            ("alpha", "two", None),
        ] {
            let line = menu_line(repo, slug, msg);
            assert_eq!(
                parse_menu_line(&line),
                Some((repo.to_string(), slug.to_string())),
                "line was {line:?}"
            );
        }
        assert_eq!(parse_menu_line(""), None);
        assert_eq!(parse_menu_line("noslash — msg"), None);
    }

    #[test]
    fn menu_command_env_override() {
        let mut env = crate::test_support::EnvGuard::new();
        env.set("WSX_WAYBAR_MENU", "wofi --dmenu -p pick");
        assert_eq!(menu_command(), vec!["wofi", "--dmenu", "-p", "pick"]);
        env.remove("WSX_WAYBAR_MENU");
        assert_eq!(menu_command(), vec!["walker", "--dmenu"]);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib waybar::menu 2>&1 | tail -10`
Expected: compile error.

- [ ] **Step 3: Implement:**

```rust
use std::io::Write;
use std::process::{Command, Stdio};

use crate::data::store::Store;
use crate::error::{Error, Result};

pub fn menu_line(repo: &str, slug: &str, message: Option<&str>) -> String {
    match message {
        Some(m) => format!("{repo}/{slug} — {m}"),
        None => format!("{repo}/{slug}"),
    }
}

/// Inverse of menu_line: everything before the first " — " is `repo/slug`,
/// split on the FIRST '/' (slugs are kebab-case, never contain '/').
pub fn parse_menu_line(line: &str) -> Option<(String, String)> {
    let target = line.split(" — ").next().unwrap_or(line).trim();
    let (repo, slug) = target.split_once('/')?;
    if repo.is_empty() || slug.is_empty() {
        return None;
    }
    Some((repo.to_string(), slug.to_string()))
}

pub fn menu_command() -> Vec<String> {
    std::env::var("WSX_WAYBAR_MENU")
        .ok()
        .and_then(|v| shlex::split(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["walker".into(), "--dmenu".into()])
}

fn notify(message: &str) {
    let _ = Command::new("notify-send").args(["wsx", message]).status();
    eprintln!("wsx: {message}");
}

pub fn run_menu(store: &Store) -> Result<()> {
    let statuses = store.all_workspace_status()?;
    let mut lines = Vec::new();
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    for repo in &repos {
        for ws in store.workspaces(repo.id)? {
            let message = statuses.get(&ws.id).and_then(|s| s.message.as_deref().map(str::to_string));
            lines.push(menu_line(&repo.name, &ws.name, message.as_deref()));
        }
    }
    if lines.is_empty() {
        notify("no workspaces");
        return Ok(());
    }
    let cmd = menu_command();
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| {
            notify(&format!("menu command '{}' failed to start", cmd[0]));
            Error::UserInput(format!("failed to launch menu '{}': {e}", cmd[0]))
        })?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(lines.join("\n").as_bytes())?;
    let out = child.wait_with_output()?;
    let selection = String::from_utf8_lossy(&out.stdout);
    let selection = selection.trim();
    if selection.is_empty() {
        return Ok(()); // dismissed
    }
    if let Some((repo, slug)) = parse_menu_line(selection) {
        crate::waybar::jump::jump(&repo, &slug)?;
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib waybar::menu 2>&1 | tail -5`
Expected: 2 passed.

- [ ] **Step 5: Manual smoke:** `cargo run --quiet -- waybar menu` → walker opens with your real workspaces; picking one jumps (TUI running or not); Escape exits silently.

- [ ] **Step 6: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(waybar): implement walker-based workspace menu"
```

---

### Task 8: Installer (`src/waybar/install.rs` + assets + `wsx setup waybar`)

**Files:**
- Create: `src/waybar/assets/wsx.jsonc`, `src/waybar/assets/wsx.css`
- Modify: `src/waybar/install.rs` (replace stub)
- Test: inline `mod install_tests` (string fixtures; TempDir for file ops)

**Interfaces:**
- Consumes: atomic-write pattern from `src/agent/mcp.rs:187` (`write to <name>.wsx-tmp.<pid> then fs::rename`); Task 1's cli arm calls `install::run()`.
- Produces: `pub fn run() -> Result<Vec<String>>` (report lines for cli to print); testable core `pub fn install_into(waybar_dir: &Path, epoch: u64) -> Result<Vec<String>>`; pure `pub fn patch_config(text: &str, include_path: &str) -> PatchOutcome` with `pub enum PatchOutcome { Patched(String), AlreadyInstalled, Unrecognized }`.

- [ ] **Step 1: Create assets.** `src/waybar/assets/wsx.jsonc`:

```jsonc
{
  "custom/wsx": {
    "exec": "wsx waybar status",
    "return-type": "json",
    "interval": 5,
    "on-click": "wsx waybar menu",
    "tooltip": true
  }
}
```

`src/waybar/assets/wsx.css`:

```css
/* wsx waybar module — status classes. Tune to your theme. */
#custom-wsx {
  padding: 0 10px;
}
#custom-wsx.blocked {
  color: #f38ba8;
}
#custom-wsx.done {
  color: #89b4fa;
}
#custom-wsx.waiting {
  color: #f9e2af;
}
#custom-wsx.working {
  color: #a6e3a1;
}
```

- [ ] **Step 2: Write failing tests:**

```rust
#[cfg(test)]
mod install_tests {
    use super::*;

    // Mirrors the user-facing omarchy layout: modules-left with custom/omarchy
    // plus a module-definition key later that must NOT be matched.
    const OMARCHY_STYLE: &str = r#"{
  "reload_style_on_change": true,
  "modules-left": [
    "custom/omarchy",
    "hyprland/workspaces#main",
  ],
  "custom/omarchy": {
    "format": "x"
  }
}
"#;

    #[test]
    fn patches_after_custom_omarchy_array_entry_not_module_def() {
        let PatchOutcome::Patched(out) = patch_config(OMARCHY_STYLE, "/home/u/.config/waybar/wsx.jsonc") else {
            panic!("expected Patched");
        };
        let omarchy_entry = out.find("\"custom/omarchy\",").unwrap();
        let wsx_entry = out.find("\"custom/wsx\",").unwrap();
        assert!(wsx_entry > omarchy_entry);
        assert!(wsx_entry < out.find("hyprland/workspaces").unwrap());
        assert!(out.contains(r#""include": ["/home/u/.config/waybar/wsx.jsonc"],"#));
    }

    #[test]
    fn patches_plain_modules_left_without_omarchy() {
        let cfg = "{\n  \"modules-left\": [\n    \"clock\",\n  ],\n}\n";
        let PatchOutcome::Patched(out) = patch_config(cfg, "/x/wsx.jsonc") else {
            panic!("expected Patched");
        };
        let wsx = out.find("custom/wsx").unwrap();
        assert!(wsx < out.find("clock").unwrap());
    }

    #[test]
    fn already_installed_and_unrecognized() {
        let done = OMARCHY_STYLE.replace("\"custom/omarchy\",", "\"custom/omarchy\",\n    \"custom/wsx\",");
        assert!(matches!(patch_config(&done, "/x"), PatchOutcome::AlreadyInstalled));
        assert!(matches!(patch_config("not even close", "/x"), PatchOutcome::Unrecognized));
        // existing include array → bail to snippets rather than risk a bad edit
        let with_include = OMARCHY_STYLE.replacen('{', "{\n  \"include\": [\"other.jsonc\"],", 1);
        assert!(matches!(patch_config(&with_include, "/x"), PatchOutcome::Unrecognized));
    }

    #[test]
    fn install_into_writes_files_backs_up_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.jsonc"), OMARCHY_STYLE).unwrap();
        let report = install_into(dir.path(), 1234).unwrap();
        assert!(dir.path().join("wsx.jsonc").exists());
        assert!(dir.path().join("wsx.css").exists());
        assert!(dir.path().join("config.jsonc.bak.1234").exists());
        let cfg = std::fs::read_to_string(dir.path().join("config.jsonc")).unwrap();
        assert!(cfg.contains("custom/wsx"));
        assert!(report.iter().any(|l| l.contains("patched")));
        // second run: no new backup, reports already-installed
        let report2 = install_into(dir.path(), 5678).unwrap();
        assert!(!dir.path().join("config.jsonc.bak.5678").exists());
        assert!(report2.iter().any(|l| l.contains("already")));
        // no temp litter
        assert!(!std::fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("wsx-tmp")));
    }

    #[test]
    fn missing_config_prints_snippets() {
        let dir = tempfile::tempdir().unwrap();
        let report = install_into(dir.path(), 1).unwrap();
        assert!(dir.path().join("wsx.jsonc").exists());
        assert!(report.iter().any(|l| l.contains("custom/wsx")), "snippet with module name expected");
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib waybar::install 2>&1 | tail -10`
Expected: compile error.

- [ ] **Step 4: Implement:**

```rust
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const MODULE_JSONC: &str = include_str!("assets/wsx.jsonc");
const MODULE_CSS: &str = include_str!("assets/wsx.css");

pub enum PatchOutcome {
    Patched(String),
    AlreadyInstalled,
    Unrecognized,
}

fn leading_ws(line: &str) -> String {
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

pub fn patch_config(text: &str, include_path: &str) -> PatchOutcome {
    if text.contains("custom/wsx") {
        return PatchOutcome::AlreadyInstalled;
    }
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();

    // 1. include: only handle the no-include case; an existing include array
    //    is left alone (snippets instead) rather than risking a bad edit.
    if lines.iter().any(|l| l.trim_start().starts_with("\"include\"")) {
        return PatchOutcome::Unrecognized;
    }
    let Some(open) = lines.iter().position(|l| l.trim() == "{") else {
        return PatchOutcome::Unrecognized;
    };
    lines.insert(open + 1, format!("  \"include\": [\"{include_path}\"],"));

    // 2. module entry: prefer right after the "custom/omarchy" ARRAY ENTRY
    //    (exact trimmed match with trailing comma — the module-definition key
    //    "custom/omarchy": { must not match), else top of modules-left,
    //    else modules-right.
    let entry_idx = lines
        .iter()
        .position(|l| l.trim() == "\"custom/omarchy\",");
    let placed = if let Some(i) = entry_idx {
        let indent = leading_ws(&lines[i]);
        lines.insert(i + 1, format!("{indent}\"custom/wsx\","));
        true
    } else {
        ["\"modules-left\"", "\"modules-right\""].iter().any(|key| {
            if let Some(i) = lines
                .iter()
                .position(|l| l.trim_start().starts_with(key) && l.contains('['))
            {
                let indent = format!("{}  ", leading_ws(&lines[i]));
                lines.insert(i + 1, format!("{indent}\"custom/wsx\","));
                true
            } else {
                false
            }
        })
    };
    if !placed {
        return PatchOutcome::Unrecognized;
    }
    PatchOutcome::Patched(lines.join("\n") + "\n")
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_file_name(format!(
        "{}.wsx-tmp.{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::Io(e)
    })
}

fn snippet_report(include_path: &str) -> Vec<String> {
    vec![
        "could not patch config.jsonc automatically — add manually:".into(),
        format!("  1. top-level: \"include\": [\"{include_path}\"],"),
        "  2. into modules-left (or -right): \"custom/wsx\",".into(),
    ]
}

pub fn install_into(waybar_dir: &Path, epoch: u64) -> Result<Vec<String>> {
    std::fs::create_dir_all(waybar_dir)?;
    let module_path = waybar_dir.join("wsx.jsonc");
    write_atomic(&module_path, MODULE_JSONC)?;
    write_atomic(&waybar_dir.join("wsx.css"), MODULE_CSS)?;
    let mut report = vec![
        format!("wrote {}", module_path.display()),
        format!("wrote {}", waybar_dir.join("wsx.css").display()),
    ];
    let include_path = module_path.display().to_string();
    let config = waybar_dir.join("config.jsonc");
    match std::fs::read_to_string(&config) {
        Ok(text) => match patch_config(&text, &include_path) {
            PatchOutcome::Patched(new_text) => {
                let backup = waybar_dir.join(format!("config.jsonc.bak.{epoch}"));
                std::fs::copy(&config, &backup)?;
                write_atomic(&config, &new_text)?;
                report.push(format!("patched {} (backup: {})", config.display(), backup.display()));
            }
            PatchOutcome::AlreadyInstalled => {
                report.push("config.jsonc already references custom/wsx".into());
            }
            PatchOutcome::Unrecognized => report.extend(snippet_report(&include_path)),
        },
        Err(_) => report.extend(snippet_report(&include_path)),
    }
    report.push("add to style.css (after existing @import lines): @import \"wsx.css\";".into());
    report.push("reload waybar: omarchy-restart-waybar (or pkill -SIGUSR2 waybar)".into());
    Ok(report)
}

pub fn run() -> Result<Vec<String>> {
    let waybar_dir = dirs::config_dir()
        .ok_or_else(|| Error::UserInput("could not resolve ~/.config".into()))?
        .join("waybar");
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    install_into(&waybar_dir, epoch)
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib waybar::install 2>&1 | tail -5`
Expected: 5 passed.

- [ ] **Step 6: fmt + commit**

```bash
cargo fmt
git add -A && git commit -m "feat(waybar): add wsx setup waybar installer with jsonc patcher"
```

---

### Task 9: Docs, full gates, live verification, PR

**Files:**
- Create: `docs/manual-tests/waybar.md`
- Modify: `README.md` (add a short "Waybar indicator (Linux)" section near the other feature/CLI docs — read the README first and match its tone/placement)

**Interfaces:** none new.

- [ ] **Step 1: Write `docs/manual-tests/waybar.md`:**

```markdown
# Manual test: waybar indicator

Prereqs: omarchy/Hyprland, waybar running, walker installed.

1. `wsx setup waybar` — reports written files + patched config (backup path shown).
2. Add `@import "wsx.css";` to ~/.config/waybar/style.css if not present; run
   `omarchy-restart-waybar`.
3. Bar shows ` N` (N = workspace count). Hover: tooltip lists every repo,
   workspaces beneath, glyphs and status messages.
4. `wsx status set blocked --message "x"` in some workspace → within 5s the
   module class turns blocked (color change). `wsx status clear` reverts.
5. Left-click → walker opens listing `repo/slug — message` lines. Escape: nothing.
6. With a wsx TUI running: pick an entry → TUI window focused, workspace selected
   (repo unfolds if folded).
7. Quit all wsx TUIs; pick an entry → new terminal opens running wsx with that
   workspace selected.
8. Kill a TUI with SIGKILL (stale socket) → click-jump still works (falls back,
   stale socket removed).
9. `wsx setup waybar` again → "already" messages, no second backup.
```

- [ ] **Step 2: README section** — after reading the existing README, add ~10 lines: what the module shows, `wsx setup waybar`, the three `wsx waybar` commands, `WSX_WAYBAR_MENU` override, Linux-only note.

- [ ] **Step 3: Run ALL CI gates**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all clean. (Known flaky: `click_chip_auto_spawns_session_when_missing` — PTY timing; rerun once before investigating.)

- [ ] **Step 4: Live verification** — execute `docs/manual-tests/waybar.md` on this machine top to bottom (install a `cargo install --path .` or use `target/debug/wsx` on PATH as appropriate — check how this machine's `wsx` is installed first with `which wsx`). Fix anything that fails before proceeding.

- [ ] **Step 5: Commit docs**

```bash
git add -A && git commit -m "docs(waybar): add README section and manual test checklist"
```

- [ ] **Step 6: Open PR** against `main` (never push to main) via the pull-request skill; PR body: summary, spec/plan links, manual-verification results, and the session link footer.
