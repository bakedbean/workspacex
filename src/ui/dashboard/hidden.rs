//! Summary rows for workspaces a repo's hide rules keep off the dashboard.

use crate::data::hide_rules::HideRule;
use crate::ui::dashboard::row::RowInputs;
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
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
    spans.push(Span::styled(
        format!("{} ", crate::ui::theme::branch_glyph(lc, nerd_fonts)),
        theme
            .lifecycle_style(lc)
            .unwrap_or_else(|| theme.dim_style()),
    ));
    spans.push(Span::styled(
        format!("{}  ·  {} hidden", g.rule.label(), g.workspaces.len()),
        theme.dim_style(),
    ));
    Line::from(spans)
}

/// Fade an expanded hidden row so it reads as revealed, not restored.
pub fn dimmed(line: Line<'static>) -> Line<'static> {
    line.patch_style(Style::default().add_modifier(Modifier::DIM))
}
