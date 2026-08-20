//! Rich walker/elephant menu entries: `wsx waybar menu-entries --json`
//! emits display-ready rows; `wsx waybar refresh-prs` sweeps the PR cache.
//! See docs/superpowers/specs/2026-07-26-elephant-menu-design.md.

use crate::data::scm_cache::ScmCacheRow;
use crate::data::store::ReportedState;
use crate::data::store::ReportedStatus;
use crate::data::store::Store;
use crate::error::Result;
use crate::git::forge::BranchLifecycle;
use crate::workspace_rows::{RowInput, collect_rows_fresh, sanitize};

const GLYPH_BRANCH: &str = "\u{e0a0}"; // powerline branch
const GLYPH_PR: &str = "\u{f407}"; // nf-oct-git_pull_request
const GLYPH_MERGED: &str = "\u{f419}"; // nf-oct-git_merge
const GLYPH_DIRTY: &str = "\u{25cf}"; // ●

// Fixed-column text layout. Walker renders entry text with GTK's set_text(),
// which force-disables Pango markup — but a label's `attributes` property
// (static color ranges over BYTE offsets) survives set_text(). The wsx walker
// theme's item_menus-wsx.xml carries such ranges, so every field below must
// sit at a constant byte offset in every row:
// - the name column is forced to ASCII (chars == bytes) and padded or
//   truncated to NAME_W,
// - number/suffix fields are ASCII, space-padded to fixed widths,
// - the 1-char glyphs (GLYPH_PR, GLYPH_MERGED, GLYPH_DIRTY) are all 3-byte
//   UTF-8 and blank out to FIGURE SPACE (also 3 bytes, digit-width in the
//   theme's monospace font), keeping byte AND visual alignment.
// `xml_attribute_ranges_match_layout` asserts the theme XML stays in sync.
const NAME_W: usize = 36;
const PR_W: usize = 6; // "#" + up to 5 digits
const SUFFIX_W: usize = 8; // "conflict" is the widest suffix
const COUNT_W: usize = 6; // sign + up to 5 digits
const BLANK_GLYPH: &str = "\u{2007}"; // figure space: 3 bytes, digit width

/// Byte offsets of the colorable fields, mirrored by the Pango attribute
/// ranges in assets/walker-theme/item_menus-wsx.xml.
pub(crate) const PR_START: usize = NAME_W + 2; // glyph + " " + "#N"
pub(crate) const PR_END: usize = PR_START + 3 + 1 + PR_W;
pub(crate) const REVIEW_START: usize = PR_END + 2 + SUFFIX_W + 2;
pub(crate) const REVIEW_END: usize = REVIEW_START + 3;
pub(crate) const DIRTY_START: usize = REVIEW_END + 2;
pub(crate) const DIRTY_END: usize = DIRTY_START + 3;
pub(crate) const ADDS_START: usize = DIRTY_END + 2;
pub(crate) const ADDS_END: usize = ADDS_START + COUNT_W;
pub(crate) const DELS_START: usize = ADDS_END + 1;
pub(crate) const DELS_END: usize = DELS_START + COUNT_W;

#[derive(serde::Serialize, Debug, PartialEq)]
pub struct MenuEntry {
    pub text: String,
    pub subtext: String,
    pub icon: String,
    pub action: String,
    /// Row CSS classes: walker adds each string as a class on the item box,
    /// which the wsx walker theme styles (colored edge per PR state, etc.).
    pub state: Vec<String>,
}

/// The padded name column: ASCII-only so chars == bytes (a non-ASCII char
/// would shift every colored range after it), truncated with ".." when it
/// overflows the column.
fn name_column(repo: &str, slug: &str) -> String {
    let mut name: String = format!("{}/{}", sanitize(repo), sanitize(slug))
        .chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect();
    if name.len() > NAME_W {
        name.truncate(NAME_W - 2);
        name.push_str("..");
    }
    name
}

pub(crate) fn compose_text(repo: &str, slug: &str, row: &ScmCacheRow) -> String {
    // Suffix words mirror the TUI's lifecycle labels; open/merged need none
    // (the merge glyph and the row's border color already carry it). An
    // unfetched state and NoPr render identically — an unknown must not
    // claim "no PR", and "no PR" earns no indicator.
    let (glyph, num, suffix) = match row.pr_lifecycle {
        Some(BranchLifecycle::PrOpen) => (GLYPH_PR, pr_num(row), ""),
        Some(BranchLifecycle::PrDraft) => (GLYPH_PR, pr_num(row), "draft"),
        Some(BranchLifecycle::PrConflicted) => (GLYPH_PR, pr_num(row), "conflict"),
        Some(BranchLifecycle::PrMerged) => (GLYPH_MERGED, pr_num(row), ""),
        Some(BranchLifecycle::PrClosed) => (GLYPH_PR, pr_num(row), "closed"),
        Some(BranchLifecycle::NoPr) | None => (BLANK_GLYPH, String::new(), ""),
    };
    // The approval mark gets its own fixed slot. A Pango attribute range is
    // a single static color, so unlike the TUI chip this mark can't be
    // colored per verdict — the glyph shapes carry the whole distinction and
    // the theme paints the slot one neutral hue.
    let review = row
        .pr_review
        .filter(|_| {
            row.pr_lifecycle
                .is_some_and(crate::ui::theme::lifecycle_shows_review)
        })
        .map(crate::ui::theme::review_glyph)
        .unwrap_or(BLANK_GLYPH);
    let dirty = if row.dirty == Some(true) {
        GLYPH_DIRTY
    } else {
        BLANK_GLYPH
    };
    let (adds, dels) = match (row.additions, row.deletions) {
        (Some(a), Some(d)) if a > 0 || d > 0 => {
            (format!("+{}", a.min(99_999)), format!("-{}", d.min(99_999)))
        }
        _ => (String::new(), String::new()),
    };
    let text = format!(
        "{name:<NAME_W$}  {glyph} {num:<PR_W$}  {suffix:<SUFFIX_W$}  {review}  {dirty}  \
         {adds:<COUNT_W$} {dels:<COUNT_W$}",
        name = name_column(repo, slug),
    );
    debug_assert_eq!(text.len(), DELS_END, "column layout drifted: {text:?}");
    debug_assert!(
        [
            PR_START,
            PR_END,
            REVIEW_START,
            REVIEW_END,
            DIRTY_START,
            DIRTY_END,
            ADDS_START,
            DELS_START
        ]
        .iter()
        .all(|&i| text.is_char_boundary(i)),
        "colored field off a char boundary: {text:?}"
    );
    text.trim_end().to_string()
}

/// "#N" or empty when the number was never fetched. Clamped so the field
/// can't overflow its column (a six-digit PR number is theoretical anyway).
fn pr_num(row: &ScmCacheRow) -> String {
    row.pr_number
        .map(|n| format!("#{}", n.min(99_999)))
        .unwrap_or_default()
}

/// Row CSS classes derived from PR/git/agent state. Walker turns each into
/// a class on the item box; the wsx walker theme colors row edges/tints.
pub(crate) fn state_classes(row: &ScmCacheRow, status: Option<&ReportedStatus>) -> Vec<String> {
    let mut classes = Vec::new();
    let pr = row.pr_lifecycle.and_then(|l| match l {
        BranchLifecycle::NoPr => None,
        BranchLifecycle::PrOpen => Some("pr-open"),
        BranchLifecycle::PrDraft => Some("pr-draft"),
        BranchLifecycle::PrConflicted => Some("pr-conflicted"),
        BranchLifecycle::PrMerged => Some("pr-merged"),
        BranchLifecycle::PrClosed => Some("pr-closed"),
    });
    if let Some(pr) = pr {
        classes.push(pr.to_string());
    }
    if let Some(d) = row.pr_review.filter(|_| {
        row.pr_lifecycle
            .is_some_and(crate::ui::theme::lifecycle_shows_review)
    }) {
        classes.push(
            match d {
                crate::git::forge::ReviewDecision::Approved => "review-approved",
                crate::git::forge::ReviewDecision::ChangesRequested => "review-changes-requested",
                crate::git::forge::ReviewDecision::ReviewRequired => "review-required",
            }
            .to_string(),
        );
    }
    if row.dirty == Some(true) {
        classes.push("dirty".to_string());
    }
    if status.map(|s| s.state) == Some(ReportedState::Blocked) {
        classes.push("blocked".to_string());
    }
    classes
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

/// Menu icon per agent state — same visual language as the waybar bar
/// glyphs, but every value must be NON-ASCII: walker treats an ASCII icon
/// string as an icon-theme NAME, and a failed lookup renders the red
/// "missing image" symbol (the bar's blocked glyph "!" hit exactly this).
fn icon_glyph(state: Option<ReportedState>) -> &'static str {
    match state {
        Some(ReportedState::Blocked) => "\u{f12a}", // nf-fa-exclamation
        other => crate::workspace_rows::state_glyph(other),
    }
}

pub(crate) fn build_entries(rows: &[RowInput], wsx_bin: &str) -> Vec<MenuEntry> {
    rows.iter()
        .map(|r| MenuEntry {
            text: compose_text(&r.repo_name, &r.slug, &r.cache),
            subtext: compose_subtext(&r.branch, r.status.as_ref()),
            icon: icon_glyph(r.status.as_ref().map(|s| s.state)).to_string(),
            action: action_cmd(wsx_bin, &r.repo_name, &r.slug),
            state: state_classes(&r.cache, r.status.as_ref()),
        })
        .collect()
}

/// Fire-and-forget `wsx waybar refresh-prs` so PR data self-heals by the
/// next menu open even when the TUI is not running.
fn spawn_pr_sweep(wsx_bin: &str) {
    use std::process::Stdio;
    let _ = std::process::Command::new(wsx_bin)
        .args(["waybar", "refresh-prs"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

pub async fn run_menu_entries(store: &Store) -> Result<()> {
    let rows = collect_rows_fresh(store).await?;
    let wsx_bin = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "wsx".into());
    let entries = build_entries(&rows, &wsx_bin);
    // Serialization of plain strings cannot fail.
    println!(
        "{}",
        serde_json::to_string(&entries).expect("serialize entries")
    );
    spawn_pr_sweep(&wsx_bin);
    Ok(())
}

pub use crate::workspace_rows::run_refresh_prs;

#[cfg(test)]
mod entry_tests {
    use super::*;
    use crate::data::store::{ReportedState, ReportedStatus};
    use crate::git::forge::ReviewDecision;

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
    fn text_with_all_indicators_at_fixed_byte_offsets() {
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrOpen),
            pr_number: Some(123),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            fetched_at: Some(0),
            ..Default::default()
        };
        let text = compose_text("workspacex", "fix-bug", &row);
        assert!(text.starts_with("workspacex/fix-bug"), "{text}");
        // Byte-slicing at the theme's attribute offsets: a slice off a char
        // boundary would panic, which is itself an alignment failure.
        assert_eq!(&text[PR_START..PR_END], "\u{f407} #123  ", "{text}");
        assert_eq!(&text[DIRTY_START..DIRTY_END], "\u{25cf}", "{text}");
        assert_eq!(&text[ADDS_START..ADDS_END], "+45   ", "{text}");
        // The row is trim_end'ed, so the last field loses its padding.
        assert_eq!(&text[DELS_START..], "-12", "{text}");
    }

    #[test]
    fn offsets_hold_for_every_field_combination() {
        // Vary name length, PR digits, suffix presence, dirty, and diff
        // magnitudes: the colored fields must never move.
        let rows = [
            ("r", "w", BranchLifecycle::PrDraft, Some(7), false, 1, 0),
            (
                "workspacex",
                "some-quite-long-workspace-name",
                BranchLifecycle::PrConflicted,
                Some(99999),
                true,
                99999,
                99999,
            ),
            ("a", "b", BranchLifecycle::PrClosed, None, true, 0, 3),
        ];
        for (repo, slug, l, n, dirty, a, d) in rows {
            let row = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: n,
                dirty: Some(dirty),
                additions: Some(a),
                deletions: Some(d),
                fetched_at: Some(0),
                ..Default::default()
            };
            let text = compose_text(repo, slug, &row);
            assert!(text.is_char_boundary(PR_START), "{text}");
            assert_eq!(&text[PR_START..PR_START + 3], "\u{f407}", "{text}");
            match n {
                Some(n) => {
                    assert!(text[PR_START..PR_END].contains(&format!("#{n}")), "{text}")
                }
                None => assert!(!text.contains('#'), "{text}"),
            }
            let dirty_field = &text[DIRTY_START..DIRTY_END];
            if dirty {
                assert_eq!(dirty_field, "\u{25cf}", "{text}");
            } else {
                assert_eq!(dirty_field, "\u{2007}", "{text}");
            }
            assert_eq!(
                text.as_bytes()[ADDS_START],
                b'+',
                "adds column moved: {text}"
            );
            assert_eq!(
                text.as_bytes()[DELS_START],
                b'-',
                "dels column moved: {text}"
            );
        }
    }

    #[test]
    fn name_column_truncates_and_forces_ascii() {
        let long = "a-very-long-workspace-name-that-overflows";
        let text = compose_text("workspacex", long, &ScmCacheRow::default());
        assert_eq!(text.len(), NAME_W, "truncated name fills the column");
        assert!(text.ends_with(".."), "{text}");

        let row = ScmCacheRow {
            additions: Some(1),
            deletions: Some(0),
            ..Default::default()
        };
        let text = compose_text("répo", "wörk", &row);
        assert!(text.starts_with("r?po/w?rk"), "{text}");
        assert_eq!(&text[ADDS_START..ADDS_END], "+1    ", "{text}");
    }

    #[test]
    fn approval_mark_sits_in_its_own_fixed_byte_column() {
        for (verdict, glyph) in [
            (ReviewDecision::Approved, "✓"),
            (ReviewDecision::ChangesRequested, "✗"),
            (ReviewDecision::ReviewRequired, "◌"),
        ] {
            let row = ScmCacheRow {
                pr_lifecycle: Some(BranchLifecycle::PrOpen),
                pr_number: Some(123),
                pr_review: Some(verdict),
                dirty: Some(true),
                additions: Some(45),
                deletions: Some(12),
                fetched_at: Some(0),
                ..Default::default()
            };
            let text = compose_text("workspacex", "fix-bug", &row);
            assert_eq!(
                &text[REVIEW_START..REVIEW_END],
                glyph,
                "{verdict:?}: {text}"
            );
            // The fields after it must not have shifted.
            assert_eq!(&text[DIRTY_START..DIRTY_END], "\u{25cf}", "{text}");
            assert_eq!(&text[ADDS_START..ADDS_END], "+45   ", "{text}");
        }
    }

    #[test]
    fn an_unmarked_row_blanks_the_column_without_moving_it() {
        // No verdict, and a lifecycle that earns no mark, must both leave a
        // figure space — same 3 bytes, same digit width — so every later
        // colored range stays put.
        let base = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrOpen),
            pr_number: Some(123),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            fetched_at: Some(0),
            ..Default::default()
        };
        let merged = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrMerged),
            pr_review: Some(ReviewDecision::Approved),
            ..base.clone()
        };
        for row in [base.clone(), merged] {
            let text = compose_text("workspacex", "fix-bug", &row);
            assert_eq!(&text[REVIEW_START..REVIEW_END], BLANK_GLYPH, "{text}");
            assert_eq!(&text[DIRTY_START..DIRTY_END], "\u{25cf}", "{text}");
        }
    }

    #[test]
    fn a_marked_row_earns_a_review_state_class() {
        // The walker theme can tint the row edge by verdict; the class is
        // how it learns which one.
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrOpen),
            pr_number: Some(7),
            pr_review: Some(ReviewDecision::ChangesRequested),
            ..Default::default()
        };
        assert!(
            state_classes(&row, None).contains(&"review-changes-requested".to_string()),
            "{:?}",
            state_classes(&row, None)
        );
        // An unmarked row earns none of them.
        let plain = ScmCacheRow {
            pr_review: None,
            ..row.clone()
        };
        assert!(
            !state_classes(&plain, None)
                .iter()
                .any(|c| c.starts_with("review-")),
            "{:?}",
            state_classes(&plain, None)
        );
    }

    #[test]
    fn xml_attribute_ranges_match_layout() {
        // The walker theme colors byte ranges of the composed text; if the
        // column constants move, the theme XML must move with them.
        let xml = include_str!("assets/walker-theme/item_menus-wsx.xml");
        for (start, end) in [
            (PR_START, PR_END),
            (REVIEW_START, REVIEW_END),
            (DIRTY_START, DIRTY_END),
            (ADDS_START, ADDS_END),
            (DELS_START, DELS_END),
        ] {
            assert!(
                xml.contains(&format!("start=\"{start}\" end=\"{end}\"")),
                "item_menus-wsx.xml missing attribute range {start}..{end}"
            );
        }
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
    fn icon_glyphs_are_never_ascii_icon_names() {
        // Walker resolves ASCII icon strings as icon-theme names; a failed
        // lookup renders the red "missing image" symbol. Every state must
        // therefore map to a non-ASCII glyph.
        for state in [
            None,
            Some(ReportedState::Working),
            Some(ReportedState::Waiting),
            Some(ReportedState::Blocked),
            Some(ReportedState::Done),
            Some(ReportedState::Busy),
        ] {
            let icon = icon_glyph(state);
            assert!(
                !icon.is_ascii(),
                "{state:?} icon {icon:?} would be looked up as an icon name"
            );
        }
    }

    #[test]
    fn menu_entry_serializes_with_lowercase_keys() {
        let e = MenuEntry {
            text: "t".into(),
            subtext: "s".into(),
            icon: "i".into(),
            action: "a".into(),
            state: vec!["pr-open".into()],
        };
        let v = serde_json::to_value([e]).unwrap();
        assert_eq!(v[0]["text"], "t");
        assert_eq!(v[0]["subtext"], "s");
        assert_eq!(v[0]["icon"], "i");
        assert_eq!(v[0]["action"], "a");
        assert_eq!(v[0]["state"][0], "pr-open");
    }

    #[test]
    fn no_emoji_dots_in_any_lifecycle() {
        // Lifecycle/diff colors come from the walker theme's Pango attribute
        // ranges now — the color-font emoji dots must never come back.
        for l in [
            BranchLifecycle::PrOpen,
            BranchLifecycle::PrDraft,
            BranchLifecycle::PrConflicted,
            BranchLifecycle::PrMerged,
            BranchLifecycle::PrClosed,
        ] {
            let row = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: Some(7),
                dirty: Some(true),
                additions: Some(4),
                deletions: Some(2),
                fetched_at: Some(0),
                ..Default::default()
            };
            let text = compose_text("r", "w", &row);
            for d in ['\u{1f7e2}', '\u{1f7e0}', '\u{1f7e3}', '\u{1f534}'] {
                assert!(!text.contains(d), "{l:?}: {text}");
            }
        }
    }

    #[test]
    fn state_classes_reflect_pr_dirty_and_blocked() {
        assert!(state_classes(&ScmCacheRow::default(), None).is_empty());
        let row = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrConflicted),
            dirty: Some(true),
            ..Default::default()
        };
        let blocked = status(ReportedState::Blocked, None);
        assert_eq!(
            state_classes(&row, Some(&blocked)),
            vec!["pr-conflicted", "dirty", "blocked"]
        );
        // NoPr earns no class — same no-indicator rule as the text segment.
        let no_pr = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::NoPr),
            dirty: Some(false),
            ..Default::default()
        };
        assert!(state_classes(&no_pr, None).is_empty());
    }

    #[tokio::test]
    async fn collect_rows_sorted_and_composed() {
        use crate::data::store::{NewWorkspace, Store};
        use crate::pty::session::AgentKind;

        let store = Store::open_in_memory().unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "x")
            .unwrap();
        let mut ids = vec![];
        for name in ["zeta", "alpha"] {
            ids.push(
                store
                    .insert_workspace(&NewWorkspace {
                        repo_id: repo,
                        name,
                        branch: &format!("x/{name}"),
                        worktree_path: &std::path::PathBuf::from(format!("/nonexistent/r/{name}")),
                        yolo: false,
                        agent: AgentKind::Claude,
                        shared: false,
                    })
                    .unwrap(),
            );
        }
        store
            .upsert_scm_pr(
                ids[0],
                &crate::git::forge::PrStatus {
                    lifecycle: crate::git::forge::BranchLifecycle::PrOpen,
                    number: Some(5),
                    url: None,
                    review: None,
                },
                0,
            )
            .unwrap();
        // Pre-seed zeta (ids[0]) with stale dirty/diff indicators: when
        // git fails on its nonexistent worktree, these must be suppressed
        // in-memory (not persisted to DB), so the row renders without ● and
        // +N −N but keeps its PR indicator (#5).
        store.upsert_scm_git(ids[0], true, 4, 2, 0).unwrap();

        let rows = crate::workspace_rows::collect_rows_fresh(&store)
            .await
            .unwrap();
        let entries = super::build_entries(&rows, "/bin/wsx");

        assert_eq!(entries.len(), 2);
        // Sorted by workspace name within repo.
        assert!(entries[0].text.starts_with("r/alpha"), "{:?}", entries[0]);
        assert!(entries[1].text.starts_with("r/zeta"), "{:?}", entries[1]);
        // Branch always present in subtext.
        assert!(entries[0].subtext.contains("x/alpha"), "{:?}", entries[0]);
        assert_eq!(entries[0].action, "/bin/wsx waybar jump r alpha");
        // zeta (entries[1]) carries the cached PR indicator #5 even though
        // its worktree is missing. Stale dirty/diff indicators are suppressed
        // when git fails: text contains #5 but NOT ● or +4.
        assert!(entries[1].text.contains("#5"), "{:?}", entries[1]);
        assert!(
            !entries[1].text.contains('\u{25cf}'),
            "stale dirty indicator should be suppressed: {:?}",
            entries[1]
        );
        assert!(
            !entries[1].text.contains("+4"),
            "stale diff indicator should be suppressed: {:?}",
            entries[1]
        );
    }
}
