//! Pinned commands: parses a newline-separated `Label=command` list into
//! addressable chips for the attached view.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedCommand {
    /// Text shown in the chip. Already trimmed; not yet width-truncated
    /// (render decides what fits).
    pub label: String,
    /// Bytes sent to the claude PTY (sans the trailing `\r`).
    pub command: String,
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
            if label.is_empty() || command.is_empty() {
                return None;
            }
            Some(PinnedCommand { label, command })
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
