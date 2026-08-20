//! `wsx menubar plugin`: renders the full SwiftBar document (header line,
//! separator, menu body) from cache-only workspace rows. Never fails —
//! errors degrade to an icon-only header. After printing, spawns the
//! detached `wsx menubar refresh` sweep so indicators self-heal.
//! See docs/superpowers/specs/2026-07-29-macos-menubar-design.md.

use std::path::Path;

use crate::data::scm_cache::ScmCacheRow;
use crate::data::store::{ReportedState, Store};
use crate::error::Result;
use crate::git::forge::BranchLifecycle;
use crate::menubar::escape::{esc_text, quote_param};
use crate::workspace_rows::{RowInput, attention_rank, collect_rows_cached, state_glyph};

const SF_SYMBOL: &str = "arrow.triangle.branch";
pub(crate) const ROW_FONT: &str = "font=SFMono-Regular size=12";

/// light,dark hex pairs for SwiftBar's per-appearance sfcolor.
fn sfcolor(state: ReportedState) -> &'static str {
    match state {
        ReportedState::Blocked => "#c92a2a,#ff6b6b",
        ReportedState::Done => "#2f9e44,#69db7c",
        ReportedState::Waiting => "#b08800,#ffd43b",
        ReportedState::Working | ReportedState::Busy => "#1971c2,#4dabf7",
    }
}

pub(crate) fn error_header() -> String {
    format!("| sfimage={SF_SYMBOL}")
}

pub(crate) fn header_line(count: usize, best: Option<ReportedState>) -> String {
    match best {
        Some(state) => format!("{count} | sfimage={SF_SYMBOL} sfcolor={}", sfcolor(state)),
        None => format!("{count} | sfimage={SF_SYMBOL}"),
    }
}

pub(crate) fn pr_field(c: &ScmCacheRow) -> String {
    let word = match c.pr_lifecycle {
        Some(BranchLifecycle::PrDraft) => "draft",
        Some(BranchLifecycle::PrConflicted) => "conflict",
        Some(BranchLifecycle::PrMerged) => "merged",
        Some(BranchLifecycle::PrClosed) => "closed",
        Some(BranchLifecycle::PrOpen) => "",
        Some(BranchLifecycle::NoPr) | None => return String::new(),
    };
    let field = match (c.pr_number, word) {
        (Some(n), "") => format!("#{n}"),
        (Some(n), w) => format!("#{n} {w}"),
        (None, "") => String::new(),
        (None, w) => w.to_string(),
    };
    // Approval mark, gated by the same predicate the TUI chip uses so the
    // menu and the dashboard can't disagree: open lifecycles only, so a
    // merged PR's stale APPROVED verdict stays out of the menu.
    // The mark rides an otherwise-empty field too — a PR whose number was
    // never cached still earns its verdict.
    match c.pr_review.filter(|_| {
        c.pr_lifecycle
            .is_some_and(crate::ui::theme::lifecycle_shows_review)
    }) {
        Some(d) => {
            let mark = crate::ui::theme::review_glyph(d);
            if field.is_empty() {
                mark.to_string()
            } else {
                format!("{field} {mark}")
            }
        }
        None => field,
    }
}

pub(crate) fn row_line(r: &RowInput) -> String {
    let mut cols = vec![format!(
        "{} {}",
        state_glyph(r.status.as_ref().map(|s| s.state)),
        esc_text(&r.slug)
    )];
    let pr = pr_field(&r.cache);
    if !pr.is_empty() {
        cols.push(pr);
    }
    if r.cache.dirty == Some(true) {
        cols.push("\u{25cf}".into());
    }
    if let (Some(a), Some(d)) = (r.cache.additions, r.cache.deletions)
        && (a > 0 || d > 0)
    {
        cols.push(format!("+{a} -{d}"));
    }
    format!("{} | {ROW_FONT}", cols.join("  "))
}

/// `branch — state: message`, the info the Linux menu shows as subtext.
fn subtitle(r: &RowInput) -> String {
    let b = r.branch.clone();
    match &r.status {
        None => b,
        Some(s) => match s.message.as_deref().filter(|m| !m.is_empty()) {
            Some(m) => format!("{b} \u{2014} {}: {}", s.state.as_str(), m),
            None => format!("{b} \u{2014} {}", s.state.as_str()),
        },
    }
}

pub(crate) fn submenu_lines(r: &RowInput, wsx_bin: &str) -> Vec<String> {
    let wt = r.worktree_path.display().to_string();
    let mut out = vec![format!("-- {} | disabled=true", esc_text(&subtitle(r)))];
    out.push(format!(
        "-- Jump | bash={} param1=\"menubar\" param2=\"jump\" param3={} param4={} terminal=false",
        quote_param(wsx_bin),
        quote_param(&r.repo_name),
        quote_param(&r.slug),
    ));
    if let (Some(n), Some(url)) = (r.cache.pr_number, r.cache.pr_url.as_deref()) {
        out.push(format!(
            "-- Open PR #{n} in browser | href={}",
            quote_param(url)
        ));
    }
    out.push(format!(
        "-- Copy worktree path | bash={} param1=\"menubar\" param2=\"copy-path\" param3={} param4={} terminal=false",
        quote_param(wsx_bin),
        quote_param(&r.repo_name),
        quote_param(&r.slug),
    ));
    out.push(format!(
        "-- Reveal in Finder | bash=\"/usr/bin/open\" param1=\"-R\" param2={} terminal=false",
        quote_param(&wt),
    ));
    out
}

pub(crate) fn render(
    repo_names: &[String],
    rows: &[RowInput],
    recaps: &std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::data::store::WorkspaceRecap,
    >,
    wsx_bin: &str,
    now_ms: i64,
) -> String {
    let best = rows
        .iter()
        .filter_map(|r| r.status.as_ref().map(|s| s.state))
        .max_by_key(|s| attention_rank(*s));
    let mut lines = vec![header_line(rows.len(), best), "---".into()];
    for repo in repo_names {
        lines.push(format!("{} | disabled=true", esc_text(repo)));
        let mut any = false;
        for r in rows.iter().filter(|r| &r.repo_name == repo) {
            any = true;
            lines.push(row_line(r));
            lines.extend(submenu_lines(r, wsx_bin));
        }
        if !any {
            lines.push("(no workspaces) | disabled=true".into());
        }
    }
    lines.push("---".into());
    lines.push("Project Manager".into());
    lines.extend(crate::menubar::pm::pm_section_lines(
        repo_names, rows, recaps, wsx_bin, now_ms,
    ));
    lines.push("---".into());
    lines.push("Refresh | refresh=true".into());
    lines.join("\n")
}

/// Full document: the "no registered repos" quiet state degrades to the
/// same icon-only header as an error (no `---`, no footer — SwiftBar shows
/// a bare menubar item with no dropdown content), otherwise the composed
/// header + menu body.
fn document(
    repo_names: &[String],
    rows: &[RowInput],
    recaps: &std::collections::HashMap<
        crate::data::store::WorkspaceId,
        crate::data::store::WorkspaceRecap,
    >,
    wsx_bin: &str,
    now_ms: i64,
) -> String {
    if repo_names.is_empty() {
        return error_header();
    }
    render(repo_names, rows, recaps, wsx_bin, now_ms)
}

fn plugin_document(store: &Store, wsx_bin: &str) -> Result<String> {
    let mut repos = crate::data::repo::list(store)?;
    repos.sort_by(|a, b| a.name.cmp(&b.name));
    let names: Vec<String> = repos.into_iter().map(|r| r.name).collect();
    let rows = collect_rows_cached(store)?;
    // A recap read failure must not blank the whole menu — degrade to
    // "no recap yet" and keep every workspace row. Mirrors app.rs.
    let recaps = store.all_workspace_recaps().unwrap_or_default();
    Ok(document(
        &names,
        &rows,
        &recaps,
        wsx_bin,
        crate::time::now_ms(),
    ))
}

/// Fire-and-forget `wsx menubar refresh` so indicators self-heal by a
/// later poll (same contract as the waybar PR sweep).
fn spawn_refresh(wsx_bin: &str) {
    use std::process::Stdio;
    let _ = std::process::Command::new(wsx_bin)
        .args(["menubar", "refresh"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Never fails: SwiftBar polls this; on any error print the bare symbol
/// and exit 0 so the bar shows a quiet idle item, never an error string.
pub fn print_plugin(db_path: &Path) {
    let wsx_bin = crate::install_common::preferred_wsx_bin(dirs::home_dir());
    match Store::open(db_path).and_then(|s| plugin_document(&s, &wsx_bin)) {
        Ok(doc) => println!("{doc}"),
        Err(_) => println!("{}", error_header()),
    }
    spawn_refresh(&wsx_bin);
}

#[cfg(test)]
mod plugin_tests {
    use super::*;
    use crate::data::scm_cache::ScmCacheRow;
    use crate::data::store::{ReportedState, ReportedStatus};
    use crate::git::forge::BranchLifecycle;
    use crate::git::forge::ReviewDecision;
    use crate::workspace_rows::RowInput;

    fn status(state: ReportedState, msg: Option<&str>) -> ReportedStatus {
        ReportedStatus {
            state,
            message: msg.map(str::to_string),
            source: "test".into(),
            reported_at: 0,
        }
    }

    fn row(repo: &str, slug: &str) -> RowInput {
        RowInput {
            id: crate::data::store::WorkspaceId(0),
            repo_name: repo.into(),
            slug: slug.into(),
            branch: format!("x/{slug}"),
            worktree_path: format!("/wt/{repo}/{slug}").into(),
            status: None,
            cache: ScmCacheRow::default(),
        }
    }

    #[test]
    fn header_counts_and_colors_by_worst_state() {
        // Idle: count, symbol, no color.
        assert_eq!(header_line(4, None), "4 | sfimage=arrow.triangle.branch");
        // Blocked outranks working → red pair.
        let h = header_line(2, Some(ReportedState::Blocked));
        assert!(h.starts_with("2 | sfimage=arrow.triangle.branch"), "{h}");
        assert!(h.contains("sfcolor=#c92a2a,#ff6b6b"), "{h}");
    }

    #[test]
    fn error_header_is_icon_only() {
        assert_eq!(error_header(), "| sfimage=arrow.triangle.branch");
    }

    #[test]
    fn row_line_composes_indicators() {
        let mut r = row("r", "fix-bug");
        r.status = Some(status(ReportedState::Working, None));
        r.cache = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrConflicted),
            pr_number: Some(123),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            ..Default::default()
        };
        let line = row_line(&r);
        assert!(line.starts_with("\u{21bb} fix-bug"), "{line}");
        assert!(line.contains("#123 conflict"), "{line}");
        assert!(line.contains('\u{25cf}'), "{line}");
        assert!(line.contains("+45 -12"), "{line}");
        assert!(line.ends_with("| font=SFMono-Regular size=12"), "{line}");
    }

    #[test]
    fn pr_field_rules_match_linux() {
        // Unknown and NoPr render identically: nothing.
        assert_eq!(pr_field(&ScmCacheRow::default()), "");
        let no_pr = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::NoPr),
            fetched_at: Some(0),
            ..Default::default()
        };
        assert_eq!(pr_field(&no_pr), "");
        // Open: bare number. Merged/draft/conflict/closed: word.
        for (l, expect) in [
            (BranchLifecycle::PrOpen, "#7"),
            (BranchLifecycle::PrDraft, "#7 draft"),
            (BranchLifecycle::PrConflicted, "#7 conflict"),
            (BranchLifecycle::PrMerged, "#7 merged"),
            (BranchLifecycle::PrClosed, "#7 closed"),
        ] {
            let c = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: Some(7),
                ..Default::default()
            };
            assert_eq!(pr_field(&c), expect, "{l:?}");
        }
    }

    #[test]
    fn pr_field_appends_the_approval_mark() {
        for (verdict, mark) in [
            (ReviewDecision::Approved, "✓"),
            (ReviewDecision::ChangesRequested, "✗"),
            (ReviewDecision::ReviewRequired, "◌"),
        ] {
            let c = ScmCacheRow {
                pr_lifecycle: Some(BranchLifecycle::PrOpen),
                pr_number: Some(7),
                pr_review: Some(verdict),
                ..Default::default()
            };
            assert_eq!(pr_field(&c), format!("#7 {mark}"), "{verdict:?}");
        }
        // Draft keeps its word and gains the mark.
        let draft = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrDraft),
            pr_number: Some(7),
            pr_review: Some(ReviewDecision::ReviewRequired),
            ..Default::default()
        };
        assert_eq!(pr_field(&draft), "#7 draft ◌");
    }

    #[test]
    fn pr_field_leaves_settled_prs_unmarked() {
        // Same gate as the TUI chip: a merged PR keeps its stale APPROVED
        // verdict in GitHub's API, and a permanent tick would be noise.
        for l in [BranchLifecycle::PrMerged, BranchLifecycle::PrClosed] {
            let c = ScmCacheRow {
                pr_lifecycle: Some(l),
                pr_number: Some(7),
                pr_review: Some(ReviewDecision::Approved),
                ..Default::default()
            };
            assert!(!pr_field(&c).contains('✓'), "{l:?}: {}", pr_field(&c));
        }
    }

    #[test]
    fn pr_field_without_a_verdict_is_unchanged() {
        let c = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrOpen),
            pr_number: Some(7),
            pr_review: None,
            ..Default::default()
        };
        assert_eq!(pr_field(&c), "#7");
    }

    #[test]
    fn clean_default_row_is_just_glyph_and_slug() {
        let line = row_line(&row("r", "w"));
        assert!(line.starts_with("\u{b7} w |"), "{line}");
        assert!(!line.contains('#'), "{line}");
    }

    #[test]
    fn submenu_has_jump_first_and_pr_only_when_cached() {
        let mut r = row("meals backend", "api-fix");
        r.status = Some(status(ReportedState::Blocked, Some("needs input")));
        let lines = submenu_lines(&r, "/usr/local/bin/wsx");
        // Subtitle first: branch — state: message, disabled (not clickable).
        assert_eq!(
            lines[0],
            "-- x/api-fix \u{2014} blocked: needs input | disabled=true"
        );
        assert_eq!(
            lines[1],
            "-- Jump | bash=\"/usr/local/bin/wsx\" param1=\"menubar\" param2=\"jump\" param3=\"meals backend\" param4=\"api-fix\" terminal=false"
        );
        assert!(!lines.iter().any(|l| l.contains("Open PR")), "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("-- Copy worktree path | bash=")
                    && l.contains("param2=\"copy-path\"")),
            "{lines:?}"
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Reveal in Finder | bash=\"/usr/bin/open\" param1=\"-R\"")),
            "{lines:?}"
        );

        r.cache.pr_number = Some(12);
        r.cache.pr_url = Some("https://github.com/o/r/pull/12".into());
        let lines = submenu_lines(&r, "/usr/local/bin/wsx");
        assert!(
            lines
                .iter()
                .any(|l| l == "-- Open PR #12 in browser | href=\"https://github.com/o/r/pull/12\""),
            "{lines:?}"
        );
    }

    #[test]
    fn text_cannot_inject_lines_or_params() {
        // A hostile status message: newline (new menu row) and pipe (param
        // separator) must both be neutralized.
        let mut r = row("r", "w");
        r.status = Some(status(
            ReportedState::Working,
            Some("evil\n-- fake | bash=\"/bin/rm\""),
        ));
        for line in submenu_lines(&r, "/bin/wsx") {
            assert!(!line.contains('\n'), "{line}");
        }
        let subtitle = &submenu_lines(&r, "/bin/wsx")[0];
        assert!(
            subtitle.contains('\u{00a6}'),
            "pipe not neutralized: {subtitle}"
        );
        assert!(!subtitle.contains(" | bash"), "{subtitle}");
    }

    #[test]
    fn pm_section_sits_between_the_repos_and_the_footer() {
        let rows = vec![row("alpha", "one")];
        let doc = render(
            &["alpha".into()],
            &rows,
            &std::collections::HashMap::new(),
            "/bin/wsx",
            0,
        );
        let lines: Vec<&str> = doc.lines().collect();
        let pm = lines.iter().position(|l| *l == "Project Manager").unwrap();
        let last_repo_row = lines
            .iter()
            .rposition(|l| l.starts_with("\u{b7} one"))
            .unwrap();
        let footer = lines
            .iter()
            .position(|l| *l == "Refresh | refresh=true")
            .unwrap();
        assert!(last_repo_row < pm && pm < footer, "{doc}");
        // Its own separator above, the footer's separator below.
        assert_eq!(lines[pm - 1], "---");
        assert_eq!(lines[footer - 1], "---");
        // The parent item is not disabled — a greyed submenu parent reads
        // as broken.
        assert!(!lines[pm].contains("disabled=true"), "{doc}");
        // And the body is present, at submenu depth.
        assert!(lines[pm + 1].starts_with("-- "), "{doc}");
    }

    #[test]
    fn pm_section_shows_recap_text_from_the_store_map() {
        let mut rows = vec![row("alpha", "one")];
        rows[0].id = crate::data::store::WorkspaceId(7);
        let mut recaps = std::collections::HashMap::new();
        recaps.insert(
            crate::data::store::WorkspaceId(7),
            crate::data::store::WorkspaceRecap {
                goal: Some("ship the thing".into()),
                state: None,
                next: None,
                updated_at: 0,
                ..Default::default()
            },
        );
        let doc = render(&["alpha".into()], &rows, &recaps, "/bin/wsx", 0);
        assert!(doc.contains("goal:  ship the thing"), "{doc}");
    }

    #[test]
    fn empty_repo_list_still_has_no_pm_section() {
        let doc = document(&[], &[], &std::collections::HashMap::new(), "/bin/wsx", 0);
        assert_eq!(doc, error_header());
        assert!(!doc.contains("Project Manager"), "{doc}");
    }

    #[test]
    fn render_groups_by_repo_and_lists_empty_repos() {
        let rows = vec![row("alpha", "one"), row("alpha", "two"), row("beta", "b1")];
        let doc = render(
            &["alpha".into(), "beta".into(), "empty".into()],
            &rows,
            &std::collections::HashMap::new(),
            "/bin/wsx",
            0,
        );
        let lines: Vec<&str> = doc.lines().collect();
        assert_eq!(lines[0], "3 | sfimage=arrow.triangle.branch");
        assert_eq!(lines[1], "---");
        let alpha = lines
            .iter()
            .position(|l| *l == "alpha | disabled=true")
            .unwrap();
        let beta = lines
            .iter()
            .position(|l| *l == "beta | disabled=true")
            .unwrap();
        let empty = lines
            .iter()
            .position(|l| *l == "empty | disabled=true")
            .unwrap();
        assert!(alpha < beta && beta < empty);
        assert_eq!(lines[empty + 1], "(no workspaces) | disabled=true");
        // Footer.
        assert_eq!(*lines.last().unwrap(), "Refresh | refresh=true");
    }

    #[test]
    fn empty_repo_list_is_icon_only_document() {
        // No registered repos or any error → icon-only header alone: no
        // `---`, no footer, nothing for SwiftBar to render as a dropdown.
        assert_eq!(
            document(&[], &[], &std::collections::HashMap::new(), "/bin/wsx", 0),
            error_header()
        );
    }

    #[test]
    fn huge_status_message_caps_rendered_subtitle() {
        let huge = "x".repeat(1000);
        let mut r = row("r", "w");
        r.status = Some(status(ReportedState::Working, Some(&huge)));
        let subtitle = &submenu_lines(&r, "/bin/wsx")[0];
        // The whole subtitle text segment (branch + state + message) is
        // capped as one unit — nowhere near the 1000-char input.
        assert!(subtitle.chars().count() < 200, "{subtitle}");
    }

    #[test]
    fn long_worktree_path_survives_uncapped_in_reveal_param() {
        // The MAX_TEXT_LEN cap is for display text only; a long real path
        // must reach `open -R` intact or Reveal in Finder breaks.
        let mut r = row("r", "w");
        let long_segment = "d".repeat(300);
        r.worktree_path = format!("/wt/r/{long_segment}").into();
        let lines = submenu_lines(&r, "/bin/wsx");
        let reveal = lines
            .iter()
            .find(|l| l.starts_with("-- Reveal in Finder"))
            .expect("reveal line present");
        assert!(reveal.contains(&long_segment), "{reveal}");
    }

    #[test]
    fn long_pr_url_survives_uncapped_in_href() {
        let mut r = row("r", "w");
        let long_segment = "u".repeat(300);
        r.cache.pr_number = Some(9);
        r.cache.pr_url = Some(format!("https://example.com/{long_segment}"));
        let lines = submenu_lines(&r, "/bin/wsx");
        let open_pr = lines
            .iter()
            .find(|l| l.starts_with("-- Open PR"))
            .expect("open PR line present");
        assert!(open_pr.contains(&long_segment), "{open_pr}");
    }

    #[test]
    fn leading_dash_status_stays_inside_its_subtitle_line() {
        // A status message starting with "-- " must stay embedded inside
        // its single subtitle line, not spawn an extra menu row.
        let mut r = row("r", "w");
        r.status = Some(status(ReportedState::Working, Some("-- fake")));
        let lines = submenu_lines(&r, "/bin/wsx");
        assert!(lines[0].starts_with("-- "), "{:?}", lines[0]);
        assert!(lines[0].contains("-- fake"), "{:?}", lines[0]);
        assert_eq!(lines[0].matches("\n").count(), 0);
    }
}
