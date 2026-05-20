# Dashboard fold leader (`z` chord) — Design

## Goal

Turn the dashboard's existing single-press `z` (toggle fold for focused repo) into a vim-fold-flavored leader chord, gaining two new actions:

- `za` — expand (open) every repo on the dashboard
- `zM` — fold (close) every repo on the dashboard

Single-press `z` is retired in favor of the two-press `zz` for the same toggle-current behavior.

## Approach

Add a `z_leader_pending: bool` to `App` (separate from the existing `leader_pending` which is `Ctrl-x`-bound — keeping the chord families distinct avoids semantic confusion). On the dashboard view, `z` arms the flag; the next keypress consumes it and dispatches. All other views are unaffected.

"Expand all" / "fold all" map cleanly onto the existing `app.dashboard.folded: HashMap<u64, bool>`: insert an explicit `false` (expanded) or `true` (folded) for every repo, overriding the renderer's default-fold heuristic.

## Decisions

### Keymap

| Keypress | State | Action |
|---|---|---|
| `z` | no leader pending | set `z_leader_pending = true`, no other action |
| `z` | z-leader pending | clear flag; toggle fold for focused repo (== today's `z` behavior) |
| `a` | z-leader pending | clear flag; for every repo, `dashboard.folded.insert(rid.0 as u64, false)` |
| `M` (Shift+m) | z-leader pending | clear flag; for every repo, `dashboard.folded.insert(rid.0 as u64, true)` |
| `Esc` | z-leader pending | clear flag |
| any other key | z-leader pending | clear flag; **eat the key** (do not pass through to the underlying dashboard handler) |

**Why eat-on-unknown-follow-up:** if someone types `zj` (z + down-arrow) expecting `zz`, neither action firing is the predictable result. Letting `j` fall through would silently move the selection and surprise the user. Press something familiar again to recover.

**Why the asymmetric `za`/`zM` pair:** vim uses `zR` (Reduce folds = open all) and `zM` (Most folds = close all). The user explicitly chose `za` for open-all, so the pair is `za`/`zM` rather than `zR`/`zM`. The `zR` slot is intentionally free for later if symmetry is wanted.

### State location

`App` gains:

```rust
pub z_leader_pending: bool,
```

Initialized `false` in `App::new`. Distinct from `leader_pending` (Ctrl-x chord state) so the two chord families don't collide.

The flag only matters on the dashboard view. Other views (attached, attached-pm, etc.) ignore `z` entirely today; this design keeps them unchanged.

### Key handler changes

In `src/app.rs`, the existing dashboard key arm:

```rust
(KeyCode::Char('z'), _) => { /* today's toggle-current logic */ }
```

is replaced by a check that branches on `app.z_leader_pending`:

- If `z_leader_pending == false`: arm the flag, return early.
- If `z_leader_pending == true`: this is the chord-second-press path. The same arm clears the flag and dispatches based on the key:
  - `z` → run today's toggle-current logic
  - `a` → fold-all-set-false
  - `M` → fold-all-set-true
  - `Esc` → no further action
  - anything else → no further action (key is eaten)

To keep this clean, the chord dispatch lives in its own small helper (e.g. `handle_z_leader_chord(app, key)`) so the main key-handler arm stays readable. The helper takes the focused-repo lookup logic (currently inlined in the `z` arm at `src/app.rs:1602`) and runs it for the `zz` branch.

### "Expand all" / "fold all" implementations

Both follow the same shape — they're one-line state mutations:

```rust
fn expand_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, false);
    }
}

fn fold_all_repos(app: &mut App) {
    for r in &app.repos {
        app.dashboard.folded.insert(r.id.0 as u64, true);
    }
}
```

Both override the renderer's `default_fold(counts)` heuristic — a repo that would default-fold (empty / all-quiet) becomes explicitly expanded under `za`, and a repo that would default-expand becomes explicitly folded under `zM`. This matches user intent: "all" means all, not "all that weren't already in the wanted state."

### Footer hint

`src/ui/dashboard/layout.rs:100` currently advertises `("z", "fold")`. Change to `("z", "fold…")` — the trailing ellipsis is the convention for "this is a chord prefix; more keys expected."

A richer "z-leader active" indicator (e.g. a dimmed pill in the footer that lights up while the flag is set) is out of scope for v1. The trailing ellipsis in the static footer plus fast keypresses should be enough; if not, a follow-up can add the live indicator.

### Tests

In `src/app.rs` test module (or wherever dashboard key tests live; follow existing patterns for setting up `App` state and invoking the key handler):

1. `z_alone_arms_leader_without_action` — press `z` from clean state; assert `z_leader_pending == true` and `dashboard.folded` is unchanged.
2. `zz_toggles_focused_repo_fold` — press `z`, then `z`; assert leader clears AND focused repo's `dashboard.folded` entry flipped (or was inserted matching toggled state). Equivalent to today's single-`z` regression.
3. `za_expands_all_repos` — set up an `App` with N repos including at least one that default-folds (empty workspace list) and at least one that's already explicitly folded. Press `z`, then `a`. Assert all repo ids appear in `dashboard.folded` with value `false`. Assert the renderer would now expand all of them.
4. `zM_folds_all_repos` — mirror of `za`. Press `z`, then `M` (`KeyCode::Char('M')` with `KeyModifiers::SHIFT`). Assert all entries are `true`.
5. `z_then_unknown_clears_leader_without_action` — press `z`, then `x` (a key not bound in the dashboard). Assert leader clears AND no fold state changed AND selection didn't move (i.e. the `x` was eaten, not passed through).
6. `z_then_esc_clears_leader` — press `z`, then `Esc`. Assert leader clears AND no action fired.
7. `a_alone_is_unbound` — press `a` from a clean state (no leader). Assert no fold state changed, no selection change, leader not set.
8. `shift_m_alone_is_unbound` — press Shift+m from clean state. Same assertion.

### Module touches

- `src/app.rs` — add `z_leader_pending` field; replace the `z` arm in the dashboard key handler; add the two `expand_all_repos` / `fold_all_repos` helpers; add the 8 tests.
- `src/ui/dashboard/layout.rs:100` — update the footer hint label.

No new files. No public API changes (`z_leader_pending` is internal to `App`; the helpers can be local module functions).

## What's intentionally NOT included

- **No `zR` / `zo` / `zc` / `zA` bindings.** Vim has a richer fold-chord family. Add later if wanted; not blocked by this design.
- **No timeout on the leader.** Press the second key whenever; until you do, `z` stays armed. Matches vim.
- **No visual "leader pending" indicator** beyond the footer's static `fold…` hint.
- **No persistence** of expand-all / fold-all state to the store. The `dashboard.folded` map lives in memory; restarting wsx returns to default-fold-by-counts behavior. This matches today's behavior for single-`z` toggling.
- **No change to attached/attached-PM views.** `z` there means whatever it does today (nothing wsx-specific; it goes to the PTY).
- **No mouse equivalent.** The fold-glyph in the by-repo header (`▾`/`▸`) at `src/ui/dashboard/by_repo.rs:42-49` doesn't currently respond to clicks; that's a separate UX project.
