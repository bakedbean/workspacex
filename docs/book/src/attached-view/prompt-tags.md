# Prompt tags

Prompt tags wrap a section of your prompt in XML tags, such as `<context>…</context>` or `<task>…</task>`, so the model can tell the parts apart. Anthropic's prompting guidance recommends this.

## Insert a tag

Press `Ctrl-x <` or click the `<>` chip in the footer to open the picker:

```
 name: ▏
  1  context                   ×12
  2  task                      ×7
  3  constraints               ×2
 [↑/↓] move   [enter] body   [1-9] pick   [^d] delete   [esc] close
```

| Key        | Action                                                        |
| ---------- | ------------------------------------------------------------- |
| Type       | Filter tags by prefix                                         |
| `Enter`    | Open the selected tag's body box, or create a new tag         |
| `1`–`9`    | Pick a listed tag (while the name field is empty)             |
| `Ctrl-d`   | Delete the selected tag                                       |
| `Esc`      | Close the picker                                              |

In the body box, type your text. `Enter` inserts a newline.

| Key      | Action                                                     |
| -------- | ---------------------------------------------------------- |
| `Ctrl-s` | Insert the wrapped text into the agent's prompt            |
| `Esc`    | Go back to the picker and keep your draft                  |

`Ctrl-s` inserts this into the agent's prompt without submitting it:

```
<context>
…your text…
</context>
```

Stack several sections, add an instruction, and press `Enter` yourself.

- Each insert raises the tag's use count. The three most-used tags appear as footer chips. Click one to open its body box.
- If wsx can't confirm the insert, for example because the agent has exited, the body box stays open with your draft.
- Tag names must match `[A-Za-z_][A-Za-z0-9_.-]*`.

## Configure

The tags and their use counts live in the `prompt_tags` setting, one `name=uses` per line:

```bash
wsx config edit prompt_tags     # edit in $EDITOR
wsx config set prompt_tags ""   # clear
```

Lines that don't parse are dropped.

The footer chips are the `$tags` bar segment. See [Themes](../configuration/themes.md) to move or restyle them. `Ctrl-x <` works even if your theme leaves the segment out. If your own `~/.config/wsx/theme.toml` sets `[attached_bottom].format`, add `($tags  )` to it and a `[tags]` table to show the chips:

```toml
[attached_bottom]
format = "$keys  ($pins  )($tags  )"

[tags]
format      = "[<$label>]()"
separator   = "  "
more_format = "[ <> ](bold)"
```
