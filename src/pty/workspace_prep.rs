//! Workspace preparation for Hermes/Codex spawns.
//!
//! Hermes and Codex read project instructions from `AGENTS.md` (rather than
//! Claude's native `CLAUDE.md` / `--append-system-prompt`), so before spawning
//! them wsx rewrites a `BEGIN/END wsx-managed` block in that file, hides it from
//! `git status`, and (for Hermes) records a spawn-timestamp marker for session
//! detection. Pure side-effecting helpers over a worktree path + SpawnMode;
//! `prepare_*_workspace` are re-exported from `pty::session` for the spawn path.

use crate::pty::command::compose_injected_prompt;
use crate::pty::session::{SpawnMode, resolve_gitdir};
use crate::pty::session_detect::{read_hermes_spawn_marker, write_hermes_spawn_marker};
// `RenameContext` is only constructed by this module's co-located tests.
#[cfg(test)]
use crate::pty::session::RenameContext;
use std::path::Path;

const HERMES_BLOCK_BEGIN: &str = "<!-- BEGIN wsx-managed -->";
const HERMES_BLOCK_END: &str = "<!-- END wsx-managed -->";

/// Marker prefixing `CLAUDE.md` content copied into a freshly-created
/// `AGENTS.md`, so a reader can tell where it came from.
const CLAUDE_PROVENANCE_COMMENT: &str = "<!-- Copied from CLAUDE.md by wsx -->";

/// Read a repo's root `CLAUDE.md`, returning its contents only if the file
/// exists and holds non-whitespace text. Used to seed a newly-created
/// `AGENTS.md` so Hermes/Codex get the same project instructions Claude reads
/// natively. Best-effort: any IO error yields `None`.
fn read_claude_md(cwd: &Path) -> Option<String> {
    let contents = std::fs::read_to_string(cwd.join("CLAUDE.md")).ok()?;
    if contents.trim().is_empty() {
        return None;
    }
    Some(contents)
}

/// Rewrite the wsx-managed section of `AGENTS.md` in `cwd`.
///
/// Strips any existing `BEGIN/END wsx-managed` block, then appends a new
/// block with `content` if Some, or writes back just the stripped content if
/// None. Skips the write entirely if the result equals the existing file.
///
/// Best-effort: any IO error is silently swallowed.
fn write_agents_md_section(cwd: &Path, content: Option<&str>) {
    let path = cwd.join("AGENTS.md");
    // Capture existence before reading: when wsx creates AGENTS.md fresh we
    // seed it with the repo's CLAUDE.md (if any) so Hermes/Codex get the same
    // project instructions Claude reads natively. Checking emptiness after the
    // read wouldn't distinguish a missing file from an empty one.
    let file_existed = path.exists();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let stripped = strip_wsx_block(&existing);
    let new = match content {
        Some(c) => {
            let mut s = stripped.into_owned();
            if !s.is_empty() && !s.ends_with('\n') {
                s.push('\n');
            }
            if !s.is_empty() {
                s.push('\n');
            }
            s.push_str(HERMES_BLOCK_BEGIN);
            s.push('\n');
            s.push_str(c);
            if !c.ends_with('\n') {
                s.push('\n');
            }
            s.push_str(HERMES_BLOCK_END);
            s.push('\n');
            // On true first creation, append the repo's CLAUDE.md after the
            // wsx block. One-time only — once the file exists, later spawns
            // preserve this content as ordinary non-wsx text and never re-copy.
            if !file_existed {
                if let Some(claude) = read_claude_md(cwd) {
                    s.push('\n');
                    s.push_str(CLAUDE_PROVENANCE_COMMENT);
                    s.push('\n');
                    s.push_str(&claude);
                    if !claude.ends_with('\n') {
                        s.push('\n');
                    }
                }
            }
            s
        }
        None => stripped.into_owned(),
    };

    if new == existing {
        return;
    }
    if new.is_empty() && !path.exists() {
        return;
    }
    let _ = std::fs::write(&path, new);
}

/// Remove a `BEGIN/END wsx-managed` block (and the surrounding blank lines
/// it produced when we wrote it) from `source`, returning a `Cow` so we
/// can avoid allocation in the common no-block path.
fn strip_wsx_block(source: &str) -> std::borrow::Cow<'_, str> {
    let Some(begin) = source.find(HERMES_BLOCK_BEGIN) else {
        return std::borrow::Cow::Borrowed(source);
    };
    let Some(end_rel) = source[begin..].find(HERMES_BLOCK_END) else {
        // Malformed (BEGIN without END) — strip from BEGIN onwards.
        return std::borrow::Cow::Owned(source[..begin].trim_end_matches('\n').to_string());
    };
    let end = begin + end_rel + HERMES_BLOCK_END.len();
    // Consume one trailing newline after END if present, so successive
    // strip/append cycles don't grow blank-line padding.
    let mut tail_start = end;
    if source.as_bytes().get(tail_start) == Some(&b'\n') {
        tail_start += 1;
    }
    // Trim trailing newlines from the prefix so we don't accumulate blank lines.
    let prefix = source[..begin].trim_end_matches('\n');
    let suffix = &source[tail_start..];
    let mut combined = String::with_capacity(prefix.len() + suffix.len() + 1);
    combined.push_str(prefix);
    if !prefix.is_empty() && !suffix.is_empty() {
        combined.push('\n');
    }
    combined.push_str(suffix);
    std::borrow::Cow::Owned(combined)
}

/// Append `name` to the gitdir's `info/exclude` if not already present.
///
/// `<worktree>/.git` may be either a directory (normal clone) or a file
/// containing `gitdir: <path>` (git worktree). We follow the file to the
/// real gitdir before writing.
///
/// Best-effort: silently no-ops on any IO/parse error or if `.git/` is
/// absent. `info/exclude` is per-gitdir-local and never committed.
fn ensure_git_exclude(worktree: &Path, name: &str) {
    let dot_git = worktree.join(".git");
    let gitdir = match resolve_gitdir(&dot_git, worktree) {
        Some(p) => p,
        None => return,
    };
    let info_dir = gitdir.join("info");
    if !info_dir.exists() && std::fs::create_dir_all(&info_dir).is_err() {
        return;
    }
    let exclude_path = info_dir.join("exclude");
    let existing = std::fs::read_to_string(&exclude_path).unwrap_or_default();
    if existing.lines().any(|l| l == name) {
        return;
    }
    let mut new = existing;
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(name);
    new.push('\n');
    let _ = std::fs::write(&exclude_path, new);
}

/// Prepare a worktree for a Hermes spawn: rewrite the wsx-managed block in
/// AGENTS.md (creating the file if needed), ensure the file is hidden
/// from `git status` via `.git/info/exclude`, and write the spawn-timestamp
/// marker used for session detection.
///
/// The marker is **one-time-write**: it records the timestamp of the *first*
/// wsx spawn for this worktree. On subsequent re-attaches (Continue mode) the
/// existing marker is preserved so the lookup query
/// `WHERE started_at >= marker_ts - 2.0` continues to find the session that
/// was created when the workspace was first opened. Overwriting on each spawn
/// would reset the timestamp to "now" and silently lose session history.
///
/// Best-effort: all IO errors are swallowed. Hermes will still launch if
/// these side effects fail; the user just loses the rename hint and session
/// detection falls back to None.
pub(crate) fn prepare_hermes_workspace(cwd: &Path, mode: &SpawnMode) {
    let injected = compose_injected_prompt(mode);
    let had_content = injected.is_some();
    write_agents_md_section(cwd, injected.as_deref());
    if had_content {
        ensure_git_exclude(cwd, "AGENTS.md");
    }
    // Marker is one-time-write: only write if no marker exists yet.
    // This preserves the original spawn timestamp across re-attaches so the
    // session-lookup query can still find the original Hermes session.
    if read_hermes_spawn_marker(cwd).is_none() {
        write_hermes_spawn_marker(cwd);
    }
}

/// Prepare a worktree for a Codex spawn: inject the wsx-managed instruction
/// block into AGENTS.md (Codex reads project instructions from there, like
/// Hermes) and hide the file from `git status`. Codex needs NO spawn-timestamp
/// marker — session detection is cwd-in-file, not marker-based.
pub(crate) fn prepare_codex_workspace(cwd: &Path, mode: &SpawnMode) {
    #[cfg(not(test))]
    crate::agent::codex_commands::sync_claude_commands_for_codex();
    let injected = compose_injected_prompt(mode);
    let had_content = injected.is_some();
    write_agents_md_section(cwd, injected.as_deref());
    if had_content {
        ensure_git_exclude(cwd, "AGENTS.md");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod hermes_git_exclude {
        use std::fs;
        use std::io::Read;

        fn init_gitdir(dir: &std::path::Path) {
            fs::create_dir_all(dir.join(".git/info")).unwrap();
        }

        fn read(path: &std::path::Path) -> String {
            let mut s = String::new();
            fs::File::open(path)
                .unwrap()
                .read_to_string(&mut s)
                .unwrap();
            s
        }

        #[test]
        fn creates_exclude_line_when_absent() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            let contents = read(&tmp.path().join(".git/info/exclude"));
            assert!(
                contents.lines().any(|l| l == "AGENTS.md"),
                "expected AGENTS.md line in {contents:?}"
            );
        }

        #[test]
        fn idempotent_when_entry_already_present() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            let exclude = tmp.path().join(".git/info/exclude");
            fs::write(&exclude, "AGENTS.md\n").unwrap();
            let before = read(&exclude);
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            let after = read(&exclude);
            assert_eq!(before, after);
        }

        #[test]
        fn handles_missing_info_dir() {
            let tmp = tempfile::tempdir().unwrap();
            fs::create_dir_all(tmp.path().join(".git")).unwrap();
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            let contents = read(&tmp.path().join(".git/info/exclude"));
            assert!(contents.contains("AGENTS.md"));
        }

        #[test]
        fn no_op_when_gitdir_absent() {
            let tmp = tempfile::tempdir().unwrap();
            // No .git/ at all. Must not panic.
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
            assert!(!tmp.path().join(".git").exists());
        }

        #[test]
        fn follows_worktree_style_git_file_with_absolute_gitdir() {
            let tmp = tempfile::tempdir().unwrap();
            // External gitdir lives outside the worktree
            let external = tempfile::tempdir().unwrap();
            let gitdir = external.path().join("worktrees/feature-x");
            fs::create_dir_all(&gitdir).unwrap();
            // worktree/.git is a FILE pointing at the external gitdir
            fs::write(
                tmp.path().join(".git"),
                format!("gitdir: {}\n", gitdir.display()),
            )
            .unwrap();

            super::ensure_git_exclude(tmp.path(), "AGENTS.md");

            let exclude = gitdir.join("info/exclude");
            let contents = read(&exclude);
            assert!(
                contents.contains("AGENTS.md"),
                "expected AGENTS.md in {}: {contents:?}",
                exclude.display()
            );
        }

        #[test]
        fn follows_worktree_style_git_file_with_relative_gitdir() {
            let tmp = tempfile::tempdir().unwrap();
            // Relative gitdir resolved against the worktree path
            let rel = "external-gitdir";
            fs::create_dir_all(tmp.path().join(rel)).unwrap();
            fs::write(tmp.path().join(".git"), format!("gitdir: {rel}\n")).unwrap();

            super::ensure_git_exclude(tmp.path(), "AGENTS.md");

            let exclude = tmp.path().join(rel).join("info/exclude");
            let contents = read(&exclude);
            assert!(contents.contains("AGENTS.md"));
        }

        #[test]
        fn returns_silently_when_git_file_unparseable() {
            let tmp = tempfile::tempdir().unwrap();
            fs::write(tmp.path().join(".git"), "not a valid git pointer\n").unwrap();
            // Must not panic and must not create any files
            super::ensure_git_exclude(tmp.path(), "AGENTS.md");
        }
    }

    mod hermes_agents_md {
        use std::fs;

        #[test]
        fn creates_file_with_fenced_block_when_absent() {
            let tmp = tempfile::tempdir().unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                contents.contains(super::HERMES_BLOCK_BEGIN),
                "missing BEGIN marker: {contents:?}"
            );
            assert!(
                contents.contains(super::HERMES_BLOCK_END),
                "missing END marker: {contents:?}"
            );
            assert!(contents.contains("inject me"));
        }

        #[test]
        fn preserves_user_content_outside_wsx_block() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "# User notes\n\nKeep me.\n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(&path).unwrap();
            assert!(contents.contains("# User notes"));
            assert!(contents.contains("Keep me."));
            assert!(contents.contains("inject me"));
        }

        #[test]
        fn replaces_existing_wsx_block_idempotently() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            super::write_agents_md_section(tmp.path(), Some("first"));
            let after_first = fs::read_to_string(&path).unwrap();
            super::write_agents_md_section(tmp.path(), Some("first"));
            let after_second = fs::read_to_string(&path).unwrap();
            assert_eq!(
                after_first, after_second,
                "second write should be byte-identical"
            );
        }

        #[test]
        fn replacing_block_with_new_content_replaces_in_place() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            super::write_agents_md_section(tmp.path(), Some("first"));
            super::write_agents_md_section(tmp.path(), Some("second"));
            let contents = fs::read_to_string(&path).unwrap();
            assert!(contents.contains("second"));
            assert!(!contents.contains("first"), "old content should be removed");
        }

        #[test]
        fn strips_block_when_content_is_none() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "user content\n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("temp"));
            super::write_agents_md_section(tmp.path(), None);
            let contents = fs::read_to_string(&path).unwrap();
            assert!(contents.contains("user content"));
            assert!(!contents.contains(super::HERMES_BLOCK_BEGIN));
            assert!(!contents.contains("temp"));
        }

        #[test]
        fn no_write_when_content_is_none_and_no_existing_block() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            // Don't create the file at all.
            super::write_agents_md_section(tmp.path(), None);
            assert!(
                !path.exists(),
                "should not create AGENTS.md just to strip nothing"
            );
        }

        #[test]
        fn survives_unreadable_agents_md() {
            use std::os::unix::fs::PermissionsExt;
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "untouchable\n").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
            // Must not panic.
            super::write_agents_md_section(tmp.path(), Some("inject"));
            // Restore perms so tempdir cleanup works.
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        }

        #[test]
        fn copies_claude_md_after_block_on_fresh_create() {
            let tmp = tempfile::tempdir().unwrap();
            fs::write(
                tmp.path().join("CLAUDE.md"),
                "# Project rules\n\nBe nice.\n",
            )
            .unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "missing provenance comment: {contents:?}"
            );
            assert!(
                contents.contains("Be nice."),
                "missing CLAUDE.md content: {contents:?}"
            );
            // CLAUDE.md content must come AFTER the wsx-managed block.
            let end_idx = contents.find(super::HERMES_BLOCK_END).unwrap();
            let prov_idx = contents.find(super::CLAUDE_PROVENANCE_COMMENT).unwrap();
            assert!(
                prov_idx > end_idx,
                "CLAUDE.md content must follow the wsx block: {contents:?}"
            );
        }

        #[test]
        fn no_claude_md_means_no_copy() {
            let tmp = tempfile::tempdir().unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                !contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "should not add provenance comment when no CLAUDE.md: {contents:?}"
            );
        }

        #[test]
        fn does_not_copy_claude_md_when_agents_md_already_exists() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("AGENTS.md");
            fs::write(&path, "# Existing notes\n").unwrap();
            fs::write(tmp.path().join("CLAUDE.md"), "Be nice.\n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(&path).unwrap();
            assert!(
                !contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "must not copy CLAUDE.md when AGENTS.md pre-existed: {contents:?}"
            );
            assert!(
                !contents.contains("Be nice."),
                "must not copy CLAUDE.md content: {contents:?}"
            );
        }

        #[test]
        fn blank_claude_md_is_not_copied() {
            let tmp = tempfile::tempdir().unwrap();
            fs::write(tmp.path().join("CLAUDE.md"), "   \n\n  \n").unwrap();
            super::write_agents_md_section(tmp.path(), Some("inject me"));
            let contents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(
                !contents.contains(super::CLAUDE_PROVENANCE_COMMENT),
                "blank CLAUDE.md should not be copied: {contents:?}"
            );
        }
    }

    mod hermes_prepare_workspace {
        use std::fs;

        fn init_gitdir(dir: &std::path::Path) {
            fs::create_dir_all(dir.join(".git/info")).unwrap();
        }

        #[test]
        fn fresh_with_rename_writes_agents_md_and_exclude() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            let mode = super::SpawnMode::Fresh {
                rename_ctx: Some(super::RenameContext {
                    current_branch: "wsx/bold-fern".into(),
                    branch_prefix: "wsx".into(),
                    repo_name: "myrepo".into(),
                    current_slug: "bold-fern".into(),
                }),
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &mode);

            let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap();
            assert!(agents.contains("<!-- BEGIN wsx-managed -->"));
            assert!(agents.contains("wsx workspace rename 'myrepo' 'bold-fern'"));

            let exclude = fs::read_to_string(tmp.path().join(".git/info/exclude")).unwrap();
            assert!(exclude.lines().any(|l| l == "AGENTS.md"));
        }

        #[test]
        fn continue_without_custom_instructions_strips_block() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            // First prepare a Fresh+rename state.
            let fresh = super::SpawnMode::Fresh {
                rename_ctx: Some(super::RenameContext {
                    current_branch: "wsx/bold-fern".into(),
                    branch_prefix: "wsx".into(),
                    repo_name: "myrepo".into(),
                    current_slug: "bold-fern".into(),
                }),
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &fresh);
            // Now spawn Continue with nothing to inject.
            let cont = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &cont);
            let agents = fs::read_to_string(tmp.path().join("AGENTS.md")).unwrap_or_default();
            assert!(
                !agents.contains("<!-- BEGIN wsx-managed -->"),
                "wsx block should be removed; got: {agents}"
            );
            assert!(
                !agents.contains("wsx workspace rename"),
                "rename text should be gone; got: {agents}"
            );
        }

        #[test]
        fn no_op_when_continue_no_custom_and_no_existing_agents_md() {
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            let cont = super::SpawnMode::Continue {
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &cont);
            assert!(!tmp.path().join("AGENTS.md").exists());
        }

        #[test]
        fn does_not_overwrite_existing_marker() {
            // Write a marker with a known timestamp, then call prepare_hermes_workspace
            // in Fresh mode. The marker must NOT be overwritten — start_ts stays 1000.0.
            let tmp = tempfile::tempdir().unwrap();
            init_gitdir(tmp.path());
            // Manually write a marker with a specific (old) timestamp.
            std::fs::write(tmp.path().join(".git/info/wsx-hermes-spawn-at"), "1000.0\n").unwrap();
            let fresh_mode = super::SpawnMode::Fresh {
                rename_ctx: None,
                custom_instructions: None,
                doctrine: None,
                additional_dirs: vec![],
                yolo: false,
            };
            super::prepare_hermes_workspace(tmp.path(), &fresh_mode);
            let marker = super::read_hermes_spawn_marker(tmp.path())
                .expect("marker should still exist after prepare");
            assert!(
                (marker.start_ts - 1000.0).abs() < 0.001,
                "start_ts must be preserved; got {}",
                marker.start_ts
            );
        }
    }

    #[test]
    fn prepare_codex_workspace_injects_rename_block_into_agents_md() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let mode = SpawnMode::Fresh {
            rename_ctx: Some(RenameContext {
                current_branch: "prefix/my-slug".to_string(),
                branch_prefix: "prefix".to_string(),
                repo_name: "myrepo".to_string(),
                current_slug: "my-slug".to_string(),
            }),
            custom_instructions: None,
            doctrine: Some("DOCTRINE-MARKER".to_string()),
            additional_dirs: vec![],
            yolo: false,
        };
        prepare_codex_workspace(cwd, &mode);
        let agents = std::fs::read_to_string(cwd.join("AGENTS.md")).unwrap();
        assert!(
            agents.contains("BEGIN wsx-managed"),
            "block markers: {agents}"
        );
        assert!(
            agents.contains("DOCTRINE-MARKER"),
            "doctrine injected: {agents}"
        );
        assert!(
            agents.contains("wsx workspace rename"),
            "rename hint: {agents}"
        );
    }

    #[test]
    fn prepare_codex_workspace_writes_no_hermes_marker() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        std::fs::create_dir_all(cwd.join(".git/info")).unwrap();
        let mode = SpawnMode::Fresh {
            rename_ctx: None,
            custom_instructions: Some("CUSTOM".to_string()),
            doctrine: None,
            additional_dirs: vec![],
            yolo: false,
        };
        prepare_codex_workspace(cwd, &mode);
        // Codex uses cwd-in-file detection, not the Hermes spawn marker.
        assert!(
            !cwd.join(".git/info/wsx-hermes-spawn-at").exists(),
            "codex must not write the hermes spawn marker"
        );
    }
}
