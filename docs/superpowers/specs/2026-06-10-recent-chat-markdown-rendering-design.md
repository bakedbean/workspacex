# RECENT CHAT: markdown rendering & legibility

**Date:** 2026-06-10
**Status:** Approved design
**Module:** `src/detail_modules/recent_chat.rs` (+ new `markdown.rs`, `theme.rs`)

## Problem

The RECENT CHAT detail-bar module renders `last_assistant_text` — the
agent's most recent assistant message — as a single block of uniform
grey text with no internal spacing. Because assistant replies are
markdown-formatted prose, the raw source leaks through: `**bold**`,
`## headings`, `- lists`, and `` `inline code` `` all appear as literal
characters. The result is hard to scan.

Today (`recent_chat.rs:44-47`) every wrapped line is painted with a
single `theme.dim_style()`, and the host (`src/ui/dashboard/detail.rs`)
only inserts blank lines *between* modules, never within one. So no
document structure survives.

## Goal

Render the assistant text as formatted markdown inside the narrow detail
column: paragraph spacing, headings, bullet/numbered lists, fenced code
blocks, blockquotes, and inline emphasis (bold/italic/code) — using
spacing, font weight, and accent colors to make the block legible.

Non-goals: links-as-hyperlinks, tables, images, nested-list rendering
beyond a single bullet marker, syntax highlighting inside code blocks.
Anything not explicitly handled passes through as plain body text.

## Decisions (locked)

- **Parser:** `pulldown-cmark` (0.13). Spec-correct CommonMark parsing;
  pure Rust, low MSRV, no C deps. It parses; *we* wrap to the column.
- **Elements rendered:** inline emphasis (bold/italic/code), headings,
  bullet + numbered lists, fenced code blocks, blockquotes.
- **Color:** accent direction. Body stays `dim`; headings brighter+bold;
  inline/code-block text in a distinct color; bullet markers in accent.

## Architecture

### New module: `src/detail_modules/markdown.rs`

One public function, pure and `DetailContext`-free:

```rust
pub fn render(text: &str, width: u16, theme: &Theme)
    -> Vec<ratatui::text::Line<'static>>
```

Rationale for a separate module: the renderer depends only on
`&str + width + &Theme`, so it is independently unit-testable and
reusable by any future module that wants to display assistant text. It
also keeps `recent_chat.rs` thin.

### `recent_chat.rs` changes

`build_lines` reduces to: resolve `events` / `last_assistant_text`
(keeping the existing `loading…` and `—` empty states), then
`markdown::render(text, width, theme)`. The current string-based
`wrap_lines` (and its tests) are **deleted** — the markdown renderer
subsumes plain-text wrapping (a plain paragraph is a single
`Paragraph` block). `session_summary.rs` keeps its own `wrap_lines`;
it is out of scope.

Register the new submodule in `src/detail_modules/mod.rs`
(`pub mod markdown;`).

## Data flow

`pulldown-cmark` provides parsing only. Wrapping to the narrow column is
ours. Three stages inside `render`:

### 1. Parse → blocks

Walk the `pulldown_cmark::Parser` event stream into an intermediate
`Vec<Block>` (both types private to the module):

```rust
enum Block {
    Paragraph(Vec<Inline>),
    Heading(Vec<Inline>),                 // level collapsed; all headings styled alike
    ListItem { marker: Marker, inlines: Vec<Inline> },
    CodeBlock(Vec<String>),               // raw lines, no inline parsing
    Quote(Vec<Inline>),
}

struct Inline { text: String, style: Style }

enum Marker { Bullet, Number(u64) }
```

Event handling:

- `Start(Strong)` / `Start(Emphasis)` push a `BOLD` / `ITALIC` modifier
  onto a style stack; emphasis nests by OR-ing modifiers
  (`**_x_**` → `BOLD | ITALIC`). `End` pops.
- `Text(s)` appends an `Inline { s, current_style }` to the open block.
- `Code(s)` (inline code) appends an `Inline` with the code color.
- `Start(Heading)` / `Start(Item)` / `Start(BlockQuote)` /
  `Start(CodeBlock)` open the corresponding block; their `End` closes it.
- `SoftBreak` → space; `HardBreak` → forced line break within the block.
- List `Start(Item)` records `Bullet` or the running `Number(n)` from the
  enclosing `List(Some(start))`.
- Unhandled tags (tables, images, links-as-tags) degrade: their inner
  `Text` still appears as plain body inlines; the tag wrapper is ignored.

### 2. Wrap each block → Lines (the core change)

A single **token-aware** greedy wrapper replaces today's string wrapper.
It packs inline tokens — each carrying its own `Style` — into lines at
`width`:

- Split each `Inline.text` into whitespace-delimited words, each
  inheriting the inline's style.
- Greedily fill a line to `width`; when a word doesn't fit, flush the
  line and start a new one.
- Within a flushed line, **merge adjacent words sharing the same
  `Style`** into one `Span` (so a bold phrase is one span, not N).
- **Over-long tokens** (longer than `width`, e.g. a long inline-code
  identifier) hard-split at the column boundary, mirroring today's
  behavior in `recent_chat.rs:61-75`.
- **List items** get a hanging indent: the marker (`• ` or `1. `) prefixes
  the first line; continuation lines are indented by the marker width so
  wrapped text aligns under the item text, not the marker.
- **Code blocks** are not word-wrapped or inline-parsed: each source line
  is indented 2 columns, painted in the code color, and hard-truncated
  (or hard-split) at `width`.
- **Blockquotes** get a `│ ` prefix (in `path` color) and italic body;
  continuation lines repeat the prefix.
- `width == 0` guard returns the text unwrapped (matches current
  `wrap_lines` guard), so a degenerate layout never panics.

### 3. Join blocks

Emit blocks in order, separated by exactly one blank `Line::from("")`.
Trim leading and trailing blank lines from the final `Vec` so the
module's output doesn't double the host's existing inter-module gap
(`detail.rs:396`).

## Styling (`theme.rs`)

Add markdown style helpers next to `status_style` / `agent_style`,
keeping the theme's "every color decision in one module" invariant. Add
**one** new theme field, `code: Color`, to the `Theme` struct and to all
five constructors (`ansi`, `wsx`, `dracula`, `jellybeans`, `nord`).

| Element | Style |
|---|---|
| Body text | `dim` (unchanged) |
| Bold | body fg + `BOLD` |
| Italic | body fg + `ITALIC` |
| Heading | `header_fg` + `BOLD` (the uniform single blank line between blocks separates it from the preceding block; no special leading blank, and none when the heading is the first block) |
| Inline code / code block | new `code` color; code blocks indented 2 cols |
| Bullet marker `•` | `attention` |
| Blockquote `│ ` prefix | `path`; body italic |

Suggested `code` values (muted but distinct from `dim`/`path`):
- `ansi`: `Color::Indexed(73)` (muted cyan)
- `wsx`: a teal/cyan RGB tuned against `bg_alt` (e.g. `Rgb(0x7e, 0xb6, 0xb0)`)
- `dracula` / `jellybeans` / `nord`: each theme's existing cyan/teal token.

Exact RGB values are finalized during implementation against each theme's
background; the contract is "distinct, legible, not darker than `dim`."

Helpers (names indicative): `md_heading_style`, `md_code_style`,
`md_bullet_style`, `md_quote_style`, returning `Style`.

## Dependencies

Add to `Cargo.toml` `[dependencies]`:

```toml
pulldown-cmark = { version = "0.13", default-features = false }
```

`default-features = false` drops the `html` rendering feature and its
`getopts` CLI dependency; we only need the event-stream parser, which is
always available.

## Testing

New unit tests in `markdown.rs`:

- Inline bold / italic / inline-code produce spans with the expected
  `Style` (modifier + fg).
- Nested emphasis `**_x_**` yields `BOLD | ITALIC`.
- Heading emits a `header_fg`+`BOLD` line preceded by a blank line.
- Bullet list: `•` marker in accent + hanging-indent continuation lines.
- Numbered list: running `1.`, `2.` markers.
- Fenced code block: indented, code-colored, no inline parsing of its
  contents (e.g. `**x**` inside a fence stays literal).
- Blockquote: `│ ` prefix + italic body.
- Long inline-code token hard-splits at `width`.
- `width == 0` returns without panic.
- Empty / whitespace-only input returns an empty (or single-blank) Vec.
- Plain prose (no markdown) wraps identically to user expectation —
  regression guard for the deleted `wrap_lines`.

Existing `recent_chat.rs` tests (`id`, `title`, empty-state line) stay
green; the empty-state assertions are unchanged.

## Risks / notes

- **Wrapping correctness is the real work**, not parsing. The token-aware
  wrapper with same-style span merging and hanging indents is where bugs
  will hide; it gets the densest test coverage.
- **Unicode width:** wrapping counts `chars()` like the current code, not
  display cells. Wide CJK / emoji can over-fill a line. This matches
  existing behavior and is out of scope to fix here.
- **`'static` lifetime:** `render` returns `Vec<Line<'static>>` (the trait
  contract). All `Inline.text` are owned `String`s, so spans own their
  text — no borrow from the parsed `&str` escapes.
