//! Extracted from ui/attached.rs.

use super::*;
use crate::ui::footer::{FooterHintAction, FooterHintSpan, key_for_glyph};

/// V5-styled footer: workspace label in `header_style`, then the `^x`
/// leader, then per-keybind chips (`<key>` in dim+bold, ` <label>` in
/// `path` color), separated by 2 spaces. Matches the dashboard footer's
/// chip pattern.
pub(super) fn footer_line(
    label: &str,
    agent: Option<AgentKind>,
    multi_pane: bool,
    theme: &Theme,
) -> (Line<'static>, Vec<FooterHintSpan>) {
    let keys: &[(&str, &str)] = if multi_pane {
        &[
            ("d", "close-pane"),
            ("←→", "focus"),
            ("u", "updates"),
            ("a", "agents"),
            ("e", "edit"),
            ("t", "term"),
            ("v", "diff"),
            ("g", "lazygit"),
            ("k", "procs"),
            ("x", "send-^x"),
        ]
    } else {
        &[
            ("d", "detach"),
            ("u", "updates"),
            ("a", "agents"),
            ("e", "edit"),
            ("t", "term"),
            ("v", "diff"),
            ("g", "lazygit"),
            ("k", "procs"),
            ("x", "send-^x"),
        ]
    };
    let label_style = Style::default().fg(theme.path);

    let mut spans: Vec<Span<'static>> = Vec::with_capacity(2 + keys.len() * 5 + 4);
    // `col` tracks the running column so each pill (and the `^x` leader pill)
    // can be recorded as a clickable hint with offsets relative to the line
    // start. The caller turns these into absolute screen rects.
    let mut hints: Vec<FooterHintSpan> = Vec::new();
    let mut col: u16 = 0;
    let push = |spans: &mut Vec<Span<'static>>, col: &mut u16, span: Span<'static>| {
        *col += span.content.chars().count() as u16;
        spans.push(span);
    };
    if let Some(a) = agent {
        push(
            &mut spans,
            &mut col,
            Span::styled("▎".to_string(), theme.agent_style(a)),
        );
        push(&mut spans, &mut col, Span::raw(" ".to_string()));
    }
    push(
        &mut spans,
        &mut col,
        Span::styled(label.to_string(), theme.header_style()),
    );
    push(&mut spans, &mut col, Span::raw("   ".to_string()));
    // ^x leader rendered as a standalone pill (no label tail). Clicking it
    // arms the leader, exactly like pressing Ctrl-x.
    let leader_start = col;
    for s in key_pill_spans("^x", theme) {
        push(&mut spans, &mut col, s);
    }
    hints.push(FooterHintSpan {
        start_col: leader_start,
        width: col - leader_start,
        action: FooterHintAction::ArmLeader,
    });
    for (key, lbl) in keys {
        push(&mut spans, &mut col, Span::raw("  ".to_string()));
        let start = col;
        for s in key_pill_spans(key, theme) {
            push(&mut spans, &mut col, s);
        }
        push(
            &mut spans,
            &mut col,
            Span::styled(format!(" {lbl}"), label_style),
        );
        if let Some(key_event) = key_for_glyph(key) {
            hints.push(FooterHintSpan {
                start_col: start,
                width: col - start,
                action: FooterHintAction::Key(key_event),
            });
        }
    }
    (Line::from(spans), hints)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footer_line_pill_wraps_key_only_not_label() {
        // The attached footer's ^x leader and each chord key get a pill;
        // the label following each chord is plain text on the bar bg. A
        // regression that re-extended bg_soft over the label would
        // visually merge key and label into one block.
        let theme = crate::ui::theme::Theme::wsx();
        let (line, _) = footer_line("ws", None, false, &theme);
        let leader = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "^x")
            .expect("expected ^x leader span");
        assert_eq!(leader.style.bg, Some(theme.bg_soft));
        let chord_key = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "d")
            .expect("expected `d` chord-key span");
        assert_eq!(chord_key.style.bg, Some(theme.bg_soft));
        let chord_label = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == " detach")
            .expect("expected ` detach` chord-label span (no chip padding)");
        assert_eq!(
            chord_label.style.bg, None,
            "label should not carry the chip bg"
        );
    }

    #[test]
    fn footer_line_prepends_agent_bar_when_present() {
        let theme = Theme::wsx();
        let (line, _) = footer_line("wsx/foo", Some(AgentKind::Codex), false, &theme);
        assert_eq!(line.spans[0].content.as_ref(), "▎");
        assert_eq!(
            line.spans[0].style.fg,
            theme.agent_style(AgentKind::Codex).fg
        );
        assert_eq!(
            line.spans[2].content.as_ref(),
            "wsx/foo",
            "label follows the bar and its trailing space"
        );
    }

    #[test]
    fn footer_line_omits_agent_bar_when_none() {
        let theme = Theme::wsx();
        let (line, _) = footer_line("project-manager", None, false, &theme);
        assert_eq!(
            line.spans[0].content.as_ref(),
            "project-manager",
            "no leading bar for the PM pane"
        );
    }

    #[test]
    fn footer_line_emits_leader_and_keybind_hints() {
        // The ^x pill arms the leader; each chord key maps to its key press.
        // Hint column runs must line up with the rendered glyphs so clicks
        // land on the right pill.
        let theme = Theme::wsx();
        let (line, hints) = footer_line("ws", None, false, &theme);
        let cells: Vec<char> = line.spans.iter().flat_map(|s| s.content.chars()).collect();
        let slice = |h: &FooterHintSpan| -> String {
            cells[h.start_col as usize..(h.start_col + h.width) as usize]
                .iter()
                .collect()
        };
        let leader = hints
            .iter()
            .find(|h| h.action == FooterHintAction::ArmLeader)
            .expect("leader hint present");
        assert_eq!(slice(leader), " ^x ", "leader hint covers the ^x pill");
        let detach = hints
            .iter()
            .find(|h| h.action == FooterHintAction::Key(key_for_glyph("d").unwrap()))
            .expect("detach hint present");
        assert_eq!(slice(detach), " d  detach", "d hint covers pill + label");
    }
}
