//! Pure rendering helpers for the change-chronology bar. The host
//! (`src/ui/attached.rs`) carves the side column and calls these to build the
//! content lines; keeping the formatting pure makes it unit-testable.

use crate::activity::chronology::{ChangeDetail, ChangeEvent};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use std::path::Path;

/// Minimum columns the agent pane must keep for the bar to be allowed.
pub const MIN_AGENT_COLS: u16 = 40;

/// Worktree-relative display path, falling back to the full path when the file
/// is not under the worktree.
pub fn relative_display(file: &Path, worktree: &Path) -> String {
    match file.strip_prefix(worktree) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => file.to_string_lossy().to_string(),
    }
}

/// Hide the bar when carving `bar_cols` would leave the agent < MIN_AGENT_COLS.
pub fn should_auto_hide(area_cols: u16, bar_cols: u16) -> bool {
    area_cols.saturating_sub(bar_cols) < MIN_AGENT_COLS
}

/// Front-truncate `s` to `max` columns with a leading `…` so the tail (the
/// filename) stays visible. Counts characters, not bytes.
fn ellipsize_start(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let tail: String = s.chars().skip(n - (max - 1)).collect();
    format!("…{tail}")
}

/// Fit a worktree-relative path into `max` columns. If it already fits, return
/// it unchanged. Otherwise abbreviate each ancestor directory (everything
/// before the parent directory) to its first character, keeping the parent
/// directory and filename intact (e.g. `docs/superpowers/specs/foo.md` →
/// `d/s/specs/foo.md`). If still too wide, front-truncate with `…`.
fn abbreviate_path(rel: &str, max: usize) -> String {
    if rel.chars().count() <= max {
        return rel.to_string();
    }
    let parts: Vec<&str> = rel.split('/').collect();
    if parts.len() > 2 {
        let last = parts.len() - 1;
        let mut out = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i > 0 {
                out.push('/');
            }
            // Ancestors (everything before the parent dir) collapse to their
            // first character; the parent dir and filename are kept whole.
            if i + 2 <= last {
                if let Some(c) = p.chars().next() {
                    out.push(c);
                }
            } else {
                out.push_str(p);
            }
        }
        if out.chars().count() <= max {
            return out;
        }
        return ellipsize_start(&out, max);
    }
    ellipsize_start(rel, max)
}

fn hhmm(timestamp_ms: i64) -> String {
    // Wall-clock HH:MM (UTC) derived from epoch ms without pulling in chrono —
    // a relative glance, not a precise local timestamp. Matches the
    // chrono-free style of activity/events.rs.
    let secs = timestamp_ms.div_euclid(1000);
    let h = secs.div_euclid(3600).rem_euclid(24);
    let m = secs.div_euclid(60).rem_euclid(60);
    format!("{h:02}:{m:02}")
}

/// Which part of an entry is keyboard-selected (for highlight). `None` when the
/// entry isn't the cursor target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryHighlight {
    None,
    Header,
    Detail,
}

/// Render one entry into lines. Line 1: `HH:MM <path>`, where the path is
/// abbreviated (ancestor dirs collapsed to their first letter) to fit the
/// width. When `expanded`, appends up to a few diff-peek lines from `detail`,
/// each prefixed with a 4-wide line-number gutter: added (`+`) lines are
/// numbered from `base_line`, removed (`-`) lines have a 5-space blank gutter
/// so columns align.
pub fn entry_lines(
    ev: &ChangeEvent,
    worktree: &Path,
    expanded: bool,
    width: u16,
    base_line: u32,
    highlight: EntryHighlight,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let rel = relative_display(&ev.file_path, worktree);
    // The header is `HH:MM ` (6 cols) followed by the path; budget the path to
    // the remaining width so long paths abbreviate instead of overflowing.
    let path_budget = (width as usize).saturating_sub(6);
    let path = abbreviate_path(&rel, path_budget);
    out.push(Line::from(vec![
        Span::styled(
            hhmm(ev.timestamp_ms),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(" "),
        Span::raw(path),
    ]));
    if expanded {
        // (line number, marker, text). `+` (added) lines carry a current-file
        // line number starting at base_line; `-` (removed) lines have none.
        let mut peek: Vec<(Option<u32>, char, String)> = Vec::new();
        match &ev.detail {
            ChangeDetail::Edit { old, new } => {
                for l in old.lines().take(2) {
                    peek.push((None, '-', l.to_string()));
                }
                for (k, l) in new.lines().take(2).enumerate() {
                    peek.push((Some(base_line.saturating_add(k as u32)), '+', l.to_string()));
                }
            }
            ChangeDetail::Write { head } => {
                for (k, l) in head.lines().take(3).enumerate() {
                    peek.push((Some(base_line.saturating_add(k as u32)), '+', l.to_string()));
                }
            }
            ChangeDetail::None => {}
        }
        for (gutter, marker, text) in peek {
            // 4-col right-aligned number + a space, then marker + text. Removed
            // lines use a 5-space blank gutter so the columns line up.
            let line = match gutter {
                Some(n) => format!("{n:>4} {marker} {text}"),
                None => format!("     {marker} {text}"),
            };
            let clipped: String = line.chars().take(width as usize).collect();
            out.push(Line::from(Span::styled(
                clipped,
                Style::default().add_modifier(Modifier::DIM),
            )));
        }
    }
    match highlight {
        EntryHighlight::None => {}
        EntryHighlight::Header => {
            if let Some(first) = out.first_mut() {
                for s in &mut first.spans {
                    s.style = s.style.add_modifier(Modifier::REVERSED);
                }
            }
        }
        EntryHighlight::Detail => {
            // peek lines are everything after the header (index 0)
            for line in out.iter_mut().skip(1) {
                for s in &mut line.spans {
                    s.style = s.style.add_modifier(Modifier::REVERSED);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::chronology::ChangeTool;
    use std::path::PathBuf;

    fn ev(file: &str, summary: &str) -> ChangeEvent {
        ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from(file),
            summary: summary.to_string(),
            detail: ChangeDetail::Edit {
                old: "a".into(),
                new: "b".into(),
            },
        }
    }

    #[test]
    fn relative_path_strips_worktree_prefix() {
        let p = relative_display(Path::new("/wt/src/main.rs"), Path::new("/wt"));
        assert_eq!(p, "src/main.rs");
    }

    #[test]
    fn relative_path_passthrough_when_not_prefixed() {
        let p = relative_display(Path::new("/other/x.rs"), Path::new("/wt"));
        assert_eq!(p, "/other/x.rs");
    }

    #[test]
    fn auto_hide_when_area_too_narrow() {
        assert!(should_auto_hide(35, 30));
        assert!(!should_auto_hide(120, 30));
    }

    #[test]
    fn collapsed_entry_is_a_single_header_line() {
        let lines = entry_lines(
            &ev("/wt/src/main.rs", "fn foo()"),
            Path::new("/wt"),
            false,
            40,
            1,
            EntryHighlight::None,
        );
        assert_eq!(
            lines.len(),
            1,
            "collapsed: just the time+path header (no summary)"
        );
    }

    #[test]
    fn expanded_entry_adds_diff_peek_lines() {
        let lines = entry_lines(
            &ev("/wt/src/main.rs", "fn foo()"),
            Path::new("/wt"),
            true,
            40,
            1,
            EntryHighlight::None,
        );
        assert!(
            lines.len() > 1,
            "expanded entry is the header plus diff-peek lines"
        );
    }

    #[test]
    fn abbreviate_keeps_short_paths_whole() {
        assert_eq!(abbreviate_path("src/main.rs", 40), "src/main.rs");
    }

    #[test]
    fn abbreviate_collapses_ancestors_keeping_parent_and_file() {
        // 32 cols doesn't fit in 30 → ancestors (src, ui) collapse to first
        // char; the parent dir (widgets) and filename are kept whole.
        let out = abbreviate_path("src/ui/widgets/chronology_bar.rs", 30);
        assert_eq!(out, "s/u/widgets/chronology_bar.rs");
    }

    #[test]
    fn abbreviate_front_truncates_when_still_too_long() {
        let out = abbreviate_path("docs/superpowers/specs/2026-06-05-foo.md", 15);
        assert!(out.chars().count() <= 15, "fits within max");
        assert!(out.starts_with('…'), "front-truncated");
        assert!(out.ends_with("foo.md"), "filename tail preserved");
    }

    #[test]
    fn abbreviate_parent_and_file_only_front_truncates() {
        // No ancestors to collapse → falls back to front-truncation.
        let out = abbreviate_path("widgets/chronology_bar.rs", 12);
        assert!(out.chars().count() <= 12);
        assert!(out.ends_with(".rs"));
    }

    #[test]
    fn header_highlight_reverses_first_line() {
        let lines = entry_lines(
            &ev("/wt/a.rs", "fn foo()"),
            Path::new("/wt"),
            true,
            40,
            1,
            EntryHighlight::Header,
        );
        let has_rev = lines[0]
            .spans
            .iter()
            .any(|s| s.style.add_modifier.contains(Modifier::REVERSED));
        assert!(has_rev, "header line should be highlighted");
    }

    #[test]
    fn detail_highlight_reverses_peek_lines_only() {
        let lines = entry_lines(
            &ev("/wt/a.rs", "fn foo()"),
            Path::new("/wt"),
            true,
            40,
            1,
            EntryHighlight::Detail,
        );
        assert!(
            !lines[0]
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::REVERSED)),
            "header must NOT be highlighted in Detail mode"
        );
        let peek_rev = lines.iter().skip(1).any(|l| {
            l.spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::REVERSED))
        });
        assert!(peek_rev, "detail peek should be highlighted");
    }

    #[test]
    fn no_highlight_leaves_lines_unreversed() {
        let lines = entry_lines(
            &ev("/wt/a.rs", "fn foo()"),
            Path::new("/wt"),
            false,
            40,
            1,
            EntryHighlight::None,
        );
        assert!(
            !lines.iter().any(|l| l
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::REVERSED))),
            "no highlight expected"
        );
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn peek_numbers_added_lines_and_blanks_removed_gutter() {
        let ev = ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Edit,
            file_path: PathBuf::from("/wt/a.rs"),
            summary: String::new(),
            detail: ChangeDetail::Edit {
                old: "old0\nold1".into(),
                new: "new0\nnew1".into(),
            },
        };
        let lines = entry_lines(&ev, Path::new("/wt"), true, 60, 42, EntryHighlight::None);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        // out[0] header; out[1..3] removed (blank gutter); out[3..5] added (42, 43)
        assert!(
            texts[1].starts_with("     -"),
            "removed gutter blank: {:?}",
            texts[1]
        );
        assert!(texts[2].starts_with("     -"), "{:?}", texts[2]);
        assert!(
            texts[3].contains("42") && texts[3].contains("+ new0"),
            "{:?}",
            texts[3]
        );
        assert!(
            texts[4].contains("43") && texts[4].contains("+ new1"),
            "{:?}",
            texts[4]
        );
    }

    #[test]
    fn write_peek_numbers_from_base_line() {
        let ev = ChangeEvent {
            timestamp_ms: 0,
            tool: ChangeTool::Write,
            file_path: PathBuf::from("/wt/a.rs"),
            summary: String::new(),
            detail: ChangeDetail::Write {
                head: "l1\nl2\nl3".into(),
            },
        };
        let lines = entry_lines(&ev, Path::new("/wt"), true, 60, 1, EntryHighlight::None);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        // Assert the full gutter so a missing number can't pass via "l1" etc.
        assert_eq!(texts[1], "   1 + l1", "{:?}", texts[1]);
        assert_eq!(texts[2], "   2 + l2", "{:?}", texts[2]);
        assert_eq!(texts[3], "   3 + l3", "{:?}", texts[3]);
    }
}
