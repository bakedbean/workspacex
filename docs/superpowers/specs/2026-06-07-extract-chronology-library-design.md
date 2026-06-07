# Extract the Change Chronology into a Standalone Library — Design

**Date:** 2026-06-07
**Status:** Approved for planning
**Builds on / revises:** the Change Chronology bar, its keyboard navigation, the
detail modal, and syntax highlighting (specs `2026-06-05-change-chronology-view`,
`2026-06-05-chronology-keyboard-navigation`, `2026-06-06-chronology-detail-modal`,
`2026-06-06-chronology-syntax-highlight`).

## Problem

The change-chronology feature — parse Claude Code session logs into a timeline of
file changes, navigate them, and view a full syntax-highlighted diff — is wholly
contained inside the `wsx` crate (~1,760 lines across five modules). It is a
self-contained capability with no real `wsx`-specific logic, yet it can only live
inside `wsx`. We want it to be its own library in its own git repository so it can
be reused (other tools that want a "what did the agent change, when" view) and so
the `wsx` crate shrinks behind a clean boundary.

## Goal

Move the chronology feature into a new crate, `chronox`, in its
own git repository, and have `wsx` consume it as a **git dependency**. The crate
splits into two layers:

- A **framework-agnostic core** (default): JSONL parsing, the `Timeline` cache and
  on-demand full-change re-extraction, the navigation/scroll state machine, the
  syntax tokenizer (emitting neutral tokens, *not* ratatui spans), and config
  resolution (over a trait, not `wsx`'s `Store`).
- An **optional `ratatui` UI layer** behind a `ratatui` feature: the bar entry
  renderer, the styled diff builder, and width-clipping — i.e. the functions that
  turn neutral tokens/lines into ratatui `Line`/`Span`. `wsx` enables this feature
  and keeps using exactly what it uses today.

`wsx` keeps its **integration** layer (the per-workspace `Timeline` cache on `App`,
`refresh_chronology`, the `Modal::ChangeDetail` variant, input handlers, render
dispatch, and the `Store`/`Repo` config glue). Nothing about the on-screen
behaviour changes.

Publishing to crates.io is **explicitly deferred** (its own optional phase). The
crate is built to publishable standards — no `wsx`-isms in the core, tests come
along, doc comments on the public API — but the public-release ceremony (README,
runnable examples, CI matrix, license, semver guarantees) is not part of this work.

### Why not publish now

The crate's entire job is parsing Claude Code's **undocumented JSONL session-log
format**, owned by Anthropic and subject to change. That is fine for our own use
(we already track it), but publishing publicly would commit us to chasing format
changes for third parties. We keep the *shape* publish-ready and the *decision*
deferred.

## Scope

- **In scope:**
  - New crate `chronox` with `core` + `ratatui`-feature layers.
  - Decouple the syntax tokenizer from ratatui (neutral `TokenKind`; ratatui
    styling moves to the feature layer).
  - Decouple config resolution from `wsx`'s `Store`/`Repo` (a small trait /
    plain-string input; `wsx` implements it).
  - Inline the two `activity/events.rs` helpers the feature uses
    (`encode_cwd`, `parse_iso8601_ms`) into the crate.
  - Re-point `wsx` at the new crate (git dependency), delete the moved code, keep
    the integration layer working and all existing tests green.
- **Out of scope:**
  - Any change to on-screen behaviour, keybindings, config keys, or the session-log
    format we parse.
  - Publishing to crates.io (deferred optional phase; the crate is built ready for
    it but the release is not done here).
  - Generalising beyond Claude Code session logs (e.g. other agents' formats) —
    that remains the existing deferred follow-up, unaffected by this move.
  - Re-homing config *storage* — `wsx` keeps owning the DB; only config
    *resolution* (parse + merge JSON → `ChronologyConfig`) moves to the crate.

## Decisions (from brainstorming)

- **Boundary = core + optional `ratatui` UI feature** (chosen). Not data-only (that
  would force `wsx` to re-home the rendering code), not data+ratatui-always (that
  would tie the crate to one framework and block any non-ratatui reuse).
- **Goal = extract as a clean standalone repo consumed via git dependency now;
  defer crates.io.** The chosen boundary already yields a publish-ready shape, so
  "open-source it later" becomes a small finishing task rather than a fork.
- **`wsx` consumes via git dependency** (`{ git = "…", rev = "…" }`), pinned to a
  rev for reproducibility, not a path dependency (the whole point is a separate
  repo) and not crates.io (deferred).
- **The neutral token representation is the crate's core highlighting output.** The
  tokenizer returns `Vec<Token>` where `Token = (String, TokenKind)`; the ratatui
  feature maps `TokenKind → ratatui::Style`. This is the one non-mechanical refactor.

## Architecture

```
repo: chronox
└── src/
    ├── lib.rs        re-exports; documents the core vs ratatui split
    ├── event.rs      ChangeEvent, ChangeDetail, ChangeTool, ChangeSource   [core]
    ├── extract.rs    extract_change_events, parse_file, load_full_change,
    │                 resolve_line_in_file, session-file discovery, the
    │                 inlined encode_cwd / parse_iso8601_ms helpers          [core]
    ├── timeline.rs   Timeline cache + stat-based invalidation               [core]
    ├── nav.rs        nav(), adjust_scroll(), clamp_scroll(), NavKey/NavAction [core]
    ├── syntax.rs     LangSpec, lang_for_path, tokenize -> Vec<Token>,
    │                 TokenKind                                              [core]
    ├── config.rs     ChronologyConfig, ChronologyOverride, Side, WidthSpec,
    │                 with_override/sanitize/resolved_width, resolve over a
    │                 ConfigSource trait                                     [core]
    └── render.rs     entry_lines, change_detail_lines_styled,
                      clip_line_to_width, TokenKind->Style mapping
                      #[cfg(feature = "ratatui")]                           [ui]

Cargo.toml
  [dependencies] serde, serde_json
  [dependencies.ratatui] optional, version matched to wsx's (0.29)
  [features] default = ["ratatui"]; ratatui = ["dep:ratatui"]
  [dev-dependencies] tempfile (already used by the existing tests)
```

```
consumer: wsx (unchanged behaviour)
  App.chronology: HashMap<WorkspaceId, chronox::Timeline>
  App::refresh_chronology(..)  -> uses crate's parse_file / session discovery
  Modal::ChangeDetail { .. }   -> built from crate's change_detail_lines_styled
  input.rs / render.rs / attached.rs -> call crate fns, same as today
  config glue: impl crate::ConfigSource for (Store, Repo)  (or pass JSON strings)
```

## The three seams to cut

These are the only places the feature touches the rest of `wsx`. Everything else
is a mechanical file move.

### Seam 1 — Syntax tokenizer ↔ ratatui (the one real refactor)

Today `src/ui/syntax.rs` builds ratatui directly:

```rust
fn kw_style() -> Style { Style::default().fg(Color::Magenta) }
pub fn highlight_code(text: &str, spec: &LangSpec) -> Vec<Span<'static>> { … }
pub fn change_detail_lines_styled(detail, base_line, lang) -> Vec<Line<'static>>
pub fn clip_line_to_width(line: &Line<'static>, width) -> Line<'static>
```

Split into core (no ratatui) + ui (ratatui):

```rust
// core (src/syntax.rs)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind { Default, Keyword, Str, Number, Comment }
pub type Token = (String, TokenKind);

/// Tokenize ONE line into (text, kind) runs. Same priority order as today:
/// line comment > string > number > keyword/identifier > default.
pub fn tokenize_line(text: &str, spec: &LangSpec) -> Vec<Token>;

/// Neutral diff model the modal renders. One entry per display line.
pub struct DiffLine {
    pub gutter: String,        // "   7 " or "     "
    pub marker: DiffMarker,    // Added | Removed
    pub code: Vec<Token>,      // tokenized (or one Default run when lang = None)
}
pub enum DiffMarker { Added, Removed }
pub fn change_detail_diff(detail: &ChangeDetail, base_line: u32,
                          lang: Option<&LangSpec>) -> Vec<DiffLine>;
```

```rust
// ui (src/render.rs, #[cfg(feature = "ratatui")])
fn style_for(kind: TokenKind) -> Style;        // Keyword->Magenta, Str->Yellow, …
pub fn change_detail_lines_styled(detail, base_line, lang) -> Vec<Line<'static>>
    // = change_detail_diff(..).map(diff_line_to_ratatui)
pub fn clip_line_to_width(line: &Line<'static>, width) -> Line<'static>  // unchanged
```

The exact colours and the gutter/marker scheme are preserved (Keyword→Magenta,
Str→Yellow, Comment→DarkGray, Number→Cyan, `+`→Green, `-`→Red, gutter DIM). The
existing `syntax.rs` tests split: tokenizer-priority assertions move to the core
test (assert on `TokenKind`), and the colour/`Line`-shape assertions stay in the
ui test (assert on ratatui `Style`/`Span`). `wsx` keeps calling
`change_detail_lines_styled` / `clip_line_to_width` from the `render` module
exactly as it does now — its call sites do not change.

### Seam 2 — Config ↔ `Store`/`Repo`

Today `src/config/chronology.rs` reads `wsx`'s DB:

```rust
use crate::data::store::{Repo, Store};
pub fn resolve_global_only(store: &Store) -> ChronologyConfig { store.get_setting("chronology_config") … }
pub fn resolve(repo: &Repo, store: &Store) -> ChronologyConfig { … repo.chronology_config … }
```

Invert it — the crate defines what it needs, `wsx` supplies it:

```rust
// crate config.rs (core)
/// Source of the two raw JSON blobs the resolver merges. `wsx` implements this
/// over its Store/Repo; tests use a trivial in-memory impl.
pub trait ConfigSource {
    fn global_json(&self) -> Option<String>;   // settings.chronology_config
    fn repo_override_json(&self) -> Option<String>; // repos.chronology_config
}
pub fn resolve(src: &impl ConfigSource) -> ChronologyConfig;
pub fn resolve_global_only(src: &impl ConfigSource) -> ChronologyConfig;
```

`ConfigSource` keeps the same merge/sanitize/parse-warn behaviour (repo wins
per-field; defaults on missing/parse-failure). `wsx` adds a thin adapter (a
newtype or a `(&Store, Option<&Repo>)` impl) at the call sites in
`config/chronology.rs`'s former home. The `tracing::warn!` calls become either a
returned/ignored parse result or a `log`-facade call — the crate should not depend
on `tracing` just for two warnings; it logs via the `log` crate (a near-universal
facade) or simply drops the warning and documents the silent-default behaviour.
**Decision:** use the `log` facade (cheap, no runtime, `wsx`'s `tracing` subscriber
can bridge `log` if desired). The exact warning text is non-load-bearing.

### Seam 3 — `activity/events.rs` helpers

`extract.rs` uses two free functions from `activity/events.rs`:

- `encode_cwd(path)` — encode a worktree path into Claude's session-dir name.
- `parse_iso8601_ms(timestamp)` — parse an ISO-8601 string to epoch ms.

Both are small and self-contained. **Inline copies into `extract.rs`** (with their
existing unit tests). `wsx` keeps its originals (other call sites may use them);
the crate carries its own copy so it has no back-dependency. If the copies ever
drift, the session-log format is the contract, not the helper.

## Data flow (unchanged end-to-end)

```
Claude session JSONL ──parse_file──► Vec<ChangeEvent> (clipped detail)
                                       │  (cached in App.chronology[ws] : Timeline)
bar render ◄── entry_lines(ev, …) ◄────┘            [ratatui feature]
select+Enter ──► load_full_change(ev) ──► full ChangeDetail
             ──► change_detail_lines_styled(detail, line, lang_for_path(path))
             ──► Modal::ChangeDetail { lines, … }   (wsx owns the variant)
modal scroll ──► clamp_scroll(scroll, len, body)    [core]
modal `e`    ──► wsx editor launch (wsx owns)
```

## Versioning & consumption

- `wsx`'s `Cargo.toml` gains:
  ```toml
  chronox = { git = "https://…/chronox", rev = "<sha>", features = ["ratatui"] }
  ```
  Pinned to a rev (reproducible builds; bump deliberately). `ratatui` feature on so
  `wsx` gets the UI layer.
- The new crate pins `ratatui = "0.29"` to match `wsx`'s exact version, so the
  `ratatui` types crossing the boundary (`Line`/`Span`/`Style`) are the same types
  in both crates (a version mismatch would make them incompatible). When `wsx`
  bumps ratatui, the crate bumps in lockstep.
- Crate is **edition 2024** to match `wsx`.

## Testing

- **Crate-internal (moves with the code):** the ~400 lines of existing tests —
  `chronology.rs` extract/source/load_full_change, `chronology_nav.rs` nav +
  clamp_scroll, `chronology_bar.rs` formatting + abbreviation + auto-hide,
  `config/chronology.rs` merge/sanitize/resolve, `syntax.rs` highlight. These port
  with mechanical edits (paths, and the syntax test split — core asserts
  `TokenKind`, ui asserts `Style`).
- **New core test:** `ConfigSource` resolution via a trivial in-memory impl
  (replaces the `Store::open_in_memory()` test) — global-only, repo-override-wins,
  parse-failure-defaults.
- **New core test:** `tokenize_line` priority order on `TokenKind` (was asserted on
  ratatui spans).
- **Feature-gated ui test:** `change_detail_lines_styled` colour + `Line` shape
  (the existing `syntax.rs` colour assertions), run under `--features ratatui`.
  CI/local must run both `cargo test` (default features, core only) and
  `cargo test --no-default-features` (core compiles without ratatui) and
  `cargo test --all-features`.
- **`wsx` regression:** `cargo test` (the integration layer + remaining tests),
  `cargo build`, `cargo clippy --all-targets`, and a **manual TUI pass** (focus
  bar, navigate, open modal, scroll all ways, syntax colours present, `e` opens
  editor, `Esc`/click closes) — the same manual script the detail-modal spec used.

## Error handling / edge cases

- Session log missing/unreadable/line-gone → `load_full_change` returns `None`,
  caller falls back to the clipped detail (unchanged).
- Config JSON missing/malformed → defaults, warning via `log` (was `tracing`).
- ratatui-feature-off build → core compiles and tests pass with no ratatui in the
  dependency tree (guarded by the `--no-default-features` CI step).
- ratatui version skew between crate and `wsx` → prevented by pinning the same
  `0.29`; documented as a lockstep-bump rule.
- Worktree path encoding / timestamp parsing → covered by the inlined helpers'
  ported tests.

## Risks

- **Low overall** — coupling is thin and one-directional; no new third-party deps
  in the core; ~400 lines of tests are the regression net.
- **Main risk: the ratatui-version contract.** A skew makes the boundary types
  incompatible with confusing errors. Mitigated by pinning + the lockstep rule,
  and surfaced early because `wsx` won't compile against a mismatched crate.
- **Secondary: cross-repo iteration friction.** During development, iterate with a
  temporary `[patch]` / path override in `wsx`, then pin to the rev for the final
  commit. Called out in the plan.
- **Token-model refactor (Seam 1)** is the only place behaviour could regress;
  covered by keeping colours/scheme identical and splitting the existing tests to
  assert both the neutral tokens and the rendered styles.

## Files

### New repository: `chronox`
- `Cargo.toml`, `src/lib.rs`, `src/event.rs`, `src/extract.rs`, `src/timeline.rs`,
  `src/nav.rs`, `src/syntax.rs`, `src/config.rs`, `src/render.rs` (feature-gated),
  `.gitignore`. (README/CI/LICENSE only in the deferred publish phase.)

### `wsx` (consumer) — modified
- `Cargo.toml` — add the git dependency with `ratatui` feature.
- Delete: `src/activity/chronology.rs`, `src/config/chronology.rs`,
  `src/ui/chronology_bar.rs`, `src/ui/chronology_nav.rs`, `src/ui/syntax.rs`
  (their content now lives in the crate).
- `src/activity/mod.rs`, `src/config/mod.rs`, `src/ui/mod.rs` — drop the removed
  modules; re-export crate types where the old paths were used, or update imports.
- `src/app.rs` — `chronology: HashMap<_, chronox::Timeline>`;
  `refresh_chronology` calls the crate.
- `src/app/input.rs`, `src/app/render.rs`, `src/ui/attached.rs`,
  `src/ui/modal.rs` — update `use` paths to the crate; behaviour unchanged.
- New thin adapter implementing `ConfigSource` over `Store`/`Repo` (small module,
  e.g. `src/config/chronology_source.rs`).

## Naming

Crate/repo name: **`chronox`** (decided). The repository lives at `~/chronox`
(`/home/eben/chronox`); `wsx` consumes it via an absolute path dependency during
development and a pinned git rev once pushed to a remote.
