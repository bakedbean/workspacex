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
Tabs are kept as tabs (shown four columns wide).
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

Tag names must start with a letter or `_` and contain only letters, digits,
`_`, `.` and `-`.

The list lives in the `prompt_tags` setting, one `name=uses` per line:

```bash
wsx config get prompt_tags
wsx config edit prompt_tags                 # opens $EDITOR on the current value
wsx config set prompt_tags "context=12
task=7"
wsx config set prompt_tags ""               # clear
```

The footer chips are the `$tags` bar segment — see
[Themes](../configuration/themes.md) to move, restyle, or drop them. The
`Ctrl-x <` chord works even when a theme omits the segment.
