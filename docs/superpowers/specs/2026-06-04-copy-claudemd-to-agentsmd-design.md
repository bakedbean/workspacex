# Copy CLAUDE.md into newly-created AGENTS.md

## Problem

wsx injects its own instructions into a repo's `AGENTS.md` (wrapped in a
`<!-- BEGIN/END wsx-managed -->` block) before spawning Hermes or Codex, which
read project instructions from `AGENTS.md`. If the file doesn't exist, wsx
creates it.

Claude reads a repo's `CLAUDE.md` natively, but Hermes and Codex never do — they
only read `AGENTS.md`. So when wsx creates an `AGENTS.md` for a repo that has a
`CLAUDE.md`, those agents miss the project instructions Claude gets for free.

## Goal

When wsx **creates** an `AGENTS.md` (it did not previously exist) and the repo
has a root `CLAUDE.md`, copy the `CLAUDE.md` contents into the new `AGENTS.md`
**after** the wsx-managed block, so Hermes/Codex get instruction parity with
Claude.

## Design

Single change point: `write_agents_md_section()` in `src/pty/session.rs` — the
only place `AGENTS.md` is created. Both `prepare_hermes_workspace()` and
`prepare_codex_workspace()` route through it, so they both get the behavior.

Logic:

1. Capture `path.exists()` **before** reading the file → `file_existed`.
2. Build the wsx-managed block exactly as today.
3. New: if `!file_existed` **and** `content` is `Some` (we're writing a wsx
   block) **and** `cwd/CLAUDE.md` exists with non-whitespace content, append
   after the `<!-- END wsx-managed -->` line:

   ```
   <blank line>
   <!-- Copied from CLAUDE.md by wsx -->
   <CLAUDE.md contents verbatim>
   ```
4. Write as before (existing no-op-on-equal and empty-and-absent guards keep).

A helper `read_claude_md(cwd) -> Option<String>` isolates the IO and the
non-whitespace check, returning `None` when the file is absent or blank.

### Idempotency / reordering

The trigger is strictly "file did not exist," so the copy fires exactly once.
On every later spawn `file_existed` is true → no re-copy, no duplication. The
copied text rides along as ordinary non-wsx content (existing strip logic
preserves it); the only side effect is the wsx block migrating below the copied
content after the second spawn. Content is never lost or duplicated.

## Scope (YAGNI)

- Only the root `CLAUDE.md` (`cwd.join("CLAUDE.md")`) — not nested or `.claude/`
  variants.
- No live re-sync if `CLAUDE.md` changes later — copy is one-time on creation.
- `AGENTS.md` is already git-excluded by the callers, so the copied content
  won't appear in `git status`.

## Testing

New tests alongside the existing `hermes_agents_md` test module:

- (a) fresh create + `CLAUDE.md` present → `AGENTS.md` contains wsx block, then
  the provenance comment, then the content.
- (b) fresh create, no `CLAUDE.md` → unchanged behavior.
- (c) `AGENTS.md` already exists → `CLAUDE.md` not copied even if present.
- (d) empty / whitespace-only `CLAUDE.md` → not copied.
