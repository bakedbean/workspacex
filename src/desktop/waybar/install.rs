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

/// True when `css` has a live `@import` of the module stylesheet itself —
/// not one inside a comment (waybar wouldn't load it) and not a different
/// file that merely ends the same way (`old-wsx.css`).
fn imports_module_css(css: &str) -> bool {
    css_imports(css).iter().any(|stmt| {
        import_target(stmt).is_some_and(|(s, e)| stmt[s..e].rsplit('/').next() == Some("wsx.css"))
    })
}

/// `css` with every `/* ... */` comment removed, leaving quoted strings
/// intact (a `/*` inside a string is text, not a comment opener). An
/// unterminated comment swallows the rest of the input, as in a browser.
fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    let mut quote: Option<char> = None;
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                out.push(c);
                if c == '\\' {
                    out.extend(chars.next());
                } else if c == q {
                    quote = None;
                }
            }
            None if c == '/' && chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for c in chars.by_ref() {
                    if prev == '*' && c == '/' {
                        break;
                    }
                    prev = c;
                }
                // Keep tokens on either side of the comment apart.
                out.push(' ');
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                }
                out.push(c);
            }
        }
    }
    out
}

/// The live `@import` statements in `css`, one per line, trimmed. Comments
/// are stripped first so a commented-out import is never mistaken for (or
/// resurrected as) a live one.
fn css_imports(css: &str) -> Vec<String> {
    strip_css_comments(css)
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("@import"))
        .map(str::to_string)
        .collect()
}

/// Byte span of the path inside an `@import` statement: the first quoted
/// string (`@import "x";`, `@import url('x');`) or a bare `url(x)`.
fn import_target(stmt: &str) -> Option<(usize, usize)> {
    if let Some(start) = stmt.find(['"', '\'']) {
        let quote = stmt[start..].chars().next()?;
        let len = stmt[start + 1..].find(quote)?;
        return Some((start + 1, start + 1 + len));
    }
    let start = stmt.find("url(")? + 4;
    let len = stmt[start..].find(')')?;
    Some((start, start + len))
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
    let styled = std::fs::read_to_string(waybar_dir.join("style.css"))
        .is_ok_and(|css| imports_module_css(&css));
    if styled {
        report.push("style.css already imports wsx.css".into());
    } else {
        report.push("add to style.css (after existing @import lines): @import \"wsx.css\";".into());
    }
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
    let (palette, source) = palette_imports(config_root);
    let css = WALKER_THEME_CSS.replace("__PALETTE_IMPORT__", &palette);
    std::fs::write(dir.join("style.css"), css)?;
    std::fs::write(dir.join("item_menus-wsx.xml"), WALKER_THEME_ITEM)?;
    Ok(format!(
        "installed walker theme: {} (palette: {source})",
        dir.display()
    ))
}

/// Palette used when the user's active walker theme can't be read: the one
/// omarchy's own walker themes import (relative to `themes/wsx/`).
const OMARCHY_PALETTE_IMPORT: &str = "@import \"../../../omarchy/current/theme/walker.css\";";
/// Where [`OMARCHY_PALETTE_IMPORT`] lands, relative to `~/.config`.
const OMARCHY_PALETTE_PATH: &str = "omarchy/current/theme/walker.css";

/// Last resort when neither the active theme nor omarchy supplies a palette:
/// define every color the wsx theme references, so GTK never drops a rule
/// (an undefined color left the whole window transparent). The accents in
/// the theme are catppuccin-mocha, so the neutrals are too.
const BUILTIN_PALETTE: &str = "@define-color base #1e1e2e;
@define-color background #1e1e2e;
@define-color border #585b70;
@define-color text #cdd6f4;
@define-color selected-text #f5e0dc;";

/// The palette lines that give the wsx theme its colors, plus a short label
/// for the setup report naming where they came from. Preference order:
/// the `@import`s of the theme named in `walker/config.toml` (so the menu
/// matches the user's launcher on any distro), then omarchy's palette when
/// it exists, then [`BUILTIN_PALETTE`]. A borrowed theme is trusted to
/// define the usual walker color names (`@base`, `@text`, ...); re-run
/// setup after switching walker themes.
pub(crate) fn palette_imports(config_root: &Path) -> (String, String) {
    let themes = config_root.join("walker/themes");
    let theme = std::fs::read_to_string(config_root.join("walker/config.toml"))
        .ok()
        .and_then(|s| s.parse::<toml::Table>().ok())
        .and_then(|t| t.get("theme")?.as_str().map(str::to_string))
        // The wsx theme's imports are what we're computing; borrowing them
        // would be circular.
        .filter(|name| name != "wsx" && !name.contains('/'));
    if let Some(name) = theme {
        let imports: Vec<String> = std::fs::read_to_string(themes.join(&name).join("style.css"))
            .map(|css| {
                css_imports(&css)
                    .iter()
                    .map(|l| rebase_import(l, &name))
                    .collect()
            })
            .unwrap_or_default();
        if !imports.is_empty() {
            return (imports.join("\n"), format!("walker theme {name}"));
        }
    }
    if config_root.join(OMARCHY_PALETTE_PATH).is_file() {
        return (OMARCHY_PALETTE_IMPORT.to_string(), "omarchy".into());
    }
    (
        BUILTIN_PALETTE.to_string(),
        "built-in defaults; the active walker theme imports no palette".into(),
    )
}

/// Re-point a relative `@import` from `themes/<theme>/style.css` so it still
/// resolves from `themes/wsx/style.css`. The two files are siblings, so
/// `../`-relative, absolute, and URL imports carry over unchanged; only a
/// same-directory import (`colors.css`) needs the `../<theme>/` prefix.
fn rebase_import(line: &str, theme: &str) -> String {
    let Some((start, end)) = import_target(line) else {
        return line.to_string();
    };
    let path = line[start..end].trim();
    if path.starts_with("../") || path.starts_with('/') || path.contains("://") {
        return line.to_string();
    }
    let rebased = format!("../{theme}/{}", path.trim_start_matches("./"));
    format!("{}{rebased}{}", &line[..start], &line[end..])
}

/// Elephant only hot-REGISTERS a freshly written menu file — its Lua doesn't
/// execute until the service restarts, so a new menu silently serves
/// "No Results" until then. Best-effort: omarchy runs elephant as a systemd
/// user unit, but other setups (e.g. Hyprland `exec-once`) run a bare
/// process with no unit to restart — that one is replaced in place. When
/// elephant isn't running at all there is nothing to reload.
fn restart_elephant() -> String {
    use std::process::{Command, Stdio};
    let unit_restarted = Command::new("systemctl")
        .args(["--user", "try-restart", "elephant"])
        // "Unit elephant.service not found" is the expected answer off
        // omarchy; don't leak it into the setup report.
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if unit_restarted {
        return "restarted elephant (menu definitions load only on restart)".into();
    }
    let running = Command::new("pgrep")
        .args(["-x", "elephant"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !running {
        return "elephant is not running; the menu loads when it next starts".into();
    }
    let _ = Command::new("pkill").args(["-x", "elephant"]).status();
    // Own process group so the daemon outlives this terminal session.
    use std::os::unix::process::CommandExt;
    match Command::new("elephant")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    {
        Ok(_) => "restarted elephant (menu definitions load only on restart)".into(),
        Err(_) => "restart elephant to load the menu: setsid -f elephant".into(),
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
                "restart walker to reload the wsx theme: omarchy-restart-walker, or \
                 pkill -x walker && setsid -f walker --gapplication-service"
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
    fn style_css_hint_only_when_import_missing() {
        let dir = tempfile::tempdir().unwrap();
        let hint = |report: &[String]| report.iter().any(|l| l.starts_with("add to style.css"));
        // No style.css, and one with only a commented-out import: hint.
        assert!(hint(&install_into(dir.path(), 1).unwrap()));
        std::fs::write(
            dir.path().join("style.css"),
            "@import \"theme.css\";\n/* @import \"wsx.css\"; */\n",
        )
        .unwrap();
        assert!(hint(&install_into(dir.path(), 2).unwrap()));
        // Live import: no hint, report says so instead.
        std::fs::write(
            dir.path().join("style.css"),
            "@import \"theme.css\";\n  @import \"wsx.css\";\n",
        )
        .unwrap();
        let report = install_into(dir.path(), 3).unwrap();
        assert!(!hint(&report), "{report:?}");
        assert!(
            report
                .iter()
                .any(|l| l == "style.css already imports wsx.css")
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

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn walker_theme_borrows_the_active_themes_palette() {
        // A non-omarchy setup (fedora-hypr): the omarchy palette path doesn't
        // exist, so hardcoding it left every color undefined and the window
        // fully transparent.
        let tmp = tempfile::tempdir().unwrap();
        write(
            &tmp.path().join("walker/config.toml"),
            "force_keyboard_focus = true\ntheme = \"fh-default\" # comment\n",
        );
        write(
            &tmp.path().join("walker/themes/fh-default/style.css"),
            "@import \"../../../fedora-hypr/current/theme/walker.css\";\n\
             @import 'colors.css';\n\n* {\n  all: unset;\n}\n",
        );
        install_walker_theme_into(tmp.path()).unwrap();
        let css = std::fs::read_to_string(tmp.path().join("walker/themes/wsx/style.css")).unwrap();
        assert!(
            css.contains("@import \"../../../fedora-hypr/current/theme/walker.css\";"),
            "{css:.600}"
        );
        // Same-directory import re-pointed at the source theme's dir.
        assert!(
            css.contains("@import '../fh-default/colors.css';"),
            "{css:.600}"
        );
        assert!(!css.contains("omarchy/current"), "{css:.600}");
        assert!(!css.contains("__PALETTE_IMPORT__"), "{css:.600}");
    }

    #[test]
    fn walker_theme_palette_fallbacks() {
        let tmp = tempfile::tempdir().unwrap();
        let palette = |root: &Path| palette_imports(root).0;
        // No config and no omarchy: built-in colors, never an import of a
        // file that doesn't exist.
        assert_eq!(palette(tmp.path()), BUILTIN_PALETTE);
        // Every color the theme references is defined by the fallback.
        for name in ["base", "background", "border", "text", "selected-text"] {
            assert!(
                WALKER_THEME_CSS.contains(&format!("@{name}")),
                "theme no longer uses @{name}; prune BUILTIN_PALETTE"
            );
            assert!(
                BUILTIN_PALETTE.contains(&format!("@define-color {name} ")),
                "BUILTIN_PALETTE missing {name}"
            );
        }
        // With omarchy's palette on disk, it wins over the built-in one.
        write(
            &tmp.path().join(OMARCHY_PALETTE_PATH),
            "@define-color base #000;\n",
        );
        assert_eq!(palette(tmp.path()), OMARCHY_PALETTE_IMPORT);
        // Config naming the wsx theme itself must not self-import.
        write(&tmp.path().join("walker/config.toml"), "theme = \"wsx\"\n");
        write(
            &tmp.path().join("walker/themes/wsx/style.css"),
            "@import \"stale.css\";\n",
        );
        assert_eq!(palette(tmp.path()), OMARCHY_PALETTE_IMPORT);
        // Theme without imports, and theme whose dir is missing.
        write(
            &tmp.path().join("walker/config.toml"),
            "theme = \"plain\"\n",
        );
        write(&tmp.path().join("walker/themes/plain/style.css"), "* {}\n");
        assert_eq!(palette(tmp.path()), OMARCHY_PALETTE_IMPORT);
        write(&tmp.path().join("walker/config.toml"), "theme = \"gone\"\n");
        assert_eq!(palette(tmp.path()), OMARCHY_PALETTE_IMPORT);
    }

    #[test]
    fn css_imports_ignore_comments_and_match_exact_targets() {
        // A multi-line comment hides imports on its inner lines; a `/*`
        // inside a string is not a comment opener.
        let css = "/*\n@import \"wsx.css\";\n*/\n\
                   @import \"a/*b.css\"; /* trailing */\n\
                   /* x */ @import url(c.css);\n";
        assert_eq!(
            css_imports(css),
            vec!["@import \"a/*b.css\";", "@import url(c.css);"]
        );
        assert!(!imports_module_css(css));
        // Exact file name, not a suffix match.
        assert!(!imports_module_css("@import \"old-wsx.css\";"));
        assert!(imports_module_css("@import url('./wsx.css');"));
        assert!(imports_module_css(
            "@import \"/home/u/.config/waybar/wsx.css\";"
        ));
    }

    #[test]
    fn palette_ignores_commented_out_imports() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("walker/config.toml"), "theme = \"t\"\n");
        write(
            &tmp.path().join("walker/themes/t/style.css"),
            "/*\n@import \"../old/palette.css\";\n*/\n@import \"../new/palette.css\";\n",
        );
        assert_eq!(
            palette_imports(tmp.path()),
            (
                "@import \"../new/palette.css\";".to_string(),
                "walker theme t".to_string()
            )
        );
    }

    #[test]
    fn rebase_import_leaves_sibling_safe_paths_alone() {
        for line in [
            "@import \"../../../x/walker.css\";",
            "@import \"/abs/walker.css\";",
            "@import url(\"file:///abs/walker.css\");",
        ] {
            assert_eq!(rebase_import(line, "t"), line);
        }
        assert_eq!(
            rebase_import("@import \"./c.css\";", "t"),
            "@import \"../t/c.css\";"
        );
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
