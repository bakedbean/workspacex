# Workspace tags — design

**Date:** 2026-10-15
**Status:** approved

## Problem

Per-repo hide rules (#419, #420) can only hide workspaces by PR state. Users
want their own labels on workspaces ("parked", "spike") and to hide
workspaces by those labels. PR state can't express intent; a tag can.

## Goals

- Free-text tags on workspaces, added and removed from the TUI and the CLI.
- Tag suggestions drawn from every tag in use across wsx.
- Per-repo hide rules keyed on a tag, alongside the existing PR rules.
- Tags visible on the dashboard row and searchable with `/`.

## Non-goals

- "Show only tagged" rules. Tag rules hide; they never focus.
- Global hide rules. Tag rules are per-repo, like PR rules.
- A managed tag list, tag rename, or tag colors.
- Pruning hide rules whose tag is no longer in use.

## Naming

"Workspace tags" throughout, to stay distinct from the unrelated prompt tags
(`Ctrl-x <`, docs/book/src/daily-use/prompt-tags.md).

## Tag names

`normalize_tag(&str) -> Option<String>` in a new `src/data/tags.rs`: trim, lowercase, then accept only `[a-z0-9_-]{1,32}`.
Everything else is rejected. The TUI and CLI both go through it, and it
guarantees a tag can't break the comma-separated `tag:<name>` token.

## Storage

Schema v30 adds:

```sql
CREATE TABLE IF NOT EXISTS workspace_tags (
  workspace_id INTEGER NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
  tag TEXT NOT NULL,
  PRIMARY KEY (workspace_id, tag)
);
```

`Store` gains:

- `add_workspace_tag(id, tag)` and `remove_workspace_tag(id, tag)`, both
  idempotent (`INSERT OR IGNORE`, plain `DELETE`).
- `all_tags() -> Vec<(String, usize)>`: distinct tags across all
  workspaces, sorted by name, with how many workspaces carry each.

`Workspace` gains `tags: Vec<Arc<str>>`, sorted, loaded alongside the
workspace lists. CLI edits reach a running TUI through the same reload path
as renames and name colors.

Repo deletion clears `workspace_tags` along with the repo's workspaces (the
cascade covers it; foreign keys are on).

## Rule model

```rust
pub enum HideRule {        // Clone, Eq, Hash, Ord — no longer Copy
    Tag(Arc<str>),         // declared first: tag rules sort before PR rules
    Pr(PrCriterion),
}
```

- Derived `Ord` orders tag rules alphabetically, then PR rules in their
  existing order. `classify` is first-match in that order, so a workspace
  matching a tag rule and a PR rule lands in the tag group.
- `token()` returns `Cow<'static, str>`; tags serialize as `tag:<name>`.
  `from_token` accepts `tag:<name>` only when `normalize_tag` returns the
  name unchanged; invalid tokens are skipped like unknown ones.
- `label()` returns `Cow<'static, str>`: `#name` for tags.
- `lifecycle()` returns `None` for tags.
- `HideRule::ALL` is renamed `PR_RULES` (the fixed six).
- `Tag(t).matches(s)` is `s.tags.contains(t)`. No in-progress exemption:
  tagging is deliberate.

Dropping `Copy` touches every site that copies a rule
(`SelectionTarget::HiddenGroup`, `expanded_hidden`, `HiddenGroupView`, the
dashboard renderers and input). `Arc<str>` keeps those clones to a refcount
bump. `SelectionTarget` itself loses `Copy`; `select_index` and friends clone.

Rules stay in `repos.hide_rules`; the storage format is unchanged, and older
wsx builds skip `tag:` tokens.

## Subject

`HideSubject` gains `tags: Vec<Arc<str>>` and becomes `Clone` only.
`App::hide_subject_with_age` fills it from the workspace, so every hide
decision (rows, selection, picker counts, updates panel) still flows through
one builder. The dashboard test fixture at src/ui/dashboard/tests.rs sets it
explicitly.

## TUI

### Tag editor

`#` on a workspace row opens `Modal::WorkspaceTags`; the `?` actions card
lists it too. Modeled on `NameColorPicker`:

```
 tags · my-slug
 > spike▏
   [x] parked
   [ ] spike        (new)
   [ ] ui
 ↑↓ move  space toggle  enter add typed  esc close
```

- The list is `all_tags()` with the workspace's tags checked, narrowed by
  the typed text (substring).
- `space` toggles the tag under the cursor.
- `enter` adds the typed text after `normalize_tag`, then clears the input.
  An invalid name shows an inline error and the modal stays open.
- When the typed text normalizes to a name not in the list, a `(new)` row
  is shown and selected first.
- Every change saves immediately and calls `app.refresh()`; `esc` closes.

### Hide-rules picker (`F`)

`Modal::HideRules` replaces `counts: [usize; 6]` with
`rows: Vec<(HideRule, usize)>`, built on open:

1. Tag rows: every tag from `all_tags()`, plus any tag this repo has a rule
   for that is no longer in use, sorted by name.
2. PR rows: `PR_RULES` in order.

Counts are this repo's workspaces that the rule `matches` (not first-match),
snapshotted on open as today. The render draws a `tagged` header above tag
rows and a `PR state` header above PR rows; headers are not selectable and
the `tagged` header is omitted when there are no tag rows. Toggle, save, and
"turning a rule off forgets its expanded state" are unchanged.

### Dashboard row

After the slug, the workspace's tags render dim as ` #a #b`. When the row
is too narrow, tags truncate first, leaving ` +N` for the ones dropped; the
status/recap column keeps its width priority.

### Hidden group row

A tag group reads `▸ # parked  ·  2 hidden`. `lifecycle() == None` draws a
dim `#` glyph in place of the branch glyph.

### `/` filter

`matches_filter` also checks tags:

- a plain needle matches any tag by substring (as it does branches);
- a needle starting with `#` matches only workspaces with a tag exactly equal
  to the rest of the needle, and nothing else.

An active filter still bypasses all hide rules.

### Updates panel and by-attention view

No change beyond classification: tag groups appear in the HIDDEN section like
PR groups, and the updates panel leaves tag-hidden workspaces out.

## CLI

```
wsx workspace tag add <tag>... [--workspace <repo>/<slug>]
wsx workspace tag rm  <tag>... [--workspace <repo>/<slug>]
wsx workspace tag list [--workspace <repo>/<slug> | --all] [--json]
```

- Targeting follows `agent list`: the current workspace by default, else
  `--workspace`.
- `add` normalizes every argument first; if any is invalid, nothing is
  written and the command exits non-zero naming the bad tag.
- `rm` of an absent tag is a no-op.
- `list` prints the workspace's tags one per line; `--all` prints
  `tag<TAB>count` for every tag in use. `--json` emits arrays.
- `workspace list --json` gains a `tags` array per workspace.

## Docs

- `docs/book/src/daily-use/workspace-tags.md` (new): adding tags, row
  display, `/#tag`, CLI.
- `hiding-workspaces.md`: a "By tag" section; tag rules win over PR rules;
  a rule for an unused tag stays listed so it can be turned off.
- `keybindings.md`: `#`.
- `SUMMARY.md` and CLI help text.

## Testing

- `hide_rules.rs`: tag token round-trip; invalid tag tokens skipped; tag
  rules sort before PR rules; `classify` prefers the tag rule; tag matching.
- `normalize_tag`: case, trim, bad characters, empty, over-length.
- Schema: v30 is idempotent across re-runs; tags cascade on workspace delete.
- Store: add/rm idempotent; `all_tags` distinct, sorted, counted.
- Picker: dynamic rows, header skipping, stale tag rule listed, per-repo
  counts.
- Dashboard `mod hide_rules`: `visible_targets` order with tag groups; tag
  truncation on narrow rows.
- Filter: substring vs `#` exact match.
- Tag editor: toggle, add typed, invalid input stays open.
- CLI: parse and run for add, rm, list.

## Commits (one PR)

1. Spec.
2. Schema v30 and store.
3. Tag rule model; drop `Copy`.
4. CLI.
5. Dashboard row and `/` filter.
6. Tag editor modal.
7. Picker dynamic rows.
8. Docs.
