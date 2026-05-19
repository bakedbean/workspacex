use super::*;
use crate::store::{RepoId, SetupStatus, WorkspaceId};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::path::PathBuf;

fn repo(id: i64, name: &str) -> Repo {
    Repo {
        id: RepoId(id),
        name: name.into(),
        path: PathBuf::from(format!("/repos/{name}")),
        branch_prefix: "".into(),
        custom_instructions: None,
        setup_script: None,
        archive_script: None,
        pinned_commands: None,
        related_repos: None,
        base_branch: None,
        created_at: 0,
    }
}

fn workspace(id: i64, repo_id: i64, name: &str, branch: &str) -> Workspace {
    Workspace {
        id: WorkspaceId(id),
        repo_id: RepoId(repo_id),
        name: name.into(),
        branch: branch.into(),
        worktree_path: PathBuf::from(format!("/w/{name}")),
        state: WorkspaceState::Ready,
        setup_status: SetupStatus::Ok,
        created_at: 0,
        yolo: false,
    }
}

fn t() -> Theme {
    Theme::default_theme()
}

fn dump(term: &Terminal<TestBackend>, w: u16, h: u16) -> String {
    let buf = term.backend().buffer();
    let mut s = String::new();
    for y in 0..h {
        let line: String = (0..w).map(|x| buf[(x, y)].symbol().to_string()).collect();
        s.push_str(line.trim_end());
        s.push('\n');
    }
    s
}

#[test]
fn renders_repo_header_with_indented_workspace() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| {
        render(
            f,
            f.area(),
            &items,
            Some(SelectionTarget::Workspace(WorkspaceId(1))),
            false,
            &t(),
            &mut state,
        )
    })
    .unwrap();
    let text = dump(&term, 120, 8);
    assert!(
        text.contains("demo") && text.contains("/repos/demo"),
        "missing header: {text}"
    );
    assert!(text.contains("alpha"), "missing workspace name: {text}");
    assert!(text.contains("active"), "missing activity column: {text}");
}

#[test]
fn renders_empty_repo_hint() {
    let mut term = Terminal::new(TestBackend::new(80, 8)).unwrap();
    let r = repo(1, "empty");
    let items = vec![Item::Header { repo: &r }, Item::EmptyHint];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 80, 8);
    assert!(text.contains("empty") && text.contains("/repos/empty"));
    assert!(text.contains("press n to create"));
}

#[test]
fn renders_multiple_repos_grouped() {
    let mut term = Terminal::new(TestBackend::new(120, 15)).unwrap();
    let r1 = repo(1, "first");
    let r2 = repo(2, "second");
    let w1 = workspace(1, 1, "alpha", "alpha");
    let w2 = workspace(2, 2, "beta", "beta");
    let items = vec![
        Item::Header { repo: &r1 },
        Item::Workspace {
            repo: &r1,
            workspace: &w1,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
        Item::Spacer,
        Item::Header { repo: &r2 },
        Item::Workspace {
            repo: &r2,
            workspace: &w2,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 15);
    let first_pos = text.find("first").expect("first repo header");
    let alpha_pos = text.find("alpha").expect("alpha workspace");
    let second_pos = text.find("second").expect("second repo header");
    let beta_pos = text.find("beta").expect("beta workspace");
    assert!(
        first_pos < alpha_pos && alpha_pos < second_pos && second_pos < beta_pos,
        "ordering wrong:\n{text}"
    );
}

#[test]
fn renders_status_counts_plain() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let st = crate::git::WorkspaceStatus {
        modified: 3,
        untracked: 1,
        ahead: 2,
        behind: 0,
    };
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: Some(st),
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    // Check only the content lines, not the footer (which is the last line).
    let lines: Vec<&str> = text.lines().collect();
    let content = if lines.len() > 1 {
        lines[..lines.len() - 1].join("\n")
    } else {
        text.clone()
    };
    assert!(content.contains("~3"), "missing modified count: {content}");
    assert!(content.contains("?1"), "missing untracked count: {content}");
    assert!(
        content.contains("\u{2191}2"),
        "missing ahead count: {content}"
    );
    assert!(
        !content.contains("\u{2193}"),
        "should not show zero behind: {content}"
    );
}

#[test]
fn renders_status_counts_nerd() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let st = crate::git::WorkspaceStatus {
        modified: 2,
        untracked: 0,
        ahead: 0,
        behind: 1,
    };
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: Some(st),
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, true, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    assert!(text.contains("\u{e0a0}"), "missing branch glyph: {text}");
    assert!(text.contains("\u{f459}"), "missing modified glyph: {text}");
    assert!(text.contains("\u{f063}"), "missing behind glyph: {text}");
}

#[test]
fn renders_event_subline_when_event_present() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    // Timestamp ~5s ago to exercise the seconds branch of format_age.
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let ev = crate::events::EventSnapshot {
        kind: crate::events::EventKind::AssistantToolUse,
        display: "ran `cargo test`".into(),
        timestamp_ms: now_ms - 5_000,
    };
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: Some(ev),
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    assert!(text.contains("\u{2514}"), "missing └ glyph: {text}");
    assert!(
        text.contains("ran `cargo test`"),
        "missing event body: {text}"
    );
    assert!(text.contains("s ago"), "missing relative time: {text}");
}

#[test]
fn renders_question_glyph_for_awaiting_answer() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: Some(crate::app::StoppedKind::AwaitingAnswer),
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| {
        render(
            f,
            f.area(),
            &items,
            None,
            false, // ASCII (nerd_fonts = false)
            &t(),
            &mut state,
        )
    })
    .unwrap();
    let text = dump(&term, 120, 8);
    assert!(text.contains("?"), "expected '?' attention marker: {text}");
    assert!(
        text.contains("question"),
        "expected 'question' activity label: {text}"
    );
}

#[test]
fn renders_check_glyph_for_complete() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: Some(crate::app::StoppedKind::Complete),
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    assert!(
        text.contains("\u{2713}"),
        "expected '✓' attention marker: {text}"
    );
    assert!(
        text.contains("complete"),
        "expected 'complete' activity label: {text}"
    );
}

#[test]
fn selection_skips_event_subline() {
    // When a workspace has a sub-line, the second workspace's main row
    // should still get the correct selection highlight index — i.e.
    // selecting workspace 2 highlights row 3 (header=0, ws1=1, sub=2, ws2=3),
    // not row 2 (the sub-line).
    let r = repo(1, "demo");
    let w1 = workspace(1, 1, "alpha", "wsx/alpha");
    let w2 = workspace(2, 1, "beta", "wsx/beta");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let ev = crate::events::EventSnapshot {
        kind: crate::events::EventKind::AssistantText,
        display: "thinking…".into(),
        timestamp_ms: now_ms - 1_000,
    };
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w1,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: Some(ev),
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
        Item::Workspace {
            repo: &r,
            workspace: &w2,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut term = Terminal::new(TestBackend::new(120, 10)).unwrap();
    let mut state = DashboardState::default();
    term.draw(|f| {
        render(
            f,
            f.area(),
            &items,
            Some(SelectionTarget::Workspace(WorkspaceId(2))),
            false,
            &t(),
            &mut state,
        )
    })
    .unwrap();
    // The second workspace becomes the 5th list item (index 4): header,
    // rule, ws1 row, ws1 sub-line, ws2 row.
    assert_eq!(state.list_state.selected(), Some(4));
}

#[test]
fn renders_clean_workspace_with_no_status() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let st = crate::git::WorkspaceStatus::default();
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: Some(st),
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    assert!(text.contains("alpha"));
    // Clean workspace should not show any count markers.
    assert!(!text.contains("~"));
    assert!(!text.contains("?"));
}

/// Strip leading list/border decoration so tests can assert on the
/// rendered row's own first character.
fn strip_border_prefix(line: &str) -> &str {
    // Skip the left border glyph (│) and any whitespace immediately after it.
    line.trim_start_matches('\u{2502}').trim_start_matches(' ')
}

#[test]
fn renders_attention_mark_when_needs_attention() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    // Look for the row that has the alpha workspace; assert ! is in the leading column.
    let line = text
        .lines()
        .find(|l| l.contains("alpha"))
        .expect("alpha row");
    let trimmed = strip_border_prefix(line);
    assert!(
        trimmed.starts_with("!"),
        "expected leading ! in: {trimmed:?}"
    );
}

#[test]
fn no_attention_mark_by_default() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let line = text
        .lines()
        .find(|l| l.contains("alpha"))
        .expect("alpha row");
    let trimmed = strip_border_prefix(line);
    assert!(!trimmed.starts_with("!"));
}

#[test]
fn activity_is_right_justified() {
    // 120-wide terminal so the long row doesn't overflow and get
    // truncated — the contract being tested is alignment of the
    // "active" column, not overflow behaviour.
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w_short = workspace(1, 1, "a", "p/a");
    let w_long = workspace(
        2,
        1,
        "very-long-workspace-name-here",
        "prefix/very-long-workspace-name-here",
    );
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w_short,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
        Item::Workspace {
            repo: &r,
            workspace: &w_long,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);

    // The "active" word should end at roughly the same column for both rows.
    let lines: Vec<&str> = text.lines().collect();
    let row_a = lines
        .iter()
        .find(|l| l.contains(" a ") || l.contains("│ a"))
        .expect("row a");
    let row_long = lines
        .iter()
        .find(|l| l.contains("very-long-workspac"))
        .expect("row long");
    // Find the *char* column where "active" starts in each row. Using char
    // indices avoids false drift from multi-byte glyphs (e.g. `…`) that
    // appear in truncated names/branches.
    let char_pos = |row: &str, needle: &str| -> usize {
        let byte_pos = row
            .find(needle)
            .unwrap_or_else(|| panic!("{needle} not in: {row}"));
        row[..byte_pos].chars().count()
    };
    let col_a = char_pos(row_a, "active");
    let col_long = char_pos(row_long, "active");
    // Allow ±2 chars tolerance for unicode glyph cell width quirks.
    let diff = (col_a as isize - col_long as isize).abs();
    assert!(
        diff <= 2,
        "activity column drifted: a={col_a} long={col_long}, lines:\n{text}"
    );
}

#[test]
fn top_summary_shows_total_and_alertable_counts() {
    let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
    let r = repo(1, "demo");
    let w_quiet = workspace(1, 1, "quiet", "wsx/quiet");
    let w_awaiting = workspace(2, 1, "blocked", "wsx/blocked");
    let w_stopped = workspace(3, 1, "thinking", "wsx/thinking");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w_quiet,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
        Item::Workspace {
            repo: &r,
            workspace: &w_awaiting,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: Some(("Bash".into(), 0)),
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
        Item::Workspace {
            repo: &r,
            workspace: &w_stopped,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: Some(crate::app::StoppedKind::Complete),
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 12);
    let top = text.lines().next().unwrap().trim();
    assert!(top.contains("wsx"), "missing 'wsx': {top}");
    assert!(top.contains("3 workspaces"), "missing total: {top}");
    assert!(
        top.contains("1 permission"),
        "missing permission count: {top}"
    );
    assert!(top.contains("1 complete"), "missing complete count: {top}");
}

#[test]
fn top_summary_omits_zero_alertable_counts() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let top = text.lines().next().unwrap().trim();
    assert!(top.contains("1 workspace"), "missing total: {top}");
    assert!(
        !top.contains("permission"),
        "unexpected permission in quiet top: {top}"
    );
    assert!(
        !top.contains("question"),
        "unexpected question in quiet top: {top}"
    );
    assert!(
        !top.contains("complete"),
        "unexpected complete in quiet top: {top}"
    );
}

#[test]
fn top_summary_handles_zero_workspaces() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let items: Vec<Item> = vec![];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let top = text.lines().next().unwrap().trim();
    assert!(top.contains("wsx"), "missing wsx: {top}");
    assert!(top.contains("0 workspaces"), "expected zero count: {top}");
    assert!(!top.contains("permission"), "unexpected permission: {top}");
    assert!(!top.contains("question"), "unexpected question: {top}");
    assert!(!top.contains("complete"), "unexpected complete: {top}");
}

#[test]
fn outer_border_is_absent() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let buf = term.backend().buffer();
    // No vertical-bar border glyphs at either edge.
    let max_x = buf.area.width - 1;
    for y in 0..8u16 {
        assert_ne!(
            buf[(0u16, y)].symbol(),
            "│",
            "expected no border at col 0, row {y}"
        );
        assert_ne!(
            buf[(max_x, y)].symbol(),
            "│",
            "expected no border at right edge col {max_x}, row {y}"
        );
    }
}

#[test]
fn repo_header_renders_with_rule_below() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let lines: Vec<&str> = text.lines().collect();
    // Find the repo header line; the next non-empty line should be a rule.
    let hdr_idx = lines
        .iter()
        .position(|l| l.contains("demo") && l.contains("/repos/demo"))
        .expect("repo header line");
    let rule = lines[hdr_idx + 1];
    let rule_chars: Vec<char> = rule.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        !rule_chars.is_empty() && rule_chars.iter().all(|c| *c == '─'),
        "expected horizontal rule under header, got: {rule:?}"
    );
}

#[test]
fn repo_header_includes_workspace_count() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w1 = workspace(1, 1, "alpha", "wsx/alpha");
    let w2 = workspace(2, 1, "beta", "wsx/beta");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w1,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
        Item::Workspace {
            repo: &r,
            workspace: &w2,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let hdr = text
        .lines()
        .find(|l| l.contains("demo") && l.contains("/repos/demo"))
        .expect("repo header line");
    assert!(
        hdr.contains("· 2"),
        "expected workspace count in header: {hdr}"
    );
}

#[test]
fn renders_awaiting_overrides_activity_and_sub_line() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: true,
            lifecycle: None,
            // 10s ago — well past the 3s threshold.
            awaiting_tool: Some(("Bash".into(), now_ms - 10_000)),
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    assert!(
        text.contains("awaiting"),
        "expected 'awaiting' label: {text}"
    );
    assert!(
        text.contains("awaiting permission: Bash"),
        "expected sub-line: {text}"
    );
    // Should NOT show 'active' even though seconds_since_activity is 0.
    let bad = text
        .lines()
        .any(|l| l.contains("alpha") && l.contains("active"));
    assert!(!bad, "should not show 'active' when awaiting: {text}");
}

#[test]
fn workspace_row_name_padded_to_fixed_width() {
    let mut term = Terminal::new(TestBackend::new(120, 12)).unwrap();
    let r = repo(1, "demo");
    let w_short = workspace(1, 1, "ab", "wsx/ab");
    let w_long = workspace(2, 1, "much-longer-name", "wsx/much-longer-name");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w_short,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
        Item::Workspace {
            repo: &r,
            workspace: &w_long,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let buf = term.backend().buffer();
    // Find the y of each workspace row by scanning the buffer for the
    // name. Then check that the glyph after the name column starts at
    // the same x on both rows.
    let find_y = |needle: &str| -> u16 {
        for y in 0..12u16 {
            let row: String = (0..120u16)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect();
            if row.contains(needle) {
                return y;
            }
        }
        panic!("not found: {needle}");
    };
    let y_short = find_y("ab ");
    let y_long = find_y("much-longer-name");
    // Branch column should start at the same x on both rows.
    // x = indent(2) + attn(1) + sep(1) + dot(1) + sep(1) + name + gutter(3)
    let probe_x: u16 = (2 + 1 + 1 + 1 + 1 + NAME_WIDTH + 3) as u16;
    // After truncation/padding, both rows' branch glyph should appear at
    // probe_x — the branch glyph differs but its starting x should match.
    let short_at = buf[(probe_x, y_short)].symbol();
    let long_at = buf[(probe_x, y_long)].symbol();
    // Both should be non-space (the branch glyph or first branch char).
    assert!(
        short_at != " " && long_at != " ",
        "branch column misaligned: short={short_at:?} long={long_at:?}"
    );
}

#[test]
fn workspace_row_branch_truncated_with_ellipsis() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let very_long_branch =
        "feat/the-quick-brown-fox-jumps-over-the-lazy-dog-multiple-times-in-a-row";
    let w = workspace(1, 1, "alpha", very_long_branch);
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let row = text
        .lines()
        .find(|l| l.contains("alpha"))
        .expect("alpha row");
    assert!(
        row.contains('…'),
        "expected branch ellipsis truncation: {row}"
    );
}

#[test]
fn activity_word_uses_warn_color_for_question_and_awaiting() {
    // Direct unit test of the style mapping.
    let theme = Theme::default_theme();
    let style_question = activity_style("question", &theme);
    let style_awaiting = activity_style("awaiting", &theme);
    assert_eq!(style_question.fg, Some(theme.warn));
    assert_eq!(style_awaiting.fg, Some(theme.warn));
}

#[test]
fn activity_word_uses_ok_color_for_complete() {
    let theme = Theme::default_theme();
    let style = activity_style("complete", &theme);
    assert_eq!(style.fg, Some(theme.ok));
}

#[test]
fn activity_word_uses_ok_color_for_active() {
    let theme = Theme::default_theme();
    let style = activity_style("active", &theme);
    assert_eq!(style.fg, Some(theme.ok));
}

#[test]
fn activity_word_uses_dim_for_off_and_resumable() {
    let theme = Theme::default_theme();
    assert_eq!(activity_style("off", &theme).fg, Some(theme.dim));
    assert_eq!(activity_style("resumable", &theme).fg, Some(theme.dim));
}

#[test]
fn activity_word_uses_default_for_idle() {
    let theme = Theme::default_theme();
    assert_eq!(activity_style("idle", &theme).fg, None);
}

#[test]
fn sub_line_indent_aligns_with_name_column() {
    use crate::events::{EventKind, EventSnapshot};
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "alpha", "wsx/alpha");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let ev = EventSnapshot {
        kind: EventKind::AssistantText,
        display: "hello".into(),
        timestamp_ms: now - 5_000,
    };
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: Some(ev),
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let buf = term.backend().buffer();
    // Find the sub-line row (contains "hello") and confirm the └ glyph
    // is at column 6.
    let mut sub_y = None;
    for y in 0..8u16 {
        let row: String = (0..120u16)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        if row.contains("hello") && row.contains('└') {
            sub_y = Some(y);
            break;
        }
    }
    let y = sub_y.expect("sub-line not found");
    assert_eq!(buf[(6u16, y)].symbol(), "└");
}

#[test]
fn setup_failed_glyph_appears_after_name() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let mut w = workspace(1, 1, "alpha", "wsx/alpha");
    w.setup_status = SetupStatus::Failed;
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let row = text.lines().find(|l| l.contains("alpha")).expect("row");
    assert!(
        row.contains("⚙!"),
        "expected ⚙! setup-failed glyph after name: {row}"
    );
    assert!(
        !row.contains("[setup-failed]"),
        "did not expect the old right-side badge: {row}"
    );
}

#[test]
fn yolo_workspace_name_uses_warn_style() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let mut w = workspace(1, 1, "wild", "wsx/wild");
    w.yolo = true;
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    let theme = t();
    term.draw(|f| render(f, f.area(), &items, None, false, &theme, &mut state))
        .unwrap();
    let buf = term.backend().buffer();
    // Find the row y containing "wild".
    let mut row_y = None;
    for y in 0..8u16 {
        let r: String = (0..120u16)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        if r.contains("wild") {
            row_y = Some(y);
            break;
        }
    }
    let y = row_y.expect("yolo row not found");
    // The name column starts at probe_x_name = 2 (indent) + 1 (attn) + 1
    // (sep) + 1 (glyph) + 1 (sep) = 6.
    let name_x: u16 = 6;
    let cell = &buf[(name_x, y)];
    assert_eq!(cell.symbol(), "w", "expected 'w' at name start: {cell:?}");
    assert_eq!(
        cell.fg,
        theme.warn_style().fg.unwrap(),
        "expected warn_style fg on yolo workspace name"
    );
}

#[test]
fn non_yolo_workspace_name_not_warn_styled() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "tame", "wsx/tame"); // yolo defaults to false
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    let theme = t();
    term.draw(|f| render(f, f.area(), &items, None, false, &theme, &mut state))
        .unwrap();
    let buf = term.backend().buffer();
    let mut row_y = None;
    for y in 0..8u16 {
        let r: String = (0..120u16)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        if r.contains("tame") {
            row_y = Some(y);
            break;
        }
    }
    let y = row_y.expect("tame row not found");
    let cell = &buf[(6u16, y)];
    assert_eq!(cell.symbol(), "t");
    assert_ne!(
        cell.fg,
        theme.warn_style().fg.unwrap(),
        "non-yolo workspace name must not use warn_style fg"
    );
}

#[test]
fn setup_failed_glyph_with_long_name_truncates_correctly() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let mut w = workspace(1, 1, "this-workspace-name-is-very-long", "wsx/long");
    w.setup_status = SetupStatus::Failed;
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: true,
            seconds_since_activity: Some(0),
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let buf = term.backend().buffer();
    let text = dump(&term, 120, 8);
    let row = text
        .lines()
        .find(|l| l.contains("this-workspace") && l.contains("⚙!"))
        .expect("row with long name + setup_failed glyph");
    // Both glyphs present:
    assert!(
        row.contains('…'),
        "expected name truncation ellipsis: {row}"
    );
    assert!(row.contains("⚙!"), "expected setup-failed glyph: {row}");
    // The branch column should still start at the same probe_x as
    // workspace_row_name_padded_to_fixed_width — i.e., the badge
    // does NOT push the branch column to the right.
    let probe_x: u16 = (2 + 1 + 1 + 1 + 1 + NAME_WIDTH + 3) as u16;
    // Find the row's y. Iterate, find the row containing both markers.
    let mut row_y = None;
    for y in 0..8u16 {
        let r: String = (0..120u16)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect();
        if r.contains("this-workspace") && r.contains("⚙!") {
            row_y = Some(y);
            break;
        }
    }
    let y = row_y.expect("row not found in buffer");
    assert_ne!(
        buf[(probe_x, y)].symbol(),
        " ",
        "branch column should start at probe_x even when badge is present"
    );
}

#[test]
fn workspace_row_shows_proc_count_when_nonzero() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "ws", "wsx/ws");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 3,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let row = text.lines().find(|l| l.contains("wsx/ws")).expect("row");
    assert!(row.contains("~3"), "expected `~3` proc count in row: {row}");
}

#[test]
fn workspace_row_hides_proc_count_when_zero() {
    let mut term = Terminal::new(TestBackend::new(120, 8)).unwrap();
    let r = repo(1, "demo");
    let w = workspace(1, 1, "quiet", "wsx/quiet");
    let items = vec![
        Item::Header { repo: &r },
        Item::Workspace {
            repo: &r,
            workspace: &w,
            session_running: false,
            seconds_since_activity: None,
            has_prior_session: false,
            status: None,
            latest_event: None,
            needs_attention: false,
            lifecycle: None,
            awaiting_tool: None,
            stopped_kind: None,
            stalled: false,
            proc_count: 0,
        },
    ];
    let mut state = DashboardState::default();
    term.draw(|f| render(f, f.area(), &items, None, false, &t(), &mut state))
        .unwrap();
    let text = dump(&term, 120, 8);
    let row = text.lines().find(|l| l.contains("quiet")).expect("row");
    assert!(
        !row.contains("~"),
        "did not expect `~` count when proc_count=0: {row}"
    );
}
