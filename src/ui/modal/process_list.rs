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
    // Full command lines are long, so give the modal more room than the
    // old 80-col cap; anything still too wide is wrapped below.
    let w = area.width.clamp(20, 100);
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
            format!("  {:<7} {}", "PID", "CMD"),
            theme.header_style(),
        )));
        for (i, p) in procs.iter().enumerate() {
            // `  PID(7) CMD` — the prefix width sets the hanging indent so
            // wrapped continuation lines align under the command column.
            let prefix = format!("  {:<7} ", p.pid);
            let prefix_w = prefix.chars().count();
            let indent = " ".repeat(prefix_w);
            let text_w = (body_area.width as usize).saturating_sub(prefix_w).max(1);
            // Fall back to the short `command` if `ps` never filled the
            // full command line (e.g. the command-ps call failed).
            let cmd = if p.cmdline.is_empty() {
                p.command.as_str()
            } else {
                p.cmdline.as_str()
            };
            for (j, seg) in wrap_text(cmd, text_w).iter().enumerate() {
                let body = if j == 0 {
                    format!("{prefix}{seg}")
                } else {
                    format!("{indent}{seg}")
                };
                if i == selected {
                    lines.push(Line::from(Span::styled(body, theme.selected_style())));
                } else {
                    lines.push(Line::from(body));
                }
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

/// Word-wrap `s` into lines no wider than `width` columns, breaking on
/// spaces and hard-splitting any single token longer than `width`.
/// Always returns at least one (possibly empty) line. A `width` of 0 is
/// degenerate and yields the input unsplit.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![s.to_string()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;
    for word in s.split(' ') {
        // Pre-split an over-long token into width-sized fragments so the
        // greedy packer below only ever sees pieces that fit on a line.
        let fragments: Vec<String> = if word.chars().count() > width {
            word.chars()
                .collect::<Vec<_>>()
                .chunks(width)
                .map(|c| c.iter().collect())
                .collect()
        } else {
            vec![word.to_string()]
        };
        for frag in fragments {
            let frag_len = frag.chars().count();
            if current_len == 0 {
                current.push_str(&frag);
                current_len = frag_len;
            } else if current_len + 1 + frag_len <= width {
                current.push(' ');
                current.push_str(&frag);
                current_len += 1 + frag_len;
            } else {
                lines.push(std::mem::take(&mut current));
                current.push_str(&frag);
                current_len = frag_len;
            }
        }
    }
    lines.push(current);
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::proc::ProcInfo;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn proc(pid: i32, command: &str, cmdline: &str, cwd: &str) -> ProcInfo {
        ProcInfo {
            pid,
            ppid: 0,
            command: command.into(),
            cmdline: cmdline.into(),
            cwd: PathBuf::from(cwd),
            listening: false,
        }
    }

    fn render_to_string(procs: &[ProcInfo], w: u16, h: u16) -> String {
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| {
            render_process_list(f, f.area(), "ws", procs, 0, None, None, &theme);
        })
        .unwrap();
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn modal_shows_full_command_not_cwd() {
        // The header is now CMD (not CWD/COMMAND) and the full command
        // line is displayed while the cwd is gone entirely.
        let procs = vec![proc(
            4242,
            "node",
            "node server.js --port 3000",
            "/workdir/secret-cwd",
        )];
        let text = render_to_string(&procs, 80, 16);
        assert!(text.contains("CMD"), "missing CMD header:\n{text}");
        assert!(!text.contains("CWD"), "CWD header still present:\n{text}");
        assert!(
            text.contains("node server.js --port 3000"),
            "full command line missing:\n{text}"
        );
        assert!(
            !text.contains("/workdir/secret-cwd"),
            "cwd should no longer be shown:\n{text}"
        );
    }

    #[test]
    fn modal_wraps_long_command_in_narrow_modal() {
        // In a narrow terminal the command exceeds the column width, so
        // it must wrap: the whole string never appears on one buffer row,
        // but its head and tail tokens are both rendered.
        let cmdline = "node /srv/app/server.js --port 3000 --inspect --max-old-space";
        let procs = vec![proc(99, "node", cmdline, "/srv/app")];
        let text = render_to_string(&procs, 40, 16);
        assert!(
            !text.contains(cmdline),
            "command should have wrapped, not fit on one line:\n{text}"
        );
        assert!(text.contains("node /srv"), "head token missing:\n{text}");
        assert!(
            text.contains("max-old-space"),
            "tail token missing (lost to wrapping):\n{text}"
        );
    }

    #[test]
    fn modal_falls_back_to_short_command_when_cmdline_empty() {
        // If the command-ps refinement never ran, cmdline is empty and we
        // fall back to the short comm rather than rendering a blank cell.
        let procs = vec![proc(7, "pnpm", "", "/wt")];
        let text = render_to_string(&procs, 80, 12);
        assert!(text.contains("pnpm"), "fallback command missing:\n{text}");
    }

    #[test]
    fn wrap_text_keeps_short_line_intact() {
        assert_eq!(wrap_text("node app.js", 40), vec!["node app.js"]);
    }

    #[test]
    fn wrap_text_breaks_on_word_boundary() {
        assert_eq!(
            wrap_text("node app.js --port 3000", 12),
            vec!["node app.js", "--port 3000"]
        );
    }

    #[test]
    fn wrap_text_hard_splits_overlong_token() {
        // A path with no spaces longer than the width is split at the
        // column boundary rather than overflowing.
        assert_eq!(wrap_text("aaaaaaaaaa", 4), vec!["aaaa", "aaaa", "aa"]);
    }

    #[test]
    fn wrap_text_empty_yields_single_empty_line() {
        assert_eq!(wrap_text("", 10), vec![""]);
    }

    #[test]
    fn wrap_text_zero_width_is_unsplit() {
        assert_eq!(wrap_text("anything here", 0), vec!["anything here"]);
    }
}
