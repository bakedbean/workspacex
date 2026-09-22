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
    // 64 KiB buffer: line writes happen inside `run_script`'s async read loop,
    // so each flush is a blocking `write` on the runtime thread. A larger buffer
    // keeps those syscalls rare even when a setup script is noisy.
    let mut w = BufWriter::with_capacity(64 * 1024, File::create(&path).ok()?);
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

/// Max lines handed back by `read`. A runaway setup script can produce a log
/// far larger than anyone will scroll through, and the viewer only ever shows
/// the end of it, so the head is dropped rather than held in memory.
pub const READ_CAP: usize = 2000;

/// Read a workspace's persisted setup log, newest-last, for the TUI viewer.
///
/// `None` means there is no log on disk — either the repo has no setup script
/// (nothing is ever written in that case, see `workspace::run_setup_logged`)
/// or the workspace predates log persistence. That is a normal state, not an
/// error, so an unreadable file is reported the same way. At most `READ_CAP`
/// lines are returned; the oldest are dropped first.
pub fn read(log_dir: &Path, repo: &str, name: &str) -> Option<Vec<String>> {
    let body = std::fs::read_to_string(setup_log_path(log_dir, repo, name)).ok()?;
    let mut lines: Vec<String> = body.lines().map(|l| l.trim_end().to_string()).collect();
    // Written with a trailing newline, so the split leaves one empty tail.
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    if lines.len() > READ_CAP {
        lines.drain(..lines.len() - READ_CAP);
    }
    Some(lines)
}

/// Move a workspace's log to its new name. The log path is derived from the
/// workspace name, so without this a rename would orphan the file and `o` on
/// the dashboard would report "no setup log" for a workspace that has one.
/// Best-effort like the rest of this module: a missing or unmovable log is
/// silently left alone rather than failing the rename.
pub fn rename(log_dir: &Path, repo: &str, old_name: &str, new_name: &str) {
    let from = setup_log_path(log_dir, repo, old_name);
    let to = setup_log_path(log_dir, repo, new_name);
    if from == to || !from.exists() {
        return;
    }
    let _ = std::fs::rename(&from, &to);
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

    #[test]
    fn read_returns_none_when_no_log_was_ever_written() {
        let logs = TempDir::new().unwrap();
        assert!(read(logs.path(), "myrepo", "never-ran").is_none());
    }

    #[test]
    fn read_returns_header_body_and_footer_without_a_trailing_blank() {
        let logs = TempDir::new().unwrap();
        let mut w = create(logs.path(), "myrepo", "foo", Path::new("/wt/foo"), 1).unwrap();
        write_line(&mut w, &SetupLine::Stdout("building".into())).unwrap();
        write_line(&mut w, &SetupLine::Stderr("warning".into())).unwrap();
        write_footer(&mut w, &SetupResult::Failed { exit_code: 2 }).unwrap();
        drop(w);

        let lines = read(logs.path(), "myrepo", "foo").expect("log should be readable");
        assert_eq!(lines[0], "=== setup: myrepo/foo ===");
        assert!(lines.contains(&"building".to_string()), "{lines:?}");
        assert!(
            lines.contains(&"! warning".to_string()),
            "stderr keeps its marker: {lines:?}"
        );
        assert_eq!(
            lines.last().unwrap(),
            "=== FAILED (exit 2) ===",
            "the footer must be the last line, not a blank: {lines:?}"
        );
    }

    #[test]
    fn read_keeps_the_newest_lines_when_over_the_cap() {
        let logs = TempDir::new().unwrap();
        let mut w = create(logs.path(), "myrepo", "noisy", Path::new("/wt/noisy"), 1).unwrap();
        for i in 0..(READ_CAP + 50) {
            write_line(&mut w, &SetupLine::Stdout(format!("line {i}"))).unwrap();
        }
        drop(w);

        let lines = read(logs.path(), "myrepo", "noisy").unwrap();
        assert_eq!(lines.len(), READ_CAP);
        assert_eq!(
            lines.last().unwrap(),
            &format!("line {}", READ_CAP + 49),
            "the tail is what the viewer needs"
        );
        assert!(
            !lines.contains(&"=== setup: myrepo/noisy ===".to_string()),
            "the header is old enough to be dropped once the cap is hit"
        );
    }

    #[test]
    fn rename_moves_the_log_to_the_new_name() {
        let logs = TempDir::new().unwrap();
        let mut w = create(logs.path(), "myrepo", "old", Path::new("/wt/old"), 1).unwrap();
        write_line(&mut w, &SetupLine::Stdout("kept".into())).unwrap();
        drop(w);

        rename(logs.path(), "myrepo", "old", "new");

        assert!(read(logs.path(), "myrepo", "old").is_none());
        let lines = read(logs.path(), "myrepo", "new").expect("log should follow the rename");
        assert!(lines.contains(&"kept".to_string()), "{lines:?}");
    }

    #[test]
    fn rename_is_a_noop_without_a_log() {
        let logs = TempDir::new().unwrap();
        rename(logs.path(), "myrepo", "old", "new");
        assert!(read(logs.path(), "myrepo", "new").is_none());
    }
}
