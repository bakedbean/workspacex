//! Reload `theme.toml` while wsx runs: a fingerprint (mtime + length)
//! check once a second on the housekeeping tick, last-good specs kept on
//! error, and a short footer notice naming the first problem.

use super::App;
use crate::data::store::Store;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Ticks between fingerprint checks: 8 × 125 ms = 1 s. (`App.tick` is `u32`.)
const CHECK_EVERY_TICKS: u32 = 8;
/// How long the footer shows a theme error.
const NOTICE_MS: u64 = 5_000;

fn fingerprint(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The `bar_theme` setting: the opt-in for `theme.toml`. Default OFF, so a
/// stock install draws the bundled bars and never reads the file; `on` /
/// `true` / `1` / `yes` enable it. Read on the once-a-second check so
/// `wsx config set bar_theme on` takes effect without a restart.
pub fn bar_theme_enabled(store: &Store) -> bool {
    matches!(
        store
            .get_setting("bar_theme")
            .ok()
            .flatten()
            .as_deref()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Some("on" | "true" | "1" | "yes")
    )
}

impl App {
    /// Point the app at its theme file and apply it now if `bar_theme` is on.
    pub fn set_theme_path(&mut self, path: PathBuf, now_ms: u64) {
        self.theme_path = Some(path);
        self.theme_active = false;
        self.theme_fingerprint = None;
        self.sync_theme(now_ms);
    }

    /// Called every tick; once a second, re-reads the `bar_theme` setting
    /// and the file fingerprint, and reloads only when either changed.
    pub fn maybe_reload_theme(&mut self, now_ms: u64) {
        if self.tick % CHECK_EVERY_TICKS != 0 {
            return;
        }
        self.sync_theme(now_ms);
    }

    /// Bring `bar_specs` in line with the setting and the file: off means the
    /// bundled default (and a cleared notice); on means the file, reloaded
    /// when it appears, disappears, or changes, or when the setting was just
    /// turned on.
    fn sync_theme(&mut self, now_ms: u64) {
        let Some(path) = self.theme_path.clone() else {
            return;
        };
        if !bar_theme_enabled(&self.store) {
            if self.theme_active {
                self.theme_active = false;
                self.theme_fingerprint = None;
                self.bar_specs = crate::config::theme_file::bundled_default(&self.theme);
                self.theme_notice = None;
                tracing::info!("bar_theme off; drawing the bundled bars");
            }
            return;
        }
        let fp = fingerprint(&path);
        if self.theme_active && fp == self.theme_fingerprint {
            return;
        }
        self.theme_active = true;
        self.theme_fingerprint = fp;
        self.reload_theme(now_ms);
    }

    /// Load the file unconditionally. Errors keep the last good specs.
    pub fn reload_theme(&mut self, now_ms: u64) {
        let Some(path) = self.theme_path.clone() else {
            return;
        };
        match crate::config::theme_file::load(&path, &self.theme) {
            Ok(specs) => {
                self.bar_specs = specs;
                self.theme_notice = None;
                tracing::info!(path = %path.display(), "theme.toml loaded");
            }
            Err(errors) => {
                for e in &errors {
                    tracing::warn!(path = %path.display(), "theme.toml: {e}");
                }
                let first = &errors[0];
                let msg = if errors.len() > 1 {
                    format!("theme.toml: {first} (+{} more)", errors.len() - 1)
                } else {
                    format!("theme.toml: {first}")
                };
                self.theme_notice = Some((msg, now_ms + NOTICE_MS));
            }
        }
    }

    /// The notice to show in the footer, if one is still live.
    pub fn theme_notice(&self, now_ms: u64) -> Option<&str> {
        self.theme_notice
            .as_ref()
            .filter(|(_, until)| now_ms < *until)
            .map(|(m, _)| m.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::Store;
    use crate::ui::bar::format;

    /// An app with `bar_theme = on`, which every reload test assumes.
    fn app() -> App {
        let app = App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-theme-reload-test"),
        )
        .unwrap();
        app.store.set_setting("bar_theme", "on").unwrap();
        app
    }

    #[test]
    fn bar_theme_setting_defaults_to_off() {
        let store = Store::open_in_memory().unwrap();
        assert!(!bar_theme_enabled(&store));
        for v in ["on", "true", "1", "yes", " ON "] {
            store.set_setting("bar_theme", v).unwrap();
            assert!(bar_theme_enabled(&store), "{v:?}");
        }
        for v in ["off", "false", "0", "no", "banana"] {
            store.set_setting("bar_theme", v).unwrap();
            assert!(!bar_theme_enabled(&store), "{v:?}");
        }
    }

    #[test]
    fn theme_file_is_ignored_until_bar_theme_is_on_and_dropped_when_off_again() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version\"\n").unwrap();
        let mut app = App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-theme-reload-test"),
        )
        .unwrap();

        // Default off: the file exists but the stock bars stay.
        app.set_theme_path(path.clone(), 0);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$keys").unwrap(),
            "off by default"
        );
        assert!(app.theme_notice(0).is_none());

        // Turned on from the CLI while running: picked up on the next check.
        app.store.set_setting("bar_theme", "on").unwrap();
        app.tick = 8;
        app.maybe_reload_theme(0);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$version").unwrap(),
            "file honored once on"
        );

        // A broken edit while on sets the notice…
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$nope\"\n").unwrap();
        app.tick = 16;
        app.maybe_reload_theme(1_000);
        assert!(app.theme_notice(1_000).is_some());

        // …and turning off snaps back to the stock bars and clears it.
        app.store.set_setting("bar_theme", "off").unwrap();
        app.tick = 24;
        app.maybe_reload_theme(2_000);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$keys").unwrap(),
            "stock bars when off"
        );
        assert!(app.theme_notice(2_000).is_none(), "notice cleared when off");

        // Back on: reloaded even though the fingerprint never changed while off.
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$usage\"\n").unwrap();
        app.store.set_setting("bar_theme", "on").unwrap();
        app.tick = 32;
        app.maybe_reload_theme(3_000);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$usage").unwrap()
        );
    }

    #[test]
    fn missing_file_is_the_bundled_default_without_notice() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = app();
        app.set_theme_path(dir.path().join("theme.toml"), 0);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$keys").unwrap()
        );
        assert!(app.theme_notice(0).is_none());
    }

    #[test]
    fn invalid_edit_keeps_last_good_and_sets_a_timed_notice() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version\"\n").unwrap();
        let mut app = app();
        app.set_theme_path(path.clone(), 0);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$version").unwrap()
        );

        std::fs::write(&path, "[dashboard_footer]\nformat = \"$nope\"\n").unwrap();
        app.reload_theme(1_000);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$version").unwrap(),
            "last good kept"
        );
        let notice = app.theme_notice(1_000).expect("notice set");
        assert!(
            notice.starts_with("theme.toml: [dashboard_footer].format"),
            "{notice}"
        );
        assert!(notice.contains("nope"), "{notice}");
        assert!(app.theme_notice(5_999).is_some());
        assert!(app.theme_notice(6_000).is_none(), "expires after 5 s");

        std::fs::write(&path, "[dashboard_footer]\nformat = \"$usage\"\n").unwrap();
        app.reload_theme(7_000);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$usage").unwrap()
        );
        assert!(
            app.theme_notice(7_000).is_none(),
            "fixed file clears the notice"
        );
    }

    #[test]
    fn tick_check_reloads_only_on_a_changed_fingerprint_once_a_second() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("theme.toml");
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version\"\n").unwrap();
        let mut app = app();
        app.set_theme_path(path.clone(), 0);

        // Different length guarantees a different fingerprint even within
        // the same mtime granularity.
        std::fs::write(&path, "[dashboard_footer]\nformat = \"$version  $usage\"\n").unwrap();
        app.tick = 3;
        app.maybe_reload_theme(0);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$version").unwrap(),
            "off-tick: no check"
        );
        app.tick = 8;
        app.maybe_reload_theme(0);
        assert_eq!(
            app.bar_specs.dashboard_footer.format,
            format::parse("$version  $usage").unwrap()
        );
    }
}
