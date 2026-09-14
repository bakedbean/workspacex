//! App settings and filesystem layout.
//!
//! This module owns the `Dirs` state/db/log path layout. The
//! `detail_bar_config` submodule resolves the detail-bar display config from
//! global + per-repo JSON.

pub mod detail_bar_config;
pub mod name_color;
pub mod usage_window;

use std::path::PathBuf;

#[cfg(test)]
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Dirs {
    state_root: PathBuf,
    /// Parent of the `wsx/` config directory: `$XDG_CONFIG_HOME` when set
    /// and absolute, else `~/.config`. Same on every platform, like
    /// starship, so `theme.toml` lives in one predictable place.
    config_root: PathBuf,
}

impl Dirs {
    pub fn discover() -> Self {
        // Honor an explicit `XDG_STATE_HOME` on every platform. `dirs::state_dir()`
        // only reads it on Linux; on macOS it returns `None`, which sent a sandboxed
        // run (sandbox/bootstrap.sh) straight into the real `~/.local/state` db.
        let state_root = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(dirs::state_dir)
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("."));
        let config_root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            state_root,
            config_root,
        }
    }

    #[cfg(test)]
    pub fn for_test(root: impl AsRef<Path>) -> Self {
        Self {
            state_root: root.as_ref().to_path_buf(),
            config_root: root.as_ref().join("config"),
        }
    }

    /// `<config_root>/wsx` — user-editable config files (currently just
    /// `theme.toml`). Distinct from `app_dir`, which holds state (db, logs).
    pub fn config_dir(&self) -> PathBuf {
        self.config_root.join("wsx")
    }
    /// `~/.config/wsx/theme.toml` — the bar theme file.
    pub fn theme_path(&self) -> PathBuf {
        self.config_dir().join("theme.toml")
    }

    pub fn app_dir(&self) -> PathBuf {
        self.state_root.join("wsx")
    }
    pub fn db_path(&self) -> PathBuf {
        self.app_dir().join("state.db")
    }
    pub fn log_dir(&self) -> PathBuf {
        self.app_dir().join("logs")
    }
    /// Per-workspace context digests written by `wsx context write`:
    /// `<app_dir>/context/<repo>/<workspace>.md`.
    pub fn context_dir(&self) -> PathBuf {
        self.app_dir().join("context")
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(self.log_dir())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_path_under_state_dir() {
        let dirs = Dirs::for_test("/tmp/wsx-test-home");
        assert_eq!(
            dirs.db_path(),
            std::path::PathBuf::from("/tmp/wsx-test-home/wsx/state.db")
        );
        assert_eq!(
            dirs.log_dir(),
            std::path::PathBuf::from("/tmp/wsx-test-home/wsx/logs")
        );
    }

    #[test]
    fn theme_path_under_config_dir() {
        let dirs = Dirs::for_test("/tmp/wsx-test-home");
        assert_eq!(
            dirs.config_dir(),
            std::path::PathBuf::from("/tmp/wsx-test-home/config/wsx")
        );
        assert_eq!(
            dirs.theme_path(),
            std::path::PathBuf::from("/tmp/wsx-test-home/config/wsx/theme.toml")
        );
    }
}
