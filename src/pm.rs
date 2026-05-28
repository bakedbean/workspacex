//! Project Manager pane: dossier file + PM Claude Code session orchestration.

use crate::error::{Error, Result};
use crate::store::{Store, WorkspaceState};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
struct WorkspacesDossier {
    generated_at_epoch_seconds: i64,
    repos: Vec<RepoEntry>,
}

#[derive(Debug, Serialize)]
struct RepoEntry {
    name: String,
    path: PathBuf,
    workspaces: Vec<WorkspaceEntry>,
}

#[derive(Debug, Serialize)]
struct WorkspaceEntry {
    name: String,
    branch: String,
    worktree_path: PathBuf,
    session_log_dir: PathBuf,
    git: GitCounts,
}

#[derive(Debug, Serialize, Default)]
struct GitCounts {
    modified: usize,
    untracked: usize,
    ahead: usize,
    behind: usize,
}

/// Write the workspaces dossier file atomically (tempfile + rename).
///
/// Only `WorkspaceState::Ready` workspaces are included. Repos with no
/// Ready workspaces appear with an empty `workspaces` array.
pub fn write_workspaces_json(store: &Store, target: &Path) -> Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let mut repos = Vec::new();
    for repo in store.repos()? {
        let mut workspaces = Vec::new();
        for ws in store.workspaces(repo.id)? {
            if ws.state != WorkspaceState::Ready {
                continue;
            }
            // Skip workspaces where the agent has never been started: nothing
            // for PM to summarize, and they pollute the list.
            if !crate::pty::session::has_prior_session_for(&ws.worktree_path, ws.agent) {
                continue;
            }
            let session_log_dir = compute_session_log_dir(&ws.worktree_path, ws.agent);
            workspaces.push(WorkspaceEntry {
                name: ws.name,
                branch: ws.branch,
                worktree_path: ws.worktree_path,
                session_log_dir,
                git: GitCounts::default(),
            });
        }
        repos.push(RepoEntry {
            name: repo.name,
            path: repo.path,
            workspaces,
        });
    }

    let dossier = WorkspacesDossier {
        generated_at_epoch_seconds: now,
        repos,
    };

    let payload = serde_json::to_string_pretty(&dossier).map_err(|e| {
        Error::Io(std::io::Error::other(format!(
            "workspaces.json serialize: {e}"
        )))
    })?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = target.with_extension("json.tmp");
    std::fs::write(&tmp, payload)?;
    std::fs::rename(&tmp, target)?;
    Ok(())
}

fn compute_session_log_dir(worktree: &Path, agent: crate::pty::session::AgentKind) -> PathBuf {
    let abs = std::fs::canonicalize(worktree).unwrap_or_else(|_| worktree.to_path_buf());
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    match agent {
        crate::pty::session::AgentKind::Claude => {
            let encoded = crate::events::encode_cwd(&abs);
            home.join(".claude/projects").join(encoded)
        }
        crate::pty::session::AgentKind::Pi => {
            let encoded = crate::pi_events::encode_cwd(&abs);
            home.join(".pi/agent/sessions").join(encoded)
        }
        crate::pty::session::AgentKind::Hermes => {
            // stub — replaced in Task 5
            home.join(".hermes/sessions")
        }
    }
}

/// Initialize the PM working directory. Creates `dir` if needed and
/// runs `git init` inside it so Claude Code is happy. Idempotent — if
/// the directory already contains a git repo, this is a no-op.
pub fn init_pm_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    if dir.join(".git").is_dir() {
        return Ok(());
    }
    let status = std::process::Command::new("git")
        .arg("init")
        .arg("-q")
        .current_dir(dir)
        .status()
        .map_err(|e| Error::Io(std::io::Error::other(format!("git init pm dir: {e}"))))?;
    if !status.success() {
        return Err(Error::Git(format!(
            "git init failed in {} (exit {:?})",
            dir.display(),
            status.code()
        )));
    }
    Ok(())
}

const PM_SYSTEM_PROMPT: &str = "\
You are a project manager for a developer running multiple parallel coding \
workspaces under wsx. Each workspace is a git worktree with its own Claude \
Code session. When asked, inspect active workspaces and report what each \
was created for, where it left off, and what's next.\n\
\n\
Data sources:\n\
  - ./workspaces.json — name, branch, worktree_path, session_log_dir.\n\
  - First user message of earliest *.jsonl in session_log_dir → original ask.\n\
  - Last entries of most recent *.jsonl in session_log_dir → recent activity.\n\
  - `cd <worktree_path> && git status/log/diff` → code state.\n\
\n\
OUTPUT FORMAT (strict, no exceptions):\n\
  - Bullet points ONLY. No headers, no preamble, no closing summary, no prose.\n\
  - One bullet per workspace. Maximum 20 words per bullet.\n\
  - Format: `- <name>: <what it's for>; <where it left off>; <next step>`\n\
    Semicolons separate the three facts. Omit any field you can't determine.\n\
  - Example: `- fix-auth: cookie expiry bug from #42; tests added but failing; debug session token regex`\n\
  - Be ruthless about brevity. The developer reads at a glance, not paragraphs.\n\
  - Do not say \"unclear\" or \"unknown\" — just omit the field.\n\
\n\
workspaces.json is refreshed by the developer on demand. Trust its contents \
over stale memory.";

/// Build the PM system prompt, optionally appending custom instructions.
pub fn pm_system_prompt(custom: Option<&str>) -> String {
    match custom.map(|s| s.trim()).filter(|s| !s.is_empty()) {
        None => PM_SYSTEM_PROMPT.to_string(),
        Some(extra) => format!("{PM_SYSTEM_PROMPT}\n\n{extra}"),
    }
}

/// Defaults OFF. On-values: `true` / `on` / `1` / `yes`. Anything else is
/// off. PM-only: workspace sessions never look at this setting.
pub fn pm_fast_mode_enabled(store: &crate::store::Store) -> bool {
    matches!(
        store.get_setting("pm_fast_mode").ok().flatten().as_deref(),
        Some("true" | "on" | "1" | "yes")
    )
}

/// The initial user message wsx sends to PM after a Fresh spawn.
pub const PM_AUTO_SUMMARY_MESSAGE: &str =
    "Give me a status summary of all active workspaces per your instructions.";

/// The user message wsx sends to PM on `r` refresh.
pub const PM_REFRESH_MESSAGE: &str =
    "Refresh: workspaces.json has been updated. Re-summarize the current state of all workspaces.";

/// Open or resume the PM session. Initializes the PM directory, refreshes
/// `workspaces.json`, spawns the PM PTY (Fresh or Continue depending on
/// whether claude has a prior session for the PM cwd), and stores it on
/// the manager. Caller decides whether to send the auto-summary message.
pub async fn open_pm(
    mgr: &mut crate::pty::session::SessionManager,
    store: &Store,
    pm_dir: &Path,
    custom_instructions: Option<String>,
) -> Result<()> {
    init_pm_dir(pm_dir)?;
    let workspaces_json = pm_dir.join("workspaces.json");
    write_workspaces_json(store, &workspaces_json)?;
    let agent = crate::pty::session::AgentKind::from_store(store);
    let resume = crate::pty::session::has_prior_session_for(pm_dir, agent);
    let mode = crate::pty::session::SpawnMode::ProjectManager {
        workspaces_json_path: workspaces_json,
        custom_instructions,
        additional_dirs: vec![],
        resume,
        fast_mode: pm_fast_mode_enabled(store),
    };
    let remote = crate::remote_control::RemoteOpts::from_store(store);
    mgr.spawn_pm(pm_dir, 80, 24, mode, remote, agent)?;
    Ok(())
}

/// Like `open_pm` but also prompts PM to produce a summary after spawn:
/// Fresh spawns get the initial auto-summary message; `--continue` resumes
/// get the refresh message so PM re-reads the freshly-written
/// workspaces.json (its conversation memory may be stale across wsx runs).
/// Call this on the FIRST open per wsx run.
pub async fn open_pm_with_auto_summary(
    mgr: &mut crate::pty::session::SessionManager,
    store: &Store,
    pm_dir: &Path,
    custom_instructions: Option<String>,
) -> Result<()> {
    let agent = crate::pty::session::AgentKind::from_store(store);
    let was_resume = crate::pty::session::has_prior_session_for(pm_dir, agent);
    open_pm(mgr, store, pm_dir, custom_instructions).await?;
    if was_resume {
        return refresh_pm(mgr, store, pm_dir).await;
    }
    if let Some(session) = mgr.pm() {
        let session = session.clone();
        tokio::spawn(async move {
            session
                .send_text_when_settled(PM_AUTO_SUMMARY_MESSAGE, 400, 5_000)
                .await;
        });
    }
    Ok(())
}

/// Refresh PM state: rewrite `workspaces.json` and send the refresh
/// user message to PM's PTY (after settle).
pub async fn refresh_pm(
    mgr: &mut crate::pty::session::SessionManager,
    store: &Store,
    pm_dir: &Path,
) -> Result<()> {
    write_workspaces_json(store, &pm_dir.join("workspaces.json"))?;
    if let Some(session) = mgr.pm() {
        let s = session.clone();
        tokio::spawn(async move {
            s.send_text_when_settled(PM_REFRESH_MESSAGE, 400, 5_000)
                .await;
        });
    }
    Ok(())
}

/// Open the PM pane and then send a refresh message so PM picks up the
/// latest workspaces.json. Used by the `p` key handler on every reopen
/// after the first (the first open goes through
/// `open_pm_with_auto_summary` which already prompts an initial
/// summary).
pub async fn open_pm_with_refresh(
    mgr: &mut crate::pty::session::SessionManager,
    store: &Store,
    pm_dir: &Path,
    custom_instructions: Option<String>,
) -> Result<()> {
    open_pm(mgr, store, pm_dir, custom_instructions).await?;
    refresh_pm(mgr, store, pm_dir).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{NewWorkspace, Store, WorkspaceState};
    use crate::test_support::{EnvGuard, cat_path};
    use tempfile::TempDir;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn open_pm_spawns_session_and_writes_workspaces_json() {
        let mut env = EnvGuard::new();
        env.set("WSX_CLAUDE_BIN", cat_path());
        let dir = TempDir::new().unwrap();
        let pm_root = dir.path().join("pm");
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        open_pm(&mut mgr, &store, &pm_root, None).await.unwrap();
        assert!(mgr.pm().is_some(), "expected pm session");
        assert!(pm_root.join("workspaces.json").exists());
    }

    #[test]
    fn workspaces_json_filters_failed_pending_and_never_started() {
        // Point HOME at a tempdir so `has_prior_session` looks at our stubbed
        // session log dirs, not the developer's real ~/.claude/projects/.
        let mut env = EnvGuard::new();
        let home = TempDir::new().unwrap();
        env.set("HOME", home.path());

        let wt_root = TempDir::new().unwrap();
        let ready_with_session = wt_root.path().join("ready-with-session");
        let ready_no_session = wt_root.path().join("ready-no-session");
        let failed = wt_root.path().join("broken");
        std::fs::create_dir_all(&ready_with_session).unwrap();
        std::fs::create_dir_all(&ready_no_session).unwrap();
        std::fs::create_dir_all(&failed).unwrap();

        // Stub a session log for `ready-with-session` so has_prior_session
        // returns true for it. Path encoding mirrors events::encode_cwd.
        let canon = std::fs::canonicalize(&ready_with_session).unwrap();
        let encoded = canon.to_string_lossy().replace(['/', '.'], "-");
        let log_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(log_dir.join("session.jsonl"), "{}\n").unwrap();

        let store = Store::open_in_memory().unwrap();
        let repo_id = store
            .add_repo(Path::new("/tmp/fake-repo"), "demo", "")
            .unwrap();
        let ws1 = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ready-with-session",
                branch: "demo/ready-with-session",
                worktree_path: &ready_with_session,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(ws1, WorkspaceState::Ready)
            .unwrap();
        let ws2 = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "ready-no-session",
                branch: "demo/ready-no-session",
                worktree_path: &ready_no_session,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(ws2, WorkspaceState::Ready)
            .unwrap();
        let ws3 = store
            .insert_workspace(&NewWorkspace {
                repo_id,
                name: "broken",
                branch: "demo/broken",
                worktree_path: &failed,
                yolo: false,
                agent: crate::pty::session::AgentKind::Claude,
            })
            .unwrap();
        store
            .set_workspace_state(ws3, WorkspaceState::Failed)
            .unwrap();

        let target_dir = TempDir::new().unwrap();
        let target = target_dir.path().join("workspaces.json");
        write_workspaces_json(&store, &target).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.contains("\"name\": \"ready-with-session\""), "{text}");
        assert!(!text.contains("\"name\": \"ready-no-session\""), "{text}");
        assert!(!text.contains("\"name\": \"broken\""), "{text}");
        assert!(text.contains("\"generated_at_epoch_seconds\""), "{text}");
    }

    #[test]
    fn init_pm_dir_creates_dir_and_git_init() {
        let dir = TempDir::new().unwrap();
        let pm_root = dir.path().join("pm");
        init_pm_dir(&pm_root).unwrap();
        assert!(pm_root.is_dir());
        assert!(pm_root.join(".git").is_dir(), "expected git repo init");
        // Idempotent: second call should not error.
        init_pm_dir(&pm_root).unwrap();
    }

    #[test]
    fn workspaces_json_empty_repo_shows_empty_array() {
        let store = Store::open_in_memory().unwrap();
        store
            .add_repo(Path::new("/tmp/empty-repo"), "empty", "")
            .unwrap();
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("workspaces.json");
        write_workspaces_json(&store, &target).unwrap();
        let text = std::fs::read_to_string(&target).unwrap();
        assert!(text.contains("\"name\": \"empty\""), "{text}");
        assert!(text.contains("\"workspaces\": []"), "{text}");
    }

    #[test]
    fn system_prompt_contains_distinctive_phrases() {
        let p = pm_system_prompt(None);
        assert!(p.contains("project manager"), "{p}");
        assert!(p.contains("./workspaces.json"), "{p}");
        assert!(p.contains("session_log_dir"), "{p}");
    }

    #[test]
    fn system_prompt_appends_custom_instructions() {
        let p = pm_system_prompt(Some("Be extra terse."));
        assert!(p.contains("project manager"), "{p}");
        assert!(p.ends_with("Be extra terse."), "{p}");
        assert!(p.contains("\n\nBe extra terse."), "{p}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn refresh_pm_rewrites_json_and_sends_message() {
        // Same shell-wrapper trick as open_pm_with_auto_summary test: cat
        // chokes on PM flags so we wrap it.
        let mut env = EnvGuard::new();
        let dir = TempDir::new().unwrap();
        let wrapper = dir.path().join("claude-stub.sh");
        std::fs::write(&wrapper, format!("#!/bin/sh\nexec {}\n", cat_path())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&wrapper).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&wrapper, perm).unwrap();

        env.set("WSX_CLAUDE_BIN", &wrapper);
        let pm_root = dir.path().join("pm");
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        open_pm(&mut mgr, &store, &pm_root, None).await.unwrap();
        let first_meta = std::fs::metadata(pm_root.join("workspaces.json"))
            .unwrap()
            .modified()
            .unwrap();
        // Wait so mtime advances on rewrite.
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let session = mgr.pm().unwrap();
        session.writer.send(b"prime\n".to_vec()).await.unwrap();
        refresh_pm(&mut mgr, &store, &pm_root).await.unwrap();
        // Allow settle + echo.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let second_meta = std::fs::metadata(pm_root.join("workspaces.json"))
            .unwrap()
            .modified()
            .unwrap();
        assert!(second_meta > first_meta, "expected mtime advance");
        let screen = session.parser.lock().unwrap().screen().contents();
        assert!(
            screen.contains("Refresh"),
            "expected refresh echo. screen: {screen:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn open_pm_with_auto_summary_writes_message_after_settle() {
        // `cat` chokes on `--allowedTools` and other PM flags, so we
        // wrap it in a tiny shell script that ignores its args and execs cat
        // reading stdin. The wrapper is a tempfile we drop after the test.
        let mut env = EnvGuard::new();
        let dir = TempDir::new().unwrap();
        let wrapper = dir.path().join("claude-stub.sh");
        std::fs::write(&wrapper, format!("#!/bin/sh\nexec {}\n", cat_path())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&wrapper).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&wrapper, perm).unwrap();

        env.set("WSX_CLAUDE_BIN", &wrapper);
        let pm_root = dir.path().join("pm");
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        open_pm_with_auto_summary(&mut mgr, &store, &pm_root, None)
            .await
            .unwrap();
        let session = mgr.pm().expect("pm session");
        // Prime activity so the settle gate has something to wait on.
        session.writer.send(b"x\n".to_vec()).await.unwrap();
        // Let the background task observe quiet + write its message; cat echoes it.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let screen = session.parser.lock().unwrap().screen().contents();
        assert!(
            screen.contains("status summary"),
            "expected auto-summary echoed by cat. screen: {screen:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn open_pm_with_auto_summary_sends_refresh_on_resume() {
        // Issue #42: across wsx runs, PM resumes via --continue and its
        // conversation memory of workspaces.json is stale. The first `p`
        // open in the new run must send a refresh prompt, not return
        // silently.
        let mut env = EnvGuard::new();
        let dir = TempDir::new().unwrap();
        let wrapper = dir.path().join("claude-stub.sh");
        std::fs::write(&wrapper, format!("#!/bin/sh\nexec {}\n", cat_path())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&wrapper).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&wrapper, perm).unwrap();
        env.set("WSX_CLAUDE_BIN", &wrapper);

        // Override HOME so has_prior_session looks at our stub, not the
        // developer's real ~/.claude/projects/.
        let home = TempDir::new().unwrap();
        env.set("HOME", home.path());

        // Create pm_dir up-front so canonicalize succeeds, then stub a
        // jsonl at the encoded session-log path so has_prior_session
        // returns true.
        let pm_root = dir.path().join("pm");
        std::fs::create_dir_all(&pm_root).unwrap();
        let canon = std::fs::canonicalize(&pm_root).unwrap();
        let encoded = canon.to_string_lossy().replace(['/', '.'], "-");
        let log_dir = home.path().join(".claude/projects").join(&encoded);
        std::fs::create_dir_all(&log_dir).unwrap();
        std::fs::write(log_dir.join("prior.jsonl"), "{}\n").unwrap();

        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        open_pm_with_auto_summary(&mut mgr, &store, &pm_root, None)
            .await
            .unwrap();
        let session = mgr.pm().expect("pm session");
        session.writer.send(b"prime\n".to_vec()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let screen = session.parser.lock().unwrap().screen().contents();
        assert!(
            screen.contains("Refresh"),
            "expected refresh echo on resumed first open. screen: {screen:?}"
        );
        // The fresh-spawn auto-summary message must NOT be sent here —
        // PM should re-summarize from its existing conversation context.
        assert!(
            !screen.contains("status summary"),
            "did not expect auto-summary message on resume. screen: {screen:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[allow(clippy::await_holding_lock)]
    async fn open_pm_with_refresh_sends_refresh_message_on_reopen() {
        // Models the issue #28 fix: closing+reopening the PM pane (`p`)
        // should refresh PM, not just rewrite workspaces.json silently.
        let mut env = EnvGuard::new();
        let dir = TempDir::new().unwrap();
        let wrapper = dir.path().join("claude-stub.sh");
        std::fs::write(&wrapper, format!("#!/bin/sh\nexec {}\n", cat_path())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut perm = std::fs::metadata(&wrapper).unwrap().permissions();
        perm.set_mode(0o755);
        std::fs::set_permissions(&wrapper, perm).unwrap();
        env.set("WSX_CLAUDE_BIN", &wrapper);

        let pm_root = dir.path().join("pm");
        let store = Store::open_in_memory().unwrap();
        store.add_repo(Path::new("/tmp/r"), "r", "").unwrap();
        let mut mgr = crate::pty::session::SessionManager::new();
        open_pm(&mut mgr, &store, &pm_root, None).await.unwrap();
        let session = mgr.pm().expect("pm session");

        // Prime the activity stream so send_text_when_settled has something
        // to wait on.
        session.writer.send(b"prime\n".to_vec()).await.unwrap();

        // Simulate the reopen path: hide-then-show in the `p` handler now
        // calls open_pm_with_refresh.
        open_pm_with_refresh(&mut mgr, &store, &pm_root, None)
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let screen = session.parser.lock().unwrap().screen().contents();
        assert!(
            screen.contains("Refresh"),
            "expected refresh echo from cat. screen: {screen:?}"
        );
    }

    #[test]
    fn pm_fast_mode_defaults_false_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert!(!pm_fast_mode_enabled(&store));
    }

    #[test]
    fn pm_fast_mode_true_for_on_values() {
        let store = Store::open_in_memory().unwrap();
        for v in ["true", "on", "1", "yes"] {
            store.set_setting("pm_fast_mode", v).unwrap();
            assert!(pm_fast_mode_enabled(&store), "expected enabled for {v:?}");
        }
    }

    #[test]
    fn pm_fast_mode_false_for_off_or_garbage_values() {
        let store = Store::open_in_memory().unwrap();
        for v in ["false", "off", "0", "no", "", "maybe", "FAST"] {
            store.set_setting("pm_fast_mode", v).unwrap();
            assert!(!pm_fast_mode_enabled(&store), "expected disabled for {v:?}");
        }
    }
}
