//! Persists a workspace's setup-script output to a log file so a failed setup
//! is inspectable after the creation modal auto-closes. All writing is
//! best-effort — callers ignore I/O errors so logging can never break the
//! create flow. See `data::workspace::run_setup_logged` for the call site.

use crate::data::setup::{SetupLine, SetupResult};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

/// `<log_dir>/setup-<repo>-<name>.log`, with repo/name sanitized to a safe
/// filename. Stable (no timestamp) so a workspace's log is always at the same
/// path; each run truncates it.
pub fn setup_log_path(log_dir: &Path, repo: &str, name: &str) -> PathBuf {
    log_dir.join(format!("setup-{}-{}.log", sanitize(repo), sanitize(name)))
}

/// Replace anything outside `[A-Za-z0-9._-]` with `-` so repo/workspace names
/// (which can contain `/`, spaces, etc.) form a safe single path segment.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Best-effort: create (truncating) the log file under `log_dir` and write the
/// header. Returns `None` if the directory or file can't be created — the
/// caller then simply skips logging.
pub fn create(
    log_dir: &Path,
    repo: &str,
    name: &str,
    worktree: &Path,
    started_secs: u64,
) -> Option<BufWriter<File>> {
    let path = setup_log_path(log_dir, repo, name);
    std::fs::create_dir_all(path.parent()?).ok()?;
    let mut w = BufWriter::new(File::create(&path).ok()?);
    write_header(&mut w, repo, name, worktree, started_secs).ok()?;
    Some(w)
}

fn write_header(
    w: &mut impl Write,
    repo: &str,
    name: &str,
    worktree: &Path,
    started_secs: u64,
) -> io::Result<()> {
    writeln!(w, "=== setup: {repo}/{name} ===")?;
    writeln!(w, "worktree: {}", worktree.display())?;
    writeln!(w, "started:  {started_secs} (unix seconds)")?;
    writeln!(w)
}

/// Write one captured line: ANSI escapes stripped, trailing whitespace trimmed,
/// blank lines skipped (matching the on-screen buffer). `Stderr` lines are
/// prefixed `! ` so a reader can tell the two streams apart.
pub fn write_line(w: &mut impl Write, line: &SetupLine) -> io::Result<()> {
    let (raw, is_err) = match line {
        SetupLine::Stdout(s) => (s, false),
        SetupLine::Stderr(s) => (s, true),
    };
    let clean = strip_ansi_escapes::strip_str(raw);
    let clean = clean.trim_end();
    if clean.is_empty() {
        return Ok(());
    }
    if is_err {
        writeln!(w, "! {clean}")
    } else {
        writeln!(w, "{clean}")
    }
}

/// Write the outcome footer.
pub fn write_footer(w: &mut impl Write, result: &SetupResult) -> io::Result<()> {
    match result {
        SetupResult::Ok => writeln!(w, "\n=== OK ==="),
        SetupResult::Failed { exit_code } => writeln!(w, "\n=== FAILED (exit {exit_code}) ==="),
        SetupResult::Skipped => writeln!(w, "\n=== SKIPPED ==="),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn path_is_stable_and_sanitized() {
        let dir = Path::new("/logs");
        assert_eq!(
            setup_log_path(dir, "myrepo", "foo"),
            PathBuf::from("/logs/setup-myrepo-foo.log")
        );
        // Slashes and spaces in repo/name become `-`.
        assert_eq!(
            setup_log_path(dir, "org/repo", "feat branch"),
            PathBuf::from("/logs/setup-org-repo-feat-branch.log")
        );
    }

    #[test]
    fn write_line_strips_ansi_prefixes_stderr_and_skips_blank() {
        let mut buf = Vec::new();
        write_line(&mut buf, &SetupLine::Stdout("\x1b[32mok\x1b[0m".into())).unwrap();
        write_line(&mut buf, &SetupLine::Stderr("boom".into())).unwrap();
        write_line(&mut buf, &SetupLine::Stdout("   ".into())).unwrap();
        let out = String::from_utf8(buf).unwrap();
        assert_eq!(out, "ok\n! boom\n");
    }

    #[test]
    fn footer_renders_each_outcome() {
        let render = |r: &SetupResult| {
            let mut buf = Vec::new();
            write_footer(&mut buf, r).unwrap();
            String::from_utf8(buf).unwrap()
        };
        assert_eq!(render(&SetupResult::Ok), "\n=== OK ===\n");
        assert_eq!(
            render(&SetupResult::Failed { exit_code: 2 }),
            "\n=== FAILED (exit 2) ===\n"
        );
        assert_eq!(render(&SetupResult::Skipped), "\n=== SKIPPED ===\n");
    }

    #[test]
    fn create_writes_a_file_with_header() {
        let logs = TempDir::new().unwrap();
        let mut w = create(
            logs.path(),
            "myrepo",
            "foo",
            Path::new("/wt/foo"),
            1718722921,
        )
        .expect("log file should be created under a writable temp dir");
        write_line(&mut w, &SetupLine::Stdout("hello".into())).unwrap();
        drop(w); // flush
        let body = std::fs::read_to_string(setup_log_path(logs.path(), "myrepo", "foo")).unwrap();
        assert!(body.contains("=== setup: myrepo/foo ==="), "{body}");
        assert!(body.contains("worktree: /wt/foo"), "{body}");
        assert!(body.contains("1718722921 (unix seconds)"), "{body}");
        assert!(body.contains("hello"), "{body}");
    }

    #[test]
    fn create_truncates_on_second_run() {
        let logs = TempDir::new().unwrap();
        // First run writes a marker line, then the writer is flushed on drop.
        let mut w1 = create(logs.path(), "myrepo", "foo", Path::new("/wt/foo"), 1).unwrap();
        write_line(&mut w1, &SetupLine::Stdout("FIRST-RUN-MARKER".into())).unwrap();
        drop(w1);
        // A second run for the same workspace must truncate, not append.
        let w2 = create(logs.path(), "myrepo", "foo", Path::new("/wt/foo"), 2).unwrap();
        drop(w2);
        let body = std::fs::read_to_string(setup_log_path(logs.path(), "myrepo", "foo")).unwrap();
        assert!(
            !body.contains("FIRST-RUN-MARKER"),
            "second run should truncate, got: {body}"
        );
    }
}
