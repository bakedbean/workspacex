//! The global settings catalog: every key `wsx config` accepts, with the
//! description, default, and value kind the settings drawer shows.
//!
//! The CLI's key check and the dashboard drawer both read [`SETTINGS`], so a
//! new key added here shows up in both.

use crate::error::{Error, Result};

/// How a setting's value is edited in the drawer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingKind {
    /// `on`/`off`; Enter flips it.
    Toggle { default_on: bool },
    /// One of a fixed list; Enter steps to the next. The first entry is the default.
    Choice(&'static [&'static str]),
    /// Free text, edited in `$EDITOR` with this file extension.
    Text { ext: &'static str },
}

#[derive(Clone, Copy, Debug)]
pub struct SettingDef {
    pub key: &'static str,
    pub group: &'static str,
    /// What an unset key means, for display only.
    pub default: &'static str,
    pub kind: SettingKind,
    pub description: &'static str,
}

const fn toggle(
    key: &'static str,
    group: &'static str,
    default_on: bool,
    description: &'static str,
) -> SettingDef {
    SettingDef {
        key,
        group,
        default: if default_on { "on" } else { "off" },
        kind: SettingKind::Toggle { default_on },
        description,
    }
}

const fn choice(
    key: &'static str,
    group: &'static str,
    values: &'static [&'static str],
    description: &'static str,
) -> SettingDef {
    SettingDef {
        key,
        group,
        default: values[0],
        kind: SettingKind::Choice(values),
        description,
    }
}

const fn text(
    key: &'static str,
    group: &'static str,
    default: &'static str,
    ext: &'static str,
    description: &'static str,
) -> SettingDef {
    SettingDef {
        key,
        group,
        default,
        kind: SettingKind::Text { ext },
        description,
    }
}

const BELLS: &[&str] = &["single", "double", "triple", "off"];

/// Grouped like the settings page in the book (`configuration/settings.md`).
pub const SETTINGS: &[SettingDef] = &[
    // General
    text(
        "branch_prefix",
        "General",
        "",
        "txt",
        "Names new branches <prefix>/<workspace>; unset uses the bare workspace name.",
    ),
    choice(
        "coding_agent",
        "General",
        &["claude", "pi", "hermes", "codex", "omp"],
        "Agent started in new workspaces.",
    ),
    // Agent sessions
    text(
        "custom_instructions",
        "Agent sessions",
        "",
        "md",
        "Text appended to the agent's system prompt on every spawn.",
    ),
    text(
        "process_doctrine",
        "Agent sessions",
        "built-in",
        "md",
        "Replaces the working doctrine injected into every session; off/none disables it.",
    ),
    text(
        "process_doctrine_extra",
        "Agent sessions",
        "",
        "md",
        "Extra \"- ...\" bullet lines appended to the doctrine.",
    ),
    toggle(
        "mcp_mirror",
        "Agent sessions",
        true,
        "Copy the source repo's MCP servers into each new worktree.",
    ),
    // Editor and tools
    text(
        "editor_cmd",
        "Editor and tools",
        "$VISUAL, then $EDITOR",
        "txt",
        "Command for `e`; the worktree path is appended unless it contains {path}.",
    ),
    text(
        "terminal_cmd",
        "Editor and tools",
        "$TERMINAL",
        "txt",
        "Command for `t`, run in the worktree; {cmd} enables desktop-widget jumps.",
    ),
    text(
        "diff_cmd",
        "Editor and tools",
        "",
        "txt",
        "Command for `v`; accepts {path} and {base}.",
    ),
    text(
        "lazygit_cmd",
        "Editor and tools",
        "",
        "txt",
        "Command for `g`, run in the worktree; must open its own window.",
    ),
    // Notifications
    toggle(
        "notifications",
        "Notifications",
        true,
        "Master switch for the terminal bell and the ! attention marker.",
    ),
    SettingDef {
        default: "double",
        ..choice(
            "notification_bell_question",
            "Notifications",
            BELLS,
            "Bell when the agent asks you a question.",
        )
    },
    choice(
        "notification_bell_complete",
        "Notifications",
        BELLS,
        "Bell when the agent finishes a turn.",
    ),
    choice(
        "notification_bell_permission",
        "Notifications",
        BELLS,
        "Bell when the agent waits on a permission prompt.",
    ),
    SettingDef {
        default: "triple",
        ..choice(
            "notification_bell_stalled",
            "Notifications",
            BELLS,
            "Bell when the agent stalls.",
        )
    },
    // Dashboard
    choice(
        "dashboard_sort_mode",
        "Dashboard",
        &["recency", "status"],
        "Workspace order within a repo; `o` toggles it too.",
    ),
    text(
        "dashboard_blocked_pin_max_age_secs",
        "Dashboard",
        "86400",
        "txt",
        "Seconds a workspace blocked on you stays pinned on top in recency mode.",
    ),
    text(
        "top_bar_show_tags",
        "Dashboard",
        "",
        "txt",
        "Tags that keep a workspace in the attached top bar despite tag hide rules. Restart to apply.",
    ),
    text(
        "dashboard_branch_width",
        "Dashboard",
        "28",
        "txt",
        "Width of the branch column, clamped to 10–80.",
    ),
    text(
        "dashboard_pr_width",
        "Dashboard",
        "16",
        "txt",
        "Width of the PR column, clamped to 8–24.",
    ),
    text(
        "detail_bar_config",
        "Dashboard",
        "built-in",
        "json",
        "JSON layout for the detail pane; see the Detail bar docs for the schema.",
    ),
    choice(
        "usage_graph_window",
        "Dashboard",
        &["24h", "1w", "1mo"],
        "Window of the $usage activity graph (shown when a theme places it).",
    ),
    text(
        "pinned_commands",
        "Dashboard",
        "",
        "txt",
        "Label=command lines shown as chips in the attached view.",
    ),
    text(
        "prompt_tags",
        "Dashboard",
        "",
        "txt",
        "name=uses lines, maintained by the prompt-tag picker.",
    ),
    // Appearance
    choice(
        "theme",
        "Appearance",
        &["wsx", "default", "dracula", "jellybeans", "nord"],
        "Base palette; default uses terminal ANSI colors. Restart to apply.",
    ),
    toggle(
        "bar_theme",
        "Appearance",
        false,
        "Read bar layout and styling from ~/.config/wsx/theme.toml.",
    ),
    toggle(
        "nerd_fonts",
        "Appearance",
        true,
        "Use Nerd Font glyphs on the dashboard.",
    ),
    // Remote
    toggle(
        "remote_control",
        "Remote",
        true,
        "Start claude with --remote-control so claude.ai/code and mobile can reach it.",
    ),
    toggle(
        "remote_control_sandbox",
        "Remote",
        false,
        "Also pass --sandbox when remote_control is on.",
    ),
    text(
        "remotes",
        "Remote",
        "",
        "txt",
        "name=command lines run by `wsx remote <name>`.",
    ),
    text(
        "shared_hosts",
        "Remote",
        "",
        "txt",
        "name=ssh-destination lines for browsing workspaces on other machines.",
    ),
];

pub fn find(key: &str) -> Option<&'static SettingDef> {
    SETTINGS.iter().find(|d| d.key == key)
}

pub fn is_known(key: &str) -> bool {
    find(key).is_some()
}

/// Whether a stored on/off value (or its absence) reads as on.
pub fn toggle_is_on(value: Option<&str>, default_on: bool) -> bool {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("on" | "true" | "1" | "yes") => true,
        Some("off" | "false" | "0" | "no") => false,
        _ => default_on,
    }
}

impl SettingDef {
    /// The value Enter stores for a toggle or choice, given the stored one;
    /// `None` for text keys, which go to `$EDITOR`.
    pub fn next_value(&self, current: Option<&str>) -> Option<String> {
        match self.kind {
            SettingKind::Toggle { default_on } => {
                let on = toggle_is_on(current, default_on);
                Some(if on { "off" } else { "on" }.to_string())
            }
            SettingKind::Choice(values) => {
                let current = current
                    .map(|v| v.trim().to_ascii_lowercase())
                    .unwrap_or_else(|| self.default.to_string());
                let i = values.iter().position(|v| *v == current);
                Some(values[i.map_or(0, |i| (i + 1) % values.len())].to_string())
            }
            SettingKind::Text { .. } => None,
        }
    }
}

/// Validate and normalize a non-empty value before it is stored. Shared by
/// `wsx config set`/`edit` and the settings drawer.
pub fn normalize(key: &str, value: &str) -> Result<String> {
    if key == "detail_bar_config" {
        detail_bar_config_validate_and_normalize(value)
    } else if key == "usage_graph_window" {
        usage_window_validate_and_normalize(value)
    } else if key.starts_with("notification_bell_") {
        bell_pattern_validate_and_normalize(value)
    } else {
        Ok(value.to_string())
    }
}

/// Seed text for the editor when the global `detail_bar_config`
/// setting is empty — the pretty-printed default config.
pub fn detail_bar_config_seed_for_empty() -> String {
    serde_json::to_string_pretty(&super::detail_bar_config::DetailBarConfig::default())
        .unwrap_or_else(|_| "{}".to_string())
}

/// Parse, sanitize, and re-serialize a global `detail_bar_config`
/// blob. Returns the pretty-printed normalized JSON.
pub fn detail_bar_config_validate_and_normalize(raw: &str) -> Result<String> {
    let mut cfg: super::detail_bar_config::DetailBarConfig = serde_json::from_str(raw)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: invalid JSON: {e}")))?;
    cfg.sanitize();
    serde_json::to_string_pretty(&cfg)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: serialize failed: {e}")))
}

/// Validate a `notification_bell_*` pattern and lower-case it. `app::bell`
/// falls back to the state default on anything it does not recognise, so
/// this is the only place a typo like `dobule` gets reported.
pub fn bell_pattern_validate_and_normalize(raw: &str) -> Result<String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "off" | "false" | "0" | "single" | "double" | "triple" => Ok(trimmed),
        _ => Err(Error::UserInput(format!(
            "notification_bell_*: expected one of off, single, double, triple (got {trimmed:?})"
        ))),
    }
}

/// Validate a `usage_graph_window` value: accept only the canonical tokens
/// (`24h`/`1w`/`1mo`), ignoring surrounding whitespace, and store the trimmed
/// canonical form. Rejects anything else so a CLI typo fails loudly instead of
/// silently falling back to `24h` at render time.
pub fn usage_window_validate_and_normalize(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if super::usage_window::UsageWindow::ALL
        .iter()
        .any(|w| w.as_setting() == trimmed)
    {
        Ok(trimmed.to_string())
    } else {
        Err(Error::UserInput(format!(
            "usage_graph_window: expected one of 24h, 1w, 1mo (got {trimmed:?})"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<_> = SETTINGS.iter().map(|d| d.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), SETTINGS.len());
    }

    #[test]
    fn every_key_has_a_description() {
        for d in SETTINGS {
            assert!(!d.description.is_empty(), "{} has no description", d.key);
        }
    }

    #[test]
    fn toggle_flips_from_default_when_unset() {
        let d = find("nerd_fonts").unwrap();
        assert_eq!(d.next_value(None).as_deref(), Some("off"));
        assert_eq!(d.next_value(Some("off")).as_deref(), Some("on"));
        let d = find("bar_theme").unwrap();
        assert_eq!(d.next_value(None).as_deref(), Some("on"));
        assert_eq!(d.next_value(Some("yes")).as_deref(), Some("off"));
    }

    #[test]
    fn choice_steps_from_stored_or_default_and_wraps() {
        let d = find("notification_bell_question").unwrap();
        // Unset means the documented default, `double`.
        assert_eq!(d.next_value(None).as_deref(), Some("triple"));
        assert_eq!(d.next_value(Some("off")).as_deref(), Some("single"));
        assert_eq!(d.next_value(Some("garbage")).as_deref(), Some("single"));
        assert_eq!(find("editor_cmd").unwrap().next_value(None), None);
    }

    #[test]
    fn every_choice_value_passes_normalize() {
        for d in SETTINGS {
            if let SettingKind::Choice(values) = d.kind {
                for v in values {
                    assert_eq!(normalize(d.key, v).unwrap(), *v, "{}={v}", d.key);
                }
            }
        }
    }
}
