//! Shared helpers for the `wsx setup waybar` and `wsx setup menubar`
//! installers.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Write `content` to `path` atomically: write to a sibling temp file, then
/// rename over `path`. The temp file is removed if the rename fails.
pub(crate) fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_file_name(format!(
        "{}.wsx-tmp.{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::Io(e)
    })
}

/// Picks the wsx binary path baked into the installed plugin/menu
/// (waybar's elephant menu, SwiftBar's plugin shim).
///
/// Dev builds (`cargo run`, `target/debug/wsx`, …) live in paths that vanish
/// the moment the build directory is cleaned or the branch is switched — if
/// `wsx setup waybar`/`wsx setup menubar` ran from one of those, the baked
/// path silently stops resolving and the menu/plugin shows nothing useful
/// forever with no obvious cause. `~/.local/bin/wsx` is the stable install
/// target every documented install path uses, so prefer it whenever it's
/// actually present, falling back to `current_exe()` (today's behavior) and
/// finally the bare "wsx" literal (resolved via PATH at invocation time) if
/// neither is available.
///
/// Takes `home` as a parameter (rather than calling `dirs::home_dir()`
/// directly) so tests can point it at a tempdir.
pub(crate) fn preferred_wsx_bin(home: Option<PathBuf>) -> String {
    if let Some(candidate) = home.map(|h| h.join(".local/bin/wsx"))
        && candidate.is_file()
    {
        return candidate.display().to_string();
    }
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "wsx".into())
}

#[cfg(test)]
mod install_common_tests {
    use super::*;

    #[test]
    fn preferred_wsx_bin_prefers_installed_path_when_present() {
        let home = tempfile::tempdir().unwrap();
        let bin_dir = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let bin_path = bin_dir.join("wsx");
        std::fs::write(&bin_path, "").unwrap();

        let resolved = preferred_wsx_bin(Some(home.path().to_path_buf()));
        assert_eq!(resolved, bin_path.display().to_string());
    }

    #[test]
    fn preferred_wsx_bin_falls_back_when_installed_path_missing() {
        let home = tempfile::tempdir().unwrap();
        // No .local/bin/wsx under this "home" — must fall back to
        // current_exe() (or the "wsx" literal), never a nonexistent path.
        let resolved = preferred_wsx_bin(Some(home.path().to_path_buf()));
        assert!(
            !resolved.starts_with(&home.path().join(".local/bin/wsx").display().to_string()),
            "{resolved}"
        );
    }
}
