//! Named remote shell commands. Stored as a newline-separated
//! `name=command` blob in the `remotes` setting; executed by name
//! via `wsx remote <name>` (which exec-replaces wsx with `sh -c`).
//!
//! See `docs/superpowers/specs/2026-05-18-named-remotes-design.md`.

use crate::data::store::Store;
use crate::error::Result;

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

/// Returns all configured remotes, alphabetized by name.
pub fn list(store: &Store) -> Result<Vec<Remote>> {
    let raw = store.get_setting("remotes")?.unwrap_or_default();
    let mut out = parse(&raw);
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Returns the command for `name`, or `None` if no remote with that
/// name is configured. When the blob contains duplicate names, the
/// last one wins (matches the order of the underlying blob).
pub fn lookup(store: &Store, name: &str) -> Result<Option<String>> {
    let raw = store.get_setting("remotes")?.unwrap_or_default();
    Ok(parse(&raw)
        .into_iter()
        .rev()
        .find(|r| r.name == name)
        .map(|r| r.command))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::store::Store;

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

    #[test]
    fn list_returns_empty_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert!(list(&store).unwrap().is_empty());
    }

    #[test]
    fn list_returns_alphabetized() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("remotes", "zebra=ssh z\napple=ssh a\nmango=ssh m\n")
            .unwrap();
        let out = list(&store).unwrap();
        let names: Vec<_> = out.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["apple", "mango", "zebra"]);
    }

    #[test]
    fn lookup_returns_command_for_known_name() {
        let store = Store::open_in_memory().unwrap();
        store
            .set_setting("remotes", "gpu=ssh gpu-box -t 'tmux attach'\n")
            .unwrap();
        assert_eq!(
            lookup(&store, "gpu").unwrap().as_deref(),
            Some("ssh gpu-box -t 'tmux attach'")
        );
    }

    #[test]
    fn lookup_returns_none_for_unknown_name() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("remotes", "gpu=ssh gpu-box\n").unwrap();
        assert!(lookup(&store, "nope").unwrap().is_none());
    }

    #[test]
    fn lookup_returns_none_when_unset() {
        let store = Store::open_in_memory().unwrap();
        assert!(lookup(&store, "anything").unwrap().is_none());
    }

    #[test]
    fn lookup_last_write_wins_for_duplicate_names() {
        let store = Store::open_in_memory().unwrap();
        store.set_setting("remotes", "h=first\nh=second\n").unwrap();
        assert_eq!(lookup(&store, "h").unwrap().as_deref(), Some("second"));
    }
}
