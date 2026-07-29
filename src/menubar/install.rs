//! `wsx setup menubar` installer: writes the SwiftBar plugin shim into the
//! SwiftBar plugin directory (resolved from SwiftBar's defaults domain)
//! and asks SwiftBar to reload. Conservative like the waybar installer:
//! when the directory can't be resolved, print instructions, never guess.

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::install_common::{preferred_wsx_bin, write_atomic};

/// Filename encodes SwiftBar's refresh interval.
pub(crate) const SHIM_NAME: &str = "wsx-menubar.10s.sh";

fn shim(wsx_bin: &str) -> String {
    let quoted = shlex::try_quote(wsx_bin)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| wsx_bin.to_string());
    format!(
        "#!/bin/sh\n# Installed by `wsx setup menubar`. Re-run it after moving wsx.\nexec {quoted} menubar plugin\n"
    )
}

/// `defaults read com.ameba.SwiftBar PluginDirectory` output → path.
/// Handles trailing newline and a leading `~`.
pub(crate) fn parse_plugin_dir(defaults_stdout: &str, home: Option<&Path>) -> Option<PathBuf> {
    let raw = defaults_stdout.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rest) = raw.strip_prefix("~/") {
        return home.map(|h| h.join(rest));
    }
    Some(PathBuf::from(raw))
}

fn plugin_dir() -> Option<PathBuf> {
    let out = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "com.ameba.SwiftBar", "PluginDirectory"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_plugin_dir(
        &String::from_utf8_lossy(&out.stdout),
        dirs::home_dir().as_deref(),
    )
}

pub fn install_into(dir: &Path, wsx_bin: &str) -> Result<Vec<String>> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(SHIM_NAME);
    write_atomic(&path, &shim(wsx_bin))?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    Ok(vec![format!("wrote {}", path.display())])
}

pub fn run() -> Result<Vec<String>> {
    let wsx_bin = preferred_wsx_bin(dirs::home_dir());
    match plugin_dir() {
        Some(dir) => {
            let mut lines = install_into(&dir, &wsx_bin)?;
            // Best-effort hot reload; -g keeps SwiftBar in the background.
            let reloaded = std::process::Command::new("/usr/bin/open")
                .args(["-g", "swiftbar://refreshallplugins"])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            lines.push(if reloaded {
                "asked SwiftBar to reload plugins".into()
            } else {
                "reload SwiftBar plugins: open -g 'swiftbar://refreshallplugins'".into()
            });
            Ok(lines)
        }
        None => Ok(vec![
            "SwiftBar not configured (no PluginDirectory in its defaults domain)".into(),
            "install it: brew install swiftbar — then launch it once and pick a plugin folder"
                .into(),
            format!("then re-run: wsx setup menubar (installs {SHIM_NAME} into that folder)"),
        ]),
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_plugin_dir_trims_and_expands_tilde() {
        let home = Path::new("/Users/u");
        assert_eq!(
            parse_plugin_dir("/Users/u/SwiftBar\n", Some(home)),
            Some("/Users/u/SwiftBar".into())
        );
        assert_eq!(
            parse_plugin_dir("~/Library/SwiftBar\n", Some(home)),
            Some("/Users/u/Library/SwiftBar".into())
        );
        assert_eq!(parse_plugin_dir("", Some(home)), None);
        assert_eq!(parse_plugin_dir("   \n", Some(home)), None);
    }

    #[test]
    fn shim_execs_quoted_wsx_plugin() {
        let s = shim("/opt/my tools/wsx");
        assert!(s.starts_with("#!/bin/sh\n"), "{s}");
        assert!(s.contains("exec '/opt/my tools/wsx' menubar plugin"), "{s}");
    }

    #[test]
    fn install_into_writes_executable_shim_idempotently() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        install_into(dir.path(), "/usr/local/bin/wsx").unwrap();
        // Re-run: overwrite, not error (refreshes the baked path).
        install_into(dir.path(), "/usr/local/bin/wsx2").unwrap();
        let path = dir.path().join(SHIM_NAME);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("/usr/local/bin/wsx2"), "{content}");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o755, 0o755, "shim must be executable");
    }
}
