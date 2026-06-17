use crate::error::{Error, Result};
use std::path::Path;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

/// Reads an async byte stream and yields text segments delimited by `\r`
/// or `\n`. Empty segments — the gap inside a `\r\n` pair, and blank lines —
/// are skipped; a trailing unterminated segment is flushed at EOF. Splitting
/// on `\r` lets in-place progress bars (pnpm/mise carriage-return redraws)
/// surface as individual segments instead of buffering until the next
/// newline. The logic is stateless across reads, so chunk boundaries — e.g. a
/// `\r\n` split across two reads — do not matter.
struct SegmentReader<R> {
    inner: R,
    pending: Vec<u8>,
    eof: bool,
}

impl<R: AsyncRead + Unpin> SegmentReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            pending: Vec::new(),
            eof: false,
        }
    }

    /// The next non-empty segment, or `None` at end of stream.
    async fn next_segment(&mut self) -> std::io::Result<Option<String>> {
        loop {
            // Carve a segment up to the first delimiter, if one is buffered.
            if let Some(idx) = self.pending.iter().position(|&b| b == b'\r' || b == b'\n') {
                let seg: Vec<u8> = self.pending.drain(..=idx).collect();
                // `seg` ends with the delimiter byte; drop it.
                let text = String::from_utf8_lossy(&seg[..seg.len() - 1]).into_owned();
                if text.is_empty() {
                    continue;
                }
                return Ok(Some(text));
            }
            if self.eof {
                if self.pending.is_empty() {
                    return Ok(None);
                }
                let text = String::from_utf8_lossy(&self.pending).into_owned();
                self.pending.clear();
                if text.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(text));
            }
            let mut chunk = [0u8; 1024];
            let n = self.inner.read(&mut chunk).await?;
            if n == 0 {
                self.eof = true;
            } else {
                self.pending.extend_from_slice(&chunk[..n]);
            }
        }
    }
}

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
    cancel: tokio_util::sync::CancellationToken,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => {
            run_script(s, repo_root, worktree, cancel, on_line).await
        }
        _ => Ok(SetupResult::Skipped),
    }
}

pub async fn run_archive<F: FnMut(SetupLine) + Send>(
    script: Option<&str>,
    repo_root: &Path,
    worktree: &Path,
    cancel: tokio_util::sync::CancellationToken,
    on_line: F,
) -> Result<SetupResult> {
    match script {
        Some(s) if !s.trim().is_empty() => {
            run_script(s, repo_root, worktree, cancel, on_line).await
        }
        _ => Ok(SetupResult::Skipped),
    }
}

async fn run_script<F: FnMut(SetupLine) + Send>(
    script: &str,
    repo_root: &Path,
    worktree: &Path,
    cancel: tokio_util::sync::CancellationToken,
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

    // An interactive shell (`-i`) sets up job control on its inherited TTY:
    // it calls `tcsetpgrp` to become the foreground process group, which
    // bumps wsx into the background. Our next TUI write then trips SIGTTOU
    // and suspends us. Put the child in a brand-new session so it has no
    // controlling terminal and cannot hijack ours; rc-file sourcing still
    // happens because `-i` governs that, not job control.
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| Error::Setup(format!("spawn: {e}")))?;
    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");

    let mut out_reader = SegmentReader::new(stdout);
    let mut err_reader = SegmentReader::new(stderr);

    loop {
        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                // Dropping `child` triggers kill_on_drop. We still return
                // before draining readers; the OS reaps the process.
                return Err(Error::Cancelled);
            }
            line = out_reader.next_segment() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stdout(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stdout read: {e}"))),
            },
            line = err_reader.next_segment() => match line {
                Ok(Some(l)) => on_line(SetupLine::Stderr(l)),
                Ok(None) => break,
                Err(e) => return Err(Error::Setup(format!("stderr read: {e}"))),
            },
        }
    }
    // Drain any remaining stderr after stdout closes (and vice versa).
    while let Ok(Some(l)) = out_reader.next_segment().await {
        on_line(SetupLine::Stdout(l));
    }
    while let Ok(Some(l)) = err_reader.next_segment().await {
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
        let r = run_setup(
            None,
            repo.path(),
            wt.path(),
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }

    #[tokio::test]
    async fn empty_and_whitespace_scripts_are_skipped() {
        let repo = TempDir::new().unwrap();
        let wt = TempDir::new().unwrap();
        let r = run_setup(
            Some(""),
            repo.path(),
            wt.path(),
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
        let r = run_setup(
            Some("   \n\t"),
            repo.path(),
            wt.path(),
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
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
            tokio_util::sync::CancellationToken::new(),
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
        let r = run_setup(
            Some("exit 7"),
            repo.path(),
            wt.path(),
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
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
            tokio_util::sync::CancellationToken::new(),
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
            tokio_util::sync::CancellationToken::new(),
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
        let r = run_archive(
            Some("exit 3"),
            repo.path(),
            wt.path(),
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        match r {
            SetupResult::Failed { exit_code } => assert_eq!(exit_code, 3),
            other => panic!("expected Failed, got {other:?}"),
        }
        let r = run_archive(
            None,
            repo.path(),
            wt.path(),
            tokio_util::sync::CancellationToken::new(),
            |_| {},
        )
        .await
        .unwrap();
        assert!(matches!(r, SetupResult::Skipped));
    }

    #[tokio::test]
    async fn run_setup_respects_cancellation() {
        use tokio_util::sync::CancellationToken;
        let tmp = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            cancel_clone.cancel();
        });
        let start = std::time::Instant::now();
        let result = run_setup(Some("sleep 10"), tmp.path(), tmp.path(), cancel, |_| {}).await;
        let elapsed = start.elapsed();
        assert!(
            matches!(result, Err(Error::Cancelled)),
            "expected Err(Cancelled), got {result:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(1500),
            "expected fast cancel, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn run_setup_completes_before_cancel_is_ignored() {
        use tokio_util::sync::CancellationToken;
        let tmp = TempDir::new().unwrap();
        let cancel = CancellationToken::new();
        let result = run_setup(Some("true"), tmp.path(), tmp.path(), cancel.clone(), |_| {}).await;
        // Cancel arrives long after run_setup has returned.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        cancel.cancel();
        assert!(matches!(result, Ok(SetupResult::Ok)), "got {result:?}");
    }

    async fn collect_segments(input: &[u8]) -> Vec<String> {
        let mut r = SegmentReader::new(std::io::Cursor::new(input.to_vec()));
        let mut out = Vec::new();
        while let Some(seg) = r.next_segment().await.unwrap() {
            out.push(seg);
        }
        out
    }

    #[tokio::test]
    async fn segments_split_on_newline() {
        assert_eq!(collect_segments(b"a\nb\nc\n").await, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn segments_split_on_carriage_return() {
        // No trailing delimiter: the final "c" is flushed at EOF.
        assert_eq!(collect_segments(b"a\rb\rc").await, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn crlf_pair_yields_single_segment() {
        assert_eq!(collect_segments(b"x\r\ny\r\n").await, vec!["x", "y"]);
    }

    #[tokio::test]
    async fn empty_segments_are_skipped_and_trailing_flushed() {
        assert_eq!(
            collect_segments(b"line\n\nblank").await,
            vec!["line", "blank"]
        );
    }

    #[tokio::test]
    async fn empty_input_yields_nothing() {
        assert_eq!(collect_segments(b"").await, Vec::<String>::new());
    }
}
