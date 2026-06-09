//! ratatui rendering for the chronology UI. Maps `sessionx`'s neutral
//! `TokenKind`/`DiffLine` model to styled ratatui `Line`/`Span`, and renders
//! bar rows. Absorbed from chronox's `render.rs` — `sessionx` is
//! framework-agnostic, so this UI mapping lives in the consumer.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use sessionx::event::ChangeEvent;
use sessionx::syntax::{
    CellKind, DiffCell, DiffLine, DiffMarker, LangSpec, Token, TokenKind, change_detail_diff,
};
use std::path::Path;

fn style_for(kind: TokenKind) -> Style {
    match kind {
        TokenKind::Keyword => Style::default().fg(Color::Magenta),
        TokenKind::Str => Style::default().fg(Color::Yellow),
        TokenKind::Comment => Style::default().fg(Color::DarkGray),
        TokenKind::Number => Style::default().fg(Color::Cyan),
        TokenKind::Default => Style::default(),
    }
}

fn token_spans(code: &[Token]) -> Vec<Span<'static>> {
    code.iter()
        .map(|(t, k)| Span::styled(t.clone(), style_for(*k)))
        .collect()
}

fn diff_line_to_ratatui(dl: &DiffLine) -> Line<'static> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let (marker, marker_style) = match dl.marker {
        DiffMarker::Added => ("+ ", Style::default().fg(Color::Green)),
        DiffMarker::Removed => ("- ", Style::default().fg(Color::Red)),
    };
    let mut spans = vec![
        Span::styled(dl.gutter.clone(), dim),
        Span::styled(marker.to_string(), marker_style),
    ];
    spans.extend(token_spans(&dl.code));
    Line::from(spans)
}

/// Map one side-by-side cell to a styled line. `None` yields an empty line (a
/// blank column). Same gutter/marker/colour vocabulary as `diff_line_to_ratatui`.
pub fn side_cell_to_line(cell: Option<&DiffCell>) -> Line<'static> {
    let Some(c) = cell else {
        return Line::default();
    };
    let dim = Style::default().add_modifier(Modifier::DIM);
    let (marker, marker_style) = match c.kind {
        CellKind::Added => ("+ ", Style::default().fg(Color::Green)),
        CellKind::Removed => ("- ", Style::default().fg(Color::Red)),
        CellKind::Context => ("  ", Style::default()),
    };
    let mut spans = vec![
        Span::styled(c.gutter.clone(), dim),
        Span::styled(marker.to_string(), marker_style),
    ];
    spans.extend(token_spans(&c.code));
    Line::from(spans)
}

/// Build the modal's styled diff lines from a change. Same colours/gutter as the
/// in-`wsx` implementation it replaces.
pub fn change_detail_lines_styled(
    detail: &sessionx::event::ChangeDetail,
    base_line: u32,
    lang: Option<&LangSpec>,
) -> Vec<Line<'static>> {
    change_detail_diff(detail, base_line, lang)
        .iter()
        .map(diff_line_to_ratatui)
        .collect()
}

/// Truncate a styled `Line` to `width` display columns (char-based), preserving
/// span styles; the boundary span is trimmed.
pub fn clip_line_to_width(line: &Line<'static>, width: usize) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in &line.spans {
        if used >= width {
            break;
        }
        let remaining = width - used;
        let cnt = span.content.chars().count();
        if cnt <= remaining {
            out.push(span.clone());
            used += cnt;
        } else {
            let truncated: String = span.content.chars().take(remaining).collect();
            out.push(Span::styled(truncated, span.style));
            break;
        }
    }
    Line::from(out)
}

// ── entry_lines and display helpers from chronology_bar.rs ───────────────────

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
    // a relative glance, not a precise local timestamp.
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
    use ratatui::style::{Color, Modifier};
    use sessionx::event::{ChangeDetail, ChangeSource, ChangeTool};
    use std::path::{Path, PathBuf};

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
    fn styled_lines_preserve_colours_and_gutter() {
        let detail = ChangeDetail::Edit {
            old: "old".into(),
            new: "let y = 1".into(),
        };
        let lines = change_detail_lines_styled(
            &detail,
            7,
            sessionx::syntax::lang_for_path(Path::new("a.rs")),
        );
        // removed line: dim 5-space gutter, red "- " marker
        assert_eq!(lines[0].spans[0].content.as_ref(), "     ");
        assert!(lines[0].spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(lines[0].spans[1].content.as_ref(), "- ");
        assert_eq!(lines[0].spans[1].style.fg, Some(Color::Red));
        // added line: gutter "   7 ", green "+ ", "let" highlighted magenta
        assert_eq!(lines[1].spans[0].content.as_ref(), "   7 ");
        assert_eq!(lines[1].spans[1].content.as_ref(), "+ ");
        assert_eq!(lines[1].spans[1].style.fg, Some(Color::Green));
        assert!(
            lines[1]
                .spans
                .iter()
                .any(|s| s.content.as_ref() == "let" && s.style.fg == Some(Color::Magenta))
        );
    }

    #[test]
    fn no_lang_is_plain_code_span() {
        let detail = ChangeDetail::Write {
            head: "let y = 1".into(),
        };
        let lines = change_detail_lines_styled(&detail, 1, None);
        assert_eq!(lines[0].spans[2].content.as_ref(), "let y = 1");
        assert_eq!(lines[0].spans[2].style.fg, None);
    }

    #[test]
    fn clip_line_preserves_styles_and_truncates() {
        let detail = ChangeDetail::Write {
            head: "abcdefgh".into(),
        };
        let line = &change_detail_lines_styled(&detail, 1, None)[0]; // "   1 + abcdefgh"
        let clipped = clip_line_to_width(line, 7);
        let text: String = clipped.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "   1 + ");
        assert_eq!(clip_line_to_width(line, 0).spans.len(), 0);
        let wide = clip_line_to_width(line, 999);
        let wide_text: String = wide.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(wide_text, "   1 + abcdefgh");
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
        let out = abbreviate_path("widgets/chronology_bar.rs", 12);
        assert!(out.chars().count() <= 12);
        assert!(out.ends_with(".rs"));
    }

    #[test]
    fn side_cell_styles_marker_gutter_and_none() {
        use sessionx::syntax::{CellKind, DiffCell, change_detail_side_by_side};
        let detail = ChangeDetail::Edit {
            old: "a".into(),
            new: "let y = 1".into(),
        };
        let rows = change_detail_side_by_side(
            &detail,
            4,
            sessionx::syntax::lang_for_path(Path::new("a.rs")),
        );
        // removed cell on the left: dim gutter, red "- " marker
        let left = side_cell_to_line(rows[0].left.as_ref());
        assert_eq!(left.spans[0].content.as_ref(), "     ");
        assert!(left.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert_eq!(left.spans[1].content.as_ref(), "- ");
        assert_eq!(left.spans[1].style.fg, Some(Color::Red));
        // added cell on the right: gutter "   4 ", green "+ ", "let" highlighted
        let right = side_cell_to_line(rows[0].right.as_ref());
        assert_eq!(right.spans[0].content.as_ref(), "   4 ");
        assert_eq!(right.spans[1].content.as_ref(), "+ ");
        assert_eq!(right.spans[1].style.fg, Some(Color::Green));
        assert!(
            right
                .spans
                .iter()
                .any(|s| s.content.as_ref() == "let" && s.style.fg == Some(Color::Magenta))
        );
        // a context cell uses a blank "  " marker with no colour
        let ctx = DiffCell {
            gutter: "   9 ".to_string(),
            kind: CellKind::Context,
            code: vec![],
        };
        let ctx_line = side_cell_to_line(Some(&ctx));
        assert_eq!(ctx_line.spans[1].content.as_ref(), "  ");
        assert_eq!(ctx_line.spans[1].style.fg, None);
        // None -> an empty line (blank column)
        assert!(side_cell_to_line(None).spans.is_empty());
    }
}
