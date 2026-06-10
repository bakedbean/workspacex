//! Integration tests using ratatui's TestBackend. Exercise the full
//! V5 render path against the design fixture.

use super::*;
use crate::data::store::{Repo, RepoId, WorkspaceId};
use crate::ui::dashboard::fixture;
use crate::ui::dashboard::layout::GroupMode;
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
        chronology_config: None,
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
                    status: w.status,
                    name: w.name.clone(),
                    branch: w.branch.clone(),
                    procs: w.procs,
                    diff: Some(crate::git::DiffStats {
                        added: w.diff_added,
                        removed: w.diff_removed,
                    }),
                    last_message: w.last_message.clone(),
                    ago_secs: w.ago_secs,
                    selected: false,
                    yolo: false,
                    setup_failed: false,
                    lifecycle: None,
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

#[test]
fn by_repo_render_includes_chrome_status_strip_and_a_repo_header() {
    let lines = render_to_strings(GroupMode::Repo);
    let joined = lines.join("\n");
    assert!(joined.contains("wsx · dashboard"), "{joined}");
    assert!(joined.contains("? 2 question"), "status strip: {joined}");
    assert!(joined.contains("▾ wsx"), "wsx repo header: {joined}");
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
        joined.contains("wsx/theme-tokens") || joined.contains("wsx/repo-overview"),
        "flat row repo/name format"
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
        .find(|w| w.row.name == "theme-tokens")
        .map(|w| crate::app::SelectionTarget::Workspace(w.workspace_id))
        .unwrap();
    let activity: Vec<u32> = vec![1; 24];
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
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
        .find(|w| w.row.name == "theme-tokens")
        .map(|w| w.workspace_id)
        .unwrap();
    let target = crate::app::SelectionTarget::Workspace(target_id);
    let activity: Vec<u32> = vec![1; 24];
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
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
    let repos: Vec<Repo> = fixtures
        .iter()
        .enumerate()
        .map(|(i, r)| fake_repo(i as i64 + 1, &r.name, &r.path))
        .collect();
    let (repo_refs, workspaces) = build_inputs(&fixtures, &repos);
    // Map workspace name → workspace_id so we can assert on names.
    let id_for: std::collections::HashMap<String, crate::data::store::WorkspaceId> = workspaces
        .iter()
        .map(|w| (w.row.name.clone(), w.workspace_id))
        .collect();
    let activity: Vec<u32> = vec![1; 24];
    let inputs = DashboardInputs {
        repos: repo_refs,
        workspaces,
        activity: &activity,
        column_widths: row::ColumnWidths::default(),
    };
    let state = DashboardState {
        group_mode: GroupMode::Repo,
        ..Default::default()
    };
    let targets = visible_targets(&inputs, &state);
    // All fixtures have sort_order: 0, so repo position is stable input
    // order. This test checks intra-repo workspace priority ordering:
    // within the 'wsx' repo, workspaces should appear in status-priority
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
        SelectionTarget::Workspace(id_for["theme-tokens"]),
        "stalled first"
    );
    assert_eq!(
        targets[wsx_header_pos + 2],
        SelectionTarget::Workspace(id_for["repo-overview"]),
        "question second"
    );
}
