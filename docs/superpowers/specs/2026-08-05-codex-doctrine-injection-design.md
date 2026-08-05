# Codex doctrine injection without AGENTS.md

Deliver wsx's process doctrine, rename hint, and custom instructions to Codex
through Codex's own config channel (`-c developer_instructions=...`) instead of
rewriting a `wsx-managed` block into the worktree's `AGENTS.md`.

Target: Codex CLI `0.146.0` (the version installed on the dev machine). Every
behavioural claim below was verified against that binary; see
[Verification log](#verification-log).

## Background

When Codex support landed
([2026-06-02](2026-06-02-codex-cli-support-design.md)), the design note read:

> **Codex has no `--append-system-prompt`.** Like Hermes, it reads project
> instructions from `AGENTS.md`. wsx already has a generic, agent-neutral
> AGENTS.md mechanism […] Codex reuses it directly.

That was true of `0.136.0` and is the reason `prepare_codex_workspace`
(`src/pty/workspace_prep.rs:194`) rewrites `AGENTS.md`, hides it via
`.git/info/exclude`, and seeds it from `CLAUDE.md` on first creation.

Two things have changed:

1. **Hermes is being deprecated.** The AGENTS.md mechanism was built for Hermes
   and inherited by Codex. Once Hermes is gone, Codex is the only consumer of a
   mechanism that exists solely because Hermes needed it.
2. **Codex `0.146.0` exposes `developer_instructions` and
   `project_doc_fallback_filenames` as config keys**, both settable per-spawn
   via the `-c` flag wsx already emits for `notify` status wiring
   (`src/pty/command.rs:503-512`).

The current mechanism has real costs, independent of Hermes:

- It **mutates the user's worktree**. Every Codex spawn writes a file the user
  did not ask for, and a second file (`.git/info/exclude`) to hide the first.
- It **conflicts with a real `AGENTS.md`**. wsx must merge into, and later strip
  from, a file the repo may legitimately own. `strip_wsx_block` exists only to
  undo wsx's own edit, including a malformed-block recovery path.
- It **delivers doctrine at the wrong authority level**. Codex injects project
  docs as a *user*-role message. `developer_instructions` is a *developer*-role
  message placed ahead of Codex's own instructions.
- The **`CLAUDE.md` copy is one-shot**. It happens only at `AGENTS.md` creation,
  so later edits to `CLAUDE.md` never reach Codex.

## The two channels

Both are `-c` overrides, composing with the existing `notify` wiring.

| Concern | Today | Replacement |
|---|---|---|
| doctrine + rename + custom | `write_agents_md_section(cwd, Some(...))` | `-c developer_instructions=<escaped>` |
| repo `CLAUDE.md` for Codex | seed `AGENTS.md` on first create | `-c project_doc_fallback_filenames=["CLAUDE.md"]` |
| hide wsx's file from git | `ensure_git_exclude(cwd, "AGENTS.md")` | *nothing — no file is written* |
| undo on Continue | `write_agents_md_section(cwd, None)` | *nothing — nothing to undo* |

`project_doc_fallback_filenames` is a true fallback: when a repo has a real
`AGENTS.md`, that file wins and `CLAUDE.md` is not read, so there is no
double-delivery. It is also strictly better than the copy it replaces, because
it resolves the file per session rather than once at creation.

### Prompt placement

`codex debug prompt-input -c 'developer_instructions="WSX-DOCTRINE-MARKER"'`
renders:

```
0  developer  "WSX-DOCTRINE-MARKER"                     ← wsx doctrine
1  developer  "You are `/root`, the primary agent…"     ← Codex's own instructions
2  developer  "<multi_agent_mode>…"
3  user       "# AGENTS.md instructions for <cwd>…"     ← where doctrine goes today
```

Codex's own guardian policy (embedded in the binary) treats both roles as
trusted: *"Only user and developer messages from the transcript, `AGENTS.md`
files, and responses to the `request_user_input` tool are trusted content."*
The change is not about trust; it is about position and about not writing to
the worktree.

## Design

### 1. `build_codex_command` emits the flags — `Fresh` only

In `src/pty/command.rs:488`, after the existing `notify` wiring and before the
`resume` tokens:

```rust
if matches!(mode, SpawnMode::Fresh { .. }) {
    if let Some(prompt) = compose_injected_prompt(mode) {
        cmd.arg("-c");
        cmd.arg(format!("developer_instructions={}", toml_basic_string(&prompt)));
    }
    cmd.arg("-c");
    cmd.arg(r#"project_doc_fallback_filenames=["CLAUDE.md"]"#);
}
```

(`matches!` rather than `if let`, matching the existing mode check at
`src/pty/command.rs:503`.)

`compose_injected_prompt` is reused unchanged; it already joins doctrine,
rename hint, and custom instructions with blank lines and returns `None` when
all three are absent.

Placement mirrors the `notify` wiring: `-c` is a global flag accepted before any
subcommand, and `Fresh` never emits `resume` tokens anyway.

**`Continue` deliberately passes neither flag.** `codex resume --last` restores
the session's stored config and silently ignores `-c` overrides for both keys —
verified, not assumed. Emitting them would make the argv assert something
untrue and would send the next reader hunting for why doctrine changes do not
take effect on re-attach. The `if let Fresh` plus a doc comment makes the
constraint discoverable from the code.

This is safe because a resumed session already carries the doctrine in its
history from the Fresh spawn that created it.

### 2. `toml_basic_string` — a new private helper

`-c key=value` parses `value` as TOML and falls back to a raw literal only on
*parse failure*. A value that parses as a non-string is a **hard launch error**:

```
$ codex debug prompt-input -c 'developer_instructions=true'
Error: invalid type: boolean `true`, expected a string
```

Custom instructions are user-supplied, so raw interpolation lets a user whose
instruction text is exactly `true`, `123`, or `[1,2]` break their own spawn.
The helper removes the class of failure rather than the instances:

```rust
/// Encode `s` as a TOML basic string (surrounding quotes included) so it can
/// be passed as the value half of a `codex -c key=value` override.
///
/// `-c` parses the value as TOML and only falls back to a raw literal when
/// parsing *fails*; a value that parses as a non-string (`true`, `123`) is a
/// hard launch error. Quoting makes every input parse as a string.
fn toml_basic_string(s: &str) -> String
```

Escapes `\` → `\\`, `"` → `\"`, newline → `\n`, tab → `\t`, carriage return →
`\r`, and any other control character as `\u00XX`. Verified round-trip: a value
containing quotes, a backslash, backticks, and newlines arrives byte-identical
in the developer message.

Pure function, no filesystem, directly unit-testable.

### 3. `prepare_codex_workspace` sheds its file writes

`src/pty/workspace_prep.rs:194` reduces to the command sync:

```rust
/// Prepare a worktree for a Codex spawn: sync Claude slash-commands into
/// Codex's prompt directory. Instruction injection is NOT done here — it goes
/// through `-c developer_instructions` in `build_codex_command`, so the Codex
/// path writes nothing to the worktree.
pub(crate) fn prepare_codex_workspace(_cwd: &Path, _mode: &SpawnMode) {
    #[cfg(not(test))]
    crate::agent::codex_commands::sync_claude_commands_for_codex();
}
```

Per the agreed scope, `write_agents_md_section`, `strip_wsx_block`,
`ensure_git_exclude`, `read_claude_md`, and their tests **stay in place
unchanged** for `prepare_hermes_workspace`. They become dead code when Hermes is
removed; that removal is a separate change.

### 4. Documentation

`docs/book/src/configuration/coding-agents.md` documents the AGENTS.md
mechanism as covering both Hermes and Codex. Update it to describe the `-c`
channel for Codex and scope the AGENTS.md text to Hermes.

## Testing

Argv assertions in `src/pty/command.rs` tests, matching the existing
`codex_*` test style (`EnvGuard` + `codex_argv` helper):

- `codex_fresh_emits_developer_instructions` — doctrine text appears as the
  value of a `-c developer_instructions=` arg.
- `codex_fresh_emits_claude_md_fallback` — the
  `project_doc_fallback_filenames=["CLAUDE.md"]` arg is present.
- `codex_fresh_with_no_injectable_content_omits_developer_instructions` — no
  doctrine, no rename ctx, no custom instructions → no `developer_instructions`
  arg, but the fallback arg is still emitted.
- `codex_continue_omits_instruction_config` — neither arg appears alongside
  `resume --last`.
- `codex_developer_instructions_value_is_quoted_toml` — a `SpawnMode` whose
  custom instructions are the literal `true` yields a value of `"true"`
  (quoted), not bare `true`.

Unit tests for `toml_basic_string`: quotes, backslashes, newlines, tab, a
control character, and empty string.

In `src/pty/workspace_prep.rs`, the two existing Codex tests
(`prepare_codex_workspace_injects_rename_block_into_agents_md`,
`prepare_codex_workspace_writes_no_hermes_marker`) are replaced by
`prepare_codex_workspace_writes_nothing_to_worktree`: given a tempdir with an
initialised `.git/info`, assert no `AGENTS.md` is created and no `AGENTS.md`
line lands in `.git/info/exclude`.

Hermes tests are untouched and must continue to pass — they are the regression
guard proving the shared helpers still work.

## Accepted regressions

**A hand-started Codex session loses doctrine on re-attach.**
`has_prior_session_for` matches any Codex session whose cwd is the worktree,
including one the user started outside wsx. wsx then re-attaches via `Continue`,
which passes no instruction flags, and that session never received doctrine from
a Fresh wsx spawn. Today's AGENTS.md mechanism would have supplied it, because
project docs are re-read every turn. This is the accepted cost of moving from
worktree state to spawn arguments.

**Custom instructions no longer refresh on re-attach.** Related-repo context and
handoff notes change over the life of a workspace; today a `Continue` spawn
rewrites them into `AGENTS.md`. They will now reach only the session that was
Fresh-spawned with them. The doctrine itself is unaffected, since it is already
in the resumed session's history.

**Stale blocks are not cleaned up.** Worktrees that already had a Codex spawn
retain their `BEGIN/END wsx-managed` block in `AGENTS.md`, which will now
duplicate the `-c` channel. The block is self-guarding — its rename clause is
conditioned on the branch still being a placeholder — and the file is excluded
from `git status`, so the effect is redundant tokens rather than wrong
behaviour. A one-time `strip_wsx_block` on Codex spawn would fix it and is a
small follow-up if the duplication proves noticeable.

## Verification log

All against `codex-cli 0.146.0` on macOS.

| Claim | How verified | Result |
|---|---|---|
| `-c developer_instructions` reaches the model | `codex exec` in a dir with no `AGENTS.md`, asked for a passphrase set only via the flag | returned the passphrase |
| Multiline markdown with backticks, quotes, `$` survives | same, with a multi-line doctrine value | delivered intact |
| Additive with a real `AGENTS.md` | dir with both a real `AGENTS.md` and the flag | model reported both facts |
| Lands as first developer message | `codex debug prompt-input` | msg 0, role `developer`, ahead of Codex's own |
| Ignored on `resume --last` | resumed a session passing a *new* fact via the flag | model answered `UNKNOWN` |
| Non-string value is a hard error | `-c developer_instructions=true` / `=123` | `Error: invalid type … expected a string` |
| Quoted basic string round-trips | `-c 'developer_instructions="…\"quotes\"…\\…\n…"'` | byte-identical in msg 0 |
| `project_doc_fallback_filenames` reads `CLAUDE.md` | dir with only `CLAUDE.md` | fact delivered, in the project-doc slot |
| …is a true fallback | added a real `AGENTS.md` alongside | only `AGENTS.md` delivered |
| …also ignored on `resume --last` | fresh vs resumed session, same flag | fresh returned the fact, resumed returned `UNKNOWN` |
| `AGENTS.md` itself *is* re-read on resume | edited `AGENTS.md`, resumed, asked about the edit | returned the new fact |

The last two rows together are the finding that shapes the design: on resume
Codex re-resolves project docs under its *default* filenames but does not apply
`-c` overrides, so both instruction channels are Fresh-only while raw
`AGENTS.md` is not.

## Out of scope

- Removing Hermes, or the shared AGENTS.md helpers it still uses.
- `model_instructions_file` / `instructions`, which replace Codex's base
  instructions rather than adding to them.
- Any change to the Claude or Pi `--append-system-prompt` paths.
