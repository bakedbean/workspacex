//! Shared column composer for V5 workspace rows. Returns a
//! `ratatui::text::Line` so view modules can drop it straight into a
//! `ListItem`.
//!
//! Columns (left → right):
//!   1-4ch ▎ agent strip (one bar per live agent, primary rightmost;
//!         derived per frame, not user-configurable — see `ColumnWidths`)
//!   1ch  ▎ gutter (status color)
//!   3ch  ├  elbow (faint, centered)
//!   2ch  status glyph or spinner frame
//!   28ch ⎇ branch (left-aligned, ellipsized)
//!   16ch ⏺ #N pr-lifecycle chip (blank when no PR)
//!   6ch  ● Np procs (or faint dot when zero)
//!   12ch +N −N diff
//!   flex └ message (or em-dash)
//!   10ch right-aligned Ns ago

use crate::git::DiffStats;
use crate::git::forge::BranchLifecycle;
use crate::pty::session::AgentKind;
use crate::ui::dashboard::column_content::{ColumnBody, ColumnEmphasis, RecapSegment, RowColumn};
use crate::ui::dashboard::spinner;
use crate::ui::dashboard::status::Status;
use crate::ui::text::{truncate, truncate_pad, truncate_words};
use crate::ui::theme::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub const DEFAULT_BRANCH_WIDTH: usize = 28;
pub const DEFAULT_PR_WIDTH: usize = 16;
pub const MIN_BRANCH_WIDTH: usize = 10;
pub const MIN_PR_WIDTH: usize = 8;
pub const MAX_BRANCH_WIDTH: usize = 80;
pub const MAX_PR_WIDTH: usize = 24;
const PROCS_WIDTH: usize = 6;
const DIFF_WIDTH: usize = 12;
const AGE_WIDTH: usize = 10;
const GUTTER_WIDTH: usize = 1;
const ELBOW_WIDTH: usize = 3;
const GLYPH_WIDTH: usize = 2;
pub const DEFAULT_AGENT_WIDTH: usize = 1;
/// Cap on the agent strip. Five live agents is one keystroke away — the
/// agents panel's `a` key adds all four kinds at once — so the strip must
/// degrade rather than grow without bound.
pub const MAX_AGENT_WIDTH: usize = 4;

/// Column widths. `branch`/`pr` are user-resizable and clamped to safe
/// ranges by `ColumnWidths::clamped` (called from the config read path) so
/// the renderer never has to defend itself against pathological inputs;
/// `agent` is derived per frame instead — see its field doc below.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnWidths {
    pub branch: usize,
    pub pr: usize,
    /// Derived per frame from the live-agent count across visible rows —
    /// NOT user-configurable and NOT read from settings. Set via
    /// `with_agent` by the view dispatchers in `dashboard::mod`.
    pub agent: usize,
}

impl ColumnWidths {
    pub fn clamped(branch: usize, pr: usize) -> Self {
        Self {
            branch: branch.clamp(MIN_BRANCH_WIDTH, MAX_BRANCH_WIDTH),
            pr: pr.clamp(MIN_PR_WIDTH, MAX_PR_WIDTH),
            agent: DEFAULT_AGENT_WIDTH,
        }
    }

    pub fn with_agent(self, agent: usize) -> Self {
        Self {
            agent: agent.clamp(DEFAULT_AGENT_WIDTH, MAX_AGENT_WIDTH),
            ..self
        }
    }
}

impl Default for ColumnWidths {
    fn default() -> Self {
        Self {
            branch: DEFAULT_BRANCH_WIDTH,
            pr: DEFAULT_PR_WIDTH,
            agent: DEFAULT_AGENT_WIDTH,
        }
    }
}

/// A workspace lifecycle badge, rendered immediately after the branch name.
/// Distinct from the agent-status glyph in column 3: that one tracks whether
/// the *agent* is live, this one whether the *workspace* is ready. Both can
/// animate at once — you can attach to a workspace while its setup runs —
/// and conflating them would lose that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleBadge {
    /// Create in flight: fetching, checking out, or running setup.
    Provisioning,
    /// Archive in flight: script, worktree removal, branch delete.
    Archiving,
    SetupFailed,
    SetupCancelled,
    /// The worktree was never created (a failed fetch or checkout).
    NoWorktree,
}

impl LifecycleBadge {
    /// Rendered text, including the leading space that separates it from the
    /// branch name. `tick` drives the spinner on the in-flight variants, and
    /// is `u32` to match `spinner::frame` (`src/ui/dashboard/spinner.rs:11`).
    pub fn glyph(self, tick: u32) -> String {
        match self {
            LifecycleBadge::Provisioning => format!(" {}⚙", spinner::frame(tick)),
            LifecycleBadge::Archiving => format!(" {}⌫", spinner::frame(tick)),
            LifecycleBadge::SetupFailed => " ⚙!".to_string(),
            LifecycleBadge::SetupCancelled => " ⚙?".to_string(),
            LifecycleBadge::NoWorktree => " ✗".to_string(),
        }
    }

    /// Display columns this badge consumes. Every variant is a space plus
    /// two cells, except `NoWorktree`, which is a space plus one.
    pub fn width(self) -> usize {
        match self {
            LifecycleBadge::NoWorktree => 2,
            _ => 3,
        }
    }
}

/// Inputs the renderer needs about one workspace, gathered by the caller
/// from `app.rs` state.
#[derive(Debug, Clone)]
pub struct RowInputs {
    pub agent: AgentKind,
    /// Live non-primary agents in this workspace, in creation order. The
    /// primary is `agent` and is rendered unconditionally; peers appear
    /// only while their session is running, so a finished reviewer drops
    /// out and the strip narrows back on its own.
    pub peers: Vec<AgentKind>,
    pub status: Status,
    pub branch: String,
    pub pr_number: Option<u32>,
    pub procs: u32,
    pub diff: Option<DiffStats>,
    pub column: Option<RowColumn>,
    pub ago_secs: Option<u64>,
    pub selected: bool,
    pub yolo: bool,
    /// Workspace lifecycle badge, rendered after the branch name. `None` for
    /// a healthy, idle workspace.
    pub badge: Option<LifecycleBadge>,
    /// A peer message is queued for an agent here that wsx has stopped trying
    /// to inject. Renders a `✉!` badge next to the branch. Orthogonal to
    /// `badge`: that one tracks the workspace's own lifecycle, this one the
    /// mail queue, and both can be showing at once.
    pub undelivered_mail: bool,
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

/// The leftmost column: one bar per live agent, right-aligned so the
/// primary stays adjacent to the status gutter and a single-agent row
/// looks exactly as it did before the strip existed. Always returns
/// exactly `widths.agent` chars — the whole row's column alignment
/// depends on it.
pub fn agent_strip_spans(
    inputs: &RowInputs,
    widths: ColumnWidths,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let cells = widths.agent.max(1);
    let total = inputs.peers.len() + 1;
    if total > cells {
        // Overflow: a `+` stands in for the peers that don't fit, then the
        // NEWEST peers, then the primary — the oldest peers are what drop
        // out. With only one cell there's no room for the marker, so the
        // primary alone is the honest render.
        let peer_cells = cells.saturating_sub(2);
        if cells >= 2 {
            spans.push(Span::styled("+".to_string(), theme.dim_style()));
        }
        for kind in &inputs.peers[inputs.peers.len() - peer_cells..] {
            spans.push(Span::styled("▎".to_string(), theme.agent_style(*kind)));
        }
    } else {
        let pad = cells - total;
        if pad > 0 {
            spans.push(Span::raw(" ".repeat(pad)));
        }
        for kind in &inputs.peers {
            spans.push(Span::styled("▎".to_string(), theme.agent_style(*kind)));
        }
    }
    spans.push(Span::styled(
        "▎".to_string(),
        theme.agent_style(inputs.agent),
    ));
    spans
}

pub fn render(
    inputs: &RowInputs,
    widths: ColumnWidths,
    tick: u32,
    theme: &Theme,
    total_width: usize,
) -> Line<'static> {
    let branch_width = widths.branch;
    let pr_width = widths.pr;
    let mut spans: Vec<Span<'static>> = Vec::new();

    // 0: agent identity strip — one fixed-per-kind colored bar per live
    // agent, primary rightmost. Sits left of the status gutter so the row
    // shows a two-tone left edge: outer = agents, inner = status. Plain
    // Unicode, no nerd-font gating (same glyph as the gutter).
    spans.extend(agent_strip_spans(inputs, widths, theme));

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

    // 4: branch — the row's identity column (the workspace name never
    // diverged from the branch in practice, so the branch alone carries
    // identity). The NAME is bold like the name column it replaced, and
    // warn-colored when YOLO; the leading branch GLYPH is colored by PR
    // lifecycle instead (dim when unknown / no PR), matching the chip column
    // that follows and the shared-workspace picker, so the glyph's shape and
    // its color tell the same story.
    // Optionally prefixed by the multi-pane-layout glyph: the
    // nf-fa-columns glyph (U+F0DB) renders as a 1-cell glyph in
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
    // with nerd fonts, nf-md-check_network while the session is alive and
    // nf-md-close_network when it isn't; hollow diamond otherwise.
    // Unlike the layout glyph this renders in BOTH font modes: shared-ness
    // matters on machines without nerd fonts too. Green while the tmux
    // session is alive (attached here or detached on the server); red when
    // the workspace is shared but no live session backs it — a "semi-failed"
    // state where the session has exited (or was never started), so a remote
    // peer browsing this host can't attach to it.
    let shared_badge_width = if inputs.shared { 2 } else { 0 };
    if inputs.shared {
        let badge = if inputs.nerd_fonts {
            if inputs.shared_active {
                "\u{f0c53} "
            } else {
                "\u{f015b} "
            }
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
    // The setup-failed badge sits IMMEDIATELY after the visible branch
    // characters (then trailing padding fills the rest of `branch_width`)
    // so it stays attached to the branch even when truncated to `…`.
    let setup_badge_width = inputs.badge.map(|b| b.width()).unwrap_or(0);
    let mail_badge_width = if inputs.undelivered_mail { 3 } else { 0 };
    let branch_glyph = crate::ui::theme::branch_glyph(inputs.lifecycle, inputs.nerd_fonts);
    let branch_text = format!("{} {}", branch_glyph, inputs.branch);
    let branch_target = branch_width
        .saturating_sub(
            layout_badge_width + shared_badge_width + setup_badge_width + mail_badge_width,
        )
        .max(1);
    let branch_truncated = truncate(&branch_text, branch_target);
    let branch_visible_width = branch_truncated.chars().count();
    let mut name_style = Style::default().add_modifier(Modifier::BOLD);
    if inputs.yolo {
        name_style = name_style.fg(theme.warn);
    }
    let glyph_style = theme
        .lifecycle_style(inputs.lifecycle)
        .unwrap_or_else(|| theme.dim_style())
        .add_modifier(Modifier::BOLD);
    // Split the ALREADY-truncated text rather than truncating the glyph and
    // the name separately, so the column's char budget is accounted for in
    // exactly one place and `branch_visible_width` above stays authoritative.
    // The `<glyph> ` prefix is two chars; in a column too narrow to hold even
    // that, everything left goes to the glyph span. `split_off` hands the
    // glyph span the existing buffer and allocates only the name, and its
    // char-boundary precondition is met by construction — the index comes
    // from `char_indices`. Bind that index before splitting: matching on the
    // iterator directly would hold a borrow across the mutation.
    let mut glyph_cell = branch_truncated;
    let name_split = glyph_cell.char_indices().nth(2).map(|(i, _)| i);
    let name_cell = name_split.map(|i| glyph_cell.split_off(i));
    spans.push(Span::styled(glyph_cell, glyph_style));
    if let Some(name_cell) = name_cell.filter(|n| !n.is_empty()) {
        spans.push(Span::styled(name_cell, name_style));
    }
    if let Some(b) = inputs.badge {
        let style = match b {
            LifecycleBadge::Provisioning | LifecycleBadge::Archiving => theme.dim_style(),
            _ => theme.err_style(),
        };
        spans.push(Span::styled(b.glyph(tick), style));
    }
    if inputs.undelivered_mail {
        spans.push(Span::styled(" ✉!".to_string(), theme.err_style()));
    }
    let consumed = layout_badge_width
        + shared_badge_width
        + branch_visible_width
        + setup_badge_width
        + mail_badge_width;
    if consumed < branch_width {
        spans.push(Span::raw(" ".repeat(branch_width - consumed)));
    }

    // 5: PR chip — the same glyph/label/color pairing as the detail-bar
    // chip (`⏺ #123 open`) so the row and the bar can't drift. Blank when
    // the branch has no PR or the lifecycle hasn't been fetched yet.
    match pr_chip_text(inputs) {
        Some(chip_text) => {
            let chip_style = theme
                .lifecycle_style(inputs.lifecycle)
                .unwrap_or_else(|| theme.dim_style());
            spans.push(Span::styled(truncate_pad(&chip_text, pr_width), chip_style));
        }
        None => spans.push(Span::raw(" ".repeat(pr_width))),
    }

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
    let left_consumed = widths.agent
        + GUTTER_WIDTH
        + ELBOW_WIDTH
        + GLYPH_WIDTH
        + branch_width
        + pr_width
        + PROCS_WIDTH
        + DIFF_WIDTH;
    let right_consumed = AGE_WIDTH;
    let message_width = total_width
        .saturating_sub(left_consumed + right_consumed)
        .max(1);
    if let Some(col) = inputs.column.as_ref() {
        let prefix = if col.reported { "▸ " } else { "└ " };
        let body_width = message_width.saturating_sub(prefix.chars().count());
        spans.push(Span::styled(
            prefix.to_string(),
            theme.status_style(inputs.status),
        ));
        let token = truncate(&col.token, body_width);
        let token_style = if inputs.status == Status::Stalled && !col.reported {
            theme.warn_style()
        } else {
            theme.status_style(inputs.status)
        };
        let mut used = token.chars().count();
        spans.push(Span::styled(token, token_style));
        let avail = body_width.saturating_sub(used);
        let (rest, rest_style) = match &col.body {
            // One grey for every recap. Fading a stale one only made the
            // text unreadable without making the staleness legible; the PM
            // pane carries that signal as an explicit `recap stale` fact.
            ColumnBody::Recap { segments } => (fit_segments(segments, avail), theme.dim_style()),
            ColumnBody::Fallback { text, emphasis } => {
                let sep_len = SEG_SEP.chars().count();
                let fitted = if avail > sep_len + 1 {
                    format!("{SEG_SEP}{}", truncate_words(text, avail - sep_len))
                } else {
                    String::new()
                };
                let style = match emphasis {
                    ColumnEmphasis::Dim => theme.dim_style(),
                    ColumnEmphasis::Status => theme.status_style(inputs.status),
                    ColumnEmphasis::Warn => theme.warn_style(),
                };
                (fitted, style)
            }
            ColumnBody::Empty => (String::new(), theme.dim_style()),
        };
        used += rest.chars().count();
        if !rest.is_empty() {
            spans.push(Span::styled(rest, rest_style));
        }
        spans.push(Span::styled(
            " ".repeat(body_width.saturating_sub(used)),
            theme.dim_style(),
        ));
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

/// The PR chip's cell text (`⏺ #123 open`), unpadded, or `None` when the
/// chip cell renders blank. Shared by `render` and `pr_chip_hit_span` so
/// the painted chip and its click target can't drift.
fn pr_chip_text(inputs: &RowInputs) -> Option<String> {
    let (glyph, label) = inputs
        .lifecycle
        .map(crate::ui::theme::lifecycle_chip)
        .filter(|(glyph, _)| !glyph.is_empty())?;
    Some(match inputs.pr_number {
        Some(n) => format!("{glyph} #{n} {label}"),
        None => format!("{glyph} {label}"),
    })
}

/// Char-offset and char-width of the clickable PR chip within a workspace
/// row, or `None` when the chip cell is blank. Offsets are relative to the
/// row's left edge; the caller adds the list area origin (and the row's y)
/// to build a screen rect. Padding to the chip cell's right is excluded so
/// clicks on blank space don't open a browser.
pub fn pr_chip_hit_span(inputs: &RowInputs, widths: ColumnWidths) -> Option<(u16, u16)> {
    let text = pr_chip_text(inputs)?;
    let x = widths.agent + GUTTER_WIDTH + ELBOW_WIDTH + GLYPH_WIDTH + widths.branch;
    let width = truncate(&text, widths.pr).chars().count();
    Some((x as u16, width as u16))
}

const SEG_SEP: &str = " · ";

/// Base width a fallback (full-field) segment is held to while later
/// segments are being placed. After every included segment has its base
/// width, clipped fallback segments expand left-to-right into whatever
/// column width is left — so a wide dashboard shows more of a full field
/// instead of stranding blank space after a fixed clip.
const FALLBACK_SEGMENT_FLOOR: usize = 32;

/// Width-fit the recap segments into `avail` chars.
///
/// Pass 1 — inclusion at base widths: authored short forms count at full
/// length (they render verbatim), fallback full fields at
/// `FALLBACK_SEGMENT_FLOOR`. When there's meaningful room
/// (`avail > sep_len + 1`) the first segment (goal) is included, truncated
/// to what remains — below that nothing is emitted. Later segments (state,
/// next) are included only when their base width fits whole; one that
/// doesn't fit is dropped along with everything after it.
///
/// Pass 2 — expansion: leftover width is granted to clipped fallback
/// segments left-to-right. Each segment renders whole when its width
/// allows, word-boundary truncated otherwise.
fn fit_segments(segments: &[RecapSegment], avail: usize) -> String {
    let sep_len = SEG_SEP.chars().count();

    let mut widths: Vec<usize> = Vec::with_capacity(segments.len());
    let mut used = 0usize;
    for (i, seg) in segments.iter().enumerate() {
        let len = seg.text.chars().count();
        let base = if seg.authored {
            len
        } else {
            len.min(FALLBACK_SEGMENT_FLOOR)
        };
        if i == 0 {
            if avail <= sep_len + 1 {
                break;
            }
            let w = base.min(avail - sep_len);
            used = sep_len + w;
            widths.push(w);
        } else if used + sep_len + base <= avail {
            used += sep_len + base;
            widths.push(base);
        } else {
            break;
        }
    }

    let mut leftover = avail.saturating_sub(used);
    for (w, seg) in widths.iter_mut().zip(segments) {
        if leftover == 0 {
            break;
        }
        let len = seg.text.chars().count();
        if !seg.authored && len > *w {
            let grow = (len - *w).min(leftover);
            *w += grow;
            leftover -= grow;
        }
    }

    // Pass 3 — render at the allocated widths, tracking the ACTUAL rendered
    // length: `truncate_words` keeps whole words, so a truncated segment can
    // come out well short of its target. That shortfall accrues as `bonus`
    // width, granted forward to later clipped fallback segments — otherwise
    // it would strand as blank space (allocation-only accounting bug).
    let mut out = String::new();
    let mut bonus = 0usize;
    for (w, seg) in widths.iter().zip(segments) {
        let len = seg.text.chars().count();
        let mut target = *w;
        if !seg.authored && len > target && bonus > 0 {
            let grow = (len - target).min(bonus);
            target += grow;
            bonus -= grow;
        }
        let rendered = if len <= target {
            seg.text.clone()
        } else {
            truncate_words(&seg.text, target)
        };
        bonus += target - rendered.chars().count();
        out.push_str(SEG_SEP);
        out.push_str(&rendered);
    }
    // The accrued shortfall can even re-admit segments pass 1 dropped: same
    // inclusion rule (whole base width fits), against bonus width only.
    for seg in segments.iter().skip(widths.len()) {
        let len = seg.text.chars().count();
        let base = if seg.authored {
            len
        } else {
            len.min(FALLBACK_SEGMENT_FLOOR)
        };
        if bonus < sep_len + base {
            break;
        }
        bonus -= sep_len + base;
        let mut target = base;
        if !seg.authored && len > base {
            let grow = (len - base).min(bonus);
            target += grow;
            bonus -= grow;
        }
        let rendered = if len <= target {
            seg.text.clone()
        } else {
            truncate_words(&seg.text, target)
        };
        bonus += target - rendered.chars().count();
        out.push_str(SEG_SEP);
        out.push_str(&rendered);
    }
    out
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
            peers: Vec::new(),
            status: Status::Question,
            branch: "bakedbean/repo-overview".into(),
            pr_number: None,
            procs: 2,
            diff: Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            column: Some(RowColumn {
                token: "asking".to_string(),
                reported: false,
                body: ColumnBody::Fallback {
                    text: "I have enough to give you a grounded tour.".into(),
                    emphasis: ColumnEmphasis::Dim,
                },
            }),
            ago_secs: Some(29),
            selected: false,
            yolo: false,
            badge: None,
            undelivered_mail: false,
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
        // Nerd fonts, dead session: the network-close icon
        // (nf-md-close_network), then the branch glyph.
        inputs.nerd_fonts = true;
        let text = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        assert!(
            text.contains("\u{f015b} \u{e0a0} bakedbean/repo-overview"),
            "nerd-font dead shared badge must be the network-close icon: {text:?}"
        );
        // Nerd fonts, live session: the network-check icon
        // (nf-md-check_network) instead.
        inputs.shared_active = true;
        let text = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        assert!(
            text.contains("\u{f0c53} \u{e0a0} bakedbean/repo-overview"),
            "nerd-font live shared badge must be the network-check icon: {text:?}"
        );
    }

    #[test]
    fn shared_badge_is_green_when_active_and_red_when_dead() {
        let theme = Theme::wsx();
        // Both font modes must color liveness identically. Nerd fonts also
        // switch the glyph with liveness (network-close when dead,
        // network-check when live); plain Unicode keeps ◇ for both.
        for (nerd_fonts, dead_badge, live_badge) in
            [(false, "◇ ", "◇ "), (true, "\u{f015b} ", "\u{f0c53} ")]
        {
            let badge_style = |inputs: &RowInputs, badge_text: &str| {
                let line = render(inputs, ColumnWidths::default(), 0, &theme, 120);
                line.spans
                    .iter()
                    .find(|s| s.content.as_ref() == badge_text)
                    .unwrap_or_else(|| {
                        panic!("badge span {badge_text:?} present (nerd_fonts={nerd_fonts})")
                    })
                    .style
            };
            let mut inputs = base();
            inputs.shared = true;
            inputs.nerd_fonts = nerd_fonts;
            // Shared but no live tmux session backs it — a "semi-failed" state
            // (the session exited or was never started, so a remote peer can't
            // attach): the error red, not idle gray.
            assert_eq!(
                badge_style(&inputs, dead_badge).fg,
                theme.err_style().fg,
                "dead shared badge must be red (nerd_fonts={nerd_fonts})"
            );
            // Live session (attached client or detached-alive): the complete
            // green — "the agent is alive in tmux right now".
            inputs.shared_active = true;
            assert_eq!(
                badge_style(&inputs, live_badge).fg,
                theme.status_style(Status::Complete).fg,
                "active badge must use the complete green (nerd_fonts={nerd_fonts})"
            );
        }
    }

    #[test]
    fn unshared_row_has_no_shared_badge_and_widths_stay_aligned() {
        let theme = Theme::wsx();
        // Compare CHAR positions, not byte offsets — ◇/⎇ are multibyte, so
        // `str::find` would report a shift even when columns are aligned.
        let procs_col = |s: &str| s.chars().position(|c| c == '●');
        // The badge consumes 2 cells of the branch column, so a shared row
        // must occupy the same display width as an unshared one and the
        // downstream columns (procs/diff/age) must start at the same offset.
        // Checked per font mode and, under nerd fonts, per liveness state,
        // since each combination renders a different badge glyph (◇ vs
        // network-close vs network-check).
        for nerd_fonts in [false, true] {
            let mut inputs = base();
            inputs.nerd_fonts = nerd_fonts;
            let unshared = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
            assert!(
                !unshared.contains('◇')
                    && !unshared.contains('\u{f0c53}')
                    && !unshared.contains('\u{f015b}'),
                "no badge on direct workspaces (nerd_fonts={nerd_fonts}): {unshared:?}"
            );
            for shared_active in [false, true] {
                let mut inputs = inputs.clone();
                inputs.shared = true;
                inputs.shared_active = shared_active;
                let shared = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
                assert_eq!(
                    procs_col(&unshared),
                    procs_col(&shared),
                    "procs column must not shift when the shared badge renders \
                     (nerd_fonts={nerd_fonts}, shared_active={shared_active}):\n  \
                     {unshared:?}\n  {shared:?}"
                );
            }
        }
    }

    #[test]
    fn renders_design_columns_in_order() {
        let theme = Theme::wsx();
        let line = render(&base(), ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.starts_with("▎"), "agent bar first: {text:?}");
        assert!(text.contains("? "), "static glyph for non-live status");
        assert!(
            text.contains("⎇ bakedbean/repo-overview"),
            "branch with glyph"
        );
        assert!(text.contains("● 2p"), "procs cell");
        assert!(text.contains("+12 −3"), "diff cell");
        assert!(
            text.contains("└ asking · I have enough"),
            "message prefix with token: {text:?}"
        );
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
        // One tick per frame since the render tick moved to `app::TICK`.
        let line2 = render(&inputs, ColumnWidths::default(), 1, &theme, 120);
        let text2 = line_text(&line2);
        assert!(text2.contains("⠙"), "spinner advances by tick 1: {text2:?}");
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
    fn column_body_emphasis_maps_to_style() {
        let theme = Theme::wsx();
        // Helper that finds the fallback-body span by its (unique) text
        // content — the body now trails the token span, prefixed by
        // `SEG_SEP`, so it is no longer at a fixed span index.
        let find_body = |line: &Line<'_>, needle: &str| -> Style {
            line.spans
                .iter()
                .find(|s| s.content.contains(needle))
                .unwrap_or_else(|| panic!("body span containing {needle:?} present"))
                .style
        };

        // Warn emphasis → warn color.
        let mut inputs = base();
        inputs.status = Status::Stalled;
        inputs.column = Some(RowColumn {
            token: "stalled".to_string(),
            reported: false,
            body: ColumnBody::Fallback {
                text: "4m quiet".into(),
                emphasis: ColumnEmphasis::Warn,
            },
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(find_body(&line, "4m quiet").fg, theme.warn_style().fg);

        // Status emphasis → the row's status color.
        inputs.status = Status::Question;
        inputs.column = Some(RowColumn {
            token: "asking".to_string(),
            reported: false,
            body: ColumnBody::Fallback {
                text: "AskUserQuestion".into(),
                emphasis: ColumnEmphasis::Status,
            },
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(
            find_body(&line, "AskUserQuestion").fg,
            theme.status_style(Status::Question).fg
        );

        // Dim emphasis → dim color.
        inputs.status = Status::Idle;
        inputs.column = Some(RowColumn {
            token: "idle".to_string(),
            reported: false,
            body: ColumnBody::Fallback {
                text: "backfill the migration".into(),
                emphasis: ColumnEmphasis::Dim,
            },
        });
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        assert_eq!(
            find_body(&line, "backfill the migration").fg,
            theme.dim_style().fg
        );
    }

    fn au(s: &str) -> RecapSegment {
        RecapSegment {
            text: s.to_string(),
            authored: true,
        }
    }

    fn fb(s: &str) -> RecapSegment {
        RecapSegment {
            text: s.to_string(),
            authored: false,
        }
    }

    #[test]
    fn fit_segments_drops_then_truncates() {
        let segs = vec![au("goal seg"), au("state"), au("next")];
        // everything fits: " · goal seg · state · next" = 26 chars
        assert_eq!(fit_segments(&segs, 26), " · goal seg · state · next");
        // next no longer fits whole → dropped
        assert_eq!(fit_segments(&segs, 25), " · goal seg · state");
        // state no longer fits whole → dropped
        assert_eq!(fit_segments(&segs, 18), " · goal seg");
        // goal itself doesn't fit → word-boundary truncation, ellipsis attached
        assert_eq!(fit_segments(&segs, 9), " · goal…");
        // no room for anything meaningful
        assert_eq!(fit_segments(&segs, 4), "");
    }

    #[test]
    fn fallback_segment_expands_into_free_width() {
        // 38-char fallback field: past the 32-char floor, but a wide column
        // has room — it renders whole instead of clipping at the floor.
        let text = "Audit V2 invoices for amount drift bug";
        let segs = vec![fb(text)];
        assert_eq!(fit_segments(&segs, 60), format!(" · {text}"));
        // At exactly floor room (3 + 32) it still clips at a word boundary.
        assert_eq!(fit_segments(&segs, 35), " · Audit V2 invoices for amount…");
    }

    #[test]
    fn word_boundary_shortfall_flows_to_later_segments() {
        // The goal's word-boundary truncation renders short of its allocated
        // 32 (here 10 chars short); that gap must not strand as blank space —
        // the next clipped fallback segment expands into it.
        let goal = "aaaa bbbb cccccccccccccccccccccccc"; // 34 chars, awkward boundary
        let state = "one two three four five six seven eight nine"; // 44 chars
        let out = fit_segments(&[fb(goal), fb(state)], 70);
        // Pass 1: both at floor 32 (3+32+3+32 = 70, leftover 0). Goal renders
        // "aaaa bbbb…" (10) → 22 chars of bonus; state grows 32 → 44 → whole.
        assert_eq!(out, format!(" · aaaa bbbb… · {state}"));
    }

    #[test]
    fn word_boundary_shortfall_readmits_dropped_segment() {
        // The goal alone consumes the whole allocation, dropping state in
        // pass 1 — but its actual render is 10 chars short of target, which
        // is room enough to admit the small authored state after all.
        let goal = "aaaa bbbb cccccccccccc"; // 22 chars
        let out = fit_segments(&[au(goal), au("st")], 23);
        assert_eq!(out, " · aaaa bbbb… · st");
    }

    #[test]
    fn later_segments_keep_their_floor_before_goal_expands() {
        // A long fallback goal must not starve the authored state segment:
        // state is included at its full width first, then the goal expands
        // into whatever is left (here 5 chars past its 32-char floor).
        let goal = "one two three four five six seven eight nine ten eleven twelve";
        let segs = vec![fb(goal), au("3/12 done")];
        assert_eq!(
            fit_segments(&segs, 52),
            " · one two three four five six seven… · 3/12 done"
        );
    }

    #[test]
    fn reported_token_gets_pointer_prefix() {
        let mut inputs = base();
        inputs.column = Some(RowColumn {
            token: "working".to_string(),
            reported: true,
            body: ColumnBody::Empty,
        });
        let theme = Theme::wsx();
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 160);
        let text = line_text(&line);
        assert!(text.contains("▸ working"), "got: {text}");
        assert!(
            !text.contains("└ "),
            "reported row must not use └ : {text:?}"
        );
    }

    #[test]
    fn recap_segments_render_after_token() {
        let mut inputs = base();
        inputs.column = Some(RowColumn {
            token: "working".to_string(),
            reported: false,
            body: ColumnBody::Recap {
                segments: vec![au("Audit V2 #2835"), au("3/12 done")],
            },
        });
        let theme = Theme::wsx();
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 160);
        let text = line_text(&line);
        assert!(
            text.contains("working · Audit V2 #2835 · 3/12 done"),
            "got: {text}"
        );
    }

    /// Recap bodies render in the plain theme grey, never faded: the DIM
    /// modifier blends the fg toward the background and left the text
    /// unreadable in terminals that honor SGR 2.
    #[test]
    fn recap_body_renders_plain_dim_never_faded() {
        let mut inputs = base();
        inputs.column = Some(RowColumn {
            token: "idle".to_string(),
            reported: false,
            body: ColumnBody::Recap {
                segments: vec![au("Audit V2 #2835")],
            },
        });
        let theme = Theme::wsx();
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 160);
        let seg_span = line
            .spans
            .iter()
            .find(|s| s.content.contains("Audit V2"))
            .expect("segment span present");
        assert_eq!(seg_span.style.fg, theme.dim_style().fg);
        assert!(!seg_span.style.add_modifier.contains(Modifier::DIM));
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
        inputs.badge = Some(LifecycleBadge::SetupFailed);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⚙!"), "setup badge present: {text:?}");
    }

    #[test]
    fn undelivered_mail_appends_badge() {
        // A peer message wsx gave up injecting stays queued rather than being
        // silently dropped, so the row has to say so — otherwise the only
        // trace is a WARN in the log file.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.undelivered_mail = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("✉!"), "mail badge present: {text:?}");
    }

    #[test]
    fn undelivered_mail_and_setup_failed_badges_both_render() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.undelivered_mail = true;
        inputs.badge = Some(LifecycleBadge::SetupFailed);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⚙!"), "setup badge present: {text:?}");
        assert!(text.contains("✉!"), "mail badge present: {text:?}");
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
        // No good Unicode equivalent to a git-merge icon — the PR chip
        // column carries the lifecycle signal in plain-Unicode mode.
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

    /// Style of the span carrying the branch glyph. The glyph is unique among
    /// a row's spans, so matching on it is enough to isolate the cell.
    fn branch_glyph_style(inputs: &RowInputs, theme: &Theme) -> Style {
        let line = render(inputs, ColumnWidths::default(), 0, theme, 120);
        let glyph = crate::ui::theme::branch_glyph(inputs.lifecycle, inputs.nerd_fonts);
        line.spans
            .iter()
            .find(|s| s.content.as_ref().starts_with(glyph))
            .expect("branch glyph span present")
            .style
    }

    #[test]
    fn branch_glyph_takes_the_lifecycle_color_and_the_name_does_not() {
        let theme = Theme::wsx();
        for lc in [
            BranchLifecycle::PrOpen,
            BranchLifecycle::PrConflicted,
            BranchLifecycle::PrMerged,
            BranchLifecycle::PrClosed,
        ] {
            let mut inputs = base();
            inputs.lifecycle = Some(lc);
            assert_eq!(
                branch_glyph_style(&inputs, &theme).fg,
                theme.lifecycle_style(Some(lc)).unwrap().fg,
                "glyph carries the {lc:?} color"
            );
            let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
            let name = line
                .spans
                .iter()
                .find(|s| s.content.as_ref().contains("bakedbean/repo-overview"))
                .expect("branch name span present");
            assert_eq!(name.style.fg, None, "the name keeps its default color");
            assert!(name.style.add_modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn branch_glyph_dims_when_the_branch_has_no_pr_status() {
        // No PR, a draft PR, and a not-yet-fetched lifecycle all share the
        // "nothing to report" color — the same fallback the chip column uses.
        let theme = Theme::wsx();
        for lc in [
            None,
            Some(BranchLifecycle::NoPr),
            Some(BranchLifecycle::PrDraft),
        ] {
            let mut inputs = base();
            inputs.lifecycle = lc;
            assert_eq!(
                branch_glyph_style(&inputs, &theme).fg,
                Some(theme.dim),
                "glyph dims for {lc:?}"
            );
        }
    }

    #[test]
    fn yolo_warn_colors_the_branch_name_but_not_the_lifecycle_glyph() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.yolo = true;
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        assert_eq!(
            branch_glyph_style(&inputs, &theme).fg,
            theme.ok_style().fg,
            "YOLO recolors the name, not the status glyph"
        );
    }

    #[test]
    fn splitting_the_glyph_span_leaves_column_widths_untouched() {
        // The glyph and the name are truncated as one string and only then
        // split into two spans, so even a column too narrow for both keeps
        // the row's total width — and never panics on a mid-glyph split.
        let theme = Theme::wsx();
        for branch_width in [MIN_BRANCH_WIDTH, 12, DEFAULT_BRANCH_WIDTH] {
            let mut inputs = base();
            inputs.lifecycle = Some(BranchLifecycle::PrOpen);
            inputs.shared = true;
            inputs.has_multi_pane_layout = true;
            inputs.badge = Some(LifecycleBadge::SetupFailed);
            let widths = ColumnWidths::clamped(branch_width, DEFAULT_PR_WIDTH);
            let line = render(&inputs, widths, 0, &theme, 120);
            assert_eq!(
                line_text(&line).chars().count(),
                120,
                "row fills the terminal width at branch_width={branch_width}"
            );
        }
    }

    #[test]
    fn pr_chip_shows_number_and_label_in_lifecycle_color() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        inputs.pr_number = Some(262);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⏺ #262 open"), "chip present: {text:?}");
        let chip = line
            .spans
            .iter()
            .find(|s| s.content.as_ref().starts_with("⏺ #262 open"))
            .expect("chip span present");
        assert_eq!(
            chip.style.fg,
            theme
                .lifecycle_style(Some(BranchLifecycle::PrOpen))
                .unwrap()
                .fg,
            "chip uses the lifecycle color"
        );
    }

    #[test]
    fn pr_chip_without_number_shows_label_only() {
        // The lifecycle can arrive before the PR number has been fetched
        // (or persisted from an older cache row without one) — mirror the
        // detail bar and show just `⏺ open`.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.lifecycle = Some(BranchLifecycle::PrMerged);
        inputs.pr_number = None;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⏺ merged"), "label-only chip: {text:?}");
        assert!(!text.contains('#'), "no number rendered: {text:?}");
    }

    #[test]
    fn no_pr_leaves_chip_column_blank_and_columns_aligned() {
        let theme = Theme::wsx();
        let procs_col = |s: &str| s.chars().position(|c| c == '●');
        // NoPr and not-yet-fetched (None) both render an empty chip cell.
        for lifecycle in [None, Some(BranchLifecycle::NoPr)] {
            let mut inputs = base();
            inputs.lifecycle = lifecycle;
            inputs.pr_number = None;
            let blank = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
            assert!(
                !blank.contains("open") && !blank.contains('⏺') && !blank.contains('#'),
                "no chip content without a PR ({lifecycle:?}): {blank:?}"
            );
            // The empty cell still consumes the column width, so procs
            // stay aligned with a row that has a chip.
            let mut with_chip = base();
            with_chip.lifecycle = Some(BranchLifecycle::PrOpen);
            with_chip.pr_number = Some(7);
            let chipped = line_text(&render(&with_chip, ColumnWidths::default(), 0, &theme, 120));
            assert_eq!(
                procs_col(&blank),
                procs_col(&chipped),
                "procs column must not shift with chip presence:\n  {blank:?}\n  {chipped:?}"
            );
        }
    }

    #[test]
    fn pr_chip_hit_span_matches_rendered_chip_position() {
        // The clickable span must land exactly on the chip characters the
        // row paints, or clicks would open PRs from blank space (or miss
        // the chip entirely).
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        inputs.pr_number = Some(262);
        let widths = ColumnWidths::default();
        let (x, w) = pr_chip_hit_span(&inputs, widths).expect("chip present");
        let text = line_text(&render(&inputs, widths, 0, &theme, 120));
        let chip_start = text.chars().position(|c| c == '⏺').expect("chip rendered");
        assert_eq!(x as usize, chip_start, "span starts at the chip glyph");
        assert_eq!(w as usize, "⏺ #262 open".chars().count());
        // A resized branch column shifts the chip; the span must follow.
        let wide = ColumnWidths::clamped(40, 20);
        let (x_wide, _) = pr_chip_hit_span(&inputs, wide).expect("chip present");
        let text = line_text(&render(&inputs, wide, 0, &theme, 120));
        let chip_start = text.chars().position(|c| c == '⏺').expect("chip rendered");
        assert_eq!(x_wide as usize, chip_start);
    }

    #[test]
    fn pr_chip_hit_span_absent_when_chip_blank() {
        let widths = ColumnWidths::default();
        for lifecycle in [None, Some(BranchLifecycle::NoPr)] {
            let mut inputs = base();
            inputs.lifecycle = lifecycle;
            assert_eq!(
                pr_chip_hit_span(&inputs, widths),
                None,
                "blank chip cell must not be clickable ({lifecycle:?})"
            );
        }
    }

    #[test]
    fn pr_chip_hit_span_width_clamps_to_column() {
        // A long chip truncates to the PR column width; the click target
        // must not spill into the procs column.
        let mut inputs = base();
        inputs.lifecycle = Some(BranchLifecycle::PrConflicted);
        inputs.pr_number = Some(1234567);
        let widths = ColumnWidths::clamped(28, MIN_PR_WIDTH);
        let (_, w) = pr_chip_hit_span(&inputs, widths).expect("chip present");
        assert!(
            (w as usize) <= MIN_PR_WIDTH,
            "span width {w} must fit the {MIN_PR_WIDTH}-wide column"
        );
    }

    #[test]
    fn yolo_colors_branch_warn_and_bold() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.yolo = true;
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let branch_span = line
            .spans
            .iter()
            .find(|s| s.content.as_ref().contains("bakedbean/repo-overview"))
            .expect("branch span present");
        assert_eq!(branch_span.style.fg, Some(theme.warn));
        assert!(branch_span.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn column_widths_clamp_outside_range() {
        let tight = ColumnWidths::clamped(2, 2);
        assert_eq!(tight.branch, MIN_BRANCH_WIDTH);
        assert_eq!(tight.pr, MIN_PR_WIDTH);
        let huge = ColumnWidths::clamped(1000, 1000);
        assert_eq!(huge.branch, MAX_BRANCH_WIDTH);
        assert_eq!(huge.pr, MAX_PR_WIDTH);
        let mid = ColumnWidths::clamped(40, 20);
        assert_eq!(mid.branch, 40);
        assert_eq!(mid.pr, 20);
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
        inputs.badge = Some(LifecycleBadge::SetupFailed);
        let line = render(&inputs, ColumnWidths::default(), 0, &theme, 120);
        let text = line_text(&line);
        assert!(text.contains("⚙!"), "setup badge present: {text:?}");
        assert!(text.contains("\u{f0db}"), "layout badge present: {text:?}");
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
        let narrow = render(&inputs, ColumnWidths::clamped(16, 16), 0, &theme, 160);
        let wide = render(&inputs, ColumnWidths::clamped(50, 16), 0, &theme, 160);
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

    fn strip_text(inputs: &RowInputs, widths: ColumnWidths) -> String {
        let theme = Theme::wsx();
        agent_strip_spans(inputs, widths, &theme)
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    }

    #[test]
    fn strip_at_width_one_is_a_single_primary_bar() {
        let inputs = base();
        assert_eq!(strip_text(&inputs, ColumnWidths::default()), "▎");
    }

    #[test]
    fn strip_right_aligns_with_primary_last() {
        let mut inputs = base();
        inputs.peers = vec![AgentKind::Codex, AgentKind::Pi];
        // Two peers + primary = 3 bars in a 4-wide field: one pad cell.
        assert_eq!(
            strip_text(&inputs, ColumnWidths::default().with_agent(4)),
            " ▎▎▎"
        );
    }

    #[test]
    fn strip_pads_when_the_row_has_fewer_agents_than_the_column() {
        let inputs = base(); // primary only
        assert_eq!(
            strip_text(&inputs, ColumnWidths::default().with_agent(4)),
            "   ▎"
        );
    }

    #[test]
    fn strip_colors_each_bar_by_its_own_kind_with_primary_rightmost() {
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.agent = AgentKind::Claude;
        inputs.peers = vec![AgentKind::Codex];
        let spans = agent_strip_spans(&inputs, ColumnWidths::default().with_agent(2), &theme);
        let bars: Vec<_> = spans.iter().filter(|s| s.content.contains('▎')).collect();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].style.fg, theme.agent_style(AgentKind::Codex).fg);
        assert_eq!(bars[1].style.fg, theme.agent_style(AgentKind::Claude).fg);
    }

    #[test]
    fn strip_overflows_with_a_plus_marker() {
        let theme = Theme::wsx();
        let mut inputs = base();
        // 4 peers + primary = 5 live, one more than MAX_AGENT_WIDTH.
        inputs.peers = vec![
            AgentKind::Codex,
            AgentKind::Pi,
            AgentKind::Hermes,
            AgentKind::Codex,
        ];
        let widths = ColumnWidths::default().with_agent(MAX_AGENT_WIDTH);
        let text = strip_text(&inputs, widths);
        assert_eq!(text, "+▎▎▎");
        assert_eq!(text.chars().count(), MAX_AGENT_WIDTH);

        // Text alone can't distinguish "kept the newest peers" from "kept
        // the oldest" — both render as `+▎▎▎`. Only the bar colors tell
        // them apart: the spec requires the OLDEST peers to drop, so the
        // two surviving peer bars must be Hermes (index 2) then Codex
        // (index 3, the second/duplicate one) — not Codex+Pi (indices 0-1),
        // which is what a `&peers[..peer_cells]` bug would keep instead.
        let spans = agent_strip_spans(&inputs, widths, &theme);
        let bars: Vec<_> = spans.iter().filter(|s| s.content.contains('▎')).collect();
        assert_eq!(bars.len(), 3, "two surviving peers + primary");
        assert_eq!(
            bars[0].style.fg,
            theme.agent_style(AgentKind::Hermes).fg,
            "oldest surviving peer bar must be Hermes, not the dropped Codex/Pi"
        );
        assert_eq!(
            bars[1].style.fg,
            theme.agent_style(AgentKind::Codex).fg,
            "newest peer bar must be the second Codex"
        );
        assert_eq!(
            bars[2].style.fg,
            theme.agent_style(inputs.agent).fg,
            "primary stays rightmost"
        );
    }

    #[test]
    fn strip_is_always_exactly_the_column_width() {
        for agent_width in 1..=MAX_AGENT_WIDTH {
            for peer_count in 0..6 {
                let mut inputs = base();
                inputs.peers = vec![AgentKind::Codex; peer_count];
                let text = strip_text(&inputs, ColumnWidths::default().with_agent(agent_width));
                assert_eq!(
                    text.chars().count(),
                    agent_width,
                    "width {agent_width}, {peer_count} peers: {text:?}"
                );
            }
        }
    }

    #[test]
    fn with_agent_clamps_to_the_cap() {
        assert_eq!(ColumnWidths::default().with_agent(0).agent, 1);
        assert_eq!(
            ColumnWidths::default().with_agent(99).agent,
            MAX_AGENT_WIDTH
        );
    }

    #[test]
    fn widening_the_strip_shifts_every_later_column_by_the_same_amount() {
        let theme = Theme::wsx();
        let procs_col = |s: &str| s.chars().position(|c| c == '●').unwrap();
        let mut inputs = base();
        inputs.procs = 2;
        let narrow = line_text(&render(&inputs, ColumnWidths::default(), 0, &theme, 120));
        let wide = line_text(&render(
            &inputs,
            ColumnWidths::default().with_agent(3),
            0,
            &theme,
            120,
        ));
        assert_eq!(procs_col(&wide), procs_col(&narrow) + 2);
    }

    #[test]
    fn pr_chip_hit_span_tracks_the_agent_column_width() {
        let mut inputs = base();
        inputs.pr_number = Some(12);
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        let (x_narrow, w_narrow) = pr_chip_hit_span(&inputs, ColumnWidths::default()).unwrap();
        let (x_wide, w_wide) =
            pr_chip_hit_span(&inputs, ColumnWidths::default().with_agent(4)).unwrap();
        assert_eq!(x_wide, x_narrow + 3, "chip must shift with the strip");
        assert_eq!(w_wide, w_narrow, "chip width is unaffected");
    }

    #[test]
    fn hit_span_matches_where_the_chip_actually_renders() {
        // Guards the real failure mode: `pr_chip_hit_span` recomputes the
        // offset independently of `left_consumed`, so the two can silently
        // disagree and send clicks to the wrong column.
        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.pr_number = Some(12);
        inputs.lifecycle = Some(BranchLifecycle::PrOpen);
        for agent_width in 1..=MAX_AGENT_WIDTH {
            let widths = ColumnWidths::default().with_agent(agent_width);
            let text = line_text(&render(&inputs, widths, 0, &theme, 160));
            let (x, _) = pr_chip_hit_span(&inputs, widths).unwrap();
            let rendered_at = text.chars().position(|c| c == '⏺').unwrap();
            assert_eq!(
                rendered_at, x as usize,
                "agent_width={agent_width}: hit span disagrees with render"
            );
        }
    }

    #[test]
    fn strip_padding_carries_no_explicit_style_because_the_list_highlight_paints_it() {
        // Documents the *intent*: the pad span itself carries no bg by
        // construction (`Span::raw`), on the theory that `List::highlight_style`
        // paints the selected-row background after the fact. That theory is
        // NOT verified by this test alone — see
        // `strip_padding_gets_the_selected_row_background_from_the_list_highlight`
        // below for the buffer-level check that actually guards it.
        let theme = Theme::wsx();
        for selected in [false, true] {
            let mut inputs = base();
            inputs.selected = selected;
            let spans = agent_strip_spans(&inputs, ColumnWidths::default().with_agent(4), &theme);
            let pad = spans
                .iter()
                .find(|s| s.content.as_ref() == "   ")
                .expect("pad span present");
            assert_eq!(
                pad.style.bg, None,
                "padding must not set its own bg (selected={selected}); \
                 the row highlight paints it instead"
            );
        }
    }

    #[test]
    fn strip_padding_gets_the_selected_row_background_from_the_list_highlight() {
        // The span-level test above can't tell "unstyled because the List
        // highlight paints it" from "unstyled and simply never gets a
        // background" — both look identical at the span level. Render
        // through the real pipeline instead: a `List` with
        // `highlight_style(theme.selected_bg_style())`, exactly as
        // `dashboard::mod::render` wires it up, then assert the ACTUAL
        // buffer bg of the pad cells. If a future ratatui bump changed how
        // (or whether) the highlight paints over unstyled cells, this test
        // — not the span-level one — is what would catch it.
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;
        use ratatui::widgets::{List, ListItem, ListState};

        let theme = Theme::wsx();
        let mut inputs = base();
        inputs.selected = true;
        // agent_width 4 with only the primary agent live leaves 3 pad cells
        // at the row's left edge (columns 0-2), then the primary bar at 3.
        let widths = ColumnWidths::default().with_agent(4);
        let line = render(&inputs, widths, 0, &theme, 40);
        let list = List::new(vec![ListItem::new(line)]).highlight_style(theme.selected_bg_style());
        let mut state = ListState::default();
        state.select(Some(0));

        let backend = TestBackend::new(40, 1);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| f.render_stateful_widget(list, f.area(), &mut state))
            .unwrap();

        let buf = term.backend().buffer();
        for x in 0..3 {
            assert_eq!(
                buf[(x, 0)].bg,
                theme.selected_bg,
                "pad cell at x={x} must carry the selected row background"
            );
        }
    }

    #[test]
    fn lifecycle_badge_derivation_table() {
        use crate::app::render::lifecycle_badge_for;
        use crate::data::in_flight::InFlightKind;
        use crate::data::store::{SetupStatus, WorkspaceState};

        let cases = [
            // (state, setup_status, in_flight, expected)
            (
                WorkspaceState::Ready,
                SetupStatus::Running,
                Some(InFlightKind::Create),
                Some(LifecycleBadge::Provisioning),
            ),
            (
                WorkspaceState::Ready,
                SetupStatus::Ok,
                Some(InFlightKind::Archive),
                Some(LifecycleBadge::Archiving),
            ),
            (
                WorkspaceState::Ready,
                SetupStatus::Failed,
                None,
                Some(LifecycleBadge::SetupFailed),
            ),
            (
                WorkspaceState::Ready,
                SetupStatus::Cancelled,
                None,
                Some(LifecycleBadge::SetupCancelled),
            ),
            (
                WorkspaceState::Failed,
                SetupStatus::NotRun,
                None,
                Some(LifecycleBadge::NoWorktree),
            ),
            (WorkspaceState::Ready, SetupStatus::Ok, None, None),
            (WorkspaceState::Ready, SetupStatus::Skipped, None, None),
            // The persisted Running status alone never badges: without a live
            // registry entry it is the residue of a crash, already swept.
            (WorkspaceState::Ready, SetupStatus::Running, None, None),
            // In-flight always wins over the persisted status.
            (
                WorkspaceState::Failed,
                SetupStatus::Failed,
                Some(InFlightKind::Archive),
                Some(LifecycleBadge::Archiving),
            ),
        ];
        for (state, setup, kind, expected) in cases {
            assert_eq!(
                lifecycle_badge_for(&state, &setup, kind),
                expected,
                "state={state:?} setup={setup:?} in_flight={kind:?}"
            );
        }
    }

    #[test]
    fn in_flight_badges_animate_and_terminal_ones_do_not() {
        let a = LifecycleBadge::Provisioning.glyph(0);
        let b = LifecycleBadge::Provisioning.glyph(1);
        assert_ne!(a, b, "provisioning must animate");
        assert_eq!(
            LifecycleBadge::SetupFailed.glyph(0),
            LifecycleBadge::SetupFailed.glyph(1),
            "terminal badges must be static"
        );
        assert_eq!(LifecycleBadge::SetupFailed.glyph(0), " ⚙!");
    }
}
