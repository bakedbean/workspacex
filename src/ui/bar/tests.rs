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
            tags: &[],
            procs: 0,
            diff: None,
            pr: None,
            model_tokens: None,
            agents: &[],
            active_agent: None,
            fleet: crate::ui::bar::fleet::empty(),
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
                tags: &[],
                procs: 0,
                diff: None,
                pr: None,
                model_tokens: None,
                agents: &[],
                active_agent: None,
                fleet: crate::ui::bar::fleet::empty(),
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

    fn footer(
        selected: bool,
        label: &str,
        width: u16,
        fleet: &crate::ui::bar::segment::SegmentMap,
    ) -> Rendered {
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
                fleet,
            },
            width,
        )
    }

    #[test]
    fn default_footer_snapshot() {
        let out = footer(true, "24h", 120, crate::ui::bar::fleet::empty());
        let text = plain(&out.line);
        assert!(
            text.starts_with(
                " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   ?  actions   q  quit"
            ),
            "{text:?}"
        );
        assert!(
            text.trim_end().ends_with("0.1.0"),
            "empty fleet: version only: {text:?}"
        );
        assert!(
            !text.contains('▁'),
            "the sparkline is no longer in the default footer: {text:?}"
        );
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
    fn default_footer_funnel_lists_nonzero_stages() {
        use crate::ui::bar::fleet::{FleetRow, FleetStats};
        let rows = vec![
            FleetRow {
                reported: Some(crate::data::store::ReportedState::Working),
                ..Default::default()
            },
            FleetRow {
                reported: Some(crate::data::store::ReportedState::Working),
                ..Default::default()
            },
            FleetRow {
                reported: Some(crate::data::store::ReportedState::Blocked),
                ..Default::default()
            },
            FleetRow {
                lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
                review: Some(crate::git::forge::ReviewDecision::Approved),
                ..Default::default()
            },
        ];
        let fleet = FleetStats::from_rows(rows, 1, 0).to_vars();
        let text = plain(&footer(false, "24h", 140, &fleet).line);
        assert!(
            text.ends_with("0.1.0  2 working  1 blocked  1 ready"),
            "{text:?}"
        );
    }

    #[test]
    fn usage_still_renders_when_a_theme_places_it() {
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[dashboard_footer]\nright_format = \"$usage\"\n",
            )
            .unwrap(),
            &theme,
        )
        .unwrap();
        let activity: Vec<u32> = (0..24).collect();
        let out = dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &activity,
                version: "0.1.0",
                window_label: "24h",
                workspace_selected: false,
                fleet: crate::ui::bar::fleet::empty(),
            },
            120,
        );
        let spark = crate::ui::dashboard::sparkline::render(&activity, 24);
        assert!(plain(&out.line).ends_with(&format!("24h {spark}")));
        // Its hit still covers exactly the `<label> <spark>` span, flush
        // against the right edge.
        let usage = out
            .hits
            .iter()
            .find(|h| matches!(h.hit, Hit::UsageGraph))
            .unwrap();
        assert_eq!(usage.width, "24h".len() as u16 + 1 + 24);
        assert_eq!(usage.start_col + usage.width, 120);
    }

    /// The bundled `tokens` module is defined but not placed; a theme that
    /// puts `$tokens` in the footer gets per-kind context sums, with kinds
    /// that have no live context dropped along with their gap.
    #[test]
    fn bundled_tokens_module_renders_when_a_theme_places_it() {
        use crate::pty::session::AgentKind;
        use crate::ui::bar::fleet::{FleetRow, FleetStats};
        let theme = Theme::wsx();
        let specs = crate::config::theme_file::resolve(
            crate::config::theme_file::ThemeFile::parse(
                "[dashboard_footer]\nright_format = \"$version(  $tokens)\"\n",
            )
            .unwrap(),
            &theme,
        )
        .unwrap();
        let rows = vec![
            FleetRow {
                context_tokens: vec![(AgentKind::Claude, 1_200_000), (AgentKind::Codex, 40_000)],
                ..Default::default()
            },
            FleetRow {
                context_tokens: vec![(AgentKind::Codex, 300_000)],
                ..Default::default()
            },
        ];
        let fleet = FleetStats::from_rows(rows, 1, 0).to_vars();
        let out = dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &[],
                version: "0.1.0",
                window_label: "24h",
                workspace_selected: false,
                fleet: &fleet,
            },
            140,
        );
        let text = plain(&out.line);
        assert!(text.ends_with("0.1.0  claude 1.2M  codex 340k"), "{text:?}");
        // Empty fleet: the whole `(  $tokens)` group drops.
        let text = plain(&footer(false, "24h", 140, crate::ui::bar::fleet::empty()).line);
        assert!(!text.contains("claude"), "{text:?}");
    }

    #[test]
    fn footer_omits_actions_pill_without_workspace() {
        assert!(
            !plain(&footer(false, "24h", 120, crate::ui::bar::fleet::empty()).line)
                .contains("actions")
        );
        assert!(
            plain(&footer(true, "24h", 120, crate::ui::bar::fleet::empty()).line)
                .contains("actions")
        );
    }

    #[test]
    fn footer_key_pill_wraps_key_only_not_label() {
        let theme = Theme::wsx();
        let out = footer(true, "24h", 120, crate::ui::bar::fleet::empty());
        let buf = render_line(&out.line, 120);
        // " ↑↓ " is cols 0..4 on the chip bg; " nav" follows on the bar bg.
        assert_eq!(buf[(1, 0)].bg, theme.bg_soft);
        assert_eq!(buf[(5, 0)].symbol(), "n");
        assert_ne!(buf[(5, 0)].bg, theme.bg_soft);
    }

    #[test]
    fn footer_hints_align_with_rendered_key_pills() {
        let out = footer(true, "24h", 120, crate::ui::bar::fleet::empty());
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

    // At 85 columns, keys (71) + the mandatory 1-cell gap + the funnel's
    // group "  2 working" (11, including its own grouped leading gap) = 83,
    // which fits with room to spare; adding `$version`'s "0.1.0" (5 more, 88
    // total) does not. `$version`'s lower priority (50 vs. the funnel's 60)
    // drops it first, so the funnel survives alone — the same slot `$usage`
    // used to hold.
    #[test]
    fn narrow_footer_drops_version_before_funnel() {
        use crate::ui::bar::fleet::{FleetRow, FleetStats};
        let rows = vec![
            FleetRow {
                reported: Some(crate::data::store::ReportedState::Working),
                ..Default::default()
            },
            FleetRow {
                reported: Some(crate::data::store::ReportedState::Working),
                ..Default::default()
            },
        ];
        let fleet = FleetStats::from_rows(rows, 1, 0).to_vars();
        let out = footer(false, "24h", 85, &fleet);
        let text = plain(&out.line);
        assert!(!text.contains("0.1.0"), "{text:?}");
        assert!(text.contains("2 working"), "{text:?}");
        assert_eq!(out.line.width(), 85);
    }

    // At 110 columns the bundled default's full content (keys 71 + gap 1
    // + version 5 = 77, well under 110) always fits with an empty fleet:
    // `(  $funnel)` drops entirely — leading gap included — since $funnel
    // renders nothing to drop against. Pinned here so a future change to
    // the bundled default's overflow priorities gets caught at this width.
    #[test]
    fn default_footer_snapshot_at_110() {
        let out = footer(false, "24h", 110, crate::ui::bar::fleet::empty());
        let text = plain(&out.line);
        assert!(
            text.starts_with(
                " ↑↓  nav   ↵  open   n  new   G  group   o  order   /  filter   q  quit"
            ),
            "{text:?}"
        );
        assert!(
            text.trim_end().ends_with("0.1.0"),
            "empty fleet: version only: {text:?}"
        );
        assert!(!text.contains('▁'), "no sparkline: {text:?}");
        assert_eq!(out.line.width(), 110);
        assert_eq!(
            out.hits
                .iter()
                .filter(|h| matches!(h.hit, Hit::Key(_)))
                .count(),
            7
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
            tags: &[],
            procs: 3,
            diff: Some(crate::git::DiffStats {
                added: 12,
                removed: 3,
            }),
            pr: pr(Some(ReviewDecision::Approved)),
            model_tokens: mt(),
            agents,
            active_agent: Some(AgentInstanceId(1)),
            fleet: crate::ui::bar::fleet::empty(),
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
        // (pill pad + `index` pad + space-led label), two spaces, the tags
        // group (no tags, so just the ` <> ` manager pill), two spaces, then
        // the rule.
        assert!(
            t.starts_with(" ^x  menu   1  PR   2  feedback   <>   ──"),
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
    fn tag_chips_follow_the_pins_and_carry_their_hits() {
        let pinned = cmds(&[("PR", "/pr")]);
        let agents = agents();
        let tags = vec![
            crate::commands::tags::PromptTag {
                name: "context".into(),
                uses: 3,
            },
            crate::commands::tags::PromptTag {
                name: "task".into(),
                uses: 1,
            },
        ];
        let mut inputs = full(&pinned, &agents);
        inputs.tags = &tags;
        let out = render(inputs, 140);
        let t = plain(&out.line);
        assert!(
            t.starts_with(" ^x  menu   1  PR  <context>  <task>   <>   ──"),
            "{t:?}"
        );
        let chips: Vec<_> = out
            .hits
            .iter()
            .filter_map(|h| match h.hit {
                Hit::TagChip(i) => Some(i),
                _ => None,
            })
            .collect();
        assert_eq!(chips, vec![0, 1]);
        assert!(out.hits.iter().any(|h| h.hit == Hit::TagsManager));
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
        // Left side is 25 cells (` ^x  menu` + 2 + ` 1  PR` + 2 + ` <> ` + 2).
        // Right side at full strength is 73: 2 + agents 26 + 3 + model 17 + 1
        // + procs 4 + 1 + diff 6 + 1 + pr 12. Dropping model removes 18,
        // agents 29, procs 5, diff 7. The `$tags` manager chip (priority 40,
        // on the `format` side) goes before `diff` (also 40, `right_format`)
        // on the tie, handing its 6 cells back — so the pr-only width is the
        // same one that squeezed diff out before the chip existed.
        let widths_and_expect: [(u16, &[&str]); 5] = [
            (126, &["agents", "model_tokens", "procs", "diff", "pr"]),
            (86, &["agents", "procs", "diff", "pr"]),
            (56, &["procs", "diff", "pr"]),
            (46, &["diff", "pr"]),
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
            tags: &[],
            procs: 0,
            diff: None,
            pr: pr(None),
            model_tokens: None,
            agents: &agents,
            active_agent: None,
            fleet: crate::ui::bar::fleet::empty(),
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
        let out = dashboard_detail(&specs, &theme, &pinned, crate::ui::bar::fleet::empty(), 80);
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
        let out = dashboard_detail(&specs, &theme, &[], crate::ui::bar::fleet::empty(), 80);
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
        let out = dashboard_detail(&specs, &theme, &nine, crate::ui::bar::fleet::empty(), 200);
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
    use crate::ui::updates_bar::{AttentionEntry, AttentionItems};
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
        let tags = vec![crate::commands::tags::PromptTag {
            name: "context".into(),
            uses: 1,
        }];
        let activity: Vec<u32> = (0..24).collect();
        let attention = Some(AttentionItems {
            entries: vec![AttentionEntry {
                workspace_id: crate::data::store::WorkspaceId(1),
                repo_name: "a".into(),
                name: "q".into(),
                age_anchor_ms: 9_000,
                status: crate::ui::dashboard::status::Status::Question,
                lifecycle: None,
            }],
            now_ms: 10_000,
            max_width: 40,
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
            tags: &tags,
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
            fleet: crate::ui::bar::fleet::empty(),
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
                fleet: crate::ui::bar::fleet::empty(),
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

#[cfg(test)]
mod attention_tests {
    use super::*;
    use crate::config::theme_file::{BarSpecs, ThemeFile, bundled_default, resolve};
    use crate::data::store::WorkspaceId;
    use crate::git::forge::BranchLifecycle;
    use crate::ui::bar::segment::{HitSpan, Segment};
    use crate::ui::dashboard::status::Status;
    use crate::ui::updates_bar::{AttentionEntry, AttentionItems};
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};

    fn entry(
        id: i64,
        repo: &str,
        name: &str,
        status: Status,
        lc: Option<BranchLifecycle>,
    ) -> AttentionEntry {
        AttentionEntry {
            workspace_id: WorkspaceId(id),
            repo_name: repo.into(),
            name: name.into(),
            age_anchor_ms: 9_000,
            status,
            lifecycle: lc,
        }
    }

    /// A Question entry with no PR: the plainest possible item.
    fn q(id: i64, repo: &str, name: &str) -> AttentionEntry {
        entry(id, repo, name, Status::Question, None)
    }

    /// Widths under the stock format: "? a/q (1s)" = 10, "! bb/ss (1s)" = 12.
    fn three_entries() -> Vec<AttentionEntry> {
        vec![
            entry(1, "a", "q", Status::Question, Some(BranchLifecycle::PrOpen)),
            entry(2, "bb", "ss", Status::Stalled, None),
            entry(3, "bb", "ss", Status::Stalled, None),
        ]
    }

    /// The bundled default with `src` merged over it.
    fn specs_with(src: &str, theme: &Theme) -> BarSpecs {
        resolve(ThemeFile::parse(src).unwrap(), theme).unwrap()
    }

    /// The attention input as the app builds it: `entries` to be fitted to
    /// `max_width` at `now_ms` (every entry is anchored at 9_000, so the
    /// age reads `1s`).
    fn attention_input(entries: &[AttentionEntry], max_width: usize) -> Option<AttentionItems> {
        Some(AttentionItems {
            entries: entries.to_vec(),
            now_ms: 10_000,
            max_width,
        })
    }

    /// Render just the `attention` segment of the attached bars under
    /// `specs`, with every other input empty.
    fn render_attention(
        specs: &BarSpecs,
        theme: &Theme,
        entries: &[AttentionEntry],
        max_width: usize,
    ) -> Option<Segment> {
        let resolver = specs.resolver(theme);
        let inputs = AttachedInputs {
            repo: "wsx",
            name: "foo",
            version: "0.1.0",
            window_label: "24h",
            activity: &[],
            agent: None,
            attention: attention_input(entries, max_width),
            pinned: &[],
            tags: &[],
            procs: 0,
            diff: None,
            pr: None,
            model_tokens: None,
            agents: &[],
            active_agent: None,
            fleet: crate::ui::bar::fleet::empty(),
        };
        attached_segments(specs, theme, inputs, &resolver).remove("attention")
    }

    fn stock(entries: &[AttentionEntry], max_width: usize) -> Segment {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        render_attention(&specs, &theme, entries, max_width).expect("attention renders")
    }

    fn hits(seg: &Segment) -> Vec<(u16, u16, Hit)> {
        seg.hits
            .iter()
            .map(|h| (h.start_col, h.width, h.hit))
            .collect()
    }

    fn more(seg: &Segment) -> Option<(u16, u16)> {
        seg.hits
            .iter()
            .find(|h| h.hit == Hit::AttentionMore)
            .map(|h| (h.start_col, h.width))
    }

    fn entry_hits(seg: &Segment) -> Vec<(u16, u16, WorkspaceId)> {
        seg.hits
            .iter()
            .filter_map(|h| match h.hit {
                Hit::Attention(id) => Some((h.start_col, h.width, id)),
                _ => None,
            })
            .collect()
    }

    /// Baseline capture: the bundled default renders two entries plus an
    /// overflow tail exactly as the original span builder did — status-
    /// styled glyph, a plain space, the name in its lifecycle hue (or the
    /// muted `path` hue), a dim ` (age)`, dim ` │ ` separators, and a dim
    /// ` … +N more` tail — with one hit per rendered entry and one over
    /// the tail. Cell-for-cell, so span boundaries are free to change.
    #[test]
    fn bundled_default_renders_the_legacy_attention_line_cell_for_cell() {
        let theme = Theme::wsx();
        // Budget 36: entries 0+1 (10 + 3 + 12 = 25) fit with the 10-cell
        // tail; entry 2 folds into it.
        let seg = stock(&three_entries(), 36);
        let expected = Line::from(vec![
            Span::styled("?", theme.status_style(Status::Question)),
            Span::raw(" "),
            Span::styled("a/q", theme.ok_style()),
            Span::styled(" (1s)", theme.dim_style()),
            Span::styled(" │ ", theme.dim_style()),
            Span::styled("!", theme.status_style(Status::Stalled)),
            Span::raw(" "),
            Span::styled("bb/ss", Style::default().fg(theme.path)),
            Span::styled(" (1s)", theme.dim_style()),
            Span::styled(" … +1 more", theme.dim_style()),
        ]);
        let actual = Line::from(seg.spans.clone());
        assert_eq!(test_util::plain(&actual), test_util::plain(&expected));
        test_util::assert_lines_match(&expected, &actual, 35);
        assert_eq!(seg.width, 35);
        assert_eq!(
            seg.hits,
            vec![
                HitSpan {
                    start_col: 0,
                    width: 10,
                    hit: Hit::Attention(WorkspaceId(1))
                },
                HitSpan {
                    start_col: 13,
                    width: 12,
                    hit: Hit::Attention(WorkspaceId(2))
                },
                HitSpan {
                    start_col: 25,
                    width: 10,
                    hit: Hit::AttentionMore
                },
            ]
        );
    }

    /// A theme lays out each entry, the separator, and the tail itself:
    /// `format` is one item (with `$style` the entry's name style),
    /// `separator` joins them, `more_format` renders the fold with
    /// `$count`. Each carries its own style and its own hit.
    #[test]
    fn a_custom_format_renders_each_entry_separator_and_tail_as_its_own_block() {
        let theme = Theme::wsx();
        let specs = specs_with(
            "[attention]\nformat = \"[$glyph $name]($style)\"\nseparator = \"[>](fg:ok)\"\nmore_format = \"[+$count](fg:warn)\"\n",
            &theme,
        );
        let all = render_attention(&specs, &theme, &three_entries(), 200).unwrap();
        assert_eq!(all.plain_text(), "? q>! ss>! ss");
        assert_eq!(
            hits(&all),
            vec![
                (0, 3, Hit::Attention(WorkspaceId(1))),
                (4, 4, Hit::Attention(WorkspaceId(2))),
                (9, 4, Hit::Attention(WorkspaceId(3))),
            ]
        );
        let span = |text: &str| {
            all.spans
                .iter()
                .find(|s| s.content.as_ref() == text)
                .unwrap_or_else(|| panic!("span {text:?} in {:?}", all.plain_text()))
                .style
        };
        assert_eq!(span(">").fg, Some(theme.ok), "separator keeps its style");
        assert_eq!(span("q").fg, Some(theme.ok), "$style is the PR-open tint");
        assert_eq!(span("ss").fg, Some(theme.path), "$style falls back to path");
        assert_eq!(
            span("?").fg,
            Some(theme.question),
            "$glyph keeps its status color inside a $style run"
        );

        // Width 8: "? q>! ss" fits but not with the "+1" tail, so the
        // second entry folds and the tail counts both.
        let folded = render_attention(&specs, &theme, &three_entries(), 8).unwrap();
        assert_eq!(folded.plain_text(), "? q+2");
        assert_eq!(
            hits(&folded),
            vec![
                (0, 3, Hit::Attention(WorkspaceId(1))),
                (3, 2, Hit::AttentionMore),
            ]
        );
        let tail = folded
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "+")
            .expect("tail literal span");
        assert_eq!(tail.style.fg, Some(theme.warn));
    }

    /// The fit uses the theme's separator width, not the stock 3 cells: a
    /// 7-cell separator makes two entries plus the tail overrun a budget
    /// the stock separator fits in, so the second entry folds.
    #[test]
    fn fitting_measures_the_themes_separator_width() {
        let theme = Theme::wsx();
        let wide = specs_with("[attention]\nseparator = \"[ ───── ](fg:dim)\"\n", &theme);
        let seg = render_attention(&wide, &theme, &three_entries(), 36).unwrap();
        assert_eq!(entry_hits(&seg).len(), 1, "{:?}", seg.plain_text());
        assert!(
            seg.plain_text().ends_with("+2 more"),
            "{:?}",
            seg.plain_text()
        );
        assert!(usize::from(seg.width) <= 36);
        // Same budget, stock separator: two entries fit (the baseline).
        assert_eq!(entry_hits(&stock(&three_entries(), 36)).len(), 2);
    }

    /// Likewise the tail: a longer `more_format` claims more of the
    /// budget, and entries give way to keep it on screen.
    #[test]
    fn fitting_measures_the_themes_tail_width() {
        let theme = Theme::wsx();
        let long = specs_with(
            "[attention]\nmore_format = \"[ and $count more workspaces](fg:dim)\"\n",
            &theme,
        );
        let seg = render_attention(&long, &theme, &three_entries(), 36).unwrap();
        assert_eq!(entry_hits(&seg).len(), 1, "{:?}", seg.plain_text());
        assert!(
            seg.plain_text().ends_with("and 2 more workspaces"),
            "{:?}",
            seg.plain_text()
        );
        assert!(usize::from(seg.width) <= 36, "{}", seg.width);
        assert_eq!(more(&seg), Some((10, 22)));
    }

    /// An item whose `format` renders empty is dropped as if it were never
    /// in the list — no separator, no hit, and not counted in the tail —
    /// exactly as `eval_items` treats empty items. An all-empty format
    /// renders nothing, so the enclosing bar group hides.
    #[test]
    fn entries_that_render_empty_are_dropped_not_joined() {
        let theme = Theme::wsx();
        let empty = specs_with("[attention]\nformat = \"\"\n", &theme);
        assert!(render_attention(&empty, &theme, &three_entries(), 80).is_none());
        assert!(render_attention(&empty, &theme, &three_entries(), 0).is_none());

        // `$repo` alone: the middle entry has no repo name, so it renders
        // empty and vanishes; the other two join with one separator.
        let repo_only = specs_with(
            "[attention]\nformat = \"$repo\"\nseparator = \"|\"\nmore_format = \"+$count\"\n",
            &theme,
        );
        let entries = vec![q(1, "aa", "x"), q(2, "", "y"), q(3, "cc", "z")];
        let seg = render_attention(&repo_only, &theme, &entries, 80).unwrap();
        assert_eq!(seg.plain_text(), "aa|cc");
        assert_eq!(
            hits(&seg),
            vec![
                (0, 2, Hit::Attention(WorkspaceId(1))),
                (3, 2, Hit::Attention(WorkspaceId(3))),
            ]
        );
        // Folding counts only entries that would have rendered: at width
        // 2 the first fits, the empty one is not "more", the third is.
        let seg = render_attention(&repo_only, &theme, &entries, 4).unwrap();
        assert_eq!(seg.plain_text(), "aa+1");
        assert_eq!(more(&seg), Some((2, 2)));
    }

    /// Restores the lifecycle cases the old builder tests covered: merged
    /// is the merged hue, and an explicit colorless lifecycle (NoPr) falls
    /// back to `path` just like an unpolled `None`.
    #[test]
    fn name_style_follows_every_lifecycle_hue() {
        let theme = Theme::wsx();
        let seg = stock(
            &[
                entry(
                    1,
                    "r",
                    "open",
                    Status::Question,
                    Some(BranchLifecycle::PrOpen),
                ),
                entry(
                    2,
                    "r",
                    "merged",
                    Status::Question,
                    Some(BranchLifecycle::PrMerged),
                ),
                entry(
                    3,
                    "r",
                    "closed",
                    Status::Question,
                    Some(BranchLifecycle::PrClosed),
                ),
                entry(
                    4,
                    "r",
                    "nopr",
                    Status::Question,
                    Some(BranchLifecycle::NoPr),
                ),
            ],
            200,
        );
        let fg = |name: &str| {
            seg.spans
                .iter()
                .find(|s| s.content.as_ref() == name)
                .unwrap_or_else(|| panic!("name span {name:?} in {:?}", seg.plain_text()))
                .style
                .fg
        };
        assert_eq!(fg("open"), Some(theme.ok));
        assert_eq!(fg("merged"), Some(theme.merged));
        assert_eq!(fg("closed"), Some(theme.err));
        assert_eq!(fg("nopr"), Some(theme.path));
    }

    /// Graded attention blocks: `styles` sets each rendered entry's block
    /// colour, wedges chain through `item_bg`/`next_bg`, the last RENDERED
    /// entry's `next` is absent even when a tail follows (so its wedge
    /// blends into the bar), and the tail's `prev` is that entry.
    #[test]
    fn graded_attention_blocks_chain_their_wedges_and_stop_at_the_fold() {
        use ratatui::style::Color;
        let theme = Theme::wsx();
        let specs = specs_with(
            concat!(
                "[attention]\nstyles = [\"bg:red\", \"bg:blue\", \"bg:green\"]\n",
                "format = '[ $name ]($style)[>](fg:item_bg bg:next_bg)'\n",
                "separator = \"\"\n",
                "more_format = \"[<](fg:prev_bg)+$count\"\n",
            ),
            &theme,
        );
        let all = render_attention(&specs, &theme, &three_entries(), 200).unwrap();
        assert_eq!(all.plain_text(), " q > ss > ss >");
        let wedges: Vec<(Option<Color>, Option<Color>)> = all
            .spans
            .iter()
            .filter(|s| s.content.as_ref() == ">")
            .map(|s| (s.style.fg, s.style.bg))
            .collect();
        assert_eq!(
            wedges,
            vec![
                (Some(Color::Red), Some(Color::Blue)),
                (Some(Color::Blue), Some(Color::Green)),
                (Some(Color::Green), None),
            ]
        );
        // The PR-open name keeps its lifecycle fg under a bg-only grade.
        let q = all
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "q")
            .unwrap();
        assert_eq!((q.style.fg, q.style.bg), (Some(theme.ok), Some(Color::Red)));

        // Width 9: " q >" (4) + " ss >" (5) = 9 fits, but not with the
        // "<+1" tail, so the second entry folds; the survivor's wedge has
        // no next, and the tail's `<` takes the survivor's bg.
        let folded = render_attention(&specs, &theme, &three_entries(), 9).unwrap();
        assert_eq!(folded.plain_text(), " q ><+2");
        let wedge = folded
            .spans
            .iter()
            .find(|s| s.content.as_ref() == ">")
            .unwrap();
        assert_eq!((wedge.style.fg, wedge.style.bg), (Some(Color::Red), None));
        let tail = folded
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "<")
            .unwrap();
        assert_eq!(tail.style.fg, Some(Color::Red));
        assert_eq!(
            hits(&folded),
            vec![
                (0, 4, Hit::Attention(WorkspaceId(1))),
                (4, 3, Hit::AttentionMore)
            ]
        );
    }

    #[test]
    fn no_entries_renders_nothing() {
        let theme = Theme::wsx();
        let specs = bundled_default(&theme);
        assert!(render_attention(&specs, &theme, &[], 80).is_none());
    }

    #[test]
    fn only_rendered_entries_get_hits() {
        // Budget 10 fits only entry 0 ("? a/q (1s)" is exactly 10).
        let seg = stock(&three_entries(), 10);
        let entries = entry_hits(&seg);
        assert_eq!(entries.len(), 1, "{:?}", seg.plain_text());
        assert_eq!(entries[0].2, WorkspaceId(1));
        assert!(more(&seg).is_some(), "the folded entries are reachable");
    }

    /// The glyph is the dashboard's, so a row reads the same in both
    /// surfaces: Waiting draws the ellipsis, Idle the dot, each in its
    /// status color.
    #[test]
    fn glyph_follows_canonical_status() {
        let theme = Theme::wsx();
        let seg = stock(
            &[
                entry(1, "a", "w", Status::Waiting, None),
                entry(2, "a", "i", Status::Idle, None),
            ],
            200,
        );
        let text = seg.plain_text();
        assert!(text.contains("\u{2026} a/w"), "waiting glyph: {text:?}");
        assert!(text.contains("\u{b7} a/i"), "idle glyph: {text:?}");
        let idle = seg
            .spans
            .iter()
            .find(|s| s.content.as_ref() == "\u{b7}")
            .expect("idle glyph span");
        assert_eq!(idle.style.fg, Some(theme.idle));
    }

    #[test]
    fn entries_give_way_so_the_tail_fits() {
        // Budget 30: entries 0+1 fit on their own (25) but not with the
        // tail (35), so entry 1 folds into the tail too: 10 + " … +2 more".
        let seg = stock(&three_entries(), 30);
        assert_eq!(entry_hits(&seg).len(), 1, "entry 1 must yield to the tail");
        assert_eq!(more(&seg), Some((10, 10)));
        assert!(usize::from(seg.width) <= 30, "{}", seg.width);
        assert!(
            seg.plain_text().ends_with("+2 more"),
            "{:?}",
            seg.plain_text()
        );
    }

    #[test]
    fn no_tail_when_everything_fits() {
        let seg = stock(&three_entries(), 200);
        assert_eq!(entry_hits(&seg).len(), 3);
        assert_eq!(more(&seg), None);
    }

    /// A first entry wider than the budget used to push the tail off
    /// screen. Its name yields (with an ellipsis) so the tail fits.
    #[test]
    fn a_long_first_name_is_ellipsized_to_keep_the_tail_visible() {
        let seg = stock(&[q(1, "repo", &"n".repeat(40)), q(2, "repo", "b")], 30);
        let text = seg.plain_text();
        assert!(usize::from(seg.width) <= 30, "{text:?}");
        assert!(text.ends_with("+1 more"), "{text:?}");
        assert!(text.contains("repo/nnn"), "name keeps its head: {text:?}");
        assert!(text.contains("…") && text.contains(" (1s)"), "{text:?}");
        let entries = entry_hits(&seg);
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].1 + more(&seg).unwrap().1,
            seg.width,
            "entry + tail must tile the segment exactly"
        );
    }

    #[test]
    fn a_zero_width_budget_still_renders_the_first_entry() {
        let seg = stock(&three_entries(), 0);
        assert_eq!(entry_hits(&seg).len(), 1);
    }

    /// "日本" is two double-width glyphs: 4 cells, 2 chars. Hit geometry
    /// must use cells or the click rects drift.
    #[test]
    fn hits_are_measured_in_terminal_cells() {
        let seg = stock(&[q(1, "a", "日本"), q(2, "a", "q")], 200);
        let entries = entry_hits(&seg);
        // "? a/日本 (1s)" = 1+1 + 1+1 + 4 + 2+2+1 = 13 cells.
        assert_eq!(entries[0].1, 13);
        assert_eq!(entries[1].0, 16);
    }

    /// Giving an entry back can push the remainder from 9 to 10 and widen
    /// the tail by a column; the fit must use the final width.
    #[test]
    fn tail_width_tracks_a_multi_digit_remainder() {
        let entries: Vec<AttentionEntry> = (1..=12).map(|i| q(i, "a", "q")).collect();
        for budget in [33usize, 34, 42] {
            let seg = stock(&entries, budget);
            let text = seg.plain_text();
            assert!(
                usize::from(seg.width) <= budget,
                "budget {budget}: {text:?}"
            );
            let (start, width) = more(&seg).expect("overflow");
            assert_eq!(
                start + width,
                seg.width,
                "budget {budget}: tail extent must end the segment: {text:?}"
            );
        }
    }
}

/// The shipped example themes under `docs/examples/` must stay loadable,
/// and every one must keep the wordmark in the app's brand colours: their
/// first cut put `$brand` on a coloured mode block with the theme's own
/// accent on the bar and "x", which erased the brand-blue branding.
#[cfg(test)]
mod example_theme_tests {
    use super::*;
    use crate::config::theme_file::load;
    use crate::ui::bar::test_util::{plain, render_line};
    use crate::ui::dashboard::layout::GroupMode;
    use crate::ui::dashboard::sort::SortMode;
    use crate::ui::theme::{BRAND_ACCENT, BRAND_WORDMARK};
    use ratatui::style::{Color, Modifier};
    use std::path::{Path, PathBuf};

    fn examples_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/examples")
    }

    fn example_themes() -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(examples_dir())
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no example themes found");
        paths
    }

    #[test]
    fn every_example_theme_validates() {
        for path in example_themes() {
            if let Err(errors) = load(&path, &Theme::wsx()) {
                panic!("{} failed to load: {errors:?}", path.display());
            }
        }
    }

    /// The orange example puts the workspace name and the pr chip on
    /// orange mode blocks, where the base theme's lifecycle tints wash
    /// out; both blocks take the file's pale xterm-cube variants through
    /// their own palettes, and the name drops to the file's chalk without
    /// a PR.
    #[test]
    fn orange_example_lightens_the_lifecycle_tints_on_its_orange_blocks() {
        use crate::git::forge::BranchLifecycle;
        use crate::ui::attached::ChipPr;
        let theme = Theme::jellybeans();
        let specs = load(&examples_dir().join("theme-orange.toml"), &theme).unwrap();
        let orange = Color::Rgb(0xd7, 0x5f, 0x00);
        let mint = Color::Rgb(0xd7, 0xff, 0xaf);
        let lilac = Color::Rgb(0xd7, 0xaf, 0xff);
        let chalk = Color::Rgb(0xee, 0xee, 0xee);
        let render = |pr: Option<ChipPr>| {
            let inputs = AttachedInputs {
                repo: "",
                name: "ws",
                version: "0.1.0",
                window_label: "24h",
                activity: &[],
                agent: None,
                attention: None,
                pinned: &[],
                tags: &[],
                procs: 0,
                diff: None,
                pr,
                model_tokens: None,
                agents: &[],
                active_agent: None,
                fleet: crate::ui::bar::fleet::empty(),
            };
            let bars = attached_bars(&specs, &theme, inputs, 80, 80);
            (
                render_line(&bars.top.line, 80),
                render_line(&bars.bottom.line, 80),
            )
        };
        let chip = |lifecycle| ChipPr {
            lifecycle,
            number: 42,
            review: None,
            unresolved: None,
        };
        let name_cell = |buf: &ratatui::buffer::Buffer| {
            let col = (0..80).find(|&x| buf[(x, 0)].symbol() == "w").unwrap();
            buf[(col, 0)].clone()
        };
        let pr_cell = |buf: &ratatui::buffer::Buffer| {
            let col = (0..80).find(|&x| buf[(x, 0)].symbol() == "#").unwrap();
            buf[(col, 0)].clone()
        };

        let (top, bottom) = render(Some(chip(BranchLifecycle::PrOpen)));
        let (name, pr) = (name_cell(&top), pr_cell(&bottom));
        assert_eq!((name.fg, name.bg), (mint, orange), "open name");
        assert_eq!((pr.fg, pr.bg), (mint, orange), "open pr");

        let (top, bottom) = render(Some(chip(BranchLifecycle::PrMerged)));
        assert_eq!(name_cell(&top).fg, lilac, "merged name");
        assert_eq!(pr_cell(&bottom).fg, lilac, "merged pr");

        let (top, _) = render(None);
        let name = name_cell(&top);
        assert_eq!((name.fg, name.bg), (chalk, orange), "no-PR name");
        assert!(name.modifier.contains(Modifier::BOLD));
    }

    /// With an empty fleet, `$funnel` is empty and its conditional group
    /// must drop *with* the arrow that leads into it — not leave a bare
    /// coloured stub dangling past the version block.
    #[test]
    fn every_example_theme_drops_the_funnel_block_when_empty() {
        for path in example_themes() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let theme = Theme::wsx();
            let specs = load(&path, &theme).unwrap();
            let activity: Vec<u32> = (0..24).collect();
            let out = dashboard_footer(
                &specs,
                &theme,
                &DashboardFooterInputs {
                    activity: &activity,
                    version: "0.1.0",
                    window_label: "24h",
                    workspace_selected: true,
                    fleet: crate::ui::bar::fleet::empty(),
                },
                120,
            );
            let text = plain(&out.line);
            assert!(
                text.trim_end().ends_with("0.1.0"),
                "{name}: expected the version block to be the last thing rendered, got {text:?}"
            );
        }
    }

    /// The brand tokens are constant across base themes, so one base
    /// (the one the orange and jellybeans files pair with) covers them all.
    #[test]
    fn example_themes_keep_the_brand_colours_on_the_wordmark() {
        for path in example_themes() {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let theme = Theme::jellybeans();
            let specs = load(&path, &theme).unwrap();
            let out = dashboard_header(
                &specs,
                &theme,
                &DashboardHeaderInputs {
                    group: GroupMode::Repo,
                    sort: SortMode::Recency,
                    repos: 9,
                    workspaces: 14,
                    filter: None,
                    view: "dashboard",
                    fleet: crate::ui::bar::fleet::empty(),
                },
                120,
            );
            let text = plain(&out.line);
            assert!(text.starts_with("▌ workspace x "), "{name}: {text:?}");
            let buf = render_line(&out.line, 120);
            let cell = |col: u16| buf[(col, 0)].clone();

            let bar = cell(0);
            assert_eq!(bar.symbol(), "▌", "{name}");
            assert_eq!(
                bar.fg, BRAND_ACCENT,
                "{name}: cursor bar must be brand blue"
            );
            assert_eq!(
                bar.bg,
                Color::Reset,
                "{name}: wordmark sits flat on the bar, not on a block"
            );

            let word = cell(2);
            assert_eq!(word.symbol(), "w", "{name}");
            assert_eq!(
                word.fg, BRAND_WORDMARK,
                "{name}: name must be the wordmark cream"
            );

            let mark = cell(12);
            assert_eq!(mark.symbol(), "x", "{name}");
            assert_eq!(mark.fg, BRAND_ACCENT, "{name}: the x must be brand blue");
            assert!(mark.modifier.contains(Modifier::BOLD), "{name}");
        }
    }
}

#[cfg(test)]
mod module_tests {
    use super::*;
    use crate::config::theme_file::{ThemeFile, resolve};
    use crate::ui::bar::fleet::{FleetRow, FleetStats};
    use crate::ui::bar::segment::SegmentMap;
    use test_util::plain;

    fn specs_with(src: &str) -> crate::config::theme_file::BarSpecs {
        resolve(ThemeFile::parse(src).unwrap(), &Theme::wsx()).unwrap()
    }

    fn fleet(working: u32, mergeable: u32) -> SegmentMap {
        let rows = (0..working).map(|_| FleetRow {
            reported: Some(crate::data::store::ReportedState::Working),
            ..Default::default()
        });
        let ready = (0..mergeable).map(|_| FleetRow {
            lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
            review: Some(crate::git::forge::ReviewDecision::Approved),
            ..Default::default()
        });
        FleetStats::from_rows(rows.chain(ready), 1, 0).to_vars()
    }

    #[test]
    fn module_renders_in_the_dashboard_footer_and_drops_zero_items() {
        let specs = specs_with(
            "[module.pipe]\nformat = \"([$working wrk](fg:ok)  )([$mergeable rdy](fg:merged))\"\n[dashboard_footer]\nright_format = \"$pipe\"\n",
        );
        let theme = Theme::wsx();
        let out = dashboard_footer(
            &specs,
            &theme,
            &DashboardFooterInputs {
                activity: &[],
                version: "0.1.0",
                window_label: "24h",
                workspace_selected: false,
                fleet: &fleet(3, 0),
            },
            80,
        );
        let text = plain(&out.line);
        assert!(text.ends_with("3 wrk  "), "{text:?}");
        assert!(
            !text.contains("rdy"),
            "zero mergeable drops its group: {text:?}"
        );
        assert!(
            out.hits.iter().all(|h| matches!(h.hit, Hit::Key(_))),
            "modules carry no hit"
        );
    }

    #[test]
    fn module_renders_in_the_attached_bottom_bar() {
        let specs = specs_with(
            "[module.pipe]\nformat = \"$working working\"\n[attached_bottom]\nright_format = \"$pipe\"\n",
        );
        let theme = Theme::wsx();
        let f = fleet(2, 0);
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
                tags: &[],
                procs: 0,
                diff: None,
                pr: None,
                model_tokens: None,
                agents: &[],
                active_agent: None,
                fleet: &f,
            },
            80,
            80,
        );
        assert!(
            plain(&bars.bottom.line).ends_with("2 working"),
            "{:?}",
            plain(&bars.bottom.line)
        );
    }

    #[test]
    fn module_priority_drops_before_keys_when_narrow() {
        let specs = specs_with(
            "[module.pipe]\nformat = \"$working working across the whole fleet right now\"\npriority = 10\n[dashboard_footer]\nformat = \"$keys\"\nright_format = \"$pipe\"\n",
        );
        let theme = Theme::wsx();
        let f = fleet(2, 0);
        let render = |w: u16| {
            plain(
                &dashboard_footer(
                    &specs,
                    &theme,
                    &DashboardFooterInputs {
                        activity: &[],
                        version: "0.1.0",
                        window_label: "24h",
                        workspace_selected: false,
                        fleet: &f,
                    },
                    w,
                )
                .line,
            )
        };
        assert!(render(160).contains("2 working"));
        let narrow = render(60);
        assert!(!narrow.contains("2 working"), "{narrow:?}");
        assert!(narrow.contains("nav"), "keys survive: {narrow:?}");
    }

    #[test]
    fn dashboard_detail_renders_a_module() {
        let specs = specs_with(
            "[module.pipe]\nformat = \"$working w\"\n[dashboard_detail]\nformat = \"$pipe\"\n",
        );
        let theme = Theme::wsx();
        let out = dashboard_detail(&specs, &theme, &[], &fleet(1, 0), 40);
        assert!(
            plain(&out.line).starts_with("1 w"),
            "{:?}",
            plain(&out.line)
        );
    }
}
