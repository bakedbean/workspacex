//! Learning which session each running omp instance is in.
//!
//! omp assigns its own session ids and reports them nowhere wsx can hook —
//! no lifecycle hook, no notify program, no create-by-id flag. What it does
//! keep is a breadcrumb per terminal (`~/.omp/agent/terminal-sessions/
//! <pts-N>`: cwd + session file) so that its own `--continue` can find "this
//! terminal's last session". wsx created that terminal, so the crumb is an
//! exact per-instance answer while the PTY is alive. This module polls it
//! for every running omp session and stores the session file on the
//! instance, which is what `app::spawn::recorded_resume_id` resumes by.
//!
//! Polled rather than captured once: `/new` inside omp rewrites the crumb,
//! and the crumb for a fresh session only appears once omp materializes it.
//! The cost is one small file read per running omp instance per poll.

use super::*;

/// How often, in housekeeping ticks (`run::TICK` = 125ms), to re-read the
/// crumbs. 16 ticks ≈ 2s: fast enough that a session started moments before
/// quitting wsx is still recorded, slow enough to be free.
pub(crate) const HARVEST_EVERY_TICKS: u32 = 16;

impl App {
    /// Record the session file each running omp instance is in, per the
    /// breadcrumb of the terminal its agent reads from
    /// (`Session::agent_terminal`: this PTY, or the tmux pane for a shared
    /// workspace), when it differs from what is stored. Crumbs older than
    /// that terminal are ignored as a previous occupant's.
    ///
    /// Also called once on quit and before a share/unshare respawn, so a
    /// `/new` performed moments earlier is not lost to the poll interval.
    pub(crate) fn harvest_omp_breadcrumbs(&self) {
        for (inst_id, session) in self.sessions.iter() {
            if session.agent != crate::pty::session::AgentKind::Omp
                || !self.instance_is_running(inst_id)
            {
                continue;
            }
            let Some((tty, terminal_created)) = session.agent_terminal() else {
                continue;
            };
            let Some(terminal_id) = crate::pty::session::omp_terminal_id(&tty) else {
                continue;
            };
            let Ok(Some(instance)) = self.store.workspace_agents_by_id(inst_id) else {
                continue;
            };
            let Some((_, ws)) = self
                .workspaces
                .iter()
                .find(|(_, w)| w.id == instance.workspace_id)
            else {
                continue;
            };
            let Some(file) = crate::pty::session::omp_breadcrumb_session_file(
                &terminal_id,
                &ws.worktree_path,
                terminal_created,
            ) else {
                continue;
            };
            let file = file.to_string_lossy().into_owned();
            if instance.agent_session_id.as_deref() == Some(file.as_str()) {
                continue;
            }
            if let Err(e) = self.store.set_instance_agent_session(inst_id, &file) {
                tracing::warn!(error = %e, "failed to record an omp session breadcrumb");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::NewWorkspace;
    use crate::pty::session::{AgentKind, SessionStatus};
    use crate::test_support::EnvGuard;

    /// An app with one workspace on a real temp worktree and one added omp
    /// instance, plus a fake omp session for it. Returns the pieces a test
    /// needs; the guards keep `HOME` and the dirs alive.
    fn fixture() -> (
        App,
        crate::data::agents::AgentInstance,
        String,
        tempfile::TempDir,
        tempfile::TempDir,
        (EnvGuard, tempfile::TempDir),
    ) {
        let home = tempfile::TempDir::new().unwrap();
        let worktree = tempfile::TempDir::new().unwrap();
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "feat",
                branch: "wsx/feat",
                worktree_path: worktree.path(),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Claude, 1).unwrap();
        let omp = store.add_workspace_agent(ws, AgentKind::Omp).unwrap();

        let mut env = EnvGuard::new();
        env.set("HOME", home.path());
        let state = tempfile::TempDir::new().unwrap();
        let mut app = App::new(store, state.path().to_path_buf()).unwrap();
        app.refresh().unwrap();
        let session = app.sessions.insert_fake_session_for(
            omp.id,
            AgentKind::Omp,
            SessionStatus::Running { pid: 1 },
        );
        let terminal_id = crate::pty::session::omp_terminal_id(
            session
                .tty_name
                .as_deref()
                .expect("a real pty has a device name"),
        )
        .expect("terminal id");
        (app, omp, terminal_id, home, worktree, (env, state))
    }

    fn write_crumb(home: &std::path::Path, terminal_id: &str, cwd: &std::path::Path, file: &str) {
        let dir = home.join(".omp/agent/terminal-sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(terminal_id),
            format!(
                "{}\n{file}\n",
                std::fs::canonicalize(cwd).unwrap().display()
            ),
        )
        .unwrap();
    }

    #[test]
    fn harvest_records_the_crumb_for_a_running_omp_session() {
        let (app, omp, terminal_id, home, worktree, _env) = fixture();
        let file = "/home/x/.omp/agent/sessions/-w/2026_abc.jsonl";
        write_crumb(home.path(), &terminal_id, worktree.path(), file);

        app.harvest_omp_breadcrumbs();
        let stored = app.store.workspace_agents_by_id(omp.id).unwrap().unwrap();
        assert_eq!(stored.agent_session_id.as_deref(), Some(file));

        // `/new` rewrote the crumb: the newer file wins.
        let file2 = "/home/x/.omp/agent/sessions/-w/2026_def.jsonl";
        write_crumb(home.path(), &terminal_id, worktree.path(), file2);
        app.harvest_omp_breadcrumbs();
        let stored = app.store.workspace_agents_by_id(omp.id).unwrap().unwrap();
        assert_eq!(stored.agent_session_id.as_deref(), Some(file2));
    }

    #[test]
    fn harvest_ignores_a_same_worktree_crumb_older_than_the_terminal() {
        // The exact peer collision: a crumb for this device number, this
        // worktree, but written before this PTY existed — a peer's leftover.
        let (app, omp, terminal_id, home, worktree, _env) = fixture();
        let file = "/home/x/.omp/agent/sessions/-w/2026_peer.jsonl";
        write_crumb(home.path(), &terminal_id, worktree.path(), file);
        let crumb = home
            .path()
            .join(".omp/agent/terminal-sessions")
            .join(&terminal_id);
        let before_spawn =
            app.sessions.get(omp.id).unwrap().spawned_at_system - std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(&crumb)
            .unwrap()
            .set_modified(before_spawn)
            .unwrap();
        app.harvest_omp_breadcrumbs();
        let stored = app.store.workspace_agents_by_id(omp.id).unwrap().unwrap();
        assert_eq!(
            stored.agent_session_id, None,
            "a stale crumb is not harvested"
        );
        // And it never overwrites a correct stored identity either.
        app.store
            .set_instance_agent_session(omp.id, "/home/x/.omp/agent/sessions/-w/2026_mine.jsonl")
            .unwrap();
        app.harvest_omp_breadcrumbs();
        let stored = app.store.workspace_agents_by_id(omp.id).unwrap().unwrap();
        assert_eq!(
            stored.agent_session_id.as_deref(),
            Some("/home/x/.omp/agent/sessions/-w/2026_mine.jsonl")
        );
    }

    #[test]
    fn harvest_ignores_a_crumb_for_another_directory() {
        let (app, omp, terminal_id, home, _worktree, _env) = fixture();
        let elsewhere = tempfile::TempDir::new().unwrap();
        write_crumb(
            home.path(),
            &terminal_id,
            elsewhere.path(),
            "/home/x/.omp/agent/sessions/-e/2026_zzz.jsonl",
        );
        app.harvest_omp_breadcrumbs();
        let stored = app.store.workspace_agents_by_id(omp.id).unwrap().unwrap();
        assert_eq!(stored.agent_session_id, None, "a recycled pts is not ours");
    }

    #[test]
    fn harvest_skips_exited_and_non_omp_sessions() {
        let (mut app, omp, terminal_id, home, worktree, _env) = fixture();
        let file = "/home/x/.omp/agent/sessions/-w/2026_abc.jsonl";
        write_crumb(home.path(), &terminal_id, worktree.path(), file);
        // Same instance, but its session has exited: nothing to harvest.
        app.sessions.insert_fake_session_for(
            omp.id,
            AgentKind::Omp,
            SessionStatus::Exited { code: 0 },
        );
        app.harvest_omp_breadcrumbs();
        let stored = app.store.workspace_agents_by_id(omp.id).unwrap().unwrap();
        assert_eq!(stored.agent_session_id, None);
    }
}
