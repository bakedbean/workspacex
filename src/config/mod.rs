//! App settings and filesystem layout.
//!
//! This module owns the `Dirs` state/db/log path layout; the
//! `detail_bar_config` submodule resolves the workspace detail-bar display
//! config from global + per-repo JSON.

pub mod chronology_source;
pub mod detail_bar_config;
pub mod usage_window;

use std::path::PathBuf;

#[cfg(test)]
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Dirs {
    state_root: PathBuf,
}

impl Dirs {
    pub fn discover() -> Self {
        let state_root = dirs::state_dir()
            .or_else(|| dirs::home_dir().map(|h| h.join(".local/state")))
            .unwrap_or_else(|| PathBuf::from("."));
        Self { state_root }
    }

    #[cfg(test)]
    pub fn for_test(root: impl AsRef<Path>) -> Self {
        Self {
            state_root: root.as_ref().to_path_buf(),
        }
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
    pub fn pm_dir(&self) -> PathBuf {
        self.app_dir().join("project-manager")
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
}
