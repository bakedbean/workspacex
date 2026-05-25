# Configurable workspace detail bar — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the workspace detail bar's display configurable via a global JSON blob and per-repo override — toggling visibility, tuning height, and independently enabling each body column.

**Architecture:** A new pure-data module `src/detail_bar_config.rs` defines the config types, defaults, serde, validation, and a `resolve(repo, store)` merge function. `Repo` gains a `detail_bar_config: Option<String>` column. The detail bar renderer (`src/ui/dashboard/detail.rs`) reads a `&DetailBarConfig` from `DetailInputs` and skips disabled columns. `app.rs` resolves the config once per draw, threads it through `dashboard_regions`, and the existing CLI / repo-settings modal surfaces edit it as a new field.

**Tech Stack:** Rust, `serde` + `serde_json` (already deps), `rusqlite` (already a dep), `ratatui` (already a dep), `tracing` (already a dep).

**Spec reference:** `docs/superpowers/specs/2026-05-25-detail-bar-config-design.md`.

---

## File Structure

**New files:**
- `src/detail_bar_config.rs` — `DetailBarConfig`, `Sections`, `Height`, `DetailBarOverride`, `HeightOverride`, `SectionsOverride`, `resolve(repo, store)`. Pure data + serde + clamping + merging.
- `docs/manual-tests/detail-bar-config.md` — manual walkthrough.

**Modified files:**
- `src/lib.rs` — register the new module.
- `src/store.rs` — schema v11 migration adds `detail_bar_config TEXT` column to `repos`; `Repo` struct gains the field; `repos()` / `add_repo` queries include it; new `set_repo_detail_bar_config` helper.
- `src/ui/dashboard/detail.rs` — remove free `preferred_height` / `MIN_HEIGHT`; `DetailInputs` gains `config: &DetailBarConfig`; body builder skips disabled columns; narrow-terminal collapse picks first enabled column.
- `src/app.rs` — resolve `DetailBarConfig` per draw; pass into `dashboard_regions` and `DetailInputs`; `detail_visible` consults `cfg.visible`; Tab cycle skips `DetailBarReply` when bar hidden; `RepoSettingField` gains `DetailBarConfig` variant; `apply_repo_setting` handles it.
- `src/ui/modal.rs` — `rows` array in `render_repo_settings` extended with the new field row.
- `src/cli.rs` — `ConfigEdit` arm seeds default JSON for `detail_bar_config` when value is empty, and parses + clamps on save.

---

## Task 1: Build `DetailBarConfig` core types + defaults

**Files:**
- Create: `src/detail_bar_config.rs`
- Modify: `src/lib.rs`

This task creates the module file with the primary `DetailBarConfig` struct, nested `Sections` and `Height` structs, baked defaults, `has_body()`, `CHROME_ROWS`, and `preferred_height()`. Tests come first.

- [ ] **Step 1: Register the module in `src/lib.rs`**

Edit `src/lib.rs` — add `pub mod detail_bar_config;` in alphabetical position between `config` and `error`:

```rust
pub mod app;
pub mod cli;
pub mod config;
pub mod detail_bar_config;
pub mod error;
pub mod events;
// ...rest unchanged
```

- [ ] **Step 2: Create `src/detail_bar_config.rs` with failing tests for defaults + round-trip**

Create the file with this content:

```rust
//! Display config for the workspace detail bar. Resolved from a
//! global JSON blob in `settings` + a per-repo JSON override on
//! `repos.detail_bar_config`. Per-repo wins per-field.
//!
//! See `docs/superpowers/specs/2026-05-25-detail-bar-config-design.md`.

use serde::{Deserialize, Serialize};

fn default_visible() -> bool {
    true
}
fn default_true() -> bool {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailBarConfig {
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub height: Height,
    #[serde(default)]
    pub sections: Sections,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sections {
    #[serde(default = "default_true")]
    pub session_summary: bool,
    #[serde(default = "default_true")]
    pub recent_chat: bool,
    #[serde(default = "default_true")]
    pub procs_and_files: bool,
}

impl Default for DetailBarConfig {
    fn default() -> Self {
        Self {
            visible: default_visible(),
            height: Height::default(),
            sections: Sections::default(),
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

impl Default for Sections {
    fn default() -> Self {
        Self {
            session_summary: true,
            recent_chat: true,
            procs_and_files: true,
        }
    }
}

impl DetailBarConfig {
    /// Number of always-on chrome rows (header + 2 rules + reply).
    pub const CHROME_ROWS: u16 = 4;

    /// True when at least one body column is enabled.
    pub fn has_body(&self) -> bool {
        self.sections.session_summary
            || self.sections.recent_chat
            || self.sections.procs_and_files
    }

    /// Compute the bar's preferred height for the current terminal.
    /// When no sections are enabled, the bar shrinks to its chrome
    /// height (`CHROME_ROWS`) regardless of the configured percent.
    pub fn preferred_height(&self, total: u16) -> u16 {
        if !self.has_body() {
            return Self::CHROME_ROWS;
        }
        let target = (u32::from(total) * u32::from(self.height.percent) / 100) as u16;
        target.clamp(self.height.min_rows, self.height.max_rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_matches_documented_baseline() {
        let cfg = DetailBarConfig::default();
        assert!(cfg.visible);
        assert_eq!(cfg.height.percent, 30);
        assert_eq!(cfg.height.min_rows, 8);
        assert_eq!(cfg.height.max_rows, 18);
        assert!(cfg.sections.session_summary);
        assert!(cfg.sections.recent_chat);
        assert!(cfg.sections.procs_and_files);
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
        assert_eq!(parsed.sections, Sections::default());
    }

    #[test]
    fn parsing_unknown_fields_succeeds() {
        let json = r#"{"unknown_future_field": 123, "visible": true}"#;
        let parsed: DetailBarConfig = serde_json::from_str(json).unwrap();
        assert!(parsed.visible);
    }

    #[test]
    fn has_body_true_when_any_section_enabled() {
        let mut cfg = DetailBarConfig::default();
        cfg.sections = Sections {
            session_summary: false,
            recent_chat: false,
            procs_and_files: true,
        };
        assert!(cfg.has_body());
    }

    #[test]
    fn has_body_false_when_all_sections_disabled() {
        let mut cfg = DetailBarConfig::default();
        cfg.sections = Sections {
            session_summary: false,
            recent_chat: false,
            procs_and_files: false,
        };
        assert!(!cfg.has_body());
    }

    #[test]
    fn preferred_height_clamps_to_min_on_short_terminal() {
        // default: 30% of 20 = 6 → clamps up to 8 (min_rows).
        assert_eq!(DetailBarConfig::default().preferred_height(20), 8);
    }

    #[test]
    fn preferred_height_returns_target_in_range() {
        // default: 30% of 50 = 15, within [8, 18].
        assert_eq!(DetailBarConfig::default().preferred_height(50), 15);
    }

    #[test]
    fn preferred_height_clamps_to_max_on_tall_terminal() {
        // default: 30% of 100 = 30 → clamps down to 18 (max_rows).
        assert_eq!(DetailBarConfig::default().preferred_height(100), 18);
    }

    #[test]
    fn preferred_height_returns_chrome_when_no_sections_enabled() {
        let mut cfg = DetailBarConfig::default();
        cfg.sections = Sections {
            session_summary: false,
            recent_chat: false,
            procs_and_files: false,
        };
        // Independent of total or configured percent.
        assert_eq!(cfg.preferred_height(20), DetailBarConfig::CHROME_ROWS);
        assert_eq!(cfg.preferred_height(100), DetailBarConfig::CHROME_ROWS);
    }

    #[test]
    fn preferred_height_respects_custom_percent_and_clamps() {
        let cfg = DetailBarConfig {
            height: Height {
                percent: 50,
                min_rows: 8,
                max_rows: 18,
            },
            ..DetailBarConfig::default()
        };
        // 50% of 50 = 25, clamped down to max_rows 18.
        assert_eq!(cfg.preferred_height(50), 18);
        // 50% of 20 = 10, within [8, 18].
        assert_eq!(cfg.preferred_height(20), 10);
    }
}
```

- [ ] **Step 3: Run the tests; they should all pass**

Run: `cargo test --lib detail_bar_config::tests`
Expected: 12 tests, all pass.

- [ ] **Step 4: Verify the rest of the crate still builds**

Run: `cargo build --lib`
Expected: success, no warnings about unused imports in `detail_bar_config.rs`.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/detail_bar_config.rs
git commit -m "feat(detail-bar): add DetailBarConfig core types and defaults"
```

---

## Task 2: Add override types + `with_override` merge

**Files:**
- Modify: `src/detail_bar_config.rs`

This task adds the partial-override structs and the per-field merge.

- [ ] **Step 1: Add failing tests for `with_override`**

Append to the `tests` module in `src/detail_bar_config.rs` (before the closing `}` of the module):

```rust
    #[test]
    fn with_override_none_returns_base() {
        let cfg = DetailBarConfig::default();
        let ovr = DetailBarOverride::default();
        assert_eq!(cfg.clone().with_override(&ovr), cfg);
    }

    #[test]
    fn with_override_replaces_visible() {
        let cfg = DetailBarConfig::default();
        let ovr = DetailBarOverride {
            visible: Some(false),
            ..Default::default()
        };
        assert!(!cfg.with_override(&ovr).visible);
    }

    #[test]
    fn with_override_replaces_section_per_field() {
        let cfg = DetailBarConfig::default();
        let ovr = DetailBarOverride {
            sections: Some(SectionsOverride {
                recent_chat: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = cfg.with_override(&ovr);
        assert!(merged.sections.session_summary);
        assert!(!merged.sections.recent_chat);
        assert!(merged.sections.procs_and_files);
    }

    #[test]
    fn with_override_replaces_height_per_field() {
        let cfg = DetailBarConfig::default();
        let ovr = DetailBarOverride {
            height: Some(HeightOverride {
                percent: Some(50),
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = cfg.with_override(&ovr);
        assert_eq!(merged.height.percent, 50);
        assert_eq!(merged.height.min_rows, 8);
        assert_eq!(merged.height.max_rows, 18);
    }

    #[test]
    fn override_round_trips_through_json() {
        let ovr = DetailBarOverride {
            visible: Some(false),
            height: Some(HeightOverride {
                percent: Some(20),
                min_rows: None,
                max_rows: None,
            }),
            sections: Some(SectionsOverride {
                session_summary: None,
                recent_chat: Some(false),
                procs_and_files: None,
            }),
        };
        let json = serde_json::to_string(&ovr).unwrap();
        let parsed: DetailBarOverride = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.visible, Some(false));
        assert_eq!(parsed.height.unwrap().percent, Some(20));
        assert_eq!(parsed.sections.unwrap().recent_chat, Some(false));
    }

    #[test]
    fn empty_override_object_parses() {
        let parsed: DetailBarOverride = serde_json::from_str("{}").unwrap();
        assert!(parsed.visible.is_none());
        assert!(parsed.height.is_none());
        assert!(parsed.sections.is_none());
    }
```

- [ ] **Step 2: Run the new tests to confirm they fail to compile**

Run: `cargo test --lib detail_bar_config::tests`
Expected: compile errors — `DetailBarOverride`, `HeightOverride`, `SectionsOverride`, and `DetailBarConfig::with_override` are not defined yet.

- [ ] **Step 3: Add the override types and merge function**

Append below the `impl DetailBarConfig` block in `src/detail_bar_config.rs`:

```rust
/// Partial override of `DetailBarConfig`. Every field is optional —
/// `None` means "inherit from base."
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailBarOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<HeightOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sections: Option<SectionsOverride>,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionsOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_summary: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recent_chat: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub procs_and_files: Option<bool>,
}
```

Then add a method on `DetailBarConfig` (inside its existing `impl` block, after `preferred_height`):

```rust
    /// Apply an override on top of self. Repo wins per-field.
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
        if let Some(s) = &ovr.sections {
            if let Some(b) = s.session_summary {
                self.sections.session_summary = b;
            }
            if let Some(b) = s.recent_chat {
                self.sections.recent_chat = b;
            }
            if let Some(b) = s.procs_and_files {
                self.sections.procs_and_files = b;
            }
        }
        self
    }
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib detail_bar_config::tests`
Expected: all tests pass (originally 12 + 6 new = 18).

- [ ] **Step 5: Commit**

```bash
git add src/detail_bar_config.rs
git commit -m "feat(detail-bar): add DetailBarOverride and per-field merge"
```

---

## Task 3: Add `sanitize()` clamping + swap

**Files:**
- Modify: `src/detail_bar_config.rs`

- [ ] **Step 1: Add failing tests**

Append to the `tests` module:

```rust
    #[test]
    fn sanitize_clamps_percent_low() {
        let mut cfg = DetailBarConfig {
            height: Height {
                percent: 0,
                min_rows: 8,
                max_rows: 18,
            },
            ..DetailBarConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.height.percent, 5);
    }

    #[test]
    fn sanitize_clamps_percent_high() {
        let mut cfg = DetailBarConfig {
            height: Height {
                percent: 200,
                min_rows: 8,
                max_rows: 18,
            },
            ..DetailBarConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.height.percent, 80);
    }

    #[test]
    fn sanitize_clamps_min_rows() {
        let mut cfg = DetailBarConfig {
            height: Height {
                percent: 30,
                min_rows: 1,
                max_rows: 18,
            },
            ..DetailBarConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.height.min_rows, 4);
    }

    #[test]
    fn sanitize_clamps_max_rows() {
        let mut cfg = DetailBarConfig {
            height: Height {
                percent: 30,
                min_rows: 8,
                max_rows: 200,
            },
            ..DetailBarConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.height.max_rows, 60);
    }

    #[test]
    fn sanitize_swaps_inverted_min_max() {
        let mut cfg = DetailBarConfig {
            height: Height {
                percent: 30,
                min_rows: 20,
                max_rows: 10,
            },
            ..DetailBarConfig::default()
        };
        cfg.sanitize();
        assert_eq!(cfg.height.min_rows, 10);
        assert_eq!(cfg.height.max_rows, 20);
    }

    #[test]
    fn sanitize_leaves_legal_values_alone() {
        let original = DetailBarConfig::default();
        let mut cfg = original.clone();
        cfg.sanitize();
        assert_eq!(cfg, original);
    }
```

- [ ] **Step 2: Run to confirm they fail**

Run: `cargo test --lib detail_bar_config::tests sanitize`
Expected: compile error — `sanitize` not defined.

- [ ] **Step 3: Implement `sanitize`**

Add a method to the `impl DetailBarConfig` block, after `with_override`:

```rust
    /// Clamp height fields into legal ranges, swapping min/max when
    /// inverted. Idempotent.
    pub fn sanitize(&mut self) {
        self.height.percent = self.height.percent.clamp(5, 80);
        self.height.min_rows = self.height.min_rows.clamp(4, 40);
        self.height.max_rows = self.height.max_rows.clamp(self.height.min_rows, 60);
        if self.height.min_rows > self.height.max_rows {
            std::mem::swap(&mut self.height.min_rows, &mut self.height.max_rows);
        }
    }
```

Note: the clamp of `max_rows` to `[min_rows, 60]` plus the swap covers the swap case explicitly even though `clamp` would normally panic if `low > high`. By clamping `max_rows` against `min_rows` first, then doing the swap, we handle the inverted case safely.

Actually, that clamp would still panic when `min_rows > 60`. Rewrite the body more carefully:

```rust
    /// Clamp height fields into legal ranges, swapping min/max when
    /// inverted. Idempotent.
    pub fn sanitize(&mut self) {
        self.height.percent = self.height.percent.clamp(5, 80);
        self.height.min_rows = self.height.min_rows.clamp(4, 40);
        self.height.max_rows = self.height.max_rows.clamp(4, 60);
        if self.height.min_rows > self.height.max_rows {
            std::mem::swap(&mut self.height.min_rows, &mut self.height.max_rows);
        }
    }
```

Use this second form — it's safe regardless of input ordering.

- [ ] **Step 4: Run tests to confirm they pass**

Run: `cargo test --lib detail_bar_config::tests`
Expected: all sanitize tests pass; existing tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/detail_bar_config.rs
git commit -m "feat(detail-bar): clamp and swap-fix height bounds in sanitize()"
```

---

## Task 4: Schema migration + `Repo` column + store helpers

**Files:**
- Modify: `src/store.rs`

This adds a v11 migration that adds the `detail_bar_config TEXT` column to the `repos` table, wires the column into `Repo`, `repos()`, and adds `set_repo_detail_bar_config`.

- [ ] **Step 1: Add a failing test for the round-trip**

Find the existing `#[cfg(test)] mod tests` in `src/store.rs` (use `grep -n "^mod tests" src/store.rs` to locate). Append inside that module:

```rust
    #[test]
    fn detail_bar_config_column_round_trips() {
        let store = Store::open_in_memory().unwrap();
        let id = store
            .add_repo(Path::new("/some/repo"), "demo", "")
            .unwrap();

        // Default: column is NULL.
        let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert!(repo.detail_bar_config.is_none());

        // Set a value, read it back.
        store
            .set_repo_detail_bar_config(id, Some(r#"{"visible":false}"#))
            .unwrap();
        let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert_eq!(
            repo.detail_bar_config.as_deref(),
            Some(r#"{"visible":false}"#)
        );

        // Clear it back to NULL.
        store.set_repo_detail_bar_config(id, None).unwrap();
        let repo = store.repos().unwrap().into_iter().find(|r| r.id == id).unwrap();
        assert!(repo.detail_bar_config.is_none());
    }
```

`Path` is already in scope via `use super::*;` at the top of the test module.

- [ ] **Step 2: Run the test to confirm it fails to compile**

Run: `cargo test --lib store::tests::detail_bar_config_column`
Expected: compile error — `detail_bar_config` is not a field on `Repo`; `set_repo_detail_bar_config` is not a method.

- [ ] **Step 3: Add the column to the `Repo` struct**

Edit `src/store.rs` around line 31-44. Add a new field before `created_at`:

```rust
#[derive(Debug, Clone)]
pub struct Repo {
    pub id: RepoId,
    pub name: String,
    pub path: PathBuf,
    pub branch_prefix: String,
    pub custom_instructions: Option<String>,
    pub setup_script: Option<String>,
    pub archive_script: Option<String>,
    pub pinned_commands: Option<String>,
    pub related_repos: Option<String>,
    pub base_branch: Option<String>,
    pub detail_bar_config: Option<String>,
    pub created_at: i64,
}
```

- [ ] **Step 4: Add the v11 migration**

Edit `src/store.rs` in the `migrate` method. Find the existing `if v < 10` block (around line 205-208) and append after it, before `Ok(())`:

```rust
        if v < 11 {
            let has_col: i64 = self.conn.query_row(
                "SELECT count(*) FROM pragma_table_info('repos') WHERE name = 'detail_bar_config'",
                [],
                |r| r.get(0),
            )?;
            if has_col == 0 {
                self.conn
                    .execute("ALTER TABLE repos ADD COLUMN detail_bar_config TEXT", [])?;
            }
            self.conn.execute("PRAGMA user_version = 11", [])?;
        }
```

- [ ] **Step 5: Update `repos()` to read the new column**

Edit `src/store.rs` around lines 229-252. Change the SQL and row mapping:

```rust
    pub fn repos(&self) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, path, branch_prefix, custom_instructions, \
                    setup_script, archive_script, pinned_commands, \
                    related_repos, base_branch, detail_bar_config, created_at \
             FROM repos ORDER BY id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Repo {
                id: RepoId(r.get(0)?),
                name: r.get(1)?,
                path: PathBuf::from(r.get::<_, String>(2)?),
                branch_prefix: r.get(3)?,
                custom_instructions: r.get(4)?,
                setup_script: r.get(5)?,
                archive_script: r.get(6)?,
                pinned_commands: r.get(7)?,
                related_repos: r.get(8)?,
                base_branch: r.get(9)?,
                detail_bar_config: r.get(10)?,
                created_at: r.get(11)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }
```

- [ ] **Step 6: Add `set_repo_detail_bar_config`**

Edit `src/store.rs` — add after `set_repo_base_branch` (around line 378-384):

```rust
    pub fn set_repo_detail_bar_config(
        &self,
        id: RepoId,
        value: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE repos SET detail_bar_config = ?1 WHERE id = ?2",
            rusqlite::params![value, id.0],
        )?;
        Ok(())
    }
```

- [ ] **Step 7: Fix any compile errors in test fixtures**

There may be one or more test helpers in `src/repo.rs` (around line 97-111) that construct `Repo` literals. They will now fail to compile because the new field is missing. Find them with `grep -n "id: RepoId" src/`:

```bash
grep -rn "id: RepoId(" src/ tests/ --include="*.rs"
```

For each spot, add `detail_bar_config: None,` to the struct literal in the right ordering (before `created_at`). Specifically in `src/repo.rs` around line 97:

```rust
    fn repo(prefix: &str, instructions: Option<&str>) -> Repo {
        Repo {
            id: RepoId(1),
            name: "demo".into(),
            path: PathBuf::from("/r"),
            branch_prefix: prefix.into(),
            custom_instructions: instructions.map(|s| s.to_string()),
            setup_script: None,
            archive_script: None,
            pinned_commands: None,
            related_repos: None,
            base_branch: None,
            detail_bar_config: None,
            created_at: 0,
        }
    }
```

If `grep` finds additional literals (likely in `src/related.rs`'s tests around line 88), apply the same fix.

- [ ] **Step 8: Run the round-trip test and the full test suite**

Run: `cargo test --lib store::tests::detail_bar_config_column_round_trips`
Expected: PASS.

Run: `cargo test --lib`
Expected: all existing tests still pass.

- [ ] **Step 9: Commit**

```bash
git add src/store.rs src/repo.rs src/related.rs
git commit -m "feat(store): add detail_bar_config column with v11 migration"
```

---

## Task 5: Add `resolve(repo, store)` with fallback + logging

**Files:**
- Modify: `src/detail_bar_config.rs`

- [ ] **Step 1: Add failing tests**

Append to the `tests` module in `src/detail_bar_config.rs`:

```rust
    use crate::store::{Repo, RepoId, Store};
    use std::path::PathBuf;

    fn test_repo(detail_bar_config: Option<&str>) -> Repo {
        Repo {
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
            detail_bar_config: detail_bar_config.map(|s| s.to_string()),
            created_at: 0,
        }
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
        store
            .set_setting("detail_bar_config", r#"{"visible": false}"#)
            .unwrap();
        let repo = test_repo(None);
        assert!(!resolve(&repo, &store).visible);
    }

    #[test]
    fn resolve_applies_repo_override_on_top_of_global() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("detail_bar_config", r#"{"visible": false}"#)
            .unwrap();
        // Override re-enables visible.
        let repo = test_repo(Some(r#"{"visible": true}"#));
        assert!(resolve(&repo, &store).visible);
    }

    #[test]
    fn resolve_falls_back_when_global_json_malformed() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("detail_bar_config", "{not json")
            .unwrap();
        let repo = test_repo(None);
        // Doesn't panic; returns default.
        assert_eq!(resolve(&repo, &store), DetailBarConfig::default());
    }

    #[test]
    fn resolve_ignores_repo_override_when_malformed() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("detail_bar_config", r#"{"visible": false}"#)
            .unwrap();
        let repo = test_repo(Some("not json"));
        // Falls back to global, ignoring bad override.
        assert!(!resolve(&repo, &store).visible);
    }

    #[test]
    fn resolve_clamps_out_of_range_percent() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("detail_bar_config", r#"{"height": {"percent": 200}}"#)
            .unwrap();
        let repo = test_repo(None);
        assert_eq!(resolve(&repo, &store).height.percent, 80);
    }
```

- [ ] **Step 2: Run to confirm they fail**

Run: `cargo test --lib detail_bar_config::tests::resolve`
Expected: compile errors — `resolve` not defined.

- [ ] **Step 3: Implement `resolve`**

First, add the imports at the top of `src/detail_bar_config.rs` (next to the existing `use serde::{Deserialize, Serialize};`):

```rust
use crate::store::{Repo, Store};
```

Then append the function at the end of the file (after the override struct definitions but outside any `impl`):

```rust
/// Resolve the effective `DetailBarConfig` for `repo`. Reads the
/// global blob from `settings` and applies the per-repo override.
/// Malformed JSON in either location logs a warning and is treated
/// as unset.
pub fn resolve(repo: &Repo, store: &Store) -> DetailBarConfig {
    let mut cfg = match store.get_setting("detail_bar_config") {
        Ok(Some(s)) => match serde_json::from_str::<DetailBarConfig>(&s) {
            Ok(parsed) => parsed,
            Err(e) => {
                tracing::warn!(
                    err = %e,
                    "detail_bar_config: global parse failed; using defaults"
                );
                DetailBarConfig::default()
            }
        },
        _ => DetailBarConfig::default(),
    };
    if let Some(raw) = repo.detail_bar_config.as_deref() {
        match serde_json::from_str::<DetailBarOverride>(raw) {
            Ok(ovr) => cfg = cfg.with_override(&ovr),
            Err(e) => {
                tracing::warn!(
                    err = %e,
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

- [ ] **Step 4: Run tests to confirm they pass**

Run: `cargo test --lib detail_bar_config::tests`
Expected: all tests pass (originally 24 + 6 new = 30).

- [ ] **Step 5: Commit**

```bash
git add src/detail_bar_config.rs
git commit -m "feat(detail-bar): add resolve() with serde fallback and sanitize"
```

---

## Task 6: Adapt the detail bar renderer

**Files:**
- Modify: `src/ui/dashboard/detail.rs`

Remove the free `preferred_height` / `MIN_HEIGHT`, take a `&DetailBarConfig` via `DetailInputs`, and skip disabled columns.

- [ ] **Step 1: Add failing tests for config-aware rendering**

Find the existing `#[cfg(test)] mod tests` block at the bottom of `src/ui/dashboard/detail.rs` (use `grep -n "mod tests" src/ui/dashboard/detail.rs`). Append these tests inside:

```rust
    use crate::detail_bar_config::{DetailBarConfig, Sections};

    #[test]
    fn renders_three_columns_with_default_config() {
        // Smoke test that the renderer accepts a default config and
        // does not panic on a typical-sized area.
        let cfg = DetailBarConfig::default();
        assert!(cfg.has_body());
        // The body builder is exercised in the existing rendering
        // tests; this test simply asserts that the default config
        // keeps all three sections enabled.
        assert!(cfg.sections.session_summary);
        assert!(cfg.sections.recent_chat);
        assert!(cfg.sections.procs_and_files);
    }

    #[test]
    fn enabled_columns_helper_returns_subset() {
        let mut cfg = DetailBarConfig::default();
        cfg.sections = Sections {
            session_summary: true,
            recent_chat: false,
            procs_and_files: true,
        };
        let cols = enabled_columns(&cfg);
        assert_eq!(cols, vec![Column::SessionSummary, Column::ProcsAndFiles]);
    }

    #[test]
    fn enabled_columns_empty_when_all_disabled() {
        let mut cfg = DetailBarConfig::default();
        cfg.sections = Sections {
            session_summary: false,
            recent_chat: false,
            procs_and_files: false,
        };
        assert!(enabled_columns(&cfg).is_empty());
    }

    #[test]
    fn column_widths_three_cols_match_legacy() {
        assert_eq!(
            column_widths(&[
                Column::SessionSummary,
                Column::RecentChat,
                Column::ProcsAndFiles
            ]),
            vec![30u16, 40, 30]
        );
    }

    #[test]
    fn column_widths_two_cols_summary_chat() {
        assert_eq!(
            column_widths(&[Column::SessionSummary, Column::RecentChat]),
            vec![43u16, 57]
        );
    }

    #[test]
    fn column_widths_two_cols_summary_procs() {
        assert_eq!(
            column_widths(&[Column::SessionSummary, Column::ProcsAndFiles]),
            vec![50u16, 50]
        );
    }

    #[test]
    fn column_widths_two_cols_chat_procs() {
        assert_eq!(
            column_widths(&[Column::RecentChat, Column::ProcsAndFiles]),
            vec![57u16, 43]
        );
    }

    #[test]
    fn column_widths_single_col_is_full() {
        assert_eq!(column_widths(&[Column::RecentChat]), vec![100u16]);
    }
```

- [ ] **Step 2: Confirm they fail to compile**

Run: `cargo test --lib ui::dashboard::detail::tests::column_widths`
Expected: compile errors — `enabled_columns`, `column_widths`, `Column` are not defined.

- [ ] **Step 3: Remove the free `preferred_height` and `MIN_HEIGHT`**

Edit `src/ui/dashboard/detail.rs` lines 7-18. Delete:

```rust
/// Minimum rows the bar needs to render usefully (1 header + 1 rule + 3
/// body + 1 rule + 1 input + 1 spacing slack).
pub const MIN_HEIGHT: u16 = 8;

/// Compute the detail bar's preferred height given the total available
/// height. Targets ~30% of the area, clamped to `[MIN_HEIGHT, 18]`.
/// The ceiling stops very tall terminals from giving the bar an
/// unreasonable share of the screen.
pub fn preferred_height(total_height: u16) -> u16 {
    let target = (u32::from(total_height) * 30 / 100) as u16;
    target.clamp(MIN_HEIGHT, 18)
}
```

Replace with:

```rust
use crate::detail_bar_config::DetailBarConfig;
```

(if the `use` is not already present near the other `use` lines around line 20; if there's already a `use crate::...` block, add it there).

- [ ] **Step 4: Add `Column` enum and helpers**

Add this near the top of `src/ui/dashboard/detail.rs`, just after the existing `use` block:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    SessionSummary,
    RecentChat,
    ProcsAndFiles,
}

pub fn enabled_columns(cfg: &DetailBarConfig) -> Vec<Column> {
    let mut out = Vec::with_capacity(3);
    if cfg.sections.session_summary {
        out.push(Column::SessionSummary);
    }
    if cfg.sections.recent_chat {
        out.push(Column::RecentChat);
    }
    if cfg.sections.procs_and_files {
        out.push(Column::ProcsAndFiles);
    }
    out
}

/// Width percentages for the enabled body columns. Preserves the
/// legacy 30/40/30 ratio when all three are present; redistributes
/// proportionally otherwise.
pub fn column_widths(cols: &[Column]) -> Vec<u16> {
    use Column::*;
    match cols {
        [] => vec![],
        [_] => vec![100],
        [SessionSummary, RecentChat] => vec![43, 57],
        [SessionSummary, ProcsAndFiles] => vec![50, 50],
        [RecentChat, ProcsAndFiles] => vec![57, 43],
        [SessionSummary, RecentChat, ProcsAndFiles] => vec![30, 40, 30],
        // Any other ordering is unreachable given the fixed display
        // order of `enabled_columns`. Fall back to even split.
        _ => {
            let n = cols.len() as u16;
            let each = 100 / n;
            (0..n).map(|_| each).collect()
        }
    }
}
```

- [ ] **Step 5: Add the `config` field to `DetailInputs`**

Edit `src/ui/dashboard/detail.rs` around line 32-54. Add the new field after `events_scanned`:

```rust
#[derive(Debug)]
pub struct DetailInputs<'a> {
    pub repo: &'a Repo,
    pub workspace: &'a Workspace,
    pub events: Option<&'a WorkspaceEvents>,
    pub procs: &'a [ProcInfo],
    pub diff: Option<DiffStats>,
    pub diff_per_file: Option<&'a std::collections::HashMap<String, DiffStats>>,
    pub lifecycle: Option<BranchLifecycle>,
    pub pr_title: Option<String>,
    pub pr_number: Option<u32>,
    pub status: Status,
    pub ago_secs: Option<u64>,
    pub reply_draft: &'a str,
    pub reply_focused: bool,
    pub events_scanned: bool,
    pub config: &'a DetailBarConfig,
}
```

- [ ] **Step 6: Update the early-bail check in `render`**

Edit `src/ui/dashboard/detail.rs` around line 59-62. Change:

```rust
    if area.height == 0 || area.height < MIN_HEIGHT {
        return;
    }
```

to:

```rust
    if area.height == 0 || area.height < inputs.config.height.min_rows {
        return;
    }
```

- [ ] **Step 7: Adapt the body layout to skip disabled columns**

Edit `src/ui/dashboard/detail.rs` around line 105-150 (the `if chunks[2].width >= 80 { ... } else { ... }` block). Replace it entirely with:

```rust
    let cols = enabled_columns(inputs.config);
    if chunks[2].width >= 80 && cols.len() > 1 {
        let widths = column_widths(&cols);
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                widths
                    .iter()
                    .map(|w| Constraint::Percentage(*w))
                    .collect::<Vec<_>>(),
            )
            .split(chunks[2]);
        for (idx, col) in cols.iter().enumerate() {
            let area = body_chunks[idx];
            render_column(f, area, *col, inputs, theme, chunks[2].height);
        }
    } else if let Some(only) = cols.first() {
        // Narrow terminal OR single enabled column → render whichever
        // column comes first in display order at full width.
        render_column(f, chunks[2], *only, inputs, theme, chunks[2].height);
    }
    // If `cols` is empty, body region is rendered as blank (no-op).
```

Then add a helper `render_column` below the existing `render` function. Place it before the existing `build_header_strip` (use `grep -n "fn build_header_strip" src/ui/dashboard/detail.rs` to locate; insert just above it):

```rust
fn render_column(
    f: &mut Frame,
    area: Rect,
    col: Column,
    inputs: &DetailInputs<'_>,
    theme: &Theme,
    body_height: u16,
) {
    use ratatui::widgets::Paragraph;
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let created_at_secs = (inputs.workspace.created_at.max(0) / 1000) as u64;
    let created_secs = now_secs.saturating_sub(created_at_secs);

    match col {
        Column::SessionSummary => {
            let lines = build_session_summary(
                if inputs.events_scanned {
                    inputs.events
                } else {
                    None
                },
                inputs.status,
                theme,
                area.width as usize,
                created_secs,
                inputs.ago_secs,
            );
            f.render_widget(Paragraph::new(lines), area);
        }
        Column::RecentChat => {
            let lines = build_recent_chat(
                if inputs.events_scanned {
                    inputs.events
                } else {
                    None
                },
                theme,
                area.width as usize,
                (body_height as usize).saturating_sub(1).max(1),
            );
            f.render_widget(Paragraph::new(lines), area);
        }
        Column::ProcsAndFiles => {
            let lines = build_procs_and_files(
                inputs.procs,
                inputs.events,
                inputs.diff_per_file,
                &inputs.workspace.worktree_path,
                theme,
                area.width as usize,
            );
            f.render_widget(Paragraph::new(lines), area);
        }
    }
}
```

The previous inline `let summary_lines = ...; let chat_lines = ...; let procs_lines = ...;` blocks (originally between lines 115-149) are now subsumed by `render_column` and should be deleted from the body of `render`.

- [ ] **Step 8: Update existing tests in detail.rs to pass `config`**

Any existing test that constructs `DetailInputs { ... }` will now fail to compile. Locate them with `grep -n "DetailInputs {" src/ui/dashboard/detail.rs` and add `config: &DetailBarConfig::default(),` to each. Use a local `let cfg = DetailBarConfig::default();` and reference `&cfg` if the inputs are short-lived.

(If the existing tests don't construct `DetailInputs` directly but exercise the lower-level builders like `build_session_summary`, no change needed.)

- [ ] **Step 9: Run the test suite**

Run: `cargo test --lib ui::dashboard::detail`
Expected: all detail tests pass (new + existing).

Run: `cargo build --lib`
Expected: there will be compile errors in `src/app.rs` because callers reference `MIN_HEIGHT`, `preferred_height`, and construct `DetailInputs` without `config`. Those are fixed in Task 7. **Do not commit yet** — fixing app.rs is part of the same logical change.

- [ ] **Step 10: (Defer commit; combined with Task 7.)**

---

## Task 7: Resolve config in `app.rs` and thread it through

**Files:**
- Modify: `src/app.rs`

- [ ] **Step 1: Replace `MIN_HEIGHT` reference with min_rows**

Edit `src/app.rs` around line 740-747. Change:

```rust
            let detail_visible = selection_is_workspace
                && area.height >= crate::ui::dashboard::detail::MIN_HEIGHT + 10;
```

to:

```rust
            let detail_cfg = resolve_dashboard_detail_cfg(app);
            let detail_visible = selection_is_workspace
                && detail_cfg.visible
                && area.height >= detail_cfg.height.min_rows + 10;
```

- [ ] **Step 2: Add the `resolve_dashboard_detail_cfg` helper**

Add this free function near the existing layout helpers in `src/app.rs` (above `dashboard_regions`, around line 1267):

```rust
/// Resolve the detail-bar config for the current selection. When a
/// workspace is selected, uses its repo's override; otherwise uses
/// global-only (no repo override applies when no repo is in focus).
fn resolve_dashboard_detail_cfg(app: &App) -> crate::detail_bar_config::DetailBarConfig {
    if let Some(SelectionTarget::Workspace(ws_id)) = app.selected_target() {
        if let Some((rid, _)) = app.workspaces.iter().find(|(_, w)| w.id == ws_id) {
            if let Some(repo) = app.repos.iter().find(|r| r.id == *rid) {
                return crate::detail_bar_config::resolve(repo, &app.store);
            }
        }
    }
    // No workspace selected → resolve from a placeholder repo with no
    // override, so global settings still apply.
    let mut cfg = crate::detail_bar_config::DetailBarConfig::default();
    if let Ok(Some(s)) = app.store.get_setting("detail_bar_config") {
        if let Ok(parsed) =
            serde_json::from_str::<crate::detail_bar_config::DetailBarConfig>(&s)
        {
            cfg = parsed;
        }
    }
    cfg.sanitize();
    cfg
}
```

- [ ] **Step 3: Thread `detail_cfg` into the `DetailInputs` construction**

Edit `src/app.rs` around line 987. Find the existing `DetailInputs { ... }` literal and add `config: &detail_cfg,` as the final field:

```rust
                        let inputs = crate::ui::dashboard::detail::DetailInputs {
                            repo,
                            workspace: ws,
                            events,
                            procs: procs.as_slice(),
                            diff,
                            diff_per_file,
                            lifecycle,
                            pr_title: None,
                            pr_number: None,
                            status,
                            ago_secs,
                            reply_draft: app.dashboard.reply_draft.as_str(),
                            reply_focused,
                            events_scanned,
                            config: &detail_cfg,
                        };
```

(Adjust field names to match what's currently there — only add the new `config: &detail_cfg,` field.)

- [ ] **Step 4: Pass cfg into `dashboard_regions`**

Change the function signature of `dashboard_regions` (around line 1268-1310). Replace the existing function with:

```rust
fn dashboard_regions(
    area: ratatui::layout::Rect,
    pm_visible: bool,
    detail_visible: bool,
    detail_cfg: &crate::detail_bar_config::DetailBarConfig,
) -> (
    ratatui::layout::Rect,
    Option<ratatui::layout::Rect>,
    Option<ratatui::layout::Rect>,
) {
    use ratatui::layout::{Constraint, Direction, Layout};
    let detail_h = detail_cfg.preferred_height(area.height);
    match (pm_visible, detail_visible) {
        (false, false) => (area, None, None),
        (false, true) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(0), Constraint::Length(detail_h)])
                .split(area);
            (chunks[0], Some(chunks[1]), None)
        }
        (true, false) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(area);
            (chunks[0], None, Some(chunks[1]))
        }
        (true, true) => {
            let pm_h = ((u32::from(area.height) * 33 / 100) as u16).max(6);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(0),
                    Constraint::Length(detail_h),
                    Constraint::Length(pm_h),
                ])
                .split(area);
            (chunks[0], Some(chunks[1]), Some(chunks[2]))
        }
    }
}
```

- [ ] **Step 5: Update the caller of `dashboard_regions` to pass the cfg**

Search for the call site with `grep -n "dashboard_regions(" src/app.rs`. It's invoked once. Change:

```rust
            let (list_area, detail_area, pm_area) =
                dashboard_regions(inner_area, app.pm_visible, detail_visible);
```

to:

```rust
            let (list_area, detail_area, pm_area) =
                dashboard_regions(inner_area, app.pm_visible, detail_visible, &detail_cfg);
```

- [ ] **Step 6: Auto-return reply focus when bar is hidden**

Still in the Dashboard match arm (around line 740-1006), after computing `detail_visible`, insert:

```rust
            // If the bar is hidden but focus is on the reply input,
            // bounce focus back to Dashboard and drop the draft.
            if !detail_visible
                && matches!(app.focus, PaneFocus::DetailBarReply)
            {
                app.focus = PaneFocus::Dashboard;
                app.dashboard.reply_draft.clear();
            }
```

Place this immediately after the `let detail_visible = ...` block from Step 1 and before the `inner_area` carve.

- [ ] **Step 7: Update Tab cycle to skip DetailBarReply when bar hidden**

Edit `src/app.rs` around lines 1718-1731. The current Tab handler in `handle_key_dashboard` is:

```rust
    // Tab when focus is on Dashboard: workspace selection → DetailBarReply;
    // repo selection with PM visible → ProjectManager.
    if matches!(app.focus, crate::ui::PaneFocus::Dashboard) && k.code == KeyCode::Tab {
        app.z_leader_pending = false;
        if matches!(app.selected_target(), Some(SelectionTarget::Workspace(_))) {
            app.focus = crate::ui::PaneFocus::DetailBarReply;
        } else if app.pm_visible {
            app.focus = crate::ui::PaneFocus::ProjectManager;
        }
        return Ok(());
    }
```

Replace the inner Workspace branch with a `cfg.visible` guard:

```rust
    // Tab when focus is on Dashboard: workspace selection → DetailBarReply
    // (unless the detail bar is hidden by config); repo selection with PM
    // visible → ProjectManager.
    if matches!(app.focus, crate::ui::PaneFocus::Dashboard) && k.code == KeyCode::Tab {
        app.z_leader_pending = false;
        let cfg = resolve_dashboard_detail_cfg(app);
        let is_workspace = matches!(
            app.selected_target(),
            Some(SelectionTarget::Workspace(_))
        );
        if is_workspace && cfg.visible {
            app.focus = crate::ui::PaneFocus::DetailBarReply;
        } else if app.pm_visible {
            app.focus = crate::ui::PaneFocus::ProjectManager;
        }
        return Ok(());
    }
```

The semantic change: when a workspace is selected but `cfg.visible == false`,
the original code went to `DetailBarReply` (a now-invisible target); the
fixed code falls through to the `else if app.pm_visible` branch instead,
matching what the user sees on screen.

No automated test seam exists for the focus cycle in this file. Verification
of the new behavior happens via the manual test in Task 10 (section 8).

- [ ] **Step 8: Run the build and full test suite**

Run: `cargo build --lib`
Expected: success.

Run: `cargo test --lib`
Expected: all tests pass (existing + new from Tasks 1-6).

Run: `cargo build`
Expected: success — includes the binary.

- [ ] **Step 9: Commit (Tasks 6 + 7 together)**

```bash
git add src/ui/dashboard/detail.rs src/app.rs
git commit -m "feat(detail-bar): apply DetailBarConfig in renderer and dashboard layout"
```

---

## Task 8: Add `RepoSettingField::DetailBarConfig` + modal row + apply

**Files:**
- Modify: `src/app.rs`
- Modify: `src/ui/modal.rs`

- [ ] **Step 1: Extend `RepoSettingField` enum**

Edit `src/app.rs` lines 40-49:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoSettingField {
    RepoName,
    BranchPrefix,
    BaseBranch,
    CustomInstructions,
    SetupScript,
    ArchiveScript,
    PinnedCommands,
    RelatedRepos,
    DetailBarConfig,
}
```

(Keep whatever derive attributes were already on the enum; don't remove them.)

Then update `ALL` (around line 51-61):

```rust
impl RepoSettingField {
    pub const ALL: [Self; 9] = [
        Self::RepoName,
        Self::BranchPrefix,
        Self::BaseBranch,
        Self::CustomInstructions,
        Self::SetupScript,
        Self::ArchiveScript,
        Self::PinnedCommands,
        Self::RelatedRepos,
        Self::DetailBarConfig,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::RepoName => "name",
            Self::BranchPrefix => "branch_prefix",
            Self::BaseBranch => "base_branch",
            Self::CustomInstructions => "custom_instructions",
            Self::SetupScript => "setup_script",
            Self::ArchiveScript => "archive_script",
            Self::PinnedCommands => "pinned_commands",
            Self::RelatedRepos => "related_repos",
            Self::DetailBarConfig => "detail_bar_config",
        }
    }
}
```

- [ ] **Step 2: Add the editor-launch arm**

Edit `src/app.rs` around line 571-591 (inside `do_pending_edit`). Add a match arm:

```rust
            RepoSettingField::DetailBarConfig => {
                let raw = repo
                    .detail_bar_config
                    .clone()
                    .unwrap_or_else(|| "{}\n".to_string());
                (raw, "json")
            }
```

Append it before the closing `}` of the `match edit.field { ... }` block.

- [ ] **Step 3: Add the save-handler arm with validation**

Edit `src/app.rs` around line 2150-2177 in `apply_repo_setting`. Add a match arm:

```rust
        RepoSettingField::DetailBarConfig => {
            if trimmed.is_empty() {
                app.store.set_repo_detail_bar_config(repo_id, None)
            } else {
                // Validate. Use DetailBarOverride (not DetailBarConfig)
                // because per-repo entries are partial overrides.
                match serde_json::from_str::<crate::detail_bar_config::DetailBarOverride>(trimmed) {
                    Ok(_) => app.store.set_repo_detail_bar_config(repo_id, Some(trimmed)),
                    Err(e) => Err(crate::error::Error::UserInput(format!(
                        "detail_bar_config is not valid JSON: {e}"
                    ))),
                }
            }
        }
```

- [ ] **Step 4: Extend the modal `rows` array**

Edit `src/ui/modal.rs` around line 535-572. Change the rows array length and add the new row at the end:

```rust
    let rows: [(crate::app::RepoSettingField, Option<&str>); 9] = [
        (
            crate::app::RepoSettingField::RepoName,
            Some(repo.name.as_str()),
        ),
        (
            crate::app::RepoSettingField::BranchPrefix,
            if repo.branch_prefix.is_empty() {
                None
            } else {
                Some(repo.branch_prefix.as_str())
            },
        ),
        (
            crate::app::RepoSettingField::BaseBranch,
            repo.base_branch.as_deref(),
        ),
        (
            crate::app::RepoSettingField::CustomInstructions,
            repo.custom_instructions.as_deref(),
        ),
        (
            crate::app::RepoSettingField::SetupScript,
            repo.setup_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::ArchiveScript,
            repo.archive_script.as_deref(),
        ),
        (
            crate::app::RepoSettingField::PinnedCommands,
            repo.pinned_commands.as_deref(),
        ),
        (
            crate::app::RepoSettingField::RelatedRepos,
            repo.related_repos.as_deref(),
        ),
        (
            crate::app::RepoSettingField::DetailBarConfig,
            repo.detail_bar_config.as_deref(),
        ),
    ];
```

- [ ] **Step 5: Run the test suite**

Run: `cargo test --lib`
Expected: all tests pass.

Run: `cargo build`
Expected: success.

- [ ] **Step 6: Commit**

```bash
git add src/app.rs src/ui/modal.rs
git commit -m "feat(detail-bar): expose detail_bar_config in repo-settings modal"
```

---

## Task 9: Pretty-print default seed + validate on CLI save

**Files:**
- Modify: `src/cli.rs`

When the user runs `wsx config edit detail_bar_config` and the stored value is empty, seed the editor with the pretty-printed default. On save, parse and clamp.

- [ ] **Step 1: Add tests in `src/cli.rs`**

Locate the existing `#[cfg(test)] mod tests` block at the bottom of `src/cli.rs`. Append:

```rust
    #[test]
    fn detail_bar_config_seed_returns_pretty_default_when_empty() {
        let seed = super::detail_bar_config_seed_for_empty();
        // Sanity: round-trips to default config.
        let parsed: crate::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&seed).unwrap();
        assert_eq!(parsed, crate::detail_bar_config::DetailBarConfig::default());
        // Pretty-printed: contains newlines.
        assert!(seed.contains('\n'));
    }

    #[test]
    fn detail_bar_config_validate_rejects_malformed() {
        let result = super::detail_bar_config_validate_and_normalize("{not json");
        assert!(result.is_err());
    }

    #[test]
    fn detail_bar_config_validate_clamps_out_of_range() {
        let json = r#"{"height": {"percent": 200}}"#;
        let normalized = super::detail_bar_config_validate_and_normalize(json).unwrap();
        let parsed: crate::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&normalized).unwrap();
        assert_eq!(parsed.height.percent, 80);
    }

    #[test]
    fn detail_bar_config_validate_accepts_partial() {
        let json = r#"{"visible": false}"#;
        let normalized = super::detail_bar_config_validate_and_normalize(json).unwrap();
        let parsed: crate::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&normalized).unwrap();
        assert!(!parsed.visible);
        assert_eq!(parsed.height.percent, 30);
    }
```

- [ ] **Step 2: Confirm tests fail**

Run: `cargo test --lib cli::tests::detail_bar_config`
Expected: compile errors — helpers don't exist yet.

- [ ] **Step 3: Add the helpers**

Add these free functions in `src/cli.rs` near the existing `open_in_editor` helper (around line 893):

```rust
/// Seed text for the editor when the global `detail_bar_config`
/// setting is empty — the pretty-printed default config.
fn detail_bar_config_seed_for_empty() -> String {
    serde_json::to_string_pretty(&crate::detail_bar_config::DetailBarConfig::default())
        .unwrap_or_else(|_| "{}".to_string())
}

/// Parse, sanitize, and re-serialize a global `detail_bar_config`
/// blob. Returns the pretty-printed normalized JSON.
fn detail_bar_config_validate_and_normalize(raw: &str) -> Result<String> {
    let mut cfg: crate::detail_bar_config::DetailBarConfig =
        serde_json::from_str(raw)
            .map_err(|e| Error::UserInput(format!("detail_bar_config: invalid JSON: {e}")))?;
    cfg.sanitize();
    serde_json::to_string_pretty(&cfg)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: serialize failed: {e}")))
}
```

- [ ] **Step 4: Modify the `ConfigEdit` arm**

Edit `src/cli.rs` lines 734-747. Replace with:

```rust
        CliAction::ConfigEdit { key } => {
            let current = store.get_setting(&key)?.unwrap_or_default();
            let seed = if key == "detail_bar_config" && current.is_empty() {
                detail_bar_config_seed_for_empty()
            } else {
                current.clone()
            };
            let new_value = open_in_editor(&key, &seed)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.is_empty() {
                store.delete_setting(&key)?;
                println!("cleared {key}");
            } else if new_value == current {
                println!("{key} unchanged");
            } else {
                let normalized = if key == "detail_bar_config" {
                    detail_bar_config_validate_and_normalize(&new_value)?
                } else {
                    new_value.clone()
                };
                store.set_setting(&key, &normalized)?;
                println!("set {key} ({} chars)", normalized.len());
            }
        }
```

Also harden `ConfigSet` for the same key (around line 709-718):

```rust
        CliAction::ConfigSet { key, source } => {
            let value = source.resolve()?;
            if value.is_empty() {
                store.delete_setting(&key)?;
                println!("cleared {key}");
            } else {
                let value = if key == "detail_bar_config" {
                    detail_bar_config_validate_and_normalize(&value)?
                } else {
                    value
                };
                store.set_setting(&key, &value)?;
                println!("set {key} ({} chars)", value.len());
            }
        }
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib cli::tests::detail_bar_config`
Expected: all 4 new tests pass.

Run: `cargo test --lib`
Expected: full suite green.

- [ ] **Step 6: Commit**

```bash
git add src/cli.rs
git commit -m "feat(cli): validate and seed detail_bar_config on edit/set"
```

---

## Task 10: Manual-test walkthrough + final verification

**Files:**
- Create: `docs/manual-tests/detail-bar-config.md`

- [ ] **Step 1: Look at existing manual tests for tone**

Run: `ls docs/manual-tests/ && head -30 docs/manual-tests/*.md | head -50`
This is just for format reference — no changes yet.

- [ ] **Step 2: Write the walkthrough**

Create `docs/manual-tests/detail-bar-config.md`:

```markdown
# Manual test — configurable detail bar

Spec: `docs/superpowers/specs/2026-05-25-detail-bar-config-design.md`

## Setup

Start wsx with at least one repo registered and a workspace selected.

## 1. Global config CLI

```bash
wsx config edit detail_bar_config
```

Expected: `$EDITOR` opens with pretty-printed default JSON:

```json
{
  "visible": true,
  "height": {
    "percent": 30,
    "min_rows": 8,
    "max_rows": 18
  },
  "sections": {
    "session_summary": true,
    "recent_chat": true,
    "procs_and_files": true
  }
}
```

Change `sections.recent_chat` to `false`, save and exit.

In wsx, select a workspace. Expected: detail bar's middle column is
gone; left + right columns redistribute to 50/50.

## 2. Per-repo override via modal

In the dashboard, press `R` to open the repo-settings modal. Navigate
down to `detail_bar_config` (last row). Press Enter.

Expected: editor opens with `{}\n` (empty override = inherit all).

Type:

```json
{"visible": false}
```

Save and exit. Expected: the detail bar is gone for workspaces in
this repo; workspaces in other repos still show the bar (with the
config from step 1 in effect).

## 3. Clear the override

Reopen the repo-settings modal, navigate to `detail_bar_config`,
press `d`. Expected: row shows `(unset)`. The detail bar returns
when selecting workspaces in this repo.

## 4. Height tuning

```bash
wsx config edit detail_bar_config
```

Set `height.percent` to `50`, save. Select a workspace.

Expected: detail bar takes roughly half the dashboard vertically
(clamped by `max_rows`).

## 5. Out-of-range clamp

```bash
wsx config edit detail_bar_config
```

Set `height.percent` to `200`, save.

Expected: the message "set detail_bar_config (… chars)" is printed.
Re-run `wsx config get detail_bar_config`. Expected: `percent` is
clamped to `80`.

## 6. Empty body

Set globally:

```json
{
  "sections": {
    "session_summary": false,
    "recent_chat": false,
    "procs_and_files": false
  }
}
```

Save. Select a workspace. Expected: bar shrinks to a tight 4 rows —
header strip, two rules, and the reply input row. No empty body
region.

## 7. Narrow terminal

Resize the terminal to under 80 columns wide with a workspace
selected and the default config restored (`wsx config edit
detail_bar_config` → `{}\n` → save). Expected: the body collapses to
the first enabled column (SESSION SUMMARY by default).

Then disable SESSION SUMMARY:

```json
{"sections": {"session_summary": false}}
```

Resize narrow again. Expected: the body collapses to RECENT CHAT.

## 8. Bar hidden + Tab cycle

Set globally:

```json
{"visible": false}
```

Select a workspace and press Tab. Expected: focus stays on Dashboard
if PM is hidden; cycles between Dashboard and ProjectManager if PM is
visible. Never enters the reply input.

Set `visible` back to true. Expected: Tab cycles Dashboard ↔
DetailBarReply (PM hidden) or Dashboard → DetailBarReply →
ProjectManager → Dashboard (PM visible).

## 9. Malformed JSON

```bash
echo "{not json" | wsx config set detail_bar_config -
```

Expected: command exits non-zero with "detail_bar_config: invalid
JSON: …". The previous valid value is preserved (`wsx config get
detail_bar_config` shows it unchanged).
```

- [ ] **Step 3: Final cargo test + cargo clippy**

Run: `cargo test`
Expected: all tests pass.

Run: `cargo clippy --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 4: Commit**

```bash
git add docs/manual-tests/detail-bar-config.md
git commit -m "docs: manual-test walkthrough for configurable detail bar"
```

---

## Self-review checklist (run before declaring done)

- [ ] `cargo test --lib` — all green.
- [ ] `cargo test` — including integration tests in `tests/`, all green.
- [ ] `cargo clippy --all-targets -- -D warnings` — clean.
- [ ] `cargo build` — success.
- [ ] Open the binary, run through `docs/manual-tests/detail-bar-config.md` end to end.
- [ ] `git log --oneline` — commits read as a clean, narrated sequence; each builds.
