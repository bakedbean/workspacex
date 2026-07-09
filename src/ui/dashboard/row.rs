//! Shared column composer for V5 workspace rows. Returns a
//! `ratatui::text::Line` so view modules can drop it straight into a
//! `ListItem`.
//!
//! Columns (left → right):
//!   1ch  ▎ gutter (status color)
//!   3ch  ├  elbow (faint, centered)
//!   2ch  status glyph or spinner frame
//!   24ch name (left-aligned, ellipsized)
//!   28ch ⎇ branch
//!   6ch  ● Np procs (or faint dot when zero)
//!   12ch +N −N diff
//!   flex └ message (or em-dash)
//!   10ch right-aligned Ns ago

use crate::git::DiffStats;
use crate::git::forge::BranchLifecycle;
use crate::pty::session::AgentKind;
use crate::ui::dashboard::column_content::{ColumnEmphasis, RowColumn};
use crate::ui::dashboard::spinner;
use crate::ui::dashboard::status::Status;
use crate::ui::text::{truncate, truncate_pad};
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub const DEFAULT_NAME_WIDTH: usize = 24;
pub const DEFAULT_BRANCH_WIDTH: usize = 28;
pub const MIN_NAME_WIDTH: usize = 10;
pub const MIN_BRANCH_WIDTH: usize = 10;
pub const MAX_NAME_WIDTH: usize = 60;
pub const MAX_BRANCH_WIDTH: usize = 80;
const PROCS_WIDTH: usize = 6;
const DIFF_WIDTH: usize = 12;
const AGE_WIDTH: usize = 10;
const GUTTER_WIDTH: usize = 1;
const ELBOW_WIDTH: usize = 3;
const GLYPH_WIDTH: usize = 2;
const AGENT_WIDTH: usize = 1;

/// User-resizable column widths. Values are clamped to safe ranges by
/// `ColumnWidths::clamped` (called from the config read path) so the
/// renderer never has to defend itself against pathological inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnWidths {
    pub name: usize,
    pub branch: usize,
}

impl ColumnWidths {
    pub fn clamped(name: usize, branch: usize) -> Self {
        Self {
            name: name.clamp(MIN_NAME_WIDTH, MAX_NAME_WIDTH),
            branch: branch.clamp(MIN_BRANCH_WIDTH, MAX_BRANCH_WIDTH),
        }
    }
}

impl Default for ColumnWidths {
    fn default() -> Self {
        Self {
            name: DEFAULT_NAME_WIDTH,
            branch: DEFAULT_BRANCH_WIDTH,
        }
    }
}

/// Inputs the renderer needs about one workspace, gathered by the caller
/// from `app.rs` state.
#[derive(Debug, Clone)]
pub struct RowInputs {
    pub agent: AgentKind,
    pub status: Status,
    pub name: String,
    pub branch: String,
    pub procs: u32,
    pub diff: Option<DiffStats>,
    pub column: Option<RowColumn>,
    pub ago_secs: Option<u64>,
    pub selected: bool,
    pub yolo: bool,
    pub setup_failed: bool,
    /// Workspace is tmux-backed ("shared"): its agent sessions live in a
    /// tmux server and survive wsx quitting. Renders a badge before the
    /// branch glyph.
    pub shared: bool,
    /// The shared workspace's tmux session is currently alive (a client is
    /// attached in this wsx, or it survives detached on the server). Colors
    /// the shared badge green; red means shared-but-no-live-session (the
    /// session died or was never started).
    pub shared_active: bool,
    pub has_multi_pane_layout: bool,
    pub lifecycle: Option<BranchLifecycle>,
    pub nerd_fonts: bool,
    pub workspace_id: crate::data::store::WorkspaceId,
}

pub fn render(
    inputs: &RowInputs,
    widths: ColumnWidths,
    tick: u32,
    theme: &Theme,
    total_width: usize,
) -> Line<'static> {
    let name_width = widths.name;
    let branch_width = widths.branch;
    let mut spans: Vec<Span<'static>> = Vec::new();

    // 0: agent identity bar — a fixed per-agent color, independent of
    // status. Sits left of the status gutter so the row shows a two-tone
    // left edge: outer = agent, inner = status. Plain Unicode, no
    // nerd-font gating (same glyph as the gutter).
    spans.push(Span::styled(
        "▎".to_string(),
        theme.agent_style(inputs.agent),
    ));

    // 1: gutter — thicker bar on the selected row gives a high-contrast
    // leading edge that doesn't rely on the row-bg tint being visible.
    let gutter_glyph = if inputs.selected { "▍" } else { "▎" };
    spans.push(Span::styled(
        gutter_glyph.to_string(),
        theme.status_style(inputs.status),
    ));

    // 2: elbow
    spans.push(Span::styled("├  ".to_string(), theme.dim_style()));

    // 3: glyph or spinner
    let glyph = if inputs.status.is_live() {
        spinner::frame(tick).to_string()
    } else {
        inputs.status.glyph().to_string()
    };
    let mut glyph_padded = String::with_capacity(2);
    glyph_padded.push_str(&glyph);
    while display_width(&glyph_padded) < GLYPH_WIDTH {
        glyph_padded.push(' ');
    }
    spans.push(Span::styled(
        glyph_padded,
        theme.status_style(inputs.status),
    ));

    // 4: name (with setup-failed badge and YOLO styling). The badge
    // sits IMMEDIATELY after the visible name characters (then trailing
    // padding fills the rest of `name_width`) so it stays attached to
    // the name even when the name is short or truncated to `…`. The
    // multi-pane-layout glyph used to live here too, but on narrow
    // displays the name truncated to `…` AND the glyph could be
    // clipped by the column edge. It now lives at the START of the
    // branch column where it never has to fight name truncation for
    // space.
    let setup_badge_width = if inputs.setup_failed { 3 } else { 0 };
    let name_target = name_width.saturating_sub(setup_badge_width).max(1);
    let name_truncated = truncate(&inputs.name, name_target);
    let name_visible_width = name_truncated.chars().count();
    let mut name_style = Style::default().add_modifier(Modifier::BOLD);
    if inputs.yolo {
        name_style = name_style.fg(theme.warn);
    }
    spans.push(Span::styled(name_truncated, name_style));
    if inputs.setup_failed {
        spans.push(Span::styled(" ⚙!".to_string(), theme.err_style()));
    }
    let consumed = name_visible_width + setup_badge_width;
    if consumed < name_width {
        spans.push(Span::raw(" ".repeat(name_width - consumed)));
    }

    // 5: branch — optionally prefixed by the multi-pane-layout glyph.
    // The nf-fa-columns glyph (U+F0DB) renders as a 1-cell glyph in
    // most nerd-font terminals, so the prefix consumes 2 display
    // cells: 1 for the glyph + 1 trailing space. The branch text
    // target shrinks by that amount so the total span width still
    // equals `branch_width` and downstream columns stay aligned.
    let layout_badge_width = if inputs.has_multi_pane_layout && inputs.nerd_fonts {
        2
    } else {
        0
    };
    if inputs.has_multi_pane_layout && inputs.nerd_fonts {
        spans.push(Span::styled("\u{f0db} ".to_string(), theme.dim_style()));
    }
    // Shared (tmux-backed) badge, immediately left of the branch glyph:
    // nf-cod-terminal_tmux when nerd fonts are on, hollow diamond otherwise
    // (the filled ◆ is the *detached* status glyph — same vocabulary).
    // Unlike the layout glyph this renders in BOTH font modes: shared-ness
    // matters on machines without nerd fonts too. Green while the tmux
    // session is alive (attached here or detached on the server); red when
    // the workspace is shared but no live session backs it — a "semi-failed"
    // state where the session has exited (or was never started), so a remote
    // peer browsing this host can't attach to it.
    let shared_badge_width = if inputs.shared { 2 } else { 0 };
    if inputs.shared {
        let badge = if inputs.nerd_fonts {
            "\u{ebc8} "
        } else {
            "◇ "
        };
        let badge_style = if inputs.shared_active {
            theme.status_style(Status::Complete)
        } else {
            theme.err_style()
        };
        spans.push(Span::styled(badge.to_string(), badge_style));
    }
    let branch_glyph = crate::ui::theme::branch_glyph(inputs.lifecycle, inputs.nerd_fonts);
    let branch_text = format!("{} {}", branch_glyph, inputs.branch);
    let branch_target = branch_width
        .saturating_sub(layout_badge_width + shared_badge_width)
        .max(1);
    let branch_padded = truncate_pad(&branch_text, branch_target);
    let branch_style = theme
        .lifecycle_style(inputs.lifecycle)
        .unwrap_or_else(|| theme.dim_style());
    spans.push(Span::styled(branch_padded, branch_style));

    // 6: procs
    let procs_cell = if inputs.procs > 0 {
        format!("● {}p", inputs.procs)
    } else {
        "  ·".to_string()
    };
    let procs_padded = truncate_pad(&procs_cell, PROCS_WIDTH);
    let procs_style = if inputs.procs > 0 {
        theme.status_style(Status::Thinking)
    } else {
        theme.dim_style()
    };
    spans.push(Span::styled(procs_padded, procs_style));

    // 7: diff
    match inputs.diff {
        Some(d) if d.added > 0 || d.removed > 0 => {
            let added_text = format!("+{}", d.added);
            let removed_text = format!("−{}", d.removed);
            let content_width = added_text.chars().count() + 1 + removed_text.chars().count();
            let pad = DIFF_WIDTH.saturating_sub(content_width);
            spans.push(Span::styled(added_text, theme.ok_style()));
            spans.push(Span::styled(" ".to_string(), theme.dim_style()));
            spans.push(Span::styled(removed_text, theme.err_style()));
            if pad > 0 {
                spans.push(Span::styled(" ".repeat(pad), theme.dim_style()));
            }
        }
        _ => {
            spans.push(Span::styled(" ".repeat(DIFF_WIDTH), theme.dim_style()));
        }
    }

    // 8: message (flex)
    let left_consumed = AGENT_WIDTH
        + GUTTER_WIDTH
        + ELBOW_WIDTH
        + GLYPH_WIDTH
        + name_width
        + branch_width
        + PROCS_WIDTH
        + DIFF_WIDTH;
    let right_consumed = AGE_WIDTH;
    let message_width = total_width
        .saturating_sub(left_consumed + right_consumed)
        .max(1);
    if let Some(col) = inputs.column.as_ref() {
        let prefix = if matches!(col.emphasis, ColumnEmphasis::Reported) {
            "▸ "
        } else {
            "└ "
        };
        let body_width = message_width.saturating_sub(prefix.chars().count());
        let body = truncate(&col.text, body_width);
        spans.push(Span::styled(
            prefix.to_string(),
            theme.status_style(inputs.status),
        ));
        let body_padded = right_pad(&body, body_width);
        let body_style = match col.emphasis {
            ColumnEmphasis::Dim => theme.dim_style(),
            ColumnEmphasis::Status => theme.status_style(inputs.status),
            ColumnEmphasis::Warn => theme.warn_style(),
            ColumnEmphasis::Reported => theme.status_style(inputs.status),
        };
        spans.push(Span::styled(body_padded, body_style));
    } else {
        let body = truncate_pad("—", message_width);
        spans.push(Span::styled(body, theme.dim_style()));
    }

    // 9: ago, right-aligned
    let ago = format_ago(inputs.ago_secs);
    let ago_padded = left_pad(&ago, AGE_WIDTH);
    spans.push(Span::styled(ago_padded, theme.dim_style()));

    Line::from(spans)
}

fn right_pad(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len >= target {
        s.to_string()
    } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(target - len));
        out
    }
}

fn left_pad(s: &str, target: usize) -> String {
    let len = s.chars().count();
    if len >= target {
        s.to_string()
    } else {
        let mut out = " ".repeat(target - len);
        out.push_str(s);
        out
    }
}

fn display_width(s: &str) -> usize {
    s.chars().count()
}

fn format_ago(secs: Option<u64>) -> String {
    match secs {
        None => "—".to_string(),
        Some(s) if s < 60 => format!("{s}s ago"),
        Some(s) if s < 3600 => format!("{}m ago", s / 60),
        Some(s) => format!("{}h ago", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::dashboard::status::Status;

    fn base() -> RowInputs {
        RowInputs {
            agent: AgentKind::Claude,
            status: Status::Question,
            name: "repo-overview".into(),
            branch: "bakedbean/repo-overview".into(),
            procs: 2,
            diff: Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            column: Some(RowColumn {
                text: "I have enough to give you a grounded tour.".into(),
                emphasis: ColumnEmphasis::Dim,
            }),
            ago_secs: Some(29),
            selected: false,
            yolo: false,
            setup_failed: false,
            shared: false,
            shared_active: false,
            has_multi_pane_layout: false,
            lifecycle: None,
            nerd_fonts: false,
            workspace_id: crate::data::store::WorkspaceId(0),
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn unselected_row_uses_thin_gutter_glyph() {
        let theme = Theme::wsx();
        let line = render(&base(), ColumnWidths::default(), 0, &theme, 120);
        let gutter = line.spans.get(1).expect("status gutter span present");
        assert_eq!(gutter.content.as_ref(), "▎");
    }

    #[test]
    fn selected_row_uses_thicker_gutter_glyph() {
        // The selection highlight is otherwise just a bg tint, which can
        // be subtle on dark terminals. A wider gutter glyph gives the
        // selected row a high-contrast leading edge independent of bg.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.selected = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let gutter = line.spans.get(1).expect("status gutter span present");
        assert_eq!(gutter.content.as_ref(), "▍");
        assert_eq!(
            gutter.style.fg,
            Some(theme.status_style(inputs.status).fg.unwrap()),
            "gutter keeps the status color even when selected"
        );
    }

    #[test]
    fn shared_badge_prefixes_branch_in_both_font_modes() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.shared = true;
        // Plain Unicode: hollow diamond, then the ⎇ branch glyph.
        let text = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        assert!(
            text.contains("◇ ⎇ bakedbean/repo-overview"),
            "shared badge must sit immediately left of the branch glyph: {text:?}"
        );
        // Nerd fonts: the tmux logo (nf-cod-terminal_tmux), then the branch glyph.
        inputs.nerd_fonts = true;
        let text = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        assert!(
            text.contains("\u{ebc8} \u{e0a0} bakedbean/repo-overview"),
            "nerd-font shared badge must be the tmux logo: {text:?}"
        );
    }

    #[test]
    fn shared_badge_is_green_when_active_and_red_when_dead() {
        let theme = Theme::wsx();
        // Both font modes: the badge glyph differs (tmux logo vs ◇) but the
        // liveness coloring must behave identically in each.
        for (nerd_fonts, badge_text) in [(false, "◇ "), (true, "\u{ebc8} ")] {
            let badge_style = |inputs: &RowInputs| {
                let line = render(inputs, ColumnWidths::default(), 0, &theme, 120);
                line.spans
                    .iter()
                    .find(|s| s.content.as_ref() == badge_text)
                    .unwrap_or_else(|| panic!("badge span present (nerd_fonts={nerd_fonts})"))
                    .style
            };
            let mut inputs = base();
            inputs.shared = true;
            inputs.nerd_fonts = nerd_fonts;
            // Shared but no live tmux session backs it — a "semi-failed" state
            // (the session exited or was never started, so a remote peer can't
            // attach): the error red, not idle gray.
            assert_eq!(
                badge_style(&inputs).fg,
                theme.err_style().fg,
                "dead shared badge must be red (nerd_fonts={nerd_fonts})"
            );
            // Live session (attached client or detached-alive): the complete
            // green — "the agent is alive in tmux right now".
            inputs.shared_active = true;
            assert_eq!(
                badge_style(&inputs).fg,
                theme.status_style(Status::Complete).fg,
                "active badge must use the complete green (nerd_fonts={nerd_fonts})"
            );
        }
    }

    #[test]
    fn unshared_row_has_no_shared_badge_and_widths_stay_aligned() {
        let theme = Theme::wsx();
        let unshared = line_text(&render(&base(), ColumnWidths::default(), 0, &theme, 120));
        assert!(
            !unshared.contains('◇') && !unshared.contains('\u{ebc8}'),
            "no badge on direct workspaces: {unshared:?}"
        );
        // The badge consumes 2 cells of the branch column, so both rows
        // must occupy the same display width and downstream columns
        // (procs/diff/age) must start at the same offset.
        let mut inputs = base();
        inputs.shared = true;
        let shared = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        // Compare CHAR positions, not byte offsets — ◇/⎇ are multibyte, so
        // `str::find` would report a shift even when columns are aligned.
        let procs_col = |s: &str| s.chars().position(|c| c == '●');
        assert_eq!(
            procs_col(&unshared),
            procs_col(&shared),
            "procs column must not shift when the shared badge renders:\n  {unshared:?}\n  {shared:?}"
        );
    }

    #[test]
    fn renders_design_columns_in_order() {
        let theme = Theme::wsx();
        let line = render(&base(), ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.starts_with("▎"), "agent bar first: {text:?}");
        assert!(text.contains("? "), "static glyph for non-live status");
        assert!(text.contains("repo-overview"), "name present");
        assert!(
            text.contains("⎇ bakedbean/repo-overview"),
            "branch with glyph"
        );
        assert!(text.contains("● 2p"), "procs cell");
        assert!(text.contains("+12 −3"), "diff cell");
        assert!(text.contains("└ I have enough"), "message prefix");
        assert!(text.trim_end().ends_with("29s ago"), "ago at end: {text:?}");
    }

    #[test]
    fn live_status_uses_spinner_frame() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.status = Status::Thinking;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⠋"), "spinner frame at tick 0: {text:?}");
        let line2 = render(&inputs, ColumnWidths::default(), 8, &theme, 120);
        let text2 = line_text(&line2);
        assert!(text2.contains("⠙"), "spinner advances by tick 8: {text2:?}");
    }

    #[test]
    fn missing_message_renders_em_dash() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.column = None;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("—"), "em-dash for missing message: {text:?}");
    }

    #[test]
    fn column_emphasis_maps_to_body_style() {
        let theme = Theme::wsx();
        // Helper that finds the body span following either prefix glyph.
        let body_after_prefix = |line: &Line<'_>| -> Style {
            let i = line
                .spans
                .iter()
                .position(|s| matches!(s.content.as_ref(), "└ " | "▸ "))
                .expect("prefix span present");
            line.spans[i + 1].style
        };

        // Warn emphasis → warn color.
        let mut inputs = base();
        inputs.status = Status::Stalled;
        inputs.column = Some(RowColumn {
            text: "stalled · 4m quiet".into(),
            emphasis: ColumnEmphasis::Warn,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(body_after_prefix(&line).fg, theme.warn_style().fg);

        // Status emphasis → the row's status color.
        inputs.status = Status::Question;
        inputs.column = Some(RowColumn {
            text: "AskUserQuestion".into(),
            emphasis: ColumnEmphasis::Status,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(
            body_after_prefix(&line).fg,
            theme.status_style(Status::Question).fg
        );

        // Dim emphasis → dim color.
        inputs.status = Status::Idle;
        inputs.column = Some(RowColumn {
            text: "backfill the migration".into(),
            emphasis: ColumnEmphasis::Dim,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(body_after_prefix(&line).fg, theme.dim_style().fg);

        // Reported emphasis → status color, ▸ prefix.
        inputs.status = Status::Stalled;
        inputs.column = Some(RowColumn {
            text: "need your call on auth".into(),
            emphasis: ColumnEmphasis::Reported,
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(
            body_after_prefix(&line).fg,
            theme.status_style(Status::Stalled).fg
        );
        let text = line_text(&line);
        assert!(text.contains("▸ "), "Reported uses ▸ prefix: {text:?}");
        assert!(
            !text.contains("└ "),
            "Reported does not use └ prefix: {text:?}"
        );
    }

    #[test]
    fn zero_procs_renders_faint_dot() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.procs = 0;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("  ·"), "faint dot for zero procs: {text:?}");
    }

    #[test]
    fn diff_cell_colors_additions_green_and_deletions_red() {
        let theme = Theme::wsx();
        let line = render(&base(), ColumnWidths::default(), 0, &theme, 120);
        let added_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "+12")
            .expect("added span present");
        let removed_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "−3")
            .expect("removed span present");
        assert_eq!(added_span.style.fg, Some(theme.ok));
        assert_eq!(removed_span.style.fg, Some(theme.err));
    }

    #[test]
    fn no_diff_leaves_column_blank() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.diff = None;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(!text.contains("+0 −0"), "no diff cell when None: {text:?}");
    }

    #[test]
    fn setup_failed_appends_badge() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.setup_failed = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⚙!"), "setup badge present: {text:?}");
    }

    #[test]
    fn nerd_fonts_swaps_branch_glyph() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("\u{e0a0}"),
            "nerd font branch glyph: {text:?}"
        );
    }

    #[test]
    fn merged_lifecycle_uses_merge_glyph_with_nerd_fonts() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.lifecycle = Some(BranchLifecycle::PrMerged);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("\u{f419}"), "git-merge glyph: {text:?}");
        assert!(
            !text.contains("\u{e0a0}"),
            "default branch glyph absent: {text:?}"
        );
    }

    #[test]
    fn closed_lifecycle_uses_closed_pr_glyph_with_nerd_fonts() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.lifecycle = Some(BranchLifecycle::PrClosed);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("\u{f4dc}"),
            "git-pull-request-closed glyph: {text:?}"
        );
    }

    #[test]
    fn unicode_mode_keeps_generic_glyph_for_merged() {
        // No good Unicode equivalent to a git-merge icon — color carries
        // the lifecycle signal in plain-Unicode mode.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = false;
        inputs.lifecycle = Some(BranchLifecycle::PrMerged);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⎇ "), "generic glyph retained: {text:?}");
    }

    #[test]
    fn open_lifecycle_uses_pr_glyph_with_nerd_fonts() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("\u{f407}"),
            "git-pull-request glyph: {text:?}"
        );
    }

    #[test]
    fn draft_lifecycle_uses_draft_pr_glyph_with_nerd_fonts() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.lifecycle = Some(BranchLifecycle::PrDraft);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("\u{f4dd}"),
            "git-pull-request-draft glyph: {text:?}"
        );
    }

    #[test]
    fn conflicted_lifecycle_reuses_open_pr_glyph() {
        // No dedicated octicon for a conflicted PR; the yellow warn color
        // already differentiates it from a clean open PR.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.lifecycle = Some(BranchLifecycle::PrConflicted);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("\u{f407}"), "open PR glyph reused: {text:?}");
    }

    #[test]
    fn no_pr_lifecycle_keeps_default_branch_glyph() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.lifecycle = Some(BranchLifecycle::NoPr);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("\u{e0a0}"),
            "default glyph for no PR: {text:?}"
        );
    }

    #[test]
    fn column_widths_clamp_outside_range() {
        let tight = ColumnWidths::clamped(2, 2);
        assert_eq!(tight.name, MIN_NAME_WIDTH);
        assert_eq!(tight.branch, MIN_BRANCH_WIDTH);
        let huge = ColumnWidths::clamped(1000, 1000);
        assert_eq!(huge.name, MAX_NAME_WIDTH);
        assert_eq!(huge.branch, MAX_BRANCH_WIDTH);
        let mid = ColumnWidths::clamped(30, 40);
        assert_eq!(mid.name, 30);
        assert_eq!(mid.branch, 40);
    }

    #[test]
    fn multi_pane_layout_appends_columns_glyph_when_nerd_fonts() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.has_multi_pane_layout = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("\u{f0db}"),
            "nf-fa-columns glyph present: {text:?}"
        );
    }

    #[test]
    fn multi_pane_layout_skipped_without_nerd_fonts() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = false;
        inputs.has_multi_pane_layout = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            !text.contains("\u{f0db}"),
            "columns glyph should not render without nerd fonts: {text:?}"
        );
    }

    #[test]
    fn layout_and_setup_failed_badges_both_render() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.has_multi_pane_layout = true;
        inputs.setup_failed = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⚙!"), "setup badge present: {text:?}");
        assert!(text.contains("\u{f0db}"), "layout badge present: {text:?}");
    }

    #[test]
    fn name_column_is_not_shrunk_by_layout_badge() {
        // The layout badge lives in the branch column now, so the name
        // column gets its full width even when the badge is showing.
        // This is the whole point of the move: on narrow displays the
        // name no longer has to give up cells (and then truncate to
        // `…`) just so the glyph can fit.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.has_multi_pane_layout = true;
        inputs.name = "exactly-24-characterz!!!".into(); // 24 chars = DEFAULT_NAME_WIDTH
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("exactly-24-characterz!!!"),
            "full name fits when badge is in branch col: {text:?}"
        );
        assert!(
            !text.contains("exactly-24-characterz!!…"),
            "name should not be truncated to make room for badge: {text:?}"
        );
    }

    #[test]
    fn layout_badge_sits_at_start_of_branch_column_before_branch_glyph() {
        // Regression guard for the "badge clipped on narrow displays"
        // bug: the layout glyph used to sit at the far end of the name
        // column where it could be clipped by the following column. It
        // now lives at the start of the branch column, immediately
        // before the branch glyph, where it is never truncated.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.has_multi_pane_layout = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(
            text.contains("\u{f0db} \u{e0a0}"),
            "columns glyph should sit immediately before branch glyph, \
             separated only by one space: {text:?}"
        );
    }

    #[test]
    fn branch_text_shrinks_to_accommodate_layout_badge() {
        // The badge takes cells from the branch column's text target,
        // so a long branch name shows fewer characters on rows that
        // have a saved layout. The total branch-column width is
        // unchanged, so downstream columns stay aligned.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.nerd_fonts = true;
        inputs.branch = "bakedbean/a-fairly-long-branch-name-here".into();
        inputs.has_multi_pane_layout = true;
        let with_badge_text = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        inputs.has_multi_pane_layout = false;
        let without_badge_text =
            line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        // Without the badge, more branch characters fit before the
        // truncation ellipsis — pick a substring that lands inside the
        // unbadged truncation window but outside the badged one.
        assert!(
            without_badge_text.contains("a-fairly-long-b"),
            "without badge, branch shows further into the name: {without_badge_text:?}"
        );
        assert!(
            !with_badge_text.contains("a-fairly-long-b"),
            "with badge, branch truncates earlier (badge took 3 cells): {with_badge_text:?}"
        );
    }

    #[test]
    fn wider_branch_pushes_other_columns_right() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.branch = "very-long-branch-name-that-takes-space".into();
        let narrow = render(&inputs, ColumnWidths::clamped(24, 16), 0, &theme, 160);
        let wide = render(&inputs, ColumnWidths::clamped(24, 50), 0, &theme, 160);
        // Both end with "29s ago" (right-aligned at total_width).
        let narrow_text = line_text(&narrow);
        let wide_text = line_text(&wide);
        assert!(narrow_text.trim_end().ends_with("29s ago"));
        assert!(wide_text.trim_end().ends_with("29s ago"));
        // The wider branch eats more space, so the message column is
        // narrower in the wide case → the message ends with `…`
        // earlier OR the diff cell content stays the same.
        // The simplest invariant: the branch substring fits more
        // characters in the wide case.
        assert!(
            wide_text.contains("very-long-branch-name-that-take"),
            "wide branch shows more of the name: {wide_text:?}"
        );
        assert!(
            !narrow_text.contains("very-long-branch-name-that-take"),
            "narrow branch truncates: {narrow_text:?}"
        );
    }

    #[test]
    fn agent_bar_is_leftmost_span_with_agent_color() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Pi;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let first = line.spans.first().expect("agent bar present");
        assert_eq!(first.content.as_ref(), "▎");
        assert_eq!(first.style.fg, theme.agent_style(AgentKind::Pi).fg);
    }

    #[test]
    fn agent_bar_precedes_status_gutter_as_two_tone_edge() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Codex; // blue
        inputs.status = Status::Complete; // green gutter — distinct from blue
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(line.spans[0].content.as_ref(), "▎", "agent bar first");
        assert_eq!(line.spans[1].content.as_ref(), "▎", "status gutter second");
        assert_eq!(
            line.spans[0].style.fg,
            theme.agent_style(AgentKind::Codex).fg
        );
        assert_eq!(
            line.spans[1].style.fg,
            theme.status_style(Status::Complete).fg
        );
        assert_ne!(line.spans[0].style.fg, line.spans[1].style.fg);
    }

    #[test]
    fn agent_bar_keeps_color_when_selected() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Hermes;
        inputs.selected = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(line.spans[0].content.as_ref(), "▎");
        assert_eq!(
            line.spans[0].style.fg,
            theme.agent_style(AgentKind::Hermes).fg
        );
        assert_eq!(
            line.spans[1].content.as_ref(),
            "▍",
            "status gutter still thickens on selection"
        );
    }

    #[test]
    fn ago_stays_right_aligned_after_agent_column() {
        let theme = Theme::wsx();
        let line = render(&base(), ColumnWidths::default(), 0, &theme, 120);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(
            text.trim_end().ends_with("29s ago"),
            "age column stays right-aligned: {text:?}"
        );
    }
}
