//! Per-repo rules that hide dashboard workspaces. Persisted in
//! `repos.hide_rules` as namespaced tokens (`tag:parked,pr:merged`) so future
//! criteria (`idle:14d`) can't collide; unknown tokens are skipped.

use crate::git::forge::BranchLifecycle;
use std::borrow::Cow;
use std::sync::Arc;

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HideRule {
    /// Declared first: derived `Ord` puts tag rules ahead of PR rules.
    Tag(Arc<str>),
    Pr(PrCriterion),
}

/// A no-PR workspace active within this many days stays visible.
pub const NO_PR_RECENT_DAYS: u64 = 3;

/// The facts a rule can test about a workspace. Grows a field per criterion family.
#[derive(Debug, Clone, Default)]
pub struct HideSubject {
    pub lifecycle: Option<BranchLifecycle>,
    /// Commits ahead of the base branch, or uncommitted changes.
    pub work_in_progress: bool,
    /// A running agent session, or a fresh wsx status of working/waiting/blocked.
    pub agent_live: bool,
    /// Seconds since last activity or creation; `None` when neither is known.
    pub idle_secs: Option<u64>,
    /// The workspace's tags, sorted.
    pub tags: Vec<Arc<str>>,
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
    pub const PR_RULES: [HideRule; 6] = [
        HideRule::Pr(PrCriterion::Merged),
        HideRule::Pr(PrCriterion::Closed),
        HideRule::Pr(PrCriterion::NoPr),
        HideRule::Pr(PrCriterion::Draft),
        HideRule::Pr(PrCriterion::Open),
        HideRule::Pr(PrCriterion::Conflicted),
    ];

    /// A tag rule; `name` must already be normalized.
    pub fn tag(name: &str) -> HideRule {
        HideRule::Tag(Arc::from(name))
    }

    pub fn token(&self) -> Cow<'static, str> {
        let c = match self {
            HideRule::Tag(t) => return Cow::Owned(format!("tag:{t}")),
            HideRule::Pr(c) => c,
        };
        Cow::Borrowed(match c {
            PrCriterion::Merged => "pr:merged",
            PrCriterion::Closed => "pr:closed",
            PrCriterion::NoPr => "pr:none",
            PrCriterion::Draft => "pr:draft",
            PrCriterion::Open => "pr:open",
            PrCriterion::Conflicted => "pr:conflicted",
        })
    }

    pub fn from_token(s: &str) -> Option<HideRule> {
        if let Some(name) = s.strip_prefix("tag:") {
            return crate::data::tags::normalize_tag(name)
                .filter(|n| n == name)
                .map(|n| HideRule::tag(&n));
        }
        HideRule::PR_RULES.into_iter().find(|r| r.token() == s)
    }

    pub fn label(&self) -> Cow<'static, str> {
        let c = match self {
            HideRule::Tag(t) => return Cow::Owned(format!("#{t}")),
            HideRule::Pr(c) => c,
        };
        Cow::Borrowed(match c {
            PrCriterion::Merged => "merged",
            PrCriterion::Closed => "closed",
            PrCriterion::NoPr => "no PR",
            PrCriterion::Draft => "draft",
            PrCriterion::Open => "open",
            PrCriterion::Conflicted => "conflicted",
        })
    }

    /// The lifecycle a PR rule stands for, for glyph and color; `None` for tags.
    pub fn lifecycle(&self) -> Option<BranchLifecycle> {
        let HideRule::Pr(c) = self else {
            return None;
        };
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
    /// back in while PR polling warms up. `pr:none` spares in-progress work;
    /// tag rules don't, since tagging is deliberate.
    pub fn matches(&self, s: &HideSubject) -> bool {
        match self {
            HideRule::Tag(t) => s.tags.contains(t),
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

    pub fn contains(&self, rule: &HideRule) -> bool {
        self.0.contains(rule)
    }

    pub fn toggle(&mut self, rule: HideRule) {
        if let Some(i) = self.0.iter().position(|r| *r == rule) {
            self.0.remove(i);
        } else {
            self.0.push(rule);
            self.0.sort();
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &HideRule> {
        self.0.iter()
    }

    /// The first rule hiding `s`. First-match keeps group counts disjoint
    /// once overlapping criteria exist.
    pub fn classify(&self, s: &HideSubject) -> Option<HideRule> {
        self.iter().find(|r| r.matches(s)).cloned()
    }

    /// Whether any rule hides `s`, where carrying any `shown` tag exempts it
    /// from every tag rule. PR rules still apply, so finished work drops out.
    pub fn hides_unless_shown(&self, s: &HideSubject, shown: &[Arc<str>]) -> bool {
        let exempt = s.tags.iter().any(|t| shown.contains(t));
        self.iter()
            .any(|r| !(exempt && matches!(r, HideRule::Tag(_))) && r.matches(s))
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
            ..Default::default()
        };
        assert!(HideRule::Pr(PrCriterion::Merged).matches(&busy));
    }

    fn tagged(tags: &[&str]) -> HideSubject {
        HideSubject {
            lifecycle: Some(BranchLifecycle::PrMerged),
            tags: tags.iter().map(|t| std::sync::Arc::from(*t)).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn tag_token_round_trips() {
        let r = HideRule::tag("parked");
        assert_eq!(r.token(), "tag:parked");
        assert_eq!(HideRule::from_token("tag:parked"), Some(r));
    }

    #[test]
    fn invalid_tag_tokens_are_skipped() {
        let rules = HideRules::parse(Some("tag:Bad Name,tag:,tag:ok"));
        assert_eq!(rules.serialize().as_deref(), Some("tag:ok"));
    }

    #[test]
    fn tag_rules_sort_before_pr_rules_alphabetically() {
        let rules = HideRules::parse(Some("pr:merged,tag:zeta,tag:alpha"));
        assert_eq!(
            rules.serialize().as_deref(),
            Some("tag:alpha,tag:zeta,pr:merged")
        );
    }

    #[test]
    fn tag_rule_matches_only_tagged_workspaces_and_ignores_activity() {
        let r = HideRule::tag("parked");
        let busy = HideSubject {
            work_in_progress: true,
            agent_live: true,
            ..tagged(&["parked"])
        };
        assert!(r.matches(&busy));
        assert!(!r.matches(&tagged(&["spike"])));
    }

    #[test]
    fn classify_prefers_tag_rule_over_pr_rule() {
        let rules = HideRules::parse(Some("pr:merged,tag:parked"));
        assert_eq!(
            rules.classify(&tagged(&["parked"])),
            Some(HideRule::tag("parked"))
        );
    }

    #[test]
    fn tag_label_and_lifecycle() {
        let r = HideRule::tag("parked");
        assert_eq!(r.label(), "#parked");
        assert_eq!(r.lifecycle(), None);
    }

    #[test]
    fn tokens_round_trip() {
        for rule in HideRule::PR_RULES {
            assert_eq!(HideRule::from_token(&rule.token()), Some(rule));
        }
    }

    #[test]
    fn parse_skips_unknown_dedupes_and_orders_canonically() {
        let rules = HideRules::parse(Some(" pr:none, idle:14d ,pr:merged,pr:none"));
        assert_eq!(
            rules.iter().cloned().collect::<Vec<_>>(),
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
        for rule in HideRule::PR_RULES {
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

    #[test]
    fn shown_tag_exempts_from_tag_rules_but_not_pr_rules() {
        let rules = HideRules::parse(Some("tag:support,tag:epics,pr:merged"));
        let shown: Vec<Arc<str>> = vec![Arc::from("support")];
        let tagged = |lc, tags: &[&str]| HideSubject {
            lifecycle: lc,
            tags: tags.iter().map(|t| Arc::from(*t)).collect(),
            ..Default::default()
        };
        let open = Some(BranchLifecycle::PrOpen);
        assert!(!rules.hides_unless_shown(&tagged(open, &["support"]), &shown));
        assert!(!rules.hides_unless_shown(&tagged(open, &["epics", "support"]), &shown));
        assert!(rules.hides_unless_shown(&tagged(open, &["epics"]), &shown));
        assert!(rules.hides_unless_shown(
            &tagged(Some(BranchLifecycle::PrMerged), &["support"]),
            &shown
        ));
        assert!(!rules.hides_unless_shown(&tagged(open, &[]), &shown));
        assert!(rules.hides_unless_shown(&tagged(open, &["support"]), &[]));
    }
}
