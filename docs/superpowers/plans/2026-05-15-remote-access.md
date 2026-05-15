# Remote access (leader-key swap + docs) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace wsx's `Ctrl-a` leader key with `Ctrl-x` so wsx works cleanly inside tmux, and ship a README section covering remote access via tmux + ssh.

**Architecture:** Single hard-coded leader, no runtime configurability (single-user tool). Add a `LEADER_KEY` constant so future swaps are one-line. Rename `ctrl_a_pending` → `leader_pending` so the field name doesn't lie. Update all user-visible hint text in the same change. Direct to main, no feature branch.

**Tech Stack:** Rust, crossterm KeyCode/KeyModifiers, ratatui (text), README markdown.

**Spec:** `docs/superpowers/specs/2026-05-15-remote-access-design.md`

---

## File Structure

- `src/app.rs` — Two attached-view key handlers (`handle_key_attached`, `handle_key_attached_pm`) and the `ctrl_a_pending` state field. One existing test (`ctrl_a_u_in_attached_pm_opens_updates_panel`).
- `src/ui/attached.rs` — Footer hint string at line 48.
- `src/ui/mod.rs` — Doc comment at line 15.
- `README.md` — 6 `Ctrl-a` references in keybinds table + flavor text.

No new files. The `LEADER_KEY` constant lives at the top of `src/app.rs` (near the other module-level state).

---

### Task 1: Swap leader key in handlers + state field

**Files:**
- Modify: `src/app.rs` (handlers ~806-828 and ~903-924, field declaration ~125 + init ~166, test ~1853)

- [ ] **Step 1: Update the existing leader test to use Ctrl-x and the new field name**

Find `ctrl_a_u_in_attached_pm_opens_updates_panel` in `src/app.rs` and:
- Rename test to `leader_u_in_attached_pm_opens_updates_panel`
- Change `KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)` → `KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)`
- Change `assert!(app.ctrl_a_pending)` → `assert!(app.leader_pending)`
- Change `assert!(!app.ctrl_a_pending)` → `assert!(!app.leader_pending)`
- Update the `// Send Ctrl-a then 'u'.` comment to `// Send the leader (Ctrl-x) then 'u'.`

- [ ] **Step 2: Run the test to confirm it fails**

```
cargo test ctrl_a_u_in_attached_pm 2>&1 | tail -5
cargo test leader_u_in_attached_pm 2>&1 | tail -5
```
Expected: compile error (test name changed and references unknown field `leader_pending`).

- [ ] **Step 3: Add LEADER_KEY constant near the top of `src/app.rs`**

Place it near the existing `use` statements / module-level items. Something like:

```rust
/// Leader key for attached-view actions. Chosen to be free in raw mode
/// and to avoid collision with tmux's default `Ctrl-b` prefix (and any
/// non-default `Ctrl-a` setups).
const LEADER_KEY: crossterm::event::KeyCode = crossterm::event::KeyCode::Char('x');
```

- [ ] **Step 4: Rename the state field**

In the App struct:
```rust
pub ctrl_a_pending: bool,
```
becomes
```rust
pub leader_pending: bool,
```

And in the constructor:
```rust
ctrl_a_pending: false,
```
becomes
```rust
leader_pending: false,
```

- [ ] **Step 5: Update `handle_key_attached` leader handling**

Replace this block (around line 806):
```rust
    // Ctrl-a prefix handling.
    if app.ctrl_a_pending {
        app.ctrl_a_pending = false;
        match k.code {
            KeyCode::Char('d') => {
                app.view = View::Dashboard;
                return Ok(());
            }
            KeyCode::Char('a') => {
                let _ = session.writer.send(vec![0x01]).await;
                return Ok(());
            }
            KeyCode::Char('u') => {
                app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
    if k.code == KeyCode::Char('a') && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.ctrl_a_pending = true;
        return Ok(());
    }
```

With:
```rust
    // Leader-key prefix handling. See `LEADER_KEY`.
    if app.leader_pending {
        app.leader_pending = false;
        match k.code {
            KeyCode::Char('d') => {
                app.view = View::Dashboard;
                return Ok(());
            }
            KeyCode::Char('x') => {
                // Send a literal Ctrl-x (0x18) to claude.
                let _ = session.writer.send(vec![0x18]).await;
                return Ok(());
            }
            KeyCode::Char('u') => {
                app.modal = Some(crate::ui::modal::Modal::UpdatesPanel { selected: 0 });
                return Ok(());
            }
            _ => return Ok(()),
        }
    }
    if k.code == LEADER_KEY && k.modifiers.contains(KeyModifiers::CONTROL) {
        app.leader_pending = true;
        return Ok(());
    }
```

- [ ] **Step 6: Update `handle_key_attached_pm` with the same swap**

The block at ~903 is identical to the one in Step 5. Apply the same replacement (search-and-replace pattern is the same).

- [ ] **Step 7: Run tests to confirm everything passes**

```
cargo test 2>&1 | tail -10
```
Expected: all tests pass. The renamed test (`leader_u_in_attached_pm_opens_updates_panel`) now compiles and exercises the new path.

- [ ] **Step 8: Commit**

```
git add src/app.rs
git commit -m "feat(keys): replace Ctrl-a leader with Ctrl-x"
```

---

### Task 2: Update user-visible hint text

**Files:**
- Modify: `src/ui/attached.rs` (footer hint, line 48)
- Modify: `src/ui/mod.rs` (doc comment, line 15)
- Modify: `src/app.rs` (test-buffer comment, line ~1831)

- [ ] **Step 1: Update the attached-view footer**

In `src/ui/attached.rs` around line 48, replace:
```rust
format!(" {label}   [Ctrl-a d] detach   [Ctrl-a u] updates   [Ctrl-a a] send Ctrl-a ");
```
With:
```rust
format!(" {label}   [Ctrl-x d] detach   [Ctrl-x u] updates   [Ctrl-x x] send Ctrl-x ");
```

- [ ] **Step 2: Update the doc comment in `src/ui/mod.rs`**

Replace the `Ctrl-a d` reference at line ~15 with `Ctrl-x d`.

- [ ] **Step 3: Update the test-buffer comment in `src/app.rs`**

Find the comment `// The bottom row is the footer with "Ctrl-a d detach". The second-` near line 1831 and update to `Ctrl-x d`.

- [ ] **Step 4: Grep for any remaining mentions and fix**

```
grep -rn "Ctrl-a\|ctrl_a\|ctrl-a" --include="*.rs" src/
```
Expected: no matches. Fix anything that remains.

- [ ] **Step 5: Run tests**

```
cargo test 2>&1 | tail -10
```
Expected: all pass.

- [ ] **Step 6: Commit**

```
git add src/
git commit -m "docs(ui): hints reflect new Ctrl-x leader"
```

---

### Task 3: Update README and add remote-access section

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Swap the 6 `Ctrl-a` references in README**

Each maps directly:
- `Ctrl-a d` → `Ctrl-x d`
- `Ctrl-a u` → `Ctrl-x u`
- `Ctrl-a a` → `Ctrl-x x` (note: the literal-passthrough also changes character)
- `Send a literal `Ctrl-a` to claude` → `Send a literal `Ctrl-x` to claude`

Find sites with:
```
grep -n "Ctrl-a" README.md
```
Update all 6 in place.

- [ ] **Step 2: Add a "Remote access" section to README**

Insert a new H2 section (placement: after CLI reference, before any deep-internals section — wherever fits the existing flow). Content:

```markdown
## Remote access

Running wsx on your desktop and attaching from a laptop or other machine works
cleanly via tmux + ssh.

**One-time setup on the host (desktop):**

```bash
# Start wsx inside a long-lived tmux session.
tmux new -As wsx 'wsx'
```

**Attach from any other machine:**

```bash
ssh desktop -t tmux attach -t wsx
```

Workspaces — and the claude sessions running inside them — keep going while
you're detached, so picking up where you left off from a different machine
just works.

**Notes:**

- wsx's leader key is `Ctrl-x`, chosen to not collide with tmux's default
  `Ctrl-b` prefix (or anyone's customized `Ctrl-a`). No tmux config needed.
- **Mosh** drops in cleanly if your network is flaky: `mosh desktop -- tmux attach -t wsx`.
- **Tailscale** (or any VPN) makes the desktop reachable from anywhere by a
  stable name without port-forwarding.
```

(Adjust the heading level and surrounding spacing to fit the README's existing
style.)

- [ ] **Step 3: Commit**

```
git add README.md
git commit -m "docs: remote-access section + Ctrl-x leader in keybinds"
```

---

### Task 4: Verify + commit spec

**Files:**
- (verify only; commit `docs/superpowers/specs/2026-05-15-remote-access-design.md` if not already committed)

- [ ] **Step 1: Run the full verification suite**

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
Expected: all green.

- [ ] **Step 2: Final grep for stale references**

```
grep -rn "Ctrl-a\|ctrl_a\|ctrl-a" .
```
Expected: no matches in `src/`, `README.md`, or `docs/`. (Build artifacts in `target/` don't count — exclude them if noisy.)

- [ ] **Step 3: Commit the spec and plan docs**

```
git add docs/superpowers/specs/2026-05-15-remote-access-design.md docs/superpowers/plans/2026-05-15-remote-access.md
git commit -m "docs: spec + plan for remote-access leader-key swap"
```

- [ ] **Step 4: Push and close issue**

```
git push origin main
gh issue view 12 --json state
```
Confirm issue #12 auto-closes (the README commit's `Closes #12` trailer should fire — add the trailer if not yet present). If not present, edit the README commit message or open issue #12 and close it manually with a reference.

---

## Self-review checklist

- [x] All 6 README `Ctrl-a` sites identified
- [x] Both `app.rs` handlers covered (handle_key_attached + handle_key_attached_pm)
- [x] Test rename covered
- [x] Byte sent on literal-passthrough updated (0x01 → 0x18)
- [x] LEADER_KEY constant introduced for future-proofing
- [x] Field renamed (`ctrl_a_pending` → `leader_pending`) to match
- [x] No placeholders or TBD
