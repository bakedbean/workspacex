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

#[cfg(test)]
mod tests {
    use super::*;
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
        let parsed: DetailBarConfig =
            serde_json::from_str(r#"{"containers": [["a", "b"], ["c"]]}"#).unwrap();
        assert_eq!(
            parsed.containers,
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string()]
            ]
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
            vec!["a".into()],
            vec!["b".into()],
            vec!["c".into()],
            vec!["d".into()],
            vec!["e".into()],
            vec!["f".into()],
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
        let ovr = DetailBarOverride {
            visible: Some(false),
            ..Default::default()
        };
        assert!(!cfg.with_override(&ovr).visible);
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
        let ovr = DetailBarOverride {
            containers: None,
            ..Default::default()
        };
        let merged = cfg.clone().with_override(&ovr);
        assert_eq!(merged.containers, cfg.containers);
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
            containers: Some(vec![vec!["recent_chat".into()]]),
        };
        let json = serde_json::to_string(&ovr).unwrap();
        let parsed: DetailBarOverride = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.visible, Some(false));
        assert_eq!(parsed.height.unwrap().percent, Some(20));
        assert_eq!(
            parsed.containers.unwrap(),
            vec![vec!["recent_chat".to_string()]]
        );
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
        store
            .set_setting("detail_bar_config", r#"{"visible": false}"#)
            .unwrap();
        let repo = test_repo(Some("not json"));
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

    #[test]
    fn resolve_repo_override_whole_replaces_containers() {
        let store = Store::open_in_memory().unwrap();
        let repo = test_repo(Some(r#"{"containers": [["recent_chat"]]}"#));
        assert_eq!(
            resolve(&repo, &store).containers,
            vec![vec!["recent_chat".to_string()]]
        );
    }

    #[test]
    fn resolve_legacy_blob_silently_ignores_sections() {
        // Stored blobs from the previous schema have a top-level `sections`
        // key. Serde silently drops unknown fields, so legacy blobs parse
        // to: their preserved scalars (visible/height) + default containers.
        let store = Store::open_in_memory().unwrap();
        let legacy = r#"{
        "visible": false,
        "height": {"percent": 25, "min_rows": 8, "max_rows": 18},
        "sections": {"session_summary": true, "recent_chat": false, "procs_and_files": true}
    }"#;
        store.set_setting("detail_bar_config", legacy).unwrap();
        let repo = test_repo(None);
        let cfg = resolve(&repo, &store);
        assert!(!cfg.visible);
        assert_eq!(cfg.height.percent, 25);
        assert_eq!(cfg.containers, DetailBarConfig::default().containers);
    }
}
