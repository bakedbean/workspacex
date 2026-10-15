//! Workspace tag names: one normalizer shared by the TUI and CLI.

pub const MAX_TAG_LEN: usize = 32;

/// Trimmed, lowercased `[a-z0-9_-]{1,32}`, or `None`. Keeps tags safe
/// inside the comma-separated `tag:<name>` hide-rule token.
pub fn normalize_tag(raw: &str) -> Option<String> {
    let t = raw.trim().to_lowercase();
    let ok = !t.is_empty()
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
            crate::error::Error::UserInput(format!(
                "invalid tag '{r}': use 1-{MAX_TAG_LEN} of a-z 0-9 - _"
            ))
        })?;
        if !out.contains(&t) {
            out.push(t);
        }
    }
    Ok(out)
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
    fn normalize_all_dedupes_after_normalizing() {
        assert_eq!(
            normalize_all(&["A".into(), "a".into(), "b".into()]).unwrap(),
            ["a", "b"]
        );
    }
}
