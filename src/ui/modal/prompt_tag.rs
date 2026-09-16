//! The prompt-tag modal: pick (or name) an XML tag, then type the body
//! that gets wrapped in it and inserted into the focused agent's composer.
//! State and the pure list helpers live here; key handling is in
//! `app::input::modal::prompt_tag`.

use super::{TextArea, panel_frame};
use crate::commands::tags::{PromptTag, is_valid_name};
use crate::ui::theme::Theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagStage {
    /// Choosing (or naming) the tag.
    Pick,
    /// Typing the body for `name`.
    Body { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTagModal {
    pub stage: TagStage,
    /// The pick stage's text field: filters the list and names a new tag.
    pub name_field: String,
    /// Index into the FILTERED list.
    pub selected: usize,
    /// The body draft. Survives Esc from the body stage back to pick.
    pub body: TextArea,
}

/// What Enter in the pick stage does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnterAction {
    Existing(String),
    Create(String),
}

impl PromptTagModal {
    pub fn pick() -> Self {
        Self {
            stage: TagStage::Pick,
            name_field: String::new(),
            selected: 0,
            body: TextArea::new(),
        }
    }

    pub fn for_tag(name: &str) -> Self {
        Self {
            stage: TagStage::Body {
                name: name.to_string(),
            },
            ..Self::pick()
        }
    }

    /// Tags whose name starts with the field's text, case-insensitively,
    /// in the caller's (display) order.
    pub fn filtered<'a>(&self, tags: &'a [PromptTag]) -> Vec<&'a PromptTag> {
        let needle = self.name_field.to_ascii_lowercase();
        tags.iter()
            .filter(|t| t.name.to_ascii_lowercase().starts_with(&needle))
            .collect()
    }

    /// `1`–`9` jump straight to a listed tag only while nothing is typed;
    /// once the field has text a digit is part of a name.
    pub fn accelerators_active(&self) -> bool {
        self.name_field.is_empty()
    }

    pub fn select_up(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_down(&mut self, len: usize) {
        self.selected = (self.selected + 1).min(len.saturating_sub(1));
    }

    /// The typed name is non-empty and not a legal tag name.
    pub fn name_invalid(&self) -> bool {
        !self.name_field.is_empty() && !is_valid_name(&self.name_field)
    }

    pub fn enter_action(&self, tags: &[PromptTag]) -> Option<EnterAction> {
        let list = self.filtered(tags);
        if let Some(t) = list.get(self.selected) {
            return Some(EnterAction::Existing(t.name.clone()));
        }
        if self.name_field.is_empty() || self.name_invalid() {
            return None;
        }
        Some(EnterAction::Create(self.name_field.clone()))
    }
}

pub fn render_prompt_tag(
    f: &mut Frame,
    area: Rect,
    modal: &PromptTagModal,
    tags: &[PromptTag],
    theme: &Theme,
) {
    let w = area.width.clamp(50, 90);
    let h = area.height.clamp(12, 24);
    match &modal.stage {
        TagStage::Pick => render_pick(f, area, w, h, modal, tags, theme),
        TagStage::Body { name } => render_body(f, area, w, h, modal, name, theme),
    }
}

fn render_pick(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    modal: &PromptTagModal,
    tags: &[PromptTag],
    theme: &Theme,
) {
    let inner = panel_frame(f, area, w, h, " Prompt tag ", theme);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let (field_area, list_area, footer_area) = (chunks[0], chunks[2], chunks[3]);

    let field_style = if modal.name_invalid() {
        theme.err_style()
    } else {
        Style::default()
    };
    let prompt = "  name: ";
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(prompt, theme.dim_style()),
            Span::styled(modal.name_field.clone(), field_style),
        ])),
        field_area,
    );
    f.set_cursor_position((
        field_area.x + (prompt.len() + modal.name_field.chars().count()) as u16,
        field_area.y,
    ));

    let list = modal.filtered(tags);
    let height = list_area.height as usize;
    let top = modal.selected.saturating_sub(height.saturating_sub(1));
    let mut lines: Vec<Line> = Vec::new();
    for (i, t) in list.iter().enumerate().skip(top).take(height) {
        let key = if modal.accelerators_active() && i < 9 {
            format!(" {} ", i + 1)
        } else {
            "   ".to_string()
        };
        let row = format!("  {key} {:<24} ×{}", t.name, t.uses);
        let style = if i == modal.selected {
            theme.selected_style()
        } else {
            Style::default()
        };
        lines.push(Line::from(Span::styled(row, style)));
    }
    if list.is_empty() {
        let hint = if modal.name_field.is_empty() {
            "  no saved tags yet — type a name and press enter"
        } else if modal.name_invalid() {
            "  letters, digits, _ . - only; must start with a letter or _"
        } else {
            "  enter creates this tag"
        };
        lines.push(Line::from(Span::styled(hint, theme.dim_style())));
    }
    f.render_widget(Paragraph::new(lines), list_area);

    f.render_widget(
        Paragraph::new(
            "[\u{2191}/\u{2193}] move   [enter] body   [1-9] pick   [^d] delete   [esc] close",
        )
        .style(theme.dim_style()),
        footer_area,
    );
}

fn render_body(
    f: &mut Frame,
    area: Rect,
    w: u16,
    h: u16,
    modal: &PromptTagModal,
    name: &str,
    theme: &Theme,
) {
    let inner = panel_frame(f, area, w, h, format!(" <{name}> "), theme);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let box_area = Rect {
        x: chunks[0].x + 1,
        width: chunks[0].width.saturating_sub(2),
        ..chunks[0]
    };
    modal.body.render(f, box_area, theme);
    f.render_widget(
        Paragraph::new("[^s] insert   [enter] newline   [esc] back").style(theme.dim_style()),
        chunks[1],
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags() -> Vec<PromptTag> {
        ["context", "constraints", "task"]
            .iter()
            .enumerate()
            .map(|(i, n)| PromptTag {
                name: (*n).into(),
                uses: 9 - i as u32,
            })
            .collect()
    }

    #[test]
    fn pick_starts_on_the_first_tag_with_accelerators_on() {
        let m = PromptTagModal::pick();
        assert_eq!(m.stage, TagStage::Pick);
        assert_eq!(m.selected, 0);
        assert!(m.accelerators_active());
    }

    #[test]
    fn for_tag_opens_straight_into_the_body_stage() {
        let m = PromptTagModal::for_tag("context");
        assert_eq!(
            m.stage,
            TagStage::Body {
                name: "context".into()
            }
        );
        assert!(m.body.is_blank());
    }

    #[test]
    fn filtering_is_a_case_insensitive_prefix_match_that_disables_accelerators() {
        let all = tags();
        let mut m = PromptTagModal::pick();
        assert_eq!(m.filtered(&all).len(), 3);
        m.name_field = "CON".into();
        let names: Vec<_> = m.filtered(&all).iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["context", "constraints"]);
        assert!(!m.accelerators_active());
    }

    #[test]
    fn selection_clamps_to_the_filtered_list() {
        let mut m = PromptTagModal::pick();
        m.select_up();
        assert_eq!(m.selected, 0);
        m.select_down(3);
        m.select_down(3);
        m.select_down(3);
        assert_eq!(m.selected, 2);
        m.select_down(0);
        assert_eq!(m.selected, 0, "an empty list has nothing to select");
    }

    #[test]
    fn enter_picks_the_selection_or_creates_a_valid_new_name() {
        let all = tags();
        let mut m = PromptTagModal::pick();
        m.selected = 1;
        assert_eq!(
            m.enter_action(&all),
            Some(EnterAction::Existing("constraints".into()))
        );
        m.name_field = "task".into();
        m.selected = 0;
        assert_eq!(
            m.enter_action(&all),
            Some(EnterAction::Existing("task".into()))
        );
        m.name_field = "examples".into();
        assert_eq!(
            m.enter_action(&all),
            Some(EnterAction::Create("examples".into()))
        );
        m.name_field = "bad name".into();
        assert_eq!(m.enter_action(&all), None);
        assert!(m.name_invalid());
        m.name_field.clear();
        assert!(!m.name_invalid());
        assert_eq!(
            PromptTagModal::pick().enter_action(&[]),
            None,
            "nothing listed and nothing typed"
        );
    }
}
