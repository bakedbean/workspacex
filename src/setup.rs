use crate::error::{Error, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RepoConfig {
    pub setup: Option<ScriptSpec>,
    pub archive: Option<ScriptSpec>,
}

pub fn load_repo_config(repo_root: &Path) -> Result<RepoConfig> {
    let path = repo_root.join(".claudette.json");
    if !path.exists() { return Ok(RepoConfig::default()); }
    let text = std::fs::read_to_string(&path)?;
    serde_json::from_str(&text)
        .map_err(|e| Error::Setup(format!(".claudette.json parse: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_file_returns_default() {
        let dir = TempDir::new().unwrap();
        let cfg = load_repo_config(dir.path()).unwrap();
        assert!(cfg.setup.is_none());
        assert!(cfg.archive.is_none());
    }

    #[test]
    fn parses_setup_and_archive() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".claudette.json"), r#"{
            "setup":   { "command": "bash", "args": ["-c", "echo hi"] },
            "archive": { "command": "true" }
        }"#).unwrap();
        let cfg = load_repo_config(dir.path()).unwrap();
        assert_eq!(cfg.setup.as_ref().unwrap().command, "bash");
        assert_eq!(cfg.setup.as_ref().unwrap().args, vec!["-c", "echo hi"]);
        assert_eq!(cfg.archive.as_ref().unwrap().command, "true");
    }

    #[test]
    fn malformed_json_is_setup_error() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".claudette.json"), "{ not json").unwrap();
        let err = load_repo_config(dir.path()).unwrap_err();
        matches!(err, Error::Setup(_));
    }

    #[test]
    fn ignores_unknown_fields() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".claudette.json"), r#"{
            "setup": { "command": "true" },
            "env_providers": ["direnv"],
            "mcp": {"servers": []}
        }"#).unwrap();
        let cfg = load_repo_config(dir.path()).unwrap();
        assert!(cfg.setup.is_some());
    }
}
