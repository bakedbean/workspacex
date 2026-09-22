//! The setup-log viewer: `o` from the workspace-actions card.
//!
//! One panel covers both halves of a workspace's setup story. While a create
//! or archive is in flight the source is the live `SetupProgress` ring buffer,
//! headed by a spinner and elapsed time. Once nothing is running the source is
//! the log persisted under `Dirs::log_dir()`, which outlives the process — so
//! the key does the same thing whether or not the user happens to catch the
//! build while it is running.

use super::*;

/// The persisted side of a workspace's setup log, resolved once when it
/// becomes the viewer's source.
///
/// `Missing` and `Unreadable` are deliberately separate. Folding a permission
/// or decode failure into "there is no log" tells the user their repo has no
/// setup script, which sends them looking in entirely the wrong place; both
/// carry the path so they can go and look themselves.
#[derive(Debug, Clone)]
pub enum StoredLog {
    /// The log's tail, newest-last. May legitimately be empty if the file is.
    Lines(Vec<String>),
    /// Nothing at the log path. The normal state for a repo with no setup
    /// script, since nothing is written in that case.
    Missing { path: String },
    /// The file is there but could not be read.
    Unreadable { path: String, error: String },
}

impl StoredLog {
    fn lines(&self) -> &[String] {
        match self {
            StoredLog::Lines(l) => l,
            _ => &[],
        }
    }
}

/// Everything the panel draws, gathered by the caller so the renderer itself
/// touches no App state.
pub struct SetupLogView<'a> {
    /// `repo/name`, for the frame title.
    pub label: &'a str,
    /// The in-flight entry being tailed, when there is one.
    pub live: Option<&'a crate::data::in_flight::InFlight>,
    /// The persisted log; `None` while `live` is the source.
    pub stored: Option<&'a StoredLog>,
    /// Lines scrolled up from the end. Clamped here and returned.
    pub scroll: usize,
    /// Spinner frame counter.
    pub tick: u32,
}

/// Draw the viewer and return `scroll` clamped to what actually fits, so the
/// caller can store the clamp back on the modal. Without that, holding Up
/// past the top of a short log would run the counter away and the first
/// several Downs would appear to do nothing.
pub fn render_setup_log(f: &mut Frame, area: Rect, view: &SetupLogView, theme: &Theme) -> usize {
    let w = area.width.clamp(20, 100);
    let h = area.height.clamp(8, 28);
    let inner = panel_frame(
        f,
        area,
        w,
        h,
        format!(" Setup log — {} ", view.label),
        theme,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
    let (status_area, body_area, footer_area) = (chunks[0], chunks[1], chunks[2]);

    // Archive never sets a `SetupPhase` — there is no phase concept for it —
    // so the status line is derived from the entry's kind rather than from
    // `phase()`, which would otherwise read "Fetching base…" while a worktree
    // is mid-deletion. See the F7 regression test below.
    let (status, lines): (String, Vec<String>) = match view.live {
        Some(f) => {
            use crate::data::in_flight::InFlightKind;
            let label = match f.kind {
                InFlightKind::Archive => "Archiving",
                InFlightKind::Create => match f.progress.lock() {
                    Ok(p) => p.phase().label(),
                    Err(_) => "Working",
                },
            };
            let secs = f.started.elapsed().as_secs();
            let frame = crate::ui::dashboard::spinner::frame(view.tick);
            let lines = match f.progress.lock() {
                Ok(p) => p.recent(usize::MAX),
                Err(_) => Vec::new(),
            };
            (
                format!("{frame} {label}…   {:02}:{:02}", secs / 60, secs % 60),
                lines,
            )
        }
        None => {
            let lines = view.stored.map(StoredLog::lines).unwrap_or(&[]).to_vec();
            // The file is truncated on each run, so what is on disk is always
            // the most recent build and nothing older.
            let status = match lines.len() {
                0 => String::new(),
                n => format!("last run — {n} line(s)"),
            };
            (status, lines)
        }
    };

    f.render_widget(
        Paragraph::new(status).style(theme.header_style()),
        status_area,
    );

    let body_h = body_area.height as usize;
    let scroll = view.scroll.min(lines.len().saturating_sub(body_h));
    if lines.is_empty() {
        // Four distinct reasons the body is empty; saying the wrong one sends
        // the user somewhere useless.
        let (empty, style) = match (view.live, view.stored) {
            // A live entry that has not printed anything yet, rather than a
            // log that does not exist.
            (Some(_), _) => ("(waiting for output…)".to_string(), theme.dim_style()),
            (None, Some(StoredLog::Unreadable { path, error })) => (
                format!("(setup log could not be read)\n\n{error}\n\n{path}"),
                theme.err_style(),
            ),
            (None, Some(StoredLog::Missing { path })) => (
                format!(
                    "(no setup log)\n\nNothing was captured for this workspace — either its \
                     repo has no setup script, or it was created before setup logging.\n\n{path}"
                ),
                theme.dim_style(),
            ),
            // The file exists and read cleanly, but holds nothing.
            (None, _) => ("(setup log is empty)".to_string(), theme.dim_style()),
        };
        f.render_widget(
            Paragraph::new(empty)
                .style(style)
                .wrap(ratatui::widgets::Wrap { trim: false }),
            body_area,
        );
    } else {
        let end = lines.len() - scroll;
        let start = end.saturating_sub(body_h);
        let width = body_area.width as usize;
        let rendered: Vec<Line> = lines[start..end]
            .iter()
            .map(|l| {
                // Stderr lines are written with a `! ` marker by
                // `setup_log::write_line`; colour them so a failing script's
                // complaints stand out from its chatter.
                let style = if l.starts_with("! ") {
                    theme.err_style()
                } else {
                    Style::default()
                };
                Line::from(Span::styled(truncate_to(l, width), style))
            })
            .collect();
        f.render_widget(Paragraph::new(rendered), body_area);
    }

    let footer = if lines.len() > body_h {
        let more = lines.len() - body_h - scroll;
        if more > 0 {
            format!("[\u{2191}/\u{2193}] scroll ({more} more above)   [g/G] top/end   [esc] close")
        } else {
            "[\u{2191}/\u{2193}] scroll   [g/G] top/end   [esc] close".to_string()
        }
    } else {
        "[esc] close".to_string()
    };
    f.render_widget(Paragraph::new(footer).style(theme.dim_style()), footer_area);

    scroll
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::in_flight::InFlight;
    use crate::data::progress::{SetupPhase, SetupProgress};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use tokio_util::sync::CancellationToken;

    /// Render at a fixed size and return `(screen text, clamped scroll)`.
    fn render_to_text(view: &SetupLogView, h: u16) -> (String, usize) {
        let theme = Theme::wsx();
        let mut term = Terminal::new(TestBackend::new(60, h)).unwrap();
        let mut clamped = 0;
        term.draw(|f| clamped = render_setup_log(f, f.area(), view, &theme))
            .unwrap();
        let buf = term.backend().buffer();
        let text = (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        (text, clamped)
    }

    fn live(kind_archive: bool, lines: &[&str], phase: SetupPhase) -> InFlight {
        let progress = SetupProgress::shared();
        {
            let mut p = progress.lock().unwrap();
            p.set_phase(phase);
            for l in lines {
                p.push_line(l);
            }
        }
        if kind_archive {
            InFlight::archive(progress, CancellationToken::new())
        } else {
            InFlight::create(progress, CancellationToken::new())
        }
    }

    fn view<'a>(
        live: Option<&'a InFlight>,
        stored: Option<&'a StoredLog>,
        scroll: usize,
    ) -> SetupLogView<'a> {
        SetupLogView {
            label: "myrepo/foo",
            live,
            stored,
            scroll,
            tick: 0,
        }
    }

    #[test]
    fn live_create_shows_phase_and_recent_lines() {
        let f = live(
            false,
            &["mise install", "Installing dependencies"],
            SetupPhase::RunningSetup,
        );
        let (text, _) = render_to_text(&view(Some(&f), None, 0), 24);
        assert!(text.contains("Setup log — myrepo/foo"), "{text}");
        assert!(text.contains("Running setup"), "missing phase:\n{text}");
        assert!(
            text.contains("Installing dependencies"),
            "missing line:\n{text}"
        );
        assert!(text.contains("[esc] close"), "missing footer:\n{text}");
    }

    /// F7 regression: an archive entry never sets a `SetupPhase` (there is no
    /// phase concept for it), so reading `p.phase().label()` unconditionally
    /// showed create's default phase ("Fetching base…") while a worktree was
    /// mid-deletion. The status line must come from the entry's
    /// `InFlightKind` instead.
    #[test]
    fn live_archive_is_labelled_truthfully_not_as_setup() {
        let f = live(true, &["removing worktree"], SetupPhase::Fetching);
        let (text, _) = render_to_text(&view(Some(&f), None, 0), 24);
        assert!(text.contains("Archiving"), "{text}");
        assert!(
            !text.contains("Fetching base"),
            "must not show create's default phase for an archive:\n{text}"
        );
        assert!(
            text.contains("removing worktree"),
            "missing progress line:\n{text}"
        );
    }

    #[test]
    fn overwide_line_is_truncated_to_the_body_width() {
        let f = live(false, &["x".repeat(500).as_str()], SetupPhase::RunningSetup);
        let (text, _) = render_to_text(&view(Some(&f), None, 0), 24);
        assert!(
            text.contains('…'),
            "over-wide line should be truncated:\n{text}"
        );
    }

    /// The whole point of the change: a workspace with nothing in flight
    /// still opens onto its persisted log.
    #[test]
    fn stored_log_renders_without_any_in_flight_work() {
        let stored = StoredLog::Lines(vec![
            "=== setup: myrepo/foo ===".into(),
            "npm ci".into(),
            "=== OK ===".into(),
        ]);
        let (text, _) = render_to_text(&view(None, Some(&stored), 0), 24);
        assert!(text.contains("npm ci"), "{text}");
        assert!(text.contains("=== OK ==="), "{text}");
        assert!(
            text.contains("last run — 3 line(s)"),
            "status should summarise the stored log:\n{text}"
        );
    }

    /// The other half of "behaves the way a user expects": a workspace whose
    /// repo has no setup script says so instead of showing an empty box, and
    /// names the path so the user can go and look.
    #[test]
    fn a_missing_log_explains_itself_and_names_the_path() {
        let missing = StoredLog::Missing {
            path: "/logs/setup-myrepo-foo.log".into(),
        };
        let (text, _) = render_to_text(&view(None, Some(&missing), 0), 24);
        assert!(text.contains("(no setup log)"), "{text}");
        assert!(text.contains("no setup script"), "{text}");
        assert!(text.contains("setup-myrepo-foo.log"), "{text}");
    }

    /// A permission or decode failure must NOT read as "this repo has no
    /// setup script" — that sends the user looking in the wrong place.
    #[test]
    fn an_unreadable_log_says_so_rather_than_claiming_there_is_none() {
        let broken = StoredLog::Unreadable {
            path: "/logs/setup-myrepo-foo.log".into(),
            error: "Permission denied (os error 13)".into(),
        };
        let (text, _) = render_to_text(&view(None, Some(&broken), 0), 24);
        assert!(text.contains("could not be read"), "{text}");
        assert!(text.contains("Permission denied"), "{text}");
        assert!(text.contains("setup-myrepo-foo.log"), "{text}");
        assert!(
            !text.contains("no setup script"),
            "must not blame a missing script for a read failure:\n{text}"
        );
    }

    #[test]
    fn an_empty_file_is_not_a_missing_one() {
        let empty = StoredLog::Lines(Vec::new());
        let (text, _) = render_to_text(&view(None, Some(&empty), 0), 24);
        assert!(text.contains("(setup log is empty)"), "{text}");
    }

    #[test]
    fn scroll_moves_the_window_up_through_the_log() {
        let all: Vec<String> = (0..60).map(|i| format!("line {i}")).collect();
        let stored = StoredLog::Lines(all.clone());
        let (bottom, _) = render_to_text(&view(None, Some(&stored), 0), 12);
        assert!(
            bottom.contains("line 59"),
            "unscrolled shows the tail:\n{bottom}"
        );

        let (up, _) = render_to_text(&view(None, Some(&stored), 20), 12);
        assert!(
            !up.contains("line 59"),
            "scrolled view left the tail:\n{up}"
        );
        assert!(up.contains("line 39"), "{up}");
    }

    /// `g` sets `scroll` to `usize::MAX` and relies on the renderer to clamp
    /// it, so the clamp has to be exact — otherwise Down would do nothing
    /// until the counter had been walked back down from MAX.
    #[test]
    fn scroll_is_clamped_to_the_top_and_reported_back() {
        let all: Vec<String> = (0..60).map(|i| format!("line {i}")).collect();
        let stored = StoredLog::Lines(all.clone());
        let (text, clamped) = render_to_text(&view(None, Some(&stored), usize::MAX), 12);
        assert!(
            text.contains("line 0"),
            "clamped view shows the head:\n{text}"
        );
        assert!(
            clamped < all.len(),
            "clamp must be a real offset, got {clamped}"
        );
        // One line of status, one of footer, two of border.
        assert_eq!(clamped, all.len() - (12 - 4));
    }

    #[test]
    fn a_log_that_fits_needs_no_scroll_hint() {
        let stored = StoredLog::Lines(vec!["one".into(), "two".into()]);
        let (text, clamped) = render_to_text(&view(None, Some(&stored), 0), 24);
        assert_eq!(clamped, 0);
        assert!(
            !text.contains("scroll"),
            "no hint when it all fits:\n{text}"
        );
    }
}
