//! Rich walker/elephant menu entries: `wsx waybar menu-entries --json`
//! emits display-ready rows; `wsx waybar refresh-prs` sweeps the PR cache.
//! See docs/superpowers/specs/2026-07-26-elephant-menu-design.md.

use crate::data::scm_cache::ScmCacheRow;
use crate::data::store::ReportedStatus;
use crate::git::forge::BranchLifecycle;
use crate::waybar::menu::sanitize;

/// Skip `gh` for a workspace whose PR state was fetched more recently than
/// this. Matches the spirit of the TUI's 30s in-memory throttle but is more
/// conservative: menu opens are burstier than TUI ticks.
pub const PR_REFRESH_THROTTLE_SECS: i64 = 120;

const GLYPH_BRANCH: &str = "\u{e0a0}"; // powerline branch
const GLYPH_PR: &str = "\u{f407}"; // nf-oct-git_pull_request
const GLYPH_MERGED: &str = "\u{f419}"; // nf-oct-git_merge
const GLYPH_DIRTY: &str = "\u{25cf}"; // ●

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct MenuEntry {
    pub text: String,
    pub subtext: String,
    pub icon: String,
    pub action: String,
}

pub(crate) fn needs_pr_refresh(fetched_at: Option<i64>, now: i64) -> bool {
    match fetched_at {
        None => true,
        Some(t) => now.saturating_sub(t) >= PR_REFRESH_THROTTLE_SECS,
    }
}

/// PR indicator, or None when there is no PR or the state was never fetched
/// (deliberately identical renderings — an unknown must not claim "no PR",
/// and "no PR" earns no glyph).
fn pr_segment(row: &ScmCacheRow) -> Option<String> {
    let lifecycle = row.pr_lifecycle?;
    let (glyph, suffix) = match lifecycle {
        BranchLifecycle::NoPr => return None,
        BranchLifecycle::PrOpen => (GLYPH_PR, None),
        BranchLifecycle::PrDraft => (GLYPH_PR, Some("draft")),
        BranchLifecycle::PrConflicted => (GLYPH_PR, Some("conflict")),
        BranchLifecycle::PrMerged => (GLYPH_MERGED, None),
        BranchLifecycle::PrClosed => (GLYPH_PR, Some("closed")),
    };
    let mut parts = vec![glyph.to_string()];
    if let Some(n) = row.pr_number {
        parts.push(format!("#{n}"));
    }
    if let Some(s) = suffix {
        parts.push(s.to_string());
    }
    Some(parts.join(" "))
}

pub(crate) fn compose_text(repo: &str, slug: &str, row: &ScmCacheRow) -> String {
    let mut parts = vec![format!("{}/{}", sanitize(repo), sanitize(slug))];
    if let Some(pr) = pr_segment(row) {
        parts.push(pr);
    }
    if row.dirty == Some(true) {
        parts.push(GLYPH_DIRTY.to_string());
    }
    if let (Some(a), Some(d)) = (row.additions, row.deletions) {
        if a + d > 0 {
            parts.push(format!("+{a} \u{2212}{d}"));
        }
    }
    parts.join("  ")
}

pub(crate) fn compose_subtext(branch: &str, status: Option<&ReportedStatus>) -> String {
    let b = format!("{GLYPH_BRANCH} {}", sanitize(branch));
    let Some(s) = status else {
        return b;
    };
    match s.message.as_deref().filter(|m| !m.is_empty()) {
        Some(m) => format!("{b} \u{2014} {}: {}", s.state.as_str(), sanitize(m)),
        None => format!("{b} \u{2014} {}", s.state.as_str()),
    }
}

fn quote(s: &str) -> String {
    shlex::try_quote(s)
        .map(|c| c.into_owned())
        // Only fails on interior NUL, which cannot survive sqlite TEXT
        // anyway; drop the offending byte rather than emit an unquoted arg.
        .unwrap_or_else(|_| format!("'{}'", s.replace(['\'', '\0'], "")))
}

pub(crate) fn action_cmd(wsx_bin: &str, repo: &str, slug: &str) -> String {
    format!(
        "{} waybar jump {} {}",
        quote(wsx_bin),
        quote(repo),
        quote(slug)
    )
}

#[cfg(test)]
mod entry_tests {
    use super::*;
    use crate::data::store::{ReportedState, ReportedStatus};

    fn status(state: ReportedState, msg: Option<&str>) -> ReportedStatus {
        ReportedStatus {
            state,
            message: msg.map(str::to_string),
            source: "test".into(),
            reported_at: 0,
        }
    }

    #[test]
    fn text_plain_when_cache_empty() {
        assert_eq!(
            compose_text("r", "w", &ScmCacheRow::default()),
            "r/w".to_string()
        );
    }

    #[test]
    fn text_with_all_indicators() {
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrOpen),
            pr_number: Some(123),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            fetched_at: Some(0),
        };
        let text = compose_text("workspacex", "fix-bug", &row);
        assert!(text.starts_with("workspacex/fix-bug"), "{text}");
        assert!(text.contains("#123"), "{text}");
        assert!(text.contains('\u{25cf}'), "{text}");
        assert!(text.contains("+45 \u{2212}12"), "{text}");
    }

    #[test]
    fn no_pr_and_unknown_render_identically() {
        let unknown = ScmCacheRow::default();
        let no_pr = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::NoPr),
            fetched_at: Some(0),
            ..Default::default()
        };
        assert_eq!(
            compose_text("r", "w", &unknown),
            compose_text("r", "w", &no_pr)
        );
    }

    #[test]
    fn draft_conflict_closed_carry_labels() {
        for (l, label) in [
            (BranchLifecycle::PrDraft, "draft"),
            (BranchLifecycle::PrConflicted, "conflict"),
            (BranchLifecycle::PrClosed, "closed"),
        ] {
            let row = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: Some(7),
                ..Default::default()
            };
            let text = compose_text("r", "w", &row);
            assert!(text.contains(label), "{l:?}: {text}");
            assert!(text.contains("#7"), "{l:?}: {text}");
        }
    }

    #[test]
    fn merged_uses_merge_glyph_without_label() {
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrMerged),
            pr_number: Some(9),
            ..Default::default()
        };
        let text = compose_text("r", "w", &row);
        assert!(text.contains('\u{f419}'), "{text}");
        assert!(!text.contains("merged"), "{text}");
    }

    #[test]
    fn clean_zero_diff_shows_no_noise() {
        let row = ScmCacheRow {
            dirty: Some(false),
            additions: Some(0),
            deletions: Some(0),
            ..Default::default()
        };
        assert_eq!(compose_text("r", "w", &row), "r/w");
    }

    #[test]
    fn subtext_variants() {
        assert_eq!(compose_subtext("x/w", None), "\u{e0a0} x/w".to_string());
        assert_eq!(
            compose_subtext("x/w", Some(&status(ReportedState::Working, Some("fixing")))),
            "\u{e0a0} x/w \u{2014} working: fixing".to_string()
        );
        assert_eq!(
            compose_subtext("x/w", Some(&status(ReportedState::Done, None))),
            "\u{e0a0} x/w \u{2014} done".to_string()
        );
    }

    #[test]
    fn subtext_sanitizes_newlines() {
        let s = compose_subtext("x/w", Some(&status(ReportedState::Working, Some("a\nb"))));
        assert!(!s.contains('\n'), "{s}");
    }

    #[test]
    fn action_cmd_quotes_spacey_repo() {
        let cmd = action_cmd("/usr/bin/wsx", "meals backend", "api-fix");
        assert_eq!(cmd, "/usr/bin/wsx waybar jump 'meals backend' api-fix");
    }

    #[test]
    fn throttle_decision() {
        assert!(needs_pr_refresh(None, 1000));
        assert!(needs_pr_refresh(Some(880), 1000));
        assert!(!needs_pr_refresh(Some(881), 1000));
        assert!(!needs_pr_refresh(Some(2000), 1000)); // clock skew: don't refetch
    }

    #[test]
    fn menu_entry_serializes_with_lowercase_keys() {
        let e = MenuEntry {
            text: "t".into(),
            subtext: "s".into(),
            icon: "i".into(),
            action: "a".into(),
        };
        let v = serde_json::to_value([e]).unwrap();
        assert_eq!(v[0]["text"], "t");
        assert_eq!(v[0]["subtext"], "s");
        assert_eq!(v[0]["icon"], "i");
        assert_eq!(v[0]["action"], "a");
    }
}
