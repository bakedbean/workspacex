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

/// The fields the shared workspace comparator reads.
///
/// Both the by-repo renderer and the nav-index builder project their own row
/// type through this trait and call [`order_workspaces`], so the two orderings
/// cannot drift. They must not: the two walk a shared flat index, so a
/// disagreement selects a different row than the one highlighted on screen.
pub trait SortRow {
    fn sort_status(&self) -> Status;
}

/// Order a repo's workspaces for display. Most urgent first.
pub fn order_workspaces<T: SortRow>(items: &mut [T]) {
    items.sort_by_key(|w| std::cmp::Reverse(w.sort_status().priority()));
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
    }

    impl SortRow for Row {
        fn sort_status(&self) -> Status {
            self.status
        }
    }

    #[test]
    fn order_workspaces_puts_most_urgent_first() {
        let mut rows = vec![
            Row {
                name: "idle",
                status: Status::Idle,
            },
            Row {
                name: "stalled",
                status: Status::Stalled,
            },
            Row {
                name: "thinking",
                status: Status::Thinking,
            },
            Row {
                name: "question",
                status: Status::Question,
            },
        ];
        order_workspaces(&mut rows);
        assert_eq!(
            rows.iter().map(|r| r.name).collect::<Vec<_>>(),
            vec!["stalled", "question", "thinking", "idle"]
        );
    }

    #[test]
    fn order_workspaces_keeps_equal_status_in_input_order() {
        let mut rows = vec![
            Row {
                name: "zebra",
                status: Status::Idle,
            },
            Row {
                name: "alpha",
                status: Status::Idle,
            },
        ];
        order_workspaces(&mut rows);
        assert_eq!(
            rows.iter().map(|r| r.name).collect::<Vec<_>>(),
            vec!["zebra", "alpha"],
            "the extraction must preserve the existing stable-sort behaviour"
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
