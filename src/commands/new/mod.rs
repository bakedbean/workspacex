//! The worker half of `wsx new`: create a workspace, start its agent, wait
//! until the Claude app can reach it, and deliver the task.

pub(crate) mod detach;

use crate::data::store::{Store, Workspace, WorkspaceId};
use crate::error::{Error, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    Delivered,
    /// Queued for the dashboard, which has not delivered it yet.
    Queued,
    NotWritten,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// `app`: the Claude app can reach it.
    Ready {
        app: bool,
    },
    TaskNotDelivered {
        queued: bool,
    },
    NotVisible {
        screen: String,
    },
    Exited {
        screen: String,
    },
}

/// What `drive` needs from a started agent.
pub(crate) trait Backend {
    fn alive(&mut self) -> bool;
    fn screen(&mut self) -> Option<String>;
    fn reachable(&mut self) -> bool;
    async fn deliver(&mut self, task: &str) -> Delivery;
}

pub(crate) struct DriveOpts {
    pub needs_bridge: bool,
    pub task: Option<String>,
    pub visible_within: Duration,
    pub keep_watching: Duration,
    pub settle: Duration,
}

const HOLD_AFTER_WRITE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(500);

/// Returns the final outcome; `report` is called exactly once, as soon as
/// there is something to tell the waiting terminal.
pub(crate) async fn drive<B: Backend>(
    backend: &mut B,
    opts: &DriveOpts,
    report: &mut dyn FnMut(&Outcome),
) -> Outcome {
    let start = tokio::time::Instant::now();
    let mut last_screen = String::new();
    let mut reported = false;
    let mut tell = |o: Outcome, reported: &mut bool| {
        if !std::mem::replace(reported, true) {
            report(&o);
        }
        o
    };
    loop {
        if let Some(s) = backend.screen().filter(|s| !s.trim().is_empty()) {
            last_screen = s;
        }
        if !backend.alive() {
            let screen = last_screen;
            return tell(Outcome::Exited { screen }, &mut reported);
        }
        let ready = if opts.needs_bridge {
            backend.reachable()
        } else {
            start.elapsed() >= opts.settle
        };
        if ready {
            break;
        }
        if start.elapsed() >= opts.visible_within {
            let o = Outcome::NotVisible {
                screen: last_screen.clone(),
            };
            let o = tell(o, &mut reported);
            let give_up = opts.visible_within + opts.keep_watching;
            if opts.task.is_none() || start.elapsed() >= give_up {
                return o;
            }
        }
        tokio::time::sleep(POLL).await;
    }
    let delivery = match &opts.task {
        Some(task) => backend.deliver(task).await,
        None => Delivery::Delivered,
    };
    let outcome = match delivery {
        Delivery::Delivered => Outcome::Ready {
            app: opts.needs_bridge,
        },
        Delivery::Queued => Outcome::TaskNotDelivered { queued: true },
        Delivery::NotWritten if !backend.alive() => Outcome::Exited {
            screen: last_screen,
        },
        Delivery::NotWritten => Outcome::TaskNotDelivered { queued: false },
    };
    tell(outcome, &mut reported)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Started {
    Dashboard,
    Headless,
}

/// A running dashboard starts the agent. Only a request nobody claimed falls
/// back: after any other failure a dashboard may still be starting it.
pub(crate) async fn start_agent(
    dashboard: impl Future<Output = Result<()>>,
    headless: impl AsyncFnOnce() -> Result<()>,
) -> Result<Started> {
    match dashboard.await {
        Ok(()) => Ok(Started::Dashboard),
        Err(Error::NoDashboard) => headless().await.map(|()| Started::Headless),
        Err(e) => Err(e),
    }
}

/// `<repo>/<slug>` as of now: the agent may have renamed its workspace.
fn current_name(store: &Store, ws: WorkspaceId) -> String {
    crate::app::messaging::workspace_ref(store, ws).unwrap_or_else(|| "(unknown)".into())
}

fn retry_lines(name: &str, task_file: Option<&Path>) -> String {
    use crate::cli::resolve::shell_quote;
    task_file.map_or(String::new(), |f| {
        format!(
            "\nsend the task again with:\n  wsx agent send --workspace {} --file {} primary",
            shell_quote(name),
            shell_quote(&f.to_string_lossy())
        )
    })
}

/// The last lines of a screen, indented.
fn tail(screen: &str) -> String {
    let lines: Vec<&str> = screen.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(12)..]
        .iter()
        .map(|l| format!("\n  | {}", l.trim_end()))
        .collect()
}

/// The text for `outcome`, and the exit code.
pub(crate) fn describe(
    store: &Store,
    ws: WorkspaceId,
    session: Option<&str>,
    task_file: Option<&Path>,
    outcome: &Outcome,
) -> (String, i32) {
    let name = current_name(store, ws);
    // Always quoted: zsh expands a bare `=word` to a command's path.
    let attach = session.map_or(String::new(), |s| {
        format!("\nattach with: tmux attach -t '={s}'")
    });
    let retry = retry_lines(&name, task_file);
    match outcome {
        Outcome::Ready { app: true } => {
            (format!("✓ {name}: agent running, open the Claude app"), 0)
        }
        Outcome::Ready { app: false } => (format!("✓ {name}: agent running"), 0),
        Outcome::TaskNotDelivered { queued: true } => {
            let file = task_file.map_or(String::new(), |f| format!(" (kept in {})", f.display()));
            (
                format!(
                    "{name}: agent running, but the task was not delivered yet; \
                     it is still queued for the dashboard{file}"
                ),
                1,
            )
        }
        Outcome::TaskNotDelivered { queued: false } => (
            format!("{name}: agent running, but the task was not delivered{retry}"),
            1,
        ),
        Outcome::NotVisible { screen } => {
            let later = task_file.map_or(String::new(), |f| {
                format!(
                    "\nthe task is kept in {} and will be sent if the agent connects \
                     within 30 minutes",
                    f.display()
                )
            });
            (
                format!(
                    "{name}: agent started, but not visible to the Claude app yet{}{attach}{later}",
                    tail(screen)
                ),
                1,
            )
        }
        Outcome::Exited { screen } => (
            format!(
                "{name}: the agent exited; the workspace exists{}{retry}",
                tail(screen)
            ),
            1,
        ),
    }
}

/// What the foreground hands the worker.
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Job {
    pub repo: String,
    pub yolo: bool,
    pub agent: String,
    pub task_file: Option<PathBuf>,
    pub exit_file: PathBuf,
    /// Set when the worker is a launchd job, which must remove itself.
    pub launchd_label: Option<String>,
}

/// Queue `task` for a dashboard once, and wait for it to land.
async fn deliver_by_dashboard(
    store: &Store,
    ws: WorkspaceId,
    task: &str,
    wait: Duration,
) -> Delivery {
    let from = std::env::var("WSX_AGENT_INSTANCE_ID")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(crate::data::store::AgentInstanceId);
    let queued = store.primary_instance_id(ws).and_then(|target| {
        let target = target.ok_or_else(|| Error::UserInput("no primary agent".into()))?;
        store.enqueue_message(ws, target, from, task)
    });
    let id = match queued {
        Ok(id) => id,
        Err(e) => {
            println!("could not queue the task: {e}");
            return Delivery::NotWritten;
        }
    };
    let start = tokio::time::Instant::now();
    loop {
        // A read that fails (a busy database, say) is no answer: ask again.
        if let Ok(Some(m)) = store.message_by_id(id) {
            if m.reached_agent() {
                return Delivery::Delivered;
            }
            if m.delivered_at.is_some() {
                return Delivery::NotWritten;
            }
        }
        if start.elapsed() >= wait {
            return Delivery::Queued;
        }
        tokio::time::sleep(POLL.min(wait)).await;
    }
}

struct Live<'a> {
    store: &'a Store,
    ws: Workspace,
    session_name: String,
    /// The held tmux client, when this process started the agent.
    held: Option<std::sync::Arc<crate::pty::session::Session>>,
}

impl Backend for Live<'_> {
    fn alive(&mut self) -> bool {
        crate::pty::tmux::has_session(&self.session_name)
    }
    fn screen(&mut self) -> Option<String> {
        crate::pty::tmux::capture_pane(&self.session_name)
    }
    fn reachable(&mut self) -> bool {
        use crate::agent::reachable;
        reachable::bridge_up(
            &reachable::sessions_dir(),
            &self.ws.worktree_path,
            reachable::pid_alive,
        )
    }
    async fn deliver(&mut self, task: &str) -> Delivery {
        use crate::app::messaging::{DELIVERY_QUIET_MS, DELIVERY_TIMEOUT_MS};
        let Some(session) = &self.held else {
            let wait = Duration::from_millis(DELIVERY_TIMEOUT_MS);
            return deliver_by_dashboard(self.store, self.ws.id, task, wait).await;
        };
        // Never queued on failure: a false can follow a write that landed.
        if session
            .send_text_when_settled(task, DELIVERY_QUIET_MS, DELIVERY_TIMEOUT_MS)
            .await
        {
            // Acked once in the PTY, which the tmux client may not have read yet:
            // letting the client go now could lose it.
            tokio::time::sleep(HOLD_AFTER_WRITE).await;
            Delivery::Delivered
        } else {
            Delivery::NotWritten
        }
    }
}

/// Written whole and last, so the follower never reads a partial code or
/// exits ahead of the output.
fn write_exit(path: &Path, code: i32) -> Result<()> {
    use std::io::Write;
    std::io::stdout().flush()?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, code.to_string())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub(crate) async fn run_worker(job_path: &Path, dirs: &crate::config::Dirs) -> Result<()> {
    let job: Job = serde_json::from_slice(&std::fs::read(job_path)?)?;
    let _ = std::fs::remove_file(job_path);
    if let Err(e) = work(&job, dirs).await {
        println!("error: {e}");
        if let Some(f) = job.task_file.as_ref().filter(|f| f.exists()) {
            println!("the task is kept in {}", f.display());
        }
        write_exit(&job.exit_file, 1)?;
    }
    // Last: this ends the process when it is the job.
    if let Some(label) = &job.launchd_label {
        detach::remove_job(label);
    }
    Ok(())
}

async fn work(job: &Job, dirs: &crate::config::Dirs) -> Result<()> {
    let task = match &job.task_file {
        Some(f) => Some(std::fs::read_to_string(f)?),
        None => None,
    };
    let store = Store::open(&dirs.db_path())?;
    let repo = crate::cli::resolve::lookup_repo(&store, &job.repo)?;
    let agent = crate::pty::session::AgentKind::from_str_or_default(Some(&job.agent));
    let worktree_base = dirs.app_dir().join("worktrees");
    std::fs::create_dir_all(&worktree_base)?;
    println!("creating a workspace in {}...", repo.name);
    let created = crate::data::workspace::create(
        &store,
        &repo,
        None,
        &worktree_base,
        job.yolo,
        true,
        agent,
        &dirs.log_dir(),
        tokio_util::sync::CancellationToken::new(),
        |_| {},
    )
    .await?;
    let ws = created.workspace;
    if let crate::data::setup::SetupResult::Failed { exit_code } = created.setup_result {
        println!("warning: setup script exited with code {exit_code}");
        if let Some(log) = &created.setup_log {
            println!("setup log: {}", log.display());
        }
    }
    println!("starting {}/{}...", repo.name, ws.name);
    if let Err(e) = start_and_drive(job, dirs, &store, ws.clone(), agent, task).await {
        let name = current_name(&store, ws.id);
        println!("the workspace {name} exists, but its agent did not start");
        let retry = retry_lines(&name, job.task_file.as_deref());
        if !retry.is_empty() {
            println!("{}", retry.trim_start());
        }
        return Err(e);
    }
    Ok(())
}

async fn start_and_drive(
    job: &Job,
    dirs: &crate::config::Dirs,
    store: &Store,
    ws: Workspace,
    agent: crate::pty::session::AgentKind,
    task: Option<String>,
) -> Result<()> {
    use crate::pty::tmux;
    let mut held = None;
    start_agent(
        async {
            let waits = crate::commands::shared::ShareWaits::default();
            crate::commands::shared::share_and_restart(store, &ws, waits, tmux::has_session)
                .await
                .map(|_| ())
        },
        async || {
            let own = Store::open(&dirs.db_path())?;
            let base = dirs.app_dir().join("worktrees");
            let mut app = crate::app::App::new_headless(own, base)?;
            held = Some(crate::app::headless::start(&mut app, ws.id)?);
            Ok(())
        },
    )
    .await?;
    let session_name = store
        .workspace_agents(ws.id)?
        .into_iter()
        .find(|i| i.is_primary)
        .and_then(|i| i.session_ref)
        .ok_or_else(|| Error::UserInput("the agent has no tmux session".into()))?;
    // Headless, the tmux client creates its session after the spawn returns.
    for _ in 0..50 {
        if tmux::has_session(&session_name) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let opts = DriveOpts {
        needs_bridge: agent == crate::pty::session::AgentKind::Claude
            && crate::agent::remote_control::enabled(store),
        task,
        visible_within: Duration::from_secs(60),
        keep_watching: Duration::from_secs(30 * 60),
        settle: Duration::from_secs(2),
    };
    let ws_id = ws.id;
    let mut live = Live {
        store,
        ws,
        session_name: session_name.clone(),
        held,
    };
    let task_file = job.task_file.as_deref();
    let mut wrote = Ok(());
    let outcome = drive(&mut live, &opts, &mut |o| {
        let (text, code) = describe(store, ws_id, Some(&session_name), task_file, o);
        // Before the exit code is visible, so success leaves nothing behind.
        if matches!(o, Outcome::Ready { .. }) {
            remove_task(task_file);
        }
        println!("{text}");
        wrote = write_exit(&job.exit_file, code);
    })
    .await;
    if let Some(session) = &live.held {
        session.detach_tmux_client().await;
    }
    wrote?;
    if matches!(outcome, Outcome::Ready { .. }) {
        remove_task(task_file);
    }
    Ok(())
}

fn remove_task(task_file: Option<&Path>) {
    if let Some(f) = task_file {
        let _ = std::fs::remove_file(f);
    }
}

/// Check what can be checked before anything is created.
pub(crate) fn preflight(
    store: &Store,
    repo: &str,
    agent: crate::pty::session::AgentKind,
) -> Result<()> {
    crate::cli::resolve::lookup_repo(store, repo)?;
    if !crate::pty::tmux::is_available() {
        return Err(Error::UserInput(format!(
            "tmux >= 3.2 ({}) is not available on this host",
            crate::pty::tmux::tmux_bin()
        )));
    }
    if let Some(bin) = crate::pty::session::missing_agent_binary(agent) {
        return Err(Error::AgentBinaryMissing(bin));
    }
    if agent == crate::pty::session::AgentKind::Claude
        && !crate::agent::remote_control::enabled(store)
    {
        eprintln!("warning: remote_control is off, so the Claude app won't list this agent");
    }
    Ok(())
}

/// The foreground half: hand the job to a detached worker and follow it.
/// Returns the exit code.
pub(crate) fn launch(
    dirs: &crate::config::Dirs,
    repo: &str,
    task: Option<&str>,
    yolo: bool,
    agent: crate::pty::session::AgentKind,
) -> Result<i32> {
    use std::io::Write;
    let dir = dirs.log_dir().join("new");
    std::fs::create_dir_all(&dir)?;
    let stem = format!("{}-{}", crate::data::store::now_ms(), std::process::id());
    let log = dir.join(format!("{stem}.log"));
    let job_file = dir.join(format!("{stem}.job"));
    let task_file = dir.join(format!("{stem}.task"));
    if let Some(task) = task {
        detach::private_file(&task_file)?.write_all(task.as_bytes())?;
    }
    let gui = detach::outside_desktop();
    let label = format!("wsx.new.{stem}");
    let write_job = |launchd_label: Option<String>| -> Result<()> {
        let job = Job {
            repo: repo.into(),
            yolo,
            agent: agent.store_value().into(),
            task_file: task.map(|_| task_file.clone()),
            exit_file: dir.join(format!("{stem}.exit")),
            launchd_label,
        };
        let _ = std::fs::remove_file(&job_file);
        detach::private_file(&job_file)?.write_all(&serde_json::to_vec(&job)?)?;
        Ok(())
    };
    let exe = std::env::current_exe()?;
    let args = [
        "new".into(),
        "--worker".into(),
        job_file.clone().into_os_string(),
    ];
    let exit_file = dir.join(format!("{stem}.exit"));
    let mut out = std::io::stdout();
    let code = 'code: {
        if gui {
            write_job(Some(label.clone()))?;
            let mut argv = vec![exe.as_os_str()];
            argv.extend(args.iter().map(|a: &std::ffi::OsString| a.as_os_str()));
            let env = detach::worker_env(std::env::vars_os(), true);
            if detach::spawn_in_gui(&label, &argv, &env, &log)? {
                // launchd reports no pid for a moment after the bootstrap.
                let began = std::time::Instant::now();
                let mut alive =
                    || began.elapsed() < Duration::from_secs(5) || detach::job_running(&label);
                break 'code detach::follow(&log, &exit_file, &mut alive, &mut out)?;
            }
            eprintln!(
                "warning: nobody is logged in at the desktop, so Claude may not be \
                 able to read its login"
            );
        }
        write_job(None)?;
        let env = detach::worker_env(std::env::vars_os(), false);
        let mut child = detach::spawn(&exe, &args, &env, &log)?;
        let mut alive = || matches!(child.try_wait(), Ok(None));
        detach::follow(&log, &exit_file, &mut alive, &mut out)?
    };
    let _ = std::fs::remove_file(&exit_file);
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Instant;

    struct Fake {
        t0: Instant,
        up_at: Option<u64>,
        dies_at: Option<u64>,
        delivery: Delivery,
        delivered: Vec<(u64, String)>,
    }

    impl Fake {
        fn new(up_at: Option<u64>) -> Self {
            Fake {
                t0: Instant::now(),
                up_at,
                dies_at: None,
                delivery: Delivery::Delivered,
                delivered: vec![],
            }
        }
        fn secs(&self) -> u64 {
            self.t0.elapsed().as_secs()
        }
        fn past(&self, at: Option<u64>) -> bool {
            at.is_some_and(|a| self.secs() >= a)
        }
    }

    impl Backend for Fake {
        fn alive(&mut self) -> bool {
            !self.past(self.dies_at)
        }
        fn screen(&mut self) -> Option<String> {
            self.alive().then(|| "Not logged in".into())
        }
        fn reachable(&mut self) -> bool {
            self.past(self.up_at)
        }
        async fn deliver(&mut self, task: &str) -> Delivery {
            self.delivered.push((self.secs(), task.into()));
            self.delivery
        }
    }

    fn opts(needs_bridge: bool, task: Option<&str>) -> DriveOpts {
        DriveOpts {
            needs_bridge,
            task: task.map(Into::into),
            visible_within: Duration::from_secs(60),
            keep_watching: Duration::from_secs(1800),
            settle: Duration::from_secs(2),
        }
    }

    /// Final outcome, plus every report with the second it was made.
    async fn go(b: &mut Fake, o: DriveOpts) -> (Outcome, Vec<(u64, Outcome)>) {
        let t0 = b.t0;
        let mut seen = vec![];
        let out = drive(b, &o, &mut |x| {
            seen.push((t0.elapsed().as_secs(), x.clone()))
        })
        .await;
        (out, seen)
    }

    fn not_visible() -> Outcome {
        Outcome::NotVisible {
            screen: "Not logged in".into(),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_task_waits_for_the_bridge() {
        let mut b = Fake::new(Some(5));
        let (out, seen) = go(&mut b, opts(true, Some("t"))).await;
        assert_eq!(out, Outcome::Ready { app: true });
        assert_eq!(seen.len(), 1);
        assert_eq!(b.delivered.len(), 1);
        assert!(b.delivered[0].0 >= 5, "{:?}", b.delivered);
    }

    #[tokio::test(start_paused = true)]
    async fn no_task_is_ready_once_reachable() {
        let mut b = Fake::new(Some(5));
        let (out, _) = go(&mut b, opts(true, None)).await;
        assert_eq!(out, Outcome::Ready { app: true });
        assert!(b.delivered.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_bridge_the_task_goes_after_settling() {
        let mut b = Fake::new(None);
        let (out, _) = go(&mut b, opts(false, Some("t"))).await;
        assert_eq!(out, Outcome::Ready { app: false });
        assert!(b.delivered[0].0 < 10);
    }

    #[tokio::test(start_paused = true)]
    async fn without_a_bridge_an_instant_exit_is_not_success() {
        let mut b = Fake::new(None);
        b.dies_at = Some(1);
        let (out, _) = go(&mut b, opts(false, None)).await;
        assert!(matches!(out, Outcome::Exited { .. }), "{out:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn unreachable_reports_at_60s_and_still_delivers_later_once() {
        let mut b = Fake::new(Some(300));
        let (out, seen) = go(&mut b, opts(true, Some("t"))).await;
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].1, not_visible());
        assert!((60..63).contains(&seen[0].0), "{seen:?}");
        assert_eq!(out, Outcome::Ready { app: true });
        assert_eq!(b.delivered.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn never_reachable_ends_without_delivering() {
        let mut b = Fake::new(None);
        let (out, seen) = go(&mut b, opts(true, Some("t"))).await;
        assert_eq!((out, seen.len()), (not_visible(), 1));
        assert!(b.delivered.is_empty());
        assert!((1860..1870).contains(&b.secs()), "{}", b.secs());
    }

    #[tokio::test(start_paused = true)]
    async fn unreachable_with_no_task_has_nothing_to_wait_for() {
        let mut b = Fake::new(None);
        let (out, _) = go(&mut b, opts(true, None)).await;
        assert_eq!(out, not_visible());
        assert!(b.secs() < 65);
    }

    #[tokio::test(start_paused = true)]
    async fn an_exit_is_reported_at_once_with_the_last_screen() {
        let mut b = Fake::new(None);
        b.dies_at = Some(3);
        let (out, seen) = go(&mut b, opts(true, Some("t"))).await;
        let want = Outcome::Exited {
            screen: "Not logged in".into(),
        };
        assert_eq!(out, want);
        assert!(seen[0].0 <= 4 && seen[0].1 == want, "{seen:?}");
        assert!(b.delivered.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn an_undelivered_task_is_tried_once() {
        for (d, queued) in [(Delivery::NotWritten, false), (Delivery::Queued, true)] {
            let mut b = Fake::new(Some(1));
            b.delivery = d;
            let (out, _) = go(&mut b, opts(true, Some("t"))).await;
            assert_eq!(out, Outcome::TaskNotDelivered { queued });
            assert_eq!(b.delivered.len(), 1);
        }
    }

    #[tokio::test]
    async fn only_no_dashboard_falls_back_to_headless() {
        let ran = std::cell::Cell::new(false);
        let headless = async || {
            ran.set(true);
            Ok(())
        };
        assert_eq!(
            start_agent(async { Ok(()) }, headless).await.unwrap(),
            Started::Dashboard
        );
        let e = start_agent(async { Err(Error::UserInput("x".into())) }, headless).await;
        assert!(matches!(e, Err(Error::UserInput(_))));
        assert!(!ran.get());
        assert_eq!(
            start_agent(async { Err(Error::NoDashboard) }, headless)
                .await
                .unwrap(),
            Started::Headless
        );
        assert!(ran.get());
    }

    fn text(o: Outcome) -> (String, i32) {
        let store = Store::open_in_memory().unwrap();
        let repo = store.add_repo(Path::new("/tmp/r"), "my repo", "").unwrap();
        let ws = store
            .insert_workspace(&crate::data::store::NewWorkspace {
                repo_id: repo,
                name: "old-slug",
                branch: "b",
                worktree_path: Path::new("/tmp/r/w"),
                yolo: true,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .conn()
            .execute("UPDATE workspaces SET name = 'new-slug'", [])
            .unwrap();
        describe(
            &store,
            ws,
            Some("wsx-r-w"),
            Some(Path::new("/t/a task")),
            &o,
        )
    }

    #[test]
    fn success_lines() {
        let (t, code) = text(Outcome::Ready { app: true });
        assert_eq!(t, "✓ my repo/new-slug: agent running, open the Claude app");
        assert_eq!(code, 0);
        assert_eq!(
            text(Outcome::Ready { app: false }),
            ("✓ my repo/new-slug: agent running".into(), 0)
        );
    }

    #[test]
    fn failures_say_how_to_recover() {
        const RETRY: &str =
            "wsx agent send --workspace 'my repo/new-slug' --file '/t/a task' primary";
        let (t, code) = text(Outcome::TaskNotDelivered { queued: false });
        assert!(code == 1 && t.contains(RETRY), "{t}");

        let (t, code) = text(Outcome::TaskNotDelivered { queued: true });
        assert!(code == 1 && t.contains("still queued"), "{t}");
        assert!(!t.contains(RETRY) && t.contains("/t/a task"), "{t}");

        let (t, code) = text(not_visible());
        assert!(
            code == 1 && t.contains("not visible to the Claude app yet"),
            "{t}"
        );
        assert!(t.contains("tmux attach -t '=wsx-r-w'"), "{t}");
        assert!(
            t.contains("Not logged in") && t.contains("/t/a task"),
            "{t}"
        );

        let (t, code) = text(Outcome::Exited {
            screen: "boom".into(),
        });
        assert!(code == 1 && t.contains("boom") && t.contains(RETRY), "{t}");
        assert!(
            t.contains("my repo/new-slug") && t.contains("exists"),
            "{t}"
        );
    }

    fn db_with_ws(dir: &Path) -> (Store, WorkspaceId) {
        let store = Store::open(&dir.join("t.db")).unwrap();
        let repo = store.add_repo(&dir.join("r"), "r", "").unwrap();
        let ws = store
            .insert_workspace(&crate::data::store::NewWorkspace {
                repo_id: repo,
                name: "w",
                branch: "w",
                worktree_path: &dir.join("w"),
                yolo: true,
                agent: crate::pty::session::AgentKind::Claude,
                shared: true,
            })
            .unwrap();
        store
            .add_primary_agent(ws, crate::pty::session::AgentKind::Claude, 1)
            .unwrap();
        (store, ws)
    }

    #[tokio::test]
    async fn a_dashboard_delivery_waits_for_the_message_to_land() {
        for (drop, want) in [(false, Delivery::Delivered), (true, Delivery::NotWritten)] {
            let tmp = tempfile::tempdir().unwrap();
            let (store, ws) = db_with_ws(tmp.path());
            let wait = Duration::from_secs(5);
            let dashboard = async {
                loop {
                    if let Some(m) = store.undelivered_messages().unwrap().pop() {
                        assert_eq!(m.body, "go");
                        if drop {
                            store.mark_dropped(m.id, "gone").unwrap();
                        } else {
                            store.mark_delivered(m.id).unwrap();
                        }
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            };
            let (got, ()) = tokio::join!(deliver_by_dashboard(&store, ws, "go", wait), dashboard);
            assert_eq!(got, want);
        }
    }

    #[tokio::test]
    async fn a_dashboard_delivery_that_never_lands_stays_queued_once() {
        let tmp = tempfile::tempdir().unwrap();
        let (store, ws) = db_with_ws(tmp.path());
        let got = deliver_by_dashboard(&store, ws, "go", Duration::from_millis(50)).await;
        assert_eq!(got, Delivery::Queued);
        assert_eq!(store.undelivered_messages().unwrap().len(), 1);
    }

    /// The whole worker, no dashboard: real git, a private tmux server, and
    /// an agent that records what is typed at it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_worker_starts_a_headless_agent_and_types_the_task_once() {
        use crate::pty::tmux;
        if !tmux::is_available() {
            eprintln!("tmux not installed; skipping");
            return;
        }
        struct Server(PathBuf);
        impl Drop for Server {
            fn drop(&mut self) {
                let _ = std::process::Command::new(tmux::tmux_bin())
                    .env("TMUX_TMPDIR", &self.0)
                    .env_remove("TMUX")
                    .arg("kill-server")
                    .output();
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().canonicalize().unwrap();
        let _server = Server(dir.clone());
        let mut env = crate::test_support::EnvGuard::new();
        env.set("TMUX_TMPDIR", &dir);
        env.set("TERM", "xterm-256color");
        env.set("HOME", &dir);
        let typed = dir.join("typed");
        let agent = dir.join("agent.sh");
        std::fs::write(
            &agent,
            format!(
                "#!/bin/sh\nprintf '\\033[?1049h\\033[2J> '\nwhile read -r l; do echo \"$l\" >> '{}'; done\n",
                typed.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&agent, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        env.set("WSX_CLAUDE_BIN", &agent);

        let repo = dir.join("repo");
        std::fs::create_dir(&repo).unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec![
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ],
        ] {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .current_dir(&repo)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let dirs = crate::config::Dirs::for_test(&dir);
        dirs.ensure().unwrap();
        {
            let store = Store::open(&dirs.db_path()).unwrap();
            let id = store.add_repo(&repo, "repo", "").unwrap();
            // A failed setup script warns; the agent still starts.
            store.set_repo_setup_script(id, Some("exit 3")).unwrap();
            store.set_setting("remote_control", "off").unwrap();
            store.set_setting("mcp_mirror", "off").unwrap();
        }
        let task_file = dir.join("task");
        std::fs::write(&task_file, "do the thing").unwrap();
        let job = dir.join("job.json");
        let body = Job {
            repo: "repo".into(),
            yolo: true,
            agent: "claude".into(),
            task_file: Some(task_file.clone()),
            exit_file: dir.join("exit"),
            launchd_label: None,
        };
        std::fs::write(&job, serde_json::to_string(&body).unwrap()).unwrap();

        run_worker(&job, &dirs).await.unwrap();

        assert_eq!(std::fs::read_to_string(dir.join("exit")).unwrap(), "0");
        assert!(!task_file.exists() && !job.exists());
        let mut got = String::new();
        for _ in 0..50 {
            got = std::fs::read_to_string(&typed).unwrap_or_default();
            if !got.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert_eq!(got.matches("do the thing").count(), 1, "{got:?}");
        let store = Store::open(&dirs.db_path()).unwrap();
        assert!(store.undelivered_messages().unwrap().is_empty());
        let r = crate::data::repo::list(&store).unwrap().remove(0);
        let ws = store.workspaces(r.id).unwrap().remove(0);
        assert!(ws.shared && ws.yolo);
        let name = store.workspace_agents(ws.id).unwrap().remove(0).session_ref;
        let name = name.unwrap();
        // Long enough for anything typed by the client going away to land.
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(tmux::has_session(&name), "agent outlives the worker");
    }

    #[tokio::test]
    async fn a_worker_that_cannot_start_still_writes_its_exit_code() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = crate::config::Dirs::for_test(tmp.path());
        dirs.ensure().unwrap();
        let task_file = tmp.path().join("task");
        std::fs::write(&task_file, "t").unwrap();
        let job = tmp.path().join("job.json");
        let body = Job {
            repo: "nope".into(),
            yolo: true,
            agent: "claude".into(),
            task_file: Some(task_file.clone()),
            exit_file: tmp.path().join("exit"),
            launchd_label: None,
        };
        std::fs::write(&job, serde_json::to_string(&body).unwrap()).unwrap();
        run_worker(&job, &dirs).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("exit")).unwrap(),
            "1"
        );
        assert!(task_file.exists(), "the task is kept");
    }

    #[test]
    fn preflight_refuses_before_anything_is_created() {
        use crate::pty::session::AgentKind::Claude;
        let mut env = crate::test_support::EnvGuard::new();
        let tmp = tempfile::tempdir().unwrap();
        let (store, _) = db_with_ws(tmp.path());
        let fails = |repo: &str| preflight(&store, repo, Claude).unwrap_err().to_string();
        assert!(fails("nope").contains("nope"));
        env.set("WSX_TMUX_BIN", "/nonexistent/wsx-no-tmux");
        assert!(fails("r").contains("wsx-no-tmux"));
        env.set("WSX_TMUX_BIN", "true");
        env.set("WSX_CLAUDE_BIN", "/nonexistent/wsx-no-claude");
        assert!(fails("r").contains("wsx-no-claude"));
    }
}
