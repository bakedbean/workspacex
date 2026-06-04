//! Synchronize Claude Code slash commands into a small local Codex plugin.

use serde_json::{Value, json};
use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};

const PLUGIN_NAME: &str = "wsx-claude-commands";

fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

fn claude_commands_dir() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".claude").join("commands"))
}

fn plugin_root() -> Option<PathBuf> {
    home_dir().map(|h| h.join("plugins").join(PLUGIN_NAME))
}

fn marketplace_path() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".agents").join("plugins").join("marketplace.json"))
}

fn plugin_manifest() -> Value {
    json!({
        "name": PLUGIN_NAME,
        "version": "1.0.0",
        "description": "Claude Code slash commands mirrored for Codex",
        "author": {
            "name": "wsx"
        },
        "homepage": "https://github.com/bakedbean/workspacex",
        "repository": "https://github.com/bakedbean/workspacex",
        "license": "MIT",
        "keywords": ["claude", "codex", "slash-commands", "wsx"],
        "interface": {
            "displayName": "Claude Commands",
            "shortDescription": "Use Claude Code slash commands in Codex.",
            "longDescription": "Mirrors Markdown command files from ~/.claude/commands into Codex.",
            "developerName": "wsx",
            "category": "Productivity",
            "capabilities": ["Interactive"],
            "defaultPrompt": [
                "Use one of my mirrored Claude commands."
            ],
            "brandColor": "#111827",
            "screenshots": []
        }
    })
}

fn marketplace_entry() -> Value {
    json!({
        "name": PLUGIN_NAME,
        "source": {
            "source": "local",
            "path": format!("./plugins/{PLUGIN_NAME}")
        },
        "policy": {
            "installation": "INSTALLED_BY_DEFAULT",
            "authentication": "ON_INSTALL"
        },
        "category": "Productivity"
    })
}

fn default_marketplace() -> Value {
    json!({
        "name": "personal",
        "interface": {
            "displayName": "Personal"
        },
        "plugins": []
    })
}

fn write_json(path: &Path, payload: &Value) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().and_then(OsStr::to_str).unwrap_or("json");
    let tmp = parent.join(format!(
        ".{file_name}.wsx-tmp.{}.{}",
        std::process::id(),
        rand::random::<u32>()
    ));
    let data = format!("{}\n", serde_json::to_string_pretty(payload)?);

    {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)?;
        file.write_all(data.as_bytes())?;
        file.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn collect_command_files(dir: &Path) -> Vec<(PathBuf, PathBuf)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                walk(root, &path, out);
            } else if file_type.is_file()
                && path.extension() == Some(OsStr::new("md"))
                && let Ok(rel) = path.strip_prefix(root)
            {
                out.push((path.clone(), rel.to_path_buf()));
            }
        }
    }

    let mut files = Vec::new();
    walk(dir, dir, &mut files);
    files.sort_by(|a, b| a.1.cmp(&b.1));
    files
}

fn sync_command_files(source_dir: &Path, commands_dir: &Path) -> std::io::Result<usize> {
    let files = collect_command_files(source_dir);
    if commands_dir.exists() {
        std::fs::remove_dir_all(commands_dir)?;
    }
    if files.is_empty() {
        return Ok(0);
    }

    for (source, rel) in &files {
        let target = commands_dir.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source, target)?;
    }
    Ok(files.len())
}

fn ensure_marketplace_entry(path: &Path) -> std::io::Result<()> {
    let mut marketplace = if path.exists() {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str::<Value>(&text).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("parse {}: {e}", path.display()),
            )
        })?
    } else {
        default_marketplace()
    };

    if !marketplace.is_object() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{} must contain a JSON object", path.display()),
        ));
    }
    if marketplace.get("name").and_then(Value::as_str).is_none() {
        marketplace["name"] = json!("personal");
    }
    if marketplace.get("interface").is_none() {
        marketplace["interface"] = json!({ "displayName": "Personal" });
    }
    if !marketplace
        .get("plugins")
        .map(Value::is_array)
        .unwrap_or(false)
    {
        if marketplace.get("plugins").is_some() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}.plugins must be an array", path.display()),
            ));
        }
        marketplace["plugins"] = json!([]);
    }

    let entry = marketplace_entry();
    let plugins = marketplace
        .get_mut("plugins")
        .and_then(Value::as_array_mut)
        .expect("plugins was normalized to an array");
    if let Some(existing) = plugins
        .iter_mut()
        .find(|p| p.get("name").and_then(Value::as_str) == Some(PLUGIN_NAME))
    {
        *existing = entry;
    } else {
        plugins.push(entry);
    }

    write_json(path, &marketplace)
}

/// Best-effort sync of global Claude Code commands into a local Codex plugin.
///
/// The generated plugin lives at `~/plugins/wsx-claude-commands` and is
/// referenced from the implicit personal marketplace at
/// `~/.agents/plugins/marketplace.json`.
pub fn sync_claude_commands_for_codex() {
    let Some(source_dir) = claude_commands_dir() else {
        return;
    };
    if !source_dir.is_dir() {
        return;
    }
    let Some(root) = plugin_root() else {
        return;
    };
    let commands_dir = root.join("commands");

    let Ok(count) = sync_command_files(&source_dir, &commands_dir) else {
        tracing::warn!(
            source = %source_dir.display(),
            target = %commands_dir.display(),
            "failed to mirror Claude commands for Codex"
        );
        return;
    };
    if count == 0 {
        return;
    }

    if let Err(e) = write_json(
        &root.join(".codex-plugin").join("plugin.json"),
        &plugin_manifest(),
    ) {
        tracing::warn!(error = %e, path = %root.display(), "failed to write Codex command plugin");
        return;
    }
    if let Some(path) = marketplace_path()
        && let Err(e) = ensure_marketplace_entry(&path)
    {
        tracing::warn!(
            error = %e,
            path = %path.display(),
            "failed to register Codex command plugin marketplace entry"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::EnvGuard;

    #[test]
    fn sync_copies_claude_commands_into_codex_plugin() {
        let mut env = EnvGuard::new();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path());

        let claude = home.path().join(".claude/commands");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("pull-request.md"), "# /pull-request\n").unwrap();
        std::fs::create_dir_all(claude.join("team")).unwrap();
        std::fs::write(claude.join("team/review.md"), "# /team:review\n").unwrap();

        sync_claude_commands_for_codex();

        let root = home.path().join("plugins/wsx-claude-commands");
        assert_eq!(
            std::fs::read_to_string(root.join("commands/pull-request.md")).unwrap(),
            "# /pull-request\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("commands/team/review.md")).unwrap(),
            "# /team:review\n"
        );

        let manifest: Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(".codex-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["name"], PLUGIN_NAME);

        let marketplace: Value = serde_json::from_str(
            &std::fs::read_to_string(home.path().join(".agents/plugins/marketplace.json")).unwrap(),
        )
        .unwrap();
        let entry = marketplace["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == PLUGIN_NAME)
            .unwrap();
        assert_eq!(entry["policy"]["installation"], "INSTALLED_BY_DEFAULT");
        assert_eq!(entry["source"]["path"], format!("./plugins/{PLUGIN_NAME}"));
    }

    #[test]
    fn sync_replaces_stale_mirrored_commands() {
        let mut env = EnvGuard::new();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path());

        let claude = home.path().join(".claude/commands");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(claude.join("current.md"), "current\n").unwrap();
        let stale = home
            .path()
            .join("plugins/wsx-claude-commands/commands/stale.md");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "stale\n").unwrap();

        sync_claude_commands_for_codex();

        assert!(
            home.path()
                .join("plugins/wsx-claude-commands/commands/current.md")
                .exists()
        );
        assert!(!stale.exists());
    }

    #[test]
    fn sync_empty_source_removes_stale_mirrored_commands() {
        let mut env = EnvGuard::new();
        let home = tempfile::tempdir().unwrap();
        env.set("HOME", home.path());

        let claude = home.path().join(".claude/commands");
        std::fs::create_dir_all(&claude).unwrap();
        let stale = home
            .path()
            .join("plugins/wsx-claude-commands/commands/stale.md");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::write(&stale, "stale\n").unwrap();

        sync_claude_commands_for_codex();

        assert!(
            !home
                .path()
                .join("plugins/wsx-claude-commands/commands")
                .exists()
        );
    }

    #[test]
    fn invalid_marketplace_json_is_not_overwritten() {
        let home = tempfile::tempdir().unwrap();
        let marketplace = home.path().join(".agents/plugins/marketplace.json");
        std::fs::create_dir_all(marketplace.parent().unwrap()).unwrap();
        std::fs::write(&marketplace, "{not json").unwrap();

        let err = ensure_marketplace_entry(&marketplace).unwrap_err();

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert_eq!(std::fs::read_to_string(&marketplace).unwrap(), "{not json");
    }
}
