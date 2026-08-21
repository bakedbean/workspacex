# Oh My Pi (`omp`) agent support — Design

## Problem

wsx supports four coding-agent harnesses: Claude Code, Pi, Hermes, and Codex.
The user wants a fifth — [oh-my-pi](https://github.com/can1357/oh-my-pi) — wired
through with the same feature set the Claude / Codex / Pi integrations have,
explicitly including YOLO sessions and pinned commands / skills.

oh-my-pi is installed on this machine as `omp` v17.4.0
(`@oh-my-pi/pi-coding-agent`, resolved via `~/.bun/bin/omp`).

### It is not the existing `pi`

The two are easy to confuse and are **not** the same harness:

| | package | binary | version here |
|---|---|---|---|
| existing `AgentKind::Pi` | `@earendil-works/pi-coding-agent` | `pi` | 0.84.2 |
| new `AgentKind::Omp`     | `@oh-my-pi/pi-coding-agent`       | `omp` | 17.4.0 |

They share ancestry — which is why the session JSONL schemas match (see
[Activity](#activity-omp_eventsrs)) — but they are separately maintained, have
divergent CLIs, and are both installed. This is an **addition**, not a
replacement: `--agent pi` keeps meaning the earendil binary.

## Goal

Add `AgentKind::Omp` such that `wsx workspace create --agent omp` (and
`wsx agent add omp`, and the `a` agent picker) spawns an `omp` session in the
worktree with the parity set the Pi and Codex integrations provide:

1. **Spawn** in the worktree's cwd.
2. **Continue** that resumes the right session for *this* worktree.
3. **Yolo** (skip approvals) when the workspace requests it.
4. **Model override** via env var.
5. **Additional directories** for related-repo context.
6. **Auto-rename system prompt** on fresh placeholder-slug worktrees.
7. **Doctrine + custom instructions** injection.
8. **Prior-session indicator** on the dashboard.
9. **Activity tailing** — RECENT CHAT / SESSION SUMMARY / last-message columns.
10. **Pinned commands and skills** working inside the session.
11. **Agent color**, picker entry, Tab-cycle entry, tmux session naming.

## Naming

The identifier is **`omp`** everywhere the user or the DB sees it: `--agent omp`,
`wsx agent add omp`, `wsx config set coding_agent omp`, the `agents.agent`
column, the `omp#2` peer chip, and the `wsx-<repo>-<slug>-omp2` tmux session.
Env vars follow the established shape: `WSX_OMP_BIN`, `WSX_OMP_MODEL`.

Chosen over `oh-my-pi` because `display_name()` feeds fixed-width dashboard
chips and tmux session names, where seven extra characters cost real columns,
and because it matches the binary the user actually types.

## Non-goals

- **An agent-adapter trait refactor.** At N=5 the case for collapsing the five
  parallel `build_*_command` functions behind a trait is stronger than it was at
  N=3, but folding it into "add a harness" would put the new integration's
  correctness behind a refactor's blast radius. File it as a follow-up.
- **Deterministic status reporting.** See
  [Status reporting](#status-reporting--deliberate-gap) — omp has no turn-lifecycle
  hook to wire, so this is not deferred laziness but an absent mechanism.
- **omp-specific capabilities with no wsx analog**: `--profile`, `--prewalk`,
  `--plan-yolo`, `--advisor`, `--max-time`, `--thinking`, `--smol`/`--slow`/`--plan`
  model roles, `--mode rpc`, collab/`join`, the auth broker. All reachable by the
  user through `~/.omp/agent/config.yml`; none need wsx plumbing to work.
- **A `WSX_OMP_PROVIDER` var.** omp documents `--provider` as legacy and prefers
  `--model provider/id`, so a provider var would be a second worse path to the
  same place. `WSX_OMP_MODEL=anthropic/claude-opus-5` covers it.
- **Migrating existing per-agent tests to be agent-parameterized.** omp gets its
  own test family, matching how Hermes and Codex were added.

## Approach

### Command building: a parallel builder

Add `build_omp_command` in `src/pty/command.rs` alongside the existing four.
omp is the most capable harness wsx targets — it is the only one besides Claude
to support *both* `--append-system-prompt` and `--add-dir` — so the builder is
closest in shape to `build_claude_command`, minus the settings/permissions
plumbing.

### Activity: wsx-local, reusing sessionx's pi parser

`sessionx` (a pinned-rev git dependency) owns the Claude / Codex / Pi JSONL
parsers. omp writes **pi-v3-schema** JSONL — verified against a real capture:
same `{"type":"message", …, "message":{…}}` envelope, same `role` vocabulary
(`user` / `assistant` / `toolResult`), same content-part types (`text`,
`thinking`, `toolCall`), same `stopReason` values, same lowercase tool names
(`read`, `bash`, `edit`, `write`, `grep`). Only the storage *location* and the
cwd *encoding* differ.

So `src/activity/omp_events.rs` lives in wsx and implements only
`encode_cwd` + `locate_session_file`, delegating `tail_session` straight to
`sessionx::activity::pi_events::tail_session`.

This was chosen over adding `omp_events` to `sessionx` because that route needs
a sibling workspace, a merged sessionx PR, and a `Cargo.toml` rev bump before
wsx can even compile the feature — a hard cross-repo ordering constraint for a
module that is 40 lines of path arithmetic. It also matches the existing
precedent: `hermes_events` is already wsx-local for the same reason (the parser
depends on wsx-side specifics, not on a shared JSONL shape).

**Revisit if** omp's schema drifts from pi's. The fixture test described in
[Testing](#testing) is the tripwire: it will fail loudly rather than silently
mis-parse.

## CLI mapping

| wsx concept | omp invocation | notes |
|---|---|---|
| `SpawnMode::Fresh` | bare `omp` | |
| `SpawnMode::Continue` | `omp -c` | cwd-scoped, see below |
| yolo | `--approval-mode yolo` | |
| non-yolo | *(no flag)* | inherit `tools.approvalMode` from the user's config, same as Codex inherits its own defaults |
| doctrine + rename prompt + custom instructions | `--append-system-prompt <combined>` | one flag, parts joined by `\n\n`, exactly as Claude and Pi do |
| additional dirs | `--add-dir <path>` (repeated) | |
| `WSX_OMP_MODEL` | `--model <pattern>` | omp fuzzy-matches (`opus`, `gpt-5.2`, `openai/gpt-5.2`) |
| `WSX_OMP_BIN` | binary override | default `omp` |

Space-separated flag forms were confirmed to work against the installed binary
(`omp --approval-mode yolo --add-dir <dir> -p …` exits 0), so the builder does
not need `--flag=value` syntax.

### Why `-c` is enough for Continue

Unlike Hermes — which needed a spawn marker and a sqlite query because
`--continue` resumes the globally-most-recent session — omp's
`SessionManager.continueRecent(cwd)` resolves in this order:

1. A **terminal breadcrumb** keyed by TTY id, used only when its recorded cwd
   equals the current cwd.
2. Otherwise `findMostRecentSession(dir)` where `dir` is **the cwd-encoded
   session directory**.

Each wsx spawn is a fresh PTY with a fresh terminal id, so path 2 is the one
that runs, and it is already scoped to the worktree. A bare `-c` therefore
resumes this worktree's own newest session — no marker file, no db query.

### Yolo

`--approval-mode yolo` is preferred over the equivalent `--auto-approve` because
it is the same knob as omp's persistent `tools.approvalMode` setting, so a
wsx yolo workspace and a user-configured yolo session are visibly the same
state rather than two mechanisms that happen to agree.

Non-yolo sessions pass **nothing**, inheriting whatever the user configured.
This mirrors the Codex decision: wsx should not silently downgrade a harness's
interactive defaults for people who tuned them.

## Session location and encoding

omp stores sessions at `~/.omp/agent/sessions/<encoded-cwd>/<ts>_<uuid>.jsonl`.

The encoding (`getDefaultSessionDirName` in omp's `src/session/session-paths.ts`)
has **three scopes**, checked in this order against the canonicalized cwd:

1. **home** — cwd is `$HOME` or under it. Name is `-` + the home-relative path
   with `/` → `-`. A cwd of exactly `$HOME` yields the bare name `-`.
   `~/.local/state/wsx/worktrees/workspacex/foo`
   → `-.local-state-wsx-worktrees-workspacex-foo`
2. **tmp** — cwd is `os.tmpdir()` or under it. Name is `-tmp`, then `-` + the
   tmp-relative path with `/` → `-`. Bare `/tmp` yields `-tmp`;
   `/tmp/a/b` yields `-tmp-a-b`.
3. **abs** — everything else. The legacy form: `--` + the absolute path with the
   leading separator stripped and `/`, `\`, `:` → `-` + `--`.
   `/srv/code/x` → `--srv-code-x--`.

All three were confirmed empirically against the installed binary, not just read
off the source: `/tmp` → `-tmp`, `/tmp/ompprobe/deep/dir` → `-tmp-ompprobe-deep-dir`,
and this worktree → `-.local-state-wsx-worktrees-workspacex-grand-verbena`.

Because omp indexes sessions by **path**, it inherits the session-bleed hazard
that `write_worktree_sessions` exists to close: archiving a workspace removes
the worktree but leaves `~/.omp/agent/sessions/<encoded-path>/` on disk, and wsx
recycles slugs — so the next workspace drawing the same slug for the same repo
lands on a byte-identical path and would otherwise spawn `-c` into a stranger's
conversation. omp takes **Pi's shape** here, not Codex's: it is a directory of
`.jsonl` files, so the snapshot records one `omp:<filename>` line per file
(rather than Codex's single `codex:<abs-rollout-path>` line), `SessionSnapshot`
grows an `omp: HashSet<String>` field, and `has_prior_omp_session` counts only
files absent from the snapshot.

Two consequences for `locate_session_file`:

- **Canonicalize first.** omp resolves symlinks before classifying, so wsx must
  `std::fs::canonicalize` the worktree (as `pi_events` already does) or a
  symlinked worktree looks up the wrong directory.
- **Fall back to the legacy absolute name.** omp 17.x migrates older
  `--<abs>--` directories into the new scoped names lazily, on first access by
  *omp itself*. A worktree whose sessions predate the migration and that omp has
  not opened since still has its history under the legacy name. Probing the
  canonical name and then the legacy `--<abs>--` name costs one `is_dir` and
  removes a class of "prior session exists but wsx says it doesn't" bugs.

## Activity: `omp_events.rs`

```rust
//! src/activity/omp_events.rs
pub fn encode_cwd(path: &Path, home: &Path, tmp: &Path) -> String;
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf>;
pub use sessionx::activity::pi_events::tail_session;
```

`encode_cwd` takes `home` and `tmp` as parameters rather than reading
`dirs::home_dir()` / `std::env::temp_dir()` internally, so the three-scope
classification is unit-testable without touching the real environment.
`locate_session_file` is the thin production wrapper that resolves both and
picks the newest `.jsonl` in the resulting directory.

Everything downstream — `has_prior_session_for`, `tail_workspace_events`, the
detail-bar modules, tool-use counts, RECENT CHAT — then works unchanged, because
it is all driven off the `TailUpdate` that `pi_events::tail_session` returns.

## What omp gives us for free

Three parity items need **no wsx code at all**, which is unusual and worth
recording so nobody later "fixes" them by adding machinery:

- **Skills.** omp's Claude discovery provider loads
  `~/.claude/skills/*/SKILL.md` (`getUserClaude(ctx)/skills` in
  `src/discovery/claude.ts`) plus project-level `.claude/skills` walking up from
  cwd. So the wsx and agent-review skills installed by
  `wsx setup install-skill` are already visible to omp, and — exactly like Pi —
  omp needs **no new `InstallTarget`** in `src/agent/skill.rs`.
- **Pinned commands.** The same provider loads `~/.claude/commands/*.md` as real
  slash commands, user level and project level. A pinned chip firing
  `/pull-request` reaches omp as the same command Claude would run. No
  Codex-style plugin mirroring (`codex_commands.rs`) is needed.
- **Superpowers.** Because the skills load, omp belongs in
  `doctrine::process_doctrine`'s `include_superpowers` set alongside Claude and
  Pi. This is not an assumption: a live probe session shows omp resolving
  `skill://using-superpowers` on startup.

## Status reporting — deliberate gap

`agent::status::for_agent(AgentKind::Omp)` returns `NoopStatus`.

omp's hook capability (`src/capability/hook.ts`) is **pre/post tool-execution
hooks only** — `{type: "pre"|"post", tool: string}`. There is no turn-lifecycle
event: no `Stop`, no `UserPromptSubmit`, no permission-prompt notification. So
unlike Claude (`--settings` hooks) and Codex (`notify`), there is nothing to
wire, and inventing a status signal out of tool-call edges would report
transitions that do not correspond to the states the dashboard shows.

omp therefore lands in exactly Pi's and Hermes's position, which is the parity
bar this work was scoped against:

- **Tier 1** (model pushes `wsx status set`) — works, via the doctrine clause.
- **Tier 2** (deterministic harness events) — absent.
- **Tier 3** (JSONL heuristic) — works, via `omp_events`.

This is called out here so it reads as a finding about omp rather than an
oversight in the implementation.

## Input delivery: needs a real capture

`ready_for_input` and `submit_writes` in `src/pty/session.rs` are the one part
of this design that **must not be written from the source**. Every existing
predicate there was read off a real cold boot captured through `vt100::Parser`,
and the module documents the Hermes arm's unconditional `true` as an
acknowledged hole precisely because nobody had a boot to read.

omp *is* installed, so it gets a real predicate. Two questions the capture
answers:

1. **`ready_for_input`.** omp must not be considered ready while its startup
   splash (`src/startup-splash.ts`) or a modal holds the screen. Whether the
   signal is alternate-screen (Claude-shaped), a composer glyph plus visible
   cursor (Codex-shaped), or composer chrome rules (Pi-shaped) is determined by
   the capture, not guessed.
2. **`submit_writes`.** Codex needs a bracketed-paste wrapper because its input
   parser does paste-burst detection. omp is the opposite risk: its
   `CustomEditor` runs its own `BracketedPasteHandler` and collapses large
   pastes into `[Paste #N]` markers — so wrapping a wsx message in
   `ESC[200~ … ESC[201~` could turn the message body into a placeholder. The
   working hypothesis is therefore **plain text + CR**, like Pi, but it is
   verified against a live spawn before it ships.

Both land as fixtures (`tests/fixtures/agent-boot/omp-preboot.bin`,
`omp-composer.bin`) and replay tests, matching the existing three.

## Touchpoints

Exhaustive `match AgentKind` sites fail to compile until the `Omp` arm is added,
so the compiler enforces most of this list. The entries marked ⚠ are the ones it
**cannot** catch — a `const` array, string literals, and the picker copy.

| Site | File | Edit |
|---|---|---|
| ⚠ Variant + `ALL` + `from_str_or_default` + `display_name` | `src/pty/agent_kind.rs` | Add `Omp`; grow `ALL` to 5; add `Some("omp")` arm; `display_name` → `"omp"`. |
| Bin env var | `src/pty/session.rs` (~line 69) | `AgentKind::Omp => "WSX_OMP_BIN"`. |
| Spawn dispatch | `src/pty/session.rs` (~line 664) | `AgentKind::Omp => build_omp_command(cwd, &mode, remote)`. |
| Ready predicate | `src/pty/session.rs` (~line 431) | `AgentKind::Omp => omp_ready(screen)` + the new fn. |
| Submit writes | `src/pty/session.rs` (~line 558) | Add `Omp` to the plain-text arm (pending capture). |
| Command builder | `src/pty/command.rs` | New `build_omp_command` + `render_rename_system_prompt_omp`. |
| Prior-session detect | `src/pty/session_detect.rs` (~line 376) | `AgentKind::Omp => has_prior_omp_session(worktree)` + the new fn, snapshot-filtered like `has_prior_pi_session`. |
| ⚠ Session snapshot | `src/pty/session_detect.rs` (`write_worktree_sessions` ~line 128, `read_worktree_sessions` ~line 160, `SessionSnapshot`) | Emit/parse `omp:<filename>` lines and add the `omp` set — see [Session location](#session-location-and-encoding). Not compiler-caught: these are string prefixes, and a miss reopens the session-bleed bug silently. |
| Activity module | `src/activity/omp_events.rs` *(new)* + `src/activity/mod.rs` | Locate + re-export tail. |
| Event tailing | `src/app/background.rs` (~lines 39, 72) | Two `AgentKind::Omp` arms. |
| Doctrine | `src/agent/doctrine.rs` (~line 101) | Add `Omp` to `include_superpowers`. |
| Status | `src/agent/status/mod.rs` (~line 57) | Falls through to `NoopStatus`; extend the "other agents" test to cover `Omp`. |
| Theme color | `src/ui/theme.rs` (~lines 8-11, 362) | New `AGENT_OMP` const + `agent_style` arm. |
| ⚠ Tab cycle | `src/app/input.rs` (~line 1402) | Insert `Codex → Omp → Claude`. |
| ⚠ CLI `--agent` validation | `src/cli.rs` (~line 997) | Replace the hardcoded `a != "pi" && …` chain with `AgentKind::ALL`. |
| ⚠ CLI help copy | `src/cli.rs` (~lines 61, 958, 985) | Add `omp` to the three blurb/usage/error strings. |
| ⚠ Docs | `docs/book/src/configuration/coding-agents.md`, `.../environment-variables.md`, `README.md`, `skills/wsx/SKILL.md` | Document `--agent omp`, `WSX_OMP_BIN`, `WSX_OMP_MODEL`, and the free-skills/commands behaviour. |

`src/agent/skill.rs` is deliberately **absent** from this list — see
[What omp gives us for free](#what-omp-gives-us-for-free).

### Incidental improvement

`--agent` validation at `src/cli.rs:997` is a hand-maintained
`a != "pi" && a != "claude" && a != "hermes" && a != "codex"` chain, while
`agent add` two hundred lines away already validates against `AgentKind::ALL`
with a comment explaining why ("so this can't drift"). Adding a fifth agent is
the moment that chain would silently reject a valid kind, so this change
converts it to the `ALL`-driven form and derives the error message from
`display_name()`. Scoped to the site being touched; no wider CLI refactor.

## Testing

Following the Codex precedent — argv assertions per spawn mode, real-capture
fixtures for anything that reads a terminal, and unit tests for path arithmetic.

**Command builder** (`src/pty/command.rs`, co-located):
- Fresh is bare `omp` with no approval flags.
- Fresh + yolo emits `--approval-mode yolo`.
- Continue emits `-c`.
- `WSX_OMP_MODEL` emits `--model <value>`; empty/whitespace is treated as unset
  (the existing Pi tests cover this trap — `export FOO=$UNSET` yields `""`).
- Additional dirs emit one `--add-dir` per path.
- Doctrine + rename prompt + custom instructions arrive as a single
  `--append-system-prompt` with parts joined by `\n\n`.
- A Fresh spawn with no injectable content omits the flag entirely.

**Encoding** (`src/activity/omp_events.rs`):
- home scope, including the bare-`$HOME` `-` case.
- tmp scope, including the bare-`/tmp` `-tmp` case.
- abs scope producing `--srv-code-x--`.
- Home takes precedence when `$HOME` is itself under the tmp root.

**Parser compatibility** (`src/activity/omp_events.rs`):
A fixture built from **real omp JSONL lines** — captured from a live session,
not hand-written — asserted through `pi_events::tail_session` to produce the
expected assistant text, tool-call names, and stop reason. This is the tripwire
for the schema-sharing bet: if omp diverges from pi, this test fails rather than
the dashboard quietly going blank.

**Boot behaviour** (`src/pty/session.rs`):
`omp_is_not_ready_mid_boot_and_is_ready_once_its_composer_paints`, replaying
`omp-preboot.bin` then `omp-composer.bin` through `vt100::Parser`, exactly as
the Claude / Codex / Pi tests do.

**Session bleed** (`src/pty/session_detect.rs`): mirroring the existing Pi test —
a worktree whose snapshot names an omp session file reports no prior session, and
a file created *after* the snapshot does. This is the regression test for slug
recycling.

**Taxonomy**: the existing `AgentKind::ALL.len()` assertion moves 4 → 5, and the
agent-picker test grows an entry.

## Delivery

Seven commits on this branch, each independently reviewable and each leaving the
crate compiling and green:

1. Enum variant plus every mechanical match site — compiles and spawns a bare
   `omp`.
2. `build_omp_command` — the full spawn-mode → CLI mapping, with argv tests.
3. `omp_events` — cwd encoding and session location.
4. Prior-session detection, the session-bleed snapshot, and background tailing.
5. Boot capture, `omp_ready`, and the `submit_writes` determination.
6. Doctrine, theme color, Tab cycle, and CLI validation.
7. Documentation.

Commit 1 deliberately leaves three placeholders (`ready_for_input`,
`has_prior_session_for`, the two `background.rs` arms) so every intermediate
commit builds. Commits 4 and 5 remove them. The branch is not shippable before
commit 5: an unguarded `ready_for_input` loses injected messages silently.

The implementation plan is
`docs/superpowers/plans/2026-08-21-oh-my-pi-support.md`.
