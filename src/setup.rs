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

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub enum SetupLine {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Clone)]
pub enum SetupResult {
    Skipped,
    Ok,
    Failed { exit_code: i32 },
}

pub async fn run_setup<F: FnMut(SetupLine) + Send>(
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult> {
    let cfg = load_repo_config(repo_root)?;
    match cfg.setup {
        None => Ok(SetupResult::Skipped),
        Some(spec) => run_script(&spec, repo_root, worktree, on_line).await,
    }
}

pub async fn run_archive<F: FnMut(SetupLine) + Send>(
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult> {
    let cfg = load_repo_config(repo_root)?;
    match cfg.archive {
        None => Ok(SetupResult::Skipped),
        Some(spec) => run_script(&spec, repo_root, worktree, on_line).await,
    }
}

async fn run_script<F: FnMut(SetupLine) + Send>(
    spec: &ScriptSpec,
    repo_root: &Path,
    worktree: &Path,
    mut on_line: F,
) -> Result<SetupResult> {
    let mut cmd = Command::new(&spec.command);
    cmd.args(&spec.args)
        .current_dir(worktree)
        .env("WSX_REPO_ROOT", repo_root)
        .env("WSX_WORKTREE", worktree)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in &spec.env { cmd.env(k, v); }

    let mut child = cmd.spawn().map_err(|e| Error::Setup(format!("spawn: {e}")))?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let mut out_reader = BufReader::new(stdout).lines();
    let mut err_reader = BufReader::new(stderr).lines();

    loop {
        tokio::select! {
            line = out_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stdout(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stdout read: {e}"))),
            },
            line = err_reader.next_line() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stderr(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stderr read: {e}"))),
            },
        }
    }
    // Drain any remaining stderr after stdout closes (and vice versa).
    while let Ok(Some(l)) = out_reader.next_line().await { on_line(SetupLine::Stdout(l)); }
    while let Ok(Some(l)) = err_reader.next_line().await { on_line(SetupLine::Stderr(l)); }

    let status = child.wait().await.map_err(|e| Error::Setup(format!("wait: {e}")))?;
    if status.success() { Ok(SetupResult::Ok) }
    else { Ok(SetupResult::Failed { exit_code: status.code().unwrap_or(-1) }) }
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

#[cfg(test)]
mod run_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn write_cfg(dir: &Path, json: &str) {
        std::fs::write(dir.join(".claudette.json"), json).unwrap();
    }

    #[tokio::test]
    async fn setup_skipped_when_no_block() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(repo.path(), wt.path(), |_| {}).await.unwrap();
        matches!(r, SetupResult::Skipped);
    }

    #[tokio::test]
    async fn setup_streams_output_and_succeeds() {
        let repo = TempDir::new().unwrap();
        write_cfg(repo.path(), r#"{"setup":{"command":"sh","args":["-c","echo hello; echo bye 1>&2"]}}"#);
        let wt = TempDir::new().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines2 = lines.clone();
        let r = run_setup(repo.path(), wt.path(), move |l| {
            lines2.lock().unwrap().push(l);
        }).await.unwrap();
        matches!(r, SetupResult::Ok);
        let lines = lines.lock().unwrap();
        assert!(lines.iter().any(|l| matches!(l, SetupLine::Stdout(s) if s == "hello")));
        assert!(lines.iter().any(|l| matches!(l, SetupLine::Stderr(s) if s == "bye")));
    }

    #[tokio::test]
    async fn setup_reports_nonzero_exit() {
        let repo = TempDir::new().unwrap();
        write_cfg(repo.path(), r#"{"setup":{"command":"sh","args":["-c","exit 7"]}}"#);
        let wt = TempDir::new().unwrap();
        let r = run_setup(repo.path(), wt.path(), |_| {}).await.unwrap();
        match r {
            SetupResult::Failed { exit_code } => assert_eq!(exit_code, 7),
            _ => panic!("expected Failed"),
        }
    }

    #[tokio::test]
    async fn setup_injects_env_vars() {
        let repo = TempDir::new().unwrap();
        write_cfg(repo.path(), r#"{"setup":{"command":"sh","args":["-c","echo $WSX_WORKTREE"]}}"#);
        let wt = TempDir::new().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines2 = lines.clone();
        run_setup(repo.path(), wt.path(), move |l| {
            lines2.lock().unwrap().push(l);
        }).await.unwrap();
        let expected = wt.path().to_string_lossy().to_string();
        assert!(lines.lock().unwrap().iter().any(|l| matches!(l, SetupLine::Stdout(s) if *s == expected)));
    }
}
