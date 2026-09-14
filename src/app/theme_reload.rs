//! Reload `theme.toml` while wsx runs: a fingerprint (mtime + length)
//! check once a second on the housekeeping tick, last-good specs kept on
//! error, and a short footer notice naming the first problem.

use super::App;
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

impl App {
    /// Point the app at its theme file and load it now.
    pub fn set_theme_path(&mut self, path: PathBuf, now_ms: u64) {
        self.theme_fingerprint = fingerprint(&path);
        self.theme_path = Some(path);
        self.reload_theme(now_ms);
    }

    /// Called every tick; does the fingerprint check once a second and
    /// reloads only when the file changed (or appeared / disappeared).
    pub fn maybe_reload_theme(&mut self, now_ms: u64) {
        if self.tick % CHECK_EVERY_TICKS != 0 {
            return;
        }
        let Some(path) = self.theme_path.as_deref() else {
            return;
        };
        let fp = fingerprint(path);
        if fp == self.theme_fingerprint {
            return;
        }
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

    fn app() -> App {
        App::new(
            Store::open_in_memory().unwrap(),
            PathBuf::from("/tmp/wsx-theme-reload-test"),
        )
        .unwrap()
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
