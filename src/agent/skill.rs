//! Embedded agent skill and installer.
//!
//! The skill teaches coding agents to drive the `wsx` CLI (workspace
//! operations, slug-vs-branch naming, cross-repo orchestration). It's
//! bundled into the binary at compile time so `wsx setup install-skill`
//! can write it to each supported agent's skill directory on any machine
//! where wsx is installed.

use crate::error::{Error, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The wsx skill content, embedded at compile time from `skills/wsx/SKILL.md`.
pub const SKILL_CONTENT: &str = include_str!("../../skills/wsx/SKILL.md");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    pub agent: &'static str,
    pub path: PathBuf,
}

/// Default Claude install location (`~/.claude/skills/wsx/SKILL.md`).
/// Returns `None` if the home directory can't be resolved.
pub fn default_claude_install_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join(".claude")
            .join("skills")
            .join("wsx")
            .join("SKILL.md")
    })
}

/// Default Codex install location (`~/.codex/skills/wsx/SKILL.md`).
/// Returns `None` if the home directory can't be resolved.
pub fn default_codex_install_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex").join("skills").join("wsx").join("SKILL.md"))
}

/// Default install location kept for older call sites.
pub fn default_install_path() -> Option<PathBuf> {
    default_claude_install_path()
}

/// Install targets for `wsx setup install-skill`.
///
/// Claude is always included because this command historically installs the
/// bundled Claude Code skill. Codex is included only when it appears to be
/// installed, either via `WSX_CODEX_BIN`, a `codex` executable on PATH, or an
/// existing `~/.codex` directory from a prior Codex run.
pub fn default_install_targets() -> Option<Vec<InstallTarget>> {
    let mut targets = vec![InstallTarget {
        agent: "Claude",
        path: default_claude_install_path()?,
    }];
    if codex_is_installed() {
        targets.push(InstallTarget {
            agent: "Codex",
            path: default_codex_install_path()?,
        });
    }
    Some(targets)
}

fn codex_is_installed() -> bool {
    std::env::var_os("WSX_CODEX_BIN").is_some()
        || binary_on_path("codex")
        || dirs::home_dir()
            .map(|h| h.join(".codex").exists())
            .unwrap_or(false)
}

fn binary_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file()
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
    write_atomic(target, SKILL_CONTENT)?;
    Ok(outcome)
}

/// Write `content` to `path` atomically: write to a unique temp file in
/// the same directory, fsync, then rename. Mirrors the pattern in
/// `src/mcp.rs` so interrupted writes can't leave a half-written skill.
fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    let pid = std::process::id();
    let tmp = parent.join(format!(".SKILL.md.wsx-tmp.{pid}.{}", rand::random::<u32>()));
    {
        let mut f = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Io(e));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EnvGuard;
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

    #[test]
    fn default_targets_include_claude_only_when_codex_is_absent() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");

        let targets = default_install_targets().unwrap();

        assert_eq!(
            targets,
            vec![InstallTarget {
                agent: "Claude",
                path: home
                    .path()
                    .join(".claude")
                    .join("skills")
                    .join("wsx")
                    .join("SKILL.md"),
            }]
        );
    }

    #[test]
    fn default_targets_include_codex_when_binary_is_on_path() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        let bin = TempDir::new().unwrap();
        std::fs::write(bin.path().join("codex"), "").unwrap();
        env.set("HOME", home.path());
        env.set("PATH", bin.path());
        env.remove("WSX_CODEX_BIN");

        let targets = default_install_targets().unwrap();

        assert!(targets.iter().any(|t| {
            t.agent == "Codex"
                && t.path
                    == home
                        .path()
                        .join(".codex")
                        .join("skills")
                        .join("wsx")
                        .join("SKILL.md")
        }));
    }

    #[test]
    fn default_targets_include_codex_when_codex_home_exists() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        std::fs::create_dir(home.path().join(".codex")).unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");

        let targets = default_install_targets().unwrap();

        assert!(targets.iter().any(|t| t.agent == "Codex"));
    }
}
