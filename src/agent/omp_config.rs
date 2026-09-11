//! Per-launch config overlay for oh-my-pi (`omp`).
//!
//! omp 18 turned off its Claude *user-level* discovery providers by default
//! (`skills.enableClaudeUser`, `commands.enableClaudeUser`; both were on in
//! 17.x). With them off, omp never scans `~/.claude/skills` or
//! `~/.claude/commands`, so the skills `wsx setup install-skill` writes there
//! and the user's pinned slash commands silently vanish from every omp session
//! (`read skill://wsx` → "Unknown skill: wsx").
//!
//! Rather than editing the user's persistent `~/.omp/agent/config.yml`, wsx
//! writes a small overlay file under its own state dir and passes it as
//! `omp --config <path>` on every spawn. omp applies the overlay for that run
//! only, so the user's own config stays untouched and the fix travels with
//! wsx.

use crate::agent::skill::install_content_to;
use crate::error::Result;
use std::path::{Path, PathBuf};

/// File name of the overlay under the wsx app dir.
pub const OVERLAY_FILE_NAME: &str = "omp-config.yml";

/// The overlay content. Only the settings wsx relies on are set; everything
/// else falls through to the user's `config.yml` and omp's defaults.
pub const OVERLAY_CONTENT: &str = "\
# Written by wsx before every omp launch and passed as `omp --config <file>`.
# omp >= 18 disables its Claude user-level discovery providers by default,
# which hides the skills `wsx setup install-skill` writes to ~/.claude/skills
# and the pinned commands in ~/.claude/commands. This overlay re-enables them
# for wsx-spawned sessions only. Do not edit: wsx rewrites it on drift.
skills:
  enableClaudeUser: true
commands:
  enableClaudeUser: true
";

/// Where the overlay lives: `<app_dir>/omp-config.yml`.
pub fn overlay_path(app_dir: &Path) -> PathBuf {
    app_dir.join(OVERLAY_FILE_NAME)
}

/// Write the overlay under `app_dir` if missing or drifted, returning its
/// path. Idempotent and atomic (temp file + rename).
pub fn ensure_overlay(app_dir: &Path) -> Result<PathBuf> {
    let path = overlay_path(app_dir);
    install_content_to(&path, OVERLAY_CONTENT)?;
    Ok(path)
}

/// `ensure_overlay` against the discovered wsx app dir. Best-effort: on any
/// failure logs a warning and returns `None`, so a read-only state dir
/// degrades to "omp without the overlay" rather than a failed spawn.
pub fn ensure_overlay_default() -> Option<PathBuf> {
    let app_dir = crate::config::Dirs::discover().app_dir();
    match ensure_overlay(&app_dir) {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::warn!(
                "could not write omp config overlay under {}: {e}; \
                 ~/.claude skills and commands may be invisible to omp",
                app_dir.display()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::skill::InstallOutcome;
    use tempfile::TempDir;

    #[test]
    fn overlay_enables_claude_user_skills_and_commands() {
        // Line-oriented check rather than a YAML parse: the crate has no YAML
        // dependency, and omp's overlay format is plain nested keys.
        let lines: Vec<&str> = OVERLAY_CONTENT.lines().collect();
        let skills = lines
            .iter()
            .position(|l| *l == "skills:")
            .expect("skills: section");
        assert_eq!(lines[skills + 1], "  enableClaudeUser: true");
        let commands = lines
            .iter()
            .position(|l| *l == "commands:")
            .expect("commands: section");
        assert_eq!(lines[commands + 1], "  enableClaudeUser: true");
        // Nothing else is forced on the user: only the two keys wsx needs.
        assert_eq!(
            lines
                .iter()
                .filter(|l| !l.starts_with('#') && l.contains("enable"))
                .count(),
            2,
            "overlay must set exactly the two toggles wsx relies on:\n{OVERLAY_CONTENT}"
        );
    }

    #[test]
    fn ensure_overlay_creates_then_reports_unchanged() {
        let tmp = TempDir::new().unwrap();
        let app_dir = tmp.path().join("wsx");
        let path = ensure_overlay(&app_dir).unwrap();
        assert_eq!(path, app_dir.join(OVERLAY_FILE_NAME));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), OVERLAY_CONTENT);
        assert_eq!(
            install_content_to(&path, OVERLAY_CONTENT).unwrap(),
            InstallOutcome::Unchanged,
            "second write must be a no-op"
        );
    }

    /// The spawn path treats a failed write as "launch omp without the
    /// overlay", so the writer must surface the error rather than panic.
    #[test]
    fn ensure_overlay_fails_when_app_dir_is_not_a_directory() {
        let tmp = TempDir::new().unwrap();
        let app_dir = tmp.path().join("wsx");
        std::fs::write(&app_dir, "not a directory").unwrap();
        assert!(
            ensure_overlay(&app_dir).is_err(),
            "a regular file where the app dir should be must fail"
        );
        assert!(!overlay_path(&app_dir).exists());
    }

    #[test]
    fn ensure_overlay_rewrites_a_drifted_file() {
        let tmp = TempDir::new().unwrap();
        let app_dir = tmp.path().join("wsx");
        std::fs::create_dir_all(&app_dir).unwrap();
        let path = overlay_path(&app_dir);
        std::fs::write(&path, "skills:\n  enableClaudeUser: false\n").unwrap();
        ensure_overlay(&app_dir).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), OVERLAY_CONTENT);
    }
}
