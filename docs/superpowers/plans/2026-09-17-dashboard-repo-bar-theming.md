# Dashboard Repo Bar Theming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Draw the by-repo dashboard's per-repo header line through the starship-style bar engine from a `[dashboard_repo]` theme table, with the bundled default reproducing today's line exactly.

**Architecture:** Five new segments (`fold`, `repo_name`, `pr_link`, `repo_path`, `status_counts`) are registered in the bar registry with providers and a `dashboard_repo` composer, the loader gains the `[dashboard_repo]` bar table, a `pad` key, and a generalised `symbols` table, and `by_repo.rs` shrinks to an adapter that computes cross-repo alignment and feeds one composer call per repo. The example themes and the book follow.

**Tech Stack:** Rust, ratatui, serde + toml. Tests via `cargo test`; CI gates `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` separately.

**Spec:** `docs/superpowers/specs/2026-09-17-dashboard-repo-bar-theming-design.md`

## Global Constraints

- Every commit is green under `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check`.
- Bar table name is `dashboard_repo`; segment names are `fold`, `repo_name`, `pr_link`, `repo_path`, `status_counts`; the click hit is `Hit::RepoPrs`.
- The bundled default must reproduce today's header: `▾ ── name  PR  /path  ────  ? 1  ✓ 2    3 ws`. The existing `by_repo.rs` tests are the parity gate.
- `pad` is accepted on `repo_name` only and must be exactly one character.
- `[fold.symbols]` keys are `expanded` and `folded`; `[agent_bar.symbols]` keys stay the agent kind names; any other segment rejects a `symbols` table.
- Commit messages end with `Claude-Session: https://claude.ai/code/session_01Kx4MjP6ySGuS716QEPAyUP`.
- Never `cd` out of the worktree; never bare `git stash`.

---

### Task 1: Generalise the `symbols` table (no behaviour change)

**Files:**
- Modify: `src/ui/bar/registry.rs` (the `SegmentDef` struct and every entry in `SEGMENTS`)
- Modify: `src/ui/bar/segment.rs:148-152` (`SegmentConfig::symbols`)
- Modify: `src/config/theme_file.rs:476,494-530` (`resolve_symbols`)
- Modify: `src/ui/bar/providers.rs:342-364,600-612,690-730,855,909,935` (`agent_bar`, `module_vars`, `agents`, tests)
- Modify: `src/ui/bar/bars.rs:39,309` (call sites pass `&cfg(...).symbols` unchanged)
- Modify: `src/config/theme_file.rs:938-985` (existing symbols tests)

**Interfaces:**
- Produces: `registry::SegmentDef.symbol_keys: &'static [&'static str]`; `SegmentConfig.symbols: Vec<(String, String)>`; `SegmentConfig::symbol_for(&self, key: &str) -> Option<&str>`; `providers::agents(..., icons: &[(String, String)], ...)`; `providers::module_vars(fleet, icons: &[(String, String)])`.

- [ ] **Step 1: Write the failing registry test**

Append to the `tests` module at the bottom of `src/ui/bar/registry.rs`:

```rust
    /// `[agent_bar.symbols]` keys are the agent kinds by display name, in
    /// `AgentKind::ALL` order; no other segment takes a symbols table yet.
    #[test]
    fn agent_bar_symbol_keys_are_the_agent_kind_names() {
        let expected: Vec<&str> = crate::pty::session::AgentKind::ALL
            .iter()
            .map(|k| k.display_name())
            .collect();
        assert_eq!(segment_def("agent_bar").unwrap().symbol_keys, expected.as_slice());
        for d in SEGMENTS.iter().filter(|d| d.name != "agent_bar") {
            assert!(d.symbol_keys.is_empty(), "{} unexpectedly takes symbols", d.name);
        }
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --lib registry::tests::agent_bar_symbol_keys -- --nocapture`
Expected: compile error, `no field symbol_keys`.

- [ ] **Step 3: Add `symbol_keys` to the registry**

In `src/ui/bar/registry.rs`, add the field to `SegmentDef` after `singleton`:

```rust
    /// Keys a `[<segment>.symbols]` table may carry, in the order the
    /// resolved entries come back. Empty for a segment that takes no such
    /// table (the loader rejects one there).
    pub symbol_keys: &'static [&'static str],
```

Add two consts next to `STYLE`/`NO_TAIL`:

```rust
const NO_SYMBOLS: &[&str] = &[];
/// `[agent_bar.symbols]`: one glyph per agent kind, by display name. Kept
/// in `AgentKind::ALL` order; `agent_bar_symbol_keys_are_the_agent_kind_names`
/// pins the two together.
const AGENT_KIND_SYMBOLS: &[&str] = &["claude", "pi", "hermes", "codex", "omp"];
```

Add `symbol_keys: NO_SYMBOLS,` to every entry in `SEGMENTS`, except `agent_bar`, which gets `symbol_keys: AGENT_KIND_SYMBOLS,`. If the test in Step 5 says the order differs from `AgentKind::ALL`, reorder the const to match `ALL`.

- [ ] **Step 4: Retype `SegmentConfig::symbols`**

In `src/ui/bar/segment.rs`, replace the `symbols` field and its doc comment:

```rust
    /// `[<segment>.symbols]`: a glyph per key from the segment's
    /// `SegmentDef::symbol_keys`, tried before `symbol`. `agent_bar` keys
    /// it by agent kind; `fold` by `expanded`/`folded`.
    pub symbols: Vec<(String, String)>,
```

And add to `impl SegmentConfig`:

```rust
    /// The glyph for `key` in this segment's `symbols` table, if set. An
    /// empty entry is a deliberate override and comes back as `Some("")`.
    pub fn symbol_for(&self, key: &str) -> Option<&str> {
        self.symbols
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, glyph)| glyph.as_str())
    }
```

- [ ] **Step 5: Drive `resolve_symbols` from the registry**

In `src/config/theme_file.rs`, replace `resolve_symbols` with:

```rust
/// Validate a `[<segment>.symbols]` table against the segment's
/// `SegmentDef::symbol_keys`: a segment with no keys takes no table at
/// all, and every key present must be one of its keys. Entries come back
/// in `symbol_keys` order regardless of the file's.
fn resolve_symbols(
    segment: &str,
    def: &SegmentDef,
    tbl: Option<&BTreeMap<String, String>>,
    errors: &mut Vec<ThemeError>,
) -> Vec<(String, String)> {
    let Some(tbl) = tbl else {
        return Vec::new();
    };
    let location = format!("[{segment}.symbols]");
    if def.symbol_keys.is_empty() {
        let takers: Vec<String> = SEGMENTS
            .iter()
            .filter(|d| !d.symbol_keys.is_empty())
            .map(|d| format!("[{}]", d.name))
            .collect();
        errors.push(error(
            location,
            format!("`symbols` is read by {} only", takers.join(" and ")),
        ));
        return Vec::new();
    }
    for key in tbl.keys() {
        if !def.symbol_keys.contains(&key.as_str()) {
            errors.push(error(
                location.clone(),
                format!(
                    "unknown key `{key}` (known: {})",
                    def.symbol_keys.join(", ")
                ),
            ));
        }
    }
    def.symbol_keys
        .iter()
        .filter_map(|&key| tbl.get(key).map(|s| (key.to_string(), s.clone())))
        .collect()
}
```

Change the call in `resolve_segment` to `let symbols = resolve_symbols(name, def, tbl.symbols.as_ref(), errors);`. Add `use crate::ui::bar::registry::SegmentDef;` if not already imported (the file already imports `SEGMENTS` and `segment_def`; extend that `use`). Remove the now-unused `AgentKind` import from this file if `cargo build` reports it.

- [ ] **Step 6: Update the readers in `providers.rs`**

`agent_bar` (around line 350): replace the `symbols.iter().find(...)` chain with

```rust
    let symbol = cfg
        .symbol_for(agent.display_name())
        .map(str::to_string)
        .or_else(|| cfg.symbol.clone())
        .unwrap_or_else(|| "▎".to_string());
```

`module_vars`: change the parameter to `icons: &[(String, String)]` and the loop body to `v.insert(format!("icon_{key}"), var(icon.clone()));` with `for (key, icon) in icons`.

`agents`: change the parameter to `icons: &[(String, String)]` and the lookup to

```rust
            if let Some((_, icon)) = icons.iter().find(|(k, _)| k == kind.display_name()) {
```

In the provider tests (around lines 909 and 935) replace `vec![(AgentKind::Codex, "X".to_string())]` with `vec![("codex".to_string(), "X".to_string())]` and `vec![(AgentKind::Pi, String::new())]` with `vec![("pi".to_string(), String::new())]`.

- [ ] **Step 7: Update the loader tests**

In `src/config/theme_file.rs` tests: in `agent_bar_symbols_parse_per_kind_and_keep_the_fallback_symbol` replace the expected vector with `vec![("claude".to_string(), "C".to_string()), ("codex".to_string(), "X".to_string())]` and drop the now-unused `use crate::pty::session::AgentKind;` inside it. `agent_bar_symbols_reject_unknown_kinds` and `symbols_table_is_an_error_off_agent_bar` keep their assertions (the new messages still contain `gpt`, `claude`, and `agent_bar`). Check `agent_bar_symbols_merge_per_kind` compiles unchanged (it works on `ThemeFile`, not `SegmentConfig`).

- [ ] **Step 8: Build, test, lint**

Run: `cargo build && cargo test --lib bar:: && cargo test --lib theme_file && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass. Fix any remaining `AgentKind`-typed `symbols` uses the compiler names (search `symbols` in `src/ui/bar/tests.rs` if it complains).

- [ ] **Step 9: Commit**

```bash
git add src/ui/bar/registry.rs src/ui/bar/segment.rs src/ui/bar/providers.rs src/config/theme_file.rs
git commit -m "Bar registry: per-segment symbol keys drive the symbols table

Claude-Session: https://claude.ai/code/session_01Kx4MjP6ySGuS716QEPAyUP"
```

---

### Task 2: Loader — `[dashboard_repo]`, the five segments, `pad`, the singleton scope

**Files:**
- Modify: `src/ui/bar/registry.rs` (five `SegmentDef`s; `FOLD_SYMBOLS` const)
- Modify: `src/ui/bar/segment.rs` (`SegmentConfig.pad`)
- Modify: `src/config/theme_file.rs` (`ThemeFile`, `SegmentTable`, merges, `resolve_segment`, `resolve`, `check_singletons`, `BarSpecs`)
- Modify: `src/ui/bar/default_theme.toml`
- Modify: `src/ui/bar/tests.rs:1025-1160` (`DASHBOARD_HEADER_ONLY` and a new `DASHBOARD_REPO_ONLY`)
- Modify: `src/ui/bar/providers.rs:855` and `src/ui/dashboard/detail.rs:816` (struct literals gain `pad: None`)

**Interfaces:**
- Consumes: `SegmentDef.symbol_keys` from Task 1.
- Produces: `ThemeFile.dashboard_repo: BarTable`; `BarSpecs.dashboard_repo: BarSpec`; `SegmentTable.pad: Option<String>`; `SegmentConfig.pad: Option<char>`; registry entries `fold`, `repo_name`, `pr_link`, `repo_path`, `status_counts`.

- [ ] **Step 1: Write the failing loader tests**

In the `tests` module of `src/config/theme_file.rs`, next to `partial_dashboard_detail_table_merges_over_the_default`:

```rust
    #[test]
    fn partial_dashboard_repo_table_merges_over_the_default() {
        let specs = ok("[dashboard_repo]\nformat = \"$repo_name\"\n");
        assert_eq!(
            specs.dashboard_repo.format,
            format::parse("$repo_name").unwrap()
        );
        assert_eq!(specs.dashboard_repo.fill, "─", "fill keeps the default");
        assert_eq!(
            specs.dashboard_repo.right_format,
            format::parse("( $status_counts)").unwrap(),
            "right side keeps the default"
        );
    }

    #[test]
    fn pr_link_twice_in_the_repo_bar_is_an_error() {
        let e = errs("[dashboard_repo]\nright_format = \"$pr_link $status_counts\"\n");
        assert!(
            e.iter().any(|e| e.location == "[dashboard_repo]"
                && e.message.contains("$pr_link")
                && e.message.contains("in the dashboard repo bar")),
            "{e:?}"
        );
    }

    #[test]
    fn pad_is_accepted_on_repo_name_only_and_must_be_one_char() {
        assert_eq!(ok("[repo_name]\npad = \" \"\n").segments["repo_name"].pad, Some(' '));
        assert_eq!(
            bundled_default(&Theme::wsx()).segments["repo_name"].pad,
            Some('─'),
            "the bundled default pads with a rule"
        );
        let e = errs("[repo_name]\npad = \"--\"\n");
        assert_eq!(e[0].location, "[repo_name].pad");
        assert!(e[0].message.contains("one character"), "{}", e[0].message);
        let e = errs("[pr]\npad = \"-\"\n");
        assert_eq!(e[0].location, "[pr].pad");
        assert!(e[0].message.contains("repo_name"), "{}", e[0].message);
    }

    #[test]
    fn fold_symbols_take_expanded_and_folded_only() {
        let specs = ok("[fold.symbols]\nexpanded = \"v\"\n");
        assert_eq!(specs.segments["fold"].symbol_for("expanded"), Some("v"));
        assert_eq!(
            specs.segments["fold"].symbol_for("folded"),
            Some("▸"),
            "the other key keeps the bundled default"
        );
        let e = errs("[fold.symbols]\nclaude = \"C\"\n");
        assert_eq!(e[0].location, "[fold.symbols]");
        assert!(e[0].message.contains("expanded"), "{}", e[0].message);
    }
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib theme_file::tests::partial_dashboard_repo theme_file::tests::pr_link_twice theme_file::tests::pad_is theme_file::tests::fold_symbols`
Expected: compile errors (`no field dashboard_repo`, `no field pad`).

- [ ] **Step 3: Register the five segments**

In `src/ui/bar/registry.rs` add `const FOLD_SYMBOLS: &[&str] = &["expanded", "folded"];` beside `AGENT_KIND_SYMBOLS`, and append to `SEGMENTS` after the `pr` entry:

```rust
    // --- the by-repo dashboard's per-repo header (`[dashboard_repo]`) ---
    SegmentDef {
        name: "fold",
        vars: &["symbol"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
        symbol_keys: FOLD_SYMBOLS,
    },
    SegmentDef {
        name: "repo_name",
        vars: &["pad", "name"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
        symbol_keys: NO_SYMBOLS,
    },
    SegmentDef {
        name: "pr_link",
        vars: &["symbol"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: true,
        symbol_keys: NO_SYMBOLS,
    },
    SegmentDef {
        name: "repo_path",
        vars: &["path"],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
        symbol_keys: NO_SYMBOLS,
    },
    SegmentDef {
        name: "status_counts",
        vars: &[
            "question", "stalled", "waiting", "thinking", "complete", "idle", "total",
        ],
        style_vars: STYLE,
        more_vars: NO_TAIL,
        items: false,
        singleton: false,
        symbol_keys: NO_SYMBOLS,
    },
```

Update `singleton_names_matches_the_flagged_entries` in the same file to expect `vec!["usage", "attention", "tags", "procs", "pr", "pr_link"]`. Update the Task 1 test `agent_bar_symbol_keys_are_the_agent_kind_names` so the loop skips `fold` too and asserts `segment_def("fold").unwrap().symbol_keys == ["expanded", "folded"]`.

- [ ] **Step 4: Add `pad` to the config types**

`src/ui/bar/segment.rs`, in `SegmentConfig` after `symbol`:

```rust
    /// `[repo_name].pad`: the character that fills the name's left pad
    /// (`$pad`). Only `repo_name` takes it; `None` on every other segment.
    pub pad: Option<char>,
```

`src/config/theme_file.rs`, in `SegmentTable` after `symbol`: `pub pad: Option<String>,` and in `SegmentTable::merge_over`: `pad: self.pad.or(base.pad),`. Add `pad: None,` to the `SegmentConfig` literals in `resolve_module`, `src/ui/bar/providers.rs` (`item_cfg`, ~line 855), and `src/ui/dashboard/detail.rs` (~line 816, if it builds a `SegmentConfig`; if it builds `BarSpecs` via the loader, nothing to do). Let `cargo build` find any other literal.

In `resolve_segment`, before `let symbols = ...`:

```rust
    let pad_loc = format!("[{name}].pad");
    let pad = match tbl.pad.as_deref() {
        Some(_) if name != "repo_name" => {
            errors.push(error(&pad_loc, "only [repo_name] takes `pad`"));
            None
        }
        Some(s) => {
            let mut chars = s.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => {
                    errors.push(error(&pad_loc, "`pad` must be exactly one character"));
                    None
                }
            }
        }
        None => None,
    };
```

and `pad,` in the returned `SegmentConfig`.

- [ ] **Step 5: Add the bar table**

`src/config/theme_file.rs`:
- `ThemeFile`: after `dashboard_detail`, add
  ```rust
      /// The by-repo dashboard's per-repo header line. A named field, not
      /// part of the flattened `segments` map, like the other bars.
      #[serde(default)]
      pub dashboard_repo: BarTable,
  ```
- `ThemeFile::merge_over`: `self.dashboard_repo = self.dashboard_repo.merge_over(base.dashboard_repo);`
- `BarSpecs`: `pub dashboard_repo: BarSpec,` after `dashboard_detail`.
- `resolve`: after the `dashboard_detail` `resolve_bar` call add
  ```rust
      let dashboard_repo = resolve_bar(
          "dashboard_repo",
          &file.dashboard_repo,
          &allowed_names,
          &resolver,
          &mut errors,
      );
  ```
  pass `&dashboard_repo` to `check_singletons` as a new last argument before `errors`, and add `dashboard_repo,` to the `Ok(BarSpecs { ... })` literal.
- `check_singletons`: add the parameter `repo: &BarSpec` and, after the detail scope:
  ```rust
      let repo_nodes: [&[Node]; 2] = [&repo.format, &repo.right_format];
      check_singleton_scope(
          "[dashboard_repo]",
          &repo_nodes,
          "in the dashboard repo bar",
          errors,
      );
  ```
  Update its doc comment to say five scopes.

- [ ] **Step 6: Add the bundled default tables**

In `src/ui/bar/default_theme.toml`, after the `[dashboard_detail]` table:

```toml
# The by-repo dashboard's per-repo header line, drawn once per repo:
# `▾ ── name  PR  /path/to/repo  ────  ? 1  ✓ 2    3 ws`. The two spaces
# closing `format` and the one opening `right_format` (plus the engine's
# mandatory blank) flank the rule with two cells each side; an empty repo
# has no counts, so no right side, and the rule runs to the edge. Cross-
# repo alignment (names right-justified to a shared column, one PR-link
# gutter for every repo) is computed by the view and arrives through
# `$pad` and `$pr_link`'s blank placeholder.
[dashboard_repo]
format       = "$fold $repo_name  ($pr_link  )$repo_path  "
right_format = "( $status_counts)"
fill         = "─"
fill_style   = "fg:dim"
```

And after the `[pr]` segment table, before the modules block:

```toml
# --- dashboard repo bar segments ---------------------------------------

# The fold glyph: `expanded` or `folded` from the table below, or a blank
# of the same width for a repo with no workspaces. Variables: $symbol
[fold]
format = "[$symbol]($style)"

[fold.symbols]
expanded = "▾"
folded   = "▸"

# The repo name, right-justified to the widest name in the list: `$pad`
# is `pad` repeated over the shortfall less one, then a space, and absent
# for the widest name. A space `pad` gives plain spaces, for a name on a
# coloured block. Variables: $pad $name
[repo_name]
pad    = "─"
format = "[$pad](fg:dim)[$name]($style)"

# The clickable "my open PRs" link: `PR`, or the Nerd Font pull-request
# glyph, lit in the open-PR green when a workspace has an open PR and dim
# otherwise. Absent when no repo in the list has a GitHub remote; a blank
# of the glyph's width, with no click, for a repo that has none while
# another does, so every path starts in the same column. `symbol`
# overrides the glyph. Variables: $symbol
[pr_link]
format = "[$symbol]($style)"

# Variables: $path
[repo_path]
format = "[$path](fg:dim)"

# This repo's workspaces by status, each count empty at zero so its group
# drops, then the total. Nothing at all for a repo with no workspaces.
# Variables: $question $stalled $waiting $thinking $complete $idle $total
[status_counts]
format = "([? $question](fg:question bold)  )([! $stalled](fg:stalled bold)  )([… $waiting](fg:waiting)  )([⠋ $thinking](fg:thinking)  )([✓ $complete](fg:complete)  )([· $idle](fg:dim)  )  [$total ws](fg:dim)"
```

Also extend the file's top comment: in the line `# Bars take \`format\`, ...` nothing changes; in the theme-token list nothing changes. Add `pad` to the segment-keys sentence: "Segments take `format`, `style` (…), `symbol`, `pad` (repo_name only), `disabled`, `priority`, and `separator` (…)".

- [ ] **Step 7: Keep the registry drift test honest**

In `src/ui/bar/tests.rs` (`segment_registry_drift_tests`), add beside `DASHBOARD_HEADER_ONLY`:

```rust
    /// The dashboard repo bar's five segments: registered like any other,
    /// but carrying per-repo state the attached bars never build.
    const DASHBOARD_REPO_ONLY: [&str; 5] =
        ["fold", "repo_name", "pr_link", "repo_path", "status_counts"];
```

and change the `expected` filter in `attached_segments_cover_every_registered_segment` to
`.filter(|name| !DASHBOARD_HEADER_ONLY.contains(name) && !DASHBOARD_REPO_ONLY.contains(name))`.
Add:

```rust
    #[test]
    fn dashboard_repo_segments_are_all_registered_names() {
        for name in DASHBOARD_REPO_ONLY {
            assert!(
                SEGMENTS.iter().any(|d| d.name == name),
                "dashboard_repo's `{name}` segment must be in registry::SEGMENTS"
            );
        }
    }
```

- [ ] **Step 8: Build, test, lint**

Run: `cargo test --lib && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass, including `bundled_default_parses_and_validates` and `every_example_theme_validates` (the examples inherit the new tables from the default). If a test in `src/ui/bar/tests.rs` pins the full singleton list or iterates `SEGMENTS` expecting a provider for each, it will name the gap; the composer comes in Task 3, so only the drift filter above should need touching here.

- [ ] **Step 9: Commit**

```bash
git add src/ui/bar/registry.rs src/ui/bar/segment.rs src/ui/bar/default_theme.toml src/ui/bar/tests.rs src/ui/bar/providers.rs src/config/theme_file.rs src/ui/dashboard/detail.rs
git commit -m "Theme loader: [dashboard_repo] bar, its five segments, pad, fold symbols

Claude-Session: https://claude.ai/code/session_01Kx4MjP6ySGuS716QEPAyUP"
```

---

### Task 3: Providers, `Hit::RepoPrs`, and the `dashboard_repo` composer

**Files:**
- Modify: `src/ui/bar/segment.rs:14-29` (`Hit`)
- Modify: `src/ui/bar/providers.rs` (five providers + `FoldState`, `PrLink`; tests)
- Modify: `src/ui/bar/bars.rs` (`DashboardRepoInputs`, `dashboard_repo`)
- Modify: `src/ui/bar/mod.rs` (re-exports)
- Modify: `src/ui/bar/tests.rs` (drift test coverage for the composer; pinned snapshot)

**Interfaces:**
- Consumes: registry entries and `SegmentConfig.pad`/`symbol_for` from Tasks 1–2; `crate::ui::dashboard::sort::StatusCounts`; `crate::ui::dashboard::status::Status`.
- Produces:
  ```rust
  pub enum Hit { ..., RepoPrs }
  #[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum FoldState { Expanded, Folded, Empty }
  #[derive(Debug, Clone, Copy)] pub struct PrLink<'a> { pub glyph: &'a str, pub linked: bool, pub open: bool }
  pub fn fold(cfg: &SegmentConfig, state: FoldState, theme: &Theme, resolver: &Resolver) -> Option<Segment>
  pub fn repo_name(cfg: &SegmentConfig, name: &str, pad_cells: usize, theme: &Theme, resolver: &Resolver) -> Option<Segment>
  pub fn pr_link(cfg: &SegmentConfig, link: Option<PrLink<'_>>, theme: &Theme, resolver: &Resolver) -> Option<Segment>
  pub fn repo_path(cfg: &SegmentConfig, path: &str, theme: &Theme, resolver: &Resolver) -> Option<Segment>
  pub fn status_counts(cfg: &SegmentConfig, counts: StatusCounts, resolver: &Resolver) -> Option<Segment>
  pub struct DashboardRepoInputs<'a> { pub fold: FoldState, pub name: &'a str, pub pad_cells: usize, pub path: &'a str, pub pr_link: Option<PrLink<'a>>, pub counts: StatusCounts, pub fleet: &'a SegmentMap }
  pub fn dashboard_repo(specs: &BarSpecs, theme: &Theme, inputs: &DashboardRepoInputs<'_>, width: u16) -> Rendered
  ```
  re-exported from `crate::ui::bar` as `DashboardRepoInputs`, `dashboard_repo`, `FoldState`, `PrLink`.

- [ ] **Step 1: Write the failing provider tests**

In the `tests` module of `src/ui/bar/providers.rs`:

```rust
    fn plain_cfg(format_src: &str) -> SegmentConfig {
        item_cfg(format_src, "")
    }

    fn resolver_for(theme: &Theme) -> Resolver<'_> {
        Resolver::new(EMPTY_PALETTE.get_or_init(HashMap::new), theme)
    }
    static EMPTY_PALETTE: std::sync::OnceLock<HashMap<String, Color>> = std::sync::OnceLock::new();

    #[test]
    fn fold_uses_its_symbols_and_blanks_an_empty_repo() {
        let theme = Theme::wsx();
        let r = resolver_for(&theme);
        let mut cfg = plain_cfg("[$symbol]($style)");
        cfg.symbols = vec![
            ("expanded".to_string(), "v".to_string()),
            ("folded".to_string(), ">".to_string()),
        ];
        let text = |s| fold(&cfg, s, &theme, &r).unwrap().plain_text();
        assert_eq!(text(FoldState::Expanded), "v");
        assert_eq!(text(FoldState::Folded), ">");
        assert_eq!(text(FoldState::Empty), " ", "a blank keeps the column");
        let bare = plain_cfg("[$symbol]($style)");
        assert_eq!(fold(&bare, FoldState::Expanded, &theme, &r).unwrap().plain_text(), "▾");
        assert_eq!(fold(&bare, FoldState::Folded, &theme, &r).unwrap().plain_text(), "▸");
    }

    #[test]
    fn repo_name_pads_with_the_configured_char_and_a_space() {
        let theme = Theme::wsx();
        let r = resolver_for(&theme);
        let cfg = plain_cfg("[$pad](fg:dim)[$name]($style)");
        assert_eq!(repo_name(&cfg, "wsx", 0, &theme, &r).unwrap().plain_text(), "wsx");
        assert_eq!(repo_name(&cfg, "wsx", 1, &theme, &r).unwrap().plain_text(), " wsx");
        assert_eq!(repo_name(&cfg, "wsx", 4, &theme, &r).unwrap().plain_text(), "─── wsx");
        let mut spaced = plain_cfg("[$pad](fg:dim)[$name]($style)");
        spaced.pad = Some(' ');
        assert_eq!(repo_name(&spaced, "wsx", 4, &theme, &r).unwrap().plain_text(), "    wsx");
    }

    #[test]
    fn pr_link_glyph_hit_and_gutter_placeholder() {
        use crate::git::forge::BranchLifecycle::PrOpen;
        let theme = Theme::wsx();
        let r = resolver_for(&theme);
        let cfg = plain_cfg("[$symbol]($style)");
        assert!(pr_link(&cfg, None, &theme, &r).is_none(), "no links anywhere: absent");

        let linked = pr_link(
            &cfg,
            Some(PrLink { glyph: "PR", linked: true, open: true }),
            &theme,
            &r,
        )
        .unwrap();
        assert_eq!(linked.plain_text(), "PR");
        assert_eq!(linked.hits.len(), 1);
        assert_eq!(linked.hits[0].hit, Hit::RepoPrs);
        assert_eq!((linked.hits[0].start_col, linked.hits[0].width), (0, 2));
        assert_eq!(linked.spans[0].style.fg, theme.lifecycle_style(Some(PrOpen)).unwrap().fg);

        let closed = pr_link(
            &cfg,
            Some(PrLink { glyph: "PR", linked: true, open: false }),
            &theme,
            &r,
        )
        .unwrap();
        assert_eq!(closed.spans[0].style.fg, theme.dim_style().fg);

        let bare = pr_link(
            &cfg,
            Some(PrLink { glyph: "PR", linked: false, open: false }),
            &theme,
            &r,
        )
        .unwrap();
        assert_eq!(bare.plain_text(), "  ", "holds the gutter open");
        assert!(bare.hits.is_empty(), "but nothing to click");

        let mut custom = plain_cfg("[$symbol]($style)");
        custom.symbol = Some("".to_string());
        let seg = pr_link(&custom, Some(PrLink { glyph: "PR", linked: true, open: false }), &theme, &r).unwrap();
        assert_eq!(seg.plain_text(), "", "symbol overrides the glyph");
    }

    #[test]
    fn status_counts_empties_zeros_and_vanishes_for_an_empty_repo() {
        use crate::ui::dashboard::sort::StatusCounts;
        let theme = Theme::wsx();
        let r = resolver_for(&theme);
        let cfg = plain_cfg("([? $question]()  )([✓ $complete]()  )([· $idle]()  )  [$total ws]()");
        let counts = StatusCounts { question: 1, complete: 2, ..Default::default() };
        assert_eq!(
            status_counts(&cfg, counts, &r).unwrap().plain_text(),
            "? 1  ✓ 2    3 ws"
        );
        assert!(status_counts(&cfg, StatusCounts::default(), &r).is_none());
    }
```

`item_cfg` already exists in that module; `Theme`, `Color`, `Hit`, `HashMap` are imported at the top of `providers.rs`. `StatusCounts` derives `Default` (`#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]` at `src/ui/dashboard/sort.rs:6`; confirm and add `Default` if missing).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test --lib providers::tests::fold_uses providers::tests::repo_name_pads providers::tests::pr_link_glyph providers::tests::status_counts_empties`
Expected: compile errors (`FoldState`, `PrLink`, `fold`, `Hit::RepoPrs` not found).

- [ ] **Step 3: Add `Hit::RepoPrs`**

In `src/ui/bar/segment.rs`, in `enum Hit` after `Pr`:

```rust
    /// A repo bar's "my open PRs" link: opens the author-filtered PR list
    /// for the repo the bar belongs to.
    RepoPrs,
```

`footer_action` already matches `_ => None`, so nothing else changes.

- [ ] **Step 4: Write the providers**

Append to `src/ui/bar/providers.rs` (before the `tests` module):

```rust
/// A repo bar's fold state, for `$fold`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldState {
    Expanded,
    Folded,
    /// No workspaces: nothing to fold, so the glyph's cells stay blank.
    Empty,
}

/// The fold glyph: `[fold.symbols]`' `expanded`/`folded` (bundled `▾`/`▸`),
/// or a blank the width of the expanded glyph for an empty repo, so the
/// name column stays aligned. `$style` is dim.
pub fn fold(
    cfg: &SegmentConfig,
    state: FoldState,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let theme = &cfg.theme(theme);
    let glyph = |key: &str, fallback: &str| cfg.symbol_for(key).unwrap_or(fallback).to_string();
    let symbol = match state {
        FoldState::Expanded => glyph("expanded", "▾"),
        FoldState::Folded => glyph("folded", "▸"),
        FoldState::Empty => {
            let expanded = glyph("expanded", "▾");
            " ".repeat(Span::raw(expanded.as_str()).width().max(1))
        }
    };
    eval_segment(
        cfg,
        &vars(vec![("symbol", var(symbol))]),
        theme.dim_style(),
        &[],
        resolver,
    )
}

/// The repo name, with `$pad` right-justifying it to the list's widest
/// name: `pad_cells` is how many cells short this name is; the pad is
/// `cfg.pad` (bundled `─`) repeated over all but the last of them, then
/// one space, and absent at zero. `$style` is the header style.
pub fn repo_name(
    cfg: &SegmentConfig,
    name: &str,
    pad_cells: usize,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let theme = &cfg.theme(theme);
    let mut v = vars(vec![("name", var(name))]);
    if pad_cells > 0 {
        let ch = cfg.pad.unwrap_or('─');
        let mut pad: String = std::iter::repeat_n(ch, pad_cells - 1).collect();
        pad.push(' ');
        v.insert("pad".to_string(), var(pad));
    }
    eval_segment(cfg, &v, theme.header_style(), &[], resolver)
}

/// What `$pr_link` draws for one repo. `None` at the composer means no
/// repo in the list has a link, so the segment is absent everywhere.
#[derive(Debug, Clone, Copy)]
pub struct PrLink<'a> {
    /// `PR`, or the Nerd Font pull-request glyph.
    pub glyph: &'a str,
    /// This repo has a GitHub remote, so the link is drawn and clickable.
    pub linked: bool,
    /// A workspace of this repo has an open, draft, or conflicted PR.
    pub open: bool,
}

/// The clickable "my open PRs" link. `cfg.symbol` overrides the glyph.
/// `$style` is the open-PR green when `open`, else dim — the same dim
/// the path takes, so the two read as one quiet cluster. A repo that is
/// not `linked` gets blanks of the glyph's width and no hit: the gutter
/// stays open so every path starts in the same column.
pub fn pr_link(
    cfg: &SegmentConfig,
    link: Option<PrLink<'_>>,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let theme = &cfg.theme(theme);
    let link = link?;
    let glyph = cfg
        .symbol
        .clone()
        .unwrap_or_else(|| link.glyph.to_string());
    if !link.linked {
        let blank = " ".repeat(Span::raw(glyph.as_str()).width());
        return eval_segment(
            cfg,
            &vars(vec![("symbol", var(blank))]),
            theme.dim_style(),
            &[],
            resolver,
        );
    }
    let style = if link.open {
        theme
            .lifecycle_style(Some(BranchLifecycle::PrOpen))
            .unwrap_or_else(|| theme.dim_style())
    } else {
        theme.dim_style()
    };
    let mut seg = eval_segment(
        cfg,
        &vars(vec![("symbol", var(glyph))]),
        style,
        &[],
        resolver,
    )?;
    seg.hit_from(0, Hit::RepoPrs);
    Some(seg)
}

/// The repo's display path. `$style` is dim.
pub fn repo_path(
    cfg: &SegmentConfig,
    path: &str,
    theme: &Theme,
    resolver: &Resolver,
) -> Option<Segment> {
    let theme = &cfg.theme(theme);
    eval_segment(
        cfg,
        &vars(vec![("path", var(path))]),
        theme.dim_style(),
        &[],
        resolver,
    )
}

/// This repo's workspaces by status. Each count is empty at zero, like
/// the fleet variables, so a `( … )` group around it drops; a repo with
/// no workspaces renders nothing at all. No state colour of its own: the
/// bundled format colours each count by its status token.
pub fn status_counts(
    cfg: &SegmentConfig,
    counts: crate::ui::dashboard::sort::StatusCounts,
    resolver: &Resolver,
) -> Option<Segment> {
    if counts.total() == 0 {
        return None;
    }
    let count = |n: u32| if n == 0 { String::new() } else { n.to_string() };
    let v = vars(vec![
        ("question", var(count(counts.question))),
        ("stalled", var(count(counts.stalled))),
        ("waiting", var(count(counts.waiting))),
        ("thinking", var(count(counts.thinking))),
        ("complete", var(count(counts.complete))),
        ("idle", var(count(counts.idle))),
        ("total", var(count(counts.total()))),
    ]);
    let resolver = &resolver.with_overlay(&cfg.palette);
    let style = segment_style(cfg, Style::default(), resolver);
    eval_format(cfg, &v, style, &[], HashMap::new(), &[], resolver)
}
```

`Span` is already imported in `providers.rs`. If `std::iter::repeat_n` is not on this toolchain (check `rust-toolchain.toml`; it is stable since 1.82), use `std::iter::repeat(ch).take(pad_cells - 1)`.

- [ ] **Step 5: Run the provider tests**

Run: `cargo test --lib providers::tests`
Expected: PASS.

- [ ] **Step 6: Write the composer**

In `src/ui/bar/bars.rs`, after `dashboard_detail`:

```rust
/// Everything one repo bar needs. The cross-repo alignment (`pad_cells`,
/// and `pr_link` being `Some` for every repo once any has a link) is the
/// view's to compute — the engine renders one line at a time.
pub struct DashboardRepoInputs<'a> {
    pub fold: providers::FoldState,
    pub name: &'a str,
    /// Cells this name falls short of the list's widest, for `$pad`.
    pub pad_cells: usize,
    pub path: &'a str,
    /// `None` when no repo in the list has a PR link.
    pub pr_link: Option<providers::PrLink<'a>>,
    pub counts: crate::ui::dashboard::sort::StatusCounts,
    pub fleet: &'a SegmentMap,
}

/// One repo's header line in the by-repo dashboard. Its five segments
/// are built here and nowhere else; `Hit::RepoPrs` rides on `$pr_link`.
pub fn dashboard_repo(
    specs: &BarSpecs,
    theme: &Theme,
    inputs: &DashboardRepoInputs<'_>,
    width: u16,
) -> Rendered {
    let resolver = specs.resolver(theme);
    let mut segments = SegmentMap::new();
    put(
        &mut segments,
        "fold",
        providers::fold(cfg(specs, "fold"), inputs.fold, theme, &resolver),
    );
    put(
        &mut segments,
        "repo_name",
        providers::repo_name(
            cfg(specs, "repo_name"),
            inputs.name,
            inputs.pad_cells,
            theme,
            &resolver,
        ),
    );
    put(
        &mut segments,
        "pr_link",
        providers::pr_link(cfg(specs, "pr_link"), inputs.pr_link, theme, &resolver),
    );
    put(
        &mut segments,
        "repo_path",
        providers::repo_path(cfg(specs, "repo_path"), inputs.path, theme, &resolver),
    );
    put(
        &mut segments,
        "status_counts",
        providers::status_counts(cfg(specs, "status_counts"), inputs.counts, &resolver),
    );
    put_modules(&mut segments, specs, inputs.fleet, &resolver);
    render_bar(
        &specs.dashboard_repo,
        &segments,
        &specs.segments,
        width,
        &resolver,
    )
}
```

In `src/ui/bar/mod.rs` extend the `pub use bars::{...}` list with `DashboardRepoInputs, dashboard_repo` and add `pub use providers::{FoldState, PrLink};`. Update the module doc comment's list of bars to include "the by-repo view's repo bars".

- [ ] **Step 7: Pin the bundled default's output**

In `src/ui/bar/tests.rs`, add a module after `dashboard_header_tests`:

```rust
/// The dashboard repo bar through the bundled default, pinned literally:
/// the same line `by_repo::header_line` drew by hand before it moved onto
/// the engine (its own tests remain the parity gate; this guards the
/// engine's spacing and fill cell for cell).
#[cfg(test)]
mod dashboard_repo_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;
    use crate::ui::bar::test_util::plain;
    use crate::ui::dashboard::sort::StatusCounts;

    fn repo_bar(width: u16, pr_link: Option<PrLink<'static>>, counts: StatusCounts) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        dashboard_repo(
            &specs,
            &theme,
            &DashboardRepoInputs {
                fold: FoldState::Expanded,
                name: "wsx",
                pad_cells: 6,
                path: "/home/eben/workspace/wsx",
                pr_link,
                counts,
                fleet: crate::ui::bar::fleet::empty(),
            },
            width,
        )
    }

    const COUNTS: StatusCounts = StatusCounts {
        question: 1,
        stalled: 1,
        waiting: 1,
        thinking: 0,
        complete: 1,
        idle: 0,
    };

    #[test]
    fn engine_repo_bar_matches_the_legacy_header() {
        let out = repo_bar(
            120,
            Some(PrLink { glyph: "PR", linked: true, open: true }),
            COUNTS,
        );
        let expected = format!(
            "▾ ───── wsx  PR  /home/eben/workspace/wsx  {}  ? 1  ! 1  … 1  ✓ 1    4 ws",
            "─".repeat(49)
        );
        assert_eq!(plain(&out.line), expected);
        assert_eq!(out.line.width(), 120);
        assert_eq!(out.hits.len(), 1);
        assert_eq!(out.hits[0].hit, Hit::RepoPrs);
        assert_eq!((out.hits[0].start_col, out.hits[0].width), (13, 2));
    }

    #[test]
    fn unlinked_repo_in_a_linked_list_keeps_the_gutter_blank() {
        let out = repo_bar(
            120,
            Some(PrLink { glyph: "PR", linked: false, open: false }),
            COUNTS,
        );
        assert!(plain(&out.line).starts_with("▾ ───── wsx      /home/eben/workspace/wsx  ─"));
        assert!(out.hits.is_empty());
    }

    #[test]
    fn empty_repo_runs_the_rule_to_the_edge() {
        let out = repo_bar(80, None, StatusCounts::default());
        let t = plain(&out.line);
        assert!(t.starts_with("▾ ───── wsx  /home/eben/workspace/wsx  ─"), "{t:?}");
        assert!(t.ends_with('─'), "{t:?}");
        assert_eq!(out.line.width(), 80);
    }
}
```

Column arithmetic for the first test: `▾ ` (2) + `───── ` (6) + `wsx` (3) + `  ` (2) puts `PR` at column 13; the left side is 43 cells, the right ` ? 1  ! 1  … 1  ✓ 1    4 ws` is 27, leaving a 50-cell gap of which the engine keeps one blank, so 49 rule characters. If the assertion fails, print `plain(&out.line)` and reconcile the arithmetic before touching the format.

- [ ] **Step 8: Cover the composer in the drift test**

In `segment_registry_drift_tests` add:

```rust
    /// The repo bar composer must build every dashboard-repo-only segment.
    #[test]
    fn dashboard_repo_builds_every_repo_only_segment() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let out = dashboard_repo(
            &specs,
            &theme,
            &DashboardRepoInputs {
                fold: FoldState::Folded,
                name: "a",
                pad_cells: 2,
                path: "/p",
                pr_link: Some(PrLink { glyph: "PR", linked: true, open: false }),
                counts: crate::ui::dashboard::sort::StatusCounts { idle: 1, ..Default::default() },
                fleet: crate::ui::bar::fleet::empty(),
            },
            80,
        );
        let t = crate::ui::bar::test_util::plain(&out.line);
        for needle in ["▸", "─ a", "PR", "/p", "1 ws"] {
            assert!(t.contains(needle), "{needle:?} missing from {t:?}");
        }
    }
```

- [ ] **Step 9: Build, test, lint**

Run: `cargo test --lib bar:: && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass.

- [ ] **Step 10: Commit**

```bash
git add src/ui/bar
git commit -m "Bar engine: dashboard_repo composer, fold/repo_name/pr_link/repo_path/status_counts providers

Claude-Session: https://claude.ai/code/session_01Kx4MjP6ySGuS716QEPAyUP"
```

---

### Task 4: `by_repo.rs` on the engine

**Files:**
- Modify: `src/ui/dashboard/by_repo.rs` (whole `header_line` body, `pr_link_gutter`, `render_list`, tests)
- Modify: `src/ui/dashboard/mod.rs:704-808` (`render_by_repo`, `render_list` call)

**Interfaces:**
- Consumes: `crate::ui::bar::{dashboard_repo, DashboardRepoInputs, FoldState, PrLink}`, `Hit::RepoPrs`.
- Produces:
  ```rust
  fn list_link_glyph(repos: &[RepoView<'_>]) -> Option<&'static str>
  fn link_glyph(nerd_fonts: bool) -> &'static str
  fn header_line(view: &RepoView<'_>, name_width: usize, list_glyph: Option<&str>, width: usize, theme: &Theme, specs: &BarSpecs, fleet: &SegmentMap) -> (Line<'static>, Option<PrLinkSpan>)
  pub fn render_list(repos: &[RepoView<'_>], widths: row::ColumnWidths, tick: u32, width: usize, theme: &Theme, specs: &BarSpecs, fleet: &SegmentMap) -> (Vec<ListItem<'static>>, Vec<RepoPrLinkSpan>)
  ```

- [ ] **Step 1: Point the existing tests at the new signature**

In the `tests` module of `src/ui/dashboard/by_repo.rs`, add a helper right after `header_text`:

```rust
    /// `header_line` through the bundled default, with the alignment inputs
    /// computed over `views` the way `render_list` does.
    fn line(
        view: &RepoView<'_>,
        views: &[RepoView<'_>],
        width: usize,
    ) -> (Line<'static>, Option<PrLinkSpan>) {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::bundled_default(&theme);
        header_line(
            view,
            name_align_width(views),
            list_link_glyph(views),
            width,
            &theme,
            &specs,
            crate::ui::bar::fleet::empty(),
        )
    }
```

Then, test by test:
- Every `header_line(&view, align, gutter, W, &theme)` / `header_line(&view, name_align_width(std::slice::from_ref(&view)), pr_link_gutter(...), W, &theme)` becomes `line(&view, std::slice::from_ref(&view), W)`; every `header_line(&views[i], name_width, gutter, W, &theme)` becomes `line(&views[i], &views, W)`. Delete the now-unused `let align = …`, `let gutter = …`, `let name_width = …` lines, and the `let theme = Theme::wsx();` lines that only fed `header_line` (keep the ones used for `theme.dim_style()` comparisons).
- `paths_align_whether_or_not_a_repo_has_a_pr_link`: replace `let gutter = pr_link_gutter(&views); assert!(gutter > 0, ...)` with `assert!(list_link_glyph(&views), "a linked repo in the list opens a gutter");`.
- `pr_link_style` helper: `let (line, _) = line(view, std::slice::from_ref(view), 120);` (rename the local to `l` to avoid shadowing the helper).
- Both `render_list(...)` calls gain two trailing arguments: `&crate::config::theme_file::bundled_default(&theme), crate::ui::bar::fleet::empty()`.
- Rewrite `counts_stay_flush_right_without_overflow` to the engine's overflow rule:

```rust
    #[test]
    fn counts_stay_flush_right_without_overflow() {
        // Once the line fits, it is exactly `width` wide and the counts end
        // at the right edge, at every width; below that the engine omits
        // the right side (rather than pushing the counts past the edge)
        // and the left side stays at its own minimum. Swept with and
        // without the PR link, which adds cells left of the rule.
        let repos = fixture::repos();
        let wsx = repos.iter().find(|r| r.name == "wsx").unwrap();
        for (label, view) in [
            ("no link", make_view(wsx, 1, true)),
            ("plain link", pr_link_view(wsx, true, false)),
            ("nerd link", pr_link_view(wsx, true, true)),
        ] {
            let views = std::slice::from_ref(&view);
            let left_min = header_text(&line(&view, views, 0).0).chars().count();
            let mut fits_from = None;
            for width in 0..=200 {
                let t = header_text(&line(&view, views, width).0);
                let len = t.chars().count();
                if t.contains("4 ws") {
                    fits_from.get_or_insert(width);
                    assert_eq!(len, width, "{label} width={width}: exactly `width` once it fits");
                    assert_eq!(substr_end_col(&line(&view, views, width).0, "4 ws"), width, "{label} width={width}");
                } else {
                    assert!(fits_from.is_none(), "{label} width={width}: counts vanished after fitting");
                    assert_eq!(len, width.max(left_min), "{label} width={width}");
                }
            }
            assert!(fits_from.is_some(), "{label}: counts never fit by 200 columns");
        }
    }
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test --lib by_repo`
Expected: compile errors (`list_link_glyph` not found, wrong arity).

- [ ] **Step 3: Replace the builder with the adapter**

In `src/ui/dashboard/by_repo.rs`:
- Delete `RULE_PAD`, `PR_LINK_PAD`, `pr_link_glyph`, and `pr_link_gutter`. Keep `PR_LINK_NERD`, `PR_LINK_PLAIN`, `has_open_pr`, `name_align_width`, `PrLinkSpan`, `RepoPrLinkSpan`.
- Add:

```rust
/// The PR link's glyph for the nerd-font setting.
fn link_glyph(nerd_fonts: bool) -> &'static str {
    if nerd_fonts {
        PR_LINK_NERD
    } else {
        PR_LINK_PLAIN
    }
}

/// Whether any repo in the list paints a PR link. When one does, every
/// repo's `$pr_link` renders — a blank of the glyph's width for the
/// others — so every path starts in the same column; when none does, the
/// segment is absent everywhere and its group drops.
fn list_link_glyph(repos: &[RepoView<'_>]) -> Option<&'static str> {
    repos.iter().any(|v| v.show_pr_link)
}
```

- Replace `header_line` wholesale:

```rust
/// Build a repo header line through the bar engine's `[dashboard_repo]`
/// bar, plus the span of its clickable PR link when one was painted. The
/// span comes from the engine's own hit list, so the paint and the click
/// target can't drift — the same contract `row::pr_chip_hit_span` keeps
/// for workspace rows. `name_width` and `any_link` are the cross-repo
/// alignment inputs (`name_align_width`, `list_link_glyph`).
fn header_line(
    view: &RepoView<'_>,
    name_width: usize,
    any_link: bool,
    width: usize,
    theme: &Theme,
    specs: &BarSpecs,
    fleet: &SegmentMap,
) -> (Line<'static>, Option<PrLinkSpan>) {
    let fold = if view.counts.total() == 0 {
        FoldState::Empty
    } else if view.expanded {
        FoldState::Expanded
    } else {
        FoldState::Folded
    };
    let pr_link = any_link.then(|| PrLink {
        glyph: link_glyph(view.nerd_fonts),
        linked: view.show_pr_link,
        open: has_open_pr(view),
    });
    let rendered = dashboard_repo(
        specs,
        theme,
        &DashboardRepoInputs {
            fold,
            name: view.name,
            pad_cells: name_width.saturating_sub(view.name.chars().count()),
            path: &view.path,
            pr_link,
            counts: view.counts,
            fleet,
        },
        u16::try_from(width).unwrap_or(u16::MAX),
    );
    let span = rendered
        .hits
        .iter()
        .find(|h| h.hit == Hit::RepoPrs)
        .map(|h| (h.start_col, h.width));
    (rendered.line, span)
}
```

with imports at the top of the file:

```rust
use crate::config::theme_file::BarSpecs;
use crate::ui::bar::segment::{Hit, SegmentMap};
use crate::ui::bar::{DashboardRepoInputs, FoldState, PrLink, dashboard_repo};
```

Drop the now-unused `Modifier`, `Span`, and `Status` imports if the compiler flags them. Update the module doc comment's second paragraph to say the header is drawn through the engine's `[dashboard_repo]` bar and point at the spec.

- `render_list` gains `specs: &BarSpecs, fleet: &SegmentMap` and computes `let any_link = list_link_glyph(repos);` in place of `gutter`, calling `header_line(view, name_width, any_link, width, theme, specs, fleet)`.

- [ ] **Step 4: Thread `specs` through the dashboard**

In `src/ui/dashboard/mod.rs`:
- `render_by_repo` gains a final parameter `specs: &crate::config::theme_file::BarSpecs` and calls `by_repo::render_list(&views, widths, tick, width, theme, specs, inputs.fleet)`.
- `render_without_footer` passes `specs` in its `GroupMode::Repo => render_by_repo(inputs, state, tick, width, theme, specs)` arm.

- [ ] **Step 5: Run the dashboard tests**

Run: `cargo test --lib dashboard`
Expected: PASS, including `by_repo_render_includes_chrome_status_strip_and_a_repo_header`, the PR-link rect test near `src/ui/dashboard/tests.rs:126`, and all of `by_repo::tests`. If `header_shows_fold_glyph_and_counts` fails on the `"▾ wsx  /home/eben/workspace/wsx  "` prefix, the `($pr_link  )` group did not drop: check that `list_link_glyph` is false for that fixture and that `pr_link` returns `None` for `link == None`.

- [ ] **Step 6: Full gate**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass. `click_chip_auto_spawns_session_when_missing` is a known flaky PTY-timing test; rerun it alone if it is the only failure.

- [ ] **Step 7: Commit**

```bash
git add src/ui/dashboard/by_repo.rs src/ui/dashboard/mod.rs
git commit -m "Dashboard repo bars drawn through the bar engine's [dashboard_repo] table

Claude-Session: https://claude.ai/code/session_01Kx4MjP6ySGuS716QEPAyUP"
```

---

### Task 5: Example themes

**Files:**
- Modify: `docs/examples/theme-orange.toml`, `theme-starship.toml`, `theme-nord.toml`, `theme-nord0.toml`, `theme-jellybeans.toml`, `theme-rose-pine.toml`, `theme-rose-pine-moon.toml`
- Modify: `src/ui/bar/tests.rs` (`example_theme_tests`)

**Interfaces:**
- Consumes: `dashboard_repo`, `DashboardRepoInputs`, `FoldState`, `PrLink` from Task 3; `load` from the loader.

- [ ] **Step 1: Write the failing example test**

In `example_theme_tests` (`src/ui/bar/tests.rs`):

```rust
    /// Every example carries its look onto the repo bars: each places all
    /// five repo segments in `[dashboard_repo]`, and a linked, expanded
    /// repo renders its name, link, path, and counts through it.
    #[test]
    fn every_example_theme_themes_the_repo_bar() {
        use crate::ui::dashboard::sort::StatusCounts;
        for path in example_themes() {
            let theme = Theme::wsx();
            let specs = load(&path, &theme).unwrap();
            let placed = crate::ui::bar::format::vars(&specs.dashboard_repo.format)
                .into_iter()
                .chain(crate::ui::bar::format::vars(&specs.dashboard_repo.right_format))
                .collect::<Vec<_>>();
            for seg in ["fold", "repo_name", "pr_link", "repo_path", "status_counts"] {
                assert!(placed.contains(&seg), "{}: ${seg} not placed", path.display());
            }
            assert_ne!(
                specs.dashboard_repo.format,
                crate::config::theme_file::bundled_default(&theme).dashboard_repo.format,
                "{}: repo bar left at the bundled default",
                path.display()
            );
            let out = dashboard_repo(
                &specs,
                &theme,
                &DashboardRepoInputs {
                    fold: FoldState::Expanded,
                    name: "wsx",
                    pad_cells: 3,
                    path: "/home/eben/wsx",
                    pr_link: Some(PrLink { glyph: "PR", linked: true, open: true }),
                    counts: StatusCounts { question: 1, complete: 2, ..Default::default() },
                    fleet: crate::ui::bar::fleet::empty(),
                },
                120,
            );
            let t = plain(&out.line);
            for needle in ["wsx", "PR", "/home/eben/wsx", "3 ws"] {
                assert!(t.contains(needle), "{}: {needle:?} missing from {t:?}", path.display());
            }
            assert_eq!(out.hits.iter().filter(|h| h.hit == Hit::RepoPrs).count(), 1, "{}", path.display());
            assert_eq!(out.line.width(), 120, "{}", path.display());
        }
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test --lib example_theme_tests::every_example_theme_themes_the_repo_bar`
Expected: FAIL on "repo bar left at the bundled default" for the first example.

- [ ] **Step 3: Orange (airline)**

In `docs/examples/theme-orange.toml`, after the `[attached_bottom]` table and before the `# --- segments` line, add:

```toml
# The repo bar as one more airline chain. The fold glyph sits flat on the
# bar in orange, then the name on the orange mode block (right-justified
# inside it with plain spaces, so every block has the same width and a
# straight edge), then the PR link and path together on smoke — the link
# takes the path's cream when there is nothing open behind it, mint when
# there is — capping into the bar. The status counts sit on an ash block
# at the right edge, pointing left, and the block drops with them for an
# empty repo.
[dashboard_repo]
format       = "$fold [ $repo_name ](bg:orange fg:black bold)[](fg:orange bg:smoke)[ ($pr_link )$repo_path ](bg:smoke fg:cream)[](fg:smoke)"
right_format = "([](fg:ash)[ $status_counts ](bg:ash))"
fill         = " "
```

and after the `[pr.palette]` table at the end of the file:

```toml
# --- repo bar segments --------------------------------------------------

[fold]
format = "[$symbol](fg:orange)"

[fold.symbols]
expanded = ""
folded   = ""

# Plain spaces pad the name inside its block.
[repo_name]
pad    = " "
format = "$pad$name"

# Inherit the block's cream; the open-PR tint is the same mint the
# workspace and pr blocks use, and the closed/none case borrows cream.
[pr_link.palette]
ok  = "mint"
dim = "cream"

[repo_path]
format = "$path"

# Status colours on ash, with idle and the total in chalk instead of the
# base theme's dim, which sinks into the block.
[status_counts]
format = "([? $question](fg:cream bold)  )([! $stalled](fg:blood bold)  )([… $waiting](fg:waiting)  )([⠋ $thinking](fg:thinking)  )([✓ $complete](fg:green)  )([· $idle](fg:chalk)  )  [$total ws](fg:chalk)"
```

- [ ] **Step 4: Starship (grey blocks, rounded caps)**

In `docs/examples/theme-starship.toml`, after the `[dashboard_header]` table:

```toml
# ----------------------------------------------------------- repo bars

# Each repo's header as a chain like the header's: the fold glyph flat on
# the bar in `first`, the name on `first` in the accent (padded with plain
# spaces inside its block), the PR link and path on `second`, and the
# counts on `fourth` at the right edge — the block drops with them for an
# empty repo.
[dashboard_repo]
format       = "$fold [](fg:first)[ $repo_name ](bg:first fg:orange bold)[](fg:first bg:second)[ ($pr_link )$repo_path ](bg:second fg:label)[](fg:second)"
right_format = "([](fg:fourth)[ $status_counts ](bg:fourth fg:label))"
fill         = " "
```

and at the end of the file:

```toml
# ------------------------------------------------------ repo bar segments

[fold]
format = "[$symbol](fg:first)"

[fold.symbols]
expanded = ""
folded   = ""

[repo_name]
pad    = " "
format = "$pad$name"

# Inherit the block's label colour when nothing is open; `added` (the
# git_metrics green) when something is.
[pr_link.palette]
ok  = "added"
dim = "label"

[repo_path]
format = "$path"

# The counts block is a mid grey, so the base theme's status tints don't
# carry: inherit the block's label colour and mark urgency with bold.
[status_counts]
format = "([? $question](bold)  )([! $stalled](bold)  )([… $waiting]()  )([⠋ $thinking]()  )([✓ $complete]()  )([· $idle]()  )  [$total ws]()"
```

- [ ] **Step 5: Nord, Nord0, Jellybeans, Rosé Pine, Rosé Pine Moon**

Each of the five files gets, after its `[dashboard_header]` table, a `[dashboard_repo]` block, and at the end of the file the same four segment tables. The format is identical across them; only the palette names differ:

| File | name block `A`/`A_fg` | link+path block `B`/`B_fg` | counts block `C`/`C_fg` |
|---|---|---|---|
| `theme-nord.toml` | `nord8` / `nord1` | `nord2` / `nord4` | `nord2` / `nord4` |
| `theme-nord0.toml` | `nord8` / `nord0` | `nord1` / `nord4` | `nord1` / `nord4` |
| `theme-jellybeans.toml` | `blue` / `bg` | `plum` / `fg` | `mid` / `fg` |
| `theme-rose-pine.toml` | `rose` / `base` | `hl_med` / `text` | `hl_high` / `text` |
| `theme-rose-pine-moon.toml` | `rose` / `base` | `hl_med` / `text` | `hl_high` / `text` |

The block, with `A`, `A_fg`, `B`, `B_fg`, `C`, `C_fg` substituted from the row:

```toml
# The repo bar as a chain like the header's: the fold glyph flat on the
# bar in the name block's colour, the name on A (padded with plain spaces
# inside its block, so every block is the same width), the PR link and
# path together on B, and the counts on C at the right edge — that block
# drops with them for an empty repo.
[dashboard_repo]
format       = "$fold [](fg:A)[ $repo_name ](bg:A fg:A_fg bold)[](fg:A bg:B)[ ($pr_link )$repo_path ](bg:B fg:B_fg)[](fg:B)"
right_format = "([](fg:C)[ $status_counts ](bg:C fg:C_fg))"
fill         = " "
```

and the segment tables:

```toml
# --- repo bar segments --------------------------------------------------

[fold]
format = "[$symbol](fg:A)"

[repo_name]
pad    = " "
format = "$pad$name"

# The link takes the path's colour beside it when nothing is open; the
# open-PR green stays the base theme's.
[pr_link.palette]
dim = "B_fg"

[repo_path]
format = "$path"

# Status colours on the dark counts block; idle and the total inherit the
# block's text colour instead of the base theme's dim.
[status_counts]
format = "([? $question](fg:question bold)  )([! $stalled](fg:stalled bold)  )([… $waiting](fg:waiting)  )([⠋ $thinking](fg:thinking)  )([✓ $complete](fg:complete)  )([· $idle]()  )  [$total ws]()"
```

Check each file's existing header uses ``/`` (the rounded caps) — they all do per their `[dashboard_header]` — so the repo bar's caps match.

- [ ] **Step 6: Validate every example from the CLI too**

Run:

```bash
cargo test --lib example_theme_tests
for f in docs/examples/theme-*.toml; do cargo run -q -- theme check "$f" || echo "FAIL $f"; done
```

Expected: the test module passes; every `theme check` exits 0 (a `note:` about `bar_theme` being off is fine).

- [ ] **Step 7: Commit**

```bash
git add docs/examples src/ui/bar/tests.rs
git commit -m "Example themes: carry each look onto the dashboard repo bars

Claude-Session: https://claude.ai/code/session_01Kx4MjP6ySGuS716QEPAyUP"
```

---

### Task 6: Book, manual test, spec status

**Files:**
- Modify: `docs/book/src/configuration/themes.md` (Bars block ~150-181; the `[dashboard_detail]` paragraph ~190; the segment-keys paragraph ~251-290; the segment table ~360-385; the singleton paragraph ~376-390; the "only … produce output" paragraph ~393-401)
- Modify: `docs/manual-tests/bar-theming.md` (append a section)
- Modify: `docs/superpowers/specs/2026-09-17-dashboard-repo-bar-theming-design.md` (status line)

- [ ] **Step 1: Book — Bars**

In the Bars code block, after the `[dashboard_detail]` table, add:

```toml
[dashboard_repo]
format       = "$fold $repo_name  ($pr_link  )$repo_path  "
right_format = "( $status_counts)"
fill         = "─"
fill_style   = "fg:dim"
```

After the `[dashboard_detail]` explanatory paragraph add:

```markdown
`[dashboard_repo]` is the by-repo dashboard's per-repo header line, drawn
once per repo: the fold glyph, the repo name right-justified to the widest
name in the list, the clickable "my open PRs" link, the path, a rule, and
the repo's workspaces counted by status flush right. Its five segments —
`fold`, `repo_name`, `pr_link`, `repo_path`, `status_counts` — are listed
below and produce output only in this bar. The alignment across repos is
computed by the view and reaches the theme as `$pad` on `repo_name` and as
a blank placeholder from `pr_link` on a repo without a link when another
repo has one; a theme that pads the name with spaces (`[repo_name] pad =
" "`) keeps the shared name column inside a coloured block. An empty repo
renders no `$status_counts`, so the bar has no right side and the fill
runs to the edge. Every bar side keeps one mandatory blank between
nonempty sides, so the stock `right_format` needs only one leading space
to reproduce the two-cell pad each side of the rule.
```

- [ ] **Step 2: Book — segment keys**

In the paragraph beginning "Each segment has its own table", after the `symbol` mention add: "`pad` (on `repo_name` only: the one character that fills `$pad`; a space gives plain spaces)". Replace "Likewise `agent_bar` alone takes a `symbols` sub-table, one glyph per agent kind, tried ahead of `symbol`:" with "Likewise two segments take a `symbols` sub-table, tried ahead of `symbol`: `agent_bar`, one glyph per agent kind, and `fold`, whose keys are `expanded` and `folded`:" and after the existing `[agent_bar.symbols]` example block add:

```toml
[fold.symbols]
expanded = ""   # nf-fa-chevron_down
folded   = ""   # nf-fa-chevron_right
```

Change "Keys must be agent kind names; any other key, or the table on another segment, is an error." to "Keys must be that segment's (agent kind names for `agent_bar`, `expanded`/`folded` for `fold`); any other key, or the table on any other segment, is an error."

- [ ] **Step 3: Book — segment table rows**

Append to the segment table after the `pr` row:

```markdown
| `fold` | `$symbol` | The repo bar's fold glyph: `expanded` or `folded` from `[fold.symbols]`, or a blank of the same width for a repo with no workspaces. `$style` is dim. Dashboard repo bar only. |
| `repo_name` | `$pad $name` | The repo name; `$pad` right-justifies it to the list's widest name (`pad` repeated, then a space) and is absent for the widest. `$style` is the header style. Dashboard repo bar only. |
| `pr_link` | `$symbol` | The "my open PRs" link (`PR`, or the Nerd Font pull-request glyph; `symbol` overrides). `$style` is the open-PR green when a workspace has an open PR, else dim. Blank, with no click, for a repo without a GitHub remote when another repo has one; absent when none does. Clickable. Dashboard repo bar only. |
| `repo_path` | `$path` | The repo's path. `$style` is dim. Dashboard repo bar only. |
| `status_counts` | `$question $stalled $waiting $thinking $complete $idle $total` | This repo's workspaces by dashboard status; each empty at zero, and the whole segment empty for a repo with no workspaces. Dashboard repo bar only. |
```

- [ ] **Step 4: Book — singletons and scopes**

In the paragraph "`pr`, `procs`, `usage`, `attention`, and `tags` may each be placed only once", add `pr_link` to the list ("`pr`, `procs`, `usage`, `attention`, `tags`, and `pr_link`"), add "or twice within the dashboard repo bar's" to the scope list, and change "These four scopes are independent" to "These five scopes are independent". In the paragraph "On the dashboard footer, only `keys`, `version`, and `usage` produce output; …", add "; on the dashboard repo bar, only `fold`, `repo_name`, `pr_link`, `repo_path`, and `status_counts`".

In the "stock formats" bullet that lists which segments bind `$style`, add `fold`, `repo_name`, `pr_link`, and `repo_path` to the list of those whose stock formats bind it.

- [ ] **Step 5: Manual test**

Append to `docs/manual-tests/bar-theming.md`:

```markdown
## 9. Repo bars

Spec: `docs/superpowers/specs/2026-09-17-dashboard-repo-bar-theming-design.md`.

With `bar_theme` off, the dashboard's by-repo view is unchanged:
`▾ ── name  PR  /path  ────  ? 1  ✓ 2    3 ws`, names right-justified to
a shared column, paths in a shared column whether or not a repo has a PR
link, an empty repo's rule running to the edge.

Copy `docs/examples/theme-orange.toml` over the file and turn
`bar_theme` on. Expected within a second: every repo header becomes an
airline chain — orange name block, smoke link-and-path block, ash counts
block at the right edge; names stay right-justified inside blocks of
equal width; an empty repo shows no counts block; the fold glyph is a
chevron. Fold and unfold a repo (`h`/`l`): the chevron turns. Click the
PR link: the browser opens the repo's open-PR list, exactly as before.

Edit `[dashboard_repo]` `format` to move `$pr_link` after `$repo_path`
and save: the link moves and stays clickable at its new position. Set
`right_format = "$pr_link $status_counts"` and save: the bar keeps its
last good look and the footer notice names `[dashboard_repo]` and
`$pr_link` (a singleton placed twice). Restore the file.

Resize the terminal below the width of the longest header: the counts
disappear from that header first, the left side clips at the edge, and
nothing overlaps.
```

- [ ] **Step 6: Spec status line and docs test**

In the spec, the `**Status:**` line already names this plan. Run:

```bash
cargo test --lib registry::tests && cargo test --lib theme_file
```

Expected: PASS (`every_fleet_var_is_documented_in_the_default_toml_and_the_book` is unaffected; nothing else reads the book). Then, if the repo has an mdbook check (`ls docs/book/book.toml`), run `mdbook build docs/book` if `mdbook` is installed; otherwise skip and say so.

- [ ] **Step 7: Full gate and commit**

Run: `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: all pass.

```bash
git add docs/book/src/configuration/themes.md docs/manual-tests/bar-theming.md docs/superpowers/specs/2026-09-17-dashboard-repo-bar-theming-design.md
git commit -m "docs: [dashboard_repo] bar, its segments, pad and fold symbols; manual test

Claude-Session: https://claude.ai/code/session_01Kx4MjP6ySGuS716QEPAyUP"
```

---

## Self-review notes

- Spec coverage: symbols generalisation (T1); `[dashboard_repo]`, `pad`, singleton scope, registry (T2); providers, `Hit::RepoPrs`, composer, pinned snapshot (T3); `by_repo.rs` adapter, parity tests, threading `specs` (T4); example themes + examples test (T5); book, manual test (T6). The spec's "accepted differences" are encoded in the rewritten overflow test in T4.
- Names used consistently: `dashboard_repo`, `DashboardRepoInputs`, `FoldState`, `PrLink`, `Hit::RepoPrs`, `list_link_glyph`, `link_glyph`, `symbol_for`, `symbol_keys`, `pad`.
