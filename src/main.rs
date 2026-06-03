#![allow(clippy::arc_with_non_send_sync)]

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;
use wsx::{app, cli, config, data::store, error::Result, git};

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        // Children are killed via Drop on Session (sends SIGKILL via ChildKiller).
        default(info);
    }));
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let dirs = config::Dirs::discover();
    dirs.ensure()?;

    // CLI path: parse args; if non-TUI, dispatch and return.
    let action = match cli::parse_args(std::env::args().collect()) {
        Ok(a) => a,
        Err(e) => {
            eprint!("{}", cli::report_cli_error(&e));
            std::process::exit(2);
        }
    };
    if !matches!(action, cli::CliAction::Tui) {
        cli::run_cli(action, &dirs).await?;
        return Ok(());
    }

    let file_appender = tracing_appender::rolling::daily(dirs.log_dir(), "wsx.log");
    let (nb, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(nb)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    git::preflight().await?;

    let store = store::Store::open(&dirs.db_path())?;
    let worktree_base = dirs.app_dir().join("worktrees");
    std::fs::create_dir_all(&worktree_base)?;
    let app = Arc::new(Mutex::new(app::App::new(store, worktree_base)?));

    // Watch for git branch renames performed by claude (or the user)
    // and propagate to the wsx store. Aborts when the runtime drops.
    tokio::spawn(app::branch_drift_poll(app.clone()));

    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::run(&mut terminal, app.clone()).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    // Drop SessionManager (kills all children).
    drop(app);
    result
}
