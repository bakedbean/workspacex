//! `wsx setup waybar` installer.
//!
//! Bundles the wsx waybar module config/css and patches an existing
//! `~/.config/waybar/config.jsonc` to reference it. The patcher is
//! deliberately conservative: it only rewrites configs whose shape it
//! recognizes (an `{`-opening top-level object, no existing `"include"` key)
//! and otherwise falls back to printing paste-ready snippets rather than
//! risking a corrupted config.

use std::path::Path;

use crate::error::{Error, Result};

/// The wsx waybar module definition, embedded at compile time.
const MODULE_JSONC: &str = include_str!("assets/wsx.jsonc");
/// The wsx waybar module stylesheet, embedded at compile time.
const MODULE_CSS: &str = include_str!("assets/wsx.css");

/// Result of attempting to patch a `config.jsonc` text in place.
pub enum PatchOutcome {
    /// The config was recognized and patched; contains the new text.
    Patched(String),
    /// `custom/wsx` is already referenced — nothing to do.
    AlreadyInstalled,
    /// The config's shape wasn't recognized (or looked risky to edit
    /// automatically, e.g. an existing `"include"` key) — caller should fall
    /// back to printing manual-install snippets.
    Unrecognized,
}

/// Leading whitespace of `line`, for matching indentation when inserting.
fn leading_ws(line: &str) -> String {
    line.chars().take_while(|c| c.is_whitespace()).collect()
}

/// Insert `"custom/wsx",` as the FIRST entry of the multi-line array whose
/// key line starts with `key`. Returns false if the key isn't found.
fn prepend_to_array(lines: &mut Vec<String>, key: &str) -> bool {
    let Some(open) = lines
        .iter()
        .position(|l| l.trim_start().starts_with(key) && l.contains('['))
    else {
        return false;
    };
    let entry_indent = format!("{}  ", leading_ws(&lines[open]));
    lines.insert(open + 1, format!("{entry_indent}\"custom/wsx\","));
    true
}

/// Insert `"custom/wsx",` as the LAST entry of the multi-line array whose key
/// line starts with `key`. Returns false if the key or its closing bracket
/// isn't found (single-line arrays are deliberately not handled — the caller
/// falls back to snippets).
fn append_to_array(lines: &mut Vec<String>, key: &str) -> bool {
    let Some(open) = lines
        .iter()
        .position(|l| l.trim_start().starts_with(key) && l.contains('['))
    else {
        return false;
    };
    let entry_indent = format!("{}  ", leading_ws(&lines[open]));
    for i in open + 1..lines.len() {
        if lines[i].trim_start().starts_with(']') {
            lines.insert(i, format!("{entry_indent}\"custom/wsx\","));
            return true;
        }
    }
    false
}

/// Text-based jsonc patcher: adds a top-level `"include"` for the wsx module
/// file and inserts `"custom/wsx",` as the FIRST entry of `modules-right`
/// (falling back to the last entry of `modules-left`), so the indicator sits
/// at the leading edge of the bar's right-side group.
pub fn patch_config(text: &str, include_path: &str) -> PatchOutcome {
    if text.contains("custom/wsx") {
        return PatchOutcome::AlreadyInstalled;
    }
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();

    // 1. include: only handle the no-include case; an existing include array
    //    is left alone (snippets instead) rather than risking a bad edit.
    if lines
        .iter()
        .any(|l| l.trim_start().starts_with("\"include\""))
    {
        return PatchOutcome::Unrecognized;
    }
    let Some(open) = lines.iter().position(|l| l.trim() == "{") else {
        return PatchOutcome::Unrecognized;
    };
    lines.insert(open + 1, format!("  \"include\": [\"{include_path}\"],"));

    // 2. module entry: first entry of modules-right so the indicator leads
    //    the bar's right-side group, else last of modules-left.
    let placed = prepend_to_array(&mut lines, "\"modules-right\"")
        || append_to_array(&mut lines, "\"modules-left\"");
    if !placed {
        return PatchOutcome::Unrecognized;
    }
    PatchOutcome::Patched(lines.join("\n") + "\n")
}

/// Write `content` to `path` atomically: write to a sibling temp file, then
/// rename over `path`. The temp file is removed if the rename fails.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
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

/// Paste-ready manual-install instructions for configs the patcher couldn't
/// (or shouldn't) touch automatically.
fn snippet_report(include_path: &str) -> Vec<String> {
    vec![
        "could not patch config.jsonc automatically — add manually:".into(),
        format!("  1. top-level: \"include\": [\"{include_path}\"],"),
        "  2. first entry of modules-right (or last of -left): \"custom/wsx\",".into(),
    ]
}

/// Testable core of the installer: writes the bundled module assets into
/// `waybar_dir` and attempts to patch `config.jsonc` in place, using `epoch`
/// to name the pre-patch backup file.
pub fn install_into(waybar_dir: &Path, epoch: u64) -> Result<Vec<String>> {
    std::fs::create_dir_all(waybar_dir)?;
    let module_path = waybar_dir.join("wsx.jsonc");
    write_atomic(&module_path, MODULE_JSONC)?;
    write_atomic(&waybar_dir.join("wsx.css"), MODULE_CSS)?;
    let mut report = vec![
        format!("wrote {}", module_path.display()),
        format!("wrote {}", waybar_dir.join("wsx.css").display()),
    ];
    let include_path = module_path.display().to_string();
    let config = waybar_dir.join("config.jsonc");
    match std::fs::read_to_string(&config) {
        Ok(text) => match patch_config(&text, &include_path) {
            PatchOutcome::Patched(new_text) => {
                let backup = waybar_dir.join(format!("config.jsonc.bak.{epoch}"));
                std::fs::copy(&config, &backup)?;
                write_atomic(&config, &new_text)?;
                report.push(format!(
                    "patched {} (backup: {})",
                    config.display(),
                    backup.display()
                ));
            }
            PatchOutcome::AlreadyInstalled => {
                report.push("config.jsonc already references custom/wsx".into());
            }
            PatchOutcome::Unrecognized => report.extend(snippet_report(&include_path)),
        },
        Err(_) => report.extend(snippet_report(&include_path)),
    }
    report.push("add to style.css (after existing @import lines): @import \"wsx.css\";".into());
    report.push("reload waybar: omarchy-restart-waybar (or pkill -SIGUSR2 waybar)".into());
    Ok(report)
}

/// Resolves `~/.config/waybar` and the current epoch, then delegates to
/// [`install_into`]. This is what `wsx setup waybar` calls.
pub fn run() -> Result<Vec<String>> {
    let waybar_dir = dirs::config_dir()
        .ok_or_else(|| Error::UserInput("could not resolve ~/.config".into()))?
        .join("waybar");
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    install_into(&waybar_dir, epoch)
}

#[cfg(test)]
mod install_tests {
    use super::*;

    // Mirrors the user-facing omarchy layout: modules-left with custom/omarchy,
    // a modules-right array, plus a module-definition key that must be ignored.
    const OMARCHY_STYLE: &str = r#"{
  "reload_style_on_change": true,
  "modules-left": [
    "custom/omarchy",
    "hyprland/workspaces#main",
  ],
  "modules-right": [
    "cpu",
    "battery",
  ],
  "custom/omarchy": {
    "format": "x"
  }
}
"#;

    #[test]
    fn patches_as_first_entry_of_modules_right() {
        let PatchOutcome::Patched(out) =
            patch_config(OMARCHY_STYLE, "/home/u/.config/waybar/wsx.jsonc")
        else {
            panic!("expected Patched");
        };
        let wsx_entry = out.find("\"custom/wsx\",").unwrap();
        assert!(wsx_entry > out.find("\"modules-right\"").unwrap());
        assert!(wsx_entry < out.find("\"cpu\",").unwrap());
        // modules-left is untouched
        assert!(out.find("\"custom/wsx\",") == out.rfind("\"custom/wsx\","));
        assert!(out.contains(r#""include": ["/home/u/.config/waybar/wsx.jsonc"],"#));
    }

    #[test]
    fn falls_back_to_last_of_modules_left_without_modules_right() {
        let cfg = "{\n  \"modules-left\": [\n    \"clock\",\n  ],\n}\n";
        let PatchOutcome::Patched(out) = patch_config(cfg, "/x/wsx.jsonc") else {
            panic!("expected Patched");
        };
        let wsx = out.find("custom/wsx").unwrap();
        assert!(wsx > out.find("clock").unwrap());
    }

    #[test]
    fn already_installed_and_unrecognized() {
        let done = OMARCHY_STYLE.replace(
            "\"custom/omarchy\",",
            "\"custom/omarchy\",\n    \"custom/wsx\",",
        );
        assert!(matches!(
            patch_config(&done, "/x"),
            PatchOutcome::AlreadyInstalled
        ));
        assert!(matches!(
            patch_config("not even close", "/x"),
            PatchOutcome::Unrecognized
        ));
        // existing include array → bail to snippets rather than risk a bad edit
        let with_include = OMARCHY_STYLE.replacen('{', "{\n  \"include\": [\"other.jsonc\"],", 1);
        assert!(matches!(
            patch_config(&with_include, "/x"),
            PatchOutcome::Unrecognized
        ));
    }

    #[test]
    fn install_into_writes_files_backs_up_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.jsonc"), OMARCHY_STYLE).unwrap();
        let report = install_into(dir.path(), 1234).unwrap();
        assert!(dir.path().join("wsx.jsonc").exists());
        assert!(dir.path().join("wsx.css").exists());
        assert!(dir.path().join("config.jsonc.bak.1234").exists());
        let cfg = std::fs::read_to_string(dir.path().join("config.jsonc")).unwrap();
        assert!(cfg.contains("custom/wsx"));
        assert!(report.iter().any(|l| l.contains("patched")));
        // second run: no new backup, reports already-installed
        let report2 = install_into(dir.path(), 5678).unwrap();
        assert!(!dir.path().join("config.jsonc.bak.5678").exists());
        assert!(report2.iter().any(|l| l.contains("already")));
        // no temp litter
        assert!(
            !std::fs::read_dir(dir.path()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("wsx-tmp"))
        );
    }

    #[test]
    fn missing_config_prints_snippets() {
        let dir = tempfile::tempdir().unwrap();
        let report = install_into(dir.path(), 1).unwrap();
        assert!(dir.path().join("wsx.jsonc").exists());
        assert!(
            report.iter().any(|l| l.contains("custom/wsx")),
            "snippet with module name expected"
        );
    }
}
