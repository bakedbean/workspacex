//! Pure sort and fold helpers for the by-repo view.

use crate::ui::dashboard::status::Status;

/// Per-repo status counts. Mirrors the design's `RepoCounts` shape.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusCounts {
    pub question: u32,
    pub stalled: u32,
    pub waiting: u32,
    pub thinking: u32,
    pub complete: u32,
    pub idle: u32,
}

impl FromIterator<Status> for StatusCounts {
    fn from_iter<I: IntoIterator<Item = Status>>(iter: I) -> Self {
        let mut c = Self::default();
        for s in iter {
            match s {
                Status::Question => c.question += 1,
                Status::Stalled => c.stalled += 1,
                Status::Waiting => c.waiting += 1,
                Status::Thinking => c.thinking += 1,
                Status::Complete => c.complete += 1,
                Status::Idle => c.idle += 1,
            }
        }
        c
    }
}

impl StatusCounts {
    pub fn total(&self) -> u32 {
        self.question + self.stalled + self.waiting + self.thinking + self.complete + self.idle
    }
}

/// How a repo's workspaces are ordered on the by-repo view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    /// Most-recently-active first, coarsely bucketed, with freshly blocked
    /// rows pinned on top. The default: in a repo with many workspaces,
    /// status ordering reshuffles the list out from under you every time an
    /// agent changes state, and what you were reaching for moves.
    #[default]
    Recency,
    /// Most urgent first by status priority. The pre-recency behaviour,
    /// kept as the escape hatch for triage-by-position.
    Status,
}

impl SortMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SortMode::Recency => "recency",
            SortMode::Status => "status",
        }
    }

    /// Parse a persisted setting value. Unrecognised values fall back to the
    /// default rather than erroring — a hand-edited settings row should not
    /// break the dashboard.
    pub fn from_str_or_default(s: &str) -> Self {
        match s {
            "status" => SortMode::Status,
            _ => SortMode::Recency,
        }
    }

    pub fn next(self) -> Self {
        match self {
            SortMode::Recency => SortMode::Status,
            SortMode::Status => SortMode::Recency,
        }
    }
}

/// How long a blocked workspace keeps its pin at the top of the list, unless
/// the `dashboard_blocked_pin_max_age_secs` setting overrides it. Past this,
/// it sorts on age like anything else — long-lived workspaces that sit
/// blocked for days are exactly what the pin must not camp on.
pub const BLOCKED_PIN_MAX_AGE_DEFAULT_SECS: u64 = 24 * 60 * 60;

/// Upper bounds of the recency buckets, in seconds. Rows inside one bucket
/// are ordered by name, not by age, so two agents both working right now
/// never trade places on you; the order only changes when a row crosses a
/// boundary.
const RECENCY_TIERS: [u64; 5] = [
    2 * 60,      // under 2m — active right now
    10 * 60,     // under 10m
    60 * 60,     // under 1h
    6 * 60 * 60, // under 6h
    24 * 60 * 60,
];

/// Which recency bucket an age falls in. Lower = more recent. A row that has
/// never been active (`None`) sorts below every bucket.
fn recency_tier(ago_secs: Option<u64>) -> u8 {
    let Some(age) = ago_secs else {
        return RECENCY_TIERS.len() as u8 + 1;
    };
    RECENCY_TIERS
        .iter()
        .position(|bound| age < *bound)
        .unwrap_or(RECENCY_TIERS.len()) as u8
}

/// Whether a row earns the top-of-list pin: it is blocked on the user *and*
/// it went quiet recently enough that the block is probably still live.
///
/// `Waiting` is deliberately excluded — it means parked on something external
/// (a build, CI), which needs no answer from the user.
fn is_pinned(status: Status, ago_secs: Option<u64>, pin_max_age_secs: u64) -> bool {
    if !matches!(status, Status::Question | Status::Stalled) {
        return false;
    }
    ago_secs.is_some_and(|age| age < pin_max_age_secs)
}

/// The fields the shared workspace comparator reads.
///
/// Both the by-repo renderer and the nav-index builder project their own row
/// type through this trait and call [`order_workspaces`], so the two orderings
/// cannot drift. They must not: the two walk a shared flat index, so a
/// disagreement selects a different row than the one highlighted on screen.
pub trait SortRow {
    fn sort_status(&self) -> Status;
    /// Seconds since this workspace was last active; `None` if it never was.
    fn sort_ago_secs(&self) -> Option<u64>;
    /// The row's display name, used as the final tiebreak so the order is
    /// total — equal keys must not depend on input order.
    fn sort_name(&self) -> &str;
}

/// Order a repo's workspaces for display.
pub fn order_workspaces<T: SortRow>(items: &mut [T], mode: SortMode, pin_max_age_secs: u64) {
    match mode {
        SortMode::Status => {
            items.sort_by_key(|w| std::cmp::Reverse(w.sort_status().priority()));
        }
        SortMode::Recency => {
            // `false` sorts before `true`, so an un-pinned row ranks below a
            // pinned one. Age only enters as a bucket; inside a bucket the
            // name decides, which is what keeps the list still.
            let key = |w: &T| {
                (
                    !is_pinned(w.sort_status(), w.sort_ago_secs(), pin_max_age_secs),
                    recency_tier(w.sort_ago_secs()),
                )
            };
            items.sort_by(|a, b| {
                key(a)
                    .cmp(&key(b))
                    .then_with(|| a.sort_name().cmp(b.sort_name()))
            });
        }
    }
}

/// Default fold state for a repo. `true` = folded by default.
/// Empty repos and all-quiet repos (no live + no attention) start folded.
pub fn default_fold(c: StatusCounts) -> bool {
    if c.total() == 0 {
        return true;
    }
    (c.question + c.stalled + c.waiting + c.thinking) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(q: u32, s: u32, w: u32, t: u32, c: u32, i: u32) -> StatusCounts {
        StatusCounts {
            question: q,
            stalled: s,
            waiting: w,
            thinking: t,
            complete: c,
            idle: i,
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Row {
        name: &'static str,
        status: Status,
        ago_secs: Option<u64>,
    }

    impl SortRow for Row {
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

    const MIN: u64 = 60;
    const HOUR: u64 = 60 * 60;

    /// A row with an age but nothing to pin it.
    fn row(name: &'static str, ago_secs: u64) -> Row {
        Row {
            name,
            status: Status::Thinking,
            ago_secs: Some(ago_secs),
        }
    }

    fn with_status(name: &'static str, status: Status, ago_secs: u64) -> Row {
        Row {
            name,
            status,
            ago_secs: Some(ago_secs),
        }
    }

    fn blocked(name: &'static str, ago_secs: u64) -> Row {
        with_status(name, Status::Question, ago_secs)
    }

    fn ordered(mut rows: Vec<Row>, mode: SortMode) -> Vec<&'static str> {
        order_workspaces(&mut rows, mode, BLOCKED_PIN_MAX_AGE_DEFAULT_SECS);
        rows.iter().map(|r| r.name).collect()
    }

    #[test]
    fn status_mode_puts_most_urgent_first() {
        let rows = vec![
            with_status("idle", Status::Idle, 1),
            with_status("stalled", Status::Stalled, 1),
            with_status("thinking", Status::Thinking, 1),
            with_status("question", Status::Question, 1),
        ];
        assert_eq!(
            ordered(rows, SortMode::Status),
            vec!["stalled", "question", "thinking", "idle"]
        );
    }

    #[test]
    fn recency_orders_fresher_tiers_first() {
        let rows = vec![
            row("six-hours", 7 * HOUR),
            row("seconds", 30),
            row("half-hour", 30 * MIN),
            row("five-minutes", 5 * MIN),
        ];
        assert_eq!(
            ordered(rows, SortMode::Recency),
            vec!["seconds", "five-minutes", "half-hour", "six-hours"]
        );
    }

    #[test]
    fn recency_orders_rows_inside_one_tier_by_name_not_age() {
        // Both under 2m: the whole point of bucketing is that two agents
        // working right now do not trade places second by second.
        let rows = vec![row("zebra", 5), row("alpha", 100)];
        assert_eq!(ordered(rows, SortMode::Recency), vec!["alpha", "zebra"]);
    }

    #[test]
    fn recency_sorts_never_active_rows_last() {
        let rows = vec![
            Row {
                name: "never",
                status: Status::Idle,
                ago_secs: None,
            },
            row("ancient", 30 * 24 * HOUR),
        ];
        assert_eq!(ordered(rows, SortMode::Recency), vec!["ancient", "never"]);
    }

    #[test]
    fn recency_pins_a_freshly_blocked_row_above_newer_activity() {
        let rows = vec![row("busy", 5), blocked("asked-me", 22 * MIN)];
        assert_eq!(ordered(rows, SortMode::Recency), vec!["asked-me", "busy"]);
    }

    #[test]
    fn recency_drops_a_long_stale_blocked_row_out_of_the_pin() {
        // The complaint this mode exists for: a workspace parked blocked for
        // weeks must not camp at the top of the repo.
        let rows = vec![row("busy", 5), blocked("blocked-for-weeks", 21 * 24 * HOUR)];
        assert_eq!(
            ordered(rows, SortMode::Recency),
            vec!["busy", "blocked-for-weeks"]
        );
    }

    #[test]
    fn recency_expires_the_pin_exactly_at_the_cutoff() {
        let mut rows = vec![row("busy", 5), blocked("edge", 100)];
        order_workspaces(&mut rows, SortMode::Recency, 100);
        assert_eq!(
            rows.iter().map(|r| r.name).collect::<Vec<_>>(),
            vec!["busy", "edge"],
            "a row exactly at the cutoff is already too old to pin"
        );
    }

    #[test]
    fn recency_orders_pinned_rows_among_themselves_by_recency() {
        let rows = vec![blocked("older", 3 * HOUR), blocked("newer", 3 * MIN)];
        assert_eq!(ordered(rows, SortMode::Recency), vec!["newer", "older"]);
    }

    #[test]
    fn recency_does_not_pin_waiting_rows() {
        // Waiting means parked on a build or CI — it needs nothing from the
        // user, so it earns no pin.
        let rows = vec![
            row("busy", 5),
            with_status("on-ci", Status::Waiting, 10 * MIN),
        ];
        assert_eq!(ordered(rows, SortMode::Recency), vec!["busy", "on-ci"]);
    }

    #[test]
    fn recency_pins_stalled_rows_too() {
        let rows = vec![
            row("busy", 5),
            with_status("stalled", Status::Stalled, 10 * MIN),
        ];
        assert_eq!(ordered(rows, SortMode::Recency), vec!["stalled", "busy"]);
    }

    #[test]
    fn sort_mode_round_trips_through_its_setting_value() {
        for mode in [SortMode::Recency, SortMode::Status] {
            assert_eq!(SortMode::from_str_or_default(mode.as_str()), mode);
        }
    }

    #[test]
    fn unknown_sort_mode_setting_falls_back_to_the_default() {
        assert_eq!(
            SortMode::from_str_or_default("nonsense"),
            SortMode::default()
        );
    }

    #[test]
    fn default_fold_empty_repo_is_folded() {
        assert!(default_fold(counts(0, 0, 0, 0, 0, 0)));
    }

    #[test]
    fn default_fold_all_idle_is_folded() {
        assert!(default_fold(counts(0, 0, 0, 0, 0, 3)));
    }

    #[test]
    fn default_fold_complete_only_is_folded() {
        assert!(default_fold(counts(0, 0, 0, 0, 5, 0)));
    }

    #[test]
    fn default_fold_thinking_is_expanded() {
        assert!(!default_fold(counts(0, 0, 0, 1, 0, 0)));
    }

    #[test]
    fn status_counts_from_iter() {
        let c = StatusCounts::from_iter([
            Status::Question,
            Status::Stalled,
            Status::Stalled,
            Status::Idle,
        ]);
        assert_eq!(c.question, 1);
        assert_eq!(c.stalled, 2);
        assert_eq!(c.idle, 1);
        assert_eq!(c.total(), 4);
    }
}
