//! Named remote shell commands. Stored as a newline-separated
//! `name=command` blob in the `remotes` setting; executed by name
//! via `wsx remote <name>` (which exec-replaces wsx with `sh -c`).
//!
//! See `docs/superpowers/specs/2026-05-18-named-remotes-design.md`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub command: String,
}

pub fn parse(text: &str) -> Vec<Remote> {
    text.lines()
        .filter_map(|raw| {
            let line = raw.trim();
            if line.is_empty() {
                return None;
            }
            let (name, command) = match line.split_once('=') {
                Some((lhs, rhs)) => (lhs.trim().to_string(), rhs.trim().to_string()),
                None => (line.to_string(), line.to_string()),
            };
            if name.is_empty() || command.is_empty() {
                return None;
            }
            Some(Remote { name, command })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_labeled_line() {
        let out = parse("ebenmini=ssh -4 -t ebenmini.local 'tmux attach'");
        assert_eq!(
            out,
            vec![Remote {
                name: "ebenmini".into(),
                command: "ssh -4 -t ebenmini.local 'tmux attach'".into(),
            }]
        );
    }

    #[test]
    fn parse_unlabeled_line_uses_command_as_name() {
        let out = parse("ssh foo");
        assert_eq!(
            out,
            vec![Remote {
                name: "ssh foo".into(),
                command: "ssh foo".into(),
            }]
        );
    }

    #[test]
    fn parse_skips_blank_lines() {
        let out = parse("a=ssh a\n\nb=ssh b\n");
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].name, "a");
        assert_eq!(out[1].name, "b");
    }

    #[test]
    fn parse_trims_both_sides_of_equals() {
        let out = parse("  gpu  =   ssh gpu-box   ");
        assert_eq!(
            out,
            vec![Remote {
                name: "gpu".into(),
                command: "ssh gpu-box".into(),
            }]
        );
    }

    #[test]
    fn parse_treats_only_first_equals_as_separator() {
        // The command may legitimately contain `=` (e.g. env vars).
        let out = parse("envset=FOO=bar ssh host");
        assert_eq!(
            out,
            vec![Remote {
                name: "envset".into(),
                command: "FOO=bar ssh host".into(),
            }]
        );
    }

    #[test]
    fn parse_drops_empty_name_or_command() {
        assert!(parse("=").is_empty());
        assert!(parse("name=").is_empty());
        assert!(parse("=cmd").is_empty());
    }

    #[test]
    fn parse_preserves_nested_quotes_verbatim() {
        // The motivating example: nested double + single quotes.
        let out = parse(r#"ebenmini=ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'""#);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].command,
            r#"ssh -4 -t ebenmini.local "zsh -lc 'tmux attach'""#
        );
    }
}
