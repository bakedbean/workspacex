//! The SwiftBar `Project Manager` section: each workspace's agent-authored
//! recap (goal / state / next), ordered blocked → waiting → stalest-first.
//! See docs/superpowers/specs/2026-07-31-menubar-pm-submenu-design.md.

use std::collections::HashMap;

use crate::data::store::{ReportedStatus, WorkspaceId, WorkspaceRecap};
use crate::menubar::escape::{esc_text, esc_text_uncapped, quote_param};
use crate::menubar::plugin::{ROW_FONT, pr_field};
use crate::util::time::format_age;
use crate::workspace_rows::{RowInput, state_glyph};

/// One workspace's PM entry: its menu row plus the recap narrative, if any.
pub(crate) struct PmCard<'a> {
    pub row: &'a RowInput,
    /// `None` when there is no recap row, or when the row exists but all
    /// three fields are absent/empty — the two cases render identically and
    /// must be indistinguishable to every consumer.
    pub recap: Option<&'a WorkspaceRecap>,
}

/// Needs-attention rank: blocked (0) before waiting (1) before the rest (2).
///
/// Mirrors the TUI digest's `ui::pm_pane::attention_rank`. Deliberately NOT
/// `workspace_rows::attention_rank`, which ranks descending and puts `Done`
/// above `Waiting` — that one exists to pick the menubar header's worst
/// state, a different question.
fn pm_attention_rank(status: Option<&ReportedStatus>) -> u8 {
    use crate::data::store::ReportedState;
    match status.map(|s| s.state) {
        Some(ReportedState::Blocked) => 0,
        Some(ReportedState::Waiting) => 1,
        _ => 2,
    }
}

/// A recap counts only if at least one field carries text. A row whose
/// fields are all NULL or empty is reachable through the CLI's partial
/// upsert and must read as "no recap yet".
fn effective_recap(recap: Option<&WorkspaceRecap>) -> Option<&WorkspaceRecap> {
    let r = recap?;
    let has_text = [&r.goal, &r.state, &r.next]
        .into_iter()
        .any(|f| f.as_deref().is_some_and(|s| !s.trim().is_empty()));
    has_text.then_some(r)
}

/// Epoch ms of the last thing the agent said — a status push or a recap
/// update, whichever is newer; `0` when it has said nothing.
///
/// The DB-side stand-in for the TUI digest's session-log activity: the
/// plugin cannot tail JSONL, and `0` for "never" reproduces the TUI's own
/// `unwrap_or(0)`, floating never-seen workspaces to the top of their rank.
fn signal_ms(row: &RowInput, recap: Option<&WorkspaceRecap>) -> i64 {
    let from_status = row.status.as_ref().map(|s| s.reported_at).unwrap_or(0);
    let from_recap = recap.map(|r| r.updated_at).unwrap_or(0);
    from_status.max(from_recap)
}

/// This repo's cards, ordered blocked → waiting → rest, stalest first.
pub(crate) fn cards_for_repo<'a>(
    rows: &'a [RowInput],
    recaps: &'a HashMap<WorkspaceId, WorkspaceRecap>,
    repo: &str,
) -> Vec<PmCard<'a>> {
    let mut cards: Vec<PmCard<'a>> = rows
        .iter()
        .filter(|r| r.repo_name == repo)
        .map(|row| PmCard {
            row,
            recap: effective_recap(recaps.get(&row.id)),
        })
        .collect();
    cards.sort_by_key(|c| {
        (
            pm_attention_rank(c.row.status.as_ref()),
            signal_ms(c.row, c.recap),
        )
    });
    cards
}

/// Cap on a rendered recap field, in chars, *including* the ellipsis.
/// Tighter than the document-wide `MAX_TEXT_LEN` because NSMenu sizes
/// itself to its widest item — one long goal line widens the whole
/// dropdown. Doctrine already asks agents for one-liners.
pub(crate) const MAX_RECAP_LEN: usize = 72;

/// Indent for a card's recap and fact lines.
///
/// Two NBSPs, because SwiftBar trims each line's title and ASCII spaces
/// would vanish. NBSP may not be enough: Swift's `CharacterSet.whitespaces`
/// includes U+00A0, and only a running SwiftBar can settle it. If the
/// manual test shows the lines flush with the header, change this to
/// `"\u{2502} "` (box-drawing light vertical + space) — not whitespace, so
/// it cannot be trimmed, and it reads as a continuation gutter. Tests
/// assert against this constant, so they hold under either value.
pub(crate) const RECAP_INDENT: &str = "\u{a0}\u{a0}";

/// A recap field escaped for display and capped at `MAX_RECAP_LEN`,
/// ellipsized when it overflows. Distinct from `esc_text`'s plain
/// truncation, which the top-level rows still use.
fn esc_recap(s: &str) -> String {
    let out = esc_text_uncapped(s);
    if out.chars().count() <= MAX_RECAP_LEN {
        return out;
    }
    let mut capped: String = out.chars().take(MAX_RECAP_LEN - 1).collect();
    capped.push('\u{2026}');
    capped
}

/// A card's lines: clickable header, one line per present recap field (or
/// the placeholder), and the facts line when any segment applies.
fn card_lines(card: &PmCard, wsx_bin: &str, now_ms: i64) -> Vec<String> {
    let r = card.row;
    let mut header = format!(
        "{} {}",
        state_glyph(r.status.as_ref().map(|s| s.state)),
        esc_text(&r.slug)
    );
    if let Some(s) = &r.status {
        header.push_str(&format!(
            "  {} {}",
            s.state.as_str(),
            format_age(now_ms - s.reported_at)
        ));
    }
    let mut out = vec![format!(
        "-- {header} | bash={} param1=\"menubar\" param2=\"jump\" param3={} param4={} terminal=false {ROW_FONT}",
        quote_param(wsx_bin),
        quote_param(&r.repo_name),
        quote_param(&r.slug),
    )];

    let detail = |text: String| format!("--{RECAP_INDENT}{text} | disabled=true {ROW_FONT}");
    match card.recap {
        Some(recap) => {
            for (label, field) in [
                ("goal:  ", &recap.goal),
                ("state: ", &recap.state),
                ("next:  ", &recap.next),
            ] {
                if let Some(v) = field.as_deref().filter(|v| !v.trim().is_empty()) {
                    out.push(detail(format!("{label}{}", esc_recap(v))));
                }
            }
        }
        None => out.push(detail("no recap yet".into())),
    }

    let mut segs: Vec<String> = Vec::new();
    let pr = pr_field(&r.cache);
    if !pr.is_empty() {
        segs.push(pr);
    }
    if r.cache.dirty == Some(true) {
        segs.push("\u{25cf}".into());
    }
    if let (Some(a), Some(d)) = (r.cache.additions, r.cache.deletions)
        && (a > 0 || d > 0)
    {
        segs.push(format!("+{a} -{d}"));
    }
    if let Some(recap) = card.recap {
        segs.push(format!("recap {}", format_age(now_ms - recap.updated_at)));
    }
    if !segs.is_empty() {
        out.push(detail(segs.join(" \u{b7} ")));
    }
    out
}

/// The whole `Project Manager` submenu body, at `--` depth: repos in the
/// document's order, each with its cards separated by `-----`, and no
/// leading or trailing separator.
pub(crate) fn pm_section_lines(
    repo_names: &[String],
    rows: &[RowInput],
    recaps: &HashMap<WorkspaceId, WorkspaceRecap>,
    wsx_bin: &str,
    now_ms: i64,
) -> Vec<String> {
    let mut lines = Vec::new();
    for (i, repo) in repo_names.iter().enumerate() {
        if i > 0 {
            lines.push("-----".to_string());
        }
        lines.push(format!("-- {} | disabled=true", esc_text(repo)));
        let cards = cards_for_repo(rows, recaps, repo);
        if cards.is_empty() {
            lines.push("-- (no workspaces) | disabled=true".to_string());
            continue;
        }
        for (j, card) in cards.iter().enumerate() {
            if j > 0 {
                lines.push("-----".to_string());
            }
            lines.extend(card_lines(card, wsx_bin, now_ms));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::scm_cache::ScmCacheRow;
    use crate::data::store::ReportedState;

    fn row(repo: &str, slug: &str, id: i64) -> RowInput {
        RowInput {
            id: WorkspaceId(id),
            repo_name: repo.into(),
            slug: slug.into(),
            branch: format!("x/{slug}"),
            worktree_path: format!("/wt/{repo}/{slug}").into(),
            status: None,
            cache: ScmCacheRow::default(),
        }
    }

    fn status(state: ReportedState, at: i64) -> ReportedStatus {
        ReportedStatus {
            state,
            message: Some("msg".into()),
            source: "test".into(),
            reported_at: at,
        }
    }

    fn recap(goal: Option<&str>, at: i64) -> WorkspaceRecap {
        WorkspaceRecap {
            goal: goal.map(str::to_string),
            state: Some("s".into()),
            next: Some("n".into()),
            updated_at: at,
            ..Default::default()
        }
    }

    #[test]
    fn orders_blocked_then_waiting_then_stalest_first() {
        let rows = vec![
            row("alpha", "fresh-working", 1),
            row("alpha", "stale-working", 2),
            row("alpha", "waiting", 3),
            row("alpha", "blocked", 4),
        ];
        let mut rows = rows;
        rows[0].status = Some(status(ReportedState::Working, 9_000));
        rows[1].status = Some(status(ReportedState::Working, 1_000));
        rows[2].status = Some(status(ReportedState::Waiting, 5_000));
        rows[3].status = Some(status(ReportedState::Blocked, 5_000));
        let no_recaps = HashMap::new();
        let cards = cards_for_repo(&rows, &no_recaps, "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(
            names,
            ["blocked", "waiting", "stale-working", "fresh-working"]
        );
    }

    #[test]
    fn recap_updated_at_counts_toward_the_stalest_tiebreak() {
        // Both working; the one whose most recent agent signal is older
        // sorts first, even when that signal is the recap, not the status.
        let mut rows = vec![
            row("alpha", "recent-recap", 1),
            row("alpha", "old-recap", 2),
        ];
        rows[0].status = Some(status(ReportedState::Working, 1_000));
        rows[1].status = Some(status(ReportedState::Working, 1_000));
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("g"), 9_000));
        recaps.insert(WorkspaceId(2), recap(Some("g"), 2_000));
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["old-recap", "recent-recap"]);
    }

    #[test]
    fn never_seen_workspace_sorts_to_the_top_of_its_rank() {
        let mut rows = vec![row("alpha", "seen", 1), row("alpha", "never", 2)];
        rows[0].status = Some(status(ReportedState::Working, 1_000));
        // "never" has no status and no recap at all → signal 0.
        let no_recaps = HashMap::new();
        let cards = cards_for_repo(&rows, &no_recaps, "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["never", "seen"]);
    }

    #[test]
    fn done_and_working_share_the_lowest_rank() {
        // Parity with the TUI digest: only blocked and waiting are ranked
        // ahead. (workspace_rows::attention_rank, used for the header
        // color, ranks Done above Waiting — deliberately not reused here.)
        let mut rows = vec![row("alpha", "done", 1), row("alpha", "waiting", 2)];
        rows[0].status = Some(status(ReportedState::Done, 5_000));
        rows[1].status = Some(status(ReportedState::Waiting, 9_000));
        let no_recaps = HashMap::new();
        let cards = cards_for_repo(&rows, &no_recaps, "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["waiting", "done"]);
    }

    #[test]
    fn filters_to_the_named_repo() {
        let rows = vec![row("alpha", "a1", 1), row("beta", "b1", 2)];
        let no_recaps = HashMap::new();
        let cards = cards_for_repo(&rows, &no_recaps, "beta");
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].row.slug, "b1");
    }

    #[test]
    fn all_empty_recap_row_is_treated_as_absent() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: None,
                state: Some(String::new()), // present but empty
                next: None,
                updated_at: 9_000,
                ..Default::default()
            },
        );
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        assert!(cards[0].recap.is_none(), "empty fields → no recap");
    }

    #[test]
    fn partially_filled_recap_is_kept() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("only a goal".into()),
                state: None,
                next: None,
                updated_at: 9_000,
                ..Default::default()
            },
        );
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        assert_eq!(cards[0].recap.unwrap().goal.as_deref(), Some("only a goal"));
    }

    #[test]
    fn absent_recap_contributes_no_signal() {
        // The all-empty recap's updated_at (9_000) must not make this
        // workspace look freshly touched.
        let mut rows = vec![row("alpha", "empty-recap", 1), row("alpha", "real", 2)];
        rows[1].status = Some(status(ReportedState::Working, 5_000));
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: None,
                state: None,
                next: None,
                updated_at: 9_000,
                ..Default::default()
            },
        );
        let cards = cards_for_repo(&rows, &recaps, "alpha");
        let names: Vec<&str> = cards.iter().map(|c| c.row.slug.as_str()).collect();
        assert_eq!(names, ["empty-recap", "real"]);
    }

    fn section(rows: &[RowInput], recaps: &HashMap<WorkspaceId, WorkspaceRecap>) -> Vec<String> {
        pm_section_lines(
            &["alpha".into()],
            rows,
            recaps,
            "/bin/wsx",
            // 1h after the fixture timestamps below, so ages are stable.
            3_600_000,
        )
    }

    #[test]
    fn populated_card_renders_header_recap_and_facts() {
        use crate::git::forge::BranchLifecycle;
        let mut rows = vec![row("alpha", "api-fix", 1)];
        rows[0].status = Some(status(ReportedState::Blocked, 3_000_000));
        rows[0].cache = ScmCacheRow {
            pr_lifecycle: Some(BranchLifecycle::PrDraft),
            pr_number: Some(12),
            dirty: Some(true),
            additions: Some(45),
            deletions: Some(12),
            ..Default::default()
        };
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("add widgets endpoint"), 0));
        let lines = section(&rows, &recaps);
        let joined = lines.join("\n");

        // Header: glyph, slug, status word + age; the only clickable line.
        assert!(
            lines.iter().any(|l| l.starts_with("-- ! api-fix")),
            "{joined}"
        );
        assert!(joined.contains("blocked 10m"), "{joined}");
        // The status *message* is not repeated here — it already appears in
        // the top-level row's own action submenu, and the recap lines are
        // the narrative this section exists for. ("msg" is the fixture's
        // status message.)
        assert!(!joined.contains("msg"), "{joined}");
        // Recap lines, each indented and disabled.
        assert!(
            lines
                .iter()
                .any(|l| l.contains("goal:  add widgets endpoint")),
            "{joined}"
        );
        assert!(lines.iter().any(|l| l.contains("state: s")), "{joined}");
        assert!(lines.iter().any(|l| l.contains("next:  n")), "{joined}");
        // Facts: PR, dirty dot, diffstat, recap age — joined with " · ".
        let facts = lines
            .iter()
            .find(|l| l.contains("#12 draft"))
            .expect(&joined);
        assert!(facts.contains('\u{25cf}'), "{facts}");
        assert!(facts.contains("+45 -12"), "{facts}");
        assert!(facts.contains("recap 1h"), "{facts}");
        assert!(
            facts.contains(" \u{b7} "),
            "segments joined with a middot: {facts}"
        );
    }

    #[test]
    fn only_the_header_line_is_clickable() {
        let mut rows = vec![row("alpha", "w", 1)];
        rows[0].status = Some(status(ReportedState::Working, 0));
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("g"), 0));
        let lines = section(&rows, &recaps);
        let clickable: Vec<&String> = lines.iter().filter(|l| l.contains("bash=")).collect();
        assert_eq!(clickable.len(), 1, "{lines:?}");
        assert!(
            clickable[0].contains("param2=\"jump\""),
            "{:?}",
            clickable[0]
        );
        assert!(
            clickable[0].contains("param3=\"alpha\""),
            "{:?}",
            clickable[0]
        );
        assert!(clickable[0].contains("param4=\"w\""), "{:?}", clickable[0]);
        // Everything below the header and the repo header is inert.
        for l in lines.iter().filter(|l| !l.contains("bash=")) {
            assert!(l.contains("disabled=true") || l == "-----", "{l}");
        }
    }

    #[test]
    fn card_without_recap_says_so_and_omits_recap_age() {
        let rows = vec![row("alpha", "w", 1)];
        let lines = section(&rows, &HashMap::new());
        let joined = lines.join("\n");
        assert!(joined.contains("no recap yet"), "{joined}");
        assert!(!joined.contains("recap 1h"), "{joined}");
        assert!(!joined.contains("goal:"), "{joined}");
    }

    #[test]
    fn card_with_no_facts_omits_the_facts_line() {
        // No status, no PR, not dirty, no recap: header + placeholder only.
        let rows = vec![row("alpha", "w", 1)];
        let lines = section(&rows, &HashMap::new());
        assert_eq!(
            lines.len(),
            3,
            "repo header + card header + placeholder: {lines:?}"
        );
    }

    #[test]
    fn partial_recap_renders_only_present_fields() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("only a goal".into()),
                state: None,
                next: None,
                updated_at: 0,
                ..Default::default()
            },
        );
        let joined = section(&rows, &recaps).join("\n");
        assert!(joined.contains("goal:  only a goal"), "{joined}");
        assert!(!joined.contains("state:"), "{joined}");
        assert!(!joined.contains("next:"), "{joined}");
        assert!(!joined.contains("no recap yet"), "{joined}");
    }

    #[test]
    fn recap_lines_are_indented_with_the_indent_constant() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("g"), 0));
        let lines = section(&rows, &recaps);
        let goal = lines.iter().find(|l| l.contains("goal:")).unwrap();
        assert!(goal.starts_with(&format!("--{RECAP_INDENT}")), "{goal}");
        assert!(
            !goal.starts_with("-- "),
            "plain spaces would be trimmed: {goal}"
        );
    }

    #[test]
    fn long_recap_field_is_capped_with_an_ellipsis() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some(&"g".repeat(200)), 0));
        let lines = section(&rows, &recaps);
        let goal = lines.iter().find(|l| l.contains("goal:")).unwrap();
        let text = goal.split(" | ").next().unwrap();
        // "--" + indent + "goal:  " + capped field.
        let field = text.rsplit("goal:  ").next().unwrap();
        assert_eq!(field.chars().count(), MAX_RECAP_LEN, "{field}");
        assert!(field.ends_with('\u{2026}'), "{field}");
    }

    #[test]
    fn short_recap_field_is_not_ellipsized() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(WorkspaceId(1), recap(Some("short"), 0));
        let joined = section(&rows, &recaps).join("\n");
        assert!(joined.contains("goal:  short"), "{joined}");
        assert!(!joined.contains('\u{2026}'), "{joined}");
    }

    #[test]
    fn recap_text_cannot_inject_lines_or_params() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("evil\n-- fake | bash=\"/bin/rm\"".into()),
                state: None,
                next: None,
                updated_at: 0,
                ..Default::default()
            },
        );
        let lines = section(&rows, &recaps);
        for l in &lines {
            assert!(!l.contains('\n'), "{l}");
        }
        // The property is that hostile text cannot be PARSED as params —
        // not that its characters vanish. SwiftBar splits a line on " | ",
        // so what matters is that no raw pipe survives into the text
        // segment; `bash=` and the quotes remain as inert display text.
        // (`esc_core` deliberately never touches `"` — param values are
        // real paths and URLs that must survive intact.)
        let goal = lines.iter().find(|l| l.contains("goal:")).unwrap();
        let (text, params) = goal.split_once(" | ").expect("line has params");
        assert!(!text.contains('|'), "raw pipe survived into text: {text}");
        assert!(text.contains('\u{00a6}'), "pipe not neutralized: {text}");
        assert_eq!(params, format!("disabled=true {ROW_FONT}"));
    }

    #[test]
    fn leading_dash_recap_cannot_become_a_separator() {
        let rows = vec![row("alpha", "w", 1)];
        let mut recaps = HashMap::new();
        recaps.insert(
            WorkspaceId(1),
            WorkspaceRecap {
                goal: Some("--- danger".into()),
                state: None,
                next: None,
                updated_at: 0,
                ..Default::default()
            },
        );
        let joined = section(&rows, &recaps).join("\n");
        assert!(
            joined.contains('\u{2011}'),
            "leading dash not guarded: {joined}"
        );
    }

    #[test]
    fn repos_and_cards_are_separated_without_a_trailing_separator() {
        let rows = vec![
            row("alpha", "a1", 1),
            row("alpha", "a2", 2),
            row("beta", "b1", 3),
        ];
        let lines = pm_section_lines(
            &["alpha".into(), "beta".into()],
            &rows,
            &HashMap::new(),
            "/bin/wsx",
            0,
        );
        assert_eq!(lines[0], "-- alpha | disabled=true");
        assert!(
            lines.iter().any(|l| l == "-- beta | disabled=true"),
            "{lines:?}"
        );
        assert!(lines.iter().any(|l| l == "-----"), "{lines:?}");
        assert_ne!(lines.last().unwrap(), "-----", "no trailing separator");
        assert_ne!(lines[0], "-----", "no leading separator");
    }

    #[test]
    fn empty_repo_renders_the_placeholder() {
        let lines = pm_section_lines(&["alpha".into()], &[], &HashMap::new(), "/bin/wsx", 0);
        assert_eq!(
            lines,
            vec![
                "-- alpha | disabled=true".to_string(),
                "-- (no workspaces) | disabled=true".to_string()
            ]
        );
    }
}
