//! Per-repo rules that hide dashboard workspaces. Persisted in
//! `repos.hide_rules` as namespaced tokens (`pr:merged,pr:none`) so future
//! criteria (`idle:14d`) can't collide; unknown tokens are skipped.

use crate::git::forge::BranchLifecycle;

/// A PR-lifecycle criterion. Declaration order is the canonical display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PrCriterion {
    Merged,
    Closed,
    NoPr,
    Draft,
    Open,
    Conflicted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HideRule {
    Pr(PrCriterion),
}

/// A no-PR workspace active within this many days stays visible.
pub const NO_PR_RECENT_DAYS: u64 = 3;

/// The facts a rule can test about a workspace. Grows a field per criterion family.
#[derive(Debug, Clone, Copy, Default)]
pub struct HideSubject {
    pub lifecycle: Option<BranchLifecycle>,
    /// Commits ahead of the base branch, or uncommitted changes.
    pub work_in_progress: bool,
    /// A running agent session, or a fresh wsx status of working/waiting/blocked.
    pub agent_live: bool,
    /// Seconds since last activity; `None` when none was ever recorded.
    pub idle_secs: Option<u64>,
}

impl HideSubject {
    /// Any sign the workspace is in use, which exempts it from `pr:none`.
    pub fn in_progress(&self) -> bool {
        self.work_in_progress
            || self.agent_live
            || self
                .idle_secs
                .is_some_and(|s| s < NO_PR_RECENT_DAYS * 86_400)
    }
}

impl HideRule {
    pub const ALL: [HideRule; 6] = [
        HideRule::Pr(PrCriterion::Merged),
        HideRule::Pr(PrCriterion::Closed),
        HideRule::Pr(PrCriterion::NoPr),
        HideRule::Pr(PrCriterion::Draft),
        HideRule::Pr(PrCriterion::Open),
        HideRule::Pr(PrCriterion::Conflicted),
    ];

    pub fn token(self) -> &'static str {
        match self {
            HideRule::Pr(PrCriterion::Merged) => "pr:merged",
            HideRule::Pr(PrCriterion::Closed) => "pr:closed",
            HideRule::Pr(PrCriterion::NoPr) => "pr:none",
            HideRule::Pr(PrCriterion::Draft) => "pr:draft",
            HideRule::Pr(PrCriterion::Open) => "pr:open",
            HideRule::Pr(PrCriterion::Conflicted) => "pr:conflicted",
        }
    }

    pub fn from_token(s: &str) -> Option<HideRule> {
        HideRule::ALL.into_iter().find(|r| r.token() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            HideRule::Pr(PrCriterion::Merged) => "merged",
            HideRule::Pr(PrCriterion::Closed) => "closed",
            HideRule::Pr(PrCriterion::NoPr) => "no PR",
            HideRule::Pr(PrCriterion::Draft) => "draft",
            HideRule::Pr(PrCriterion::Open) => "open",
            HideRule::Pr(PrCriterion::Conflicted) => "conflicted",
        }
    }

    /// The lifecycle a PR rule stands for, for glyph and color. `None` for
    /// non-PR rules once they exist.
    pub fn lifecycle(self) -> Option<BranchLifecycle> {
        let HideRule::Pr(c) = self;
        Some(match c {
            PrCriterion::Merged => BranchLifecycle::PrMerged,
            PrCriterion::Closed => BranchLifecycle::PrClosed,
            PrCriterion::NoPr => BranchLifecycle::NoPr,
            PrCriterion::Draft => BranchLifecycle::PrDraft,
            PrCriterion::Open => BranchLifecycle::PrOpen,
            PrCriterion::Conflicted => BranchLifecycle::PrConflicted,
        })
    }

    /// An unresolved lifecycle never matches, so rows don't blink out and
    /// back in while PR polling warms up. `pr:none` spares in-progress work.
    pub fn matches(self, s: &HideSubject) -> bool {
        match self {
            HideRule::Pr(PrCriterion::NoPr) => {
                s.lifecycle == Some(BranchLifecycle::NoPr) && !s.in_progress()
            }
            HideRule::Pr(_) => s.lifecycle.is_some() && s.lifecycle == self.lifecycle(),
        }
    }
}

/// A repo's rule set: deduplicated, in canonical order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HideRules(Vec<HideRule>);

impl HideRules {
    pub fn parse(raw: Option<&str>) -> Self {
        let mut rules: Vec<HideRule> = raw
            .unwrap_or("")
            .split(',')
            .filter_map(|t| HideRule::from_token(t.trim()))
            .collect();
        rules.sort();
        rules.dedup();
        HideRules(rules)
    }

    /// `None` when empty, so the column stores NULL rather than "".
    pub fn serialize(&self) -> Option<String> {
        if self.0.is_empty() {
            return None;
        }
        Some(
            self.0
                .iter()
                .map(|r| r.token())
                .collect::<Vec<_>>()
                .join(","),
        )
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, rule: HideRule) -> bool {
        self.0.contains(&rule)
    }

    pub fn toggle(&mut self, rule: HideRule) {
        if let Some(i) = self.0.iter().position(|r| *r == rule) {
            self.0.remove(i);
        } else {
            self.0.push(rule);
            self.0.sort();
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = HideRule> + '_ {
        self.0.iter().copied()
    }

    /// The first rule hiding `s`. First-match keeps group counts disjoint
    /// once overlapping criteria exist.
    pub fn classify(&self, s: &HideSubject) -> Option<HideRule> {
        self.iter().find(|r| r.matches(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::forge::BranchLifecycle;

    fn subject(lc: Option<BranchLifecycle>) -> HideSubject {
        HideSubject {
            lifecycle: lc,
            ..Default::default()
        }
    }

    const NO_PR: HideRule = HideRule::Pr(PrCriterion::NoPr);
    const DAY: u64 = 86_400;

    fn no_pr() -> HideSubject {
        subject(Some(BranchLifecycle::NoPr))
    }

    #[test]
    fn no_pr_hides_when_nothing_is_in_progress() {
        assert!(NO_PR.matches(&no_pr()));
        let stale = HideSubject {
            idle_secs: Some(NO_PR_RECENT_DAYS * DAY),
            ..no_pr()
        };
        assert!(NO_PR.matches(&stale));
    }

    #[test]
    fn no_pr_spares_work_in_progress() {
        let s = HideSubject {
            work_in_progress: true,
            ..no_pr()
        };
        assert!(!NO_PR.matches(&s));
    }

    #[test]
    fn no_pr_spares_a_live_agent() {
        let s = HideSubject {
            agent_live: true,
            ..no_pr()
        };
        assert!(!NO_PR.matches(&s));
    }

    #[test]
    fn no_pr_spares_recent_activity() {
        let s = HideSubject {
            idle_secs: Some(NO_PR_RECENT_DAYS * DAY - 1),
            ..no_pr()
        };
        assert!(!NO_PR.matches(&s));
    }

    #[test]
    fn other_pr_rules_ignore_activity() {
        let busy = HideSubject {
            lifecycle: Some(BranchLifecycle::PrMerged),
            work_in_progress: true,
            agent_live: true,
            idle_secs: Some(0),
        };
        assert!(HideRule::Pr(PrCriterion::Merged).matches(&busy));
    }

    #[test]
    fn tokens_round_trip() {
        for rule in HideRule::ALL {
            assert_eq!(HideRule::from_token(rule.token()), Some(rule));
        }
    }

    #[test]
    fn parse_skips_unknown_dedupes_and_orders_canonically() {
        let rules = HideRules::parse(Some(" pr:none, idle:14d ,pr:merged,pr:none"));
        assert_eq!(
            rules.iter().collect::<Vec<_>>(),
            vec![
                HideRule::Pr(PrCriterion::Merged),
                HideRule::Pr(PrCriterion::NoPr)
            ]
        );
        assert_eq!(rules.serialize().as_deref(), Some("pr:merged,pr:none"));
    }

    #[test]
    fn empty_and_null_parse_to_no_rules_and_serialize_to_none() {
        assert!(HideRules::parse(None).is_empty());
        assert!(HideRules::parse(Some("")).is_empty());
        assert_eq!(HideRules::default().serialize(), None);
    }

    #[test]
    fn toggle_adds_then_removes_and_keeps_order() {
        let mut rules = HideRules::default();
        rules.toggle(HideRule::Pr(PrCriterion::NoPr));
        rules.toggle(HideRule::Pr(PrCriterion::Merged));
        assert_eq!(rules.serialize().as_deref(), Some("pr:merged,pr:none"));
        rules.toggle(HideRule::Pr(PrCriterion::Merged));
        assert_eq!(rules.serialize().as_deref(), Some("pr:none"));
    }

    #[test]
    fn pr_rule_matches_only_its_lifecycle() {
        let merged = HideRule::Pr(PrCriterion::Merged);
        assert!(merged.matches(&subject(Some(BranchLifecycle::PrMerged))));
        assert!(!merged.matches(&subject(Some(BranchLifecycle::PrOpen))));
        let none = HideRule::Pr(PrCriterion::NoPr);
        assert!(none.matches(&subject(Some(BranchLifecycle::NoPr))));
    }

    #[test]
    fn none_lifecycle_never_matches() {
        for rule in HideRule::ALL {
            assert!(!rule.matches(&subject(None)), "{rule:?}");
        }
    }

    #[test]
    fn classify_returns_first_matching_rule() {
        let rules = HideRules::parse(Some("pr:merged,pr:none"));
        assert_eq!(
            rules.classify(&subject(Some(BranchLifecycle::PrMerged))),
            Some(HideRule::Pr(PrCriterion::Merged))
        );
        assert_eq!(
            rules.classify(&subject(Some(BranchLifecycle::PrOpen))),
            None
        );
    }
}
