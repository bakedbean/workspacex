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

/// Render one entry into lines. Line 1: `HH:MM file`. Line 2: dim summary.
/// When `expanded`, appends up to a few diff-peek lines from `detail`.
pub fn entry_lines(
    ev: &ChangeEvent,
    worktree: &Path,
    expanded: bool,
    width: u16,
    highlight: EntryHighlight,
) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let rel = relative_display(&ev.file_path, worktree);
    out.push(Line::from(vec![
        Span::styled(
            hhmm(ev.timestamp_ms),
            Style::default().add_modifier(Modifier::DIM),
        ),
        Span::raw(" "),
        Span::raw(rel),
    ]));
    out.push(Line::from(Span::styled(
        ev.summary.clone(),
        Style::default().add_modifier(Modifier::DIM | Modifier::ITALIC),
    )));
    if expanded {
        let peek: Vec<String> = match &ev.detail {
            ChangeDetail::Edit { old, new } => {
                let mut v = Vec::new();
                for l in old.lines().take(2) {
                    v.push(format!("- {l}"));
                }
                for l in new.lines().take(2) {
                    v.push(format!("+ {l}"));
                }
                v
            }
            ChangeDetail::Write { head } => {
                head.lines().take(3).map(|l| format!("+ {l}")).collect()
            }
            ChangeDetail::None => Vec::new(),
        };
        for l in peek {
            let clipped: String = l.chars().take(width as usize).collect();
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
            // peek lines are everything after the header (0) and summary (1)
            for line in out.iter_mut().skip(2) {
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
    fn entry_produces_header_and_summary_lines() {
        let lines = entry_lines(
            &ev("/wt/src/main.rs", "fn foo()"),
            Path::new("/wt"),
            false,
            40,
            EntryHighlight::None,
        );
        assert_eq!(lines.len(), 2, "B fidelity: header + summary, no diff peek");
    }

    #[test]
    fn expanded_entry_adds_diff_peek_lines() {
        let lines = entry_lines(
            &ev("/wt/src/main.rs", "fn foo()"),
            Path::new("/wt"),
            true,
            40,
            EntryHighlight::None,
        );
        assert!(lines.len() > 2, "expanded entry includes diff peek");
    }

    #[test]
    fn header_highlight_reverses_first_line() {
        let lines = entry_lines(
            &ev("/wt/a.rs", "fn foo()"),
            Path::new("/wt"),
            true,
            40,
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
            EntryHighlight::Detail,
        );
        assert!(
            !lines[0]
                .spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::REVERSED)),
            "header must NOT be highlighted in Detail mode"
        );
        let peek_rev = lines.iter().skip(2).any(|l| {
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
}
