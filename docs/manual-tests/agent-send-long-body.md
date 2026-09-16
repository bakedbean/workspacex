# Manual test — Long `wsx agent send` bodies reach Claude whole

Guards the byte shape `submit_writes` writes for Claude (`src/pty/session.rs`)
against the input layer of whatever Claude Code version is installed. The
automated test (`long_claude_delivery_arrives_whole_banner_first`) proves the
framing reaches the PTY intact; only a live Claude can prove its tokenizer
still accumulates the framed body into one paste and still reads the trailing
CR as submit.

Background: the PTY hands Claude Code a multi-KB body as several ~1 KB reads
in one burst. Unframed, Claude Code 2.1.273 kept only the last read — a
3277-byte body arrived as its final 211 bytes with no banner.

Setup: two workspaces on any repo, one `claude` and one `codex` (or a second
`claude`), with the `wsx` dashboard open so deliveries are injected. Attach
the claude one and wait for its composer.

## 1. Short body

From the sender workspace:

```bash
wsx agent send --workspace <repo>/<claude-slug> primary "ping: reply with the word pong"
```

Expect, in the claude session: a user turn reading `[message from
<repo>/<slug> primary]` on its first line, `ping: …` on the second, and the
agent replies. The message must submit on its own — nothing left sitting in
the composer.

## 2. Multi-KB, multi-paragraph body

```bash
body=$(for i in $(seq 1 14); do printf 'PARA%02d: %s\n\n' "$i" "$(printf 'w%02d ' $(seq 1 60))"; done)
wsx agent send --workspace <repo>/<claude-slug> primary "Reply with the first 8 characters and the last 8 characters of this message, and how many PARA lines you see.

$body
END-OF-MESSAGE"
```

Expect: the recorded user turn starts with the `[message from …]` banner and
ends with `END-OF-MESSAGE`, and the reply reports 14 PARA lines. Confirm from
the transcript rather than the screen — the composer may show the body as a
`[Pasted text #1 +N lines]` placeholder, which is fine:

```bash
f=$(ls -t ~/.claude/projects/<encoded-worktree-path>/*.jsonl | head -1)
grep -o '"type":"user"[^}]*' "$f" | head -c 300
```

A reply naming only the last paragraph, or a turn that starts mid-sentence,
is the regression.

## 3. Delivery while the agent is mid-turn

Give the claude agent a slow task (`run sleep 20 in the shell`), then send the
body from step 2 while it is running. Expect the same whole banner-first user
turn once the agent picks the queued message up.

If step 2 or 3 fails on a newer Claude Code, the doc comment on
`submit_writes` records what was measured and why the bracketed shape was
chosen; re-measure before changing the shape.
