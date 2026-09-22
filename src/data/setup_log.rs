//! Persists a workspace's setup-script output to a log file so a failed setup
//! is inspectable after the creation modal auto-closes. All writing is
//! best-effort — callers ignore I/O errors so logging can never break the
//! create flow. See `data::workspace::run_setup_logged` for the call site.

use crate::data::setup::{SetupLine, SetupResult};
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
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

/// Max bytes `read` will pull off disk, taken from the END of the file.
///
/// `READ_CAP` alone bounds only what is *retained*. Reading the whole file
/// first and trimming afterwards means a verbose setup script (a full
/// `cargo build`, an `npm ci` with a noisy resolver) decides how much the
/// dashboard allocates — and this read happens on the UI path while the App
/// mutex is held, so it stalls every other App user too. 256 KiB is far more
/// than `READ_CAP` lines of ordinary build output needs and puts a ceiling on
/// both the read and the allocation.
const READ_TAIL_BYTES: u64 = 256 * 1024;

/// Max characters kept per line. A setup script that emits a megabyte-long
/// line (a minified bundle, a base64 blob) would otherwise be retained whole
/// for a viewer that can only show one terminal width of it.
const MAX_LINE_CHARS: usize = 2048;

/// Read the tail of a workspace's persisted setup log, newest-last, for the
/// TUI viewer.
///
/// `None` means there is no log on disk — either the repo has no setup script
/// (nothing is ever written in that case, see `workspace::run_setup_logged`)
/// or the workspace predates log persistence. That is a normal state, not an
/// error, so an unreadable file is reported the same way for now.
///
/// Bounded twice over, because this runs on the UI path: at most
/// `READ_TAIL_BYTES` are read from the end of the file, then at most
/// `READ_CAP` lines are kept, each truncated to `MAX_LINE_CHARS`. A log
/// larger than the byte budget loses its head, including the `=== setup: ===`
/// header — the same trade `READ_CAP` already makes, and the tail is the part
/// that says how the run ended.
pub fn read(log_dir: &Path, repo: &str, name: &str) -> Option<Vec<String>> {
    let path = setup_log_path(log_dir, repo, name);
    let mut f = File::open(&path).ok()?;
    let len = f.metadata().ok()?.len();
    let truncated = len > READ_TAIL_BYTES;
    if truncated {
        f.seek(SeekFrom::Start(len - READ_TAIL_BYTES)).ok()?;
    }
    let mut buf = Vec::with_capacity(len.min(READ_TAIL_BYTES) as usize + 1);
    f.take(READ_TAIL_BYTES).read_to_end(&mut buf).ok()?;
    // Seeking to a byte offset can land mid-codepoint, so decode lossily
    // rather than failing the whole read over one split character.
    let body = String::from_utf8_lossy(&buf);

    let mut lines = body.lines();
    // The first line after a mid-file seek is a fragment of whatever line
    // straddled the boundary; drop it rather than show half a line.
    if truncated {
        lines.next();
    }
    let mut lines: Vec<String> = lines
        .map(|l| {
            let l = l.trim_end();
            match l.char_indices().nth(MAX_LINE_CHARS) {
                Some((i, _)) => format!("{}…", &l[..i]),
                None => l.to_string(),
            }
        })
        .collect();
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

    /// `READ_CAP` bounds what is retained, not what is read. Without a byte
    /// budget a verbose setup script decides how much the dashboard allocates,
    /// on the UI path, under the App lock.
    #[test]
    fn read_only_pulls_the_tail_of_a_huge_log_off_disk() {
        let logs = TempDir::new().unwrap();
        let mut w = create(logs.path(), "myrepo", "huge", Path::new("/wt/huge"), 1).unwrap();
        // Comfortably past the byte budget: ~1 MiB of 100-char lines.
        let filler = "y".repeat(100);
        for i in 0..10_000 {
            write_line(&mut w, &SetupLine::Stdout(format!("{filler} {i}"))).unwrap();
        }
        write_footer(&mut w, &SetupResult::Ok).unwrap();
        drop(w);

        let path = setup_log_path(logs.path(), "myrepo", "huge");
        let on_disk = std::fs::metadata(&path).unwrap().len();
        assert!(
            on_disk > READ_TAIL_BYTES,
            "test needs a log past the budget, got {on_disk}"
        );

        let lines = read(logs.path(), "myrepo", "huge").unwrap();
        let held: usize = lines.iter().map(|l| l.len()).sum();
        assert!(
            (held as u64) <= READ_TAIL_BYTES,
            "retained {held} bytes from a {on_disk}-byte log"
        );
        assert_eq!(
            lines.last().unwrap(),
            "=== OK ===",
            "the tail is what the viewer needs: {:?}",
            lines.last()
        );
        assert!(
            !lines.iter().any(|l| l.starts_with("=== setup:")),
            "a log past the byte budget loses its head, header included"
        );
    }

    /// A seek to a byte offset lands mid-line; the fragment must be dropped
    /// rather than shown as if it were a whole line.
    #[test]
    fn read_drops_the_partial_line_at_the_seek_boundary() {
        let logs = TempDir::new().unwrap();
        let mut w = create(logs.path(), "myrepo", "split", Path::new("/wt/split"), 1).unwrap();
        // Distinctive, uniform lines: any retained line must be whole.
        for i in 0..40_000 {
            write_line(&mut w, &SetupLine::Stdout(format!("<<{i:09}>>"))).unwrap();
        }
        drop(w);

        let lines = read(logs.path(), "myrepo", "split").unwrap();
        assert!(!lines.is_empty());
        for l in &lines {
            assert!(
                l.starts_with("<<") && l.ends_with(">>"),
                "partial line survived the seek boundary: {l:?}"
            );
        }
    }

    #[test]
    fn read_truncates_an_absurdly_long_line() {
        let logs = TempDir::new().unwrap();
        let mut w = create(logs.path(), "myrepo", "wide", Path::new("/wt/wide"), 1).unwrap();
        write_line(&mut w, &SetupLine::Stdout("z".repeat(MAX_LINE_CHARS * 4))).unwrap();
        drop(w);

        let lines = read(logs.path(), "myrepo", "wide").unwrap();
        let long = lines.iter().find(|l| l.starts_with('z')).unwrap();
        assert_eq!(
            long.chars().count(),
            MAX_LINE_CHARS + 1,
            "plus the ellipsis"
        );
        assert!(long.ends_with('…'));
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
