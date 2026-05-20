//! Embedded Claude Code skill and installer.
//!
//! The skill teaches Claude Code to drive the `wsx` CLI (workspace
//! operations, slug-vs-branch naming, cross-repo orchestration). It's
//! bundled into the binary at compile time so `wsx setup install-skill`
//! can write it to `~/.claude/skills/wsx/SKILL.md` on any machine where
//! wsx is installed.

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// The wsx skill content, embedded at compile time from `skills/wsx/SKILL.md`.
pub const SKILL_CONTENT: &str = include_str!("../skills/wsx/SKILL.md");

/// Default install location for the wsx skill (`~/.claude/skills/wsx/SKILL.md`).
/// Returns `None` if the home directory can't be resolved.
pub fn default_install_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join(".claude")
            .join("skills")
            .join("wsx")
            .join("SKILL.md")
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// Wrote the skill to a new location.
    Created,
    /// Existing file content already matched; no write performed.
    Unchanged,
    /// Overwrote an existing file whose content differed.
    Updated,
}

/// Install the embedded skill to `target`. Creates parent directories as
/// needed. If `target` already contains identical content, no write is
/// performed and `Unchanged` is returned (so users can re-run safely and
/// see no false "updated" output).
pub fn install_to(target: &Path) -> Result<InstallOutcome> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let outcome = match std::fs::read_to_string(target) {
        Ok(existing) if existing == SKILL_CONTENT => return Ok(InstallOutcome::Unchanged),
        Ok(_) => InstallOutcome::Updated,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => InstallOutcome::Created,
        Err(e) => return Err(Error::Io(e)),
    };
    std::fs::write(target, SKILL_CONTENT)?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn skill_content_has_frontmatter() {
        assert!(
            SKILL_CONTENT.starts_with("---\n"),
            "skill missing YAML frontmatter"
        );
        assert!(
            SKILL_CONTENT.contains("name: wsx"),
            "skill frontmatter missing name field"
        );
    }

    #[test]
    fn install_creates_when_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("deep").join("nested").join("SKILL.md");
        assert_eq!(install_to(&target).unwrap(), InstallOutcome::Created);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), SKILL_CONTENT);
    }

    #[test]
    fn install_is_idempotent_on_identical_content() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("SKILL.md");
        install_to(&target).unwrap();
        // Second install of identical content should report Unchanged
        // without rewriting.
        assert_eq!(install_to(&target).unwrap(), InstallOutcome::Unchanged);
    }

    #[test]
    fn install_overwrites_when_content_differs() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("SKILL.md");
        std::fs::write(&target, "stale content").unwrap();
        assert_eq!(install_to(&target).unwrap(), InstallOutcome::Updated);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), SKILL_CONTENT);
    }
}
