//! The `terminal_cmd` setting as a jump reads it, shared by the Linux jump
//! (`desktop::jump`) and the macOS menubar's.

/// `terminal_cmd` is honored only when it carries a `{cmd}` placeholder.
/// The setting is also the dashboard's `[t]` terminal, usually a bare
/// program or app-open command (`alacritty`, `open -a iTerm`) that can't run
/// a command, and guessing an argv position for one would misfire.
pub(crate) fn resolve_terminal_template(configured: Option<&str>, cmd: &str) -> Option<String> {
    let t = configured?.trim();
    if t.is_empty() || !t.contains("{cmd}") {
        return None;
    }
    Some(t.replace("{cmd}", cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_template_requires_cmd_placeholder() {
        // With {cmd}: substituted. Without: None, and the caller falls through
        // to its own defaults (a bare `open -a iTerm` can't carry a command).
        assert_eq!(
            resolve_terminal_template(Some("alacritty -e {cmd}"), "wsx --select r/s"),
            Some("alacritty -e wsx --select r/s".into())
        );
        assert_eq!(resolve_terminal_template(Some("open -a iTerm"), "x"), None);
        assert_eq!(resolve_terminal_template(None, "x"), None);
        assert_eq!(resolve_terminal_template(Some("  "), "x"), None);
    }
}
