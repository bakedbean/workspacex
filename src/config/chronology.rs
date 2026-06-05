//! Display config for the change-chronology bar. Resolved from a global JSON
//! blob in `settings` (`chronology_config`) + a per-repo JSON override on
//! `repos.chronology_config`. Scalar fields merge per-field; repo wins.
//! Mirrors `src/config/detail_bar_config.rs`.
//!
//! See `docs/superpowers/specs/2026-06-05-change-chronology-view-design.md`.

use crate::data::store::{Repo, Store};
use serde::{Deserialize, Serialize};

fn default_visible() -> bool {
    true
}
fn default_percent() -> u8 {
    32
}
fn default_min_cols() -> u16 {
    24
}
fn default_max_cols() -> u16 {
    60
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Left,
    Right,
}

impl Default for Side {
    fn default() -> Self {
        Side::Right
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidthSpec {
    #[serde(default = "default_percent")]
    pub percent: u8,
    #[serde(default = "default_min_cols")]
    pub min_cols: u16,
    #[serde(default = "default_max_cols")]
    pub max_cols: u16,
}

impl Default for WidthSpec {
    fn default() -> Self {
        Self {
            percent: default_percent(),
            min_cols: default_min_cols(),
            max_cols: default_max_cols(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChronologyConfig {
    #[serde(default = "default_visible")]
    pub visible: bool,
    #[serde(default)]
    pub side: Side,
    #[serde(default)]
    pub width: WidthSpec,
}

impl Default for ChronologyConfig {
    fn default() -> Self {
        Self {
            visible: default_visible(),
            side: Side::default(),
            width: WidthSpec::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChronologyOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<WidthSpec>,
}

impl ChronologyConfig {
    pub fn with_override(mut self, ovr: &ChronologyOverride) -> Self {
        if let Some(v) = ovr.visible {
            self.visible = v;
        }
        if let Some(s) = ovr.side {
            self.side = s;
        }
        if let Some(w) = &ovr.width {
            self.width = w.clone();
        }
        self
    }

    /// Clamp into legal ranges and swap inverted min/max. Idempotent.
    pub fn sanitize(&mut self) {
        self.width.percent = self.width.percent.clamp(10, 80);
        self.width.min_cols = self.width.min_cols.clamp(12, 120);
        self.width.max_cols = self.width.max_cols.clamp(12, 160);
        if self.width.min_cols > self.width.max_cols {
            std::mem::swap(&mut self.width.min_cols, &mut self.width.max_cols);
        }
    }

    /// Column width for an attach area `total` columns wide: `percent` of
    /// `total`, clamped to `[min_cols, max_cols]`.
    pub fn resolved_width(&self, total: u16) -> u16 {
        let target = (u32::from(total) * u32::from(self.width.percent) / 100) as u16;
        target.clamp(self.width.min_cols, self.width.max_cols)
    }
}

/// Resolve the global config only (no repo override). Defaults on missing key
/// or parse failure. Mirrors `detail_bar_config::resolve_global_only`.
pub fn resolve_global_only(store: &Store) -> ChronologyConfig {
    let mut cfg = match store.get_setting("chronology_config") {
        Ok(Some(raw)) => serde_json::from_str(&raw).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "chronology_config: global parse failed; using defaults");
            ChronologyConfig::default()
        }),
        _ => ChronologyConfig::default(),
    };
    cfg.sanitize();
    cfg
}

/// Resolve global config with the per-repo override applied. Mirrors
/// `detail_bar_config::resolve`.
pub fn resolve(repo: &Repo, store: &Store) -> ChronologyConfig {
    let mut cfg = resolve_global_only(store);
    if let Some(raw) = repo.chronology_config.as_deref() {
        match serde_json::from_str::<ChronologyOverride>(raw) {
            Ok(ovr) => cfg = cfg.with_override(&ovr),
            Err(e) => {
                tracing::warn!(error = %e, "chronology_config: repo override parse failed; ignoring");
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
    fn default_is_visible_right_sane_width() {
        let c = ChronologyConfig::default();
        assert!(c.visible);
        assert_eq!(c.side, Side::Right);
        assert_eq!(c.width.percent, 32);
        assert!(c.width.min_cols <= c.width.max_cols);
    }

    #[test]
    fn override_merges_per_field() {
        let base = ChronologyConfig::default();
        let ovr = ChronologyOverride {
            visible: Some(false),
            side: Some(Side::Left),
            width: None,
        };
        let merged = base.with_override(&ovr);
        assert!(!merged.visible);
        assert_eq!(merged.side, Side::Left);
        assert_eq!(merged.width.percent, 32, "unspecified width inherits");
    }

    #[test]
    fn sanitize_clamps_and_swaps() {
        let mut c = ChronologyConfig::default();
        c.width.percent = 99;
        c.width.min_cols = 80;
        c.width.max_cols = 10;
        c.sanitize();
        assert!(c.width.percent <= 80);
        assert!(
            c.width.min_cols <= c.width.max_cols,
            "inverted min/max swapped"
        );
    }

    #[test]
    fn resolved_width_clamps_to_min_and_max() {
        let mut c = ChronologyConfig::default();
        c.width.percent = 50;
        c.width.min_cols = 20;
        c.width.max_cols = 30;
        assert_eq!(
            c.resolved_width(200),
            30,
            "50% of 200 = 100, clamped to max 30"
        );
        assert_eq!(
            c.resolved_width(20),
            20,
            "50% of 20 = 10, clamped to min 20"
        );
    }
}
