//! Rendering for `wsx help`, `wsx <group> --help`, and usage errors.

use super::groups::GROUPS;
use crate::error::Error;

pub fn render_root_help() -> String {
    let mut out = String::from("wsx — git-worktree workspace manager\n\n");
    out.push_str("USAGE:\n  wsx [COMMAND]            (no command launches the TUI)\n\n");
    out.push_str("COMMANDS:\n");
    let width = GROUPS.iter().map(|g| g.name.len()).max().unwrap_or(0);
    for g in GROUPS {
        out.push_str(&format!(
            "  {:<width$}   {}\n",
            g.name,
            g.blurb,
            width = width
        ));
    }
    out.push_str("\nRun `wsx <command> --help` for command details.\n");
    out
}

pub fn render_group_help(name: &str) -> String {
    let Some(g) = GROUPS.iter().find(|g| g.name == name) else {
        return render_root_help();
    };
    let mut out = format!("wsx {} — {}\n\n", g.name, g.blurb);
    out.push_str(&format!("USAGE:\n  wsx {} <command> [args]\n\n", g.name));
    out.push_str("COMMANDS:\n");
    let width = g.commands.iter().map(|c| c.usage.len()).max().unwrap_or(0);
    for c in g.commands {
        out.push_str(&format!(
            "  {:<width$}   {}\n",
            c.usage,
            c.blurb,
            width = width
        ));
    }
    out
}

pub fn render_usage_error(group: Option<&str>, msg: &str) -> String {
    let block = match group {
        Some(g) => render_group_help(g),
        None => render_root_help(),
    };
    format!("error: {msg}\n\n{block}")
}

/// Formats a CLI error for stderr. Usage errors render the matching help
/// block; everything else falls back to a one-line message.
pub fn report_cli_error(e: &Error) -> String {
    match e {
        Error::Usage { group, msg } => render_usage_error(*group, msg),
        other => format!("error: {other}\n"),
    }
}
