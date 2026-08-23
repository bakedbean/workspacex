//! `wsx setup waybar` installer.
//!
//! Bundles the wsx waybar module config/css and patches an existing
//! `~/.config/waybar/config.jsonc` to reference it. The patcher is
//! deliberately conservative: it only rewrites configs whose shape it
//! recognizes (an `{`-opening top-level object, no existing `"include"` key)
//! and otherwise falls back to printing paste-ready snippets rather than
//! risking a corrupted config.

use std::path::Path;

use crate::desktop::install_support::{preferred_wsx_bin, write_atomic};
use crate::error::{Error, Result};

/// The wsx waybar module definition, embedded at compile time.
const MODULE_JSONC: &str = include_str!("assets/wsx.jsonc");
/// The wsx waybar module stylesheet, embedded at compile time.
const MODULE_CSS: &str = include_str!("assets/wsx.css");
/// The elephant menu definition, embedded at compile time.
const MENU_LUA: &str = include_str!("assets/wsx.lua");
/// The wsx walker theme (widened, subtext visible), embedded at compile time.
const WALKER_THEME_LAYOUT: &str = include_str!("assets/walker-theme/layout.xml");
const WALKER_THEME_CSS: &str = include_str!("assets/walker-theme/style.css");
/// Item layout for the menus:wsx provider — carries the static Pango
/// attribute ranges that color the fixed-column fields (see waybar::entries).
const WALKER_THEME_ITEM: &str = include_str!("assets/walker-theme/item_menus-wsx.xml");

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

/// Write the elephant menu definition under `config_root` (normally
/// `~/.config`), substituting the shell-quoted wsx binary path. Creating the
/// directory is harmless when elephant isn't installed — the menu only
/// activates once `walker` is detected on PATH (see waybar::menu).
pub fn install_elephant_menu_into(config_root: &Path, wsx_bin: &str) -> Result<String> {
    let dir = config_root.join("elephant/menus");
    std::fs::create_dir_all(&dir)?;
    let quoted = shlex::try_quote(wsx_bin)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| wsx_bin.to_string());
    let path = dir.join("wsx.lua");
    std::fs::write(&path, MENU_LUA.replace("__WSX_BIN__", &quoted))?;
    Ok(format!("installed elephant menu: {}", path.display()))
}

/// Write the wsx walker theme under `config_root` (normally `~/.config`).
/// Omarchy's default walker theme hides the item subtext line (`font-size:
/// 0px`) and sizes the window for the app launcher, which crams every
/// workspace indicator onto one truncated line — this theme is the same look
/// with the subtext visible and a wider window. `waybar::menu` passes
/// `-t wsx` only when the theme is installed.
pub fn install_walker_theme_into(config_root: &Path) -> Result<String> {
    let dir = config_root.join("walker/themes/wsx");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("layout.xml"), WALKER_THEME_LAYOUT)?;
    std::fs::write(dir.join("style.css"), WALKER_THEME_CSS)?;
    std::fs::write(dir.join("item_menus-wsx.xml"), WALKER_THEME_ITEM)?;
    Ok(format!("installed walker theme: {}", dir.display()))
}

/// Elephant only hot-REGISTERS a freshly written menu file — its Lua doesn't
/// execute until the service restarts, so a new menu silently serves
/// "No Results" until then. Best-effort: `try-restart` is a no-op when
/// elephant isn't running, and any failure degrades to a printed hint.
fn restart_elephant() -> String {
    match std::process::Command::new("systemctl")
        .args(["--user", "try-restart", "elephant"])
        .status()
    {
        Ok(s) if s.success() => "restarted elephant (menu definitions load only on restart)".into(),
        _ => "restart elephant to load the menu: systemctl --user try-restart elephant".into(),
    }
}

/// Resolves `~/.config/waybar` and the current epoch, then delegates to
/// [`install_into`], then writes the elephant menu definition (see
/// [`install_elephant_menu_into`]) with the wsx binary path resolved by
/// [`preferred_wsx_bin`]. This is what `wsx setup waybar` calls.
pub fn run() -> Result<Vec<String>> {
    let config_root =
        dirs::config_dir().ok_or_else(|| Error::UserInput("could not resolve ~/.config".into()))?;
    let waybar_dir = config_root.join("waybar");
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut lines = install_into(&waybar_dir, epoch)?;
    let wsx_bin = preferred_wsx_bin(dirs::home_dir());
    match install_elephant_menu_into(&config_root, &wsx_bin) {
        Ok(line) => {
            lines.push(line);
            lines.push(restart_elephant());
        }
        Err(e) => lines.push(format!("elephant menu skipped: {e}")),
    }
    match install_walker_theme_into(&config_root) {
        Ok(line) => {
            lines.push(line);
            // Walker scans theme files once at service startup; a running
            // walker service keeps rendering the old theme until restarted.
            lines.push(
                "restart walker to reload the wsx theme: omarchy-restart-walker (or pkill walker)"
                    .into(),
            );
        }
        Err(e) => lines.push(format!("walker theme skipped: {e}")),
    }
    Ok(lines)
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

    #[test]
    fn walker_theme_installs_wide_layout_with_visible_subtext() {
        let tmp = tempfile::tempdir().unwrap();
        let line = install_walker_theme_into(tmp.path()).unwrap();
        let dir = tmp.path().join("walker/themes/wsx");
        assert!(dir.join("layout.xml").exists(), "{line}");
        let layout = std::fs::read_to_string(dir.join("layout.xml")).unwrap();
        assert!(
            layout.contains("<property name=\"width-request\">1000</property>"),
            "widened window: {layout:.100}"
        );
        assert!(
            layout.contains("<property name=\"max-content-width\">960</property>"),
            "widened scroll area: {layout:.100}"
        );
        // Walker hides the "Waiting for elephant..." hint only on the window
        // of the theme active at connect time (the config default, not wsx),
        // so a non-default theme must ship the hint pre-hidden. Walker still
        // shows it explicitly if elephant actually disconnects.
        let hint = layout
            .split_once("id=\"ElephantHint\"")
            .map(|(_, rest)| rest.split("</object>").next().unwrap())
            .expect("layout has an ElephantHint label");
        assert!(
            hint.contains("<property name=\"visible\">false</property>"),
            "ElephantHint must start hidden: {hint}"
        );
        // The provider item layout carries the field-coloring attributes.
        let item = std::fs::read_to_string(dir.join("item_menus-wsx.xml")).unwrap();
        assert!(item.contains("<attributes>"), "{item:.200}");
        let css = std::fs::read_to_string(dir.join("style.css")).unwrap();
        // The whole point of the theme: subtext must NOT be zeroed out.
        assert!(css.contains(".item-subtext"), "{css:.200}");
        assert!(
            !css.contains("font-size: 0px"),
            "subtext hidden: {css:.200}"
        );
        // Re-install overwrites without error (setup is re-runnable).
        install_walker_theme_into(tmp.path()).unwrap();
    }

    #[test]
    fn elephant_menu_installs_with_quoted_binary_path() {
        let tmp = tempfile::tempdir().unwrap();
        let line = install_elephant_menu_into(tmp.path(), "/opt/my tools/wsx").unwrap();
        let lua_path = tmp.path().join("elephant/menus/wsx.lua");
        assert!(lua_path.exists(), "{line}");
        let lua = std::fs::read_to_string(&lua_path).unwrap();
        assert!(lua.contains("'/opt/my tools/wsx'"), "{lua}");
        assert!(lua.contains("waybar menu-entries --json"), "{lua}");
        assert!(lua.contains("function GetEntries()"), "{lua}");
        assert!(!lua.contains("__WSX_BIN__"), "{lua}");
        // Re-install overwrites without error (setup is re-runnable).
        install_elephant_menu_into(tmp.path(), "/usr/bin/wsx").unwrap();
        let lua = std::fs::read_to_string(&lua_path).unwrap();
        assert!(lua.contains("/usr/bin/wsx"), "{lua}");
    }
}
