//! Locate oh-my-pi session JSONL files for activity tailing.
//!
//! omp stores sessions at `~/.omp/agent/sessions/<encoded-cwd>/<ts>_<uuid>.jsonl`.
//! The **schema** of those files is pi's (v3): the same `{"type":"message", …,
//! "message":{…}}` envelope, the same `user`/`assistant`/`toolResult` roles, the
//! same `text`/`thinking`/`toolCall` content parts, the same `stopReason`
//! vocabulary, and the same lowercase tool names. Verified against a real omp
//! v17.4.0 capture. So this module reimplements only the *location* — the
//! parser is `sessionx`'s pi parser, re-exported below.
//!
//! If omp's schema ever diverges from pi's, the fixture test in
//! `omp_jsonl_parses_through_the_pi_parser` fails loudly, which is the signal to
//! fork the parser rather than keep sharing it.

use std::path::{Path, PathBuf};

/// Read new lines from an omp session file and parse them as pi-schema JSONL.
///
/// Deliberately the pi parser, not a copy — see the module docs.
pub use sessionx::activity::pi_events::tail_session;

/// Collapse the separators omp collapses when encoding a path segment.
fn collapse(s: &str) -> String {
    s.replace(['/', '\\', ':'], "-")
}

/// Encode `cwd` the way omp names its session directory.
///
/// Mirrors `getDefaultSessionDirName` in omp's `src/session/session-paths.ts`,
/// which classifies the (canonicalized) cwd into one of three scopes, **in this
/// order**:
///
/// 1. **home** — `cwd` is `home` or under it. `-` + the home-relative path with
///    separators collapsed to `-`. `home` itself yields the bare `-`.
/// 2. **tmp** — `cwd` is `tmp` or under it. `-tmp`, then (when the relative part
///    is non-empty) `-` + the tmp-relative path collapsed the same way.
/// 3. **abs** — everything else. omp's legacy form: `--` + the absolute path
///    with the leading separator stripped and `/`, `\`, `:` collapsed to `-`,
///    + `--`.
///
/// Order matters: a home that lives under the tmp root (containers, some CI
/// images) must still take the home branch.
///
/// `home` and `tmp` are parameters rather than being read from the environment
/// so the classification is testable without touching the real machine.
pub fn encode_cwd(cwd: &Path, home: &Path, tmp: &Path) -> String {
    if let Ok(rel) = cwd.strip_prefix(home) {
        // The "-" prefix already ends in a separator, so nothing is inserted.
        return format!("-{}", collapse(&rel.to_string_lossy()));
    }
    if let Ok(rel) = cwd.strip_prefix(tmp) {
        let encoded = collapse(&rel.to_string_lossy());
        // The "-tmp" prefix does not end in a separator, so omp inserts one —
        // but only when there is a relative part to separate from.
        return if encoded.is_empty() {
            "-tmp".to_string()
        } else {
            format!("-tmp-{encoded}")
        };
    }
    format!(
        "--{}--",
        collapse(cwd.to_string_lossy().trim_start_matches('/'))
    )
}

/// omp's legacy absolute session-dir name, used before 17.x introduced the
/// home/tmp scopes.
///
/// omp migrates these lazily, on first access **by omp itself** — so a worktree
/// whose history predates the migration and that omp has not reopened since
/// still has its sessions filed here. Probing it costs one `is_dir` and closes a
/// "prior session exists but wsx can't see it" gap.
fn legacy_dir_name(cwd: &Path) -> String {
    format!(
        "--{}--",
        collapse(cwd.to_string_lossy().trim_start_matches('/'))
    )
}

/// The `$HOME` and temp roots omp classifies against, both canonicalized so a
/// symlinked home (`/home` → `/usr/home`, macOS `/tmp` → `/private/tmp`) lands
/// in the same scope omp puts it in.
fn scope_roots() -> Option<(PathBuf, PathBuf)> {
    let home = dirs::home_dir()?;
    let home = std::fs::canonicalize(&home).unwrap_or(home);
    let tmp = std::env::temp_dir();
    let tmp = std::fs::canonicalize(&tmp).unwrap_or(tmp);
    Some((home, tmp))
}

/// omp's session directory for `worktree`, or `None` when the path can't be
/// canonicalized or there is no home dir. The returned directory is not
/// guaranteed to exist.
pub fn session_dir(worktree: &Path) -> Option<PathBuf> {
    let (home, tmp) = scope_roots()?;
    let abs = std::fs::canonicalize(worktree).ok()?;
    let root = dirs::home_dir()?.join(".omp/agent/sessions");
    let canonical = root.join(encode_cwd(&abs, &home, &tmp));
    if canonical.is_dir() {
        return Some(canonical);
    }
    let legacy = root.join(legacy_dir_name(&abs));
    if legacy.is_dir() {
        return Some(legacy);
    }
    Some(canonical)
}

/// Locate the newest session file for a worktree, or `None` when omp has none.
///
/// Canonicalizes first: omp resolves symlinks before classifying the cwd, so a
/// symlinked worktree would otherwise be looked up under the wrong name.
pub fn locate_session_file(worktree: &Path) -> Option<PathBuf> {
    let session_dir = session_dir(worktree)?;
    if !session_dir.is_dir() {
        return None;
    }
    let candidates = std::fs::read_dir(&session_dir)
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                return None;
            }
            let mtime = entry.metadata().ok()?.modified().ok()?;
            Some((mtime, entry.file_name(), path))
        });
    newest_by_mtime_then_name(candidates)
}

/// Pick the newest session from `(mtime, file_name, path)` candidates, breaking
/// equal mtimes by file name.
///
/// Split out from [`locate_session_file`] so the choice can be tested
/// independently of `read_dir` order — which is the whole point of the
/// tie-break, and which a filesystem-backed test cannot demonstrate because it
/// only ever sees whatever order the OS happens to yield.
///
/// Equal mtimes are reachable in practice: coarse filesystem timestamp
/// granularity, or two sessions written inside the same tick. Since `read_dir`
/// order is unspecified, taking whichever arrived first would make the choice
/// vary between runs and let the dashboard tail an older session.
///
/// The file name is the right tie-break rather than merely a stable one: omp
/// names sessions `<ISO-8601-ish timestamp>_<uuid>.jsonl`, zero-padded and
/// most-significant field first, so a lexicographic max over names is also
/// chronological order.
fn newest_by_mtime_then_name<I>(candidates: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = (std::time::SystemTime, std::ffi::OsString, PathBuf)>,
{
    candidates
        .into_iter()
        .max_by(|(a_time, a_name, _), (b_time, b_name, _)| {
            a_time.cmp(b_time).then_with(|| a_name.cmp(b_name))
        })
        .map(|(_, _, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// Home scope: the name is `-` followed by the home-relative path with `/`
    /// collapsed to `-`. Verified against omp v17.4.0 on a real run, which put
    /// this worktree's sessions in
    /// `-.local-state-wsx-worktrees-workspacex-grand-verbena`.
    #[test]
    fn home_scope_strips_home_and_collapses_separators() {
        assert_eq!(
            encode_cwd(
                Path::new("/home/eben/.local/state/wsx/worktrees/repo/slug"),
                Path::new("/home/eben"),
                Path::new("/tmp"),
            ),
            "-.local-state-wsx-worktrees-repo-slug"
        );
    }

    /// A cwd of exactly $HOME has an empty relative path, which omp encodes as
    /// the bare prefix.
    #[test]
    fn home_itself_encodes_to_a_bare_dash() {
        assert_eq!(
            encode_cwd(
                Path::new("/home/eben"),
                Path::new("/home/eben"),
                Path::new("/tmp")
            ),
            "-"
        );
    }

    /// Tmp scope uses the `-tmp` prefix, which does NOT end in `-`, so omp
    /// inserts a separator before the relative part. Verified on a real run:
    /// `/tmp/ompprobe/deep/dir` → `-tmp-ompprobe-deep-dir`.
    #[test]
    fn tmp_scope_prefixes_with_tmp_and_inserts_a_separator() {
        assert_eq!(
            encode_cwd(
                Path::new("/tmp/ompprobe/deep/dir"),
                Path::new("/home/eben"),
                Path::new("/tmp")
            ),
            "-tmp-ompprobe-deep-dir"
        );
    }

    /// The tmp root itself has an empty relative path, so it is the bare prefix
    /// with no trailing separator. Verified on a real run: `/tmp` → `-tmp`.
    #[test]
    fn tmp_root_itself_encodes_to_bare_tmp() {
        assert_eq!(
            encode_cwd(
                Path::new("/tmp"),
                Path::new("/home/eben"),
                Path::new("/tmp")
            ),
            "-tmp"
        );
    }

    /// Anything outside home and tmp falls back to omp's legacy absolute form.
    #[test]
    fn absolute_scope_uses_the_legacy_double_dash_form() {
        assert_eq!(
            encode_cwd(
                Path::new("/srv/code/x"),
                Path::new("/home/eben"),
                Path::new("/tmp")
            ),
            "--srv-code-x--"
        );
    }

    /// omp classifies home before tmp, so a home that lives under the tmp root
    /// (containers, some CI images) still takes the home branch. Getting this
    /// backwards would send every session lookup to the wrong directory on
    /// exactly those machines.
    #[test]
    fn home_wins_when_home_is_itself_under_the_tmp_root() {
        assert_eq!(
            encode_cwd(
                Path::new("/tmp/home/ci/proj"),
                Path::new("/tmp/home/ci"),
                Path::new("/tmp")
            ),
            "-proj"
        );
    }

    /// The newest .jsonl in the encoded directory wins, and non-jsonl files are
    /// ignored.
    ///
    /// Both temp dirs live under the system temp root, so `work` takes
    /// `encode_cwd`'s **tmp** branch here (it is not under the fake `$HOME`).
    /// That is why the expected directory name is computed through `encode_cwd`
    /// rather than hardcoded — it stays correct whichever branch applies.
    #[test]
    fn locate_picks_the_newest_jsonl_in_the_encoded_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let canon_home = std::fs::canonicalize(home.path()).unwrap();
        let tmp = std::env::temp_dir();
        let tmp = std::fs::canonicalize(&tmp).unwrap_or(tmp);
        let encoded = encode_cwd(&abs, &canon_home, &tmp);
        let dir = home.path().join(".omp/agent/sessions").join(&encoded);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignore me").unwrap();
        // Explicit mtimes rather than write order plus a sleep: a sleep short
        // enough to keep the suite fast can land inside a coarse filesystem's
        // timestamp granularity, which would make this assert nothing.
        seed_jsonl(&dir, "1770000000_old.jsonl", 1_770_000_000);
        seed_jsonl(&dir, "1770000001_new.jsonl", 1_770_000_100);

        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        assert_eq!(
            locate_session_file(work.path())
                .unwrap()
                .file_name()
                .unwrap(),
            "1770000001_new.jsonl"
        );
    }

    /// Write a session file with an explicit mtime, so ordering assertions do
    /// not depend on the filesystem's timestamp granularity.
    fn seed_jsonl(dir: &std::path::Path, name: &str, unix_secs: u64) {
        let path = dir.join(name);
        std::fs::write(&path, "{}").unwrap();
        let when = std::time::UNIX_EPOCH + std::time::Duration::from_secs(unix_secs);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    /// Two sessions can share an mtime — coarse filesystem timestamp
    /// granularity, or two writes in one tick — and `read_dir` order is
    /// unspecified, so without a tie-break which one we tail would vary between
    /// runs.
    ///
    /// Driven directly rather than through the filesystem on purpose: a
    /// directory-backed test only ever observes the one order the OS happens to
    /// yield, so it passes with or without the tie-break and proves nothing.
    /// Feeding both permutations is what actually demonstrates
    /// order-independence.
    #[test]
    fn newest_breaks_equal_mtimes_by_filename_in_either_input_order() {
        let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_770_000_000);
        let earlier = (
            t,
            std::ffi::OsString::from("2026-08-21T13-40-02-005Z_aaaa.jsonl"),
            PathBuf::from("/s/2026-08-21T13-40-02-005Z_aaaa.jsonl"),
        );
        let later = (
            t,
            std::ffi::OsString::from("2026-08-21T13-55-11-900Z_bbbb.jsonl"),
            PathBuf::from("/s/2026-08-21T13-55-11-900Z_bbbb.jsonl"),
        );
        for order in [
            vec![earlier.clone(), later.clone()],
            vec![later.clone(), earlier.clone()],
        ] {
            assert_eq!(
                newest_by_mtime_then_name(order).unwrap(),
                later.2,
                "equal mtimes must resolve by filename regardless of input order"
            );
        }
    }

    /// A newer mtime still wins outright, even when its name sorts lower — the
    /// tie-break must not become the primary key.
    #[test]
    fn newest_prefers_mtime_over_filename() {
        let older = (
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_770_000_000),
            std::ffi::OsString::from("zzzz.jsonl"),
            PathBuf::from("/s/zzzz.jsonl"),
        );
        let newer = (
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_770_000_100),
            std::ffi::OsString::from("aaaa.jsonl"),
            PathBuf::from("/s/aaaa.jsonl"),
        );
        assert_eq!(
            newest_by_mtime_then_name(vec![older, newer.clone()]).unwrap(),
            newer.2
        );
    }

    #[test]
    fn locate_returns_none_when_the_worktree_has_no_session_dir() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        assert!(locate_session_file(work.path()).is_none());
    }

    /// The load-bearing bet of this module: omp's JSONL is pi's schema, so
    /// sessionx's pi parser reads it. This replays a REAL omp session capture
    /// (tests/fixtures/omp-session.jsonl, produced by omp v17.4.0) rather than
    /// a hand-written approximation, so a schema divergence fails here instead
    /// of silently blanking the dashboard.
    #[test]
    fn omp_jsonl_parses_through_the_pi_parser() {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/omp-session.jsonl");
        let update = tail_session(&fixture, 0).expect("omp jsonl must parse");
        assert!(update.new_offset > 0, "parser consumed nothing");
        assert!(
            !update.events.is_empty(),
            "expected parsed events from a real session"
        );
        // NOT `first_user_text`: the shared pi parser never populates that
        // field (only the claude and codex parsers do), so pi workspaces have
        // the same gap. What proves the user line parsed is the event it emits.
        assert!(
            update
                .events
                .iter()
                .any(|e| matches!(e.kind, crate::activity::events::EventKind::UserMessage)),
            "the user prompt must parse into a UserMessage event: {:?}",
            update.events
        );
        assert!(
            update.last_assistant_text.is_some(),
            "assistant text must be recovered"
        );
        assert!(
            !update.tool_use_starts.is_empty(),
            "omp's toolCall parts must be recognised as tool starts"
        );
        let names: Vec<&str> = update
            .tool_use_starts
            .iter()
            .map(|(_, name, _)| name.as_str())
            .collect();
        assert!(
            names.contains(&"bash"),
            "omp uses pi's lowercase tool names: {names:?}"
        );
    }

    /// The pre-17.x absolute name omp migrates only on its own first access. A
    /// worktree whose history predates that migration must still be findable.
    #[test]
    fn locate_falls_back_to_the_legacy_absolute_dir_name() {
        let home = tempfile::TempDir::new().unwrap();
        let work = tempfile::TempDir::new().unwrap();
        let abs = std::fs::canonicalize(work.path()).unwrap();
        let dir = home
            .path()
            .join(".omp/agent/sessions")
            .join(legacy_dir_name(&abs));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("1770000000_legacy.jsonl"), "{}").unwrap();

        let mut env = crate::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        assert_eq!(
            locate_session_file(work.path())
                .unwrap()
                .file_name()
                .unwrap(),
            "1770000000_legacy.jsonl"
        );
    }
}
