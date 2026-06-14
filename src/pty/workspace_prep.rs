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
use std::path::Path;

pub(crate) const HERMES_BLOCK_BEGIN: &str = "<!-- BEGIN wsx-managed -->";
pub(crate) const HERMES_BLOCK_END: &str = "<!-- END wsx-managed -->";

/// Marker prefixing `CLAUDE.md` content copied into a freshly-created
/// `AGENTS.md`, so a reader can tell where it came from.
pub(crate) const CLAUDE_PROVENANCE_COMMENT: &str = "<!-- Copied from CLAUDE.md by wsx -->";

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
pub(crate) fn write_agents_md_section(cwd: &Path, content: Option<&str>) {
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
pub(crate) fn ensure_git_exclude(worktree: &Path, name: &str) {
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
