//! `wsx setup plasma` installer.
//!
//! Writes the bundled KDE Plasma 6 applet into the user's plasmoid directory
//! (`~/.local/share/plasma/plasmoids/<id>/`), which is where
//! `kpackagetool6 --install` would put it and where plasmashell looks for it
//! — so kpackagetool6 isn't needed. Re-running overwrites the package in
//! place, which also refreshes the wsx binary path baked into it.
//!
//! The applet drives existing commands rather than Plasma-specific ones:
//! `wsx waybar status` for the indicator (its text/class/tooltip payload is
//! already bar-agnostic), `wsx workspace list --json` for the popup, and
//! `wsx waybar jump` when a workspace is picked.

use std::path::Path;

use crate::desktop::install_support::{preferred_wsx_bin, shell_quote, write_atomic};
use crate::error::{Error, Result};

/// The applet's plugin id: its package directory name, and what
/// `plasmawindowed` takes. Must match `KPlugin.Id` in `metadata.json`.
pub(crate) const APPLET_ID: &str = "io.github.bakedbean.wsx";

/// The package metadata, embedded at compile time.
const METADATA_JSON: &str = include_str!("assets/metadata.json");
/// The applet UI, embedded at compile time; see [`main_qml`].
const MAIN_QML: &str = include_str!("assets/contents/ui/main.qml");

/// The applet UI with the wsx binary baked in. The shell-quoted path is
/// substituted as a JSON string literal, which QML reads as a JS string, so
/// no path can break out of it.
fn main_qml(wsx_bin: &str) -> String {
    MAIN_QML.replace(
        "__WSX_BIN__",
        &serde_json::Value::String(shell_quote(wsx_bin)).to_string(),
    )
}

/// Testable core of the installer: writes the applet package under
/// `plasmoids_dir` (normally `~/.local/share/plasma/plasmoids`).
pub fn install_into(plasmoids_dir: &Path, wsx_bin: &str) -> Result<Vec<String>> {
    let package = plasmoids_dir.join(APPLET_ID);
    let ui_dir = package.join("contents/ui");
    std::fs::create_dir_all(&ui_dir)?;
    write_atomic(&package.join("metadata.json"), METADATA_JSON)?;
    write_atomic(&ui_dir.join("main.qml"), &main_qml(wsx_bin))?;
    Ok(vec![format!(
        "installed Plasma applet: {}",
        package.display()
    )])
}

/// Resolves `~/.local/share/plasma/plasmoids` and the wsx binary (see
/// [`preferred_wsx_bin`]), then delegates to [`install_into`]. This is what
/// `wsx setup plasma` calls.
pub fn run() -> Result<Vec<String>> {
    let data_root = dirs::data_dir()
        .ok_or_else(|| Error::UserInput("could not resolve ~/.local/share".into()))?;
    let wsx_bin = preferred_wsx_bin(dirs::home_dir());
    let mut lines = install_into(&data_root.join("plasma/plasmoids"), &wsx_bin)?;
    lines.push(
        "add it to a panel: right-click the panel > Add or Manage Widgets, search for \"wsx\""
            .into(),
    );
    // plasmashell keeps an applet's QML loaded until it restarts, so an
    // applet already on a panel keeps running the old copy.
    lines.push(
        "already on a panel? restart plasmashell to load this version: \
         kquitapp6 plasmashell && kstart plasmashell"
            .into(),
    );
    Ok(lines)
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn metadata_id_matches_package_dir() {
        // plasmashell only loads a package whose directory name is its id.
        let v: serde_json::Value = serde_json::from_str(METADATA_JSON).unwrap();
        assert_eq!(v["KPlugin"]["Id"], APPLET_ID);
        assert_eq!(v["KPackageStructure"], "Plasma/Applet");
    }

    #[test]
    fn main_qml_bakes_in_quoted_wsx_bin() {
        let qml = main_qml("/opt/my tools/wsx");
        assert!(!qml.contains("__WSX_BIN__"), "{qml}");
        assert!(
            qml.contains(r#"property string wsx: "'/opt/my tools/wsx'""#),
            "{qml}"
        );
        // A double quote in the path is escaped, not a string terminator.
        let qml = main_qml("/opt/a\"b/wsx");
        assert!(
            qml.contains(r#"property string wsx: "'/opt/a\"b/wsx'""#),
            "{qml}"
        );
    }

    #[test]
    fn install_into_writes_package_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let report = install_into(dir.path(), "/usr/bin/wsx").unwrap();
        let package = dir.path().join(APPLET_ID);
        assert!(
            report[0].contains(&package.display().to_string()),
            "{report:?}"
        );
        assert_eq!(
            std::fs::read_to_string(package.join("metadata.json")).unwrap(),
            METADATA_JSON
        );
        let qml_path = package.join("contents/ui/main.qml");
        assert!(
            std::fs::read_to_string(&qml_path)
                .unwrap()
                .contains("wsx: \"/usr/bin/wsx\"")
        );
        // Re-run: overwrite in place (refreshing the baked path), not error.
        install_into(dir.path(), "/home/u/.local/bin/wsx").unwrap();
        let qml = std::fs::read_to_string(&qml_path).unwrap();
        assert!(qml.contains("wsx: \"/home/u/.local/bin/wsx\""), "{qml}");
        assert!(!qml.contains("/usr/bin/wsx"), "{qml}");
        // No temp litter anywhere in the package.
        for sub in [package.clone(), package.join("contents/ui")] {
            assert!(
                !std::fs::read_dir(&sub).unwrap().any(|e| e
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("wsx-tmp")),
                "{}",
                sub.display()
            );
        }
    }
}
