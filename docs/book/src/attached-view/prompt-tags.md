# Prompt tags

Anthropic's prompting guidance recommends separating the parts of a prompt
with XML tags — `<context>…</context>`, `<task>…</task>`,
`<constraints>…</constraints>` — so the model can tell them apart. Prompt
tags make that one keystroke from the attached view.

Press `Ctrl-x <` (or click the `<>` chip in the chat footer) to open the
picker:

```
 name: ▏
  1  context                   ×12
  2  task                      ×7
  3  constraints               ×2
 [↑/↓] move   [enter] body   [1-9] pick   [^d] delete   [esc] close
```

- Typing filters the list by prefix. `Enter` on a listed tag opens its
  body box; `Enter` on a name that isn't listed creates it.
- `1`–`9` jump straight to a listed tag while the name field is empty.
- `Ctrl-d` deletes the selected tag.

The body box is a small multi-line editor: `Enter` inserts a newline,
arrows/Home/End move, `Esc` goes back to the picker (keeping your draft).
Tabs are kept as tabs (shown four columns wide). Pasted CRLF line endings
become single newlines; control characters other than newline and tab are
stripped when inserting.
`Ctrl-s` inserts

```
<context>
…your text…
</context>
```

into the agent's composer **without submitting**, so you can stack several
tagged sections and add a plain instruction before pressing Enter yourself.
Each insert bumps the tag's use count; the three most-used tags sit in the
footer as `<context>`-style chips — click one to go straight to its body box.

Tag names are ASCII-only: they must start with a letter or `_` and contain
only letters, digits, `_`, `.` and `-` (`[A-Za-z_][A-Za-z0-9_.-]*`).

If the insert can't be confirmed — no agent in the focused pane, or the
agent has exited or stopped responding — the body box stays open with your
draft and a one-line notice, so nothing you typed is lost.

The list lives in the `prompt_tags` setting, one `name=uses` per line:

```bash
wsx config get prompt_tags
wsx config edit prompt_tags                 # opens $EDITOR on the current value
wsx config set prompt_tags "context=12
task=7"
wsx config set prompt_tags ""               # clear
```

Lines that don't parse (an invalid name, a non-numeric count) are dropped
when the list is read and disappear on the next save.

The footer chips are the `$tags` bar segment — see
[Themes](../configuration/themes.md) to move, restyle, or drop them. The
`Ctrl-x <` chord works even when a theme omits the segment.

If you have your own `~/.config/wsx/theme.toml` with an explicit
`[attached_bottom].format`, the chips only appear once you add `($tags  )`
beside `$pins` in that format and give the segment a `[tags]` table:

```toml
[attached_bottom]
format = "$keys  ($pins  )($tags  )"

[tags]
format      = "[<$label>]()"
separator   = "  "
more_format = "[ <> ](bold)"
```

The bundled examples in `docs/examples/theme-*.toml` carry styled versions
of both.
