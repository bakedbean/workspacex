//! Integration tests using ratatui's TestBackend. Exercise the full
//! V5 render path against the design fixture.

use super::*;
use crate::data::store::{Repo, RepoId, WorkspaceId};
use crate::pty::session::AgentKind;
use crate::ui::dashboard::column_content::{ColumnBody, ColumnEmphasis, RowColumn};
use crate::ui::dashboard::fixture;
use crate::ui::dashboard::layout::GroupMode;
use crate::ui::dashboard::row::MAX_AGENT_WIDTH;
use crate::ui::theme::Theme;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

fn fake_repo(id: i64, name: &str, path: &str) -> Repo {
    Repo {
        id: RepoId(id),
        name: name.to_string(),
        path: PathBuf::from(path),
        branch_prefix: String::new(),
        custom_instructions: None,
        setup_script: None,
        archive_script: None,
        pinned_commands: None,
        related_repos: None,
        base_branch: None,
        detail_bar_config: None,
        created_at: 0,
        sort_order: 0,
    }
}

fn build_inputs<'a>(
    fixtures: &'a [fixture::FixtureRepo],
    repos: &'a [Repo],
) -> (Vec<&'a Repo>, Vec<WorkspaceItem<'a>>) {
    let mut wsks: Vec<WorkspaceItem<'a>> = Vec::new();
    for (repo, fr) in repos.iter().zip(fixtures.iter()) {
        for (i, w) in fr.workspaces.iter().enumerate() {
            let id = WorkspaceId((repo.id.0 * 100) + i as i64);
            wsks.push(WorkspaceItem {
                repo,
                workspace_id: id,
                status: w.status,
                row: row::RowInputs {
                    agent: crate::pty::session::AgentKind::Claude,
                    peers: Vec::new(),
                    status: w.status,
                    branch: w.branch.clone(),
                    pr_number: None,
                    procs: w.procs,
                    diff: Some(crate::git::DiffStats {
                        added: w.diff_added,
                        removed: w.diff_removed,
                    }),
                    column: w.last_message.clone().map(|t| RowColumn {
                        token: "idle".to_string(),
                        reported: false,
                        body: ColumnBody::Fallback {
                            text: t,
                            emphasis: ColumnEmphasis::Dim,
                        },
                    }),
                    ago_secs: w.ago_secs,
                    selected: false,
                    yolo: false,
                    badge: None,
                    undelivered_mail: false,
                    shared: false,
                    shared_active: false,
                    lifecycle: None,
                    review: None,
                    nerd_fonts: false,
                    workspace_id: id,
                    has_multi_pane_layout: false,
                },
            });
        }
    }
    (repos.iter().collect(), wsks)
}

fn render_to_strings(group: GroupMode) -> Vec<String> {
    let fixtures = fixture::repos();
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    let activity: Vec<u32> = (0..24).collect();
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &Default::default(),
        nerd_fonts: false,
    };
    let mut state = DashboardState {
        group_mode: group,
        ..Default::default()
    };
    let theme = Theme::wsx();
    let backend = TestBackend::new(160, 40);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, f.area(), &inputs, &mut state, 0, &theme))
        .unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

/// Only repos the `GithubRemotes` cache says live on github.com get a
/// header PR link, and each returned rect must land exactly on the glyph
/// painted in the buffer — not on the padding beside it.
#[test]
fn repo_pr_link_rects_land_on_rendered_glyphs() {
    let fixtures = fixture::repos();
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    // Every repo but the first is on GitHub.
    let github = crate::git::github_remotes::GithubRemotes::probed(
        repos.iter().map(|r| (r.id, r.id != repos[0].id)),
    );
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    let activity: Vec<u32> = (0..24).collect();
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &github,
        nerd_fonts: false,
    };
    let mut state = DashboardState {
        group_mode: GroupMode::Repo,
        ..Default::default()
    };
    let theme = Theme::wsx();
    let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut rects: Vec<(RepoId, Rect)> = Vec::new();
    term.draw(|f| {
        rects = render_without_footer(f, f.area(), &inputs, &mut state, 0, &theme).repo_pr_links
    })
    .unwrap();
    let buf = term.backend().buffer().clone();

    let linked: Vec<RepoId> = rects.iter().map(|(id, _)| *id).collect();
    let expected: Vec<RepoId> = repos
        .iter()
        .map(|r| r.id)
        .filter(|id| *id != repos[0].id)
        .collect();
    assert_eq!(linked, expected, "only GitHub repos are clickable");

    for (repo_id, r) in &rects {
        let text: String = (r.x..r.x + r.width)
            .map(|x| buf[(x, r.y)].symbol().to_string())
            .collect();
        assert_eq!(text, "PR", "rect for repo {repo_id:?} must cover its link");
    }
}

/// A repo name of double-width characters pushes the link two cells right
/// per character. The hit span must be measured in terminal cells, like the
/// rendering is — counting Unicode scalars would leave the rect behind.
#[test]
fn repo_pr_link_rect_survives_a_wide_repo_name() {
    let fixtures = fixture::repos();
    let mut repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    repos[0].name = "倉庫名".to_string(); // 3 chars, 6 cells
    let github = crate::git::github_remotes::GithubRemotes::probed([(repos[0].id, true)]);
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    let activity: Vec<u32> = (0..24).collect();
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &github,
        nerd_fonts: false,
    };
    let mut state = DashboardState {
        group_mode: GroupMode::Repo,
        ..Default::default()
    };
    let theme = Theme::wsx();
    let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut rects: Vec<(RepoId, Rect)> = Vec::new();
    term.draw(|f| {
        rects = render_without_footer(f, f.area(), &inputs, &mut state, 0, &theme).repo_pr_links
    })
    .unwrap();
    let buf = term.backend().buffer().clone();

    let (_, r) = rects.first().expect("the wide-named repo is linked");
    let text: String = (r.x..r.x + r.width)
        .map(|x| buf[(x, r.y)].symbol().to_string())
        .collect();
    assert_eq!(text, "PR", "rect must track the link past a wide name");
}

/// The by-attention view has no repo headers, so it offers no repo links.
#[test]
fn by_attention_view_has_no_repo_pr_links() {
    let inputs = fixture_dashboard_inputs();
    let mut state = DashboardState {
        group_mode: GroupMode::Attention,
        ..Default::default()
    };
    let theme = Theme::wsx();
    let mut term = Terminal::new(TestBackend::new(160, 40)).unwrap();
    let mut targets = ListClickTargets::default();
    term.draw(|f| targets = render_without_footer(f, f.area(), &inputs, &mut state, 0, &theme))
        .unwrap();
    assert!(targets.repo_pr_links.is_empty());
}

/// Render `render_without_footer` with every fixture workspace given an open
/// PR, and assert each returned PR-chip rect lands exactly on the chip text
/// painted in the buffer. Shared by the per-group-mode and scrolled tests.
fn assert_pr_rects_match_buffer(group: GroupMode, height: u16, select_last: bool) {
    let fixtures = fixture::repos();
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    let (repo_refs, mut workspaces) = build_inputs(&fixtures, &repos);
    for w in &mut workspaces {
        w.row.lifecycle = Some(crate::git::forge::BranchLifecycle::PrOpen);
        w.row.pr_number = Some(100 + w.workspace_id.0 as u32);
    }
    // One chipless row: its workspace must not get a click rect.
    let no_pr_id = workspaces[0].workspace_id;
    workspaces[0].row.lifecycle = None;
    workspaces[0].row.pr_number = None;
    let last_id = workspaces.last().unwrap().workspace_id;
    let activity: Vec<u32> = (0..24).collect();
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &Default::default(),
        nerd_fonts: false,
    };
    let mut state = DashboardState {
        group_mode: group,
        ..Default::default()
    };
    if select_last {
        // Selecting the last row forces the list to scroll on a short
        // terminal, so the rects must survive a non-zero list offset.
        state.selection = Some(SelectionTarget::Workspace(last_id));
    }
    let theme = Theme::wsx();
    let backend = TestBackend::new(160, height);
    let mut term = Terminal::new(backend).unwrap();
    let mut rects: Vec<(WorkspaceId, Rect)> = Vec::new();
    term.draw(|f| {
        rects = render_without_footer(f, f.area(), &inputs, &mut state, 0, &theme).pr_chips
    })
    .unwrap();
    let buf = term.backend().buffer().clone();
    assert!(!rects.is_empty(), "chip rects returned ({group:?})");
    assert!(
        !rects.iter().any(|(id, _)| *id == no_pr_id),
        "a chipless row must not be clickable ({group:?})"
    );
    for (ws_id, r) in &rects {
        let text: String = (r.x..r.x + r.width)
            .map(|x| buf[(x, r.y)].symbol().to_string())
            .collect();
        let expected = format!("⏺ #{} open", 100 + ws_id.0);
        assert_eq!(
            text, expected,
            "rect for workspace {ws_id:?} must cover its chip ({group:?})"
        );
    }
    if select_last {
        assert!(
            state.list_state.offset() > 0,
            "short terminal + last-row selection should scroll the list"
        );
        assert!(
            rects.iter().any(|(id, _)| *id == last_id),
            "the scrolled-to row's chip must be clickable"
        );
    }
}

#[test]
fn pr_chip_rects_land_on_rendered_chips_in_both_group_modes() {
    assert_pr_rects_match_buffer(GroupMode::Repo, 40, false);
    assert_pr_rects_match_buffer(GroupMode::Attention, 40, false);
}

#[test]
fn pr_chip_rects_survive_list_scroll() {
    assert_pr_rects_match_buffer(GroupMode::Repo, 12, true);
}

#[test]
fn by_repo_render_includes_chrome_status_strip_and_a_repo_header() {
    let lines = render_to_strings(GroupMode::Repo);
    let joined = lines.join("\n");
    assert!(joined.contains("workspace x · dashboard"), "{joined}");
    assert!(joined.contains("? 2 question"), "status strip: {joined}");
    // wsx header: name right-justified, path left-justified after it.
    assert!(
        joined.contains("/home/eben/workspace/wsx"),
        "wsx repo header: {joined}"
    );
    assert!(
        joined.contains("theme-tokens"),
        "stalled workspace row: {joined}"
    );
    assert!(joined.contains("24h "), "footer sparkline label");
}

#[test]
fn footer_row_paints_chip_bg_but_no_bar_bg() {
    // End-to-end check: after the whole render path runs, the bottom row
    // (the footer) must contain the chip background (bg_soft fills the
    // cells behind each key chord) and must NOT contain any bg_alt
    // bar-bg fill — the footer chrome blends flat with the main bg.
    let fixtures = fixture::repos();
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    let activity: Vec<u32> = (0..24).collect();
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &Default::default(),
        nerd_fonts: false,
    };
    let mut state = DashboardState::default();
    let theme = Theme::wsx();
    let backend = TestBackend::new(160, 40);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, f.area(), &inputs, &mut state, 0, &theme))
        .unwrap();
    let buf = term.backend().buffer();
    let footer_y = buf.area.height - 1;
    let mut saw_bar = false;
    let mut saw_chip = false;
    for x in 0..buf.area.width {
        match buf[(x, footer_y)].bg {
            b if b == theme.bg_alt => saw_bar = true,
            b if b == theme.bg_soft => saw_chip = true,
            _ => {}
        }
    }
    assert!(
        !saw_bar,
        "footer row should NOT contain bg_alt bar-bg cells"
    );
    assert!(saw_chip, "footer row should contain bg_soft chip-bg cells");
}

#[test]
fn by_attention_render_emits_section_headers() {
    let lines = render_to_strings(GroupMode::Attention);
    let joined = lines.join("\n");
    assert!(joined.contains("◆ NEEDS ATTENTION"), "{joined}");
    assert!(joined.contains("● WORKING"), "{joined}");
    assert!(joined.contains("✓ RECENT"), "{joined}");
    assert!(joined.contains("  QUIET REPOS"), "{joined}");
    assert!(
        joined.contains("wsx/bakedbean/theme-tokens")
            || joined.contains("wsx/bakedbean/repo-overview"),
        "flat row repo/branch format"
    );
}

#[test]
fn render_sets_list_state_to_selected_workspace_index() {
    let fixtures = fixture::repos();
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    let target = workspaces
        .iter()
        .find(|w| w.row.branch == "bakedbean/theme-tokens")
        .map(|w| crate::app::SelectionTarget::Workspace(w.workspace_id))
        .unwrap();
    let activity: Vec<u32> = vec![1; 24];
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &Default::default(),
        nerd_fonts: false,
    };
    let mut state = DashboardState {
        group_mode: GroupMode::Repo,
        selection: Some(target),
        ..Default::default()
    };
    let theme = Theme::wsx();
    let backend = TestBackend::new(160, 40);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, f.area(), &inputs, &mut state, 0, &theme))
        .unwrap();
    assert!(
        state.list_state.selected().is_some(),
        "list_state should have a selected index when selection is set"
    );
}

#[test]
fn selected_workspace_row_renders_with_thicker_gutter() {
    // End-to-end: when a workspace is selected, the rendered buffer for
    // that row's status gutter (column 1, immediately right of the
    // per-agent identity bar in column 0) must be `▍` (thicker bar).
    // Other rows keep the thin `▎` gutter. This guards against the wiring
    // regressing independently of row::render unit tests.
    let fixtures = fixture::repos();
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    let target_id = workspaces
        .iter()
        .find(|w| w.row.branch == "bakedbean/theme-tokens")
        .map(|w| w.workspace_id)
        .unwrap();
    let target = crate::app::SelectionTarget::Workspace(target_id);
    let activity: Vec<u32> = vec![1; 24];
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &Default::default(),
        nerd_fonts: false,
    };
    let mut state = DashboardState {
        group_mode: GroupMode::Repo,
        selection: Some(target),
        ..Default::default()
    };
    let theme = Theme::wsx();
    let backend = TestBackend::new(160, 40);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| render(f, f.area(), &inputs, &mut state, 0, &theme))
        .unwrap();
    let buf = term.backend().buffer().clone();
    let mut saw_thick = 0;
    for y in 0..buf.area.height {
        let gutter_cell = buf[(1, y)].symbol().to_string();
        if gutter_cell == "▍" {
            saw_thick += 1;
        }
    }
    assert_eq!(
        saw_thick, 1,
        "exactly one row should render the thick selection gutter"
    );
}

#[test]
fn visible_targets_by_repo_matches_render_order() {
    use crate::app::SelectionTarget;
    let fixtures = fixture::repos();
    let mut repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    // Give every repo a DISTINCT, deliberately out-of-input-order
    // `sort_order` so the expected render/nav order differs from the
    // fixture input order. Without this, all fixtures share sort_order==0
    // and a cross-repo ordering regression would slip through unnoticed.
    // Reversing the input index yields a unique key per repo whose
    // ascending sort is the *reverse* of the fixture/input order.
    let n = repos.len() as i64;
    for (i, repo) in repos.iter_mut().enumerate() {
        repo.sort_order = (n - 1 - i as i64) * 10;
    }
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    // Map workspace branch → workspace_id so we can assert on rows.
    let id_for: std::collections::HashMap<String, crate::data::store::WorkspaceId> = workspaces
        .iter()
        .map(|w| (w.row.branch.clone(), w.workspace_id))
        .collect();
    let activity: Vec<u32> = vec![1; 24];
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &Default::default(),
        nerd_fonts: false,
    };
    let state = DashboardState {
        group_mode: GroupMode::Repo,
        ..Default::default()
    };
    let targets = visible_targets(&inputs, &state);

    // ---- Cross-repo ordering (the lockstep this test guards) ----
    // The nav builder (`visible_targets`) and the renderer
    // (`render_by_repo` -> `by_repo::order_repos`) must emit repo headers
    // in the SAME order, namely ascending by persisted `sort_order`.
    // Repos were assigned distinct, reversed sort_order above, so the
    // expected order is the reverse of the fixture/input order — proving
    // both paths actually sort and don't just echo input order.
    let nav_repo_order: Vec<RepoId> = targets
        .iter()
        .filter_map(|t| match t {
            SelectionTarget::Repo(id) => Some(*id),
            _ => None,
        })
        .collect();
    // Reproduce the renderer's repo ordering via the exact function it
    // uses (`by_repo::order_repos`), built from the same `inputs.repos`.
    let mut render_views: Vec<crate::ui::dashboard::by_repo::RepoView<'_>> = inputs
        .repos
        .iter()
        .map(|r| crate::ui::dashboard::by_repo::RepoView {
            id: r.id.0 as u64,
            name: &r.name,
            path: r.path.to_string_lossy().into_owned(),
            counts: Default::default(),
            expanded: true,
            sort_order: r.sort_order,
            workspaces: Vec::new(),
            show_pr_link: false,
            nerd_fonts: false,
        })
        .collect();
    crate::ui::dashboard::by_repo::order_repos(&mut render_views);
    let render_repo_order: Vec<RepoId> = render_views.iter().map(|v| RepoId(v.id as i64)).collect();
    // Expected: repos sorted ascending by the sort_order we injected,
    // which is the reverse of input order.
    let mut expected_order: Vec<RepoId> = inputs.repos.iter().map(|r| r.id).collect();
    expected_order.sort_by_key(|id| {
        inputs
            .repos
            .iter()
            .find(|r| r.id == *id)
            .unwrap()
            .sort_order
    });
    assert_eq!(
        nav_repo_order, render_repo_order,
        "nav and render must agree on cross-repo ordering"
    );
    assert_eq!(
        nav_repo_order, expected_order,
        "both paths must order repos ascending by sort_order"
    );
    // Sanity: the chosen sort_order really does reorder repos (so the
    // assertions above are not trivially satisfied by input order).
    let input_order: Vec<RepoId> = inputs.repos.iter().map(|r| r.id).collect();
    assert_ne!(
        nav_repo_order, input_order,
        "fixture must exercise a non-trivial reordering"
    );

    // ---- Intra-repo workspace ordering (unchanged) ----
    // Within the 'wsx' repo, workspaces should appear in status-priority
    // order (theme-tokens=Stalled first, then repo-overview=Question,
    // list-virtualization=Waiting, tech-stack-question=Complete).
    let wsx_repo_id = inputs.repos.iter().find(|r| r.name == "wsx").unwrap().id;
    let wsx_header_pos = targets
        .iter()
        .position(|t| matches!(t, SelectionTarget::Repo(id) if *id == wsx_repo_id))
        .expect("wsx header present");
    // Expect: header, then 4 workspaces in priority order.
    assert_eq!(
        targets[wsx_header_pos + 1],
        SelectionTarget::Workspace(id_for["bakedbean/theme-tokens"]),
        "stalled first"
    );
    assert_eq!(
        targets[wsx_header_pos + 2],
        SelectionTarget::Workspace(id_for["bakedbean/repo-overview"]),
        "question second"
    );
}

#[test]
fn repo_order_breaks_sort_order_ties_by_id_in_lockstep() {
    use crate::app::SelectionTarget;
    // Two repos deliberately share a sort_order (a tie that could only arise
    // from a manual DB edit). The immutable id tiebreaker must produce a total,
    // deterministic order — ascending id within the tie — and the nav builder
    // (`visible_targets`) must agree with the renderer (`order_repos`) exactly.
    // Ids/input order are arranged so the correct output is NOT the input
    // order, so the assertions can't pass by accident.
    let mut repos = [
        fake_repo(3, "gamma", "/tmp/g"),
        fake_repo(1, "alpha", "/tmp/a"),
        fake_repo(2, "beta", "/tmp/b"),
    ];
    repos[0].sort_order = 5; // gamma (id 3)
    repos[1].sort_order = 5; // alpha (id 1) — ties gamma
    repos[2].sort_order = 1; // beta  (id 2)

    let activity: Vec<u32> = vec![0; 24];
    let inputs = DashboardInputs {
        repos: repos.iter().collect(),
        workspaces: Vec::new(),
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: &Default::default(),
        nerd_fonts: false,
    };
    let state = DashboardState {
        group_mode: GroupMode::Repo,
        ..Default::default()
    };

    // Total order ascending by (sort_order, id): beta(1,2), alpha(5,1), gamma(5,3).
    let expected = vec![RepoId(2), RepoId(1), RepoId(3)];

    let targets = visible_targets(&inputs, &state);
    let nav: Vec<RepoId> = targets
        .iter()
        .filter_map(|t| match t {
            SelectionTarget::Repo(id) => Some(*id),
            _ => None,
        })
        .collect();

    let mut views: Vec<crate::ui::dashboard::by_repo::RepoView<'_>> = inputs
        .repos
        .iter()
        .map(|r| crate::ui::dashboard::by_repo::RepoView {
            id: r.id.0 as u64,
            name: &r.name,
            path: r.path.to_string_lossy().into_owned(),
            counts: Default::default(),
            expanded: true,
            sort_order: r.sort_order,
            workspaces: Vec::new(),
            show_pr_link: false,
            nerd_fonts: false,
        })
        .collect();
    crate::ui::dashboard::by_repo::order_repos(&mut views);
    let render: Vec<RepoId> = views.iter().map(|v| RepoId(v.id as i64)).collect();

    assert_eq!(nav, expected, "nav breaks sort_order ties by ascending id");
    assert_eq!(
        render, expected,
        "render breaks sort_order ties by ascending id"
    );
    assert_eq!(nav, render, "nav and render agree under a sort_order tie");
}

fn base_row() -> row::RowInputs {
    row::RowInputs {
        agent: crate::pty::session::AgentKind::Claude,
        peers: Vec::new(),
        status: crate::ui::dashboard::status::Status::Idle,
        branch: "bb/some-branch".to_string(),
        pr_number: None,
        procs: 0,
        diff: None,
        column: None,
        ago_secs: None,
        selected: false,
        yolo: false,
        badge: None,
        undelivered_mail: false,
        shared: false,
        shared_active: false,
        lifecycle: None,
        review: None,
        nerd_fonts: false,
        workspace_id: WorkspaceId(1),
        has_multi_pane_layout: false,
    }
}

fn item_with_column<'a>(repo: &'a Repo, column: Option<RowColumn>) -> WorkspaceItem<'a> {
    WorkspaceItem {
        repo,
        workspace_id: WorkspaceId(1),
        status: crate::ui::dashboard::status::Status::Idle,
        row: row::RowInputs {
            column,
            ..base_row()
        },
    }
}

#[test]
fn matches_filter_matches_status_token() {
    let repo = fake_repo(1, "repo", "/tmp/repo");
    let item = item_with_column(
        &repo,
        Some(RowColumn {
            token: "working".into(),
            reported: false,
            body: ColumnBody::Empty,
        }),
    );
    assert!(matches_filter(&item, "work"));
    assert!(!matches_filter(&item, "blocked"));
}

#[test]
fn matches_filter_matches_recap_segments() {
    let repo = fake_repo(1, "repo", "/tmp/repo");
    let seg = |t: &str| column_content::RecapSegment {
        text: t.into(),
        authored: true,
    };
    let item = item_with_column(
        &repo,
        Some(RowColumn {
            token: "idle".into(),
            reported: false,
            body: ColumnBody::Recap {
                segments: vec![seg("Audit V2 invoices"), seg("fix drift calc")],
            },
        }),
    );
    assert!(matches_filter(&item, "drift"));
    assert!(matches_filter(&item, "audit"));
    assert!(!matches_filter(&item, "nonexistent"));
}

#[test]
fn matches_filter_matches_fallback_text_and_branch() {
    let repo = fake_repo(1, "repo", "/tmp/repo");
    let item = item_with_column(
        &repo,
        Some(RowColumn {
            token: "idle".into(),
            reported: false,
            body: ColumnBody::Fallback {
                text: "migrate auth flow".into(),
                emphasis: ColumnEmphasis::Dim,
            },
        }),
    );
    assert!(matches_filter(&item, "auth"));
    assert!(matches_filter(&item, "some-branch"));
    let bare = item_with_column(&repo, None);
    assert!(!matches_filter(&bare, "anything"));
}

// ---- derived_agent_width fixtures ----

fn row_with_peers(n: usize) -> RowInputs {
    let mut r = base_row();
    r.peers = vec![AgentKind::Codex; n];
    r
}

/// Build a full `DashboardInputs` from the shared design fixture, leaked to
/// `'static` so tests can own it directly (mirrors `render_to_strings`'
/// construction but hands the caller the `DashboardInputs` itself instead of
/// rendering it immediately).
fn fixture_dashboard_inputs() -> DashboardInputs<'static> {
    let fixtures: &'static [fixture::FixtureRepo] = Box::leak(fixture::repos().into_boxed_slice());
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    let repos: &'static [Repo] = Box::leak(repos.into_boxed_slice());
    let (repo_refs, workspaces) = build_inputs(fixtures, repos);
    let activity: &'static [u32] = Box::leak(vec![0u32; 24].into_boxed_slice());
    DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity,
        column_widths: row::ColumnWidths::default(),
        github_remotes: Box::leak(Box::default()),
        nerd_fonts: false,
    }
}

/// Same as `fixture_dashboard_inputs`, but every workspace has an open PR so
/// every row renders a clickable PR chip.
fn fixture_dashboard_inputs_with_pr() -> DashboardInputs<'static> {
    let mut inputs = fixture_dashboard_inputs();
    for w in inputs.workspaces.iter_mut() {
        w.row.lifecycle = Some(crate::git::forge::BranchLifecycle::PrOpen);
        w.row.pr_number = Some(100 + w.workspace_id.0 as u32);
    }
    inputs
}

fn give_workspace_peers(inputs: &mut DashboardInputs<'_>, index: usize, n: usize) {
    inputs.workspaces[index].row.peers = vec![AgentKind::Codex; n];
}

/// Render a single `ListItem` through a 1-row `TestBackend` and read the
/// buffer back as a string, so tests can assert on exactly what a flat list
/// index will paint on screen.
fn item_text(item: &ratatui::widgets::ListItem<'static>) -> String {
    let backend = TestBackend::new(160, 1);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| {
        let list = List::new(vec![item.clone()]);
        f.render_widget(list, f.area());
    })
    .unwrap();
    let buf = term.backend().buffer().clone();
    (0..buf.area.width)
        .map(|x| buf[(x, 0)].symbol().to_string())
        .collect()
}

#[test]
fn derived_agent_width_is_one_when_no_workspace_has_peers() {
    let rows = [row_with_peers(0), row_with_peers(0)];
    assert_eq!(derived_agent_width(rows.iter()), 1);
}

#[test]
fn derived_agent_width_takes_the_max_across_rows() {
    let rows = [row_with_peers(0), row_with_peers(2), row_with_peers(1)];
    assert_eq!(derived_agent_width(rows.iter()), 3);
}

#[test]
fn derived_agent_width_clamps_to_the_cap() {
    let rows = [row_with_peers(9)];
    assert_eq!(derived_agent_width(rows.iter()), MAX_AGENT_WIDTH);
}

#[test]
fn derived_agent_width_of_nothing_is_one() {
    let rows: Vec<RowInputs> = Vec::new();
    assert_eq!(derived_agent_width(rows.iter()), 1);
}

#[test]
fn folded_repos_do_not_widen_the_strip() {
    // A peer-heavy workspace inside a collapsed repo is not drawn, so
    // it must not tax the recap column of the rows that ARE drawn. Fold
    // ONLY the peer-heavy repo, leaving another repo expanded, so the
    // assertion actually distinguishes "folded rows excluded from the
    // derivation" from "folded rows counted" — every workspace has a PR
    // chip (so chips is non-empty either way) and the visible chip's x
    // must land exactly where it would at the default (unwidened) width.
    let mut inputs = fixture_dashboard_inputs_with_pr();
    give_workspace_peers(&mut inputs, 0, 3);
    let folded_repo_id = inputs.workspaces[0].repo.id;
    let mut state = DashboardState::default();
    state.folded.insert(folded_repo_id.0 as u64, true);
    let (items, chips, _) = render_by_repo(&inputs, &mut state, 0, 160, &Theme::wsx());
    assert!(!chips.is_empty(), "other repos still render rows");
    let (ws_id, flat_idx, (x, _w)) = chips[0];
    let rendered = item_text(&items[flat_idx]);
    assert_eq!(
        rendered.chars().position(|c| c == '⏺'),
        Some(x as usize),
        "hit span must match where the chip actually rendered:\n  {rendered:?}"
    );
    let visible_row = &inputs
        .workspaces
        .iter()
        .find(|w| w.workspace_id == ws_id)
        .unwrap()
        .row;
    let (x_unwidened, _) =
        row::pr_chip_hit_span(visible_row, row::ColumnWidths::default()).unwrap();
    assert_eq!(
        x, x_unwidened,
        "the folded repo's 3 peers must not widen the strip for a visible row"
    );
}

#[test]
fn chip_hit_spans_use_the_widened_strip() {
    // The regression this task exists to prevent: hit spans computed at
    // the unwidened width while rows render at the widened one.
    let mut inputs = fixture_dashboard_inputs_with_pr();
    give_workspace_peers(&mut inputs, 0, 2);
    let mut state = DashboardState::default();
    let (items, chips, _) = render_by_repo(&inputs, &mut state, 0, 160, &Theme::wsx());
    let (ws_id, flat_idx, (x, _w)) = chips[0];
    let rendered = item_text(&items[flat_idx]);
    assert_eq!(
        rendered.chars().position(|c| c == '⏺'),
        Some(x as usize),
        "hit span must match where the chip actually rendered:\n  {rendered:?}"
    );
    // Prove the derived width actually reaches the render, not just that
    // both consumers happen to agree at whatever (possibly unwidened)
    // width they were both given. 2 peers + primary = 3 live agents, one
    // more cell than the default width of 1, so every chip — including
    // rows with no peers of their own — must sit 2 columns further right
    // than it would at the unwidened default.
    let row_for_chip = &inputs
        .workspaces
        .iter()
        .find(|w| w.workspace_id == ws_id)
        .unwrap()
        .row;
    let (x_default, _) = row::pr_chip_hit_span(row_for_chip, row::ColumnWidths::default()).unwrap();
    assert_eq!(
        x,
        x_default + 2,
        "derived width must reach the render: a 3-wide strip shifts every chip 2 columns"
    );
}
