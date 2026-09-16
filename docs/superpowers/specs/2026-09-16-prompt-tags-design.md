# Prompt tags: XML-wrapped prompt sections from the attached view

Anthropic's prompting guidance recommends structuring prompts with XML
tags — `<context>…</context>`, `<task>…</task>`, `<constraints>…</constraints>`
— so the model can tell sections apart. Typing the tags by hand in an
agent's composer is tedious enough that nobody does it. This spec adds a
**prompt tags** feature to the attached (agent chat) view: pick or name a
tag, type the body in a small multi-line box, and wsx inserts
`<tag>\nbody\n</tag>` into the focused agent's composer — unsubmitted, so
several tagged sections can be stacked before Enter. Tags are remembered
and ranked by how often they are used; the top three sit in the chat
footer as chips beside the pinned commands.

## Goals

- From the attached view, wrap a typed body in a named XML tag and insert
  it into the focused pane's agent composer without submitting.
- Remember tag names and how often each is used; surface the most-used
  ones first everywhere they are listed.
- Reach the feature by keyboard (`^x <`, plus a row in the `^x` nav
  overlay) and by mouse (footer chips for the top tags, and a manager chip).
- Manage the saved list (create, delete) from the same modal.
- Persist through the existing settings table so `wsx config get/set
  prompt_tags` works like `pinned_commands`.
- Make the footer chips a theme segment (`$tags`) so bar themes can place,
  restyle, or drop them like `$pins`.

## Non-goals (v1)

- Tags with a saved default body (snippets). A tag is a name only.
- Per-repo tag lists. The list is global.
- Submitting the tagged text. Insertion never sends a trailing CR.
- The dashboard detail chip row. (The remote-attach view does get the
  chips and `^x <`: its PTY is the ssh hop into the remote agent, so the
  insert is plain bytes exactly as `$pins` already behaves there.)
- Handing the body to `$EDITOR`. The in-TUI box is the only editor.
- Vim-style editing in the body box. Cursor movement, insert, delete,
  newline only.
- Attributes on tags (`<context lang="rust">`).

## User-facing behaviour

### Opening

- `^x <` in the attached view opens the **prompt tag** modal in its
  *pick* stage. The `^x` nav overlay lists it as `<   prompt tag`.
- Clicking the footer's manager chip (`<>` by default) does the same.
- Clicking one of the footer's tag chips opens the modal directly in the
  *body* stage for that tag.

### Pick stage

A panel titled ` Prompt tag ` with:

- A single-line **name field** at the top. Typing filters the list below
  by case-insensitive prefix; the field is also how a new tag is named.
- The **tag list**: saved tags sorted by uses (desc), then name (asc).
  Each row shows an accelerator pill `[N]` (1–9, only while the name
  field is empty), the name, and a dim `×N` use count. ↑/↓ move the
  selection; the list scrolls to keep it visible.
- Footer hint: `[↑/↓] move  [enter] body  [1-9] pick  [^d] delete  [esc] close`.

Keys:

| Key | Effect |
|---|---|
| printable char / Backspace | edit the name field; selection resets to the first match |
| `1`–`9` (name field empty) | jump to body stage for the Nth listed tag |
| ↑ / ↓ | move selection within the filtered list |
| Enter | if the selection is a listed tag → body stage for it; else if the name field is a valid new name → create it (0 uses) and go to body stage; else no-op |
| `ctrl-d` | delete the selected tag (removed from the list and the setting immediately; no confirm — it is one line of config) |
| Esc | close |

A name is valid when it matches `[A-Za-z_][A-Za-z0-9_.-]*` (a safe subset
of XML `Name`). An invalid name is shown in the error colour and Enter
does nothing.

### Body stage

A panel titled ` <name> ` containing a **multi-line textbox** that fills
the panel. Footer hint: `[^s] insert  [enter] newline  [esc] back`.

Keys:

| Key | Effect |
|---|---|
| printable char | insert at cursor |
| Enter | insert newline |
| Backspace / Delete | delete before / at cursor |
| ← → ↑ ↓ | move cursor (↑/↓ keep the column where possible) |
| Home / End | line start / end |
| `ctrl-s` | insert into the agent (below); no-op when the body is blank |
| Esc | back to pick stage, keeping the draft body |

Pasted text (bracketed paste from the terminal) is inserted verbatim at
the cursor. The box wraps long lines visually at the panel width and
scrolls vertically to keep the cursor in view.

### Insertion

On `ctrl-s` with a non-blank body, wsx writes

```
<name>
{body}
</name>
```

into the focused pane's PTY, wrapped as a bracketed paste so the embedded
newlines land in the composer instead of submitting it. The tag's use
count is incremented and persisted, the modal closes, and the pane
scrolls to live. The user's own Enter submits.

If the focused pane has no live session, the modal closes with an
`Error` modal: `no running agent in the focused pane`.

### Footer chips

The attached view's bottom row gains a `$tags` segment after `$pins`
(bundled default: `format = "$keys  ($pins  )($tags  )"`). It renders up
to **three** chips for the most-used tags — `‹ context ›` style, label
truncated with `…` past 12 columns — followed by a manager chip whose
text comes from the segment's `more_format` (default `<>`). Tags with
zero uses still count toward the three (a fresh tag shows up right away).
When no tags exist, only the manager chip renders. Under a bar theme
that omits `$tags`, the keyboard path still works.

`[tags]` segment table (bundled defaults, mirroring `[pins]`):

```toml
# Prompt tags: the top three by use, then the manager chip. Variables:
# $label ($index is 1-based position, exposed for themes that want it)
[tags]
format      = "[<$label>](fg:path)"
separator   = "  "
more_format = "[ <> ](bg:bg_soft fg:dim bold)"
priority    = 40
```

`priority = 40` makes the segment drop before `$pins` (100) but after
`$procs` (30) when the row is too narrow.

The example themes in `docs/examples/` that place `$pins` get `$tags`
alongside it and a `[tags]` table in their palette.

## Data model & persistence

New module `src/commands/tags.rs`, shaped like `commands/pinned.rs`:

```rust
pub struct PromptTag { pub name: String, pub uses: u32 }

pub fn parse(text: &str) -> Vec<PromptTag>;      // "name=uses" per line; bare "name" = 0;
                                                 // trims, skips blanks, drops invalid names,
                                                 // last duplicate wins
pub fn serialize(tags: &[PromptTag]) -> String;  // "name=uses\n" per tag, in `sorted` order
pub fn sorted(tags: &mut Vec<PromptTag>);        // uses desc, then name asc
pub fn bump(tags: &mut Vec<PromptTag>, name: &str);   // +1, inserting at 1 if absent
pub fn remove(tags: &mut Vec<PromptTag>, name: &str) -> bool;
pub fn is_valid_name(name: &str) -> bool;
pub fn wrap(name: &str, body: &str) -> String;   // "<name>\n{body}\n</name>" (body rtrimmed of
                                                 // trailing newlines so the closing tag is on its own line)
pub const CHIP_COUNT: usize = 3;
```

Storage: settings key **`prompt_tags`**, one `name=uses` line per tag.
The key joins the `wsx config` allowlist (`src/cli/parse/config.rs`), so
`wsx config get/set/edit prompt_tags` work. Reads go through the memoized
`Store::get_setting`; writes through `set_setting` (which invalidates the
cache).

`App` gains `prompt_tags_cache: Vec<PromptTag>`, refreshed in the
attached render pass beside `pinned_commands_cache` and after every
modal mutation (create / delete / bump) so the footer updates on the next
frame without a settings round trip per frame.

## Byte shape into the agent

`src/pty/session.rs` gains, beside `submit_writes`:

```rust
pub(crate) fn insert_writes(agent: AgentKind, text: &str) -> Vec<u8>
```

returning `ESC[200~ text ESC[201~` for every agent kind, and a
`Session::insert_text(&self, text: &str) -> bool` that sends it as one
`WriteReq::Bytes` write (no CR) after `scroll_to_live()`, returning
`false` when the writer channel is closed.

Rationale: Claude, Codex and pi all honour bracketed paste and keep the
newlines as composer newlines; a plain `\n` would submit in some of them.
`submit_writes` deliberately leaves omp unwrapped because omp shows an
accepted paste as a `[Paste #N]` placeholder — for *insertion* that
placeholder is acceptable (the text still submits), but the implementer
must verify against a live omp that (a) the placeholder expands to the
text on submit and (b) plain `\n` bytes do not submit. If (a) fails and
(b) holds, `insert_writes` sends omp plain text. The finding is recorded
in the function's doc comment either way.

## Modal state

`Modal::PromptTag(PromptTagModal)` in `src/ui/modal/mod.rs`, state struct
in `src/ui/modal/prompt_tag.rs`:

```rust
pub struct PromptTagModal {
    pub stage: TagStage,
    pub name_field: String,     // pick-stage filter / new-name input
    pub selected: usize,        // index into the *filtered* list
    pub body: TextArea,         // survives Esc from body → pick
}
pub enum TagStage { Pick, Body { name: String } }
```

Pure helpers on the struct (unit-testable, no terminal):
`filtered<'a>(&self, tags: &'a [PromptTag]) -> Vec<&'a PromptTag>`,
`select_up/down`, `enter_target(&self, tags) -> Option<EnterAction>` where
`EnterAction::Existing(name) | Create(name)`.

Rendering (`render_prompt_tag`) uses `panel_frame` like the other modals:
width clamped 50–90, height 12–24. Pick stage: name field row, blank,
list, footer. Body stage: textbox, footer.

Key handling lives in `src/app/input/modal/prompt_tag.rs`, dispatched
from the modal input router like `repo_settings`. It reads the tag list
from `app.prompt_tags_cache`, mutates it, and writes back through
`set_setting("prompt_tags", serialize(..))`.

## Text area

New `src/ui/modal/textarea.rs`: a small owned buffer

```rust
pub struct TextArea { lines: Vec<String>, cursor: (usize /*row*/, usize /*col, chars*/), goal_col: Option<usize> }
```

with `insert_char`, `insert_str` (splits on `\n`), `newline`, `backspace`,
`delete`, `move_left/right/up/down`, `home`, `end`, `text() -> String`,
`is_blank()`, and a `render(f, area, theme)` that soft-wraps each line at
`area.width`, scrolls so the cursor row is visible, and calls
`f.set_cursor_position`. Char-indexed (not byte-indexed) so multi-byte
input behaves.

## Entry points & routing

- **Leader**: `KeyCode::Char('<')` arm in `dispatch_leader_action`
  (`src/app/input/leader.rs`) opens the modal in pick stage. Added to
  `nav_menu_items` as `NavItem { glyph: "<", label: "prompt tag" }` after
  `k processes`. `<` is not a letter, so `agent_switch_keys`' pool is
  unaffected; its comment and test list `<` as reserved for completeness.
- **Bar hits**: `Hit::TagChip(usize)` and `Hit::TagsManager` in
  `src/ui/bar/segment.rs`. `providers::tags(cfg, tags, resolver)` emits
  the chips (index into the *sorted* cache, like `pins`) and the manager
  chip through `more_format`. `route_hits` fills new
  `PanesDrawOutput::tag_chip_rects: Vec<(usize, Rect)>` and
  `tags_manager_rect: Option<Rect>`; `App` mirrors both, and `mouse.rs`
  checks them next to `chip_rects`.
- **Segment registration**: `put(.., "tags", providers::tags(..))` in
  `attached_bars` only (not `dashboard_detail`). `AttachedInputs` gains
  `tags: &[PromptTag]`.

## Error handling

- Invalid tag name: shown in error colour, Enter ignored; never persisted.
- Malformed lines in the `prompt_tags` setting (bad names, non-numeric
  uses) are dropped on parse and disappear on the next serialize — same
  posture as pinned commands, surfaced via `wsx config get`.
- Writer channel closed (agent exited mid-modal): `insert_text` returns
  `false`; the modal closes and an `Error` modal reports
  `agent is not running`. The use count is **not** bumped.
- Settings write failure: `Error` modal with the store error; the
  in-memory cache keeps the mutation so the UI stays consistent until
  the next successful write.

## Testing

Unit (all pure, no terminal):

- `commands::tags`: parse (bare names, `=uses`, blanks, invalid names
  dropped, duplicate last-wins), serialize round-trip, sort order, bump
  inserts/increments, remove, `is_valid_name` cases, `wrap` trailing
  newline handling.
- `textarea`: insert/newline/backspace at boundaries, `insert_str` with
  embedded newlines, up/down goal column, multi-byte chars.
- `PromptTagModal`: filtering, accelerator gating on empty field,
  `enter_target` existing vs create vs invalid.
- `insert_writes` byte shape per agent; `Session::insert_text` against
  `Session::fake` (closed channel → `false`).
- `providers::tags`: at most three chips, `Hit::TagChip(i)` indexes,
  manager chip present with zero tags, `more_format` text.
- Bar routing: `route_hits` fills `tag_chip_rects` / `tags_manager_rect`.
- Leader: `^x <` opens the modal (extend `src/app/input/tests/leader.rs`).
- Modal input flow: pick → body → `ctrl-s` writes the wrapped bytes to a
  fake session, bumps and persists the count, closes the modal;
  `ctrl-d` removes and persists.

Manual (`docs/manual-tests/prompt-tags.md`): per agent kind (claude,
codex, pi, hermes, omp) insert a three-line body and confirm it lands
unsubmitted with newlines intact; footer chips reorder after use; theme
with `$tags` removed still reaches the modal via `^x <`.

## Docs

- README feature bullet.
- `docs/book/src/daily-use/prompt-tags.md` (+ `SUMMARY.md` entry after
  pinned commands): what it does, keys, the `prompt_tags` setting, the
  `$tags` segment.
- `docs/book/src/configuration/themes.md`: `$tags` / `[tags]` in the
  segment reference.
- `src/ui/bar/default_theme.toml` comments and the example themes.

## Implementation order (each a commit)

1. `commands::tags` model + `prompt_tags` config key.
2. `insert_writes` / `Session::insert_text`.
3. `textarea` widget.
4. `PromptTagModal` state, render, input; leader `<` + nav row.
5. `$tags` segment, hits, mouse routing, default theme + examples.
6. Docs + manual test page.
