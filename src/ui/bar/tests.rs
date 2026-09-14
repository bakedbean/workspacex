//! Tests for the bar composers in `bars.rs`.

use super::bars::*;
use super::format;
use super::render::Rendered;
use super::segment::Hit;
use super::test_util;
use crate::ui::theme::Theme;

#[cfg(test)]
mod attention_budget_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;
    use crate::pty::session::AgentKind;

    fn inputs<'a>(repo: &'a str, name: &'a str, agent: Option<AgentKind>) -> AttachedInputs<'a> {
        AttachedInputs {
            repo,
            name,
            version: "0.1.0",
            window_label: "24h",
            activity: &[],
            agent,
            attention: None,
            pinned: &[],
            procs: 0,
            diff: None,
            pr: None,
            model_tokens: None,
            agents: &[],
            active_agent: None,
        }
    }

    /// The prefix the deleted `info_line_prefix_width` hardcoded: the
    /// agent bar (`▎` + a space) when present, the label in CELLS, and the
    /// stock format's 3-column gap before the attention items.
    fn stock_prefix(label: &str, agent: Option<AgentKind>) -> usize {
        let bar = if agent.is_some() { 2 } else { 0 };
        bar + ratatui::text::Span::raw(label).width() + 3
    }

    /// Parity with the stock prefix: for the bundled default the probe
    /// measures exactly `▎ ` + label + the 3-column gap, with and without
    /// an agent bar. (The old call site then subtracted a further 3 as an
    /// unexplained right margin; the probe measures what the bar actually
    /// draws, so those 3 columns are now available to the items.)
    #[test]
    fn bundled_default_budget_matches_the_stock_prefix() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        for agent in [Some(AgentKind::Claude), None] {
            let budget = attention_width_budget(&specs, &theme, inputs("wsx", "foo", agent), 80);
            assert_eq!(
                budget,
                80 - stock_prefix("wsx/foo", agent),
                "agent={agent:?}"
            );
        }
    }

    /// Cells, not chars: "日本" is 2 chars but 4 cells, so a wide label
    /// must cost the budget 2 more columns than a 2-cell one. (Moved from
    /// `info_line_prefix_width_counts_cells_not_chars`.)
    #[test]
    fn budget_counts_label_cells_not_chars() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let wide = attention_width_budget(
            &specs,
            &theme,
            inputs("r", "日本", Some(AgentKind::Claude)),
            80,
        );
        let narrow = attention_width_budget(
            &specs,
            &theme,
            inputs("r", "ab", Some(AgentKind::Claude)),
            80,
        );
        assert_eq!(wide, narrow - 2);
    }

    /// A custom format gets its own measurement: `[ $workspace ](bg:blue)`
    /// is `label + 2` cells and `( $attention)` contributes its leading
    /// space, so the items get `width - (label + 2) - 1`. The old
    /// hardcoded prefix would have claimed 3 columns that this format
    /// never draws, overrunning the bar.
    #[test]
    fn custom_format_budget_follows_that_format() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("[ $workspace ](bg:blue)( $attention)").unwrap();
        let label_width = "wsx/foo".len();
        let budget = attention_width_budget(
            &specs,
            &theme,
            inputs("wsx", "foo", Some(AgentKind::Claude)),
            80,
        );
        assert_eq!(budget, 80 - (label_width + 2) - 1);
    }

    /// A bar wider than the terminal leaves no room at all rather than
    /// underflowing.
    #[test]
    fn budget_saturates_at_zero_on_a_narrow_bar() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        assert_eq!(
            attention_width_budget(&specs, &theme, inputs("wsx", "foo", None), 4),
            0
        );
    }

    /// `$attention` in `attached_top.right_format`, with a nonempty left
    /// side (`$workspace`): the chrome is the probe side's own width minus
    /// its one probe cell, plus the OTHER (left) side's full width plus the
    /// mandatory blank between them — mirroring `render_bar`'s accounting
    /// for a nonempty pair of sides, just on the right-side probe instead
    /// of the left.
    #[test]
    fn attention_in_a_right_format_accounts_for_the_other_sides_width_too() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("$workspace").unwrap();
        specs.attached_top.right_format = format::parse("( $attention)").unwrap();
        let label_width = "wsx/foo".len();
        let budget = attention_width_budget(
            &specs,
            &theme,
            inputs("wsx", "foo", Some(AgentKind::Claude)),
            80,
        );
        // Probe side (right): "( $attention)" -> " x", 2 cells, minus the
        // 1 probe cell = 1. Other side (left): "wsx/foo" (label_width) + 1
        // mandatory blank.
        assert_eq!(budget, 80 - 1 - (label_width + 1));
    }

    /// `$attention` placed in `attached_bottom.format` (not the top bar)
    /// measures the BOTTOM bar's own right side — here the stock
    /// `right_format`, rendering a real PR chip — proving the budget
    /// follows placement across bars, not just within `attached_top`.
    #[test]
    fn attention_in_the_bottom_bars_format_uses_the_bottom_bars_own_right_side() {
        use crate::git::forge::BranchLifecycle;
        use crate::ui::attached::ChipPr;

        fn pr_input(base: AttachedInputs<'_>) -> AttachedInputs<'_> {
            AttachedInputs {
                pr: Some(ChipPr {
                    lifecycle: BranchLifecycle::PrOpen,
                    number: 42,
                    review: None,
                    unresolved: None,
                }),
                ..base
            }
        }

        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        // No placement in the top bar at all.
        specs.attached_top.format = format::parse("$workspace").unwrap();
        // Attention lives in the bottom bar's `format`; `right_format`
        // stays the stock one, which draws a PR chip when `pr` is set.
        specs.attached_bottom.format = format::parse("( $attention)").unwrap();

        // Measure the PR chip's own rendered width independently, via a
        // wide, non-dropping render of the bottom bar's hits.
        let bars = attached_bars(
            &specs,
            &theme,
            pr_input(inputs("wsx", "foo", None)),
            200,
            200,
        );
        let pr_hit = bars
            .bottom
            .hits
            .iter()
            .find(|h| h.hit == Hit::Pr)
            .expect("pr chip rendered");
        let pr_width = usize::from(pr_hit.width);

        let budget =
            attention_width_budget(&specs, &theme, pr_input(inputs("wsx", "foo", None)), 80);

        // Probe side (left, `format`): "( $attention)" -> " x", 2 cells,
        // minus the 1 probe cell = 1. Other side (right): the stock
        // `right_format`'s leading literal space plus the PR chip's own
        // width, plus 1 mandatory blank.
        assert_eq!(budget, 80 - 1 - (1 + pr_width + 1));
    }

    /// `$attention` absent from all four attached-bar format strings:
    /// nothing constrains the items, so the full width is available.
    #[test]
    fn attention_absent_everywhere_leaves_the_full_width() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("$workspace").unwrap();
        let budget = attention_width_budget(&specs, &theme, inputs("wsx", "foo", None), 80);
        assert_eq!(budget, 80);
    }
}

#[cfg(test)]
mod attached_bars_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;

    /// A segment normally in the bottom bar (`keys`, the `^x menu` pill)
    /// renders — and keeps its hit — when the theme puts it in the top
    /// bar's format instead: proof the two bars share one segment map.
    #[test]
    fn bottom_bar_segment_renders_in_top_bar_with_its_hit() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.attached_top.format = format::parse("$keys").unwrap();
        let bars = attached_bars(
            &specs,
            &theme,
            AttachedInputs {
                repo: "wsx",
                name: "foo",
                version: "0.1.0",
                window_label: "24h",
                activity: &[],
                agent: None,
                attention: None,
                pinned: &[],
                procs: 0,
                diff: None,
                pr: None,
                model_tokens: None,
                agents: &[],
                active_agent: None,
            },
            60,
            60,
        );
        assert!(
            test_util::plain(&bars.top.line).starts_with(" ^x  menu"),
            "{:?}",
            test_util::plain(&bars.top.line)
        );
        assert_eq!(bars.top.hits[0].start_col, 0);
        assert_eq!(bars.top.hits[0].hit, Hit::ArmLeader);
    }
}

#[cfg(test)]
mod footer_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;
    use crate::ui::bar::test_util::{plain, render_line};
    use crossterm::event::KeyCode;

    fn footer(selected: bool, label: &str, width: u16) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let activity: Vec<u32> = (0..24).collect();
        dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &activity,
                version: "0.1.0",
                window_label: label,
                workspace_selected: selected,
            },
            width,
        )
    }

    #[test]
    fn default_footer_snapshot() {
        let out = footer(true, "24h", 120);
        let text = plain(&out.line);
        assert!(
            text.starts_with(
                " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   ?  actions   q  quit"
            ),
            "{text:?}"
        );
        let spark = crate::ui::dashboard::sparkline::render(&(0..24).collect::<Vec<u32>>(), 24);
        assert!(text.ends_with(&format!("0.1.0  24h {spark}")), "{text:?}");
        assert_eq!(out.line.width(), 120);
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            8
        );
    }

    #[test]
    fn footer_omits_actions_pill_without_workspace() {
        assert!(!plain(&footer(false, "24h", 120).line).contains("actions"));
        assert!(plain(&footer(true, "24h", 120).line).contains("actions"));
    }

    #[test]
    fn footer_key_pill_wraps_key_only_not_label() {
        let theme = Theme::wsx();
        let out = footer(true, "24h", 120);
        let buf = render_line(&out.line, 120);
        // " ↑↓ " is cols 0..4 on the chip bg; " nav" follows on the bar bg.
        assert_eq!(buf[(1, 0)].bg, theme.bg_soft);
        assert_eq!(buf[(5, 0)].symbol(), "n");
        assert_ne!(buf[(5, 0)].bg, theme.bg_soft);
    }

    #[test]
    fn footer_hints_align_with_rendered_key_pills() {
        let out = footer(true, "24h", 120);
        let buf = render_line(&out.line, 120);
        let order = out
            .hits
            .iter()
            .find(|h| matches!(h.hit, Hit::Key(k) if k.code == KeyCode::Char('o')))
            .expect("order hint");
        let cells: String = (order.start_col..order.start_col + order.width)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        assert_eq!(cells, " o  order");
    }

    #[test]
    fn footer_usage_hit_covers_label_and_sparkline() {
        let out = footer(true, "1w", 120);
        let usage = out.hits.iter().find(|h| h.hit == Hit::UsageGraph).unwrap();
        assert_eq!(usage.width, 2 + 1 + 24);
        assert_eq!(usage.start_col + usage.width, 120);
    }

    // At 100 columns the full content (keys 71 + gap 1 + version 5 + "  " 2
    // + usage 28 = 107, without the actions pill) doesn't fit. `version`'s
    // lower priority drops it first: keys 71 + gap 1 + usage 28 = 100 fits
    // exactly, so the usage graph survives and lands flush against the
    // right edge. `workspace_selected: false` (no `actions` pill) — with
    // it, keys alone are already 84 wide, leaving no room for usage either,
    // so this specifically exercises version-drops-before-usage rather than
    // everything-drops.
    #[test]
    fn narrow_footer_drops_version_before_usage() {
        let out = footer(false, "24h", 100);
        let text = plain(&out.line);
        assert!(!text.contains("0.1.0"), "{text:?}");
        let spark = crate::ui::dashboard::sparkline::render(&(0..24).collect::<Vec<u32>>(), 24);
        assert!(text.ends_with(&format!("24h {spark}")), "{text:?}");
        assert_eq!(out.line.width(), 100);
        let usage = out.hits.iter().find(|h| h.hit == Hit::UsageGraph).unwrap();
        assert_eq!(usage.start_col + usage.width, 100);

        // Narrower still (60): even usage alone no longer fits, so both
        // right-side segments are gone — but the left side is never
        // dropped, only clipped by whatever renders the (now wider than
        // requested) line.
        let out = footer(false, "24h", 60);
        let text = plain(&out.line);
        assert!(!text.contains("0.1.0"), "{text:?}");
        assert!(!text.contains("24h"), "{text:?}");
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            7
        );
    }

    /// Durable evidence for fix round 2: these two strings were verified
    /// byte-for-byte against the pre-Task-7 legacy footer builder
    /// (temporarily restored in git history for that one check, then
    /// removed again — see the `engine_footer_matches_legacy_footer` commit
    /// history) before this test was written. Pinning them here means a
    /// future change to the bundled default's overflow priorities gets
    /// caught without needing to resurrect the legacy code again.
    ///
    /// `workspace_selected: false` fits its full content at 110 (keys 71 +
    /// gap 4 + version 5 + "  " 2 + usage 28 = 110) without dropping
    /// anything, so it matches legacy exactly. `workspace_selected: true`
    /// does NOT: with the `actions` pill, keys alone are 84 wide, leaving
    /// only 110 - 84 - 1 = 25 cells for the right side — 3 short of even
    /// `usage` alone (28) — so both `version` and `usage` drop and legacy
    /// parity does not apply (legacy has no such drop and would overflow
    /// to 120 cells instead); this asserts the engine's own, intentional
    /// behavior at that width.
    #[test]
    fn default_footer_snapshot_at_110() {
        let spark = crate::ui::dashboard::sparkline::render(&(0..24).collect::<Vec<u32>>(), 24);

        let out = footer(false, "24h", 110);
        let expected = format!(
            "{}{}0.1.0  24h {spark}",
            " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   q  quit",
            " ".repeat(4),
        );
        assert_eq!(plain(&out.line), expected);
        assert_eq!(out.line.width(), 110);
        let usage = out.hits.iter().find(|h| h.hit == Hit::UsageGraph).unwrap();
        assert_eq!(usage.start_col + usage.width, 110);
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            7
        );

        let out = footer(true, "24h", 110);
        let expected = format!(
            "{}{}",
            " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   ?  actions   q  quit",
            " ".repeat(26)
        );
        assert_eq!(plain(&out.line), expected);
        assert_eq!(out.line.width(), 110);
        assert!(out.hits.iter().all(|h| h.hit != Hit::UsageGraph));
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            8
        );
    }
}

#[cfg(test)]
mod bottom_tests {
    use super::*;
    use crate::commands::pinned::PinnedCommand;
    use crate::config::theme_file::bundled_default;
    use crate::data::store::AgentInstanceId;
    use crate::git::forge::{BranchLifecycle, ReviewDecision};
    use crate::pty::session::AgentKind;
    use crate::ui::attached::ChipPr;
    use crate::ui::bar::test_util::{plain, render_line};
    use crate::ui::detail_modules::session_summary::ChipModelTokens;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs
            .iter()
            .map(|(l, c)| PinnedCommand {
                label: (*l).into(),
                command: (*c).into(),
            })
            .collect()
    }
    fn pr(review: Option<ReviewDecision>) -> Option<ChipPr> {
        Some(ChipPr {
            lifecycle: BranchLifecycle::PrOpen,
            number: 42,
            review,
            unresolved: None,
        })
    }
    fn mt() -> Option<ChipModelTokens> {
        Some(ChipModelTokens {
            model: Some("opus 4.8".into()),
            tokens: "45k/200k".into(),
            warn: false,
        })
    }
    fn agents() -> Vec<(AgentInstanceId, AgentKind, String, Option<char>)> {
        vec![
            (
                AgentInstanceId(1),
                AgentKind::Claude,
                "claude".into(),
                Some('q'),
            ),
            (
                AgentInstanceId(2),
                AgentKind::Codex,
                "codex".into(),
                Some('w'),
            ),
        ]
    }
    fn render(inputs: AttachedInputs<'_>, width: u16) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        attached_bars(&specs, &theme, inputs, width, width).bottom
    }
    fn full<'a>(
        pinned: &'a [PinnedCommand],
        agents: &'a [(AgentInstanceId, AgentKind, String, Option<char>)],
    ) -> AttachedInputs<'a> {
        AttachedInputs {
            repo: "wsx",
            name: "foo",
            version: "0.1.0",
            window_label: "24h",
            activity: &[],
            agent: None,
            attention: None,
            pinned,
            procs: 3,
            diff: Some(crate::git::DiffStats {
                added: 12,
                removed: 3,
            }),
            pr: pr(Some(ReviewDecision::Approved)),
            model_tokens: mt(),
            agents,
            active_agent: Some(AgentInstanceId(1)),
        }
    }
    fn present(out: &Rendered) -> Vec<&'static str> {
        let t = plain(&out.line);
        let mut v = Vec::new();
        if t.contains("claude") {
            v.push("agents");
        }
        if t.contains("45k/200k") {
            v.push("model_tokens");
        }
        if t.contains("3p") {
            v.push("procs");
        }
        if t.contains("+12") {
            v.push("diff");
        }
        if t.contains("#42") {
            v.push("pr");
        }
        v
    }

    #[test]
    fn default_bottom_snapshot_and_hits() {
        let pinned = cmds(&[("PR", "/pr"), ("feedback", "/fb")]);
        let agents = agents();
        let out = render(full(&pinned, &agents), 120);
        let t = plain(&out.line);
        // keys ` ^x  menu`, two literal spaces, chips ` 1  PR` and ` 2  feedback`
        // (pill pad + `index` pad + space-led label), two spaces, then the rule.
        assert!(
            t.starts_with(" ^x  menu   1  PR   2  feedback  ──"),
            "{t:?}"
        );
        // Each agent pill ends with its ` q ` key pill (trailing pad), then the
        // 3-cell separator / group gap, so four spaces precede `○` and `opus`.
        assert!(
            t.ends_with("● claude  q    ○ codex  w    opus 4.8 45k/200k ● 3p +12 −3 ⏺ #42 open ✓"),
            "{t:?}"
        );
        assert_eq!(out.line.width(), 120);
        let chips: Vec<_> = out
            .hits
            .iter()
            .filter(|h| matches!(h.hit, Hit::PinnedChip(_)))
            .collect();
        assert_eq!(chips.len(), 2);
        assert_eq!(chips[0].width, 6, "` 1  PR` is 6 cells");
        assert_eq!(chips[1].width, 12, "` 2  feedback` is 12 cells");
        assert_eq!(chips[1].start_col, chips[0].start_col + 6 + 2, "2-cell gap");
        let leader = out.hits.iter().find(|h| h.hit == Hit::ArmLeader).unwrap();
        assert_eq!((leader.start_col, leader.width), (0, 9));
        let pr = out.hits.iter().find(|h| h.hit == Hit::Pr).unwrap();
        assert_eq!(pr.start_col + pr.width, 120, "PR chip hugs the right edge");
        let ids: Vec<_> = out
            .hits
            .iter()
            .filter_map(|h| match h.hit {
                Hit::Agent(id) => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(ids, vec![AgentInstanceId(1), AgentInstanceId(2)]);
    }

    #[test]
    fn zero_procs_and_clean_diff_render_nothing() {
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.procs = 0;
        inputs.diff = Some(crate::git::DiffStats {
            added: 0,
            removed: 0,
        });
        let out = render(inputs, 120);
        assert_eq!(present(&out), vec!["model_tokens", "pr"]);
        assert!(out.hits.iter().all(|h| h.hit != Hit::Procs));
    }

    #[test]
    fn pr_mark_is_absent_without_a_verdict() {
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.pr = pr(None);
        let t = plain(&render(inputs, 120).line);
        assert!(t.ends_with("⏺ #42 open"), "{t:?}");
    }

    #[test]
    fn narrow_rows_drop_model_tokens_then_agents_then_procs_then_diff() {
        let pinned = cmds(&[("PR", "/pr")]);
        let agents = agents();
        // Left side is 19 cells (` ^x  menu` + 2 + ` 1  PR` + 2). Right side at
        // full strength is 73: 2 + agents 26 + 3 + model 17 + 1 + procs 4 + 1
        // + diff 6 + 1 + pr 12. Dropping model removes 18, agents 29, procs 5,
        // diff 7.
        let widths_and_expect: [(u16, &[&str]); 5] = [
            (120, &["agents", "model_tokens", "procs", "diff", "pr"]),
            (80, &["agents", "procs", "diff", "pr"]),
            (50, &["procs", "diff", "pr"]),
            (40, &["diff", "pr"]),
            (35, &["pr"]),
        ];
        for (w, expect) in widths_and_expect {
            let out = render(full(&pinned, &agents), w);
            assert_eq!(
                present(&out),
                expect.to_vec(),
                "width {w}: {:?}",
                plain(&out.line)
            );
        }
    }

    #[test]
    fn model_tokens_warn_style_is_the_warn_color() {
        let theme = Theme::wsx();
        let pinned = cmds(&[]);
        let agents = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.model_tokens = Some(ChipModelTokens {
            model: None,
            tokens: "190k/200k".into(),
            warn: true,
        });
        inputs.pr = None;
        inputs.diff = None;
        inputs.procs = 0;
        let out = render(inputs, 60);
        let buf = render_line(&out.line, 60);
        // No PR: the accepted trailing-blank defect (see the theme comment)
        // lands at the very last cell; the tokens' own last character sits
        // one cell in from it.
        assert_eq!(buf[(58, 0)].symbol(), "k");
        assert_eq!(buf[(58, 0)].fg, theme.warn);
        assert_eq!(buf[(59, 0)].symbol(), " ");
    }

    /// A single-agent workspace renders no agent pills (`agents` needs 2+ to
    /// show any), so `model_tokens` becomes the first element of the
    /// flush-right block. With trailing per-element separators, a missing
    /// element takes its own gap with it — the rule is followed by exactly
    /// the stock two-cell gap, not the 4-cell gap that would land after
    /// agents specifically (3-cell group gap + 1-cell model_tokens gap).
    #[test]
    fn single_agent_stats_block_sits_two_cells_after_the_rule() {
        let pinned = cmds(&[]);
        let agents: Vec<(AgentInstanceId, AgentKind, String, Option<char>)> = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.agents = &[];
        inputs.active_agent = None;
        let out = render(inputs, 120);
        let t = plain(&out.line);
        assert!(t.contains("──  opus 4.8 45k/200k"), "{t:?}");
        assert!(!t.contains("   opus"), "{t:?}");
    }

    /// The accepted tradeoff of trailing separators: `$pr` is bare (no
    /// trailing separator of its own), so a workspace with no PR leaves the
    /// diff count's own trailing gap as one dangling blank cell at the very
    /// right edge, instead of hugging it exactly.
    #[test]
    fn no_pr_leaves_only_a_trailing_blank() {
        let pinned = cmds(&[]);
        let agents: Vec<(AgentInstanceId, AgentKind, String, Option<char>)> = vec![];
        let mut inputs = full(&pinned, &agents);
        inputs.agents = &[];
        inputs.active_agent = None;
        inputs.pr = None;
        let out = render(inputs, 120);
        let t = plain(&out.line);
        assert!(t.ends_with("+12 −3 "), "{t:?}");
        assert_eq!(out.line.width(), 120);
    }

    /// `[pr].symbol` overrides the lifecycle glyph the provider would
    /// otherwise use — the amendment to Task 8/9's shared-segment-map plan.
    #[test]
    fn pr_symbol_overrides_lifecycle_glyph() {
        let theme = Theme::wsx();
        let mut specs = bundled_default(&theme);
        specs.segments.get_mut("pr").unwrap().symbol = Some("X".into());
        let pinned = cmds(&[]);
        let agents = vec![];
        let inputs = AttachedInputs {
            repo: "wsx",
            name: "foo",
            version: "0.1.0",
            window_label: "24h",
            activity: &[],
            agent: None,
            attention: None,
            pinned: &pinned,
            procs: 0,
            diff: None,
            pr: pr(None),
            model_tokens: None,
            agents: &agents,
            active_agent: None,
        };
        let out = attached_bars(&specs, &theme, inputs, 120, 120).bottom;
        let t = plain(&out.line);
        assert!(t.ends_with("X #42 open"), "{t:?}");
    }
}

/// Parity with the legacy `render_pinned_chip_row` painter, captured
/// BEFORE that painter and `chip_row::layout_chip_row` were deleted: the
/// legacy painter and `dashboard_detail` were both rendered into
/// identical `TestBackend` buffers and compared cell-for-cell with
/// `test_util::assert_rows_match`, and their returned `(pinned index,
/// rect)` lists compared with `assert_eq!` — for two pins at width 80, no
/// pins at width 80, and nine pins at width 200. That run passed
/// (`dashboard_detail_matches_the_legacy_pinned_chip_row ... ok`, 1
/// passed; 0 failed — see the refactor-56 report for the full run). The
/// exact text and rects it captured are pinned below as literals, so this
/// keeps guarding the engine after the legacy code is gone.
#[cfg(test)]
mod dashboard_detail_parity_tests {
    use super::*;
    use crate::commands::pinned::PinnedCommand;
    use crate::config::theme_file::bundled_default;
    use crate::ui::bar::render::hit_rects;
    use crate::ui::bar::test_util::plain;
    use ratatui::layout::Rect;

    fn cmds(specs: &[(&str, &str)]) -> Vec<PinnedCommand> {
        specs
            .iter()
            .map(|(l, c)| PinnedCommand {
                label: (*l).into(),
                command: (*c).into(),
            })
            .collect()
    }

    /// `dashboard_detail`'s hits, as the same `(pinned index, rect)` shape
    /// the legacy painter returned.
    fn pinned_rects(out: &Rendered, width: u16) -> Vec<(usize, Rect)> {
        hit_rects(Rect::new(0, 0, width, 1), &out.hits)
            .into_iter()
            .filter_map(|(rect, hit)| match hit {
                Hit::PinnedChip(i) => Some((i, rect)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn two_pins_match_the_legacy_pinned_chip_row() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let pinned = cmds(&[("PR", "/pr"), ("feedback", "/fb")]);
        let out = dashboard_detail(&specs, &theme, &pinned, 80);
        assert_eq!(
            plain(&out.line),
            " 1  PR   2  feedback  ──────────────────────────────────────────────────────────"
        );
        assert_eq!(
            pinned_rects(&out, 80),
            vec![(0, Rect::new(0, 0, 6, 1)), (1, Rect::new(8, 0, 12, 1))]
        );
    }

    #[test]
    fn no_pins_is_a_full_width_rule_like_the_legacy_painter() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let out = dashboard_detail(&specs, &theme, &[], 80);
        assert_eq!(
            plain(&out.line),
            "────────────────────────────────────────────────────────────────────────────────"
        );
        assert!(pinned_rects(&out, 80).is_empty());
    }

    #[test]
    fn nine_pins_at_width_200_match_the_legacy_pinned_chip_row() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let nine = cmds(&[
            ("one", "/1"),
            ("two", "/2"),
            ("three", "/3"),
            ("four", "/4"),
            ("five", "/5"),
            ("six", "/6"),
            ("seven", "/7"),
            ("eight", "/8"),
            ("nine", "/9"),
        ]);
        let out = dashboard_detail(&specs, &theme, &nine, 200);
        assert_eq!(
            plain(&out.line),
            " 1  one   2  two   3  three   4  four   5  five   6  six   7  seven   8  eight   9  nine  ──────────────────────────────────────────────────────────────────────────────────────────────────────────────"
        );
        assert_eq!(
            pinned_rects(&out, 200),
            vec![
                (0, Rect::new(0, 0, 7, 1)),
                (1, Rect::new(9, 0, 7, 1)),
                (2, Rect::new(18, 0, 9, 1)),
                (3, Rect::new(29, 0, 8, 1)),
                (4, Rect::new(39, 0, 8, 1)),
                (5, Rect::new(49, 0, 7, 1)),
                (6, Rect::new(58, 0, 9, 1)),
                (7, Rect::new(69, 0, 9, 1)),
                (8, Rect::new(80, 0, 8, 1)),
            ]
        );
    }
}

/// Guards the three places a segment must agree: `registry::SEGMENTS`
/// (its declaration), `providers.rs` (the function that renders it), and
/// `bars.rs` (the composer that wires the provider into a segment map).
/// Add a segment to `providers.rs`/`bars.rs` without registering it — or
/// register it without wiring it in — and this fails.
#[cfg(test)]
mod segment_registry_drift_tests {
    use super::*;
    use crate::commands::pinned::PinnedCommand;
    use crate::config::theme_file::bundled_default;
    use crate::data::store::AgentInstanceId;
    use crate::git::DiffStats;
    use crate::git::forge::{BranchLifecycle, ReviewDecision};
    use crate::pty::session::AgentKind;
    use crate::ui::attached::ChipPr;
    use crate::ui::bar::registry::SEGMENTS;
    use crate::ui::detail_modules::session_summary::ChipModelTokens;
    use crate::ui::updates_bar::AttentionLine;
    use std::collections::BTreeSet;

    /// Every input present at once: two agents, a PR with a review
    /// verdict, a diff, running procs, model/tokens, pins, an attention
    /// line, an agent bar, and a version/usage graph — so every segment's
    /// provider has what it needs to produce output.
    #[test]
    fn attached_segments_cover_every_registered_segment() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        let resolver = specs.resolver(&theme);

        let pinned = vec![PinnedCommand {
            label: "PR".into(),
            command: "/pr".into(),
        }];
        let agents = vec![
            (
                AgentInstanceId(1),
                AgentKind::Claude,
                "claude".into(),
                Some('q'),
            ),
            (
                AgentInstanceId(2),
                AgentKind::Codex,
                "codex".into(),
                Some('w'),
            ),
        ];
        let activity: Vec<u32> = (0..24).collect();
        let attention = Some(AttentionLine {
            line: ratatui::text::Line::from("x"),
            segments: Vec::new(),
            more: None,
        });

        let inputs = AttachedInputs {
            repo: "wsx",
            name: "foo",
            version: "0.1.0",
            window_label: "24h",
            activity: &activity,
            agent: Some(AgentKind::Claude),
            attention,
            pinned: &pinned,
            procs: 3,
            diff: Some(DiffStats {
                added: 12,
                removed: 3,
            }),
            pr: Some(ChipPr {
                lifecycle: BranchLifecycle::PrOpen,
                number: 42,
                review: Some(ReviewDecision::Approved),
                unresolved: Some(1),
            }),
            model_tokens: Some(ChipModelTokens {
                model: Some("opus".into()),
                tokens: "45k/200k".into(),
                warn: false,
            }),
            agents: &agents,
            active_agent: Some(AgentInstanceId(1)),
        };

        let segments = attached_segments(&specs, &theme, inputs, &resolver);
        let got: BTreeSet<&str> = segments.keys().map(String::as_str).collect();
        let expected: BTreeSet<&str> = SEGMENTS
            .iter()
            .map(|d| d.name)
            .filter(|name| !DASHBOARD_HEADER_ONLY.contains(name))
            .collect();
        assert_eq!(
            got, expected,
            "attached_segments's output must cover every registered segment name \
             that isn't dashboard-header-only"
        );
    }

    /// The dashboard header's five segments: registered like any other, but
    /// carrying dashboard-only state, so the attached bars never build them.
    const DASHBOARD_HEADER_ONLY: [&str; 5] = ["brand", "group", "sort", "filter", "counts"];

    /// The dashboard footer's three segments (`keys`, `version`, `usage`)
    /// must all be registered names, not private to `dashboard_footer`.
    #[test]
    fn dashboard_footer_segments_are_all_registered_names() {
        for name in ["keys", "version", "usage"] {
            assert!(
                SEGMENTS.iter().any(|d| d.name == name),
                "dashboard_footer's `{name}` segment must be in registry::SEGMENTS"
            );
        }
    }

    /// Likewise the dashboard header's five.
    #[test]
    fn dashboard_header_segments_are_all_registered_names() {
        for name in DASHBOARD_HEADER_ONLY {
            assert!(
                SEGMENTS.iter().any(|d| d.name == name),
                "dashboard_header's `{name}` segment must be in registry::SEGMENTS"
            );
        }
    }
}

/// The dashboard header, and its parity with the deleted
/// `layout::top_chrome` painter. The three `LEGACY_*` strings below were
/// captured from a live `assert_lines_match` run against `top_chrome`
/// (cell-for-cell: symbol and background everywhere, foreground and
/// modifiers on every non-blank cell) while the legacy painter still
/// existed, in the commit that introduced this module. Pinning them as
/// literals keeps them guarding the engine now that it is gone.
#[cfg(test)]
mod dashboard_header_tests {
    use super::*;
    use crate::config::theme_file::bundled_default;
    use crate::ui::bar::test_util::{plain, render_line};
    use crate::ui::dashboard::layout::GroupMode;
    use crate::ui::dashboard::sort::SortMode;

    /// (group Repo, sort Recency, no filter) at width 120.
    const LEGACY_PLAIN: &str = "▌ workspace x · dashboard      group: repo attention   sort: recency status                      9 repos · 14 workspaces";
    /// (group Attention, sort Status, filter "auth") at width 120.
    const LEGACY_FILTERED: &str = "▌ workspace x · dashboard      group: repo attention   sort: recency status  /auth               9 repos · 14 workspaces";
    /// (group Repo, sort Recency, a filter of 80 `x`s) at width 120. Both
    /// the legacy painter and the engine cap the needle at
    /// `FILTER_ECHO_MAX`, and both shed the sort tabs to make room for it.
    const LEGACY_LONG_FILTER: &str = "▌ workspace x · dashboard      group: repo attention  /xxxxxxxxxxxxxxxxxxxxxxx…                  9 repos · 14 workspaces";

    fn header(group: GroupMode, sort: SortMode, filter: Option<&str>, width: u16) -> Rendered {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        dashboard_header(
            &specs,
            &theme,
            &DashboardHeaderInputs {
                group,
                sort,
                repos: 9,
                workspaces: 14,
                filter,
                view: "dashboard",
            },
            width,
        )
    }

    /// The column a label starts at. Every char on this line is one cell
    /// wide, so a char offset is a column.
    fn col_of(text: &str, needle: &str) -> u16 {
        let byte = text.find(needle).expect("label is on the line");
        u16::try_from(text[..byte].chars().count()).unwrap()
    }

    #[test]
    fn engine_header_matches_the_legacy_top_chrome() {
        let needle = "x".repeat(80);
        let cases: [(GroupMode, SortMode, Option<&str>, &str); 3] = [
            (GroupMode::Repo, SortMode::Recency, None, LEGACY_PLAIN),
            (
                GroupMode::Attention,
                SortMode::Status,
                Some("auth"),
                LEGACY_FILTERED,
            ),
            (
                GroupMode::Repo,
                SortMode::Recency,
                Some(&needle),
                LEGACY_LONG_FILTER,
            ),
        ];
        for (group, sort, filter, expected) in cases {
            let out = header(group, sort, filter, 120);
            assert_eq!(plain(&out.line), expected, "filter={filter:?}");
            assert_eq!(out.line.width(), 120);
        }
    }

    #[test]
    fn header_shows_app_name_and_counts() {
        let t = plain(&header(GroupMode::Repo, SortMode::Recency, None, 100).line);
        assert!(t.starts_with("▌ workspace x · dashboard"), "{t:?}");
        assert!(t.contains("group: "), "{t:?}");
        assert!(t.contains("repo"), "{t:?}");
        assert!(t.contains("attention"), "{t:?}");
        assert!(t.trim_end().ends_with("9 repos · 14 workspaces"), "{t:?}");
    }

    #[test]
    fn header_names_both_sort_modes() {
        let t = plain(&header(GroupMode::Repo, SortMode::Recency, None, 120).line);
        assert!(t.contains("sort: "), "{t:?}");
        assert!(t.contains("recency"), "{t:?}");
        assert!(t.contains("status"), "{t:?}");
    }

    /// Without the echo, `/` gives no feedback and rows disappearing from
    /// the list have no visible cause.
    #[test]
    fn header_echoes_the_active_filter() {
        let t = plain(&header(GroupMode::Repo, SortMode::Recency, Some("auth"), 100).line);
        assert!(t.contains("/auth"), "{t:?}");
        // The echo's own prefix, not a bare `/`, so the assertion tracks
        // the echo and not some other span that grows a slash later.
        let bare = plain(&header(GroupMode::Repo, SortMode::Recency, None, 100).line);
        assert!(!bare.contains("  /"), "{bare:?}");
    }

    /// `/` with an empty buffer still echoes, so the keypress registers
    /// before the first character is typed.
    #[test]
    fn header_echoes_an_empty_filter() {
        let t = plain(&header(GroupMode::Repo, SortMode::Recency, Some(""), 100).line);
        assert!(t.contains("  /"), "{t:?}");
    }

    /// A long needle is capped at `FILTER_ECHO_MAX`; with room to spare
    /// the counts still fit beside it.
    #[test]
    fn header_truncates_a_long_filter_and_keeps_counts() {
        let needle = "x".repeat(80);
        let width = 120u16;
        let out = header(GroupMode::Repo, SortMode::Recency, Some(&needle), width);
        let t = plain(&out.line);
        assert!(
            out.line.width() <= usize::from(width),
            "width {width}: {t:?}"
        );
        assert!(t.contains("  /"), "echo present: {t:?}");
        assert!(t.contains('…'), "needle truncates: {t:?}");
        assert!(
            t.trim_end().ends_with("9 repos · 14 workspaces"),
            "counts kept: {t:?}"
        );
    }

    /// Where the engine deliberately parts from the legacy painter. Legacy
    /// budgeted the needle against the room actually left on the line, so a
    /// long needle shrank and the counts always survived. The engine caps
    /// the needle at `FILTER_ECHO_MAX` and, since `[filter]` sits at the
    /// default priority and never drops, pays for it out of `[counts]`
    /// (50) instead — and below about 80 columns the line runs long and is
    /// clipped by whatever renders it. The rationale is unchanged: a needle
    /// with no visible cause is worse than a truncated one.
    #[test]
    fn a_long_filter_costs_the_counts_rather_than_being_shortened_further() {
        let needle = "x".repeat(80);
        let out = header(GroupMode::Repo, SortMode::Recency, Some(&needle), 100);
        let t = plain(&out.line);
        assert!(t.contains('…'), "{t:?}");
        assert!(!t.contains("9 repos"), "the counts pay for the echo: {t:?}");
        assert_eq!(out.line.width(), 100);

        // Narrower still: nothing droppable is left and the echo itself is
        // load-bearing, so the line overflows and the caller clips it.
        let out = header(GroupMode::Repo, SortMode::Recency, Some(&needle), 60);
        assert!(out.line.width() > 60, "{:?}", plain(&out.line));
    }

    /// `[sort]`'s priority 30 beats `[counts]`'s 50, so the sort tabs go
    /// first: the order stays reachable via `o` and the footer hint,
    /// whereas the counts have no other home on this line. 120 holds
    /// everything; 80 holds the counts but not the tabs on top of them.
    #[test]
    fn header_sheds_the_sort_tabs_before_the_counts() {
        let at = |w| plain(&header(GroupMode::Repo, SortMode::Recency, None, w).line);
        assert!(at(120).contains("sort: "), "{:?}", at(120));
        assert!(at(120).contains("9 repos · 14 workspaces"), "{:?}", at(120));
        assert!(!at(80).contains("sort: "), "{:?}", at(80));
        assert!(at(80).contains("9 repos · 14 workspaces"), "{:?}", at(80));
        // The brand and group tabs are load-bearing and survive both.
        assert!(at(80).starts_with("▌ workspace x"), "{:?}", at(80));
        assert!(at(80).contains("group: "), "{:?}", at(80));
    }

    /// Anything past `width` would be clipped off-screen by ratatui, so
    /// the rendered width — not the concatenated text — is the property
    /// that matters.
    #[test]
    fn header_never_overflows_a_narrow_terminal() {
        for width in [100u16, 90, 80, 60] {
            for filter in [None, Some("auth")] {
                let out = header(GroupMode::Repo, SortMode::Recency, filter, width);
                assert!(
                    out.line.width() <= usize::from(width),
                    "width {width} filter {filter:?} overflowed to {}: {:?}",
                    out.line.width(),
                    plain(&out.line)
                );
            }
        }
    }

    /// The active tab is the one painted on the selection background —
    /// reading the cells is what distinguishes it, since every mode's
    /// label is always drawn.
    #[test]
    fn header_highlights_the_active_group_and_sort_modes() {
        let theme = Theme::wsx();
        let check = |group: GroupMode, sort: SortMode, active: &str, inactive: &str| {
            let out = header(group, sort, None, 120);
            let t = plain(&out.line);
            let buf = render_line(&out.line, 120);
            assert_eq!(
                buf[(col_of(&t, active), 0)].bg,
                theme.selected_bg,
                "{active} is active in {t:?}"
            );
            assert_ne!(
                buf[(col_of(&t, inactive), 0)].bg,
                theme.selected_bg,
                "{inactive} is inactive in {t:?}"
            );
        };
        check(GroupMode::Repo, SortMode::Recency, "recency", "status");
        check(GroupMode::Attention, SortMode::Status, "status", "recency");
        check(
            GroupMode::Repo,
            SortMode::Recency,
            "repo attention",
            "attention",
        );
        check(GroupMode::Attention, SortMode::Recency, "attention", "repo");
    }
}
