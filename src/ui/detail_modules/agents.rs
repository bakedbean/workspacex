//! Agents module. The Claude subagents running in the selected workspace:
//! how many, and what each is doing now, as Claude Code's own footer
//! shows them.
//!
//! ```text
//! 2 subagents running
//! ● Find the poller                       1m
//!   Explore · Grep(fn poll)
//! ● Review the diff                      12s
//!   general-purpose · Bash(Run the tests)
//! ```
//!
//! A subagent silent for a while shows `quiet 20m · ` ahead of its last
//! activity. [`subagent_lines`] draws the list for the side panel's Agents
//! tab too.

use crate::activity::subagents::Subagent;
use crate::ui::detail_modules::{DetailContext, DetailModule};
use crate::ui::text::{display_width, truncate};
use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

pub struct Agents;

impl DetailModule for Agents {
    fn id(&self) -> &'static str {
        "agents"
    }
    fn title(&self) -> &'static str {
        "AGENTS"
    }
    fn lines(&self, ctx: &DetailContext<'_>, width: u16) -> Vec<Line<'static>> {
        let now_ms = crate::util::time::now_ms();
        let mut out = vec![count_line(ctx.subagents.len(), ctx.theme)];
        out.extend(subagent_lines(
            ctx.subagents,
            width as usize,
            ctx.theme,
            now_ms,
        ));
        out
    }
}

/// `2 subagents running`, or that none are.
pub fn count_line(n: usize, theme: &Theme) -> Line<'static> {
    let text = match n {
        0 => "No subagents running.".to_string(),
        1 => "1 subagent running".to_string(),
        n => format!("{n} subagents running"),
    };
    Line::from(Span::styled(text, theme.dim_style()))
}

/// Two lines per subagent: its description with how long it has run,
/// then its type and latest activity. Each is cut to `width`.
pub fn subagent_lines(
    subs: &[Subagent],
    width: usize,
    theme: &Theme,
    now_ms: i64,
) -> Vec<Line<'static>> {
    let mut out = Vec::with_capacity(subs.len() * 2);
    for s in subs {
        let age = crate::util::time::format_age(now_ms.saturating_sub(s.started_ms));
        let headline = if s.description.is_empty() {
            s.agent_type.as_str()
        } else {
            s.description.as_str()
        };
        // `● ` before, ` <age>` after.
        let room = width.saturating_sub(2 + 1 + display_width(&age));
        let title = truncate(headline, room);
        let gap = room.saturating_sub(display_width(&title)) + 1;
        out.push(Line::from(vec![
            Span::styled("● ", theme.ok_style()),
            Span::raw(title),
            Span::styled(format!("{}{age}", " ".repeat(gap)), theme.dim_style()),
        ]));
        let activity = s.activity.as_deref().unwrap_or("starting…");
        let kind = if s.agent_type.is_empty() || s.description.is_empty() {
            String::new()
        } else {
            format!("{} · ", s.agent_type)
        };
        // Silent a while: it may be on a slow command, or its session may
        // have died under it. Say so ahead of what it last did.
        let quiet = s
            .quiet_for(now_ms)
            .map(|ms| format!("quiet {} · ", crate::util::time::format_age(ms)))
            .unwrap_or_default();
        let room = width.saturating_sub(2);
        let quiet_shown = truncate(&quiet, room);
        let rest = truncate(
            &format!("{kind}{activity}"),
            room.saturating_sub(display_width(&quiet_shown)),
        );
        let mut spans = vec![Span::raw("  ")];
        if !quiet_shown.is_empty() {
            spans.push(Span::styled(quiet_shown, theme.warn_style()));
        }
        spans.push(Span::styled(rest, theme.row_dim_style()));
        out.push(Line::from(spans));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::detail_modules::tests_helpers::stub_context;

    fn sub(desc: &str, kind: &str, activity: Option<&str>, started_ms: i64) -> Subagent {
        Subagent {
            id: desc.into(),
            agent_type: kind.into(),
            description: desc.into(),
            background: true,
            activity: activity.map(String::from),
            started_ms,
            last_write_ms: started_ms,
        }
    }

    fn text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn id_and_title() {
        assert_eq!(Agents.id(), "agents");
        assert_eq!(Agents.title(), "AGENTS");
    }

    #[test]
    fn none_running_says_so() {
        let out = text(&Agents.lines(&stub_context(), 40));
        assert_eq!(out, ["No subagents running."]);
    }

    #[test]
    fn each_subagent_shows_its_task_age_type_and_activity() {
        // The module reads the clock; recent stamps keep them from reading
        // as quiet.
        let t = crate::util::time::now_ms();
        let subs = [
            sub("Find the poller", "Explore", Some("Grep(fn poll)"), t),
            sub("Review", "general-purpose", None, t),
        ];
        let mut ctx = stub_context();
        ctx.subagents = Box::leak(Box::new(subs));
        let out = text(&Agents.lines(&ctx, 30));
        assert_eq!(out[0], "2 subagents running");
        assert!(out[1].starts_with("● Find the poller"));
        assert_eq!(out[2], "  Explore · Grep(fn poll)");
        assert_eq!(out[4], "  general-purpose · starting…");
        assert_eq!(out.len(), 5);
    }

    #[test]
    fn a_quiet_subagent_says_how_long_before_its_last_activity() {
        let quiet_ms = crate::activity::subagents::QUIET_AFTER_MS + 5 * 60_000;
        let subs = [sub(
            "Build",
            "general-purpose",
            Some("Bash(cargo build)"),
            0,
        )];
        let out = text(&subagent_lines(&subs, 60, &Theme::default(), quiet_ms));
        assert_eq!(out[1], "  quiet 20m · general-purpose · Bash(cargo build)");
    }

    #[test]
    fn the_age_sits_flush_right_and_long_lines_are_cut() {
        let subs = [sub(
            "A very long task description indeed",
            "Explore",
            Some("Bash(an extremely long command line that overflows)"),
            0,
        )];
        let out = text(&subagent_lines(&subs, 24, &Theme::default(), 90_000));
        assert_eq!(display_width(&out[0]), 24);
        assert!(out[0].ends_with(" 1m"));
        assert!(display_width(&out[1]) <= 24);
        assert!(out[1].ends_with('…'));
    }
}
