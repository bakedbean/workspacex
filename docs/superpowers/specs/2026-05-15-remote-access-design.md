# Remote access via tmux + ssh — Design

**Issue:** [#12](https://github.com/bakedbean/workspacex/issues/12)

## Goal

Enable a single user to reach their wsx instance running on one machine (desktop) from another machine (laptop), without writing any networking code into wsx.

## Approach

Lean on tmux + ssh. wsx runs inside a long-lived tmux session on the host machine; the user attaches over ssh from elsewhere. This is the simplest viable path and requires no architectural changes to wsx itself.

The only blocker to this "just work" is that wsx's leader key is `Ctrl-a`, which collides with tmux's default prefix. This spec resolves that collision and ships supporting documentation.

## Decisions

- **Leader key swap.** Replace `Ctrl-a` with `Ctrl-x` as wsx's leader key throughout the application. No back-compat shim — wsx has no production usage to preserve. The escape arm `Ctrl-a a` (send literal `Ctrl-a` to claude) becomes `Ctrl-x x` (send literal `Ctrl-x`).
- **Remote-access documentation.** Ship a new section in `README.md` covering the tmux+ssh setup, plus brief notes on mosh/Tailscale for remote-friendly variants.
- **Single change.** Code and docs ship together in one commit (or tight series) since they're causally linked — the docs assume the new leader key.
- **Direct to main.** Functional correctness work, not subjective; no feature branch.

## Scope

### In
1. Replace `Ctrl-a` with `Ctrl-x` in the attached-view key handler (`src/app.rs::handle_key_attached` and the `app.ctrl_a_pending` state field — likely renamed to `leader_pending`).
2. Update the PM-attached handler that shares the same prefix mechanism.
3. Update all user-visible hint text: footers, status rows, modals.
4. Update inline doc comments referencing `Ctrl-a`.
5. Update tests that exercise the leader-key prefix.
6. Update `README.md` references (6 sites identified).
7. New `README.md` section: "Remote access".

### Out
- Detecting `TMUX` env var and printing hints on startup. Adds noise; collision is gone so detection serves no purpose.
- Making the leader configurable. Single-user tool, single hard-coded choice is simpler.
- Server/client architecture or web frontend (rejected directions during brainstorm).
- Auto-launching wsx inside tmux (e.g. a `wsx serve` subcommand). Out of scope; user invokes `tmux new -As wsx 'wsx'` manually.

## Implementation notes

### Field rename
`app.ctrl_a_pending: bool` → `app.leader_pending: bool`. Keybind-name-agnostic so future swaps wouldn't churn the field name.

### Key constant
Introduce a module-level `LEADER_KEY: KeyCode = KeyCode::Char('x')` so a future swap is one line, even though we don't expose it as user-configurable.

### Documentation block
The README section answers, in order: (1) the one-liner setup, (2) the recommended ssh-attach command, (3) why this works (workspaces keep running while detached), (4) latency/connectivity options (mosh, Tailscale), (5) gotchas (clipboard, scrollback behavior in nested terminals — call out only the ones we hit in practice).

## Risks

- **Muscle memory.** User has been using `Ctrl-a`; switching is annoying for a few days. Acceptable per user direction ("haven't started using it in any serious capacity").
- **Other Ctrl-x consumers.** Some terminals or shells may have bindings on `Ctrl-x` outside raw mode, but wsx runs in raw mode while attached, so keystrokes go directly to wsx. Inside the claude PTY (after `Ctrl-x x`), the literal byte 0x18 reaches claude — same as any other keystroke. No expected issue.
- **Footer length.** The dashboard footer is already wide. The leader key only appears in attached-view footers, not the dashboard footer, so this isn't affected.

## Out-of-scope follow-ups (future ideas, not commitments)

- `wsx remote-attach <host>` subcommand that wraps the `ssh ... tmux attach` invocation.
- Self-hosted web frontend, if mobile access ever becomes a need.
- Daemon/client split, if multi-user or multi-machine state-sharing ever becomes a need.
