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
}
