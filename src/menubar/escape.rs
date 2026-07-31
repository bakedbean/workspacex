//! SwiftBar string escaping: the injection barrier between wsx data and the
//! menu protocol, plus length caps and param quoting. Shared by the plugin
//! document renderer and the Project Manager section.

use crate::workspace_rows::sanitize;

/// Cap on a rendered line's *display text* segment, in chars — keeps one
/// hostile/huge status message from ballooning the SwiftBar document. Never
/// applied to param values (paths, URLs), which must survive intact or the
/// action they drive (open path, open URL) breaks.
pub(crate) const MAX_TEXT_LEN: usize = 120;

/// Injection barrier shared by display text and param values: control chars
/// collapse (via sanitize) and the protocol's text/params separator '|'
/// becomes a broken bar, so no user-controlled string can smuggle params or
/// extra rows. Uncapped and no dash guard — safe for param values (quoted,
/// not line-initial) where truncation would corrupt a real path or URL.
pub(crate) fn esc_core(s: &str) -> String {
    sanitize(s).replace('|', "\u{00a6}")
}

/// `esc_core` plus a guard on a leading '-' (so the string can't read as a
/// '---' separator or '--' submenu marker), with no length cap. The shared
/// primitive: `esc_text` caps it plainly, while the PM section caps it with
/// an ellipsis. A single capped helper cannot serve both.
pub(crate) fn esc_text_uncapped(s: &str) -> String {
    let mut out = esc_core(s);
    if out.starts_with('-') {
        out.replace_range(0..1, "\u{2011}");
    }
    out
}

/// Display-text sanitizer: `esc_text_uncapped` truncated to `MAX_TEXT_LEN`.
/// Only for text that renders directly on a menu line — never for param
/// values.
pub(crate) fn esc_text(s: &str) -> String {
    let out = esc_text_uncapped(s);
    if out.chars().count() > MAX_TEXT_LEN {
        return out.chars().take(MAX_TEXT_LEN).collect();
    }
    out
}

/// All bash=/paramN=/href= values are double-quoted; interior quotes degrade
/// to '\'' (a path with a double quote is pathological — keeping the
/// protocol unbreakable beats preserving it). Uses the uncapped, unguarded
/// `esc_core` — param values are real paths/URLs consumed by the action they
/// drive, not display text, so they must never be truncated or dash-shifted.
pub(crate) fn quote_param(s: &str) -> String {
    format!("\"{}\"", esc_core(s).replace('"', "'"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_text_caps_length() {
        let huge = "x".repeat(1000);
        let capped = esc_text(&huge);
        assert_eq!(capped.chars().count(), MAX_TEXT_LEN);
        assert_eq!(capped, "x".repeat(MAX_TEXT_LEN));
    }

    #[test]
    fn esc_text_uncapped_does_not_truncate() {
        let huge = "x".repeat(1000);
        assert_eq!(esc_text_uncapped(&huge).chars().count(), 1000);
    }

    #[test]
    fn esc_text_guards_leading_dash() {
        // A repo named "---evil" must not render a line that IS a bare
        // separator once escaped.
        let escaped = esc_text("---evil");
        assert_ne!(escaped, "---");
        assert!(!escaped.starts_with('-'), "{escaped}");
    }

    #[test]
    fn esc_core_neutralizes_pipes_and_control_chars() {
        assert_eq!(esc_core("a|b"), "a\u{00a6}b");
        assert_eq!(esc_core("a\nb\tc"), "a b c");
        // No dash guard, no cap — param values must survive intact.
        assert_eq!(esc_core("-x"), "-x");
    }

    #[test]
    fn quote_param_wraps_and_degrades_double_quotes() {
        assert_eq!(quote_param("/a/b"), "\"/a/b\"");
        assert_eq!(quote_param("/a/\"b\""), "\"/a/'b'\"");
    }
}
