//! Pure rendering helpers for the change-chronology bar. The host
//! (`src/ui/attached.rs`) carves the side column and calls these to build the
//! content lines; keeping the formatting pure makes it unit-testable.

use crate::activity::chronology::ChangeEvent;
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

pub fn hhmm(timestamp_ms: i64) -> String {
    // Wall-clock HH:MM (UTC) derived from epoch ms without pulling in chrono —
    // a relative glance, not a precise local timestamp. Matches the
    // chrono-free style of activity/events.rs.
    let secs = timestamp_ms.div_euclid(1000);
    let h = secs.div_euclid(3600).rem_euclid(24);
    let m = secs.div_euclid(60).rem_euclid(60);
    format!("{h:02}:{m:02}")
}

/// One bar row: `HH:MM <abbreviated path>`, reversed when `selected`.
pub fn entry_lines(
    ev: &ChangeEvent,
    worktree: &Path,
    width: u16,
    selected: bool,
) -> Vec<Line<'static>> {
    let rel = relative_display(&ev.file_path, worktree);
    let path_budget = (width as usize).saturating_sub(6);
    let path = abbreviate_path(&rel, path_budget);
    let style = if selected {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    };
    let time_style = if selected {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::DIM)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };
    vec![Line::from(vec![
        Span::styled(hhmm(ev.timestamp_ms), time_style),
        Span::styled(" ", style),
        Span::styled(path, style),
    ])]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::chronology::{ChangeDetail, ChangeSource, ChangeTool};
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
            source: ChangeSource::default(),
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
    fn entry_is_a_single_header_line() {
        let lines = entry_lines(
            &ev("/wt/src/main.rs", "fn foo()"),
            Path::new("/wt"),
            40,
            false,
        );
        assert_eq!(lines.len(), 1, "one row: the time+path header");
    }

    #[test]
    fn selected_entry_reverses_its_spans() {
        let lines = entry_lines(
            &ev("/wt/src/main.rs", "fn foo()"),
            Path::new("/wt"),
            40,
            true,
        );
        assert!(
            lines[0]
                .spans
                .iter()
                .all(|s| s.style.add_modifier.contains(Modifier::REVERSED)),
            "selected row should be fully reversed"
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
}
