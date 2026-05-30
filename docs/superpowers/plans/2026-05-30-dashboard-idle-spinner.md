# Dashboard Idle Spinner Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a static `Idle (·)` indicator instead of the animated braille spinner for a chat session that is running but has never been given a prompt.

**Architecture:** Add a `user_has_prompted: bool` input to `Status::classify`, derived in `App::classify_status` from the existing `WorkspaceEvents::first_user_text`. When a session is running but the user has not prompted, return `Status::Idle` (which is non-live, so no spinner) instead of `Thinking`/`Waiting`.

**Tech Stack:** Rust, `cargo test`. The codebase compiles as one crate, so a signature change touches all call sites at once; the plan plumbs the parameter first (no behavior change), then adds the gate via TDD.

---

### Task 1: Plumb `user_has_prompted` through the classifier (no behavior change)

**Files:**
- Modify: `src/ui/dashboard/status.rs` (signature + all test call sites)
- Modify: `src/app.rs:462-483` (derive and pass the flag)

- [ ] **Step 1: Add the parameter to `Status::classify` with a no-op**

In `src/ui/dashboard/status.rs`, change the signature to add `user_has_prompted: bool` immediately before `has_prior_session: bool`:

```rust
    pub fn classify(
        awaiting_tool: bool,
        stopped_kind: Option<StoppedKind>,
        stalled: bool,
        seconds_since_activity: Option<u64>,
        session_running: bool,
        user_has_prompted: bool,
        has_prior_session: bool,
    ) -> Self {
```

Then add a no-op consume at the very top of the function body (right after the opening `{`, before the `pty_active` line) so it compiles without an unused-variable warning. This task adds NO logic — that comes in Task 2:

```rust
        // Consumed in Task 2; no behavior change yet.
        let _ = user_has_prompted;
```

- [ ] **Step 2: Derive and pass the flag from `App::classify_status`**

In `src/app.rs`, inside `classify_status`, add the derivation just before the `Status::classify(` call at line 475 (e.g. directly after the `awaiting` binding):

```rust
        let user_has_prompted = self
            .workspace_events
            .get(&ws.id)
            .is_some_and(|e| e.first_user_text.is_some());
```

Then update the call to pass it between `running` and `has_prior`:

```rust
        crate::ui::dashboard::status::Status::classify(
            awaiting,
            stopped_kind,
            stalled,
            secs,
            running,
            user_has_prompted,
            has_prior,
        )
```

- [ ] **Step 3: Update every test call site in `status.rs`**

The new argument is the second-to-last positional argument (before `has_prior_session`). Insert `true` for every call where `session_running` is `true`, and `false` for the two calls where `session_running` is `false`. Apply these exact edits in `src/ui/dashboard/status.rs`:

Line 147 (running=true):
```rust
            Status::classify(true, Some(StoppedKind::Complete), true, s(5), true, true, true),
```

Line 158 (running=true):
```rust
            Status::classify(true, None, false, s(0), true, true, false),
```

Line 162 (running=true):
```rust
            Status::classify(true, None, false, s(1), true, true, false),
```

Line 173 (running=true):
```rust
            Status::classify(true, None, false, None, true, true, false),
```

The two multi-line calls at lines 184 and 199 (both `awaiting_answer*`, running=true) — insert `true,` on its own line between the `true,` (session_running) line and the final `true` (has_prior_session) line. After editing, each reads:
```rust
            Status::classify(
                false,
                Some(StoppedKind::AwaitingAnswer),
                false,
                s(0), // s(1) for the second call
                true,
                true,
                true
            ),
```

Line 214 (running=true):
```rust
            Status::classify(false, Some(StoppedKind::Complete), false, s(1), true, true, true),
```

Line 222 (running=true):
```rust
            Status::classify(false, None, true, s(0), true, true, true),
```

Line 230 (running=true):
```rust
            Status::classify(false, None, false, s(0), true, true, false),
```

Line 234 (running=true):
```rust
            Status::classify(false, None, false, s(29), true, true, false),
```

Line 242 (running=true):
```rust
            Status::classify(false, None, false, s(30), true, true, false),
```

Line 246 (running=true):
```rust
            Status::classify(false, None, false, s(3600), true, true, false),
```

Line 254 (running=false):
```rust
            Status::classify(false, None, false, None, false, false, true),
```

Line 258 (running=false):
```rust
            Status::classify(false, None, false, None, false, false, false),
```

- [ ] **Step 4: Run the status tests to confirm plumbing compiles with no behavior change**

Run: `cargo test --lib ui::dashboard::status`
Expected: PASS (all existing tests still pass; the new parameter is a no-op).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/status.rs src/app.rs
git commit -m "refactor(status): plumb user_has_prompted into classify (no-op)"
```

---

### Task 2: Gate the running branch on `user_has_prompted` (TDD)

**Files:**
- Test: `src/ui/dashboard/status.rs` (new tests in the existing `tests` module)
- Modify: `src/ui/dashboard/status.rs` (`classify` body)

- [ ] **Step 1: Write the failing tests**

Add these tests to the `tests` module in `src/ui/dashboard/status.rs`:

```rust
    #[test]
    fn running_but_never_prompted_is_idle() {
        // Session is live and the agent's welcome UI has produced recent PTY
        // output (s(0)), but the user has never submitted a prompt — nothing
        // has happened yet, so show Idle, not the Thinking spinner.
        assert_eq!(
            Status::classify(false, None, false, s(0), true, false, false),
            Status::Idle
        );
    }

    #[test]
    fn running_but_never_prompted_is_idle_when_pty_unknown() {
        assert_eq!(
            Status::classify(false, None, false, None, true, false, false),
            Status::Idle
        );
    }

    #[test]
    fn never_prompted_does_not_override_higher_priority_states() {
        // The not-prompted gate sits below the early returns, so a stall,
        // permission prompt, or completion still wins even with prompted=false.
        assert_eq!(
            Status::classify(false, None, true, s(0), true, false, false),
            Status::Stalled
        );
        assert_eq!(
            Status::classify(true, None, false, s(5), true, false, false),
            Status::Question
        );
        assert_eq!(
            Status::classify(false, Some(StoppedKind::Complete), false, None, true, false, false),
            Status::Complete
        );
    }

    #[test]
    fn running_and_prompted_still_thinking_when_recent() {
        // Regression: once the user has prompted, recent activity is Thinking.
        assert_eq!(
            Status::classify(false, None, false, s(0), true, true, false),
            Status::Thinking
        );
    }
```

- [ ] **Step 2: Run the new tests to verify they fail for the right reason**

Run: `cargo test --lib ui::dashboard::status::tests::running_but_never_prompted`
Expected: FAIL — `running_but_never_prompted_is_idle` and `..._when_pty_unknown` assert `Idle` but currently get `Thinking` (the no-op gate isn't wired yet). The ordering and `_and_prompted_` tests pass already.

- [ ] **Step 3: Replace the no-op with the real gate**

In `classify`, delete the `let _ = user_has_prompted;` line added in Task 1. Then change the `session_running` branch so the gate runs before the activity match:

```rust
        if session_running {
            if !user_has_prompted {
                // Session is live but the user has never submitted a prompt —
                // the agent is idle at its welcome screen, so nothing has
                // happened yet. Show Idle (static `·`) rather than the spinner.
                return Status::Idle;
            }
            match seconds_since_activity {
                Some(s) if s < 30 => Status::Thinking,
                Some(_) => Status::Waiting,
                None => Status::Thinking,
            }
        } else {
            // No live session — `has_prior_session` distinguishes
            // "resumable" from "off" today; both collapse to Idle in V5.
            let _ = has_prior_session;
            Status::Idle
        }
```

- [ ] **Step 4: Run the full status test module to verify all pass**

Run: `cargo test --lib ui::dashboard::status`
Expected: PASS (new never-prompted tests plus all existing regression tests).

- [ ] **Step 5: Commit**

```bash
git add src/ui/dashboard/status.rs
git commit -m "fix(dashboard): show idle, not spinner, for never-prompted sessions"
```

---

### Task 3: Full build and test sweep

**Files:** none (verification only)

- [ ] **Step 1: Build the whole crate**

Run: `cargo build`
Expected: PASS — confirms the `app.rs` call site and any other consumers compile.

- [ ] **Step 2: Run the full test suite**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 3: Lint**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: PASS — confirms no unused-variable or other warnings from the new parameter.
