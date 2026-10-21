//! Summary rows for workspaces a repo's hide rules keep off the dashboard.

use crate::data::hide_rules::HideRule;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::theme::Theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// One rule's hidden rows within a repo, already ordered for display.
#[derive(Debug, Clone)]
pub struct HiddenGroupView {
    pub rule: HideRule,
    pub expanded: bool,
    pub workspaces: Vec<RowInputs>,
}

/// `  ▸ ⎇ merged  ·  4 hidden`, optionally prefixed by a repo name (by-attention view).
/// Dim while collapsed; an expanded group's label and rows render at full brightness.
pub fn hidden_group_line(
    g: &HiddenGroupView,
    repo_prefix: Option<&str>,
    nerd_fonts: bool,
    theme: &Theme,
) -> Line<'static> {
    let lc = g.rule.lifecycle();
    let fold = if g.expanded { '▾' } else { '▸' };
    let text = if g.expanded {
        Style::default()
    } else {
        theme.dim_style()
    };
    let mut spans = vec![Span::styled(format!("  {fold} "), text)];
    if let Some(name) = repo_prefix {
        spans.push(Span::styled(format!("{name} · "), text));
    }
    // Tag rules have no lifecycle; their `#name` label stands in for the glyph.
    if lc.is_some() {
        spans.push(Span::styled(
            format!("{} ", crate::ui::theme::branch_glyph(lc, nerd_fonts)),
            theme.lifecycle_style(lc).unwrap_or(text),
        ));
    }
    spans.push(Span::styled(
        format!("{}  ·  {} hidden", g.rule.label(), g.workspaces.len()),
        text,
    ));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::hide_rules::PrCriterion;

    fn group(expanded: bool) -> HiddenGroupView {
        group_for(HideRule::Tag("parked".into()), expanded)
    }

    fn group_for(rule: HideRule, expanded: bool) -> HiddenGroupView {
        HiddenGroupView {
            rule,
            expanded,
            workspaces: Vec::new(),
        }
    }

    #[test]
    fn collapsed_group_label_is_dim() {
        let theme = Theme::default();
        let line = hidden_group_line(&group(false), Some("repo"), false, &theme);
        assert_eq!(line.spans[0].style, theme.dim_style());
        assert_eq!(line.spans[1].style, theme.dim_style());
        assert_eq!(line.spans.last().unwrap().style, theme.dim_style());
    }

    #[test]
    fn expanded_group_label_is_not_dim() {
        let theme = Theme::default();
        let line = hidden_group_line(&group(true), Some("repo"), false, &theme);
        assert_eq!(line.spans[0].style, Style::default());
        assert_eq!(line.spans[1].style, Style::default());
        assert_eq!(line.spans.last().unwrap().style, Style::default());
    }

    #[test]
    fn expanded_uncolored_lifecycle_glyph_is_not_dim() {
        let theme = Theme::default();
        for pr in [PrCriterion::NoPr, PrCriterion::Draft] {
            let line = hidden_group_line(&group_for(HideRule::Pr(pr), true), None, false, &theme);
            assert_eq!(line.spans[1].style, Style::default(), "{pr:?}");
        }
    }
}
