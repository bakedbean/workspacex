//! Pinned commands: parses a newline-separated `Label=command` list into
//! addressable chips for the attached view.

use crate::agent::skill::BUNDLED_SKILLS;
use crate::pty::session::AgentKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedCommand {
    /// Text shown in the chip. Already trimmed; not yet width-truncated
    /// (render decides what fits).
    pub label: String,
    /// Text written to the agent PTY (sans any trailing `\r`).
    pub command: String,
    /// Whether firing the chip also submits: `true` appends `\r` so the
    /// agent runs the command; `false` leaves it typed in the prompt for
    /// the user to finish (a config line ending in `...` or `…`).
    pub submit: bool,
}

/// Trailing markers that turn a chip into "type but don't submit".
const NO_SUBMIT_MARKERS: [&str; 2] = ["...", "\u{2026}"];

impl PinnedCommand {
    /// The command text as `agent` needs to receive it.
    ///
    /// omp only exposes skills as `/skill:<name>`, and its builtins win over
    /// a bare `/<name>`: omp 18's builtin `/handoff` summarizes and compacts
    /// the session in place, so a `/handoff` chip never reaches the wsx
    /// skill and sits on omp's "Generating handoff…" spinner. For omp, a
    /// chip naming a bundled skill is rewritten to the `/skill:` form.
    /// Everything else, and every other agent, gets the command verbatim.
    pub fn command_for(&self, agent: AgentKind) -> String {
        if agent == AgentKind::Omp {
            if let Some(rest) = self.command.strip_prefix('/') {
                let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                let name = &rest[..name_end];
                if BUNDLED_SKILLS.iter().any(|s| s.name == name) {
                    return format!("/skill:{rest}");
                }
            }
        }
        self.command.clone()
    }

    /// Bytes to write to `agent`'s PTY when the chip fires: the command
    /// (see [`Self::command_for`]), plus a carriage return when the chip
    /// submits.
    pub fn pty_bytes(&self, agent: AgentKind) -> Vec<u8> {
        let mut bytes = self.command_for(agent).into_bytes();
        if self.submit {
            bytes.push(b'\r');
        }
        bytes
    }
}

pub fn parse(text: &str) -> Vec<PinnedCommand> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() {
                return None;
            }
            let (label, command) = match line.split_once('=') {
                Some((lhs, rhs)) => (lhs.trim().to_string(), rhs.trim().to_string()),
                None => (line.to_string(), line.to_string()),
            };
            // `/agent-review ...` strips the marker but keeps the trailing
            // space so the cursor lands where the argument goes.
            let (command, submit) = match NO_SUBMIT_MARKERS
                .iter()
                .find_map(|m| command.strip_suffix(m))
            {
                Some(rest) => (rest.to_string(), false),
                None => (command, true),
            };
            if label.is_empty() || command.trim().is_empty() {
                return None;
            }
            Some(PinnedCommand {
                label,
                command,
                submit,
            })
        })
        .collect()
}

pub fn resolve(global: Option<&str>, repo: Option<&str>) -> Vec<PinnedCommand> {
    let repo_has_value = repo.map(|s| !s.trim().is_empty()).unwrap_or(false);
    let source = if repo_has_value { repo } else { global };
    match source {
        Some(text) => parse(text),
        None => Vec::new(),
    }
}

/// Truncate a chip label to fit within `max_cols` columns. If `s` exceeds
/// the budget, returns `s` truncated to `max_cols - 1` chars + `…`.
pub fn truncate_label(s: &str, max_cols: usize) -> String {
    if s.chars().count() <= max_cols {
        return s.to_string();
    }
    let keep: String = s.chars().take(max_cols.saturating_sub(1)).collect();
    format!("{keep}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_labeled_line() {
        let out = parse("PR=/pull-request");
        assert_eq!(
            out,
            vec![PinnedCommand {
                label: "PR".into(),
                command: "/pull-request".into(),
                submit: true,
            }]
        );
    }

    #[test]
    fn parse_unlabeled_line_uses_command_as_label() {
        let out = parse("/feedback");
        assert_eq!(
            out,
            vec![PinnedCommand {
                label: "/feedback".into(),
                command: "/feedback".into(),
                submit: true,
            }]
        );
    }

    #[test]
    fn parse_skips_blank_lines() {
        let out = parse("PR=/pull-request\n\n/feedback\n");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].label, "PR");
        assert_eq!(out[1].label, "/feedback");
    }

    #[test]
    fn parse_trims_both_sides_of_equals() {
        let out = parse("  Loop  =   /loop /babysit-prs   ");
        assert_eq!(
            out,
            vec![PinnedCommand {
                label: "Loop".into(),
                command: "/loop /babysit-prs".into(),
                submit: true,
            }]
        );
    }

    #[test]
    fn parse_treats_only_first_equals_as_separator() {
        // The label is everything before the first `=`. Anything after is the
        // command, including further `=` characters (rare but valid for some
        // commands).
        let out = parse("Kv=/set FOO=bar");
        assert_eq!(
            out,
            vec![PinnedCommand {
                label: "Kv".into(),
                command: "/set FOO=bar".into(),
                submit: true,
            }]
        );
    }

    #[test]
    fn parse_returns_lines_past_nine_uncapped() {
        // Render layer caps at 9; parser does not.
        let input = (1..=12)
            .map(|n| format!("/cmd{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse(&input).len(), 12);
    }

    #[test]
    fn parse_drops_empty_label_or_command() {
        // A line that's just `=` is malformed; drop it. A line where the
        // command after `=` is empty after trim is also dropped.
        assert!(parse("=").is_empty());
        assert!(parse("Label=").is_empty());
        assert!(parse("=cmd").is_empty()); // label is empty after trim
    }

    #[test]
    fn parse_trailing_dots_marks_no_submit_and_keeps_space() {
        let out = parse("agent-review=/agent-review ...");
        assert_eq!(
            out,
            vec![PinnedCommand {
                label: "agent-review".into(),
                command: "/agent-review ".into(),
                submit: false,
            }]
        );
    }

    #[test]
    fn parse_trailing_ellipsis_char_marks_no_submit() {
        let out = parse("rev=/agent-review\u{2026}");
        assert_eq!(out[0].command, "/agent-review");
        assert!(!out[0].submit);
    }

    #[test]
    fn parse_unlabeled_no_submit_keeps_marker_in_label() {
        // The label is the raw line so the chip shows the user it won't
        // submit; only the command loses the marker.
        let out = parse("/agent-review ...");
        assert_eq!(out[0].label, "/agent-review ...");
        assert_eq!(out[0].command, "/agent-review ");
        assert!(!out[0].submit);
    }

    #[test]
    fn parse_marker_mid_command_stays_literal() {
        let out = parse("X=/say ... and more");
        assert_eq!(out[0].command, "/say ... and more");
        assert!(out[0].submit);
    }

    #[test]
    fn parse_marker_then_trailing_whitespace_still_no_submit() {
        // The line is trimmed before the suffix check, so whitespace after
        // the marker doesn't hide it.
        let out = parse("rev=/agent-review ...   ");
        assert_eq!(out[0].command, "/agent-review ");
        assert!(!out[0].submit);
    }

    #[test]
    fn parse_drops_bare_marker() {
        assert!(parse("X=...").is_empty());
        assert!(parse("...").is_empty());
    }

    #[test]
    fn pty_bytes_appends_cr_only_when_submitting() {
        let submit = PinnedCommand {
            label: "PR".into(),
            command: "/pull-request".into(),
            submit: true,
        };
        assert_eq!(submit.pty_bytes(AgentKind::Claude), b"/pull-request\r");
        let typed = PinnedCommand {
            label: "rev".into(),
            command: "/agent-review ".into(),
            submit: false,
        };
        assert_eq!(typed.pty_bytes(AgentKind::Claude), b"/agent-review ");
    }

    fn cmd(command: &str) -> PinnedCommand {
        PinnedCommand {
            label: "x".into(),
            command: command.into(),
            submit: true,
        }
    }

    #[test]
    fn omp_bundled_skill_chip_uses_skill_prefix() {
        // omp's builtin `/handoff` compacts the session in place and would
        // shadow the wsx skill; `/skill:handoff` is the only form that
        // reaches the skill.
        assert_eq!(
            cmd("/handoff").command_for(AgentKind::Omp),
            "/skill:handoff"
        );
        assert_eq!(
            cmd("/agent-review codex").command_for(AgentKind::Omp),
            "/skill:agent-review codex"
        );
        assert_eq!(cmd("/wsx").command_for(AgentKind::Omp), "/skill:wsx");
        assert_eq!(
            cmd("/handoff").pty_bytes(AgentKind::Omp),
            b"/skill:handoff\r"
        );
    }

    #[test]
    fn omp_leaves_non_skill_chips_alone() {
        for c in [
            "/pull-request",
            "/handoffs",
            "/skill:handoff",
            "handoff",
            "/",
        ] {
            assert_eq!(cmd(c).command_for(AgentKind::Omp), c);
        }
    }

    #[test]
    fn other_agents_send_bundled_skill_chips_verbatim() {
        for agent in AgentKind::ALL
            .iter()
            .copied()
            .filter(|a| *a != AgentKind::Omp)
        {
            assert_eq!(cmd("/handoff").command_for(agent), "/handoff", "{agent:?}");
        }
    }

    #[test]
    fn resolve_repo_overrides_global() {
        let out = resolve(Some("A=/global"), Some("B=/repo"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "B");
    }

    #[test]
    fn resolve_empty_repo_falls_back_to_global() {
        let out = resolve(Some("A=/global"), Some(""));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "A");
    }

    #[test]
    fn resolve_whitespace_only_repo_falls_back_to_global() {
        let out = resolve(Some("A=/global"), Some("   \n  \n"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "A");
    }

    #[test]
    fn resolve_no_repo_uses_global() {
        let out = resolve(Some("A=/global"), None);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "A");
    }

    #[test]
    fn resolve_both_none_returns_empty() {
        assert!(resolve(None, None).is_empty());
    }

    #[test]
    fn resolve_no_global_uses_repo() {
        let out = resolve(None, Some("B=/repo"));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, "B");
    }

    #[test]
    fn resolve_malformed_non_empty_repo_still_wins_over_global() {
        // Repo text is non-empty after trim but parses to zero commands.
        // Per spec, the repo value "wins" — surfacing the config error to
        // the user rather than silently falling back to the global list.
        let out = resolve(Some("A=/global"), Some("=\n=\n"));
        assert!(out.is_empty(), "expected zero chips, got {out:?}");
    }

    #[test]
    fn truncate_label_short_passthrough() {
        assert_eq!(truncate_label("PR", 12), "PR");
    }

    #[test]
    fn truncate_label_long_uses_ellipsis() {
        assert_eq!(truncate_label("/loop /babysit-prs", 12), "/loop /baby…");
    }

    #[test]
    fn truncate_label_exact_width_passthrough() {
        assert_eq!(truncate_label("abcdefghijkl", 12), "abcdefghijkl");
    }
}
