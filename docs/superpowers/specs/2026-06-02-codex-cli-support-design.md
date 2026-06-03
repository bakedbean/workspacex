# Codex CLI support

Add OpenAI's **Codex CLI** as a fourth coding agent in wsx, alongside Claude,
Pi, and Hermes. A Codex workspace should be a first-class peer: spawnable,
attachable, resumable, status-tracked, selectable in the agent picker, and
driving the dashboard detail bar with real session activity.

Target: Codex CLI `0.136.0` (the version installed on the dev machine).

## Background: how Codex differs from the existing three

The central abstraction is `AgentKind` in `src/pty/session.rs` (variants
`Claude`, `Pi`, `Hermes`). Each agent supplies: a binary name + `WSX_*_BIN`
override, a `build_*_command` that maps `SpawnMode` → CLI invocation, a
session-detection function, and an activity-events parser
(`src/activity/*_events.rs`). Codex slots into all of these.

Where Codex lands relative to the others:

| Concern | Claude | Pi | Hermes | **Codex** |
|---|---|---|---|---|
| Instruction injection | `--append-system-prompt` | `--append-system-prompt` | `AGENTS.md` block | **`AGENTS.md` block** |
| Per-worktree continue | `--continue` | `--continue` | `--resume <id>` (marker) | **`codex resume --last`** (cwd-filtered natively) |
| Session storage | `~/.claude/projects/<enc-cwd>/*.jsonl` | `~/.pi/agent/sessions/--<enc-cwd>--/*.jsonl` | `~/.hermes/state.db` (sqlite) | **`~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`** (cwd inside file) |
| Activity parser | `events.rs` (JSONL) | `pi_events.rs` (JSONL) | `hermes_events.rs` (sqlite) | **`codex_events.rs`** (JSONL) |

Two facts drive the design:

1. **Codex has no `--append-system-prompt`.** Like Hermes, it reads project
   instructions from `AGENTS.md`. wsx already has a generic, agent-neutral
   AGENTS.md mechanism (`write_agents_md_section`, `compose_injected_prompt`,
   `ensure_git_exclude` — the markers read `wsx-managed`, not `hermes`). Codex
   reuses it directly.

2. **Codex's `codex resume --last` is cwd-filtered by default** (`--all`
   "disables cwd filtering"). The per-worktree continue semantics wsx needs
   come for free — no marker/sqlite scheme like Hermes required.

## CLI mapping

Binary: `codex` (override `WSX_CODEX_BIN`). All flags below are accepted by
both the bare `codex` invocation and the `codex resume` subcommand (verified
against `codex resume --help` on 0.136.0), so the flag-assembly logic is shared
across spawn modes.

| `SpawnMode` | Invocation |
|---|---|
| `Fresh` | `codex` |
| `Continue` | `codex resume --last` |
| `ProjectManager { resume: false }` | `codex` |
| `ProjectManager { resume: true }` | `codex resume --last` |

Shared flags appended after the subcommand tokens:

- **Model**: `WSX_CODEX_MODEL` (trimmed, non-empty) → `-m <model>`. No provider
  env var in this cut (Codex provider selection lives in `~/.codex/config.toml`
  / `--oss`; out of scope).
- **Approval/sandbox**, by mode:
  - Dev workspace, **yolo** → `--dangerously-bypass-approvals-and-sandbox`.
  - Dev workspace, **non-yolo** → *no flags* (bare Codex defaults: interactive
    approvals + `workspace-write` sandbox). Mirrors Claude's non-yolo behavior
    of keeping its normal permission prompts. (Decision confirmed during
    brainstorming.)
  - **ProjectManager** → `--ask-for-approval never --sandbox read-only`. PM only
    reads (`git status/log/diff`, file reads) and must not prompt; read-only
    sandbox + never-ask is the Codex equivalent of Claude PM's
    `--dangerously-skip-permissions`.
- **Remote control**: no-op for Codex (wsx's `RemoteOpts` targets Claude's
  `--remote-control`; Codex's unrelated `--remote` connects to an app server).
  Same stance as Pi/Hermes.

Instruction injection (doctrine + rename hint + custom instructions + PM
prompt) is **not** a flag — it goes through `AGENTS.md` via a new
`prepare_codex_workspace(cwd, &mode)`, a thin wrapper that calls the existing
`compose_injected_prompt` + `write_agents_md_section` + `ensure_git_exclude`.
Unlike `prepare_hermes_workspace`, it writes **no** spawn-timestamp marker —
Codex session detection is cwd-in-file, not marker-based.

The rename-system-prompt text is identical to the Pi/Hermes variant (plain
bash, no wsx-level permission pre-auth), so `compose_injected_prompt` is shared
unchanged.

### Doctrine: superpowers excluded

`process_doctrine` includes the "use superpowers skills" clause for Claude and
Pi, excludes it for Hermes. Codex is **excluded** too: the superpowers skills
are installed as Claude Code plugins under `~/.claude` and Codex can't load
them, so injecting that clause would point Codex at tooling it doesn't have.
(One-line flip in `doctrine.rs` if that changes later.)

## Session detection & location

Codex rollout files are date-partitioned, with the originating directory stored
**inside** the file: the first JSONL line is
`{"type":"session_meta","payload":{...,"cwd":"<abs path>",...}}`. There is no
per-cwd directory to stat, so locating "this worktree's session" is a hybrid of
Pi (newest JSONL, byte-offset tail) and Hermes (match-by-cwd-content).

`locate_session_file(worktree) -> Option<PathBuf>` (in `codex_events.rs`):

1. Canonicalize `worktree`.
2. Walk `~/.codex/sessions/**/rollout-*.jsonl`, collect candidates with mtime.
3. Sort by mtime descending; read **only the first line** of each, parse
   `session_meta.payload.cwd`, return the first whose `cwd` equals the
   canonical worktree.
4. Bound the scan (cap at the N most-recent files, e.g. 500) so a long history
   can't make the 2s dashboard poll pathological. Log if the cap is hit.

`has_prior_codex_session(worktree)` = `locate_session_file(worktree).is_some()`,
wired into `has_prior_session_for`. This is what gates Fresh-vs-Continue, so
`codex resume --last` is only ever issued when a matching session exists.

PM dossier `compute_session_log_dir` (in `pm.rs`): return the
unsupported-marker path, same as Hermes — Codex has no single per-cwd log dir
for PM-Claude to tail. (A Codex *workspace* still gets full detail-bar activity
via `codex_events`; this only affects the PM dossier's session-tail field.)

## Activity parser: `codex_events.rs`

Produces the same `TailUpdate` the dashboard consumes, tailing the located
rollout JSONL by byte offset (append-only, like `pi_events`; on file shrink →
`reset_from_zero`). Codex emits two parallel streams; we map a chosen subset to
avoid double-counting:

| Codex line | → wsx |
|---|---|
| `event_msg/user_message` (`payload.message`) | real human turn → `first_user_text`, `human_replied_after_last_stop`, user `EventSnapshot` |
| `event_msg/agent_message` (`payload.message`) | assistant narration → `last_assistant_text`, `longest_assistant_text_in_batch`, assistant `EventSnapshot` |
| `event_msg/task_complete` (`payload.last_agent_message`) | end-of-turn → `last_stop_reason = end_turn`, final assistant recap |
| `event_msg/task_started` | turn boundary `EventSnapshot` |
| `response_item/function_call` (`call_id`, `name`, ts) | `tool_use_starts`, `tool_use_counts`, tool `EventSnapshot` |
| `response_item/function_call_output` (`call_id`) | `tool_use_resolves` |
| `response_item/message` (any role) | **ignored** — assistant copies duplicate `agent_message`; user/developer copies are synthetic `<environment_context>`/permissions context, not real input |
| `response_item/reasoning` | **ignored** — encrypted, no displayable text |
| `event_msg/token_count`, `turn_context` | **ignored** |

`edited_file_paths` is **best-effort / empty** in this cut: Codex edits via
sandboxed `exec_command` shell calls (and an `apply_patch` tool), and inferring
touched paths from arbitrary shell is unreliable. The field stays empty; the
RECENT FILES detail module simply shows nothing for Codex until a follow-up
adds `apply_patch` parsing. This is the one explicit parity gap, called out so
it isn't mistaken for "covered."

Stop-reason model: Codex's turn lifecycle is `task_started` → … →
`task_complete`. We map `task_complete` to the `end_turn` stop reason. Codex has
no Claude-style `tool_use`/`max_tokens` stop variants exposed in the rollout, so
the classifier sees turns as running until a `task_complete` lands — matching
how the idle/working dashboard state should read for Codex.

## Touchpoints (all mechanical except `codex_events.rs`)

`src/pty/session.rs`: enum variant; `ALL` → `[AgentKind; 4]`; `resolved_binary`
(`WSX_CODEX_BIN`); `from_str_or_default` (`"codex"`); `display_name`/
`default_binary`/`store_value` (`"codex"`); `has_prior_session_for` arm;
`spawn_session` arm (`prepare_codex_workspace` + `build_codex_command`); new
`build_codex_command`, `has_prior_codex_session`, `prepare_codex_workspace`.

`src/activity/mod.rs`: `pub mod codex_events;`. `src/activity/codex_events.rs`:
new parser. `src/app/background.rs`: `locate_session_file` + `tail_session`
dispatch arms.

`src/agent/pm.rs`: `compute_session_log_dir` arm (unsupported marker).
`src/agent/doctrine.rs`: leave `include_superpowers` excluding Codex; add a
Codex doctrine test.

`src/cli.rs`: accept `"codex"` in `--agent` validation + help/error strings.
`src/app/input.rs`: Tab-cycle arm (`Hermes → Codex → Claude`).

Tests: `ALL.len()` 3 → 4 and Codex assertions in `session.rs`; agent-picker
"three agents" → "four" in `input_tests.rs`; `codex_events` unit tests over a
fixture rollout JSONL; doctrine test.

`README.md`: agent overview, `coding_agent` config values, agents table row,
Codex spawn/session/env-var section, `--agent` help, `WSX_CODEX_BIN` /
`WSX_CODEX_MODEL` env-var rows.

## Testing strategy

- **Unit (pure)**: `from_str_or_default("codex")`, `display_name`, `ALL`
  membership, `build_codex_command` arg assembly per mode (Fresh / Continue /
  PM / yolo / model env var) asserting on the `CommandBuilder` argv, via the
  `WSX_CODEX_BIN` seam + `EnvGuard`.
- **`codex_events` parser**: check in a small fixture `rollout-*.jsonl` under
  `tests/` (or an inline string), assert `TailUpdate` fields: tool start/resolve
  pairing, assistant/user text, `task_complete` → stop reason, byte-offset
  advance, and shrink → `reset_from_zero`.
- **`locate_session_file`**: temp `~/.codex/sessions/…` with two rollout files
  for different cwds; assert the matching-cwd newest one is returned and a
  non-matching cwd yields `None`.
- **Spawn smoke**: reuse the `cat`-wrapper pattern (`WSX_CODEX_BIN` → stub) to
  confirm `spawn_session` launches for `AgentKind::Codex`.

## Out of scope (this cut)

- Codex provider/`--oss` selection env vars.
- `edited_file_paths` extraction (RECENT FILES blank for Codex).
- PM dossier session-tail for Codex workspaces (unsupported marker, as Hermes).
- Codex's experimental `--remote`/remote-control integration.
