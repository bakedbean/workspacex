# Named remotes — `wsx remote <name>` — Design

## Goal

Let the user store frequently-used remote shell commands (typically `ssh -t host '…tmux attach…'`) under short names and run them with `wsx remote <name>`. The motivating example:

```
wsx remote ebenmini
# expands to and execs:
ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'"
```

The stored value is an arbitrary shell command. There is nothing SSH-specific about the storage or execution path — the feature is "named command runner that exec-replaces wsx". SSH-into-tmux is the dominant use case, which is why the value is run through `sh -c` (preserves the nested quoting users naturally write).

## Approach

Reuse the existing `pinned_commands` storage and parser shape: one row in the `settings` table at key `remotes`, value is a newline-separated `name=command` blob. Three new CLI surfaces. No new table, no migration, no new dependencies.

Execution exec-replaces the wsx process on Unix so the user lands directly inside the remote session with full TTY pass-through.

## Decisions

### CLI surface

```
wsx remote                 # list configured names (sorted), one per line
wsx remote <name>          # exec the stored command for <name>
wsx config edit remotes    # opens $EDITOR on the blob (already works via existing config edit)
```

`wsx config get|set|list remotes` work automatically once `"remotes"` is added to `known_setting_key` in `src/cli.rs`.

No `wsx remote add/remove/list` subcommands — list is the no-arg form; add/remove happen by editing the blob. This matches the established wsx pattern for free-text user configuration (`pinned_commands`, `custom_instructions`, `setup_script`).

### Storage

Existing `settings` table, key `remotes`, value example:

```
ebenmini=ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'"
gpu=ssh gpu-box -t 'tmux -u attach -t main || tmux -u new -s main'
```

Parser rules (identical to `src/pinned.rs::parse`):

- Split on the **first** `=` only — `=` inside the command is preserved verbatim.
- Both sides trimmed of surrounding whitespace.
- Blank lines skipped.
- Lines with an empty name or empty command are dropped.
- Order in the blob is preserved; the runtime list is alphabetized for display.

Duplicate names: last write wins (last matching line in the blob). No warning — matches `pinned_commands` behavior.

### Execution

`wsx remote <name>` resolves the command string, then on Unix calls `std::os::unix::process::CommandExt::exec` on:

```rust
Command::new("sh").arg("-c").arg(&command)
```

`exec` replaces the wsx process image with `sh`, so:

- No parent zombie or intermediate fork.
- Signals (Ctrl-C, SIGWINCH, etc.) flow directly to `ssh`.
- When the remote session exits, the user is back at their local shell, exactly as if they'd typed the ssh command themselves.

If `exec` returns at all, it's because spawning `sh` failed (e.g. `sh` missing on PATH). In that case, surface the `io::Error` and exit non-zero.

Rationale for `sh -c` over manual argv splitting: the example value contains nested single + double quotes (`"zsh -lc 'tmux attach'"`). Letting the shell parse the value means users paste the same string they'd type at a terminal — no escape gymnastics, no surprises.

Non-Unix platforms: wsx is Linux/macOS only today (the rest of the codebase assumes Unix PTYs, `nix`, etc.), so this design is Unix-only without a `cfg` fallback.

### Error paths

- **`wsx remote <name>` for an unknown name:**
  ```
  no remote named 'foo'
  available: ebenmini, gpu
  ```
  Exit code: non-zero (existing `Error::UserInput` path).
- **`wsx remote <name>` with no remotes configured:**
  ```
  no remotes configured. add one with: wsx config edit remotes
  ```
- **`wsx remote` (list) with no remotes configured:** prints the same hint as above, exit 0.
- **Empty `<name>` arg** (`wsx remote ""`): treated as unknown, same as above.

### Module layout

- **New file `src/remotes.rs`** — `pub fn parse(text: &str) -> Vec<Remote>`, `pub fn list(store: &Store) -> Result<Vec<Remote>>`, `pub fn lookup(store: &Store, name: &str) -> Result<Option<String>>`. `Remote` is `{ name: String, command: String }`.
- **Rename `src/remote.rs` → `src/remote_control.rs`**. The module only exposes `RemoteOpts`, `enabled()`, `sandbox_enabled()`; updating `use` sites is mechanical. This frees the `remote` name for the new CLI concept and makes the existing module's purpose clearer (it's specifically about claude's `--remote-control` flag).
- **`src/cli.rs`** — two new `CliAction` variants: `RemoteList`, `RemoteRun { name }`. Parse the `remote` subcommand. Dispatch in `run_cli`. Add `"remotes"` to `known_setting_key`.
- **`src/lib.rs`** — re-export `remotes` module; rename `remote` → `remote_control` re-export.

### Why exec lives in `cli.rs` (not `remotes.rs`)

`remotes.rs` stays pure data + lookup. Process replacement is a side-effecting CLI concern; it lives in the `CliAction::RemoteRun` arm of `run_cli`. This keeps `remotes.rs` trivially unit-testable.

### Tests

- **`src/remotes.rs`:** parser tests mirroring `src/pinned.rs::tests` — labeled lines, blank-line skip, first-`=` split semantics, dropped-when-empty rules. Lookup test against `Store::open_in_memory()`. Alphabetization test for `list`.
- **`src/cli.rs`:** arg-parsing tests for `parse(&["remote"])` → `RemoteList`, `parse(&["remote", "foo"])` → `RemoteRun { name: "foo" }`. `known_setting_key("remotes")` accepts.
- **No test for `exec` itself** — process replacement is not unit-testable. Verified via manual smoke (documented in `docs/manual-tests/`).

### Manual smoke test

Add `docs/manual-tests/named-remotes.md`:

1. `wsx config set remotes "demo=echo hello && sleep 1"`
2. `wsx remote` → prints `demo`.
3. `wsx remote demo` → prints `hello`, sleeps 1s, returns to local shell with exit 0. wsx process is gone (verify with `ps`).
4. `wsx remote nope` → prints unknown-name error with `available: demo`, exits non-zero.
5. `wsx config edit remotes` → adds a second line; both names appear in `wsx remote`.
6. Real-world: `wsx config set remotes "self=ssh -t localhost 'tmux new -s wsx-test || tmux attach -t wsx-test'"`; `wsx remote self` lands in tmux; `Ctrl-b d` then `exit` returns to local shell.

## Out of scope

- `wsx remote add/remove/list` subcommands (use `wsx config edit remotes`).
- Structured fields (host, user, flags) — value is a raw shell command on purpose.
- Shell completion for remote names — easy follow-up if useful.
- A "default" remote / `wsx remote` (no arg) connecting to a default — current no-arg form is `list`.
- Per-repo remote overrides — remotes are global; they're machines, not project state.
- Anything Windows-specific.
