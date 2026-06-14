//! Embedded agent skills and installer.
//!
//! Skills are bundled into the binary at compile time (see `BUNDLED_SKILLS`)
//! so `wsx setup install-skill` can write each one to every supported agent's
//! skill directory on any machine where wsx is installed. Currently: the `wsx`
//! skill (drives the `wsx` CLI — workspace operations, slug-vs-branch naming,
//! cross-repo orchestration) and the `agent-pr` skill (spawns a peer review
//! agent for the current branch).

use crate::error::{Error, Result};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The wsx skill content, embedded at compile time from `skills/wsx/SKILL.md`.
/// Retained as a named const for tests and any direct call sites.
pub const SKILL_CONTENT: &str = include_str!("../../skills/wsx/SKILL.md");

/// The agent-pr skill content, embedded from `skills/agent-pr/SKILL.md`.
pub const AGENT_PR_SKILL_CONTENT: &str = include_str!("../../skills/agent-pr/SKILL.md");

/// A skill bundled into the binary and installed by `wsx setup install-skill`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundledSkill {
    /// Directory name under each agent's `skills/` dir (`<dir>/<name>/SKILL.md`).
    pub name: &'static str,
    /// Markdown content embedded at compile time.
    pub content: &'static str,
}

/// Every skill wsx ships. Installed for each detected agent.
pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        name: "wsx",
        content: SKILL_CONTENT,
    },
    BundledSkill {
        name: "agent-pr",
        content: AGENT_PR_SKILL_CONTENT,
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallTarget {
    /// Display name of the agent (`"Claude"`, `"Codex"`, `"Hermes"`).
    pub agent: &'static str,
    /// The bundled skill's directory name (`"wsx"`, `"agent-pr"`).
    pub skill: &'static str,
    /// The content to write for this skill.
    pub content: &'static str,
    /// Destination file (`<skills-dir>/<skill>/SKILL.md`).
    pub path: PathBuf,
}

/// Claude's skills directory (`~/.claude/skills`). `None` if no home dir.
pub fn claude_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("skills"))
}

/// Codex's skills directory (`~/.codex/skills`). `None` if no home dir.
pub fn codex_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codex").join("skills"))
}

/// Hermes's skills directory (`~/.hermes/skills`). `None` if no home dir.
pub fn hermes_skills_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".hermes").join("skills"))
}

/// Install targets for `wsx setup install-skill`: every bundled skill, for
/// every detected agent.
///
/// Claude is always included. Codex is included when `WSX_CODEX_BIN` is set,
/// `codex` is on PATH, or `~/.codex` exists. Hermes is included when
/// `WSX_HERMES_BIN` is set, `hermes` is on PATH, or `~/.hermes` exists.
///
/// There is intentionally no separate Pi target: Pi loads skills from
/// `~/.claude/skills` (the same reason Pi, like Claude, receives the
/// superpowers-skills doctrine clause and Codex does not — see
/// `agent::doctrine`), so the Claude target already covers it.
pub fn default_install_targets() -> Option<Vec<InstallTarget>> {
    let mut agents: Vec<(&'static str, PathBuf)> = vec![("Claude", claude_skills_dir()?)];
    if codex_is_installed() {
        agents.push(("Codex", codex_skills_dir()?));
    }
    if hermes_is_installed() {
        agents.push(("Hermes", hermes_skills_dir()?));
    }
    let mut targets = Vec::new();
    for (agent, dir) in agents {
        for skill in BUNDLED_SKILLS {
            targets.push(InstallTarget {
                agent,
                skill: skill.name,
                content: skill.content,
                path: dir.join(skill.name).join("SKILL.md"),
            });
        }
    }
    Some(targets)
}

fn codex_is_installed() -> bool {
    std::env::var_os("WSX_CODEX_BIN").is_some()
        || binary_on_path("codex")
        || dirs::home_dir()
            .map(|h| h.join(".codex").is_dir())
            .unwrap_or(false)
}

fn hermes_is_installed() -> bool {
    std::env::var_os("WSX_HERMES_BIN").is_some()
        || binary_on_path("hermes")
        || dirs::home_dir()
            .map(|h| h.join(".hermes").is_dir())
            .unwrap_or(false)
}

fn binary_on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join(name);
        candidate.is_file() && is_executable(&candidate)
    })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
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

/// Install one bundled skill (`target.content`) to `target.path`. Creates
/// parent directories as needed. Returns `Unchanged` without writing when the
/// file already holds identical content (safe to re-run).
pub fn install_to(target: &InstallTarget) -> Result<InstallOutcome> {
    install_content_to(&target.path, target.content)
}

/// Write `content` to `path`, creating parent dirs and reporting
/// Created/Updated/Unchanged. Atomic: writes a temp file then renames. Used by
/// `install_to` and directly by tests.
fn install_content_to(path: &Path, content: &str) -> Result<InstallOutcome> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let outcome = match std::fs::read_to_string(path) {
        Ok(existing) if existing == content => return Ok(InstallOutcome::Unchanged),
        Ok(_) => InstallOutcome::Updated,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => InstallOutcome::Created,
        Err(e) => return Err(Error::Io(e)),
    };
    write_atomic(path, content)?;
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
        assert!(
            SKILL_CONTENT.contains("description:"),
            "skill frontmatter missing description field (needed for discovery)"
        );
    }

    #[test]
    fn agent_pr_skill_has_frontmatter() {
        assert!(
            AGENT_PR_SKILL_CONTENT.starts_with("---\n"),
            "agent-pr skill missing YAML frontmatter"
        );
        assert!(
            AGENT_PR_SKILL_CONTENT.contains("name: agent-pr"),
            "agent-pr skill frontmatter missing name field"
        );
        assert!(
            AGENT_PR_SKILL_CONTENT.contains("description:"),
            "agent-pr skill frontmatter missing description field (needed for discovery)"
        );
    }

    #[test]
    fn install_to_writes_the_targets_own_content() {
        // The public wrapper must write `target.content`, not a hardcoded
        // skill — a regression to the wsx content would otherwise go unnoticed.
        let tmp = TempDir::new().unwrap();
        let target = InstallTarget {
            agent: "Claude",
            skill: "agent-pr",
            content: AGENT_PR_SKILL_CONTENT,
            path: tmp.path().join("agent-pr").join("SKILL.md"),
        };
        assert_eq!(install_to(&target).unwrap(), InstallOutcome::Created);
        assert_eq!(
            std::fs::read_to_string(&target.path).unwrap(),
            AGENT_PR_SKILL_CONTENT
        );
        assert_ne!(AGENT_PR_SKILL_CONTENT, SKILL_CONTENT);
    }

    #[test]
    fn install_creates_when_missing() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("deep").join("nested").join("SKILL.md");
        assert_eq!(
            install_content_to(&target, SKILL_CONTENT).unwrap(),
            InstallOutcome::Created
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), SKILL_CONTENT);
    }

    #[test]
    fn install_is_idempotent_on_identical_content() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("SKILL.md");
        install_content_to(&target, SKILL_CONTENT).unwrap();
        assert_eq!(
            install_content_to(&target, SKILL_CONTENT).unwrap(),
            InstallOutcome::Unchanged
        );
    }

    #[test]
    fn install_overwrites_when_content_differs() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("SKILL.md");
        std::fs::write(&target, "stale content").unwrap();
        assert_eq!(
            install_content_to(&target, SKILL_CONTENT).unwrap(),
            InstallOutcome::Updated
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), SKILL_CONTENT);
    }

    #[test]
    fn default_targets_cover_every_bundled_skill_for_claude_only() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");
        env.remove("WSX_HERMES_BIN");

        let targets = default_install_targets().unwrap();

        // Only Claude is detected, but one target per bundled skill.
        assert_eq!(targets.len(), BUNDLED_SKILLS.len());
        assert!(targets.iter().all(|t| t.agent == "Claude"));
        let claude_skills = home.path().join(".claude").join("skills");
        assert!(targets.iter().any(|t| {
            t.skill == "wsx"
                && t.path == claude_skills.join("wsx").join("SKILL.md")
                && t.content == SKILL_CONTENT
        }));
        assert!(targets.iter().any(|t| {
            t.skill == "agent-pr"
                && t.path == claude_skills.join("agent-pr").join("SKILL.md")
                && t.content == AGENT_PR_SKILL_CONTENT
        }));
    }

    #[test]
    fn default_targets_include_codex_when_binary_is_on_path() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        let bin = TempDir::new().unwrap();
        let codex = bin.path().join("codex");
        std::fs::write(&codex, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&codex, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
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
        let codex_targets: Vec<_> = targets.iter().filter(|t| t.agent == "Codex").collect();
        assert_eq!(codex_targets.len(), BUNDLED_SKILLS.len());
        assert!(codex_targets.iter().any(|t| t.skill == "agent-pr"));
        assert!(codex_targets.iter().any(|t| t.skill == "wsx"));
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
        let codex_targets: Vec<_> = targets.iter().filter(|t| t.agent == "Codex").collect();
        assert_eq!(codex_targets.len(), BUNDLED_SKILLS.len());
        assert!(codex_targets.iter().any(|t| t.skill == "agent-pr"));
        assert!(codex_targets.iter().any(|t| t.skill == "wsx"));
    }

    #[test]
    fn default_targets_ignore_codex_home_regular_file() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        std::fs::write(home.path().join(".codex"), "").unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");

        let targets = default_install_targets().unwrap();

        assert!(!targets.iter().any(|t| t.agent == "Codex"));
    }

    #[test]
    fn default_targets_include_codex_when_codex_bin_env_is_set() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.set("WSX_CODEX_BIN", "/custom/codex");

        let targets = default_install_targets().unwrap();

        assert!(targets.iter().any(|t| t.agent == "Codex"));
        let codex_targets: Vec<_> = targets.iter().filter(|t| t.agent == "Codex").collect();
        assert_eq!(codex_targets.len(), BUNDLED_SKILLS.len());
        assert!(codex_targets.iter().any(|t| t.skill == "agent-pr"));
        assert!(codex_targets.iter().any(|t| t.skill == "wsx"));
    }

    #[test]
    fn default_targets_include_hermes_when_binary_is_on_path() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        let bin = TempDir::new().unwrap();
        let hermes = bin.path().join("hermes");
        std::fs::write(&hermes, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&hermes, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        env.set("HOME", home.path());
        env.set("PATH", bin.path());
        env.remove("WSX_CODEX_BIN");
        env.remove("WSX_HERMES_BIN");

        let targets = default_install_targets().unwrap();

        assert!(targets.iter().any(|t| {
            t.agent == "Hermes"
                && t.path
                    == home
                        .path()
                        .join(".hermes")
                        .join("skills")
                        .join("wsx")
                        .join("SKILL.md")
        }));
        let hermes_targets: Vec<_> = targets.iter().filter(|t| t.agent == "Hermes").collect();
        assert_eq!(hermes_targets.len(), BUNDLED_SKILLS.len());
        assert!(hermes_targets.iter().any(|t| t.skill == "agent-pr"));
        assert!(hermes_targets.iter().any(|t| t.skill == "wsx"));
    }

    #[test]
    fn default_targets_include_hermes_when_hermes_home_exists() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        std::fs::create_dir(home.path().join(".hermes")).unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");
        env.remove("WSX_HERMES_BIN");

        let targets = default_install_targets().unwrap();

        assert!(targets.iter().any(|t| t.agent == "Hermes"));
        let hermes_targets: Vec<_> = targets.iter().filter(|t| t.agent == "Hermes").collect();
        assert_eq!(hermes_targets.len(), BUNDLED_SKILLS.len());
        assert!(hermes_targets.iter().any(|t| t.skill == "agent-pr"));
        assert!(hermes_targets.iter().any(|t| t.skill == "wsx"));
    }

    #[test]
    fn default_targets_ignore_hermes_home_regular_file() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        std::fs::write(home.path().join(".hermes"), "").unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");
        env.remove("WSX_HERMES_BIN");

        let targets = default_install_targets().unwrap();

        assert!(!targets.iter().any(|t| t.agent == "Hermes"));
    }

    #[test]
    fn default_targets_include_hermes_when_hermes_bin_env_is_set() {
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        env.set("HOME", home.path());
        env.set("PATH", "");
        env.remove("WSX_CODEX_BIN");
        env.set("WSX_HERMES_BIN", "/custom/hermes");

        let targets = default_install_targets().unwrap();

        assert!(targets.iter().any(|t| t.agent == "Hermes"));
        let hermes_targets: Vec<_> = targets.iter().filter(|t| t.agent == "Hermes").collect();
        assert_eq!(hermes_targets.len(), BUNDLED_SKILLS.len());
        assert!(hermes_targets.iter().any(|t| t.skill == "agent-pr"));
        assert!(hermes_targets.iter().any(|t| t.skill == "wsx"));
    }
}
