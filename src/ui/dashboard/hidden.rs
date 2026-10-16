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

/// Fade an expanded hidden row so it reads as revealed, not restored. Muted
/// row columns (recap, age) step down from `row_dim` to `dim` instead of
/// taking DIM, which made them unreadable off the selection background.
pub fn dimmed(mut line: Line<'static>, theme: &Theme) -> Line<'static> {
    for span in &mut line.spans {
        if span.style.fg == Some(theme.row_dim) {
            span.style = span.style.fg(theme.dim);
        } else if span.style.fg != Some(theme.dim) {
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
    fn dimmed_fades_colored_spans_and_steps_row_grey_down_to_dim() {
        let theme = Theme::default();
        let line = Line::from(vec![
            Span::styled("branch", Style::default().fg(theme.path)),
            Span::styled("recap text", theme.row_dim_style()),
            Span::styled("  29s ago", theme.dim_style()),
        ]);
        let out = dimmed(line, &theme);
        assert!(out.spans[0].style.add_modifier.contains(Modifier::DIM));
        for span in &out.spans[1..] {
            assert_eq!(span.style.fg, Some(theme.dim));
            assert!(!span.style.add_modifier.contains(Modifier::DIM));
        }
    }
}
