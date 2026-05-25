//! Display config for the workspace detail bar. Resolved from a
//! global JSON blob in `settings` + a per-repo JSON override on
//! `repos.detail_bar_config`. Per-repo wins per-field.
//!
//! See `docs/superpowers/specs/2026-05-25-detail-bar-config-design.md`.

use crate::store::{Repo, Store};
use serde::{Deserialize, Serialize};

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
    #[serde(default = "default_true")]
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
            visible: default_true(),
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
        self.sections.session_summary || self.sections.recent_chat || self.sections.procs_and_files
    }

    /// Compute the bar's preferred height for the current terminal.
    /// When no sections are enabled, the bar shrinks to its chrome
    /// height (`CHROME_ROWS`) regardless of the configured percent.
    /// Defensive against inverted `min_rows`/`max_rows` bounds: uses
    /// the lower as the floor and the higher as the ceiling, so it
    /// never panics on user-supplied configs that haven't been
    /// sanitized yet.
    pub fn preferred_height(&self, total: u16) -> u16 {
        if !self.has_body() {
            return Self::CHROME_ROWS;
        }
        let target = (u32::from(total) * u32::from(self.height.percent) / 100) as u16;
        let lo = self.height.min_rows.min(self.height.max_rows);
        let hi = self.height.min_rows.max(self.height.max_rows);
        target.clamp(lo, hi)
    }

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
}

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

/// Resolve the global-only `DetailBarConfig`. Reads the global blob
/// from `settings` (no repo override). Malformed JSON logs a warning
/// and falls back to defaults. Always returns a sanitized config.
///
/// Use this when no repo is in focus (e.g. selection is a repo header
/// or nothing). When a workspace IS selected, call `resolve` instead.
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
/// global blob from `settings` and applies the per-repo override.
/// Malformed JSON in either location logs a warning and is treated
/// as unset.
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
        let cfg = DetailBarConfig {
            sections: Sections {
                session_summary: false,
                recent_chat: false,
                procs_and_files: true,
            },
            ..DetailBarConfig::default()
        };
        assert!(cfg.has_body());
    }

    #[test]
    fn has_body_false_when_all_sections_disabled() {
        let cfg = DetailBarConfig {
            sections: Sections {
                session_summary: false,
                recent_chat: false,
                procs_and_files: false,
            },
            ..DetailBarConfig::default()
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
        let cfg = DetailBarConfig {
            sections: Sections {
                session_summary: false,
                recent_chat: false,
                procs_and_files: false,
            },
            ..DetailBarConfig::default()
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

    #[test]
    fn preferred_height_does_not_panic_with_inverted_bounds() {
        let cfg = DetailBarConfig {
            height: Height {
                percent: 30,
                min_rows: 20,
                max_rows: 10,
            },
            ..DetailBarConfig::default()
        };
        // 30% of 50 = 15. With inverted bounds (min=20, max=10), the
        // defensive swap treats lo=10, hi=20, so 15 sits in range and
        // is returned unchanged. The key assertion is "does not panic".
        let h = cfg.preferred_height(50);
        assert!((10..=20).contains(&h));
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
    fn resolve_global_only_returns_default_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(resolve_global_only(&store), DetailBarConfig::default());
    }

    #[test]
    fn resolve_global_only_logs_and_defaults_on_malformed() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("detail_bar_config", "{not json").unwrap();
        // Doesn't panic; returns default.
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
        store.set_setting("detail_bar_config", "{not json").unwrap();
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
}
