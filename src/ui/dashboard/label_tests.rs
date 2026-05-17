use super::*;
use crate::forge::BranchLifecycle;

fn line_text(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn line_fg(l: &Line) -> Option<ratatui::style::Color> {
    l.spans.iter().find_map(|s| s.style.fg)
}

#[test]
fn nerd_no_lifecycle_uses_branch_glyph() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", true, None, &t);
    assert_eq!(line_text(&l), "\u{e0a0} feat/x");
    assert_eq!(line_fg(&l), None);
}

#[test]
fn nerd_open_pr_uses_pr_glyph_and_ok_color() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrOpen), &t);
    assert_eq!(line_text(&l), "\u{f407} feat/x");
    assert_eq!(line_fg(&l), Some(t.ok));
}

#[test]
fn nerd_draft_pr_annotates_and_stays_uncolored() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrDraft), &t);
    assert_eq!(line_text(&l), "\u{f407} feat/x draft");
    assert_eq!(line_fg(&l), None);
}

#[test]
fn nerd_conflicted_pr_uses_pr_glyph_and_warn_color() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrConflicted), &t);
    assert_eq!(line_text(&l), "\u{f407} feat/x");
    assert_eq!(line_fg(&l), Some(t.warn));
}

#[test]
fn nerd_merged_pr_uses_merge_glyph_and_merged_color() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrMerged), &t);
    assert_eq!(line_text(&l), "\u{f419} feat/x");
    assert_eq!(line_fg(&l), Some(t.merged));
}

#[test]
fn nerd_closed_pr_uses_x_glyph_and_err_color() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", true, Some(BranchLifecycle::PrClosed), &t);
    assert_eq!(line_text(&l), "\u{f659} feat/x");
    assert_eq!(line_fg(&l), Some(t.err));
}

#[test]
fn nerd_no_pr_uses_branch_glyph_uncolored() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", true, Some(BranchLifecycle::NoPr), &t);
    assert_eq!(line_text(&l), "\u{e0a0} feat/x");
    assert_eq!(line_fg(&l), None);
}

#[test]
fn ascii_open_pr_appends_pr_suffix() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrOpen), &t);
    assert_eq!(line_text(&l), "feat/x (pr)");
    assert_eq!(line_fg(&l), Some(t.ok));
}

#[test]
fn ascii_draft_pr_appends_draft_suffix_uncolored() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrDraft), &t);
    assert_eq!(line_text(&l), "feat/x (draft)");
    assert_eq!(line_fg(&l), None);
}

#[test]
fn ascii_conflicted_pr_appends_conflict_suffix() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrConflicted), &t);
    assert_eq!(line_text(&l), "feat/x (conflict)");
    assert_eq!(line_fg(&l), Some(t.warn));
}

#[test]
fn ascii_merged_pr_appends_merged_suffix() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrMerged), &t);
    assert_eq!(line_text(&l), "feat/x (merged)");
    assert_eq!(line_fg(&l), Some(t.merged));
}

#[test]
fn ascii_closed_pr_appends_closed_suffix() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", false, Some(BranchLifecycle::PrClosed), &t);
    assert_eq!(line_text(&l), "feat/x (closed)");
    assert_eq!(line_fg(&l), Some(t.err));
}

#[test]
fn ascii_no_pr_is_plain_uncolored() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", false, Some(BranchLifecycle::NoPr), &t);
    assert_eq!(line_text(&l), "feat/x");
    assert_eq!(line_fg(&l), None);
}

#[test]
fn ascii_none_is_plain_uncolored() {
    let t = Theme::default_theme();
    let l = format_branch_label("feat/x", false, None, &t);
    assert_eq!(line_text(&l), "feat/x");
    assert_eq!(line_fg(&l), None);
}
