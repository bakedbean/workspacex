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
//! and the real detector, so they cover the marker's whole lifecycle rather
//! than any one function's view of it.

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

/// Age a session file by `days`, so a test can express the real timeline —
/// the reported case sat a week between the two workspaces — instead of
/// leaning on the sub-second gap between two statements.
fn backdate(file: &Path, days: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
    let f = std::fs::File::options().write(true).open(file).unwrap();
    f.set_times(
        std::fs::FileTimes::new()
            .set_accessed(when)
            .set_modified(when),
    )
    .unwrap();
}

/// Fractional Unix epoch seconds recorded in a worktree's epoch marker.
fn read_epoch(repo: &Path, worktree_name: &str) -> f64 {
    let marker = repo
        .join(".git/worktrees")
        .join(worktree_name)
        .join("info/wsx-worktree-epoch");
    std::fs::read_to_string(&marker)
        .unwrap_or_else(|e| panic!("no marker at {}: {e}", marker.display()))
        .trim()
        .parse()
        .unwrap()
}

/// What the filesystem thinks a session file's start instant is, by the same
/// rule the detector uses (birth time, falling back to mtime).
fn session_ts(home: &Path, worktree: &Path, id: &str) -> f64 {
    let abs = std::fs::canonicalize(worktree).unwrap();
    let file = home
        .join(".claude/projects")
        .join(wsx::activity::events::encode_cwd(&abs))
        .join(format!("{id}.jsonl"));
    let md = std::fs::metadata(&file).unwrap();
    md.created()
        .or_else(|_| md.modified())
        .unwrap()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}

/// Assertion context for the "is this session mine?" comparison. Worth
/// printing on every failure: the two timestamps come from different clocks
/// (userspace `SystemTime::now` for the marker, the kernel's coarse clock via
/// the filesystem for the session file), so a bare true/false says nothing
/// about which way they actually landed.
fn timing(repo: &Path, worktree_name: &str, home: &Path, worktree: &Path, id: &str) -> String {
    let epoch = read_epoch(repo, worktree_name);
    let session = session_ts(home, worktree, id);
    format!(
        "worktree epoch={epoch:.6}, session={session:.6}, session-epoch={:.6}s",
        session - epoch
    )
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
    let repo = init_repo();
    let base = TempDir::new().unwrap();
    let wt = base.path().join("phantom-fern");

    // --- First life: workspace runs, Claude persists a conversation. ---
    wsx::git::create_worktree(repo.path(), "eben/phantom-fern", None, &wt)
        .await
        .unwrap();
    let stale = seed_claude_session(home.path(), &wt, "656c166a-911b-4375-9db9-007b8456f3e3");

    {
        let mut env = wsx::test_support::EnvGuard::new();
        env.set("HOME", home.path());
        assert!(
            has_prior_session_for(&wt, AgentKind::Claude),
            "the workspace that owns this session must resume it — {}",
            timing(
                repo.path(),
                "phantom-fern",
                home.path(),
                &wt,
                "656c166a-911b-4375-9db9-007b8456f3e3"
            )
        );
    }

    // --- Archive. The worktree goes; the session index under ~/.claude stays. ---
    wsx::git::remove_worktree(repo.path(), &wt).await.unwrap();
    delete_branch(repo.path(), "eben/phantom-fern");
    // A week passes, as it did in the reported case. Ageing the file is what
    // makes this a stale-session test rather than a same-instant one.
    backdate(&stale, 7);

    // --- Second life: same slug, same repo, byte-identical path. ---
    wsx::git::create_worktree(repo.path(), "eben/phantom-fern", None, &wt)
        .await
        .unwrap();

    assert!(
        stale.exists(),
        "precondition: the old session index survives archival — that is the whole hazard"
    );

    let mut env = wsx::test_support::EnvGuard::new();
    env.set("HOME", home.path());
    assert!(
        !has_prior_session_for(&wt, AgentKind::Claude),
        "a workspace on a recycled slug must spawn fresh, not --continue into \
         the previous occupant's conversation — {}",
        timing(
            repo.path(),
            "phantom-fern",
            home.path(),
            &wt,
            "656c166a-911b-4375-9db9-007b8456f3e3"
        )
    );
}

#[tokio::test]
async fn a_workspaces_own_session_still_resumes_after_the_fix() {
    // The gate must not cost normal resume: kill the TUI, reopen, get your
    // conversation back.
    let home = TempDir::new().unwrap();
    let repo = init_repo();
    let base = TempDir::new().unwrap();
    let wt = base.path().join("ancient-olive");

    wsx::git::create_worktree(repo.path(), "eben/ancient-olive", None, &wt)
        .await
        .unwrap();
    let _ = seed_claude_session(home.path(), &wt, "11111111-2222-3333-4444-555555555555");

    let mut env = wsx::test_support::EnvGuard::new();
    env.set("HOME", home.path());
    assert!(
        has_prior_session_for(&wt, AgentKind::Claude),
        "a session started inside this worktree's lifetime must still resume — {}",
        timing(
            repo.path(),
            "ancient-olive",
            home.path(),
            &wt,
            "11111111-2222-3333-4444-555555555555"
        )
    );
}

#[tokio::test]
async fn an_adopted_worktree_keeps_its_pre_existing_session() {
    // `data::workspace::import_existing` adopts a worktree that already lived
    // on disk, so its sessions legitimately predate the registry row. Such a
    // worktree never goes through `create_worktree` and so carries no epoch
    // marker — which must mean "no gate", not "reject everything".
    let home = TempDir::new().unwrap();
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
    let _ = seed_claude_session(home.path(), &wt, "99999999-8888-7777-6666-555555555555");

    let marker = repo
        .path()
        .join(".git/worktrees/hand-rolled/info/wsx-worktree-epoch");
    assert!(
        !marker.exists(),
        "precondition: a worktree wsx did not create carries no epoch"
    );

    let mut env = wsx::test_support::EnvGuard::new();
    env.set("HOME", home.path());
    assert!(
        has_prior_session_for(&wt, AgentKind::Claude),
        "an unmarked worktree must keep the original ungated behaviour"
    );
}
