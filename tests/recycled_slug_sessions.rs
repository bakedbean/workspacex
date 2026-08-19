//! Regression: a workspace created on a recycled slug must not inherit the
//! previous occupant's agent conversation.
//!
//! wsx derives a worktree path from `<base>/<repo>/<slug>`, and archiving a
//! workspace frees its slug. Draw that slug again for the same repo — by the
//! name generator or by an explicit `--name` — and the new workspace lands on
//! the byte-identical path. Claude/Pi/Codex index their sessions by that path
//! and their indexes outlive the workspace, so path alone cannot answer "is
//! this conversation mine?".
//!
//! Observed in the field: `ssk-web/phantom-fern` was archived, the slug came
//! up again a week later, and the fresh workspace spawned with `--continue`
//! straight into the previous occupant's week-old conversation.
//!
//! These tests drive the real `git::create_worktree` / `git::remove_worktree`
//! and the real detector, so they cover the snapshot's whole lifecycle rather
//! than any one function's view of it. Each holds an `EnvGuard` for its whole
//! body: the snapshot is taken *during* `create_worktree` and reads `$HOME`,
//! so a guard scoped to the assertion alone would snapshot the real home.

use std::path::Path;
use std::process::Command as StdCmd;
use tempfile::TempDir;
use wsx::pty::session::{AgentKind, has_prior_session_for};

fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.email", "t@e.com"][..],
        &["config", "user.name", "T"][..],
        &["commit", "--allow-empty", "-q", "-m", "init"][..],
    ] {
        assert!(
            StdCmd::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap()
                .success(),
            "git {args:?} failed"
        );
    }
    dir
}

/// Write a Claude session JSONL into the index Claude would use for `worktree`.
fn seed_claude_session(home: &Path, worktree: &Path, id: &str) -> std::path::PathBuf {
    let abs = std::fs::canonicalize(worktree).unwrap();
    let encoded = wsx::activity::events::encode_cwd(&abs);
    let dir = home.join(".claude/projects").join(encoded);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{id}.jsonl"));
    std::fs::write(&file, "{}\n").unwrap();
    file
}

/// The snapshot recorded for a worktree, for assertion messages. The verdict
/// alone says nothing about *why* a session was or wasn't treated as the
/// worktree's own; the recorded names do.
fn snapshot_of(repo: &Path, worktree_name: &str) -> String {
    let marker = repo
        .join(".git/worktrees")
        .join(worktree_name)
        .join("info/wsx-worktree-sessions");
    match std::fs::read_to_string(&marker) {
        Ok(b) if b.trim().is_empty() => "snapshot: <empty>".to_string(),
        Ok(b) => format!(
            "snapshot: [{}]",
            b.split_whitespace().collect::<Vec<_>>().join(", ")
        ),
        Err(e) => format!("snapshot: <unreadable: {e}>"),
    }
}

fn delete_branch(repo: &Path, branch: &str) {
    assert!(
        StdCmd::new("git")
            .current_dir(repo)
            .args(["branch", "-D", branch])
            .status()
            .unwrap()
            .success()
    );
}

#[tokio::test]
async fn recycled_slug_does_not_inherit_the_previous_occupants_session() {
    let home = TempDir::new().unwrap();
    let mut env = wsx::test_support::EnvGuard::new();
    env.set("HOME", home.path());

    let repo = init_repo();
    let base = TempDir::new().unwrap();
    let wt = base.path().join("phantom-fern");

    // --- First life: workspace runs, Claude persists a conversation. ---
    wsx::git::create_worktree(repo.path(), "eben/phantom-fern", None, &wt)
        .await
        .unwrap();
    let stale = seed_claude_session(home.path(), &wt, "656c166a-911b-4375-9db9-007b8456f3e3");
    assert!(
        has_prior_session_for(&wt, AgentKind::Claude),
        "the workspace that owns this session must resume it — {}",
        snapshot_of(repo.path(), "phantom-fern")
    );

    // --- Archive. The worktree goes; the session index under ~/.claude stays. ---
    wsx::git::remove_worktree(repo.path(), &wt).await.unwrap();
    delete_branch(repo.path(), "eben/phantom-fern");

    // --- Second life: same slug, same repo, byte-identical path. ---
    wsx::git::create_worktree(repo.path(), "eben/phantom-fern", None, &wt)
        .await
        .unwrap();
    assert!(
        stale.exists(),
        "precondition: the old session index survives archival — that is the whole hazard"
    );
    assert!(
        !has_prior_session_for(&wt, AgentKind::Claude),
        "a workspace on a recycled slug must spawn fresh, not --continue into \
         the previous occupant's conversation — {}",
        snapshot_of(repo.path(), "phantom-fern")
    );
}

#[tokio::test]
async fn a_rapidly_recycled_slug_is_still_not_inherited() {
    // The verdict must not depend on how much time passed between the two
    // lives. An earlier design compared the session's file timestamp against
    // the worktree's creation instant, which needed slack to absorb clock
    // skew — and any slack is a window in which a fast archive-and-recreate
    // slips through. Naming the pre-existing sessions outright has no window:
    // this test recycles the slug as fast as the filesystem allows.
    let home = TempDir::new().unwrap();
    let mut env = wsx::test_support::EnvGuard::new();
    env.set("HOME", home.path());

    let repo = init_repo();
    let base = TempDir::new().unwrap();
    let wt = base.path().join("rapid");

    wsx::git::create_worktree(repo.path(), "eben/rapid", None, &wt)
        .await
        .unwrap();
    seed_claude_session(home.path(), &wt, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    wsx::git::remove_worktree(repo.path(), &wt).await.unwrap();
    delete_branch(repo.path(), "eben/rapid");
    wsx::git::create_worktree(repo.path(), "eben/rapid", None, &wt)
        .await
        .unwrap();

    assert!(
        !has_prior_session_for(&wt, AgentKind::Claude),
        "recycling a slug within milliseconds must still spawn fresh — {}",
        snapshot_of(repo.path(), "rapid")
    );
}

#[tokio::test]
async fn a_workspaces_own_session_still_resumes() {
    // The gate must not cost normal resume: kill the TUI, reopen, get your
    // conversation back.
    let home = TempDir::new().unwrap();
    let mut env = wsx::test_support::EnvGuard::new();
    env.set("HOME", home.path());

    let repo = init_repo();
    let base = TempDir::new().unwrap();
    let wt = base.path().join("ancient-olive");

    wsx::git::create_worktree(repo.path(), "eben/ancient-olive", None, &wt)
        .await
        .unwrap();
    seed_claude_session(home.path(), &wt, "11111111-2222-3333-4444-555555555555");

    assert!(
        has_prior_session_for(&wt, AgentKind::Claude),
        "a session started inside this worktree's lifetime must still resume — {}",
        snapshot_of(repo.path(), "ancient-olive")
    );
}

#[tokio::test]
async fn an_adopted_worktree_keeps_its_pre_existing_session() {
    // `data::workspace::import_existing` adopts a worktree that already lived
    // on disk, so its sessions legitimately predate the registry row. Such a
    // worktree never goes through `create_worktree` and so carries no
    // snapshot — which must mean "no gate", not "reject everything".
    let home = TempDir::new().unwrap();
    let mut env = wsx::test_support::EnvGuard::new();
    env.set("HOME", home.path());

    let repo = init_repo();
    let base = TempDir::new().unwrap();
    let wt = base.path().join("hand-rolled");

    // Made by hand, the way a user's own `git worktree add` would.
    assert!(
        StdCmd::new("git")
            .current_dir(repo.path())
            .args(["worktree", "add", "-q", "-b", "adopted"])
            .arg(&wt)
            .status()
            .unwrap()
            .success()
    );
    seed_claude_session(home.path(), &wt, "99999999-8888-7777-6666-555555555555");

    let marker = repo
        .path()
        .join(".git/worktrees/hand-rolled/info/wsx-worktree-sessions");
    assert!(
        !marker.exists(),
        "precondition: a worktree wsx did not create carries no snapshot"
    );
    assert!(
        has_prior_session_for(&wt, AgentKind::Claude),
        "an unsnapshotted worktree must keep the original ungated behaviour"
    );
}
