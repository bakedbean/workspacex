# Detail-bar modules and containers — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the workspace detail bar's body from three hard-coded columns into a configurable sequence of 1–4 containers, each holding one or more modules drawn from a registry. Implements [issue #98](https://github.com/bakedbean/workspacex/issues/98) per [`docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`](../specs/2026-05-25-detail-bar-modules-design.md).

**Architecture:** A new `src/detail_modules/` crate-root directory owns a `DetailModule` trait, a `Registry` (owned by `App`), a `DetailContext<'a>` push-bundle, and one file per built-in (`session_summary`, `recent_chat`, `processes`, `recent_files`). `src/detail_bar_config.rs` is replaced with a `containers: Vec<Vec<String>>` schema (whole-replace per-repo override; scalars still per-field). `src/ui/dashboard/detail.rs` keeps chrome (header, rules, reply, height math) but its body renderer becomes a generic container/module splitter that dispatches via the registry.

**Tech Stack:** Rust 2024 edition, `ratatui` 0.29 (`Layout::vertical/horizontal`, `Constraint`, `Frame`), `serde` + `serde_json` (config schema), `tracing` (warnings), `rusqlite` (no schema change — the `detail_bar_config TEXT` column on `repos` already exists). Tests follow the existing in-file `#[cfg(test)] mod tests` pattern with `Buffer`-backed `Frame` assertions.

---

## File Structure

**New files:**
- `src/detail_modules/mod.rs` — trait `DetailModule`, `DetailContext<'a>`, `Registry`, `register_builtins()`.
- `src/detail_modules/session_summary.rs` — `SessionSummary` struct + `impl DetailModule`.
- `src/detail_modules/recent_chat.rs` — `RecentChat` struct + `impl DetailModule`.
- `src/detail_modules/processes.rs` — `Processes` struct + `impl DetailModule` + extracted `build_processes` builder.
- `src/detail_modules/recent_files.rs` — `RecentFiles` struct + `impl DetailModule` + extracted `build_recent_files` builder.
- `docs/manual-tests/detail-bar-modules.md` — manual walkthrough.

**Modified files:**
- `src/detail_bar_config.rs` — schema replaced (drop `Sections`/`SectionsOverride`; add `containers`); `sanitize` extended; tests rewritten.
- `src/lib.rs` — declare `pub mod detail_modules;`.
- `src/ui/dashboard/detail.rs` — `DetailInputs` gains `registry: &'a Registry`; `pr_title` becomes `Option<&'a str>`; body renderer replaced; `Column`, `enabled_columns`, `column_widths`, `render_column`, `build_session_summary`, `build_recent_chat`, `build_procs_and_files` deleted (their bodies move into module files); a new `render_unknown_placeholder` is added.
- `src/app.rs` — `App` gains `pub registry: detail_modules::Registry`; `App::new` calls `register_builtins`; `DetailInputs` construction in the draw path threads `&self.registry` and switches `pr_title` to a borrow.
- `src/cli.rs` — `wsx config edit detail_bar_config` editor seed updated; `wsx config set detail_bar_config <file>` parser swapped (any spot that constructs/parses `DetailBarConfig` follows).
- `README.md` — replace the old `detail_bar_config` documentation block (`sections.*` keys) with the new `containers` schema. The block lives in the existing config-reference section.

**Files whose tests change but whose code doesn't change beyond the above list:**
- `src/store.rs` — no code change; existing tests for `detail_bar_config TEXT` column stay valid.

---

## Phase A — Foundation: new config schema

### Task 1: Replace `DetailBarConfig` schema (drop sections, add containers)

**Files:**
- Modify: `src/detail_bar_config.rs:1-655`

The whole file is replaced. Existing tests for `sections.*` go; new tests for `containers` come in. We do this as one task because the struct and its impls are interdependent — a partial swap won't compile.

- [ ] **Step 1: Write the failing tests** (replace the existing `mod tests` block at the bottom of `src/detail_bar_config.rs`)

Keep the imports block (`use super::*;` and the `test_repo`/`Store` helpers). Replace the `#[test]` functions with these. They reference `containers`, which doesn't exist yet — that's the failure we expect.

```rust
#[test]
fn default_matches_documented_baseline() {
    let cfg = DetailBarConfig::default();
    assert!(cfg.visible);
    assert_eq!(cfg.height.percent, 30);
    assert_eq!(cfg.height.min_rows, 8);
    assert_eq!(cfg.height.max_rows, 18);
    assert_eq!(
        cfg.containers,
        vec![
            vec!["session_summary".to_string()],
            vec!["recent_chat".to_string()],
            vec!["processes".to_string(), "recent_files".to_string()],
        ]
    );
}

#[test]
fn default_round_trips_through_json() {
    let cfg = DetailBarConfig::default();
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: DetailBarConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, parsed);
}

#[test]
fn parsing_empty_object_yields_default() {
    let parsed: DetailBarConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(parsed, DetailBarConfig::default());
}

#[test]
fn parsing_partial_blob_fills_missing_with_defaults() {
    let parsed: DetailBarConfig = serde_json::from_str(r#"{"visible": false}"#).unwrap();
    assert!(!parsed.visible);
    assert_eq!(parsed.height, Height::default());
    assert_eq!(parsed.containers, DetailBarConfig::default().containers);
}

#[test]
fn parsing_containers_list_of_lists() {
    let parsed: DetailBarConfig = serde_json::from_str(
        r#"{"containers": [["a", "b"], ["c"]]}"#,
    ).unwrap();
    assert_eq!(
        parsed.containers,
        vec![vec!["a".to_string(), "b".to_string()], vec!["c".to_string()]]
    );
}

#[test]
fn parsing_unknown_fields_succeeds() {
    let json = r#"{"unknown_future_field": 123, "visible": true}"#;
    let parsed: DetailBarConfig = serde_json::from_str(json).unwrap();
    assert!(parsed.visible);
}

#[test]
fn has_body_true_when_any_container_non_empty() {
    let mut cfg = DetailBarConfig::default();
    cfg.containers = vec![vec![], vec!["x".into()]];
    assert!(cfg.has_body());
}

#[test]
fn has_body_false_when_all_containers_empty() {
    let mut cfg = DetailBarConfig::default();
    cfg.containers = vec![vec![], vec![]];
    assert!(!cfg.has_body());
}

#[test]
fn preferred_height_clamps_to_min_on_short_terminal() {
    assert_eq!(DetailBarConfig::default().preferred_height(20), 8);
}

#[test]
fn preferred_height_returns_target_in_range() {
    assert_eq!(DetailBarConfig::default().preferred_height(50), 15);
}

#[test]
fn preferred_height_clamps_to_max_on_tall_terminal() {
    assert_eq!(DetailBarConfig::default().preferred_height(100), 18);
}

#[test]
fn preferred_height_returns_chrome_when_no_body() {
    let mut cfg = DetailBarConfig::default();
    cfg.containers = vec![vec![], vec![]];
    assert_eq!(cfg.preferred_height(20), DetailBarConfig::CHROME_ROWS);
    assert_eq!(cfg.preferred_height(100), DetailBarConfig::CHROME_ROWS);
}

#[test]
fn minimum_height_chrome_only_when_no_body() {
    let mut cfg = DetailBarConfig::default();
    cfg.containers = vec![vec![]];
    assert_eq!(cfg.minimum_height(), DetailBarConfig::CHROME_ROWS);
}

#[test]
fn minimum_height_uses_min_rows_when_body_present() {
    let cfg = DetailBarConfig::default();
    assert_eq!(cfg.minimum_height(), cfg.height.min_rows);
}

#[test]
fn sanitize_clamps_percent_low() {
    let mut cfg = DetailBarConfig::default();
    cfg.height.percent = 0;
    cfg.sanitize();
    assert_eq!(cfg.height.percent, 5);
}

#[test]
fn sanitize_clamps_percent_high() {
    let mut cfg = DetailBarConfig::default();
    cfg.height.percent = 200;
    cfg.sanitize();
    assert_eq!(cfg.height.percent, 80);
}

#[test]
fn sanitize_clamps_min_rows() {
    let mut cfg = DetailBarConfig::default();
    cfg.height.min_rows = 1;
    cfg.sanitize();
    assert_eq!(cfg.height.min_rows, 4);
}

#[test]
fn sanitize_clamps_max_rows() {
    let mut cfg = DetailBarConfig::default();
    cfg.height.max_rows = 200;
    cfg.sanitize();
    assert_eq!(cfg.height.max_rows, 60);
}

#[test]
fn sanitize_swaps_inverted_min_max() {
    let mut cfg = DetailBarConfig::default();
    cfg.height.min_rows = 20;
    cfg.height.max_rows = 10;
    cfg.sanitize();
    assert_eq!(cfg.height.min_rows, 10);
    assert_eq!(cfg.height.max_rows, 20);
}

#[test]
fn sanitize_resets_default_when_containers_empty() {
    let mut cfg = DetailBarConfig::default();
    cfg.containers = vec![];
    cfg.sanitize();
    assert_eq!(cfg.containers, DetailBarConfig::default().containers);
}

#[test]
fn sanitize_truncates_containers_to_four() {
    let mut cfg = DetailBarConfig::default();
    cfg.containers = vec![
        vec!["a".into()], vec!["b".into()], vec!["c".into()],
        vec!["d".into()], vec!["e".into()], vec!["f".into()],
    ];
    cfg.sanitize();
    assert_eq!(cfg.containers.len(), 4);
    assert_eq!(cfg.containers[0], vec!["a".to_string()]);
    assert_eq!(cfg.containers[3], vec!["d".to_string()]);
}

#[test]
fn sanitize_leaves_empty_inner_lists_alone() {
    let mut cfg = DetailBarConfig::default();
    cfg.containers = vec![vec!["a".into()], vec![], vec!["b".into()]];
    cfg.sanitize();
    assert_eq!(cfg.containers.len(), 3);
    assert!(cfg.containers[1].is_empty());
}

#[test]
fn with_override_none_returns_base() {
    let cfg = DetailBarConfig::default();
    let ovr = DetailBarOverride::default();
    assert_eq!(cfg.clone().with_override(&ovr), cfg);
}

#[test]
fn with_override_replaces_visible() {
    let cfg = DetailBarConfig::default();
    let ovr = DetailBarOverride { visible: Some(false), ..Default::default() };
    assert!(!cfg.with_override(&ovr).visible);
}

#[test]
fn with_override_replaces_height_per_field() {
    let cfg = DetailBarConfig::default();
    let ovr = DetailBarOverride {
        height: Some(HeightOverride { percent: Some(50), ..Default::default() }),
        ..Default::default()
    };
    let merged = cfg.with_override(&ovr);
    assert_eq!(merged.height.percent, 50);
    assert_eq!(merged.height.min_rows, 8);
    assert_eq!(merged.height.max_rows, 18);
}

#[test]
fn with_override_whole_replaces_containers_when_some() {
    let cfg = DetailBarConfig::default();
    let ovr = DetailBarOverride {
        containers: Some(vec![vec!["recent_chat".into()]]),
        ..Default::default()
    };
    let merged = cfg.with_override(&ovr);
    assert_eq!(merged.containers, vec![vec!["recent_chat".to_string()]]);
}

#[test]
fn with_override_leaves_containers_when_none() {
    let cfg = DetailBarConfig::default();
    let ovr = DetailBarOverride { containers: None, ..Default::default() };
    let merged = cfg.clone().with_override(&ovr);
    assert_eq!(merged.containers, cfg.containers);
}

#[test]
fn override_round_trips_through_json() {
    let ovr = DetailBarOverride {
        visible: Some(false),
        height: Some(HeightOverride { percent: Some(20), min_rows: None, max_rows: None }),
        containers: Some(vec![vec!["recent_chat".into()]]),
    };
    let json = serde_json::to_string(&ovr).unwrap();
    let parsed: DetailBarOverride = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.visible, Some(false));
    assert_eq!(parsed.height.unwrap().percent, Some(20));
    assert_eq!(parsed.containers.unwrap(), vec![vec!["recent_chat".to_string()]]);
}

#[test]
fn empty_override_object_parses() {
    let parsed: DetailBarOverride = serde_json::from_str("{}").unwrap();
    assert!(parsed.visible.is_none());
    assert!(parsed.height.is_none());
    assert!(parsed.containers.is_none());
}

#[test]
fn resolve_global_only_returns_default_when_unset() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(resolve_global_only(&store), DetailBarConfig::default());
}

#[test]
fn resolve_global_only_logs_and_defaults_on_malformed() {
    let store = Store::open_in_memory().unwrap();
    store.set_setting("detail_bar_config", "{not json").unwrap();
    assert_eq!(resolve_global_only(&store), DetailBarConfig::default());
}

#[test]
fn resolve_global_only_clamps_via_sanitize() {
    let store = Store::open_in_memory().unwrap();
    store
        .set_setting("detail_bar_config", r#"{"height": {"percent": 200}}"#)
        .unwrap();
    assert_eq!(resolve_global_only(&store).height.percent, 80);
}

#[test]
fn resolve_returns_default_when_nothing_set() {
    let store = Store::open_in_memory().unwrap();
    let repo = test_repo(None);
    assert_eq!(resolve(&repo, &store), DetailBarConfig::default());
}

#[test]
fn resolve_uses_global_when_only_global_set() {
    let store = Store::open_in_memory().unwrap();
    store.set_setting("detail_bar_config", r#"{"visible": false}"#).unwrap();
    let repo = test_repo(None);
    assert!(!resolve(&repo, &store).visible);
}

#[test]
fn resolve_applies_repo_override_on_top_of_global() {
    let store = Store::open_in_memory().unwrap();
    store.set_setting("detail_bar_config", r#"{"visible": false}"#).unwrap();
    let repo = test_repo(Some(r#"{"visible": true}"#));
    assert!(resolve(&repo, &store).visible);
}

#[test]
fn resolve_falls_back_when_global_json_malformed() {
    let store = Store::open_in_memory().unwrap();
    store.set_setting("detail_bar_config", "{not json").unwrap();
    let repo = test_repo(None);
    assert_eq!(resolve(&repo, &store), DetailBarConfig::default());
}

#[test]
fn resolve_ignores_repo_override_when_malformed() {
    let store = Store::open_in_memory().unwrap();
    store.set_setting("detail_bar_config", r#"{"visible": false}"#).unwrap();
    let repo = test_repo(Some("not json"));
    assert!(!resolve(&repo, &store).visible);
}

#[test]
fn resolve_clamps_out_of_range_percent() {
    let store = Store::open_in_memory().unwrap();
    store.set_setting("detail_bar_config", r#"{"height": {"percent": 200}}"#).unwrap();
    let repo = test_repo(None);
    assert_eq!(resolve(&repo, &store).height.percent, 80);
}

#[test]
fn resolve_repo_override_whole_replaces_containers() {
    let store = Store::open_in_memory().unwrap();
    let repo = test_repo(Some(r#"{"containers": [["recent_chat"]]}"#));
    assert_eq!(resolve(&repo, &store).containers, vec![vec!["recent_chat".to_string()]]);
}

#[test]
fn resolve_legacy_blob_silently_ignores_sections() {
    // Stored blobs from the previous schema have a top-level `sections`
    // key. Serde silently drops unknown fields (no `deny_unknown_fields`
    // on the new struct), so legacy blobs parse to: their preserved
    // scalars (visible/height) + default containers. Bar still renders
    // with the default 3-container layout. The spec's wording about
    // "parse fails" is overstated — the actual behavior is "unknown
    // field silently dropped, defaults fill in the gap."
    let store = Store::open_in_memory().unwrap();
    let legacy = r#"{
        "visible": false,
        "height": {"percent": 25, "min_rows": 8, "max_rows": 18},
        "sections": {"session_summary": true, "recent_chat": false, "procs_and_files": true}
    }"#;
    store.set_setting("detail_bar_config", legacy).unwrap();
    let repo = test_repo(None);
    let cfg = resolve(&repo, &store);
    assert!(!cfg.visible); // scalar preserved
    assert_eq!(cfg.height.percent, 25); // scalar preserved
    assert_eq!(cfg.containers, DetailBarConfig::default().containers); // dropped sections → default containers
}
```

- [ ] **Step 2: Run the tests and verify they fail to compile**

Run: `cargo test --lib detail_bar_config`
Expected: compile errors — `no field 'containers' on type 'DetailBarConfig'`, plus references to `Sections`/`SectionsOverride` that no longer match the new tests.

- [ ] **Step 3: Replace the config struct + impls (lines 1-239 of the file)**

Replace lines 1–239 of `src/detail_bar_config.rs` (everything above `#[cfg(test)] mod tests`) with the following. The test module (which you just rewrote) stays untouched.

```rust
//! Display config for the workspace detail bar. Resolved from a
//! global JSON blob in `settings` + a per-repo JSON override on
//! `repos.detail_bar_config`. Per-repo `containers` whole-replaces;
//! scalar fields merge per-field.
//!
//! See `docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`.

use crate::store::{Repo, Store};
use serde::{Deserialize, Serialize};

fn default_visible() -> bool {
    true
}
fn default_percent() -> u8 {
    30
}
fn default_min_rows() -> u16 {
    8
}
fn default_max_rows() -> u16 {
    18
}
fn default_containers() -> Vec<Vec<String>> {
    vec![
        vec!["session_summary".to_string()],
        vec!["recent_chat".to_string()],
        vec!["processes".to_string(), "recent_files".to_string()],
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailBarConfig {
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub height: Height,
    #[serde(default = "default_containers")]
    pub containers: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Height {
    #[serde(default = "default_percent")]
    pub percent: u8,
    #[serde(default = "default_min_rows")]
    pub min_rows: u16,
    #[serde(default = "default_max_rows")]
    pub max_rows: u16,
}

impl Default for DetailBarConfig {
    fn default() -> Self {
        Self {
            visible: default_visible(),
            height: Height::default(),
            containers: default_containers(),
        }
    }
}

impl Default for Height {
    fn default() -> Self {
        Self {
            percent: default_percent(),
            min_rows: default_min_rows(),
            max_rows: default_max_rows(),
        }
    }
}

impl DetailBarConfig {
    /// Number of always-on chrome rows (header + 2 rules + reply).
    pub const CHROME_ROWS: u16 = 4;

    /// True when at least one container is non-empty.
    pub fn has_body(&self) -> bool {
        self.containers.iter().any(|c| !c.is_empty())
    }

    /// Smallest terminal height at which the bar can render usefully.
    pub fn minimum_height(&self) -> u16 {
        if self.has_body() {
            self.height.min_rows
        } else {
            Self::CHROME_ROWS
        }
    }

    /// Compute the bar's preferred height for the current terminal.
    /// Returns `CHROME_ROWS` when no container has any modules.
    /// Defensive against inverted `min_rows`/`max_rows`.
    pub fn preferred_height(&self, total: u16) -> u16 {
        if !self.has_body() {
            return Self::CHROME_ROWS;
        }
        let target = (u32::from(total) * u32::from(self.height.percent) / 100) as u16;
        let lo = self.height.min_rows.min(self.height.max_rows);
        let hi = self.height.min_rows.max(self.height.max_rows);
        target.clamp(lo, hi)
    }

    /// Apply an override on top of self. Repo wins per-field for
    /// scalars; `containers` is whole-replace.
    pub fn with_override(mut self, ovr: &DetailBarOverride) -> Self {
        if let Some(v) = ovr.visible {
            self.visible = v;
        }
        if let Some(h) = &ovr.height {
            if let Some(p) = h.percent {
                self.height.percent = p;
            }
            if let Some(m) = h.min_rows {
                self.height.min_rows = m;
            }
            if let Some(m) = h.max_rows {
                self.height.max_rows = m;
            }
        }
        if let Some(c) = &ovr.containers {
            self.containers = c.clone();
        }
        self
    }

    /// Clamp into legal ranges, swap inverted min/max, truncate
    /// `containers` to 4, reset to default when empty. Idempotent.
    pub fn sanitize(&mut self) {
        self.height.percent = self.height.percent.clamp(5, 80);
        self.height.min_rows = self.height.min_rows.clamp(4, 40);
        self.height.max_rows = self.height.max_rows.clamp(4, 60);
        if self.height.min_rows > self.height.max_rows {
            std::mem::swap(&mut self.height.min_rows, &mut self.height.max_rows);
        }

        if self.containers.is_empty() {
            tracing::warn!("detail_bar_config.containers was empty; using default layout");
            self.containers = default_containers();
            return;
        }
        if self.containers.len() > 4 {
            tracing::warn!(
                len = self.containers.len(),
                "detail_bar_config.containers > 4; truncating to first 4"
            );
            self.containers.truncate(4);
        }
        // Empty inner lists (spacer containers) are kept — the user
        // opted in. Module-ID validity is checked at render time.
    }
}

/// Partial override of `DetailBarConfig`. `None` fields inherit from base.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailBarOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<HeightOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub containers: Option<Vec<Vec<String>>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeightOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_rows: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rows: Option<u16>,
}

/// Resolve the global-only `DetailBarConfig`. Reads the global blob
/// from `settings`. Malformed JSON logs a warning and falls back to
/// defaults. Always returns a sanitized config.
pub fn resolve_global_only(store: &Store) -> DetailBarConfig {
    let mut cfg = match store.get_setting("detail_bar_config") {
        Ok(Some(s)) => match serde_json::from_str::<DetailBarConfig>(&s) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "detail_bar_config: global parse failed; using defaults"
                );
                DetailBarConfig::default()
            }
        },
        _ => DetailBarConfig::default(),
    };
    cfg.sanitize();
    cfg
}

/// Resolve the effective `DetailBarConfig` for `repo`. Reads the
/// global blob and applies the per-repo override. Malformed JSON in
/// either location logs a warning and is treated as unset.
pub fn resolve(repo: &Repo, store: &Store) -> DetailBarConfig {
    let mut cfg = resolve_global_only(store);
    if let Some(raw) = repo.detail_bar_config.as_deref() {
        match serde_json::from_str::<DetailBarOverride>(raw) {
            Ok(ovr) => cfg = cfg.with_override(&ovr),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    repo = %repo.name,
                    "detail_bar_config: repo override parse failed; ignoring"
                );
            }
        }
    }
    cfg.sanitize();
    cfg
}
```

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test --lib detail_bar_config`
Expected: all tests pass. (The rest of the crate won't compile yet because `detail.rs` still references `sections`. That's resolved in Task 4. Tests for the `detail_bar_config` module specifically should compile and pass via `--lib detail_bar_config`.)

If `cargo test --lib detail_bar_config` itself won't compile due to cross-module references from other tests, run instead:
Run: `cargo check -p wsx --lib 2>&1 | grep -E "error\[E" | head -20`
Expected: errors are confined to `src/ui/dashboard/detail.rs` and `src/app.rs` referencing `sections.*` — confirming the schema swap is isolated to this file.

- [ ] **Step 5: Commit**

```bash
git add src/detail_bar_config.rs
git commit -m "feat(detail-bar-config): replace sections schema with containers (issue #98)"
```

(The crate as a whole won't build yet — `detail.rs` still references the old schema. Subsequent tasks fix that. This is acceptable for a feature branch; we won't push until the chain compiles.)

---

### Task 2: Create `src/detail_modules/mod.rs` skeleton (trait, context, registry)

**Files:**
- Create: `src/detail_modules/mod.rs`
- Modify: `src/lib.rs` (declare module)

The skeleton ships the trait, an empty registry, and a `register_builtins` stub that does nothing yet. Built-ins land in subsequent tasks. We verify the trait/registry surface compiles and the registry works with a mock module.

- [ ] **Step 1: Write the failing tests**

Create `src/detail_modules/mod.rs` with this content (tests + a minimal mock; trait/registry to follow in step 3):

```rust
//! Detail-bar modules. Pluggable units that render into a container
//! slot in the workspace detail bar. The host (chrome layer in
//! `src/ui/dashboard/detail.rs`) iterates over configured container
//! IDs, looks each up in the `Registry`, and dispatches `render`.
//!
//! See `docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`.

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Constraint;

    struct MockModule {
        id: &'static str,
        title: &'static str,
        hint: Constraint,
    }
    impl DetailModule for MockModule {
        fn id(&self) -> &'static str { self.id }
        fn title(&self) -> &'static str { self.title }
        fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint { self.hint }
        fn render(
            &self,
            _area: ratatui::layout::Rect,
            _ctx: &DetailContext<'_>,
            _frame: &mut ratatui::Frame<'_>,
        ) {}
    }

    #[test]
    fn empty_registry_returns_none() {
        let reg = Registry::new();
        assert!(reg.get("anything").is_none());
    }

    #[test]
    fn register_and_get_round_trip() {
        let mut reg = Registry::new();
        reg.register(Box::new(MockModule {
            id: "foo",
            title: "FOO",
            hint: Constraint::Length(3),
        }));
        let m = reg.get("foo").expect("module foo should be registered");
        assert_eq!(m.id(), "foo");
        assert_eq!(m.title(), "FOO");
    }

    #[test]
    fn get_unknown_returns_none() {
        let mut reg = Registry::new();
        reg.register(Box::new(MockModule {
            id: "foo", title: "FOO", hint: Constraint::Length(1),
        }));
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn ids_enumerates_all_registered() {
        let mut reg = Registry::new();
        reg.register(Box::new(MockModule {
            id: "a", title: "A", hint: Constraint::Length(1),
        }));
        reg.register(Box::new(MockModule {
            id: "b", title: "B", hint: Constraint::Length(1),
        }));
        let ids: std::collections::HashSet<_> = reg.ids().collect();
        assert!(ids.contains("a"));
        assert!(ids.contains("b"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn register_builtins_stub_does_nothing_initially() {
        // Built-ins are populated in later tasks. For now the stub
        // must compile and leave the registry empty.
        let mut reg = Registry::new();
        register_builtins(&mut reg);
        assert_eq!(reg.ids().count(), 0);
    }
}
```

- [ ] **Step 2: Run the tests and verify they fail to compile**

Run: `cargo test --lib detail_modules`
Expected: compile errors — `DetailModule`, `DetailContext`, `Registry`, `register_builtins` are all undefined.

- [ ] **Step 3: Add the trait, context, and registry above the test block**

Insert this block at the top of `src/detail_modules/mod.rs`, just below the file-level doc comment and above `#[cfg(test)] mod tests`:

```rust
use crate::events::WorkspaceEvents;
use crate::forge::BranchLifecycle;
use crate::git::DiffStats;
use crate::proc::ProcInfo;
use crate::store::{Repo, Workspace};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use std::collections::HashMap;

/// Borrowed snapshot of everything a module might need to render.
/// Built once per draw by the chrome layer in
/// `src/ui/dashboard/detail.rs` and passed by reference to each module.
/// Zero allocations per draw — all fields are borrowed or `Copy`.
pub struct DetailContext<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub diff_per_file: Option<&'a HashMap<String, DiffStats>>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<&'a str>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub events_scanned: bool,
    pub theme: &'a Theme,
}

pub trait DetailModule: Send + Sync {
    /// Stable identifier used in config JSON. Lowercase snake_case.
    fn id(&self) -> &'static str;

    /// Heading drawn above the module's body by the host. Modules do
    /// not render their own title.
    fn title(&self) -> &'static str;

    /// Vertical sizing hint when multiple modules stack in one
    /// container. Fed directly to `Layout::vertical(...)`. Receives
    /// the context so data-dependent modules (e.g. `Processes`) can
    /// size to their current contents.
    fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint;

    /// Render the module's body into `area`. The host has already
    /// drawn the title row above `area`.
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>);
}

pub struct Registry {
    modules: HashMap<&'static str, Box<dyn DetailModule>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { modules: HashMap::new() }
    }

    pub fn register(&mut self, m: Box<dyn DetailModule>) {
        let id = m.id();
        self.modules.insert(id, m);
    }

    pub fn get(&self, id: &str) -> Option<&dyn DetailModule> {
        self.modules.get(id).map(|b| b.as_ref())
    }

    pub fn ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.modules.keys().copied()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Populate `reg` with the built-in modules. Built-ins are added in
/// subsequent tasks; this is currently a no-op.
pub fn register_builtins(_reg: &mut Registry) {
    // Built-ins land in Tasks 4–7.
}
```

- [ ] **Step 4: Declare the module in `src/lib.rs`**

Find the existing `pub mod detail_bar_config;` line in `src/lib.rs` and add this immediately after it:

```rust
pub mod detail_modules;
```

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test --lib detail_modules`
Expected: all five tests pass. Other crate-wide compile errors from Task 1 are still present and expected; they get fixed in Task 8.

If the test command fails because the crate as a whole doesn't compile, run instead:
Run: `cargo check --lib 2>&1 | grep -E "src/detail_modules" | head`
Expected: no compile errors from `src/detail_modules/*`.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/detail_modules/mod.rs
git commit -m "feat(detail-modules): scaffold trait, context, and registry"
```

---

## Phase B — Built-in modules

### Task 3: Extract `SessionSummary` module

**Files:**
- Create: `src/detail_modules/session_summary.rs`
- Modify: `src/detail_modules/mod.rs` (declare submodule, register in `register_builtins`)

The body builder `build_session_summary` already exists in `src/ui/dashboard/detail.rs` (line ~350). We move its body into the module's `render` impl. The original function in `detail.rs` is left in place for now — Task 8 deletes it once the new dispatch path is live.

- [ ] **Step 1: Write the failing test**

Create `src/detail_modules/session_summary.rs` with this content (tests inline; impl in step 3):

```rust
//! Session summary module. Shows the agent's current status, last
//! activity, and tool-use trace for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct SessionSummary;

// impl DetailModule lands in step 3.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::DetailContext;

    #[test]
    fn id_is_session_summary() {
        assert_eq!(SessionSummary.id(), "session_summary");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(SessionSummary.title(), "SESSION SUMMARY");
    }

    #[test]
    fn height_hint_is_min_three() {
        // Build a minimal context; height_hint must not panic on empty data.
        let ctx = crate::detail_modules::tests_helpers::stub_context();
        assert_eq!(SessionSummary.height_hint(&ctx), Constraint::Min(3));
    }
}
```

The test references `crate::detail_modules::tests_helpers::stub_context()` — a shared fixture builder. Add it now to `src/detail_modules/mod.rs`. Insert this above `#[cfg(test)] mod tests`:

```rust
#[cfg(test)]
pub(crate) mod tests_helpers {
    use super::*;
    use crate::store::{NewWorkspace, Repo, RepoId, Store, WorkspaceState};
    use std::path::PathBuf;

    /// Build a minimal `DetailContext` backed by leaked allocations.
    /// Sufficient for unit-testing module methods that don't need
    /// realistic data. Test-only — leaks are fine.
    pub fn stub_context() -> DetailContext<'static> {
        let repo: &'static Repo = Box::leak(Box::new(Repo {
            id: RepoId(1),
            name: "demo".into(),
            path: PathBuf::from("/r"),
            branch_prefix: String::new(),
            custom_instructions: None,
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
        }));
        let store = Store::open_in_memory().expect("in-mem store");
        let ws_id = store
            .insert_workspace(NewWorkspace {
                repo_id: repo.id,
                name: "ws".into(),
                branch: "br".into(),
                worktree_path: PathBuf::from("/wt"),
                state: WorkspaceState::Active,
            })
            .expect("insert workspace");
        let workspace = store.get_workspace(ws_id).expect("get").expect("some");
        let workspace: &'static crate::store::Workspace = Box::leak(Box::new(workspace));
        let theme: &'static Theme = Box::leak(Box::new(Theme::default()));
        DetailContext {
            repo,
            workspace,
            events: None,
            procs: &[],
            diff: None,
            diff_per_file: None,
            lifecycle: None,
            pr_title: None,
            pr_number: None,
            status: Status::Idle,
            ago_secs: None,
            events_scanned: false,
            theme,
        }
    }
}
```

If the `Store`/`NewWorkspace`/`WorkspaceState`/`Workspace` API signatures differ in current code, adjust the helper to match — the goal is a working `DetailContext`, not a specific construction path. Use the `test_repo` helper pattern from `src/detail_bar_config.rs:558` as a reference if needed.

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test --lib detail_modules::session_summary`
Expected: compile error — `SessionSummary` doesn't implement `DetailModule` (no `id`, `title`, `height_hint` methods).

- [ ] **Step 3: Add the impl**

Replace the `// impl DetailModule lands in step 3.` line in `src/detail_modules/session_summary.rs` with:

```rust
impl DetailModule for SessionSummary {
    fn id(&self) -> &'static str { "session_summary" }
    fn title(&self) -> &'static str { "SESSION SUMMARY" }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Min(3)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let created_at_secs = (ctx.workspace.created_at.max(0) / 1000) as u64;
        let created_secs = now_secs.saturating_sub(created_at_secs);

        let lines = crate::ui::dashboard::detail::build_session_summary(
            if ctx.events_scanned { ctx.events } else { None },
            ctx.status,
            ctx.theme,
            area.width as usize,
            created_secs,
            ctx.ago_secs,
        );
        frame.render_widget(Paragraph::new(lines), area);
    }
}
```

(`build_session_summary` is currently `pub(super)` in `detail.rs`. Bump its visibility to `pub(crate)` in `detail.rs` so the module can call it. Also bump the visibility of `build_recent_chat`, `build_procs_and_files`, `build_header_strip`, and `build_reply_row` while you're there — Tasks 4–6 and Task 8 need them too. One-line change per `pub(super)` → `pub(crate)`.)

- [ ] **Step 4: Register the built-in**

In `src/detail_modules/mod.rs`, add `pub mod session_summary;` near the top (just under the trait/registry block, above the test helper module). Replace the body of `register_builtins` with:

```rust
pub fn register_builtins(reg: &mut Registry) {
    reg.register(Box::new(session_summary::SessionSummary));
}
```

Also update the test `register_builtins_stub_does_nothing_initially` in `src/detail_modules/mod.rs` to reflect the new state — replace it with:

```rust
#[test]
fn register_builtins_includes_session_summary() {
    let mut reg = Registry::new();
    register_builtins(&mut reg);
    assert!(reg.get("session_summary").is_some());
}
```

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test --lib detail_modules`
Expected: all `detail_modules` tests pass (session_summary's three tests + the registry tests + the updated `register_builtins_includes_session_summary`).

If the crate doesn't compile yet because `detail.rs` still depends on the old schema, run:
Run: `cargo check --lib --tests 2>&1 | grep "src/detail_modules\|src/lib.rs" | head`
Expected: no compile errors in `src/detail_modules/*`.

- [ ] **Step 6: Commit**

```bash
git add src/detail_modules/mod.rs src/detail_modules/session_summary.rs src/ui/dashboard/detail.rs
git commit -m "feat(detail-modules): extract SessionSummary module"
```

---

### Task 4: Extract `RecentChat` module

**Files:**
- Create: `src/detail_modules/recent_chat.rs`
- Modify: `src/detail_modules/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/detail_modules/recent_chat.rs`:

```rust
//! Recent chat module. Renders the agent's most recent user/assistant
//! turns for the selected workspace.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct RecentChat;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;

    #[test]
    fn id_is_recent_chat() {
        assert_eq!(RecentChat.id(), "recent_chat");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(RecentChat.title(), "RECENT CHAT");
    }

    #[test]
    fn height_hint_is_min_three() {
        let ctx = stub_context();
        assert_eq!(RecentChat.height_hint(&ctx), Constraint::Min(3));
    }
}
```

- [ ] **Step 2: Run the test and verify it fails**

Run: `cargo test --lib detail_modules::recent_chat`
Expected: compile error — `RecentChat` doesn't implement `DetailModule`.

- [ ] **Step 3: Add the impl**

Append to `src/detail_modules/recent_chat.rs`:

```rust
impl DetailModule for RecentChat {
    fn id(&self) -> &'static str { "recent_chat" }
    fn title(&self) -> &'static str { "RECENT CHAT" }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Min(3)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let lines = crate::ui::dashboard::detail::build_recent_chat(
            if ctx.events_scanned { ctx.events } else { None },
            ctx.theme,
            area.width as usize,
            (area.height as usize).saturating_sub(1).max(1),
        );
        frame.render_widget(Paragraph::new(lines), area);
    }
}
```

- [ ] **Step 4: Register the built-in**

In `src/detail_modules/mod.rs`:
- Add `pub mod recent_chat;` next to `pub mod session_summary;`.
- Extend `register_builtins`:

```rust
pub fn register_builtins(reg: &mut Registry) {
    reg.register(Box::new(session_summary::SessionSummary));
    reg.register(Box::new(recent_chat::RecentChat));
}
```

- Update the `register_builtins_includes_session_summary` test (or add a new one):

```rust
#[test]
fn register_builtins_includes_recent_chat() {
    let mut reg = Registry::new();
    register_builtins(&mut reg);
    assert!(reg.get("recent_chat").is_some());
}
```

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test --lib detail_modules`
Expected: all `detail_modules` tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/detail_modules/mod.rs src/detail_modules/recent_chat.rs
git commit -m "feat(detail-modules): extract RecentChat module"
```

---

### Task 5: Split `build_procs_and_files` into `build_processes` + `build_recent_files`

**Files:**
- Modify: `src/ui/dashboard/detail.rs` (around line 471)

This is a pure refactor of the existing combined builder. The new builders will be called by the new `Processes` and `RecentFiles` modules in Tasks 6–7. The old `build_procs_and_files` stays for now (still called by the old `render_column` arm); Task 8 deletes it.

- [ ] **Step 1: Read the existing implementation**

Open `src/ui/dashboard/detail.rs` and read lines 471–544. The function has two clearly delimited halves: PROCESSES (lines 482–503) and RECENT FILES (lines 505–542).

- [ ] **Step 2: Write the failing tests**

In `src/ui/dashboard/detail.rs`, find the `#[cfg(test)] mod tests` block. Add these tests inside it:

```rust
#[test]
fn build_processes_empty_emits_dash() {
    let theme = Theme::default();
    let lines = build_processes(&[], &theme, 40);
    // First line is the label, second is the "—" placeholder.
    assert_eq!(lines.len(), 2);
    let label = lines[0].spans.iter().map(|s| s.content.as_ref()).collect::<String>();
    assert_eq!(label, "PROCESSES");
    let placeholder = lines[1].spans.iter().map(|s| s.content.as_ref()).collect::<String>();
    assert_eq!(placeholder, "—");
}

#[test]
fn build_recent_files_empty_emits_dash() {
    let theme = Theme::default();
    let path = std::path::PathBuf::from("/wt");
    let lines = build_recent_files(None, None, &path, &theme, 40);
    assert_eq!(lines.len(), 2);
    let label = lines[0].spans.iter().map(|s| s.content.as_ref()).collect::<String>();
    assert_eq!(label, "RECENT FILES");
    let placeholder = lines[1].spans.iter().map(|s| s.content.as_ref()).collect::<String>();
    assert_eq!(placeholder, "—");
}

#[test]
fn build_procs_and_files_matches_split_builders_concatenated() {
    // Back-compat sanity: the combined builder should equal the two
    // split builders concatenated (when both are called with the same
    // inputs). Used to verify the extraction is mechanical.
    let theme = Theme::default();
    let path = std::path::PathBuf::from("/wt");
    let combined = build_procs_and_files(&[], None, None, &path, &theme, 40);
    let mut split = build_processes(&[], &theme, 40);
    split.extend(build_recent_files(None, None, &path, &theme, 40));
    // Compare rendered text only — Span style equality can be brittle.
    let combined_text: Vec<String> = combined.iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect();
    let split_text: Vec<String> = split.iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect();
    assert_eq!(combined_text, split_text);
}
```

- [ ] **Step 3: Run the tests and verify they fail**

Run: `cargo test --lib ui::dashboard::detail::tests::build_processes_empty_emits_dash`
Expected: compile error — `build_processes` and `build_recent_files` are undefined.

- [ ] **Step 4: Add the two new builders next to the existing one**

In `src/ui/dashboard/detail.rs`, just *above* the existing `pub(super) fn build_procs_and_files` definition (line ~471), add:

```rust
/// Render the PROCESSES module body. Returns the label row plus one
/// row per process (capped at 5, with a "+N more" suffix when over
/// the cap), or a single "—" placeholder when empty.
pub(crate) fn build_processes(
    procs: &[ProcInfo],
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);
    out.push(Line::from(Span::styled("PROCESSES".to_string(), label_style)));
    if procs.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        let visible = procs.iter().take(5);
        for p in visible {
            let cmd = truncate_to_chars(&p.command, column_width.saturating_sub(4));
            out.push(Line::from(vec![
                Span::styled("● ".to_string(), theme.status_style(Status::Thinking)),
                Span::styled(cmd, theme.dim_style()),
            ]));
        }
        if procs.len() > 5 {
            out.push(Line::from(Span::styled(
                format!("+{} more", procs.len() - 5),
                theme.dim_style(),
            )));
        }
    }
    out
}

/// Render the RECENT FILES module body. Returns the label row plus
/// one row per file (capped at 5), each annotated with per-file diff
/// stats when available. Single "—" placeholder when empty.
pub(crate) fn build_recent_files(
    events: Option<&WorkspaceEvents>,
    diff_per_file: Option<&std::collections::HashMap<String, DiffStats>>,
    worktree_path: &std::path::Path,
    theme: &Theme,
    column_width: usize,
) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::new();
    let label_style = Style::default().fg(theme.path).add_modifier(Modifier::BOLD);
    out.push(Line::from(Span::styled("RECENT FILES".to_string(), label_style)));
    let files: Vec<&String> = events
        .map(|e| e.recent_edited_files.iter().collect())
        .unwrap_or_default();
    if files.is_empty() {
        out.push(Line::from(Span::styled("—".to_string(), theme.dim_style())));
    } else {
        for f in files.iter().take(5) {
            let diff = lookup_file_diff(f, worktree_path, diff_per_file);
            let suffix_width = match diff {
                Some(d) if d.added > 0 || d.removed > 0 => {
                    4 + d.added.to_string().chars().count() + d.removed.to_string().chars().count()
                }
                _ => 0,
            };
            let display = display_relative_path(f, worktree_path);
            let path_width = column_width.saturating_sub(suffix_width);
            let truncated = truncate_to_chars_left(&display, path_width);
            let mut spans: Vec<Span<'static>> = vec![Span::styled(truncated, theme.dim_style())];
            if let Some(d) = diff
                && (d.added > 0 || d.removed > 0)
            {
                spans.push(Span::raw("  ".to_string()));
                spans.push(Span::styled(format!("+{}", d.added), theme.ok_style()));
                spans.push(Span::raw(" ".to_string()));
                spans.push(Span::styled(format!("−{}", d.removed), theme.err_style()));
            }
            out.push(Line::from(spans));
        }
    }
    out
}
```

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test --lib ui::dashboard::detail::tests::build_processes_empty_emits_dash ui::dashboard::detail::tests::build_recent_files_empty_emits_dash ui::dashboard::detail::tests::build_procs_and_files_matches_split_builders_concatenated`
Expected: all three pass.

If the crate-wide compile is still broken from Task 1, run only the test functions above by filename — `cargo test` filters by substring.

- [ ] **Step 6: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "refactor(detail-bar): split build_procs_and_files into two builders"
```

---

### Task 6: Extract `Processes` module

**Files:**
- Create: `src/detail_modules/processes.rs`
- Modify: `src/detail_modules/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/detail_modules/processes.rs`:

```rust
//! Processes module. Shows the running processes attached to the
//! selected workspace (capped at 6, scaled to procs count).

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct Processes;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;
    use crate::proc::ProcInfo;

    #[test]
    fn id_is_processes() {
        assert_eq!(Processes.id(), "processes");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(Processes.title(), "PROCESSES");
    }

    #[test]
    fn height_hint_zero_procs_returns_length_one() {
        let ctx = stub_context();
        assert_eq!(Processes.height_hint(&ctx), Constraint::Length(1));
    }

    #[test]
    fn height_hint_three_procs_returns_length_three() {
        let procs = vec![
            ProcInfo { pid: 1, command: "a".into() },
            ProcInfo { pid: 2, command: "b".into() },
            ProcInfo { pid: 3, command: "c".into() },
        ];
        let mut ctx = stub_context();
        ctx.procs = Box::leak(procs.into_boxed_slice());
        assert_eq!(Processes.height_hint(&ctx), Constraint::Length(3));
    }

    #[test]
    fn height_hint_ten_procs_capped_at_six() {
        let procs: Vec<ProcInfo> = (0..10)
            .map(|i| ProcInfo { pid: i, command: format!("c{i}") })
            .collect();
        let mut ctx = stub_context();
        ctx.procs = Box::leak(procs.into_boxed_slice());
        assert_eq!(Processes.height_hint(&ctx), Constraint::Length(6));
    }
}
```

If `ProcInfo` has additional fields beyond `pid` and `command`, adjust the literal in the test to match the current struct. Run `grep -n "pub struct ProcInfo" src/proc.rs` to confirm the shape before writing the test.

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test --lib detail_modules::processes`
Expected: compile error — `Processes` doesn't implement `DetailModule`.

- [ ] **Step 3: Add the impl**

Append to `src/detail_modules/processes.rs`:

```rust
impl DetailModule for Processes {
    fn id(&self) -> &'static str { "processes" }
    fn title(&self) -> &'static str { "PROCESSES" }
    fn height_hint(&self, ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Length(ctx.procs.len().clamp(1, 6) as u16)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let lines = crate::ui::dashboard::detail::build_processes(
            ctx.procs,
            ctx.theme,
            area.width as usize,
        );
        frame.render_widget(Paragraph::new(lines), area);
    }
}
```

- [ ] **Step 4: Register the built-in**

In `src/detail_modules/mod.rs`:
- Add `pub mod processes;` to the module-declaration block.
- Extend `register_builtins`:

```rust
pub fn register_builtins(reg: &mut Registry) {
    reg.register(Box::new(session_summary::SessionSummary));
    reg.register(Box::new(recent_chat::RecentChat));
    reg.register(Box::new(processes::Processes));
}
```

- Add a registry test:

```rust
#[test]
fn register_builtins_includes_processes() {
    let mut reg = Registry::new();
    register_builtins(&mut reg);
    assert!(reg.get("processes").is_some());
}
```

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test --lib detail_modules`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/detail_modules/mod.rs src/detail_modules/processes.rs
git commit -m "feat(detail-modules): extract Processes module"
```

---

### Task 7: Extract `RecentFiles` module

**Files:**
- Create: `src/detail_modules/recent_files.rs`
- Modify: `src/detail_modules/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `src/detail_modules/recent_files.rs`:

```rust
//! Recent files module. Shows files the agent has recently edited
//! within the workspace, with per-file diff stats.

use crate::detail_modules::{DetailContext, DetailModule};
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};

pub struct RecentFiles;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detail_modules::tests_helpers::stub_context;

    #[test]
    fn id_is_recent_files() {
        assert_eq!(RecentFiles.id(), "recent_files");
    }

    #[test]
    fn title_is_uppercase() {
        assert_eq!(RecentFiles.title(), "RECENT FILES");
    }

    #[test]
    fn height_hint_is_min_three() {
        let ctx = stub_context();
        assert_eq!(RecentFiles.height_hint(&ctx), Constraint::Min(3));
    }
}
```

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test --lib detail_modules::recent_files`
Expected: compile error.

- [ ] **Step 3: Add the impl**

Append:

```rust
impl DetailModule for RecentFiles {
    fn id(&self) -> &'static str { "recent_files" }
    fn title(&self) -> &'static str { "RECENT FILES" }
    fn height_hint(&self, _ctx: &DetailContext<'_>) -> Constraint {
        Constraint::Min(3)
    }
    fn render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>) {
        use ratatui::widgets::Paragraph;
        let lines = crate::ui::dashboard::detail::build_recent_files(
            ctx.events,
            ctx.diff_per_file,
            &ctx.workspace.worktree_path,
            ctx.theme,
            area.width as usize,
        );
        frame.render_widget(Paragraph::new(lines), area);
    }
}
```

- [ ] **Step 4: Register the built-in**

In `src/detail_modules/mod.rs`:
- Add `pub mod recent_files;`.
- Extend `register_builtins`:

```rust
pub fn register_builtins(reg: &mut Registry) {
    reg.register(Box::new(session_summary::SessionSummary));
    reg.register(Box::new(recent_chat::RecentChat));
    reg.register(Box::new(processes::Processes));
    reg.register(Box::new(recent_files::RecentFiles));
}
```

- Add:

```rust
#[test]
fn register_builtins_includes_recent_files() {
    let mut reg = Registry::new();
    register_builtins(&mut reg);
    assert!(reg.get("recent_files").is_some());
}
```

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test --lib detail_modules`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/detail_modules/mod.rs src/detail_modules/recent_files.rs
git commit -m "feat(detail-modules): extract RecentFiles module"
```

---

## Phase C — Integration: wire modules into detail.rs and app.rs

### Task 8: Replace `detail.rs` body rendering with the generic dispatcher

**Files:**
- Modify: `src/ui/dashboard/detail.rs` (entire `render` body + `render_column` + `Column` + `enabled_columns` + `column_widths`; delete `build_session_summary`, `build_recent_chat`, `build_procs_and_files`; update `DetailInputs`)

This is the largest task. The chrome stays; the body splitter becomes a generic container/module dispatcher; the old column code is deleted. The crate becomes compileable again at the end of this task — Task 1's changes have been waiting for this integration.

- [ ] **Step 1: Update `DetailInputs<'a>` to carry registry + borrowed pr_title**

In `src/ui/dashboard/detail.rs` find `pub struct DetailInputs<'a>` (around line 62) and replace it with:

```rust
pub struct DetailInputs<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub diff_per_file: Option<&'a std::collections::HashMap<String, DiffStats>>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<&'a str>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub reply_draft: &'a str,
    pub reply_focused: bool,
    pub events_scanned: bool,
    pub config: &'a DetailBarConfig,
    pub registry: &'a crate::detail_modules::Registry,
}
```

- [ ] **Step 2: Replace the body rendering in `render`**

In the same file, find `pub fn render(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme)` (around line 90). Replace the entire function body (lines 90–179) with:

```rust
pub fn render(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme) {
    if area.height == 0 || area.height < inputs.config.minimum_height() {
        return;
    }
    use ratatui::layout::{Constraint, Direction, Layout};
    use ratatui::widgets::Paragraph;

    let body_constraint = if inputs.config.has_body() {
        Constraint::Min(1)
    } else {
        Constraint::Length(0)
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header strip
            Constraint::Length(1), // rule
            body_constraint,       // body (N containers, or 0 when empty)
            Constraint::Length(1), // rule
            Constraint::Length(1), // reply row
        ])
        .split(area);

    let header = build_header_strip(
        &inputs.workspace.name,
        &inputs.workspace.branch,
        inputs.lifecycle,
        inputs.diff,
        inputs.procs.len() as u32,
        inputs.status,
        inputs.ago_secs,
        theme,
        chunks[0].width as usize,
    );
    f.render_widget(Paragraph::new(header), chunks[0]);
    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[1].width as usize),
            theme.dim_style(),
        ))),
        chunks[1],
    );

    render_body(f, chunks[2], inputs, theme);

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "─".repeat(chunks[3].width as usize),
            theme.dim_style(),
        ))),
        chunks[3],
    );

    let reply = build_reply_row(
        inputs.reply_draft,
        inputs.reply_focused,
        theme,
        chunks[4].width as usize,
    );
    f.render_widget(Paragraph::new(reply), chunks[4]);

    if inputs.reply_focused {
        let cx = reply_cursor_x(inputs.reply_draft, chunks[4].width as usize);
        f.set_cursor_position((chunks[4].x + cx, chunks[4].y));
    }
}

fn render_body(f: &mut Frame, area: Rect, inputs: &DetailInputs<'_>, theme: &Theme) {
    use ratatui::layout::{Constraint, Direction, Layout};

    let cfg = inputs.config;
    if !cfg.has_body() || area.height == 0 {
        return;
    }

    // Narrow-terminal collapse: < 80 cols → first non-empty container only.
    let containers: Vec<&Vec<String>> = if area.width < 80 {
        cfg.containers.iter().find(|c| !c.is_empty()).into_iter().collect()
    } else {
        cfg.containers.iter().collect()
    };

    let widths = equal_widths(containers.len());
    let column_areas = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(
            widths.iter().map(|w| Constraint::Percentage(*w)).collect::<Vec<_>>(),
        )
        .split(area);

    let ctx = crate::detail_modules::DetailContext {
        repo: inputs.repo,
        workspace: inputs.workspace,
        events: inputs.events,
        procs: inputs.procs,
        diff: inputs.diff,
        diff_per_file: inputs.diff_per_file,
        lifecycle: inputs.lifecycle,
        pr_title: inputs.pr_title,
        pr_number: inputs.pr_number,
        status: inputs.status,
        ago_secs: inputs.ago_secs,
        events_scanned: inputs.events_scanned,
        theme,
    };

    for (col_area, ids) in column_areas.iter().zip(containers.iter()) {
        render_container(f, *col_area, ids, &ctx, inputs.registry, theme);
    }
}

fn render_container(
    f: &mut Frame,
    area: Rect,
    module_ids: &[String],
    ctx: &crate::detail_modules::DetailContext<'_>,
    reg: &crate::detail_modules::Registry,
    theme: &Theme,
) {
    use ratatui::layout::{Constraint, Direction, Layout};

    if module_ids.is_empty() || area.height == 0 {
        return;
    }

    enum Slot<'a> {
        Found(&'a dyn crate::detail_modules::DetailModule),
        Unknown(&'a str),
    }
    let slots: Vec<Slot<'_>> = module_ids
        .iter()
        .map(|id| match reg.get(id) {
            Some(m) => Slot::Found(m),
            None => {
                tracing::warn!(id = %id, "detail_bar: unknown module id in container");
                Slot::Unknown(id.as_str())
            }
        })
        .collect();

    // Per slot: [title row, body, gap row]. Last slot's gap is Length(0).
    let constraints: Vec<Constraint> = slots
        .iter()
        .enumerate()
        .flat_map(|(i, slot)| {
            let last = i == slots.len() - 1;
            let body = match slot {
                Slot::Found(m) => m.height_hint(ctx),
                Slot::Unknown(_) => Constraint::Length(0),
            };
            let title = Constraint::Length(1);
            let gap = if last { Constraint::Length(0) } else { Constraint::Length(1) };
            [title, body, gap]
        })
        .collect();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let label_style = Style::default().fg(ctx.theme.path).add_modifier(Modifier::BOLD);

    for (i, slot) in slots.iter().enumerate() {
        let title_area = chunks[i * 3];
        let body_area = chunks[i * 3 + 1];
        match slot {
            Slot::Found(m) => {
                use ratatui::widgets::Paragraph;
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        m.title().to_string(),
                        label_style,
                    ))),
                    title_area,
                );
                m.render(body_area, ctx, f);
            }
            Slot::Unknown(id) => {
                use ratatui::widgets::Paragraph;
                f.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        format!("[unknown: {id}]"),
                        theme.dim_style(),
                    ))),
                    title_area,
                );
            }
        }
    }
}

fn equal_widths(n: usize) -> Vec<u16> {
    match n {
        0 => vec![],
        1 => vec![100],
        2 => vec![50, 50],
        3 => vec![33, 33, 34],
        4 => vec![25, 25, 25, 25],
        _ => unreachable!("sanitize() guarantees containers.len() in 1..=4"),
    }
}
```

- [ ] **Step 3: Delete dead code**

In the same file, delete the following items (they're now replaced by the generic dispatcher and the module files):

- `pub enum Column` (around line 19) and all its variants.
- `pub fn enabled_columns` (around line 25).
- `pub fn column_widths` (around line 42).
- `fn render_column` (around line 181).
- `pub(crate) fn build_session_summary` — its body now lives in `src/detail_modules/session_summary.rs`. Delete the function and any helpers it uses that aren't used by anything else.
- `pub(crate) fn build_recent_chat` — likewise.
- `pub(crate) fn build_procs_and_files` — likewise. The split builders `build_processes` and `build_recent_files` introduced in Task 5 stay (they're called by the new modules).

Also delete the existing tests in the file that reference deleted items: `enabled_columns_helper_returns_subset`, `enabled_columns_empty_when_all_disabled`, `column_widths_three_cols_match_legacy`, `column_widths_two_cols_summary_chat`, `column_widths_two_cols_summary_procs`, `column_widths_two_cols_chat_procs`, `column_widths_single_col_is_full`. Also delete any tests for `build_session_summary`, `build_recent_chat`, and `build_procs_and_files` whose body or signature no longer applies — leave the `build_processes_empty_emits_dash` and `build_recent_files_empty_emits_dash` tests added in Task 5, and delete `build_procs_and_files_matches_split_builders_concatenated` (the combined builder is gone).

- [ ] **Step 4: Add new tests for the generic dispatcher**

In the `#[cfg(test)] mod tests` block of `src/ui/dashboard/detail.rs`, add:

```rust
#[test]
fn equal_widths_one_through_four() {
    assert_eq!(equal_widths(1), vec![100]);
    assert_eq!(equal_widths(2), vec![50, 50]);
    assert_eq!(equal_widths(3), vec![33, 33, 34]);
    assert_eq!(equal_widths(4), vec![25, 25, 25, 25]);
}

#[test]
fn equal_widths_zero_is_empty() {
    assert_eq!(equal_widths(0), Vec::<u16>::new());
}
```

(Tests that exercise the full render path against a `Buffer` are valuable but verbose to set up; see Task 11 for end-to-end render tests. The dispatcher's purely-functional bits — `equal_widths` and `render_unknown_placeholder`-style behavior — are tested here. The visual snapshot tests for the module bodies live in the per-module files via the `build_*` builders, which are already tested in Task 5.)

- [ ] **Step 5: Update header strip's `pr_title` usage if needed**

Search the file for any use of `inputs.pr_title` — it's now `Option<&str>` instead of `Option<String>`. Replace any `.clone()` with `.as_deref()` is not needed (it's already a borrow). If `build_header_strip` takes `Option<&str>` already, no change; if it takes `Option<String>`, change the call site to pass `inputs.pr_title.map(|s| s.to_string())` *only* if the helper requires ownership, otherwise refactor the helper to take `Option<&str>` (preferred).

Run: `grep -n "pr_title" src/ui/dashboard/detail.rs`
Expected: no `pr_title.clone()` or `pr_title.as_ref().map(|s| s.clone())` patterns remain.

- [ ] **Step 6: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass. The crate compiles end-to-end for the first time since Task 1.

If `cargo test` fails to compile because `src/app.rs` still references removed items (`Column`, `enabled_columns`, etc.) or passes `pr_title: Option<String>` / lacks `registry`, those errors are expected — Task 9 fixes them. Run instead:

Run: `cargo check --lib 2>&1 | grep "src/app.rs" | head`
Expected: only errors in `src/app.rs` remain.

- [ ] **Step 7: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "feat(detail-bar): generic container/module body dispatcher"
```

---

### Task 9: Wire `Registry` into `App` and update the draw path

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Add the registry field to `App`**

Find the `pub struct App` definition in `src/app.rs`. Add to its field list:

```rust
pub registry: crate::detail_modules::Registry,
```

- [ ] **Step 2: Initialize the registry in the App constructor**

Find `App::new` (or the equivalent construction function). Just before the `App { ... }` struct-literal, add:

```rust
let mut registry = crate::detail_modules::Registry::new();
crate::detail_modules::register_builtins(&mut registry);
```

Inside the struct literal, add the field assignment:

```rust
registry,
```

- [ ] **Step 3: Update `DetailInputs` construction in the draw path**

Find where `DetailInputs { ... }` is constructed for the detail bar render (search for `DetailInputs {` in `src/app.rs`). Update the literal to:

- Replace `pr_title: <existing>` with `pr_title: <existing>.as_deref()` if `<existing>` is currently `&Option<String>` (most likely) or `Some(s) if s is String` → `pr_title: <field>.as_deref()`. Inspect the surrounding code and use the simplest form that yields `Option<&str>`.
- Add `registry: &self.registry,` as a field.

- [ ] **Step 4: Remove any references to deleted symbols**

Search the file for references to items deleted in Task 8: `Column`, `enabled_columns`, `column_widths`. If any remain (e.g. in focus-cycle helpers or layout pre-checks), replace them with equivalent uses of `inputs.config.has_body()` / `inputs.config.containers.len()` / the new `equal_widths` helper. If none remain, this step is a no-op.

Run: `grep -n "enabled_columns\|column_widths\|Column::" src/app.rs`
Expected: no matches.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --lib`
Expected: all tests pass. The crate now compiles and tests pass end-to-end.

- [ ] **Step 6: Run clippy + fmt to catch follow-on issues**

Run: `cargo clippy --all-targets --no-deps -- -D warnings`
Expected: no warnings.

Run: `cargo fmt`

- [ ] **Step 7: Commit**

```bash
git add src/app.rs
git commit -m "feat(app): wire detail-modules Registry into App and draw path"
```

---

### Task 10: Update the CLI editor seed for `wsx config edit detail_bar_config`

**Files:**
- Modify: `src/cli.rs`

The previous spec's CLI flow ([`src/cli.rs:734-744` per spec](../specs/2026-05-25-detail-bar-config-design.md)) seeds the editor buffer with the pretty-printed default config and validates JSON on save. The serialization shape changed; the helper code still does the right thing but the seed value must use the new defaults. If the seed is computed dynamically via `serde_json::to_string_pretty(&DetailBarConfig::default())`, no change is needed beyond verification.

- [ ] **Step 1: Verify the current behavior**

Run: `grep -n "detail_bar_config" src/cli.rs`
Expected output: locations that read `detail_bar_config` for `config edit`, `config get`, `config set`.

If the seed is computed via `serde_json::to_string_pretty(&DetailBarConfig::default())`, the new default automatically becomes the seed — proceed to step 3 (validation test).

If the seed is a hardcoded string with `sections.*`, replace it with the dynamic form. Show the existing literal and the replacement.

- [ ] **Step 2: Update the seed if needed**

If a hardcoded seed exists, replace it. Otherwise, add (if missing) the validation arm that parses on save with `serde_json::from_str::<DetailBarConfig>(&edited)` and calls `cfg.sanitize()` before persisting.

- [ ] **Step 3: Add a test that verifies the seeded default parses with the new schema**

In the `#[cfg(test)] mod tests` block of `src/cli.rs`, add:

```rust
#[test]
fn detail_bar_config_default_seed_round_trips() {
    use crate::detail_bar_config::DetailBarConfig;
    let seed = serde_json::to_string_pretty(&DetailBarConfig::default())
        .expect("serialize default");
    let parsed: DetailBarConfig = serde_json::from_str(&seed)
        .expect("seed must parse with new schema");
    assert_eq!(parsed, DetailBarConfig::default());
    // Spot-check: the new shape uses `containers`, not `sections`.
    assert!(seed.contains("\"containers\""));
    assert!(!seed.contains("\"sections\""));
}

#[test]
fn detail_bar_config_set_with_too_many_containers_is_truncated() {
    use crate::detail_bar_config::DetailBarConfig;
    let raw = serde_json::json!({
        "containers": [
            ["a"], ["b"], ["c"], ["d"], ["e"], ["f"]
        ]
    }).to_string();
    let mut parsed: DetailBarConfig = serde_json::from_str(&raw).expect("parse");
    parsed.sanitize();
    assert_eq!(parsed.containers.len(), 4);
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib cli::tests::detail_bar_config_default_seed_round_trips cli::tests::detail_bar_config_set_with_too_many_containers_is_truncated`
Expected: both pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): seed detail_bar_config editor with new containers default"
```

---

## Phase D — End-to-end tests, docs, and verification

### Task 11: End-to-end render tests for the dispatcher

**Files:**
- Modify: `src/ui/dashboard/detail.rs` (extend tests)

Verifies the full render path against a `Buffer`-backed `Frame`. Uses the existing test scaffolding pattern (look for `Frame`/`Terminal::new` usage already in the test module).

- [ ] **Step 1: Add helper to build a `DetailInputs` for tests**

In the test module of `src/ui/dashboard/detail.rs`, add (if not already present from prior tests) a helper that constructs a `DetailInputs<'_>` with a default config and a populated registry. Inspect the existing test fixtures in the file — there's likely already a `make_inputs(...)` or similar; if so, add a `registry` field. If not, add:

```rust
fn make_test_registry() -> crate::detail_modules::Registry {
    let mut reg = crate::detail_modules::Registry::new();
    crate::detail_modules::register_builtins(&mut reg);
    reg
}
```

- [ ] **Step 2: Write the rendering tests**

Add these to the `#[cfg(test)] mod tests` block. They render the bar into a `Buffer` and assert on placeholders / structural properties — not on exact byte output, which is fragile to theme/styling tweaks.

```rust
#[test]
fn render_with_unknown_module_shows_placeholder() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let reg = make_test_registry();
    let cfg = DetailBarConfig {
        containers: vec![vec!["seshun_summary".into()]],
        ..DetailBarConfig::default()
    };
    // ... construct DetailInputs with cfg and reg ...
    // (Use whatever construction pattern existing tests use. The key
    // assertion is that the rendered buffer contains "[unknown:".)

    let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
    term.draw(|f| {
        let area = f.area();
        // Build inputs here referencing the helpers above.
        let inputs: DetailInputs<'_> = build_inputs_for_test(/* ... */, &cfg, &reg);
        let theme = Theme::default();
        render(f, area, &inputs, &theme);
    }).unwrap();

    let buf = term.backend().buffer();
    let text: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| buf[(x, y)].symbol().to_string()))
        .collect();
    assert!(text.contains("[unknown: seshun_summary]"));
}

#[test]
fn render_one_container_full_width() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let reg = make_test_registry();
    let cfg = DetailBarConfig {
        containers: vec![vec!["recent_chat".into()]],
        ..DetailBarConfig::default()
    };
    let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
    term.draw(|f| {
        let area = f.area();
        let inputs = build_inputs_for_test(/* ... */, &cfg, &reg);
        let theme = Theme::default();
        render(f, area, &inputs, &theme);
    }).unwrap();
    // Assert the buffer contains the RECENT CHAT title row.
    let buf = term.backend().buffer();
    let text: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| buf[(x, y)].symbol().to_string()))
        .collect();
    assert!(text.contains("RECENT CHAT"));
}

#[test]
fn render_narrow_terminal_shows_only_first_non_empty_container() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let reg = make_test_registry();
    let cfg = DetailBarConfig {
        containers: vec![vec![], vec!["recent_chat".into()], vec!["processes".into()]],
        ..DetailBarConfig::default()
    };
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap(); // < 80 cols
    term.draw(|f| {
        let area = f.area();
        let inputs = build_inputs_for_test(/* ... */, &cfg, &reg);
        let theme = Theme::default();
        render(f, area, &inputs, &theme);
    }).unwrap();
    let buf = term.backend().buffer();
    let text: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| buf[(x, y)].symbol().to_string()))
        .collect();
    assert!(text.contains("RECENT CHAT"));
    assert!(!text.contains("PROCESSES")); // collapsed away
}

#[test]
fn render_all_empty_containers_shows_chrome_only() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    let reg = make_test_registry();
    let cfg = DetailBarConfig {
        containers: vec![vec![], vec![]],
        ..DetailBarConfig::default()
    };
    let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
    term.draw(|f| {
        let area = f.area();
        let inputs = build_inputs_for_test(/* ... */, &cfg, &reg);
        let theme = Theme::default();
        render(f, area, &inputs, &theme);
    }).unwrap();
    let buf = term.backend().buffer();
    let text: String = (0..buf.area.height)
        .flat_map(|y| (0..buf.area.width).map(move |x| buf[(x, y)].symbol().to_string()))
        .collect();
    // No module body titles should appear.
    assert!(!text.contains("SESSION SUMMARY"));
    assert!(!text.contains("RECENT CHAT"));
    assert!(!text.contains("PROCESSES"));
    assert!(!text.contains("RECENT FILES"));
}
```

Replace `build_inputs_for_test(/* ... */, &cfg, &reg)` with whatever the existing test helper signature is. If no such helper exists, write one that mirrors the construction pattern in the deleted `enabled_columns_*` tests — borrowing a `Repo`, `Workspace`, etc. from leaked `Box::leak` allocations is acceptable in tests.

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib ui::dashboard::detail::tests::render_with_unknown_module_shows_placeholder ui::dashboard::detail::tests::render_one_container_full_width ui::dashboard::detail::tests::render_narrow_terminal_shows_only_first_non_empty_container ui::dashboard::detail::tests::render_all_empty_containers_shows_chrome_only`
Expected: all four pass.

- [ ] **Step 4: Commit**

```bash
git add src/ui/dashboard/detail.rs
git commit -m "test(detail-bar): end-to-end render tests for container dispatcher"
```

---

### Task 12: Manual-test walkthrough doc

**Files:**
- Create: `docs/manual-tests/detail-bar-modules.md`

- [ ] **Step 1: Write the walkthrough**

Create `docs/manual-tests/detail-bar-modules.md`:

```markdown
# Detail bar modules — manual walkthrough

Verifies the container/module system end-to-end per
[`docs/superpowers/specs/2026-05-25-detail-bar-modules-design.md`](../superpowers/specs/2026-05-25-detail-bar-modules-design.md).

## Setup

Launch wsx with a workspace that has at least one running process and
recent agent activity:

```bash
cargo run --release
```

Select a workspace on the dashboard so the detail bar appears.

## Steps

1. **Default layout** — observe three equal-width columns: SESSION SUMMARY
   (left), RECENT CHAT (middle), PROCESSES stacked above RECENT FILES (right).
   Widths are 33/33/34.

2. **Edit the global config:**

   ```bash
   wsx config edit detail_bar_config
   ```

   The editor opens with the pretty-printed default config. Confirm it
   contains a `containers` array of length 3.

3. **Single-container layout** — change `containers` to:

   ```json
   "containers": [["recent_chat"]]
   ```

   Save and exit. The detail bar collapses to a single full-width chat
   column.

4. **Four-container layout** — `wsx config edit detail_bar_config` again,
   change `containers` to:

   ```json
   "containers": [
     ["session_summary"],
     ["recent_chat"],
     ["processes"],
     ["recent_files"]
   ]
   ```

   Save. Observe four equal-width columns (25% each), procs and files
   separated.

5. **Stacked modules** — change one container to stack two modules:

   ```json
   "containers": [
     ["session_summary"],
     ["recent_chat", "recent_files"],
     ["processes"]
   ]
   ```

   Save. Observe the middle column has RECENT CHAT on top and RECENT
   FILES below, sized by their height hints (chat grows; files takes
   its minimum).

6. **Unknown module ID** — change one entry to a typo:

   ```json
   "containers": [["seshun_summary"], ["recent_chat"], ["processes"]]
   ```

   Save. Observe `[unknown: seshun_summary]` placeholder in the left
   column; other columns render normally.

7. **Per-repo override** — open the repo settings modal (`s`), navigate
   to `detail_bar_config`, press Enter. Set:

   ```json
   {"containers": [["recent_chat"]]}
   ```

   Save. Workspaces in this repo show only the single chat column;
   workspaces in other repos still show the global layout.

8. **Clear override** — press `[d]` on the `detail_bar_config` row. The
   bar reverts to the global layout for this repo.

9. **Hide the bar entirely** — `wsx config edit detail_bar_config`,
   set `"visible": false`. Save. The detail bar disappears from the
   dashboard.

10. **Narrow terminal** — resize the terminal below 80 columns. Observe
    only the first non-empty container renders, at 100% width.

## Restore

```bash
wsx config edit detail_bar_config
```

Replace contents with `{}` and save — restores defaults.
```

- [ ] **Step 2: Commit**

```bash
git add docs/manual-tests/detail-bar-modules.md
git commit -m "docs(manual-tests): walkthrough for detail-bar modules"
```

---

### Task 13: Update README documentation block

**Files:**
- Modify: `README.md`

The README has a block describing `detail_bar_config` with the old `sections.*` keys (see commit `9112078` from earlier work). Replace it with the new schema.

- [ ] **Step 1: Locate the existing block**

Run: `grep -n "detail_bar_config\|sections.session_summary" README.md`
Expected: a block (likely under a "Configuration" heading) documenting the per-section toggles.

- [ ] **Step 2: Replace the block**

Replace the existing `detail_bar_config` documentation block with content of this form (adjust wording and headings to match the surrounding README style):

```markdown
### `detail_bar_config`

The workspace detail bar's layout is configurable via a global setting
and an optional per-repo override.

**Global** — `wsx config edit detail_bar_config`:

```json
{
  "visible": true,
  "height": { "percent": 30, "min_rows": 8, "max_rows": 18 },
  "containers": [
    ["session_summary"],
    ["recent_chat"],
    ["processes", "recent_files"]
  ]
}
```

- `visible` — toggle the bar on or off.
- `height.percent` — fraction of dashboard height (clamped 5–80).
- `height.min_rows`, `height.max_rows` — row clamps (4–60).
- `containers` — outer list of 1–4 containers (equal-width columns).
  Each container is an inner list of module IDs, stacked vertically
  in the order given.

**Built-in modules:**
- `session_summary` — agent status and activity.
- `recent_chat` — most recent agent turns.
- `processes` — running processes attached to the workspace.
- `recent_files` — recently edited files with per-file diff stats.

**Per-repo override** — in the repo settings modal (`s`), the
`detail_bar_config` row accepts a `DetailBarOverride`. Scalars
(`visible`, `height.*`) merge per-field; `containers` whole-replaces.

```json
{ "containers": [["recent_chat"]] }
```
```

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs(readme): document detail_bar_config containers schema"
```

---

### Task 14: Full verification (build, tests, clippy, fmt, manual smoke)

**Files:** (none modified)

- [ ] **Step 1: Build the binary**

Run: `cargo build --release`
Expected: clean build, no warnings.

- [ ] **Step 2: Run the full test suite**

Run: `cargo test --all-targets`
Expected: all tests pass.

- [ ] **Step 3: Clippy at the strict bar**

Run: `cargo clippy --all-targets --no-deps -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Format check**

Run: `cargo fmt --check`
Expected: no changes needed.

- [ ] **Step 5: Manual smoke test**

Walk through `docs/manual-tests/detail-bar-modules.md` end-to-end on a
real workspace. If any step fails, fix and re-run from that step.

- [ ] **Step 6: Commit any final cleanup**

If steps 3 or 4 surface issues that require fixes:

```bash
cargo fmt
git add -u
git commit -m "style: cargo fmt"
```

---

## Self-Review Notes

- **Spec coverage check.** Every section of the spec maps to tasks:
  - Architecture overview / file structure → Tasks 1, 2, 8.
  - Module trait + built-ins → Tasks 2, 3, 4, 6, 7 (with Task 5 doing the procs-and-files split that enables Tasks 6 and 7).
  - Config schema + resolution → Task 1.
  - Layout/rendering integration → Task 8 (detail.rs) + Task 9 (app.rs).
  - Edit surfaces → Task 10 (CLI). Repo-settings modal already exists in `src/app.rs` and `src/ui/modal.rs`; its save handler routes through the same `serde_json::from_str::<DetailBarOverride>` it always did, so swapping the override struct's shape in Task 1 makes the modal path work without further code change. Task 14's manual smoke step covers it.
  - Edge cases → Task 1 (sanitize / parse failures), Task 8 (unknown module placeholder / narrow collapse / all-empty), Task 11 (E2E tests assert these behaviors).
  - Testing → Tasks 1, 2, 3, 4, 5, 6, 7, 10, 11 (unit + dispatcher + E2E).
  - Manual verification → Task 12.
  - Docs → Task 13.
- **Type consistency.** `DetailContext` and `DetailModule` signatures used in Tasks 2, 3, 4, 6, 7, 8 match: `height_hint(&self, ctx: &DetailContext<'_>) -> Constraint`; `render(&self, area: Rect, ctx: &DetailContext<'_>, frame: &mut Frame<'_>)`. `DetailInputs::pr_title` is `Option<&'a str>` from Task 8 onward; the `Option<String>` → `Option<&str>` migration happens at the app.rs call site in Task 9.
- **Placeholder scan.** Task 11 leaves test-helper construction (`build_inputs_for_test(/* ... */)`) intentionally generic because the existing test scaffold in `detail.rs` isn't quoted here — the engineer is expected to follow the existing pattern. This is a controlled placeholder, not a plan failure: the alternative is reproducing several hundred lines of test setup code that may diverge from reality. If the engineer can't find a matching pattern, that's a signal to ask, not to guess.
