# Manual test — Prompt tags

Spec: `docs/superpowers/specs/2026-09-16-prompt-tags-design.md`

Use a scratch state dir so the real tag list is untouched:

```bash
scratch=$(mktemp -d /tmp/wsx-tags-test.XXXXXX)
export XDG_CONFIG_HOME="$scratch/config" XDG_STATE_HOME="$scratch/state"
```

Register a repo and create one workspace per agent kind you have installed
(claude, codex, pi, hermes, omp). For each:

## 1. Insert lands unsubmitted, newlines intact

1. Attach, wait for the agent's composer.
2. `Ctrl-x <`, type `context`, Enter.
3. Type `first line`, Enter, `second line`, `Ctrl-s`.
4. Expect: the composer shows `<context>` / `first line` / `second line` /
   `</context>` (omp may show a `[Paste #1]` placeholder instead — that is
   expected) and the agent has NOT started a turn.
5. Type `summarise the above` and press Enter. Expect the agent's reply to
   reference both lines (this is the check that omp expands the placeholder).

Record the omp outcome in `insert_writes`' doc comment if it differs from
what the comment says.

## 2. Footer chips reorder by use

1. Insert under `context` twice and `task` once.
2. Expect the footer to read `<context>  <task>   <>` with `context` first.
3. Click `<task>`: the body box opens titled ` <task> `.
4. Click `<>`: the picker opens.

## 3. Manager keys

1. In the picker, type `Bad Name` — the field turns red and Enter does nothing.
2. Clear it, ↓ to `task`, `Ctrl-d` — `task` disappears;
   `wsx config get prompt_tags` no longer lists it.

## 4. Theme without `$tags`

1. `wsx theme init`, remove `($tags  )` from `[attached_bottom].format`.
2. Expect no chips, and `Ctrl-x <` still opens the picker.

## 5. Remote attach

1. Attach to a shared workspace on another host (`H`).
2. `Ctrl-x <`, pick a tag, insert. Expect the text in the remote agent's
   composer, unsubmitted.
