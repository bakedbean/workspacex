//! Summary rows for workspaces a repo's hide rules keep off the dashboard.

use crate::data::hide_rules::HideRule;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::theme::Theme;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};

/// One rule's hidden rows within a repo, already ordered for display.
#[derive(Debug, Clone)]
pub struct HiddenGroupView {
    pub rule: HideRule,
    pub expanded: bool,
    pub workspaces: Vec<RowInputs>,
}

/// `  ▸ ⎇ merged  ·  4 hidden`, optionally prefixed by a repo name (by-attention view).
pub fn hidden_group_line(
    g: &HiddenGroupView,
    repo_prefix: Option<&str>,
    nerd_fonts: bool,
    theme: &Theme,
) -> Line<'static> {
    let lc = g.rule.lifecycle();
    let fold = if g.expanded { '▾' } else { '▸' };
    let mut spans = vec![Span::styled(format!("  {fold} "), theme.dim_style())];
    if let Some(name) = repo_prefix {
        spans.push(Span::styled(format!("{name} · "), theme.dim_style()));
    }
    // Tag rules have no lifecycle; their `#name` label stands in for the glyph.
    if lc.is_some() {
        spans.push(Span::styled(
            format!("{} ", crate::ui::theme::branch_glyph(lc, nerd_fonts)),
            theme
                .lifecycle_style(lc)
                .unwrap_or_else(|| theme.dim_style()),
        ));
    }
    spans.push(Span::styled(
        format!("{}  ·  {} hidden", g.rule.label(), g.workspaces.len()),
        theme.dim_style(),
    ));
    Line::from(spans)
}

/// Fade an expanded hidden row so it reads as revealed, not restored. Spans
/// already in the dim grey (recap, age) are left alone: DIM on top made them
/// unreadable unless the selection background lit them up.
pub fn dimmed(mut line: Line<'static>, theme: &Theme) -> Line<'static> {
    for span in &mut line.spans {
        if span.style.fg != Some(theme.dim) {
            span.style = span.style.add_modifier(Modifier::DIM);
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn dimmed_fades_colored_spans_but_not_dim_grey_ones() {
        let theme = Theme::default();
        let line = Line::from(vec![
            Span::styled("branch", Style::default().fg(theme.path)),
            Span::styled("recap text", theme.dim_style()),
            Span::styled("  29s ago", theme.dim_style()),
        ]);
        let out = dimmed(line, &theme);
        assert!(out.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(!out.spans[1].style.add_modifier.contains(Modifier::DIM));
        assert!(!out.spans[2].style.add_modifier.contains(Modifier::DIM));
    }
}
