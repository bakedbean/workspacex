# Manual smoke test: named remotes (`wsx remote <name>`)

The automated test suite covers the parser, store lookup, and CLI
arg parsing. This procedure covers what tests can't: process
replacement via `exec`, TTY pass-through, and shell-quoted command
strings reaching `sh -c` unmangled.

## Setup

Build the release binary or use `cargo run --`. Steps below use
`wsx` as shorthand for either.

## Test 1: empty state

```
wsx remote
```

Expected: `no remotes configured. add one with: wsx config edit remotes`

## Test 2: basic exec + return-to-shell

```
wsx config set remotes "demo=echo hello && sleep 1"
wsx remote
# prints: demo
wsx remote demo
# prints: hello (1s pause), exit 0, back at local shell
```

Verify `ps` shows no leftover `wsx` process during the sleep — exec
should have replaced it.

## Test 3: unknown name lists available

```
wsx remote nope
```

Expected: non-zero exit, message includes `no remote named 'nope'.
available: demo`.

## Test 4: nested-quote command (the motivating example)

```
wsx config edit remotes
# add a line like:
#   self=ssh -4 -t localhost "zsh -lc 'tmux new -s wsx-test || tmux attach -t wsx-test'"
wsx remote self
```

Expected:
- Lands inside a tmux session on the remote (`tmux ls` from another
  shell confirms `wsx-test` exists).
- `Ctrl-b d` detaches; ssh exits; you're back at the local shell.
- The nested `"…'…'…"` quoting reached `sh -c` intact — no
  "command not found" or quoting errors.

## Test 5: signal pass-through

```
wsx remote demo  # but with a longer sleep, e.g. "demo=sleep 30"
# press Ctrl-C
```

Expected: Ctrl-C kills `sleep` (not wsx — wsx is already gone),
shell returns immediately with exit 130.

## Cleanup

```
wsx config set remotes ""
```
