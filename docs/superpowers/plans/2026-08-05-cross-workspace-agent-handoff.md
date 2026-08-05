# Cross-Workspace Agent Handoff Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let an agent create a wsx workspace and hand the task to *that workspace's own agent* with a written brief, instead of `cd`-ing into the new worktree and continuing to drive it from the originating session.

**Architecture:** The message-delivery path is already workspace-agnostic — `Store::undelivered_messages` has no workspace filter and `App::drain_agent_messages` cold-spawns the target session. Three small CLI/data changes make that path *addressable* (`--workspace`, a `primary` label alias, an origin-qualified sender banner), one adds a safety warning, and three rewrite the prompt text that currently prescribes the opposite behavior.

**Tech Stack:** Rust 2024 edition, rusqlite (SQLite), ratatui/tokio (TUI), `mdbook` for `docs/book`. No new dependencies.

**Spec:** `docs/superpowers/specs/2026-08-05-cross-workspace-agent-handoff-design.md`

## Global Constraints

- **rustfmt is pinned in CI to 1.95.0.** Ambient `cargo fmt` uses a different rustfmt and false-passes. Every commit step must run `mise exec rust@1.95.0 -- cargo fmt --all` before `git add`.
- **Clippy must be clean:** `cargo clippy --all-targets -- -D warnings`.
- **No new dependencies.** `tempfile = "3"` is already a dev-dependency; everything else uses `std`.
- **`primary` is a reserved agent label.** No `AgentKind::display_name()` may ever return `"primary"` (current kinds: `claude`, `pi`, `hermes`, `codex`).
- **Workspace spec format is `<repo>/<slug>`, split on the LAST `/`.** Repo names may contain spaces (e.g. `meals backend`); workspace slugs never contain `/`. This mirrors `tui_ipc::parse_line` (`src/tui_ipc.rs:47`).
- **`agent_messages.workspace_id` keeps its current meaning:** the workspace the message is delivered *to*. No task changes that.
- **Doctrine text is injected into every agent session's system prompt.** Keep clauses tight; push detail into the skill.
- Some `app::input` PTY-timing tests flake under a full `cargo test`. If one fails, re-run it in isolation before treating it as a regression.

---

### Task 1: `primary` label alias

The sender of a handoff cannot run `wsx agent list` against another workspace, so it cannot discover that workspace's agent labels. A reserved `primary` label gives it a name that is always correct for a freshly created workspace (which has exactly one agent, and it is primary).

**Files:**
- Modify: `src/data/agents.rs:186-197` (`Store::resolve_instance_label`)
- Test: `src/data/agents.rs` (`mod store_tests`, same file)

**Interfaces:**
- Consumes: `Store::primary_instance_id(ws) -> Result<Option<AgentInstanceId>>` (already exists, `src/data/agents.rs:208`)
- Produces: `Store::resolve_instance_label(ws, label)` gains the behavior `label == "primary"` → the workspace's primary instance id. Signature unchanged: `fn resolve_instance_label(&self, ws: WorkspaceId, label: &str) -> Result<Option<AgentInstanceId>>`

- [ ] **Step 1: Write the failing tests**

Add to `mod store_tests` in `src/data/agents.rs` (it already has a `seed_ws_with_primary` helper that creates repo `r`, workspace `w1`, and a primary `claude`):

```rust
    #[test]
    fn resolve_primary_alias_returns_the_primary_instance() {
        let store = Store::open_in_memory().unwrap();
        let ws = seed_ws_with_primary(&store);
        // A second claude exists so `primary` cannot match by kind alone.
        let second = store.add_workspace_agent(ws, AgentKind::Claude).unwrap();
        let primary = store.primary_instance_id(ws).unwrap().unwrap();
        assert_ne!(primary, second.id);
        assert_eq!(
            store.resolve_instance_label(ws, "primary").unwrap(),
            Some(primary)
        );
        // The alias must not shadow ordinary labels.
        assert_eq!(
            store.resolve_instance_label(ws, "claude").unwrap(),
            Some(primary)
        );
        assert_eq!(
            store.resolve_instance_label(ws, "claude#2").unwrap(),
            Some(second.id)
        );
    }

    #[test]
    fn primary_alias_is_agent_kind_agnostic() {
        // The alias must work when the primary is not a claude — the sender
        // of a handoff does not know the target workspace's agent kind.
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r2"), "r2", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "w2",
                branch: "wsx/w2",
                worktree_path: std::path::Path::new("/tmp/r2/w2"),
                yolo: false,
                agent: AgentKind::Hermes,
                shared: false,
            })
            .unwrap();
        let p = store.add_primary_agent(ws, AgentKind::Hermes, 1).unwrap();
        assert_eq!(
            store.resolve_instance_label(ws, "primary").unwrap(),
            Some(p.id)
        );
        assert_eq!(store.resolve_instance_label(ws, "claude").unwrap(), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib primary_alias -- --nocapture`
Expected: FAIL — `resolve_primary_alias_returns_the_primary_instance` asserts `Some(primary)` but gets `None` (no instance has the label `primary`).

- [ ] **Step 3: Implement the alias**

Replace `Store::resolve_instance_label` in `src/data/agents.rs`:

```rust
    /// Resolve a label like "claude" or "claude#2" to an instance id.
    ///
    /// The reserved label `primary` resolves to the workspace's primary
    /// instance. A caller addressing *another* workspace cannot run
    /// `wsx agent list` against it to discover labels, so `primary` gives it a
    /// name that is always correct for a freshly created workspace (exactly one
    /// agent, and it is primary). No `AgentKind::display_name()` is `primary`,
    /// so the alias cannot shadow a real label.
    pub fn resolve_instance_label(
        &self,
        ws: WorkspaceId,
        label: &str,
    ) -> Result<Option<AgentInstanceId>> {
        if label == "primary" {
            return self.primary_instance_id(ws);
        }
        Ok(self
            .workspace_agents(ws)?
            .into_iter()
            .find(|i| i.label() == label)
            .map(|i| i.id))
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib data::agents`
Expected: PASS — all `store_tests` and `tests` in that module, including the pre-existing `resolve_label_and_primary_id`.

- [ ] **Step 5: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/data/agents.rs
git commit -m "feat(agents): reserve 'primary' as a label alias for a workspace's primary agent"
```

---

### Task 2: cross-workspace targeting for `wsx agent send`

**Files:**
- Modify: `src/cli.rs:63-66` (the `send` `CmdInfo` help entry)
- Modify: `src/cli.rs:485-488` (`CliAction::AgentSend` variant)
- Modify: `src/cli.rs:1109-1123` (`parse_agent`, the `send` arm)
- Modify: `src/cli.rs:1894-1908` (`CliAction::AgentSend` dispatch)
- Modify: `src/cli.rs:2924-2932` (existing `parses_agent_send_joins_prompt` test — it destructures `AgentSend` exhaustively and will stop compiling)
- Create: `resolve_workspace_spec` helper, placed immediately after `lookup_workspace` (`src/cli.rs:2106-2116`)
- Test: `src/cli.rs` (`mod tests`, same file)

**Interfaces:**
- Consumes: `lookup_repo` / `lookup_workspace` patterns (`src/cli.rs:2099`, `:2106`); `crate::data::repo::list(store) -> Result<Vec<Repo>>`; `Store::workspaces(RepoId) -> Result<Vec<Workspace>>`; `Store::resolve_instance_label` with the `primary` alias from Task 1.
- Produces:
  - `CliAction::AgentSend { target: String, prompt: String, workspace: Option<String> }`
  - `fn resolve_workspace_spec(store: &Store, spec: &str) -> Result<Workspace>`
  - `const USAGE_AGENT_SEND: &str`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/cli.rs` (starts at `src/cli.rs:2174`):

```rust
    #[test]
    fn parses_agent_send_with_workspace_flag() {
        match parse(&[
            "agent",
            "send",
            "--workspace",
            "backend/add-widgets",
            "primary",
            "do",
            "the",
            "thing",
        ])
        .unwrap()
        {
            CliAction::AgentSend {
                target,
                prompt,
                workspace,
            } => {
                assert_eq!(target, "primary");
                assert_eq!(prompt, "do the thing");
                assert_eq!(workspace.as_deref(), Some("backend/add-widgets"));
            }
            other => panic!("expected AgentSend, got {other:?}"),
        }
    }

    #[test]
    fn agent_send_flags_are_only_recognised_before_the_label() {
        // Everything from the label onward is body, so a message that itself
        // starts with `--` is preserved verbatim rather than parsed as a flag.
        match parse(&["agent", "send", "claude", "--workspace", "is", "a", "flag"]).unwrap() {
            CliAction::AgentSend {
                target,
                prompt,
                workspace,
            } => {
                assert_eq!(target, "claude");
                assert_eq!(prompt, "--workspace is a flag");
                assert_eq!(workspace, None);
            }
            other => panic!("expected AgentSend, got {other:?}"),
        }
    }

    #[test]
    fn agent_send_rejects_incomplete_invocations() {
        assert!(parse(&["agent", "send", "--workspace"]).is_err()); // flag needs a value
        assert!(parse(&["agent", "send", "--workspace", "backend/x"]).is_err()); // no label
        assert!(parse(&["agent", "send", "--workspace", "backend/x", "primary"]).is_err()); // no body
    }

    fn seed_spec_store() -> crate::data::store::Store {
        use crate::data::store::{NewWorkspace, Store};
        let store = Store::open_in_memory().unwrap();
        // A repo name containing a space exercises the split-on-LAST-slash rule.
        let repo = store
            .add_repo(std::path::Path::new("/tmp/mb"), "meals backend", "wsx")
            .unwrap();
        store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "api-fix",
                branch: "wsx/api-fix",
                worktree_path: std::path::Path::new("/tmp/mb/api-fix"),
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store
    }

    #[test]
    fn workspace_spec_splits_on_the_last_slash() {
        let store = seed_spec_store();
        let ws = resolve_workspace_spec(&store, "meals backend/api-fix").unwrap();
        assert_eq!(ws.name, "api-fix");
    }

    #[test]
    fn workspace_spec_errors_name_the_valid_alternatives() {
        let store = seed_spec_store();

        let e = resolve_workspace_spec(&store, "noslug").unwrap_err().to_string();
        assert!(e.contains("<repo>/<slug>"), "must show the expected form: {e}");

        let e = resolve_workspace_spec(&store, "/api-fix").unwrap_err().to_string();
        assert!(e.contains("<repo>/<slug>"), "empty repo is malformed: {e}");

        let e = resolve_workspace_spec(&store, "meals backend/").unwrap_err().to_string();
        assert!(e.contains("<repo>/<slug>"), "empty slug is malformed: {e}");

        let e = resolve_workspace_spec(&store, "nope/api-fix").unwrap_err().to_string();
        assert!(e.contains("meals backend"), "must list known repos: {e}");

        let e = resolve_workspace_spec(&store, "meals backend/nope").unwrap_err().to_string();
        assert!(e.contains("api-fix"), "must list known slugs: {e}");
    }
```

Also update the pre-existing test at `src/cli.rs:2924` so it still compiles and pins the default:

```rust
    #[test]
    fn parses_agent_send_joins_prompt() {
        match parse(&["agent", "send", "claude#2", "hello", "there"]).unwrap() {
            CliAction::AgentSend {
                target,
                prompt,
                workspace,
            } => {
                assert_eq!(target, "claude#2");
                assert_eq!(prompt, "hello there");
                assert_eq!(workspace, None, "no flag → current workspace");
            }
            other => panic!("expected AgentSend, got {other:?}"),
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib cli::tests 2>&1 | head -40`
Expected: FAIL to **compile** — `CliAction::AgentSend` has no field `workspace`, and `resolve_workspace_spec` is not defined. A compile failure is the correct "red" here.

- [ ] **Step 3: Add the `workspace` field to the action**

In `src/cli.rs:485`, change the variant:

```rust
    AgentSend {
        target: String,
        prompt: String,
        /// `<repo>/<slug>` when addressing an agent in ANOTHER workspace;
        /// `None` means the current workspace (the pre-existing behavior).
        workspace: Option<String>,
    },
```

- [ ] **Step 4: Parse the `--workspace` flag**

Add the usage const next to `parse_agent` in `src/cli.rs`:

```rust
const USAGE_AGENT_SEND: &str = "agent send [--workspace <repo>/<slug>] <label> <prompt>";
```

Replace the `Some("send")` arm of `parse_agent` (`src/cli.rs:1109-1123`):

```rust
        Some("send") => {
            let mut workspace: Option<String> = None;
            // Flags are recognised ONLY before the label. Everything from the
            // label onward is positional, so a message body that itself starts
            // with `--` is preserved verbatim.
            let target = loop {
                let arg = it.next().ok_or_else(|| Error::Usage {
                    group: None,
                    msg: USAGE_AGENT_SEND.into(),
                })?;
                match arg.as_str() {
                    "--workspace" => {
                        workspace = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--workspace needs value (<repo>/<slug>)".into(),
                        })?);
                    }
                    _ => break arg,
                }
            };
            let rest: Vec<String> = it.collect();
            if rest.is_empty() {
                return Err(Error::Usage {
                    group: None,
                    msg: USAGE_AGENT_SEND.into(),
                });
            }
            Ok(CliAction::AgentSend {
                target,
                prompt: rest.join(" "),
                workspace,
            })
        }
```

- [ ] **Step 5: Add the spec resolver**

Insert immediately after `lookup_workspace` (`src/cli.rs:2116`):

```rust
/// Resolve a `--workspace <repo>/<slug>` spec to a workspace.
///
/// Splits on the LAST `/`: repo names may contain spaces and other
/// characters, but a workspace slug never contains `/` (the same assumption
/// `tui_ipc::parse_line` makes). Errors list the valid alternatives, because
/// the caller is usually an agent that cannot enumerate them itself.
fn resolve_workspace_spec(
    store: &crate::data::store::Store,
    spec: &str,
) -> Result<crate::data::store::Workspace> {
    let malformed = || {
        Error::UserInput(format!(
            "--workspace expects <repo>/<slug>, got '{spec}'"
        ))
    };
    let (repo_name, slug) = spec.rsplit_once('/').ok_or_else(malformed)?;
    if repo_name.is_empty() || slug.is_empty() {
        return Err(malformed());
    }
    let repos = crate::data::repo::list(store)?;
    let repo = repos
        .iter()
        .find(|r| r.name == repo_name)
        .ok_or_else(|| {
            Error::UserInput(format!(
                "--workspace: no repo named '{repo_name}'; known repos: {}",
                join_or_none(repos.iter().map(|r| r.name.as_str()))
            ))
        })?;
    let workspaces = store.workspaces(repo.id)?;
    workspaces
        .iter()
        .find(|w| w.name == slug)
        .cloned()
        .ok_or_else(|| {
            Error::UserInput(format!(
                "--workspace: no workspace '{slug}' in repo '{repo_name}'; known: {}",
                join_or_none(workspaces.iter().map(|w| w.name.as_str()))
            ))
        })
}

/// Comma-join names for an error hint, or `(none)` when the list is empty.
fn join_or_none<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let v: Vec<&str> = names.collect();
    if v.is_empty() {
        "(none)".to_string()
    } else {
        v.join(", ")
    }
}
```

- [ ] **Step 6: Wire the dispatch**

Replace the `CliAction::AgentSend` arm (`src/cli.rs:1894-1908`):

```rust
        CliAction::AgentSend {
            target,
            prompt,
            workspace,
        } => {
            let target_ws = match workspace.as_deref() {
                Some(spec) => resolve_workspace_spec(&store, spec)?,
                None => resolve_current_workspace(&store)?,
            };
            let target_id = store
                .resolve_instance_label(target_ws.id, &target)?
                .ok_or_else(|| {
                    // `wsx agent list` only reports the CURRENT workspace, so
                    // list the target's labels inline instead of pointing at it.
                    let labels = store
                        .workspace_agents(target_ws.id)
                        .map(|v| {
                            let names: Vec<String> = v.iter().map(|i| i.label()).collect();
                            join_or_none(names.iter().map(|s| s.as_str()))
                        })
                        .unwrap_or_else(|_| "(unknown)".to_string());
                    Error::UserInput(format!(
                        "no agent '{target}' in workspace {}; agents there: {labels}",
                        target_ws.name
                    ))
                })?;
            let from = std::env::var("WSX_AGENT_INSTANCE_ID")
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .map(crate::data::store::AgentInstanceId);
            store.enqueue_message(target_ws.id, target_id, from, &prompt)?;
            match workspace.as_deref() {
                Some(_) => println!("queued message to {target} in {}", target_ws.name),
                None => println!("queued message to {target}"),
            }
        }
```

Note: `enqueue_message` takes the **target's** workspace id, preserving the column's existing meaning.

- [ ] **Step 7: Update the help text**

In `src/cli.rs:63-66`, replace the `send` `CmdInfo`:

```rust
            CmdInfo {
                usage: "send [--workspace <repo>/<slug>] <label> <message...>",
                blurb: "Queue an async message to an agent here or in another workspace",
            },
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test --lib cli::`
Expected: PASS — including the updated `parses_agent_send_joins_prompt`.

Then check the help renders:

Run: `cargo run -- agent --help`
Expected: the `send` line shows `send [--workspace <repo>/<slug>] <label> <message...>`.

- [ ] **Step 9: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/cli.rs
git commit -m "feat(cli): address agents in other workspaces from wsx agent send"
```

---

### Task 3: origin-qualified sender label

A handoff message is enqueued against the *target's* workspace while its sender lives elsewhere, so the current `sender_label` — which scans `workspace_agents(msg.workspace_id)` — cannot find the sender at all. Switching to the global instance lookup fixes that and yields the sender's workspace for free, which is what qualifies the banner.

**Files:**
- Modify: `src/app/messaging.rs:15-24` (`sender_label`)
- Test: `src/app/messaging.rs` (`mod tests`, same file)

**Interfaces:**
- Consumes: `Store::workspace_agents_by_id(AgentInstanceId) -> Result<Option<AgentInstance>>` (`src/data/agents.rs:200`); `Store::workspace_by_id(WorkspaceId) -> Result<Option<Workspace>>` (`src/data/store.rs:312`); `Store::repos() -> Result<Vec<Repo>>` (`src/data/repo.rs:98`)
- Produces: `sender_label(store, msg) -> Option<String>` — unchanged signature; returns `"claude"` for a same-workspace sender and `"workspacex/parent-task claude"` for a cross-workspace one. `delivery_banner` is unchanged.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/app/messaging.rs`:

```rust
    #[test]
    fn sender_label_qualifies_a_cross_workspace_origin() {
        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "workspacex", "wsx")
            .unwrap();
        let mk = |name: &str, path: &str| {
            store
                .insert_workspace(&NewWorkspace {
                    repo_id: repo,
                    name,
                    branch: &format!("wsx/{name}"),
                    worktree_path: std::path::Path::new(path),
                    yolo: false,
                    agent: AgentKind::Claude,
                    shared: false,
                })
                .unwrap()
        };
        let origin = mk("parent-task", "/tmp/r/parent-task");
        let child = mk("child-task", "/tmp/r/child-task");
        let sender = store.add_primary_agent(origin, AgentKind::Claude, 1).unwrap();
        let target = store.add_primary_agent(child, AgentKind::Claude, 1).unwrap();

        // A handoff: enqueued against the TARGET's workspace, sent from `origin`.
        store
            .enqueue_message(child, target.id, Some(sender.id), "TASK: build it")
            .unwrap();
        let msg = store.undelivered_messages().unwrap().pop().unwrap();

        let label = sender_label(&store, &msg);
        assert_eq!(label.as_deref(), Some("workspacex/parent-task claude"));
        assert_eq!(
            delivery_banner(label.as_deref(), "TASK: build it"),
            "[message from workspacex/parent-task claude]\nTASK: build it"
        );
    }
```

The pre-existing `sender_label_resolves_originating_instance` test is the regression guard for the same-workspace case — do not modify it.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib sender_label_qualifies -- --nocapture`
Expected: FAIL — `assertion left == right` with `left: None`. The sender lives in `parent-task` but the lookup scans `child-task`'s agents, so it finds nothing.

- [ ] **Step 3: Implement the global lookup and qualification**

Replace `sender_label` in `src/app/messaging.rs`:

```rust
/// Resolve the human-readable sender label for a message (None → CLI/human origin).
///
/// The sender is looked up GLOBALLY by instance id rather than within
/// `msg.workspace_id`, because a handoff is enqueued against the *target's*
/// workspace while its sender lives elsewhere. When the sender is in a
/// different workspace than the message, the label is qualified with
/// `<repo>/<slug> ` so the recipient can see where the work came from.
pub fn sender_label(store: &Store, msg: &AgentMessage) -> Option<String> {
    let from = msg.from_agent_id?;
    let sender = store.workspace_agents_by_id(from).ok()??;
    let label = sender.label();
    if sender.workspace_id == msg.workspace_id {
        return Some(label);
    }
    match workspace_ref(store, sender.workspace_id) {
        Some(origin) => Some(format!("{origin} {label}")),
        // Origin workspace row is gone (archived mid-flight): the bare label
        // is still better than dropping the sender entirely.
        None => Some(label),
    }
}

/// `<repo>/<slug>` for a workspace id, or None if either row is missing.
fn workspace_ref(store: &Store, ws: crate::data::store::WorkspaceId) -> Option<String> {
    let w = store.workspace_by_id(ws).ok()??;
    let repo = store.repos().ok()?.into_iter().find(|r| r.id == w.repo_id)?;
    Some(format!("{}/{}", repo.name, w.name))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib app::messaging`
Expected: PASS — both `sender_label_qualifies_a_cross_workspace_origin` and the untouched `sender_label_resolves_originating_instance` and `banner_tags_sender`.

- [ ] **Step 5: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/app/messaging.rs
git commit -m "feat(messaging): qualify cross-workspace senders with <repo>/<slug> in the banner"
```

---

### Task 4: warn when no dashboard can deliver

Delivery only happens inside a running `wsx` TUI tick (`App::drain_agent_messages`, reached from `src/app.rs:1364`). With no dashboard up, a queued handoff sits in `agent_messages` indefinitely and the new workspace never starts — silently, from the sender's point of view.

**Files:**
- Modify: `src/tui_ipc.rs` (add `any_live_tui` after `live_socket_candidates`, `src/tui_ipc.rs:44`)
- Modify: `src/cli.rs` (`CliAction::AgentSend` dispatch, after `enqueue_message`)
- Test: `src/tui_ipc.rs` (`mod ipc_tests`, same file)

**Interfaces:**
- Consumes: `tui_ipc::live_socket_candidates() -> Vec<(PathBuf, u32)>`; `tui_ipc::socket_path_for(pid: u32) -> PathBuf`; `crate::test_support::EnvGuard` (`src/test_support.rs:73`, methods `new()` / `set(key, value)`)
- Produces: `pub fn any_live_tui() -> bool`

- [ ] **Step 1: Write the failing test**

Add to `mod ipc_tests` in `src/tui_ipc.rs`:

```rust
    #[test]
    fn any_live_tui_detects_a_listener_and_ignores_a_stale_socket() {
        let dir = tempfile::tempdir().unwrap();
        let mut guard = crate::test_support::EnvGuard::new();
        guard.set("XDG_RUNTIME_DIR", dir.path());
        std::fs::create_dir_all(socket_dir()).unwrap();

        // Nothing at all.
        assert!(!any_live_tui(), "empty socket dir is not a live TUI");

        // A stale socket: bind then drop. Dropping a UnixListener does NOT
        // unlink the path, so the file survives with nobody listening —
        // exactly what a crashed TUI leaves behind.
        let stale = socket_path_for(999_999);
        {
            let _dead = std::os::unix::net::UnixListener::bind(&stale).unwrap();
        }
        assert!(stale.exists(), "precondition: stale socket file remains");
        assert!(!any_live_tui(), "a socket with no listener is not a live TUI");
        std::fs::remove_file(&stale).unwrap();

        // A real listener.
        let live = socket_path_for(std::process::id());
        let _listener = std::os::unix::net::UnixListener::bind(&live).unwrap();
        assert!(any_live_tui(), "a bound listener is a live TUI");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib any_live_tui -- --nocapture`
Expected: FAIL to compile — `any_live_tui` is not defined.

- [ ] **Step 3: Implement `any_live_tui`**

Add to `src/tui_ipc.rs`, directly after `live_socket_candidates`:

```rust
/// Whether a `wsx` TUI is currently listening on one of its IPC sockets.
///
/// Messages queued by `wsx agent send` are only injected by a running TUI
/// (`App::drain_agent_messages`), so a queued handoff with no dashboard up
/// never reaches its target. A live listener accepts a connection; a stale
/// socket file left behind by a dead process refuses it.
pub fn any_live_tui() -> bool {
    live_socket_candidates()
        .into_iter()
        .any(|(path, _pid)| std::os::unix::net::UnixStream::connect(path).is_ok())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib tui_ipc`
Expected: PASS — including the pre-existing `parse_line_handles_spaces_in_repo_names` and `socket_path_shape`.

- [ ] **Step 5: Warn from the send path**

In `src/cli.rs`, in the `CliAction::AgentSend` arm, insert immediately after `store.enqueue_message(...)?;` and before the `match workspace.as_deref()` that prints:

```rust
            if !crate::tui_ipc::any_live_tui() {
                // The TUI is the only thing that injects queued messages, so
                // without one this send is a no-op the sender would never
                // notice. Not an error: the row is queued, not lost.
                eprintln!(
                    "warning: no wsx dashboard is running — this message is queued and \
                     will not be delivered until one starts. Tell the user to open `wsx`."
                );
            }
```

- [ ] **Step 6: Verify the whole crate still builds and tests pass**

Run: `cargo test --lib`
Expected: PASS. If an `app::input` test fails, re-run it alone (`cargo test --lib app::input::<name>`) before treating it as a regression — those flake on PTY timing under the full suite.

- [ ] **Step 7: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/tui_ipc.rs src/cli.rs
git commit -m "feat(cli): warn when no dashboard is live to deliver a queued message"
```

---

### Task 5: doctrine handoff clauses

**Files:**
- Modify: `src/agent/doctrine.rs` (add two consts after `CLAUSE_WSX_SKILL` at `:28`; push them in `process_doctrine` at `:87`)
- Test: `src/agent/doctrine.rs` (`mod tests`, same file)

**Interfaces:**
- Consumes: `process_doctrine(agent: AgentKind) -> String` (`src/agent/doctrine.rs:80`); clauses are joined with `"\n"` and each const is exactly one `- ` bullet.
- Produces: `CLAUSE_HANDOFF_OUT`, `CLAUSE_HANDOFF_IN` — included for **every** `AgentKind` (unlike `CLAUSE_SUPERPOWERS`, which is Claude/Pi only).

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src/agent/doctrine.rs`:

```rust
    #[test]
    fn doctrine_teaches_handoff_to_every_agent() {
        for agent in [
            AgentKind::Claude,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Codex,
        ] {
            let d = process_doctrine(agent);
            assert!(
                d.contains("wsx agent send --workspace"),
                "{agent:?} must learn the cross-workspace send: {d}"
            );
            assert!(
                d.contains("wsx workspace create <repo> --name <slug>"),
                "{agent:?} must learn to name the new workspace: {d}"
            );
            assert!(
                d.to_lowercase()
                    .contains("do not `cd` into the new worktree"),
                "{agent:?} must be told not to drive the workspace it created: {d}"
            );
        }
    }

    #[test]
    fn doctrine_tells_the_receiver_a_brief_is_its_task() {
        let d = process_doctrine(AgentKind::Claude);
        assert!(d.contains("handoff brief"), "receiving side missing: {d}");
        assert!(
            d.contains("wsx recap set --goal"),
            "receiver must seed its recap goal from the brief: {d}"
        );
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib doctrine -- --nocapture`
Expected: FAIL — `doctrine_teaches_handoff_to_every_agent` panics with "Claude must learn the cross-workspace send" and the full doctrine dumped.

- [ ] **Step 3: Add the clauses**

Insert into `src/agent/doctrine.rs` after `CLAUSE_WSX_SKILL` (`:30`):

```rust
const CLAUSE_HANDOFF_OUT: &str = "- Start a new workspace instead of a new branch. \
    When the work ahead needs a new branch, or shifts to a concern independent \
    enough that this session's history would be noise, do not branch here — \
    create a workspace and hand the task to its own agent: \
    `wsx workspace create <repo> --name <slug>`, then \
    `wsx agent send --workspace <repo>/<slug> primary \"<brief>\"`. Always pass \
    `--name`; an unnamed workspace forces the new agent to rename it before it \
    can start. The brief is the receiving agent's ONLY context: state the task \
    and what done looks like, why it exists, the decisions and file:line \
    pointers it needs, the constraints, and the first concrete step — write it \
    so it still makes sense if this session were deleted. Then tell the user \
    which workspace is now working on what and return to your own task; do NOT \
    `cd` into the new worktree and work there yourself.";

const CLAUSE_HANDOFF_IN: &str = "- If your first input is a handoff brief from \
    another workspace's agent, that brief is your task. Set `wsx recap set \
    --goal` from it before you start.";
```

Then in `process_doctrine` (`src/agent/doctrine.rs:87`), push them between the skill and status clauses:

```rust
    clauses.push(CLAUSE_WSX_SKILL);
    clauses.push(CLAUSE_HANDOFF_OUT);
    clauses.push(CLAUSE_HANDOFF_IN);
    clauses.push(CLAUSE_STATUS);
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib doctrine`
Expected: PASS — including the pre-existing per-agent clause tests and the `resolve_effective_doctrine` tests.

- [ ] **Step 5: Eyeball the rendered doctrine**

Run: `cargo test --lib doctrine_teaches_handoff_to_every_agent -- --nocapture`
Expected: PASS. (To read the text, temporarily flip an assert to `assert!(false, "{d}")`, read it, and revert — do not commit that.)

- [ ] **Step 6: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/agent/doctrine.rs
git commit -m "feat(doctrine): hand new branches to a new workspace instead of branching in place"
```

---

### Task 6: related-repos prompt rewrite

`build_read_only_prompt` currently instructs the exact behavior we are removing — step 2 says "`cd` there to make changes, commit, and push".

**Files:**
- Modify: `src/agent/related.rs:34-72` (`build_read_only_prompt`, steps 2–3 and the closing paragraphs)
- Test: `src/agent/related.rs` (`mod tests`, same file)

**Interfaces:**
- Consumes: nothing new.
- Produces: `build_read_only_prompt(&[(String, PathBuf)]) -> Option<String>` — same signature; step 1 (create + slug rules) and the read-only warning are unchanged. The pre-existing test `build_read_only_prompt_includes_orchestration_commands` (`src/agent/related.rs:190`) asserts the prompt still contains `wsx workspace create`, `wsx workspace path`, `branch_prefix`, and `Do NOT pass a full branch name` — all four must survive.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src/agent/related.rs`:

```rust
    #[test]
    fn read_only_prompt_prescribes_handoff_not_cd_and_work() {
        let r = vec![("frontend".to_string(), PathBuf::from("/work/frontend"))];
        let out = build_read_only_prompt(&r).unwrap();
        assert!(
            out.contains("wsx agent send --workspace <repo>/<slug> primary"),
            "prompt must teach the handoff send: {out}"
        );
        assert!(
            out.contains("Do NOT `cd` into the sibling worktree"),
            "prompt must forbid working in the sibling from this session: {out}"
        );
        assert!(
            !out.contains("`cd` there to make changes"),
            "the old cd-and-work instruction must be gone: {out}"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib related -- --nocapture`
Expected: FAIL — `read_only_prompt_prescribes_handoff_not_cd_and_work` panics on the first assert; the old text is still in place.

- [ ] **Step 3: Rewrite steps 2–3 and the closing paragraphs**

In `src/agent/related.rs`, replace everything in the `format!` from `\x20 2.` through the "Other useful commands" paragraph. The result (step 1 above it and the trailing "Read, grep, and quote freely" line are unchanged):

```rust
         \x20 2. `wsx agent send --workspace <repo>/<slug> primary \
         \"<brief>\"` — hand the task to that workspace's own agent. The \
         brief is its ONLY context: state the task and what done looks \
         like, the API shape or decisions settled here, `file:line` \
         pointers, the constraints, and the first concrete step. Write it \
         so it still makes sense if this session were deleted.\n\
         \x20 3. Tell the user which workspace now owns the sibling task, \
         then carry on with your own. Do NOT `cd` into the sibling \
         worktree and make the changes yourself — that leaves the new \
         workspace idle on the dashboard and piles this task's history \
         into the wrong session.\n\
         \x20 4. Each repo gets its own branch and its own PR. To \
         coordinate \"ship together\", cross-link the PRs in each \
         description and ask the user to merge in dependency order.\n\n\
         Workspaces in different repos do not share Claude session state. \
         The brief is your handoff channel for the initial task; for \
         anything that must outlive either session, propagate it via \
         commits and PR bodies rather than assuming the other session \
         remembers.\n\n\
         Other useful commands: `wsx workspace path <repo> <slug>` (read \
         the sibling worktree without working in it), `wsx workspace list \
         [<repo>]`, `wsx workspace rename <repo> <old-slug> <new-slug>`, \
         `wsx workspace archive <repo> <slug>`.\n\n\
         Read, grep, and quote freely from these read-only paths. Just \
         don't write to them.\n"
```

Note `wsx workspace path` moves into "Other useful commands" rather than being deleted — the pre-existing test at `:190` requires it, and reading the sibling worktree is still legitimate.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib agent::related`
Expected: PASS — the new test plus all four pre-existing `build_read_only_prompt_*` tests.

- [ ] **Step 5: Commit**

```bash
mise exec rust@1.95.0 -- cargo fmt --all
cargo clippy --all-targets -- -D warnings
git add src/agent/related.rs
git commit -m "feat(related): brief a sibling workspace's agent instead of working in its worktree"
```

---

### Task 7: skill and book documentation

**Files:**
- Modify: `skills/wsx/SKILL.md` — CLI surface (line ~38), Cross-repo orchestration (lines ~81-95), Common mistakes (lines ~97-103); add a new `## Handing off to a new workspace` section
- Modify: `docs/book/src/configuration/multi-agent-workspaces.md:41-54` and `:85`

**Interfaces:**
- Consumes: the CLI surface built in Tasks 1–4. Every command shown must match exactly: `wsx agent send [--workspace <repo>/<slug>] <label> <message…>`, label `primary`.
- Produces: documentation only — no code depends on this task.

- [ ] **Step 1: Update the SKILL.md CLI surface**

In `skills/wsx/SKILL.md`, replace the `wsx agent send` line in the `## CLI surface` block:

```
wsx agent send [--workspace <repo>/<slug>] <label> <message…>
                                            # async message to an agent; omit
                                            # --workspace for a peer here.
                                            # label `primary` = that workspace's
                                            # primary agent (always correct for a
                                            # workspace you just created).
```

- [ ] **Step 2: Add the handoff section**

Insert a new section in `skills/wsx/SKILL.md` immediately before `## Cross-repo orchestration`:

````markdown
## Handing off to a new workspace

Creating a workspace and then working in it yourself defeats the purpose: the
new workspace sits idle on the dashboard while this session's history grows.
Create it, brief its agent, and go back to your own task.

**When.** Two triggers:

- **Hard:** the work ahead needs a new branch. Branching inside this worktree
  is the wrong move — create a workspace instead.
- **Soft:** the work shifts to a concern independent enough that this session's
  history would be noise. It must genuinely stand alone; a subtask of what
  you're already doing does not qualify.

**How.** Two commands:

```
wsx workspace create <repo> --name <slug>
wsx agent send --workspace <repo>/<slug> primary "<brief>"
```

Always pass `--name` — an unnamed workspace forces the new agent to rename it
before it can start. Use `primary` as the label: you cannot run `wsx agent
list` against another workspace, and a fresh workspace has exactly one agent.

**The brief.** It is the receiving agent's *only* context. Write it so it still
makes sense if this session were deleted.

```
TASK:        what to build or fix, and what done looks like
WHY:         the decision or finding that led here
CONTEXT:     contracts, types, names, file:line pointers — anything decided in
             my session that is not yet in the repo
CONSTRAINTS: don't touch X; follow the pattern at path:line; merge after PR #N
START:       the first concrete step
```

**Then.** Tell the user which workspace is now working on what, and return to
your own task.

**Worked example:**

```
wsx workspace create backend --name add-widgets-endpoint
wsx agent send --workspace backend/add-widgets-endpoint primary "
TASK: Add POST /widgets returning 201 with the created Widget. Done when the
handler, its route registration, and a happy-path + validation test are in.
WHY: The frontend work in workspacex/widgets-ui needs this endpoint; I settled
the payload shape there and it is not in any repo yet.
CONTEXT: Request body is {name: string, qty: int}; response is the full Widget
including server-assigned id and created_at. Follow the pattern in
src/api/gadgets.rs:40-88 — same validation helper, same error envelope.
CONSTRAINTS: Don't change the existing GET /widgets response shape. This must
merge BEFORE workspacex/widgets-ui.
START: read src/api/gadgets.rs:40-88, then src/api/mod.rs route table.
"
```

Delivery requires a running `wsx` dashboard — the TUI is what injects queued
messages. If `agent send` warns that none is running, tell the user to open
`wsx`, or the handoff will sit undelivered.
````

- [ ] **Step 3: Invert Cross-repo orchestration step 3**

In `skills/wsx/SKILL.md`, replace step 3 of `## Cross-repo orchestration` and the paragraph that closes the section:

```markdown
3. **Brief its agent and hand off.**
   ```
   wsx agent send --workspace <other-repo>/<slug> primary "<brief>"
   ```
   See [Handing off to a new workspace](#handing-off-to-a-new-workspace) for
   what the brief must contain. Do NOT `cd` into the sibling worktree and make
   the changes yourself.
```

and replace the closing paragraph ("If the work is large enough that you want
separate Claude sessions per repo…") with:

```markdown
The sibling session does not share your context — the brief is your handoff
channel for the initial task. For anything that must outlive either session,
propagate it via commits and PR bodies.
```

- [ ] **Step 4: Add the common mistake**

Append to `## Common mistakes` in `skills/wsx/SKILL.md`:

```markdown
- **Driving a workspace you created.** Creating a workspace and then `cd`-ing
  into it leaves it idle on the dashboard and piles its history into the wrong
  session. Create, brief, hand off.
```

- [ ] **Step 5: Update the book page**

In `docs/book/src/configuration/multi-agent-workspaces.md`, replace the command block at line 44 and the paragraph at line 47:

````markdown
```bash
wsx agent send [--workspace <repo>/<slug>] <label> <message…>
```

`<label>` is an agent's footer/list label (`claude`, `claude#2`, `codex`, …),
or the reserved label `primary` for the workspace's primary agent. The rest of
the line is the message body. Without `--workspace` the target is the current
workspace; with it, any workspace — which is how one agent hands a task to a
freshly created workspace's agent. Delivery is **asynchronous**: the message is
queued and injected into the target's session on the next tick, prefixed with a
banner so the recipient knows where it came from:

```
[message from claude#2]
…your message body…
```

A sender in a *different* workspace is qualified with its `<repo>/<slug>`, so
the recipient can see which workspace the work came from:

```
[message from workspacex/parent-task claude]
…your message body…
```
````

Then extend the paragraph at line 54, appending after "…errors with a hint to run `wsx agent list`.":

```markdown
Queued messages are injected by the running `wsx` TUI, so `wsx agent send`
warns on stderr when no dashboard is running — the message stays queued and is
delivered when one starts.
```

And extend the paragraph at line 85, appending:

```markdown
`--workspace <repo>/<slug>` overrides that resolution for the *target*;
`$WSX_AGENT_INSTANCE_ID` still identifies the sender, which is how a
cross-workspace message gets its `<repo>/<slug>`-qualified banner.
```

- [ ] **Step 6: Verify the docs match the shipped CLI**

Run: `cargo run -- agent --help`
Expected: the `send` usage line matches what SKILL.md and the book show, character for character in the flag and label names.

Run: `grep -n "cd\` there to make changes\|Staying in the same Claude session" skills/wsx/SKILL.md`
Expected: no matches — the old prescriptions are gone.

- [ ] **Step 7: Commit**

```bash
git add skills/wsx/SKILL.md docs/book/src/configuration/multi-agent-workspaces.md
git commit -m "docs(skill): handing off to a new workspace"
```

---

### Task 8: full verification and install

**Files:** none modified — this task verifies and installs.

**Interfaces:**
- Consumes: everything from Tasks 1–7.
- Produces: a verified branch and a refreshed `~/.claude/skills/wsx/SKILL.md`.

- [ ] **Step 1: Run the full test suite**

Run: `cargo test`
Expected: PASS. If an `app::input` test fails, re-run it in isolation before treating it as a regression — those flake on PTY timing under the full suite.

- [ ] **Step 2: Check formatting with the pinned toolchain**

Run: `mise exec rust@1.95.0 -- cargo fmt --all --check`
Expected: no output, exit 0. Ambient `cargo fmt` uses a different rustfmt and will false-pass, so this exact command is the gate.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Smoke-test the send path end to end**

With a `wsx` dashboard running in another terminal, from inside this worktree:

```bash
wsx workspace list                       # pick an existing <repo>/<slug>
wsx agent send --workspace <repo>/<slug> primary "handoff smoke test — ignore"
```

Expected: prints `queued message to primary in <slug>`, and the target workspace's agent spawns (if cold) and receives a message banner reading `[message from <this-repo>/<this-slug> claude]`.

Then stop the dashboard and repeat the `agent send`. Expected: the same `queued message…` line on stdout **plus** the `warning: no wsx dashboard is running…` line on stderr.

- [ ] **Step 5: Reinstall the bundled skill**

`~/.claude/skills/wsx/SKILL.md` is a plain copy written by `wsx setup install-skill`, not a symlink, and it is already stale (it predates the recap section). Refresh it from this branch:

```bash
cargo build --release
./target/release/wsx setup install-skill
diff skills/wsx/SKILL.md ~/.claude/skills/wsx/SKILL.md
```

Expected: `diff` reports no differences.

- [ ] **Step 6: Push and open a PR**

```bash
git push -u origin HEAD
```

Then open a PR summarizing: the CLI can now address agents in other workspaces; the doctrine and related-repos prompt prescribe handoff instead of `cd`-and-work; delivery still requires a running dashboard, which the CLI now warns about.

---

## Spec coverage

| Spec section | Task |
|---|---|
| 1. CLI — address agents in another workspace | 2 (`--workspace`, `resolve_workspace_spec`, dispatch, help) |
| 1. The `primary` label alias | 1 |
| 2. Sender labelling across workspaces | 3 |
| 3. Warn when no dashboard can deliver | 4 |
| 4. Doctrine (`CLAUSE_HANDOFF`) | 5 (split into `_OUT` / `_IN` so each const is one bullet) |
| 5. Related-repos prompt | 6 |
| 6. Skill + book | 7 |
| Testing section | folded into each task; full-suite gate in 8 |
| Commits section | Tasks 1–3 are a finer split of the spec's commit 1, so each is independently reviewable |
