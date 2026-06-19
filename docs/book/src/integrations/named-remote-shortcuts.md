```
wsx remote                 # list configured names (alphabetized), one per line
wsx remote <name>          # exec the stored command — process-replaces wsx
wsx config edit remotes    # opens $EDITOR on the blob
```

Stores frequently-used remote shell commands — typically `ssh -t host '…tmux attach…'` for reattaching a wsx session running on another machine (see [Remote access](remote-access.md)) — under short names. The value is an arbitrary shell command run through `sh -c`, so nested quoting works as you'd type it at a terminal.

The `remotes` setting is a newline-separated blob, one `name=command` per line. **There is no `wsx remote add`** — `wsx config edit remotes` opens the existing blob in `$EDITOR`, and you add a remote by appending a new line. Clearing the buffer and typing only the new line replaces every other remote, so always keep the existing lines unless you mean to drop them. Example:

```
ebenmini=ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'"
gpu=ssh gpu-box -t 'tmux -u attach -t main || tmux -u new -s main'
```

Parser rules: only the **first** `=` separates name from command (so `=` inside the command, e.g. an inline env-var, is preserved); whitespace around `=` is trimmed; blank lines are skipped; lines with an empty name or command are dropped; duplicate names take the last value.

`wsx remote <name>` `exec`-replaces the wsx process with `sh -c <command>`, so signals and TTY state flow straight through to the remote session; when it exits you're back at your local shell with no wsx parent process. Unknown names error out with the list of available names.
