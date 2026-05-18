use crate::error::{Error, Result};
use std::path::Path;
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
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => run_script(s, repo_root, worktree, on_line).await,
        _ => Ok(SetupResult::Skipped),
    }
}

pub async fn run_archive<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => run_script(s, repo_root, worktree, on_line).await,
        _ => Ok(SetupResult::Skipped),
    }
}

async fn run_script<F: FnMut(SetupLine) + Send>(
    script: &str,
    repo_root: &Path,
    worktree: &Path,
    mut on_line: F,
) -> Result<SetupResult> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .filter(|s| {
            // POSIX-only shells (dash, ash, real /bin/sh on Linux) don't
            // support `-l` and would refuse to start. Fall back to bash so
            // -ilc semantics are honored regardless of host shell.
            let name = std::path::Path::new(s)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            !matches!(name, "sh" | "dash" | "ash")
        })
        .unwrap_or_else(|| "/bin/bash".to_string());
    let mut cmd = Command::new(&shell);
    cmd.arg("-ilc")
        .arg(script)
        .current_dir(worktree)
        .env("WSX_REPO_ROOT", repo_root)
        .env("WSX_WORKTREE", worktree)
        .env("ITERM_SHELL_INTEGRATION_INSTALLED", "Yes")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Setup(format!("spawn: {e}")))?;
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
    while let Ok(Some(l)) = out_reader.next_line().await {
        on_line(SetupLine::Stdout(l));
    }
    while let Ok(Some(l)) = err_reader.next_line().await {
        on_line(SetupLine::Stderr(l));
    }

    let status = child
        .wait()
        .await
        .map_err(|e| Error::Setup(format!("wait: {e}")))?;
    if status.success() {
        Ok(SetupResult::Ok)
    } else {
        Ok(SetupResult::Failed {
            exit_code: status.code().unwrap_or(-1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[tokio::test]
    async fn none_script_is_skipped() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(None, repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }

    #[tokio::test]
    async fn empty_and_whitespace_scripts_are_skipped() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(Some(""), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
        let r = run_setup(Some("   \n\t"), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }

    #[tokio::test]
    async fn setup_streams_stdout_and_stderr_and_succeeds() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines2 = lines.clone();
        let r = run_setup(
            Some("echo hello; echo bye 1>&2"),
            repo.path(),
            wt.path(),
            move |l| {
                lines2.lock().unwrap().push(l);
            },
        )
        .await
        .unwrap();
        assert!(matches!(r, SetupResult::Ok));
        let lines = lines.lock().unwrap();
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stdout(s) if s == "hello"))
        );
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stderr(s) if s == "bye"))
        );
    }

    #[tokio::test]
    async fn setup_reports_nonzero_exit() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(Some("exit 7"), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        match r {
            SetupResult::Failed { exit_code } => assert_eq!(exit_code, 7),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn setup_reports_command_not_found() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(
            Some("definitely-not-a-real-command-xyz"),
            repo.path(),
            wt.path(),
            |_| {},
        )
        .await
        .unwrap();
        match r {
            SetupResult::Failed { exit_code } => assert_eq!(exit_code, 127),
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn setup_injects_env_vars() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let lines = Arc::new(Mutex::new(Vec::new()));
        let lines2 = lines.clone();
        run_setup(
            Some("echo $WSX_WORKTREE; echo $WSX_REPO_ROOT"),
            repo.path(),
            wt.path(),
            move |l| {
                lines2.lock().unwrap().push(l);
            },
        )
        .await
        .unwrap();
        let expected_wt = wt.path().to_string_lossy().to_string();
        let expected_repo = repo.path().to_string_lossy().to_string();
        let lines = lines.lock().unwrap();
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stdout(s) if *s == expected_wt))
        );
        assert!(
            lines
                .iter()
                .any(|l| matches!(l, SetupLine::Stdout(s) if *s == expected_repo))
        );
    }

    #[tokio::test]
    async fn run_archive_executes_the_provided_script() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_archive(Some("exit 3"), repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        match r {
            SetupResult::Failed { exit_code } => assert_eq!(exit_code, 3),
            other => panic!("expected Failed, got {other:?}"),
        }
        let r = run_archive(None, repo.path(), wt.path(), |_| {})
            .await
            .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }
}
