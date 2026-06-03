# CLI `--help` and `--version`

Add a discoverable help system to the `wsx` CLI. Today there is **no**
`--help`, `-h`, `help`, or `--version` — the only usage hints are one-line
error strings scattered through the parser, and a bad invocation prints an ugly
`Error: UserInput("agent send <label> <prompt>")` Debug dump. After this change:

- `wsx --help` / `wsx -h` / `wsx help` print top-level help (all command groups).
- `wsx <group> --help` / `wsx help <group>` print that group's commands (e.g.
  `wsx agent --help`).
- `wsx --version` / `wsx -V` print the crate version.
- A misused command prints `error: <what went wrong>` followed by that group's
  help block, to stderr, exit code 2.
- Bare `wsx` still launches the TUI — unchanged.

The help text is generated from a **single static registry** that the parser
also references, so help can never drift from the real commands.

## Background: the current parser

`src/cli.rs` (1584 lines) hand-rolls argument parsing — no `clap` or any arg
library, and we are deliberately keeping it that way (the table-driven approach
was chosen over adopting clap). `parse_args(Vec<String>) -> Result<CliAction>`
is one giant top-level `match` on the first token (`repo`, `config`, `remote`,
`workspace`, `agent`, `setup`), each arm an inner `match` on the subcommand.
Leaves return a `CliAction` variant or `Error::UserInput(String)` with a
one-line usage hint. `None` (bare `wsx`) returns `CliAction::Tui`.

`main()` (`src/main.rs:38`) calls `parse_args`, and for any non-`Tui` action
calls `run_cli`. Errors propagate up through `main() -> Result<()>`, so today
they surface via Rust's default `Termination` as a `Debug` dump of the `Error`
enum — not a clean message.

`Error` (`src/error.rs`) is a `thiserror` enum; usage problems currently use the
catch-all `UserInput(String)` variant.

## Design

### 1. Command registry — the single source of truth

A static description of the command tree lives in `cli.rs`:

```rust
struct CmdInfo {
    usage: &'static str,   // e.g. "send <label> <message...>"
    blurb: &'static str,   // e.g. "Queue an async message to a peer agent"
}
struct GroupInfo {
    name: &'static str,    // "agent"
    blurb: &'static str,   // "List, add, and message agents in a workspace"
    commands: &'static [CmdInfo],
}
static GROUPS: &[GroupInfo] = &[ /* workspace, agent, repo, config, remote, setup */ ];
```

Both the `--help` output **and** the misuse-error output render from `GROUPS`.
Usage strings live here once; the parser no longer carries duplicated full-usage
hint strings.

**Scope of the registry:** it documents the user-facing command groups —
`workspace`, `agent`, `repo`, `config`, `remote`, `setup`. Commands documented
elsewhere that are thin wrappers (e.g. `setup install-skill`) get a single
entry. The `Tui` (no-arg) behavior is noted in the top-level USAGE line, not as
a command.

### 2. New `CliAction` variants and `Error` variant

```rust
// cli.rs
enum HelpTopic { Root, Group(&'static str) }
enum CliAction {
    // …existing…
    Help(HelpTopic),
    Version,
}

// error.rs
enum Error {
    // …existing…
    #[error("{msg}")]
    Usage { group: Option<&'static str>, msg: String },
}
```

`Error::Usage` carries only a `group` *name* (a `&'static str` tag), never a
reference to CLI types — `error.rs` stays free of CLI logic. The CLI layer
interprets the tag to pick the help block. `group: None` means the misuse was at
the top level (unknown command) → top-level help.

`Help` and `Version` are normal successful actions handled in `run_cli`,
printing to **stdout** and returning `Ok(())` (exit 0).

### 3. Parser changes (`parse_args`)

**Trigger detection.** A token is a help request if it equals `--help`, `-h`, or
`help`; a version request if `--version` or `-V`.

- First token is a help/version trigger → `Help(Root)` / `Version`.
- First token is a known group, and a help trigger appears anywhere in that
  group's args (`wsx agent --help`) → `Help(Group("agent"))`.
- `wsx help <group>` → `Help(Group("<group>"))`; `wsx help <unknown>` →
  `Help(Root)` (be lenient — show everything rather than erroring on a help
  request).
- Bare `wsx` → `Tui` (unchanged).

**Resolving runtime strings to static names.** Both `HelpTopic::Group` and
`Error::Usage.group` hold `&'static str`, but argv tokens are owned `String`s. A
single helper bridges them:

```rust
fn group_name(s: &str) -> Option<&'static str> {
    GROUPS.iter().map(|g| g.name).find(|&n| n == s)
}
```

It returns the registry's static name for a known group, else `None`. This is
used for help-topic resolution (`wsx help agent` → `Group("agent")`;
`wsx help bogus` → `Root`) **and** for error tagging (below) — so a misuse only
gets tagged with a group that actually exists.

**Per-group extraction.** Each top-level arm's body moves into its own function:
`parse_repo`, `parse_config`, `parse_remote`, `parse_workspace`, `parse_agent`,
`parse_setup`, each `(&mut Args) -> Result<CliAction>`. The top-level dispatcher
becomes:

```rust
let first = it.next();
if is_help(first.as_deref()) { return Ok(Help(Root)); }
if is_version(first.as_deref()) { return Ok(Version); }
let action = match first.as_deref() {
    None              => Ok(Tui),
    Some("repo")      => parse_repo(&mut it),
    Some("agent")     => parse_agent(&mut it),
    /* … */
    Some("help")      => return Ok(help_topic_from(it.next())),
    Some(other)       => Err(Error::Usage { group: None,
                              msg: format!("unknown command: {other}") }),
};
// Tag any untagged Usage error with the group we dispatched into.
// tag_group sets `group` only when it is currently None AND `first`
// resolves via group_name() to a real group — so `wsx bogus` stays None.
action.map_err(|e| tag_group(e, first.as_deref()))
```

This both enables per-group misuse help **and** breaks the 1584-line giant
`match` into navigable, independently-testable units. This is the targeted
cleanup that serves the feature (the file has grown too large); we are *not*
doing unrelated refactoring beyond the extraction the error-tagging requires.

**Leaf error messages** shrink from full-usage hints to short "what went wrong"
messages, returned as `Error::Usage { group: None, msg }` (the dispatcher tags
the group). Examples: `"missing arguments"`, `"--name needs a value"`,
`"unknown setting key: <k>"`. Each group parser may set `group` explicitly when
it knows it (e.g. a sub-group), but the default tag-on-the-way-out covers the
common case.

### 4. Rendering + `main` wiring

Rendering helpers in `cli.rs`, all returning `String` so they are unit-testable
without capturing stdout/stderr:

- `render_root_help() -> String` — USAGE line + group table + footer
  ("Run `wsx <command> --help` for command details").
- `render_group_help(name) -> String` — group blurb + its commands (usage +
  blurb), from `GROUPS`.
- `render_usage_error(group: Option<&str>, msg: &str) -> String` —
  `error: {msg}\n\n{group-or-root help block}`.

`run_cli` handles `Help(topic)` → print the rendered help to **stdout**, exit 0;
`Version` → print `wsx {CARGO_PKG_VERSION}` to stdout, exit 0.

`main` is adjusted so a `Usage` error from the CLI path prints
`render_usage_error(...)` to **stderr** and exits with code **2**, rather than
the current Debug dump. Other (non-`Usage`) errors keep their existing
surfacing. The TUI path is untouched.

### 5. Help granularity (explicit YAGNI boundary)

Help stops at the **group** level. `wsx agent send --help` shows the **agent
group** block (which includes the `send <label> <message...>` line), not a
dedicated per-command page. There is no per-command help renderer. This matches
the agreed trigger set (`wsx <group> --help`) and keeps the registry flat.

### 6. Output shape

Top-level (`wsx --help`):

```
wsx — git-worktree workspace manager

USAGE:
  wsx [COMMAND]            (no command launches the TUI)

COMMANDS:
  workspace   Create, list, rename, and archive workspaces
  agent       List, add, and message agents in a workspace
  repo        Register and configure repositories
  config      Get and set global settings
  remote      Run saved remote shortcuts
  setup       One-off setup helpers (skill install)

Run `wsx <command> --help` for command details.
```

Group (`wsx agent --help`):

```
wsx agent — List, add, and message agents in a workspace

USAGE:
  wsx agent <command> [args]

COMMANDS:
  list                          Show agents in the current workspace
  add <kind>                    Attach an agent (claude|pi|hermes|codex)
  send <label> <message...>     Queue an async message to a peer agent
```

Misuse (`wsx agent send`):

```
error: missing arguments

wsx agent — List, add, and message agents in a workspace

USAGE:
  wsx agent <command> [args]

COMMANDS:
  list                          Show agents in the current workspace
  add <kind>                    Attach an agent (claude|pi|hermes|codex)
  send <label> <message...>     Queue an async message to a peer agent
```

## Testing

Pure parse/registry/render tests in `cli.rs` (no TUI, no store):

- **Triggers** — `parse_args` maps each form to the right action:
  `wsx --help` / `-h` / `help` → `Help(Root)`; `wsx agent --help` /
  `wsx help agent` → `Help(Group("agent"))`; `wsx --version` / `-V` → `Version`;
  bare `wsx` → `Tui` (regression guard).
- **Misuse tagging** — `wsx agent send` (missing args) → `Err(Usage { group:
  Some("agent"), .. })`; `wsx bogus` → `Err(Usage { group: None, .. })`.
- **Registry/parser completeness invariant** — every group name dispatched in
  `parse_args` has a `GROUPS` entry, and every `GROUPS` entry name is a group
  the parser dispatches. This is the anti-drift guard.
- **Render content** — `render_group_help("agent")` contains `list`, `add`,
  `send`; `render_root_help()` contains every group name;
  `render_usage_error(Some("agent"), "missing arguments")` starts with
  `error: missing arguments` and contains the agent block.
- **Existing CLI parse tests** stay green (the per-group extraction must not
  change any accepted invocation).

## Documentation

Close the loop with the docs work already on this branch:

- README "CLI reference" intro: one line — "Run `wsx --help` (or
  `wsx <command> --help`) to list commands and arguments."
- Skill `## CLI surface` block: same one-line pointer.

## Out of scope

- Adopting `clap` or any arg-parsing library.
- Per-command (vs per-group) help pages.
- `--provider`/colorized/`man`-page output.
- Reformatting or renaming existing commands; this change only *adds* help and
  reshapes error output.
