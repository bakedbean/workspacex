mod app;
mod config;
mod error;
mod git;
mod names;
mod pty;
mod repo;
mod setup;
mod store;
mod ui;
mod workspace;

use crate::error::Result;
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        // Children are killed via Drop on Session (sends SIGKILL via ChildKiller).
        default(info);
    }));
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let dirs = config::Dirs::discover();
    dirs.ensure()?;

    let file_appender = tracing_appender::rolling::daily(dirs.log_dir(), "wsx.log");
    let (nb, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt().with_writer(nb)
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")))
        .init();

    git::preflight().await?;

    let store = store::Store::open(&dirs.db_path())?;
    let worktree_base = dirs.app_dir().join("worktrees");
    std::fs::create_dir_all(&worktree_base)?;
    let app = Arc::new(Mutex::new(app::App::new(store, worktree_base)?));

    install_panic_hook();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::run(&mut terminal, app.clone()).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Drop SessionManager (kills all children).
    drop(app);
    result
}
