//! Workspace tag names: one normalizer shared by the TUI and CLI.

pub const MAX_TAG_LEN: usize = 32;
/// The tag grammar in words, for error messages.
pub const TAG_RULE: &str = "use 1-32 of a-z 0-9 - _, starting with a letter or digit";

/// Trimmed, lowercased `[a-z0-9][a-z0-9_-]{0,31}`, or `None`. Keeps tags safe
/// inside the `tag:<name>` hide-rule token and unmistakable for CLI flags.
pub fn normalize_tag(raw: &str) -> Option<String> {
    let t = raw.trim().to_lowercase();
    let ok = t.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
        && t.len() <= MAX_TAG_LEN
        && t.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    ok.then_some(t)
}

/// Normalize every tag or none: a CLI batch with one typo writes nothing.
pub fn normalize_all(raw: &[String]) -> crate::error::Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for r in raw {
        let t = normalize_tag(r).ok_or_else(|| {
            crate::error::Error::UserInput(format!("invalid tag '{r}': {TAG_RULE}"))
        })?;
        if !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
}

/// A typed tag list, split on spaces and commas, then `normalize_all`.
pub fn parse_list(raw: &str) -> crate::error::Result<Vec<String>> {
    let parts: Vec<String> = raw
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .collect();
    normalize_all(&parts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_and_lowercases() {
        assert_eq!(normalize_tag("  Parked ").as_deref(), Some("parked"));
    }

    #[test]
    fn accepts_digits_dash_underscore() {
        assert_eq!(normalize_tag("q4_spike-2").as_deref(), Some("q4_spike-2"));
    }

    #[test]
    fn rejects_empty_and_bad_chars() {
        for bad in ["", "   ", "a b", "a,b", "a:b", "#a", "ünï"] {
            assert_eq!(normalize_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn rejects_leading_dash_or_underscore() {
        // `--json` would be read as a CLI flag by `wsx workspace tag`.
        for bad in ["-a", "--json", "_a"] {
            assert_eq!(normalize_tag(bad), None, "{bad:?}");
        }
        assert_eq!(normalize_tag("a-_").as_deref(), Some("a-_"));
    }

    #[test]
    fn rule_text_matches_max_len() {
        assert!(TAG_RULE.contains(&format!("1-{MAX_TAG_LEN} ")));
    }

    #[test]
    fn enforces_max_len() {
        assert!(normalize_tag(&"a".repeat(MAX_TAG_LEN)).is_some());
        assert!(normalize_tag(&"a".repeat(MAX_TAG_LEN + 1)).is_none());
    }

    #[test]
    fn normalize_all_rejects_whole_batch_on_one_bad_tag() {
        let err = normalize_all(&["ok".into(), "bad tag".into()]).unwrap_err();
        assert!(err.to_string().contains("bad tag"), "{err}");
    }

    #[test]
    fn parse_list_splits_on_spaces_and_commas_and_dedupes() {
        assert_eq!(
            parse_list(" UI, spike,,ui  q4 ").unwrap(),
            ["ui", "spike", "q4"]
        );
        assert!(parse_list("  , ").unwrap().is_empty());
        assert!(parse_list("ui #bad").is_err());
    }

    #[test]
    fn normalize_all_dedupes_after_normalizing() {
        assert_eq!(
            normalize_all(&["A".into(), "a".into(), "b".into()]).unwrap(),
            ["a", "b"]
        );
    }
}
