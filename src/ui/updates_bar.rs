//! Content selection for the attached-view "other workspaces" status row.
//!
//! Pure module: takes pre-computed slices of App state, returns an inline
//! list of attention-needing workspaces. The caller (typically
//! `attached::render`) handles drawing. The activity-fallback path that
//! previously surfaced "most recent event" was removed — issue #18 makes
//! the status row exclusively about workspaces that need user action.

use crate::activity::events::WorkspaceEvents;
use crate::data::store::WorkspaceId;
use crate::git::forge::BranchLifecycle;
use crate::ui::dashboard::sort::{SortMode, SortRow, order_workspaces};
use crate::ui::dashboard::status::Status;
use crate::ui::theme::Theme;
use ratatui::text::{Line, Span};

/// Activity classification mirrors `app::ActivityState`. Kept here as a
/// re-export-friendly enum so updates_bar doesn't depend on app.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// Agent paused waiting for the user to answer a question.
    AwaitingAnswer,
    /// Agent finished a task and is awaiting acknowledgment.
    Complete,
    Awaiting,
    Active,
    Idle,
    /// Claude has stalled mid-tool-chain.
    Stalled,
    Waiting,
    Off,
}

#[derive(Debug, Clone)]
pub struct WorkspaceUpdateInfo<'a> {
    pub id: WorkspaceId,
    pub name: &'a str,
    pub repo_name: &'a str,
    pub events: Option<&'a WorkspaceEvents>,
    pub activity: ActivityState,
    pub needs_attention: bool,
    /// Cached PR lifecycle for this workspace's branch, mirroring the
    /// dashboard's `app.pr_lifecycle`. Drives the attention entry's
    /// name color so the top bar matches the dashboard's PR hues.
    pub lifecycle: Option<BranchLifecycle>,
    /// `Some((tool_name, first_seen_ms))` when a tool_use has been pending
    /// for the App's stale threshold. Caller computes via
    /// `App::awaiting_permission`.
    pub awaiting_tool: Option<(String, i64)>,
    /// The dashboard's recency signal for this workspace (seconds since
    /// last interaction, `None` = never active). Caller computes via
    /// `workspace_age_secs` so the attention line orders entries exactly
    /// like the dashboard's NEEDS ATTENTION section.
    pub ago_secs: Option<u64>,
    /// The dashboard's canonical classification for this workspace.
    /// Caller computes via `App::classify_status`; deliberately NOT
    /// derived from the legacy bell `activity`, whose classifier lacks
    /// the dashboard's PTY-active question suppression and pushed
    /// `ReportedState` handling, so the two can disagree. Drives the
    /// sort so ordering matches the dashboard rows.
    pub status: Status,
}

/// One workspace that the user should pay attention to. Carries
/// pre-computed display fields so the renderer doesn't need access to live
/// App state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttentionEntry {
    pub workspace_id: WorkspaceId,
    pub repo_name: String,
    pub name: String,
    /// Anchor epoch-ms for the "(5m)" age display. The most recent of:
    /// pending tool_use timestamp, latest event timestamp, or `now`.
    pub age_anchor_ms: i64,
    /// The workspace's dashboard status. Drives the row glyph and its
    /// color, so an entry reads exactly like its dashboard row.
    pub status: Status,
    /// PR lifecycle for this workspace, used to color the `repo/name`
    /// text with the same hues the dashboard uses (green=open,
    /// purple=merged, …). `None` (or a colorless lifecycle like NoPr)
    /// falls back to the muted `path` color.
    pub lifecycle: Option<BranchLifecycle>,
}

/// The rendered attention line plus the clickable geometry of each entry.
/// Returned by [`format_attention_line_styled`] so the render pass can map
/// entries to screen rects for mouse hit-testing.
#[derive(Debug, Clone)]
pub struct AttentionLine {
    pub line: Line<'static>,
    /// One segment per *rendered* entry (the `included` ones, not the
    /// `… +N more` overflow). Columns are 0-based from the line's left edge.
    pub segments: Vec<AttentionSegment>,
}

/// The clickable extent of one attention entry: which workspace it points to
/// and where it sits within the line (column offset + width, in cells).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionSegment {
    pub workspace_id: WorkspaceId,
    pub start_col: u16,
    pub width: u16,
}

/// One-char glyph for the inline status row. Mirrors the dashboard's
/// attn-marker vocabulary so users see the same icons in both surfaces.
pub fn glyph_for_activity(a: ActivityState) -> char {
    match a {
        ActivityState::AwaitingAnswer => '?',
        ActivityState::Complete => '\u{2713}', // ✓ CHECK MARK
        ActivityState::Awaiting | ActivityState::Stalled => '⚠',
        // Defensive default — non-alertable states shouldn't appear
        // in the status row (collect_attention filters by
        // needs_attention) but be safe.
        _ => '⚠',
    }
}

/// V5-styled variant of `format_attention_line`. Produces a `Line` whose
/// per-entry glyph is colored by the workspace's V5 `Status`, repo/name
/// in `path`, age in `dim`, separators in `dim`.
pub fn format_attention_line_styled(
    entries: &[AttentionEntry],
    now_ms: i64,
    max_width: usize,
    theme: &Theme,
) -> Option<AttentionLine> {
    if entries.is_empty() {
        return None;
    }
    // Compute the visual width of one entry: "<glyph> <repo>/<name> (<age>)".
    let widths: Vec<usize> = entries
        .iter()
        .map(|e| {
            let age = format_age(now_ms.saturating_sub(e.age_anchor_ms));
            1 + 1
                + e.repo_name.chars().count()
                + 1
                + e.name.chars().count()
                + 2
                + age.chars().count()
                + 1
        })
        .collect();
    let sep_w = 3; // " │ "
    let mut included = 0usize;
    let mut total = 0usize;
    for (i, w) in widths.iter().enumerate() {
        let s = if i == 0 { 0 } else { sep_w };
        if total + s + w > max_width {
            break;
        }
        total += s + w;
        included += 1;
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut segments: Vec<AttentionSegment> = Vec::new();
    // Always render at least one entry; if the first doesn't fit we emit
    // it as-is and rely on ratatui's clipping.
    if included == 0 {
        included = 1;
    }
    let mut col: usize = 0;
    for (i, e) in entries.iter().take(included).enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ".to_string(), theme.dim_style()));
            col += sep_w;
        }
        let entry_start = col;
        let glyph = e.status.glyph().to_string();
        spans.push(Span::styled(glyph, theme.status_style(e.status)));
        spans.push(Span::raw(" ".to_string()));
        // Color the name by PR lifecycle to match the dashboard (green
        // open, purple merged, …). Colorless lifecycles (NoPr/PrDraft)
        // and never-polled workspaces fall back to the muted `path` hue.
        let name_style = theme
            .lifecycle_style(e.lifecycle)
            .unwrap_or_else(|| ratatui::style::Style::default().fg(theme.path));
        spans.push(Span::styled(
            format!("{}/{}", e.repo_name, e.name),
            name_style,
        ));
        let age = format_age(now_ms.saturating_sub(e.age_anchor_ms));
        spans.push(Span::styled(format!(" ({age})"), theme.dim_style()));
        col += widths[i];
        segments.push(AttentionSegment {
            workspace_id: e.workspace_id,
            start_col: entry_start as u16,
            width: widths[i] as u16,
        });
    }
    let remaining = entries.len().saturating_sub(included);
    if remaining > 0 {
        spans.push(Span::styled(
            format!(" … +{remaining} more"),
            theme.dim_style(),
        ));
    }
    Some(AttentionLine {
        line: Line::from(spans),
        segments,
    })
}

impl SortRow for WorkspaceUpdateInfo<'_> {
    fn sort_status(&self) -> Status {
        self.status
    }
    fn sort_ago_secs(&self) -> Option<u64> {
        self.ago_secs
    }
    fn sort_name(&self) -> &str {
        self.name
    }
}

/// Collect every workspace except the currently-attached one, as the
/// attached view's top-bar row. Workspaces whose `needs_attention` flag is
/// set are promoted ahead of the rest; inside each group the dashboard's
/// own comparator decides under the dashboard's current sort mode, so the
/// row reads the same way the by-repo list does. Callers pass
/// `DashboardState::sort_mode` and `blocked_pin_max_age_secs`.
pub fn collect_workspace_row(
    candidates: &[WorkspaceUpdateInfo],
    attached_workspace: Option<WorkspaceId>,
    now_ms: i64,
    sort_mode: SortMode,
    pin_max_age_secs: u64,
) -> Vec<AttentionEntry> {
    let (mut flagged, mut rest): (Vec<&WorkspaceUpdateInfo>, Vec<&WorkspaceUpdateInfo>) =
        candidates
            .iter()
            .filter(|c| Some(c.id) != attached_workspace)
            .partition(|c| c.needs_attention);
    order_workspaces(&mut flagged, sort_mode, pin_max_age_secs);
    order_workspaces(&mut rest, sort_mode, pin_max_age_secs);
    flagged
        .into_iter()
        .chain(rest)
        .map(|c| {
            let age_anchor_ms = c
                .awaiting_tool
                .as_ref()
                .map(|(_, t)| *t)
                .or_else(|| {
                    c.events
                        .and_then(|e| e.latest.as_ref().map(|s| s.timestamp_ms))
                })
                .unwrap_or(now_ms);
            AttentionEntry {
                workspace_id: c.id,
                repo_name: c.repo_name.to_string(),
                name: c.name.to_string(),
                age_anchor_ms,
                status: c.status,
                lifecycle: c.lifecycle,
            }
        })
        .collect()
}

// Moved to `crate::util::time` so non-TUI callers (the macOS menubar) can use it
// without depending on ratatui widget code. Re-exported here because the
// updates bar, the updates panel, and the PM digest all reach it by this path.
pub use crate::util::time::format_age;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::events::{EventKind, EventSnapshot, WorkspaceEvents};
    use crate::data::store::WorkspaceId;
    use crate::ui::dashboard::sort::BLOCKED_PIN_MAX_AGE_DEFAULT_SECS as PIN;

    /// Test fixture: derive a canonical status from the legacy activity.
    fn status_for_activity(a: ActivityState) -> Status {
        match a {
            ActivityState::AwaitingAnswer => Status::Question,
            ActivityState::Stalled => Status::Stalled,
            ActivityState::Awaiting => Status::Question,
            ActivityState::Complete => Status::Complete,
            ActivityState::Active => Status::Thinking,
            ActivityState::Waiting => Status::Waiting,
            ActivityState::Idle | ActivityState::Off => Status::Idle,
        }
    }

    type WsOwned = (
        WorkspaceId,
        Option<WorkspaceEvents>,
        ActivityState,
        bool,
        Option<(String, i64)>,
        String,      // name
        String,      // repo_name
        Option<u64>, // ago_secs
        Status,      // canonical dashboard status
    );

    fn ws(
        id: i64,
        name: &str,
        events: Option<WorkspaceEvents>,
        activity: ActivityState,
        needs_attention: bool,
        awaiting: Option<(String, i64)>,
    ) -> WsOwned {
        ws_ago(id, name, events, activity, needs_attention, awaiting, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn ws_ago(
        id: i64,
        name: &str,
        events: Option<WorkspaceEvents>,
        activity: ActivityState,
        needs_attention: bool,
        awaiting: Option<(String, i64)>,
        ago_secs: Option<u64>,
    ) -> WsOwned {
        (
            WorkspaceId(id),
            events,
            activity,
            needs_attention,
            awaiting,
            name.to_string(),
            "test-repo".to_string(),
            ago_secs,
            // Tests that need the canonical status to diverge from the
            // legacy activity construct WorkspaceUpdateInfo directly.
            status_for_activity(activity),
        )
    }

    fn snap(display: &str, timestamp_ms: i64) -> EventSnapshot {
        EventSnapshot {
            kind: EventKind::AssistantText,
            display: display.to_string(),
            timestamp_ms,
        }
    }

    fn events_with_latest(display: &str, timestamp_ms: i64) -> WorkspaceEvents {
        WorkspaceEvents {
            latest: Some(snap(display, timestamp_ms)),
            ..Default::default()
        }
    }

    fn to_candidates(rows: &[WsOwned]) -> Vec<WorkspaceUpdateInfo<'_>> {
        rows.iter()
            .map(
                |(
                    id,
                    events,
                    activity,
                    needs_attention,
                    awaiting,
                    name,
                    repo_name,
                    ago_secs,
                    status,
                )| {
                    WorkspaceUpdateInfo {
                        id: *id,
                        name: name.as_str(),
                        repo_name: repo_name.as_str(),
                        events: events.as_ref(),
                        activity: *activity,
                        needs_attention: *needs_attention,
                        lifecycle: None,
                        awaiting_tool: awaiting.clone(),
                        ago_secs: *ago_secs,
                        status: *status,
                    }
                },
            )
            .collect()
    }

    #[test]
    fn collect_workspace_row_lists_unflagged_workspaces() {
        // The row is a workspace list, not an alert list: a workspace with
        // no attention flag still gets an entry.
        let evt = events_with_latest("recent", 5_000);
        let rows = [ws(1, "busy", Some(evt), ActivityState::Idle, false, None)];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "busy");
        assert_eq!(entries[0].status, Status::Idle);
    }

    #[test]
    fn collect_workspace_row_promotes_flagged_before_dashboard_order() {
        // Flagged workspaces come first regardless of the dashboard
        // comparator; inside each group the comparator decides (recency
        // mode here, so the fresher unflagged row precedes the older one).
        let rows = [
            ws_ago(
                1,
                "older",
                None,
                ActivityState::Idle,
                false,
                None,
                Some(600),
            ),
            ws_ago(2, "fresh", None, ActivityState::Idle, false, None, Some(5)),
            ws_ago(
                3,
                "flagged-stale",
                None,
                ActivityState::Complete,
                true,
                None,
                Some(9_000),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["flagged-stale", "fresh", "older"]);
    }

    #[test]
    fn collect_workspace_row_sorts_most_recently_interacted_first() {
        // Same status priority — order falls to the dashboard's ago_secs
        // recency signal (smaller = more recent = first), NOT the event
        // timestamps (which here would give the opposite order).
        let older_evt = events_with_latest("stale-ws-newer-event", 9_000);
        let newer_evt = events_with_latest("fresh-ws-older-event", 1_000);
        let rows = [
            ws_ago(
                1,
                "stale",
                Some(older_evt),
                ActivityState::Awaiting,
                true,
                None,
                Some(600),
            ),
            ws_ago(
                2,
                "fresh",
                Some(newer_evt),
                ActivityState::Awaiting,
                true,
                None,
                Some(5),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "fresh");
        assert_eq!(entries[1].name, "stale");
    }

    #[test]
    fn collect_workspace_row_status_mode_sorts_priority_before_recency() {
        // The dashboard's `sort: status` mode: Stalled (5) outranks
        // AwaitingAnswer/Question (4) outranks Waiting (3), even when
        // lower-priority entries are more recent.
        let rows = [
            ws_ago(
                1,
                "waiting",
                None,
                ActivityState::Waiting,
                true,
                None,
                Some(1),
            ),
            ws_ago(
                2,
                "question",
                None,
                ActivityState::AwaitingAnswer,
                true,
                None,
                Some(2),
            ),
            ws_ago(
                3,
                "stalled",
                None,
                ActivityState::Stalled,
                true,
                None,
                Some(300),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Status, PIN);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["stalled", "question", "waiting"]);
    }

    #[test]
    fn collect_workspace_row_recency_mode_orders_like_the_dashboard() {
        // The dashboard's default `sort: recency` mode: a freshly blocked
        // row is pinned on top, everything else sits in its recency
        // bucket, and a block older than the pin window sorts on age like
        // any other row. Status priority must NOT float a days-old
        // stalled workspace over rows that were active a minute ago.
        const DAY: u64 = 24 * 60 * 60;
        let rows = [
            ws_ago(
                1,
                "stale-stalled",
                None,
                ActivityState::Stalled,
                true,
                None,
                Some(3 * DAY),
            ),
            ws_ago(
                2,
                "fresh-waiting",
                None,
                ActivityState::Waiting,
                true,
                None,
                Some(30),
            ),
            ws_ago(
                3,
                "fresh-question",
                None,
                ActivityState::AwaitingAnswer,
                true,
                None,
                Some(60),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["fresh-question", "fresh-waiting", "stale-stalled"]);
    }

    #[test]
    fn collect_workspace_row_sorts_by_canonical_status_not_legacy_activity() {
        // Both carry the legacy bell activity AwaitingAnswer (→ Question),
        // but the dashboard's canonical classifier downgraded ws1 to
        // Waiting (e.g. PTY-active question suppression). The sort must
        // follow the canonical status, so ws2 outranks ws1 despite ws1
        // being far more recent.
        let mk = |id: i64, name: &'static str, status: Status, ago: u64| WorkspaceUpdateInfo {
            id: WorkspaceId(id),
            name,
            repo_name: "test-repo",
            events: None,
            activity: ActivityState::AwaitingAnswer,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            ago_secs: Some(ago),
            status,
        };
        let candidates = vec![
            mk(1, "suppressed", Status::Waiting, 1),
            mk(2, "question", Status::Question, 500),
        ];
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries[0].name, "question");
        assert_eq!(entries[1].name, "suppressed");
    }

    #[test]
    fn collect_workspace_row_sorts_never_active_last() {
        let rows = [
            ws_ago(1, "never", None, ActivityState::Awaiting, true, None, None),
            ws_ago(
                2,
                "old",
                None,
                ActivityState::Awaiting,
                true,
                None,
                Some(9_999),
            ),
        ];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries[0].name, "old");
        assert_eq!(entries[1].name, "never");
    }

    #[test]
    fn collect_workspace_row_excludes_currently_attached() {
        let evt = events_with_latest("evt", 5_000);
        let rows = [ws(1, "self", Some(evt), ActivityState::Waiting, true, None)];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(
            &candidates,
            Some(WorkspaceId(1)),
            10_000,
            SortMode::Recency,
            PIN,
        );
        assert!(entries.is_empty());
    }

    #[test]
    fn collect_workspace_row_uses_awaiting_tool_timestamp_as_anchor() {
        // awaiting_tool's first-seen ts takes priority over latest event ts
        let evt = events_with_latest("old", 1_000);
        let rows = [ws(
            1,
            "ws",
            Some(evt),
            ActivityState::Awaiting,
            true,
            Some(("Bash".to_string(), 8_000)),
        )];
        let candidates = to_candidates(&rows);
        let entries = collect_workspace_row(&candidates, None, 10_000, SortMode::Recency, PIN);
        assert_eq!(entries[0].age_anchor_ms, 8_000);
    }

    #[test]
    fn styled_line_colors_name_by_pr_lifecycle() {
        use crate::git::forge::BranchLifecycle;
        let theme = Theme::wsx();
        let entries = vec![
            // Open PR -> green (theme.ok).
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "r".into(),
                name: "open".into(),
                age_anchor_ms: 9_000,
                status: Status::Question,
                lifecycle: Some(BranchLifecycle::PrOpen),
            },
            // Merged PR -> purple (theme.merged).
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "r".into(),
                name: "merged".into(),
                age_anchor_ms: 9_000,
                status: Status::Complete,
                lifecycle: Some(BranchLifecycle::PrMerged),
            },
            // No PR -> falls back to the muted path color.
            AttentionEntry {
                workspace_id: WorkspaceId(3),
                repo_name: "r".into(),
                name: "nopr".into(),
                age_anchor_ms: 9_000,
                status: Status::Question,
                lifecycle: Some(BranchLifecycle::NoPr),
            },
        ];
        let line = format_attention_line_styled(&entries, 10_000, 200, &theme)
            .expect("line")
            .line;
        let name_fg = |name: &str| {
            line.spans
                .iter()
                .find(|s| s.content.as_ref().ends_with(name))
                .unwrap_or_else(|| panic!("name span for {name:?} present"))
                .style
                .fg
        };
        assert_eq!(name_fg("r/open"), Some(theme.ok), "open PR name is green");
        assert_eq!(
            name_fg("r/merged"),
            Some(theme.merged),
            "merged PR name is purple"
        );
        assert_eq!(
            name_fg("r/nopr"),
            Some(theme.path),
            "no-PR name falls back to path color"
        );
    }

    #[test]
    fn styled_line_colors_each_entry_by_status() {
        let theme = Theme::wsx();
        let entries = vec![
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "a".into(),
                name: "q".into(),
                age_anchor_ms: 9_000,
                status: Status::Question,
                lifecycle: None,
            },
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "b".into(),
                name: "s".into(),
                age_anchor_ms: 9_000,
                status: Status::Stalled,
                lifecycle: None,
            },
        ];
        let line = format_attention_line_styled(&entries, 10_000, 200, &theme)
            .expect("line")
            .line;
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("? a/q"), "first entry glyph + name: {text:?}");
        assert!(
            text.contains("! b/s"),
            "second entry glyph + name: {text:?}"
        );
        // First glyph span carries the Question color.
        let q_glyph = &line.spans[0];
        assert_eq!(q_glyph.content.as_ref(), "?");
        assert_eq!(q_glyph.style.fg, Some(theme.question));
        // After "? ", "a/q", " (1s)", " │ ", the next non-sep glyph is "!".
        // Search for it explicitly.
        let stalled = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "!")
            .expect("stalled glyph present");
        assert_eq!(stalled.style.fg, Some(theme.stalled));
    }

    #[test]
    fn styled_line_returns_none_when_empty() {
        let theme = Theme::wsx();
        assert!(format_attention_line_styled(&[], 0, 80, &theme).is_none());
    }

    #[test]
    fn styled_line_emits_clickable_segment_per_entry() {
        let theme = Theme::wsx();
        let entries = vec![
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "a".into(),
                name: "q".into(),
                age_anchor_ms: 9_000,
                status: Status::Question,
                lifecycle: None,
            },
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "bb".into(),
                name: "ss".into(),
                age_anchor_ms: 9_000,
                status: Status::Stalled,
                lifecycle: None,
            },
        ];
        // now_ms - age_anchor_ms = 1_000 -> age "1s" (2 chars) for both.
        let out = format_attention_line_styled(&entries, 10_000, 200, &theme).expect("line");
        assert_eq!(out.segments.len(), 2, "one segment per rendered entry");

        // Entry 0: "? a/q (1s)" -> 1+1 + 1 +1+ 1 + 2 + 2 + 1 = 10 cols, at col 0.
        assert_eq!(out.segments[0].workspace_id, WorkspaceId(1));
        assert_eq!(out.segments[0].start_col, 0);
        assert_eq!(out.segments[0].width, 10);

        // Entry 1 width: "! bb/ss (1s)" -> 1+1 + 2 +1+ 2 + 2 + 2 + 1 = 12 cols.
        // start_col = entry0 width (10) + separator (3) = 13.
        assert_eq!(out.segments[1].workspace_id, WorkspaceId(2));
        assert_eq!(out.segments[1].start_col, 13);
        assert_eq!(out.segments[1].width, 12);
    }

    #[test]
    fn styled_line_segments_exclude_overflow_more_tail() {
        let theme = Theme::wsx();
        let entries = vec![
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "a".into(),
                name: "q".into(),
                age_anchor_ms: 9_000,
                status: Status::Question,
                lifecycle: None,
            },
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "bb".into(),
                name: "ss".into(),
                age_anchor_ms: 9_000,
                status: Status::Stalled,
                lifecycle: None,
            },
        ];
        // max_width 10 fits only entry 0 ("? a/q (1s)" is exactly 10).
        let out = format_attention_line_styled(&entries, 10_000, 10, &theme).expect("line");
        assert_eq!(
            out.segments.len(),
            1,
            "only the included entry is clickable"
        );
        assert_eq!(out.segments[0].workspace_id, WorkspaceId(1));
    }

    #[test]
    fn styled_line_glyph_follows_canonical_status() {
        // The glyph is the dashboard's, so a row reads the same in both
        // surfaces: Waiting draws the ellipsis, Idle the dot.
        let theme = Theme::wsx();
        let entries = vec![
            AttentionEntry {
                workspace_id: WorkspaceId(1),
                repo_name: "a".into(),
                name: "w".into(),
                age_anchor_ms: 9_000,
                status: Status::Waiting,
                lifecycle: None,
            },
            AttentionEntry {
                workspace_id: WorkspaceId(2),
                repo_name: "a".into(),
                name: "i".into(),
                age_anchor_ms: 9_000,
                status: Status::Idle,
                lifecycle: None,
            },
        ];
        let line = format_attention_line_styled(&entries, 10_000, 200, &theme)
            .expect("line")
            .line;
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("\u{2026} a/w"), "waiting glyph: {text:?}");
        assert!(text.contains("\u{b7} a/i"), "idle glyph: {text:?}");
        let idle = line
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{b7}")
            .expect("idle glyph span");
        assert_eq!(idle.style, theme.status_style(Status::Idle));
    }
}
