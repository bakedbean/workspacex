use std::io::Write;
use std::process::{Command, Stdio};

use crate::data::store::Store;
use crate::error::{Error, Result};

pub fn menu_line(repo: &str, slug: &str, message: Option<&str>) -> String {
    match message {
        Some(m) => format!("{repo}/{slug} — {m}"),
        None => format!("{repo}/{slug}"),
    }
}

/// Inverse of menu_line: everything before the first " — " is `repo/slug`,
/// split on the FIRST '/' (slugs are kebab-case, never contain '/').
pub fn parse_menu_line(line: &str) -> Option<(String, String)> {
    let target = line.split(" — ").next().unwrap_or(line).trim();
    let (repo, slug) = target.split_once('/')?;
    if repo.is_empty() || slug.is_empty() {
        return None;
    }
    Some((repo.to_string(), slug.to_string()))
}

pub fn menu_command() -> Vec<String> {
    std::env::var("WSX_WAYBAR_MENU")
        .ok()
        .and_then(|v| shlex::split(&v))
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["walker".into(), "--dmenu".into()])
}

fn notify(message: &str) {
    let _ = Command::new("notify-send").args(["wsx", message]).status();
    eprintln!("wsx: {message}");
}

pub fn run_menu(store: &Store) -> Result<()> {
    let statuses = store.all_workspace_status()?;
    let mut lines = Vec::new();
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    for repo in &repos {
        for ws in store.workspaces(repo.id)? {
            let message = statuses
                .get(&ws.id)
                .and_then(|s| s.message.as_deref().map(str::to_string));
            lines.push(menu_line(&repo.name, &ws.name, message.as_deref()));
        }
    }
    if lines.is_empty() {
        notify("no workspaces");
        return Ok(());
    }
    let cmd = menu_command();
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| {
            notify(&format!("menu command '{}' failed to start", cmd[0]));
            Error::UserInput(format!("failed to launch menu '{}': {e}", cmd[0]))
        })?;
    child
        .stdin
        .take()
        .expect("piped stdin")
        .write_all(lines.join("\n").as_bytes())?;
    let out = child.wait_with_output()?;
    let selection = String::from_utf8_lossy(&out.stdout);
    let selection = selection.trim();
    if selection.is_empty() {
        return Ok(()); // dismissed
    }
    if let Some((repo, slug)) = parse_menu_line(selection) {
        crate::waybar::jump::jump(&repo, &slug)?;
    }
    Ok(())
}

#[cfg(test)]
mod menu_tests {
    use super::*;

    #[test]
    fn menu_line_round_trips() {
        for (repo, slug, msg) in [
            ("alpha", "one", Some("fixing the bug")),
            ("meals backend", "api-fix", Some("has — a dash")),
            ("alpha", "two", None),
        ] {
            let line = menu_line(repo, slug, msg);
            assert_eq!(
                parse_menu_line(&line),
                Some((repo.to_string(), slug.to_string())),
                "line was {line:?}"
            );
        }
        assert_eq!(parse_menu_line(""), None);
        assert_eq!(parse_menu_line("noslash — msg"), None);
    }

    #[test]
    fn menu_command_env_override() {
        let mut env = crate::test_support::EnvGuard::new();
        env.set("WSX_WAYBAR_MENU", "wofi --dmenu -p pick");
        assert_eq!(menu_command(), vec!["wofi", "--dmenu", "-p", "pick"]);
        env.remove("WSX_WAYBAR_MENU");
        assert_eq!(menu_command(), vec!["walker", "--dmenu"]);
    }
}
