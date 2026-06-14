//! Extracted from ui/modal.rs.

use super::*;

/// Render the floating process-list modal. Reads live App state via
/// borrowed slices so the modal updates on every render tick.
#[allow(clippy::too_many_arguments)]
pub fn render_process_list(
    f: &mut Frame,
    area: Rect,
    workspace_name: &str,
    procs: &[crate::activity::proc::ProcInfo],
    selected: usize,
    input: Option<&str>,
    notice: Option<&str>,
    theme: &Theme,
) {
    let w = area.width.clamp(20, 80);
    let h = area.height.clamp(8, 25);
    let inner = panel_frame(
        f,
        area,
        w,
        h,
        format!(" Processes — {workspace_name} "),
        theme,
    );

    let has_notice = notice.is_some();
    let constraints = if has_notice {
        vec![
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);
    let body_area = chunks[0];
    let (notice_area, footer_area) = if has_notice {
        (Some(chunks[1]), chunks[2])
    } else {
        (None, chunks[1])
    };

    if procs.is_empty() {
        f.render_widget(
            Paragraph::new("(no tracked processes)").style(theme.dim_style()),
            body_area,
        );
    } else {
        let mut lines: Vec<Line> = Vec::new();
        lines.push(Line::from(Span::styled(
            format!("  {:<7} {:<20} {}", "PID", "COMMAND", "CWD"),
            theme.header_style(),
        )));
        for (i, p) in procs.iter().enumerate() {
            let body = format!(
                "  {:<7} {:<20} {}",
                p.pid,
                truncate(&p.command, 20),
                p.cwd.display()
            );
            if i == selected {
                lines.push(Line::from(Span::styled(body, theme.selected_style())));
            } else {
                lines.push(Line::from(body));
            }
        }
        f.render_widget(Paragraph::new(lines), body_area);
    }

    if let (Some(area), Some(text)) = (notice_area, notice) {
        let style = if text.starts_with("error") {
            theme.err_style()
        } else {
            theme.ok_style()
        };
        f.render_widget(Paragraph::new(text).style(style), area);
    }

    if let Some(buf) = input {
        f.render_widget(
            Paragraph::new(format!("run: {buf}\u{2588}   [enter] launch  [esc] cancel"))
                .style(theme.header_style()),
            footer_area,
        );
    } else {
        f.render_widget(
            Paragraph::new(
                "[\u{2191}/\u{2193}] move   [r] run   [k] term   [K] kill   [esc] close",
            )
            .style(theme.dim_style()),
            footer_area,
        );
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('\u{2026}');
        out
    }
}
