//! Host picker, remote workspace list, and ssh attach/detach.

use super::*;
use crate::data::store::Store;
use crate::test_support::EnvGuard;
use std::path::PathBuf;
// `dashboard_renders_split_with_pm_title_when_visible_even_without_session`
// (the PTY-placeholder render test) is gone — the dashboard's PM pane
// now always renders the digest (`render_digest`), whose own render
// tests live in `src/ui/pm_pane.rs::digest_tests`.

use super::common::*;
use crossterm::event::KeyModifiers;

#[tokio::test]
async fn capital_h_opens_host_picker_with_configured_hosts() {
    // Capital H opens a picker over the configured shared hosts,
    // sorted by name (shared_hosts::list already sorts; the picker
    // just snapshots that order). No workspace selection required.
    let (mut app, _) = make_app_with_n_repos(0);
    app.store
        .set_setting("shared_hosts", "mini=eben@mini\nlab=eben@lab")
        .unwrap();
    press(&mut app, 'H', KeyModifiers::SHIFT).await;
    match &app.modal {
        Some(Modal::RemoteHostPicker { hosts, selected }) => {
            assert_eq!(hosts.len(), 2);
            assert_eq!(hosts[0].0, "lab", "expected sorted by name: {hosts:?}");
            assert_eq!(hosts[1].0, "mini");
            assert_eq!(*selected, 0);
        }
        other => panic!("expected host picker, got {other:?}"),
    }
}

#[tokio::test]
async fn capital_h_with_no_hosts_explains_config_edit() {
    let (mut app, _) = make_app_with_n_repos(0);
    press(&mut app, 'H', KeyModifiers::SHIFT).await;
    match &app.modal {
        Some(Modal::Error { message }) => {
            assert!(
                message.contains("config edit shared_hosts"),
                "expected hint to name the setting command: {message}"
            );
        }
        other => panic!("expected error modal, got {other:?}"),
    }
}

#[tokio::test]
async fn enter_in_host_picker_fetches_and_populates_remote_list() {
    // Full round trip without real ssh: WSX_SSH_BIN points at a fake
    // script that emits a valid one-workspace shared-list JSON array
    // (mirrors shared_hosts::tests::fetch_shared_list_parses_fake_ssh_output_and_surfaces_stderr).
    let mut env = EnvGuard::new();
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("fake-ssh-ok.sh");
    std::fs::write(
        &script,
        "#!/bin/sh\necho '[{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"agents\":[]}]'\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", script.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    store.set_setting("shared_hosts", "mini=eben@mini").unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        let h = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('H'),
            crossterm::event::KeyModifiers::SHIFT,
        );
        handle_event(&mut g, &app, CtEvent::Key(h)).await.unwrap();
        assert!(
            matches!(g.modal, Some(Modal::RemoteHostPicker { .. })),
            "expected host picker after H, got {:?}",
            g.modal
        );
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(enter))
            .await
            .unwrap();
        // Immediately after Enter, modal should be RemoteListLoading and
        // a fetch generation should be pending.
        assert!(
            matches!(g.modal, Some(Modal::RemoteListLoading { .. })),
            "expected loading modal immediately after Enter; got {:?}",
            g.modal
        );
        assert!(g.pending_remote_gen.is_some());
    }
    // Wait for the spawned fetch + reconcile to finish.
    wait_until(&app, "remote fetch to finish (list populated)", |g| {
        g.remote_list.is_some() && g.pending_remote_gen.is_none()
    })
    .await;
    let g = app.lock().await;
    let list = g.remote_list.as_ref().expect("remote_list populated");
    assert_eq!(list.host_name, "mini");
    assert_eq!(list.records.len(), 1);
    assert_eq!(list.records[0].workspace, "w");
    assert!(
        matches!(g.modal, Some(Modal::RemoteWorkspaceList { .. })),
        "expected the fetch to open RemoteWorkspaceList; got {:?}",
        g.modal
    );
}

#[tokio::test]
async fn clicking_a_host_chip_fetches_that_hosts_remote_list() {
    // The `$hosts` chip skips the picker: a click on the second chip
    // fetches from the second host's destination, end to end through a
    // fake ssh that logs its argv.
    let mut env = EnvGuard::new();
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("ssh-args.log");
    let script = dir.path().join("fake-ssh-ok.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\necho '[{{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"agents\":[]}}]'\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", script.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        g.remote_hosts_cache = crate::commands::shared_hosts::parse("alpha=me@alpha\nbeta=me@beta");
        g.host_chip_rects = vec![
            (0, ratatui::layout::Rect::new(60, 23, 7, 1)),
            (1, ratatui::layout::Rect::new(68, 23, 6, 1)),
        ];
        let click = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 70,
            row: 23,
            modifiers: crossterm::event::KeyModifiers::empty(),
        };
        handle_event(&mut g, &app, CtEvent::Mouse(click))
            .await
            .unwrap();
        assert!(
            matches!(&g.modal, Some(Modal::RemoteListLoading { host_name }) if host_name == "beta"),
            "{:?}",
            g.modal
        );
    }
    wait_until(&app, "remote fetch to finish (list populated)", |g| {
        g.remote_list.is_some() && g.pending_remote_gen.is_none()
    })
    .await;
    let g = app.lock().await;
    let list = g.remote_list.as_ref().expect("remote_list populated");
    assert_eq!(list.host_name, "beta");
    assert_eq!(list.dest, "me@beta");
    assert!(matches!(g.modal, Some(Modal::RemoteWorkspaceList { .. })));
    let argv = std::fs::read_to_string(&log).unwrap();
    assert!(argv.lines().any(|l| l == "me@beta"), "{argv:?}");
    assert!(!argv.contains("me@alpha"), "{argv:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_remote_spawns_ssh_and_detach_severs_client_only() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    // Fake ssh: prove argv shape, then stream a heartbeat like a remote attach.
    let log = dir.path().join("ssh-args.log");
    let fake = dir.path().join("fake-ssh.sh");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$@\" > {}\nfor i in $(seq 1 60); do echo remote-beat; sleep 1; done\n",
            log.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();

    crate::app::attach_remote(
        &mut app,
        crate::app::RemoteTarget {
            host_name: "mini".into(),
            dest: "eben@mini".into(),
            tmux: "wsx-r-w".into(),
        },
        80,
        24,
    )
    .unwrap();
    assert!(matches!(app.view, crate::ui::View::AttachedRemote));
    let session = app.remote.clone().unwrap();
    // beats arrive through the PTY
    let mut seen = false;
    for _ in 0..50 {
        if session
            .parser
            .lock()
            .unwrap()
            .screen()
            .contents()
            .contains("remote-beat")
        {
            seen = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(seen, "expected remote heartbeat through the PTY");
    // argv shape: -t <dest> -- <ONE pre-quoted remote command>. The remote
    // command must be a single ssh argv element (it still contains
    // multiple shell words), routed through `sh -l` like the list fetch:
    // sshd hands the joined string to the user's default shell in
    // non-login mode (`zsh -c`), which reads only ~/.zshenv — on stock
    // macOS, homebrew's
    // tmux isn't on that PATH ("zsh:1: command not found: tmux" on a real
    // host). `sh -l` reads ~/.profile, the one documented PATH
    // requirement shared with the fetch. The tmux =target stays
    // single-quoted (zsh =word expansion; see #226). `-u` forces UTF-8:
    // the ssh/sh -l context has no locale (LC_CTYPE=C on real hosts), and
    // without it tmux downgrades Unicode line-drawing to ACS/ASCII —
    // rendering pane borders as rows of literal q's.
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(
        args.contains("-t eben@mini -- sh -lc \"tmux -u attach -t '=wsx-r-w'\""),
        "remote command must run tmux -u via a login shell with the =target quoted: {args}"
    );
    assert!(
        !args.contains("-t =wsx-r-w"),
        "unquoted =target must not appear (zsh =-expansion hazard): {args}"
    );
    assert!(
        session.tmux_session.is_none(),
        "remote sessions must never own a local tmux backend"
    );

    crate::app::detach_remote(&mut app);
    assert!(app.remote.is_none() && matches!(app.view, crate::ui::View::Dashboard));
    assert!(app.remote_target.is_none());
}

/// `target.tmux` arrives from the remote host's JSON and is interpolated
/// into a shell-parsed string; `attach_remote` must reject anything
/// outside the sanitized `[A-Za-z0-9_-]` charset at the boundary instead
/// of trusting the wire (a quote or whitespace would break the remote
/// quoting and could inject).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn attach_remote_rejects_unsanitized_tmux_names() {
    let store = Store::open_in_memory().unwrap();
    let mut app = App::new(store, PathBuf::from("/tmp/wsx-test")).unwrap();
    for hostile in ["wsx'; touch /tmp/pwned; echo '", "a b", "", "name\n"] {
        let err = crate::app::attach_remote(
            &mut app,
            crate::app::RemoteTarget {
                host_name: "mini".into(),
                dest: "eben@mini".into(),
                tmux: hostile.into(),
            },
            80,
            24,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("invalid remote tmux session name"),
            "hostile name {hostile:?} must be rejected, got: {err}"
        );
        assert!(app.remote.is_none(), "no session may spawn for {hostile:?}");
        assert!(
            matches!(app.view, crate::ui::View::Dashboard),
            "view must be unchanged for {hostile:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn key_in_attached_remote_after_ssh_exit_bounces_to_dashboard_with_error() {
    // Fake ssh that exits immediately (e.g. stale tmux session name):
    // pressing any key in AttachedRemote must return to the dashboard
    // and raise an error modal naming the host/session.
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    let fake = dir.path().join("fake-ssh-exit.sh");
    std::fs::write(&fake, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        crate::app::attach_remote(
            &mut g,
            crate::app::RemoteTarget {
                host_name: "mini".into(),
                dest: "eben@mini".into(),
                tmux: "wsx-r-w".into(),
            },
            80,
            24,
        )
        .unwrap();
        assert!(matches!(g.view, crate::ui::View::AttachedRemote));
    }
    // Wait for the child to actually exit so the status flips to Exited.
    wait_until(&app, "ssh child to exit", |g| {
        g.remote.as_ref().is_some_and(|s| {
            matches!(
                *s.status.read().unwrap(),
                crate::pty::session::SessionStatus::Exited { .. }
            )
        })
    })
    .await;
    let mut g = app.lock().await;
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('a'),
        crossterm::event::KeyModifiers::empty(),
    );
    handle_event(&mut g, &app, CtEvent::Key(key)).await.unwrap();
    assert!(matches!(g.view, crate::ui::View::Dashboard));
    assert!(g.remote.is_none() && g.remote_target.is_none());
    match &g.modal {
        Some(Modal::Error { message }) => {
            assert!(
                message.contains("mini/wsx-r-w"),
                "error should name host/session: {message}"
            );
        }
        other => panic!("expected error modal, got {other:?}"),
    }
}

/// The remote/shared bottom bar must show the host's GLOBAL pinned commands
/// and the workspace's PR chip (recovered from the retained `remote_list`
/// record whose agent owns the attached tmux), not just the `^x menu` hint.
/// Repo-scoped stats the host doesn't ship (procs/diff/model) stay off.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_bottom_bar_shows_global_pinned_and_pr_chip() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    // Fake ssh that streams a heartbeat so the remote session stays live.
    let fake = dir.path().join("fake-ssh.sh");
    std::fs::write(
        &fake,
        "#!/bin/sh\nfor i in $(seq 1 60); do echo beat; sleep 1; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    // Global pinned commands are resolved locally and drive the remote agent.
    store
        .set_setting("pinned_commands", "Commit=/commit\nTest=/test")
        .unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();

    // A retained remote list whose live agent owns the tmux we attach to,
    // carrying a PR (open, #142) so the chip is recoverable by tmux match.
    app.remote_list = Some({
        use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
        crate::app::RemoteList {
            host_name: "mini".into(),
            dest: "eben@mini".into(),
            can_share: false,
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                shared: true,
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: Some("wsx-r-w".into()),
                    alive: true,
                }],
                lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
                pr_number: Some(142),
                tags: vec![],
            }],
        }
    });

    crate::app::attach_remote(
        &mut app,
        crate::app::RemoteTarget {
            host_name: "mini".into(),
            dest: "eben@mini".into(),
            tmux: "wsx-r-w".into(),
        },
        80,
        24,
    )
    .unwrap();
    assert!(matches!(app.view, crate::ui::View::AttachedRemote));

    let backend = TestBackend::new(120, 24);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| crate::app::render::draw_for_test(f, &mut app))
        .unwrap();
    let text: String = term
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();

    assert!(text.contains("menu"), "the ^x menu hint must still render");
    assert!(
        text.contains("Commit") && text.contains("Test"),
        "global pinned chips must render in the remote bar: {text:?}"
    );
    assert!(
        text.contains("142"),
        "the PR chip (#142) must render, recovered via tmux->record match: {text:?}"
    );
    // The dispatch cache is populated so `^x <digit>` / clicks can fire.
    assert_eq!(app.pinned_commands_cache.len(), 2);
    // No local WorkspaceId backs a remote PR, so its click target stays off.
    assert!(app.pr_link_rect.is_none());

    crate::app::detach_remote(&mut app);
}

/// `^x <digit>` in the remote view fires the matching global pinned command
/// into the ssh PTY, driving the remote agent. The fake ssh `cat`s its stdin
/// back out, so the dispatched command text lands on the remote screen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_leader_digit_fires_pinned_command_into_ssh() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    let fake = dir.path().join("fake-ssh-cat.sh");
    std::fs::write(&fake, "#!/bin/sh\ncat\n").unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        // The render pass populates the dispatch cache from settings; seed it
        // directly here since this test drives keys without a render tick.
        g.pinned_commands_cache = crate::commands::pinned::parse("Ship=/ship-it\nTest=/test");
        crate::app::attach_remote(
            &mut g,
            crate::app::RemoteTarget {
                host_name: "mini".into(),
                dest: "eben@mini".into(),
                tmux: "wsx-r-w".into(),
            },
            80,
            24,
        )
        .unwrap();
    }

    // ^x then '1' -> first pinned command (/ship-it) echoes back via `cat`.
    for k in [
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::CONTROL,
        ),
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('1'),
            crossterm::event::KeyModifiers::empty(),
        ),
    ] {
        let mut g = app.lock().await;
        handle_event(&mut g, &app, CtEvent::Key(k)).await.unwrap();
    }

    wait_until(&app, "pinned command to echo back through ssh", |g| {
        g.remote.as_ref().is_some_and(|s| {
            s.parser
                .lock()
                .unwrap()
                .screen()
                .contents()
                .contains("/ship-it")
        })
    })
    .await;

    let mut g = app.lock().await;
    crate::app::detach_remote(&mut g);
}

/// `^x` in the remote view arms the leader menu: Esc dismisses it without
/// detaching, and ↓ + Enter runs the highlighted row (`<` prompt tag).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_leader_menu_navigates_and_dispatches() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = EnvGuard::new();
    let fake = dir.path().join("fake-ssh-cat.sh");
    std::fs::write(&fake, "#!/bin/sh\ncat\n").unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        crate::app::attach_remote(
            &mut g,
            crate::app::RemoteTarget {
                host_name: "mini".into(),
                dest: "eben@mini".into(),
                tmux: "wsx-r-w".into(),
            },
            80,
            24,
        )
        .unwrap();
    }
    let ctrl_x = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        KeyModifiers::CONTROL,
    );
    let plain = |code| crossterm::event::KeyEvent::new(code, KeyModifiers::empty());
    let mut g = app.lock().await;

    handle_event(&mut g, &app, CtEvent::Key(ctrl_x))
        .await
        .unwrap();
    assert!(g.leader_pending, "Ctrl-x must arm the remote menu");
    handle_event(
        &mut g,
        &app,
        CtEvent::Key(plain(crossterm::event::KeyCode::Esc)),
    )
    .await
    .unwrap();
    assert!(!g.leader_pending);
    assert!(
        matches!(g.view, crate::ui::View::AttachedRemote),
        "Esc must not detach"
    );

    handle_event(&mut g, &app, CtEvent::Key(ctrl_x))
        .await
        .unwrap();
    handle_event(
        &mut g,
        &app,
        CtEvent::Key(plain(crossterm::event::KeyCode::Down)),
    )
    .await
    .unwrap();
    assert!(g.leader_pending, "arrows keep the menu open");
    handle_event(
        &mut g,
        &app,
        CtEvent::Key(plain(crossterm::event::KeyCode::Enter)),
    )
    .await
    .unwrap();
    assert!(!g.leader_pending);
    assert!(
        matches!(g.modal, Some(crate::ui::modal::Modal::PromptTag(_))),
        "Enter on the second row opens the prompt tag picker"
    );
    crate::app::detach_remote(&mut g);
}

#[tokio::test]
async fn esc_during_remote_list_loading_clears_pending_gen_and_stale_fetch_no_ops() {
    // Esc while RemoteListLoading is up must close the modal AND clear
    // pending_remote_gen so the in-flight fetch's reconcile becomes a
    // no-op via its gen guard, instead of reopening a modal (or an
    // error) the user has already backed out of.
    let mut env = EnvGuard::new();
    let dir = tempfile::tempdir().unwrap();
    // Deliberately slow so Esc can land before the fetch completes.
    let script = dir.path().join("fake-ssh-slow.sh");
    std::fs::write(&script, "#!/bin/sh\nsleep 2\necho '[]'\n").unwrap();
    std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.set("WSX_SSH_BIN", script.to_str().unwrap());

    let store = Store::open_in_memory().unwrap();
    store.set_setting("shared_hosts", "mini=eben@mini").unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(store, tmp.path().to_path_buf()).unwrap(),
    ));
    {
        let mut g = app.lock().await;
        let h = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('H'),
            crossterm::event::KeyModifiers::SHIFT,
        );
        handle_event(&mut g, &app, CtEvent::Key(h)).await.unwrap();
        let enter = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(enter))
            .await
            .unwrap();
        assert!(matches!(g.modal, Some(Modal::RemoteListLoading { .. })));
        let esc = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::empty(),
        );
        handle_event(&mut g, &app, CtEvent::Key(esc)).await.unwrap();
        assert!(
            g.modal.is_none(),
            "Esc should close the loading modal immediately"
        );
        assert!(
            g.pending_remote_gen.is_none(),
            "Esc should clear pending_remote_gen so the late reconcile no-ops"
        );
    }
    // Let the slow fetch finish and reconcile run; it must not resurrect
    // any modal or repopulate remote_list.
    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
    let g = app.lock().await;
    assert!(
        g.modal.is_none(),
        "stale reconcile must not reopen a modal; got {:?}",
        g.modal
    );
    assert!(
        g.remote_list.is_none(),
        "stale reconcile must not populate remote_list"
    );
}

#[tokio::test]
async fn remote_workspace_list_navigation_bounded_to_live_rows() {
    // The dead `codex#2` agent is filtered out, leaving a single live row,
    // so `j` cannot advance the selection past index 0.
    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
    app.remote_list = Some(mixed_liveness_remote_list());
    app.modal = Some(Modal::RemoteWorkspaceList {
        selected: 0,
        notice: None,
    });
    let shared_app = Arc::new(Mutex::new(
        App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
    ));

    let j = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('j'),
        crossterm::event::KeyModifiers::NONE,
    );
    handle_key_modal(&mut app, &shared_app, j).await.unwrap();
    match &app.modal {
        Some(Modal::RemoteWorkspaceList { selected, .. }) => assert_eq!(
            *selected, 0,
            "only one live row exists, so j must not move past it"
        ),
        other => panic!("expected RemoteWorkspaceList, got {other:?}"),
    }
}

#[tokio::test]
async fn remote_workspace_list_enter_with_no_live_rows_notices() {
    // When every shared workspace on the host has a dead session there are
    // no rows at all; Enter can't resolve a target, so the modal stays open
    // with the "no live session" notice rather than attaching.
    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
    app.remote_list = Some(all_dead_remote_list());
    app.modal = Some(Modal::RemoteWorkspaceList {
        selected: 0,
        notice: None,
    });
    let shared_app = Arc::new(Mutex::new(
        App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
    ));

    let enter = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::empty(),
    );
    handle_key_modal(&mut app, &shared_app, enter)
        .await
        .unwrap();
    match &app.modal {
        Some(Modal::RemoteWorkspaceList { notice, .. }) => {
            assert_eq!(
                notice.as_deref(),
                Some("no live session to attach to"),
                "expected the no-live-session notice"
            );
        }
        other => panic!("expected RemoteWorkspaceList to stay open, got {other:?}"),
    }
}

#[tokio::test]
async fn remote_workspace_list_esc_closes_modal_and_clears_remote_list() {
    let store = Store::open_in_memory().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = App::new(store, tmp.path().to_path_buf()).unwrap();
    app.remote_list = Some(mixed_liveness_remote_list());
    app.modal = Some(Modal::RemoteWorkspaceList {
        selected: 0,
        notice: None,
    });
    let shared_app = Arc::new(Mutex::new(
        App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
    ));

    let esc = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Esc,
        crossterm::event::KeyModifiers::empty(),
    );
    handle_key_modal(&mut app, &shared_app, esc).await.unwrap();
    assert!(app.modal.is_none(), "Esc should close the modal");
    assert!(
        app.remote_list.is_none(),
        "Esc should clear app.remote_list (ephemeral contract)"
    );
}

/// A host that can share, listing one direct workspace and nothing live.
fn shareable_remote_list() -> crate::app::RemoteList {
    use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
    crate::app::RemoteList {
        host_name: "mini".into(),
        dest: "eben@mini".into(),
        can_share: true,
        records: vec![SharedWorkspaceRecord {
            repo: "r".into(),
            workspace: "w".into(),
            branch: "b".into(),
            worktree_path: "/x".into(),
            shared: false,
            agents: vec![SharedAgentRecord {
                label: "claude".into(),
                agent: "claude".into(),
                tmux_session: None,
                alive: false,
            }],
            lifecycle: None,
            pr_number: None,
            tags: vec![],
        }],
    }
}

/// An app showing `shareable_remote_list` with its one (dormant) row selected,
/// and ssh pointed at a fake running `script`.
fn app_on_shareable_list(
    dir: &std::path::Path,
    script: &str,
) -> (Arc<Mutex<App>>, EnvGuard, tempfile::TempDir) {
    let fake = dir.join("fake-ssh.sh");
    std::fs::write(&fake, format!("#!/bin/sh\n{script}")).unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut env = EnvGuard::new();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());
    let tmp = tempfile::TempDir::new().unwrap();
    let mut app = App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap();
    app.remote_list = Some(shareable_remote_list());
    app.modal = Some(Modal::RemoteWorkspaceList {
        selected: 0,
        notice: None,
    });
    (Arc::new(Mutex::new(app)), env, tmp)
}

async fn key(app: &Arc<Mutex<App>>, code: crossterm::event::KeyCode) {
    let mut g = app.lock().await;
    let k = crossterm::event::KeyEvent::new(code, KeyModifiers::empty());
    handle_event(&mut g, app, CtEvent::Key(k)).await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn enter_on_dormant_row_confirms_then_shares_and_attaches() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    // The share call prints the fresh record; the attach that follows
    // streams like a live tmux client.
    let (app, _env, _tmp) = app_on_shareable_list(
        dir.path(),
        "case \"$*\" in\n\
         *'workspace share r w --restart --json'*) echo '{\"repo\":\"r\",\"workspace\":\"w\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"shared\":true,\"agents\":[{\"label\":\"claude\",\"agent\":\"claude\",\"tmux_session\":\"wsx-r-w\",\"alive\":true}]}';;\n\
         *) for i in $(seq 1 60); do echo remote-beat; sleep 1; done;;\n\
         esac\n",
    );

    key(&app, KeyCode::Enter).await;
    match &app.lock().await.modal {
        Some(Modal::RemoteShareConfirm {
            repo,
            workspace,
            shared,
            ..
        }) => assert!(repo == "r" && workspace == "w" && !shared),
        other => panic!("expected the share confirmation, got {other:?}"),
    }

    key(&app, KeyCode::Char('y')).await;
    assert!(matches!(
        app.lock().await.modal,
        Some(Modal::RemoteShareRunning { .. })
    ));

    wait_until(&app, "the share to finish and attach", |g| {
        matches!(g.view, crate::ui::View::AttachedRemote)
    })
    .await;
    let mut g = app.lock().await;
    assert_eq!(g.remote_target.as_ref().unwrap().tmux, "wsx-r-w");
    assert!(
        g.remote_list.as_ref().unwrap().records[0].shared,
        "the listed record is replaced by the fresh one"
    );
    crate::app::detach_remote(&mut g);
}

#[tokio::test]
async fn a_failed_remote_share_returns_to_the_list_with_the_reason() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let (app, _env, _tmp) = app_on_shareable_list(
        dir.path(),
        "echo 'error: no wsx dashboard on this host picked up the request' >&2\nexit 1\n",
    );

    key(&app, KeyCode::Enter).await;
    key(&app, KeyCode::Char('y')).await;
    wait_until(&app, "the failed share to reconcile", |g| {
        g.pending_remote_gen.is_none()
    })
    .await;
    match &app.lock().await.modal {
        Some(Modal::RemoteWorkspaceList {
            notice: Some(n), ..
        }) => assert!(n.contains("no wsx dashboard"), "{n}"),
        other => panic!("expected the list with the error, got {other:?}"),
    }
}

#[tokio::test]
async fn esc_while_sharing_returns_to_the_list_and_ignores_the_result() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let (app, _env, _tmp) = app_on_shareable_list(dir.path(), "sleep 1\necho '{}'\n");

    key(&app, KeyCode::Enter).await;
    key(&app, KeyCode::Char('y')).await;
    key(&app, KeyCode::Esc).await;
    {
        let g = app.lock().await;
        assert!(g.pending_remote_gen.is_none());
        assert!(matches!(g.modal, Some(Modal::RemoteWorkspaceList { .. })));
    }
    // `n` on the confirmation backs out without sharing.
    key(&app, KeyCode::Enter).await;
    key(&app, KeyCode::Char('n')).await;
    assert!(matches!(
        app.lock().await.modal,
        Some(Modal::RemoteWorkspaceList { notice: None, .. })
    ));
}

/// A fake ssh for the create flow: `repo list` answers two repos, the
/// create logs its remote command to `log` and prints the new record, and
/// anything else (the attach) streams like a live tmux client.
fn create_flow_script(log: &std::path::Path) -> String {
    format!(
        "case \"$*\" in\n\
         *'repo list --json'*) echo '[{{\"name\":\"a\"}},{{\"name\":\"r\"}}]';;\n\
         *'workspace create'*) printf '%s\\n' \"$*\" > {log}; echo '{{\"repo\":\"r\",\"workspace\":\"fresh\",\"branch\":\"b\",\"worktree_path\":\"/x\",\"shared\":true,\"agents\":[{{\"label\":\"codex\",\"agent\":\"codex\",\"tmux_session\":\"wsx-r-fresh\",\"alive\":true}}]}}';;\n\
         *) for i in $(seq 1 60); do echo remote-beat; sleep 1; done;;\n\
         esac\n",
        log = log.display()
    )
}

async fn type_text(app: &Arc<Mutex<App>>, text: &str) {
    for c in text.chars() {
        key(app, crossterm::event::KeyCode::Char(c)).await;
    }
}

async fn remote_form(app: &Arc<Mutex<App>>) -> crate::ui::modal::RemoteNewWorkspaceForm {
    match &app.lock().await.modal {
        Some(Modal::RemoteNewWorkspace(form)) => form.clone(),
        other => panic!("expected the remote new-workspace form, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capital_n_in_remote_list_creates_a_yolo_workspace_and_attaches() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("create.log");
    let (app, _env, _tmp) = app_on_shareable_list(dir.path(), &create_flow_script(&log));

    {
        let mut g = app.lock().await;
        let k = crossterm::event::KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT);
        handle_event(&mut g, &app, CtEvent::Key(k)).await.unwrap();
        assert!(matches!(
            g.modal,
            Some(Modal::RemoteReposLoading { yolo: true, .. })
        ));
    }
    wait_until(&app, "the host's repos to load", |g| {
        matches!(g.modal, Some(Modal::RemoteNewWorkspace(_)))
    })
    .await;
    let form = remote_form(&app).await;
    assert_eq!(form.repos, ["a", "r"]);
    assert_eq!(form.repo, 1, "the selected row's repo is preselected");
    assert!(form.yolo, "N opens the form in yolo mode");
    assert_eq!(form.agent, None, "the host's default agent until Tab");

    type_text(&app, "fresh").await;
    // Tab past the host default to an explicit agent.
    let codex = crate::pty::session::AgentKind::Codex;
    while remote_form(&app).await.agent != Some(codex) {
        key(&app, KeyCode::Tab).await;
    }
    key(&app, KeyCode::Enter).await;
    assert!(matches!(
        app.lock().await.modal,
        Some(Modal::RemoteCreateRunning(_))
    ));

    wait_until(&app, "the create to finish and attach", |g| {
        matches!(g.view, crate::ui::View::AttachedRemote)
    })
    .await;
    let mut g = app.lock().await;
    assert_eq!(g.remote_target.as_ref().unwrap().tmux, "wsx-r-fresh");
    assert!(
        g.remote_list
            .as_ref()
            .unwrap()
            .records
            .iter()
            .any(|r| r.workspace == "fresh"),
        "the new workspace joins the listing"
    );
    crate::app::detach_remote(&mut g);
    let sent = std::fs::read_to_string(&log).unwrap();
    assert!(
        sent.contains("wsx workspace create r --name fresh --agent codex --yolo --start --json"),
        "{sent}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ctrl_y_toggles_yolo_and_a_failed_create_reopens_the_form() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let (app, _env, _tmp) = app_on_shareable_list(
        dir.path(),
        "case \"$*\" in\n\
         *'repo list --json'*) echo '[{\"name\":\"r\"}]';;\n\
         *) echo \"error: branch 'wsx/dup' already exists\" >&2; exit 1;;\n\
         esac\n",
    );

    key(&app, KeyCode::Char('n')).await;
    wait_until(&app, "the host's repos to load", |g| {
        matches!(g.modal, Some(Modal::RemoteNewWorkspace(_)))
    })
    .await;
    assert!(!remote_form(&app).await.yolo);
    {
        let mut g = app.lock().await;
        let k = crossterm::event::KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL);
        handle_event(&mut g, &app, CtEvent::Key(k)).await.unwrap();
    }
    let form = remote_form(&app).await;
    assert!(form.yolo, "^y turns yolo on");
    assert_eq!(form.name_buffer, "", "^y must not type a 'y'");

    // The listing already has r/w: refused before any round trip.
    type_text(&app, "w").await;
    key(&app, KeyCode::Enter).await;
    let form = remote_form(&app).await;
    assert!(
        form.notice.as_deref().unwrap().contains("already has"),
        "{form:?}"
    );

    key(&app, KeyCode::Backspace).await;
    type_text(&app, "dup").await;
    key(&app, KeyCode::Enter).await;
    wait_until(&app, "the failed create to reconcile", |g| {
        g.pending_remote_gen.is_none()
    })
    .await;
    let form = remote_form(&app).await;
    assert_eq!(form.name_buffer, "dup", "the form keeps what was typed");
    assert!(form.yolo);
    assert!(
        form.notice.as_deref().unwrap().contains("already exists"),
        "{form:?}"
    );

    key(&app, KeyCode::Esc).await;
    assert!(matches!(
        app.lock().await.modal,
        Some(Modal::RemoteWorkspaceList { .. })
    ));
}

#[tokio::test]
async fn n_on_an_old_host_explains_instead_of_creating() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let (app, _env, _tmp) = app_on_shareable_list(dir.path(), "exit 1\n");
    app.lock().await.remote_list.as_mut().unwrap().can_share = false;
    key(&app, KeyCode::Char('n')).await;
    match &app.lock().await.modal {
        Some(Modal::RemoteWorkspaceList {
            notice: Some(n), ..
        }) => assert!(n.contains("too old"), "{n}"),
        other => panic!("expected the list with a notice, got {other:?}"),
    }
}

/// Opens the remote new-workspace form on `app` (host answering `r`).
async fn open_remote_form(app: &Arc<Mutex<App>>) {
    key(app, crossterm::event::KeyCode::Char('n')).await;
    wait_until(app, "the host's repos to load", |g| {
        matches!(g.modal, Some(Modal::RemoteNewWorkspace(_)))
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_create_whose_agents_did_not_start_joins_the_list_ready_to_start() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    // The host made the workspace but no dashboard started it: the record
    // comes back on stdout with the reason, and the command exits non-zero.
    let (app, _env, _tmp) = app_on_shareable_list(
        dir.path(),
        "case \"$*\" in\n\
         *'repo list --json'*) echo '[{\"name\":\"r\"}]';;\n\
         *) echo '{\"repo\":\"r\",\"workspace\":\"fresh\",\"branch\":\"b\",\"worktree_path\":\"/y\",\"shared\":true,\"agents\":[{\"label\":\"claude\",\"agent\":\"claude\",\"tmux_session\":\"wsx-r-fresh\",\"alive\":false}],\"start_error\":\"no wsx dashboard on this host picked up the request\"}'; exit 1;;\n\
         esac\n",
    );
    open_remote_form(&app).await;
    type_text(&app, "fresh").await;
    key(&app, KeyCode::Enter).await;
    wait_until(&app, "the partial create to reconcile", |g| {
        g.pending_remote_gen.is_none()
    })
    .await;

    let selected = {
        let g = app.lock().await;
        assert!(!matches!(g.view, crate::ui::View::AttachedRemote));
        let Some(Modal::RemoteWorkspaceList {
            selected,
            notice: Some(n),
        }) = &g.modal
        else {
            panic!("expected the list with a notice, got {:?}", g.modal);
        };
        assert!(n.contains("created r/fresh"), "{n}");
        assert!(n.contains("no wsx dashboard"), "{n}");
        let rows = crate::app::remote_rows(g.remote_list.as_ref().unwrap());
        assert_eq!(
            rows[*selected].workspace, "fresh",
            "the new row is selected"
        );
        *selected
    };
    // Enter on it offers to start it, rather than creating it again.
    key(&app, KeyCode::Enter).await;
    match &app.lock().await.modal {
        Some(Modal::RemoteShareConfirm {
            selected: s,
            workspace,
            shared,
            ..
        }) => assert!(*s == selected && workspace == "fresh" && *shared),
        other => panic!("expected the start confirmation, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_create_whose_setup_failed_says_so_instead_of_attaching() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _env, _tmp) = app_on_shareable_list(
        dir.path(),
        "case \"$*\" in\n\
         *'repo list --json'*) echo '[{\"name\":\"r\"}]';;\n\
         *) echo '{\"repo\":\"r\",\"workspace\":\"fresh\",\"branch\":\"b\",\"worktree_path\":\"/y\",\"shared\":true,\"agents\":[{\"label\":\"claude\",\"agent\":\"claude\",\"tmux_session\":\"wsx-r-fresh\",\"alive\":true}],\"setup_exit_code\":3,\"setup_log\":\"/logs/fresh.log\"}';;\n\
         esac\n",
    );
    open_remote_form(&app).await;
    type_text(&app, "fresh").await;
    key(&app, crossterm::event::KeyCode::Enter).await;
    wait_until(&app, "the create to reconcile", |g| {
        g.pending_remote_gen.is_none()
    })
    .await;
    let g = app.lock().await;
    assert!(!matches!(g.view, crate::ui::View::AttachedRemote));
    let Some(Modal::RemoteWorkspaceList {
        selected,
        notice: Some(n),
    }) = &g.modal
    else {
        panic!("expected the list with a notice, got {:?}", g.modal);
    };
    assert!(n.contains("exited with code 3"), "{n}");
    assert!(n.contains("/logs/fresh.log"), "{n}");
    let rows = crate::app::remote_rows(g.remote_list.as_ref().unwrap());
    assert_eq!(
        rows[*selected].attach_session(),
        Some("wsx-r-fresh"),
        "the live new row is selected, one Enter from attaching"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_blank_name_is_generated_before_the_create_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("create.log");
    let (app, _env, _tmp) = app_on_shareable_list(dir.path(), &create_flow_script(&log));
    open_remote_form(&app).await;
    key(&app, crossterm::event::KeyCode::Enter).await;
    let name = match &app.lock().await.modal {
        Some(Modal::RemoteCreateRunning(form)) => form.name_buffer.clone(),
        other => panic!("expected the create to run, got {other:?}"),
    };
    assert!(!name.is_empty());
    wait_until(&app, "the create to attach", |g| {
        matches!(g.view, crate::ui::View::AttachedRemote)
    })
    .await;
    crate::app::detach_remote(&mut *app.lock().await);
    let sent = std::fs::read_to_string(&log).unwrap();
    assert!(sent.contains(&format!("--name {name} ")), "{sent}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn esc_while_loading_repos_or_creating_ignores_the_late_result() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("create.log");
    // Every answer comes late, so Esc always lands first.
    let script = format!("sleep 1\n{}", create_flow_script(&log));
    let (app, _env, _tmp) = app_on_shareable_list(dir.path(), &script);

    key(&app, KeyCode::Char('n')).await;
    key(&app, KeyCode::Esc).await;
    assert!(app.lock().await.pending_remote_gen.is_none());
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(
        matches!(
            app.lock().await.modal,
            Some(Modal::RemoteWorkspaceList { .. })
        ),
        "the late repo list must not reopen the form"
    );

    open_remote_form(&app).await;
    type_text(&app, "fresh").await;
    key(&app, KeyCode::Enter).await;
    key(&app, KeyCode::Esc).await;
    match &app.lock().await.modal {
        Some(Modal::RemoteWorkspaceList {
            notice: Some(n), ..
        }) => assert!(n.contains("carries on"), "{n}"),
        other => panic!("expected the list, got {other:?}"),
    }
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    let g = app.lock().await;
    assert!(
        !matches!(g.view, crate::ui::View::AttachedRemote),
        "the late create must not attach"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_with_no_repos_explains_on_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _env, _tmp) = app_on_shareable_list(dir.path(), "echo '[]'\n");
    key(&app, crossterm::event::KeyCode::Char('n')).await;
    wait_until(&app, "the repo fetch to reconcile", |g| {
        g.pending_remote_gen.is_none()
    })
    .await;
    match &app.lock().await.modal {
        Some(Modal::RemoteWorkspaceList {
            notice: Some(n), ..
        }) => assert!(n.contains("no repos"), "{n}"),
        other => panic!("expected the list with a notice, got {other:?}"),
    }
}

/// `workspace create --start` asks the dashboard to start a workspace it
/// has never loaded; the share handler must reload rather than answer
/// "workspace not found".
#[tokio::test]
async fn force_share_finds_a_workspace_created_after_the_last_refresh() {
    use crate::data::store::{NewWorkspace, WorkspaceState};
    let mut env = EnvGuard::new();
    // Stop at the tmux check, past the lookup under test.
    env.set("WSX_TMUX_BIN", "/nonexistent/tmux");
    let (mut app, ids) = make_app_with_n_repos(1);
    let ws = app
        .store
        .insert_workspace(&NewWorkspace {
            repo_id: ids[0],
            name: "fresh",
            branch: "repo-0/fresh",
            worktree_path: std::path::Path::new("/tmp/wsx-fold-test/fresh"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: true,
        })
        .unwrap();
    app.store
        .set_workspace_state(ws, WorkspaceState::Ready)
        .unwrap();
    let err = crate::app::session::force_share_workspace(&mut app, ws)
        .unwrap_err()
        .to_string();
    assert!(err.contains("tmux"), "{err}");
}

/// A fake ssh that plays a host's `wsx workspace tag`: tags live one per line
/// in `<dir>/host-tags`, every call is logged to `<dir>/tag-args.log`, and
/// writes take `delay` seconds. Tag `bad` is refused; tag `late` is saved but
/// the call still fails, like a timeout after the host committed. Anything
/// else (the attach itself) `cat`s.
fn fake_tag_host(dir: &std::path::Path, delay: &str, tags: &[&str]) -> EnvGuard {
    let state = dir.join("host-tags");
    std::fs::write(
        &state,
        tags.iter().map(|t| format!("{t}\n")).collect::<String>(),
    )
    .unwrap();
    let log = dir.join("tag-args.log");
    let fake = dir.join("fake-ssh.sh");
    std::fs::write(
        &fake,
        format!(
            r#"#!/bin/sh
S={state}
case "$*" in
*'workspace tag list'*)
  echo "$*" >> {log}
  printf '['; sed 's/.*/"&"/' "$S" | paste -sd, - | tr -d '\n'; echo ']'; exit 0;;
*'workspace tag '*)
  echo "$*" >> {log}
  sleep {delay}
  op=$(echo "$*" | sed -E 's/.*workspace tag (add|rm) .*/\1/')
  t=$(echo "$*" | sed -E 's/.*workspace tag (add|rm) ([^ ]+) .*/\2/')
  [ "$t" = bad ] && {{ echo nope >&2; exit 1; }}
  grep -vx "$t" "$S" > "$S.new"; mv "$S.new" "$S"
  [ "$op" = add ] && {{ echo "$t" >> "$S"; sort -o "$S" "$S"; }}
  [ "$t" = late ] && {{ echo 'timed out' >&2; exit 1; }}
  cat "$S"; exit 0;;
esac
cat
"#,
            state = state.display(),
            log = log.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let mut env = EnvGuard::new();
    env.set("WSX_SSH_BIN", fake.to_str().unwrap());
    env
}

/// An app attached to `r/w` on `eben@mini`, listed with `tags`, with the
/// `#` editor open on it.
async fn remote_tag_editor(tags: &[&str]) -> (Arc<Mutex<App>>, tempfile::TempDir) {
    use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};
    let tmp = tempfile::TempDir::new().unwrap();
    let app = Arc::new(Mutex::new(
        App::new(Store::open_in_memory().unwrap(), tmp.path().to_path_buf()).unwrap(),
    ));
    let mut g = app.lock().await;
    g.remote_list = Some(crate::app::RemoteList {
        host_name: "mini".into(),
        dest: "eben@mini".into(),
        can_share: true,
        records: vec![SharedWorkspaceRecord {
            repo: "r".into(),
            workspace: "w".into(),
            branch: "b".into(),
            worktree_path: "/x".into(),
            shared: true,
            agents: vec![SharedAgentRecord {
                label: "claude".into(),
                agent: "claude".into(),
                tmux_session: Some("wsx-r-w".into()),
                alive: true,
            }],
            lifecycle: None,
            pr_number: None,
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }],
    });
    crate::app::attach_remote(
        &mut g,
        crate::app::RemoteTarget {
            host_name: "mini".into(),
            dest: "eben@mini".into(),
            tmux: "wsx-r-w".into(),
        },
        80,
        24,
    )
    .unwrap();
    for (code, mods) in [
        (KeyCode::Char('x'), KeyModifiers::CONTROL),
        (KeyCode::Char('#'), KeyModifiers::SHIFT),
    ] {
        let k = CtEvent::Key(crossterm::event::KeyEvent::new(code, mods));
        handle_event(&mut g, &app, k).await.unwrap();
    }
    drop(g);
    (app, tmp)
}

/// Type `text` into the open tag editor and press Enter.
async fn enter_tag(app: &Arc<Mutex<App>>, text: &str) {
    let mut g = app.lock().await;
    for code in text.chars().map(KeyCode::Char).chain([KeyCode::Enter]) {
        let k = CtEvent::Key(crossterm::event::KeyEvent::new(code, KeyModifiers::empty()));
        handle_event(&mut g, app, k).await.unwrap();
    }
}

fn listed_tags(g: &App) -> Vec<String> {
    g.remote_list.as_ref().unwrap().records[0].tags.clone()
}

fn editor_state(g: &App) -> Option<(Vec<String>, Option<String>)> {
    match &g.modal {
        Some(crate::ui::modal::Modal::WorkspaceTags { tags, notice, .. }) => {
            Some((tags.clone(), notice.clone()))
        }
        _ => None,
    }
}

/// `^x #` while attached to a remote workspace opens the tag editor on it,
/// and an added tag shows at once and reaches the host.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_leader_hash_tags_the_workspace_on_the_host() {
    use crate::ui::modal::{Modal, TagTarget};
    let dir = tempfile::tempdir().unwrap();
    let _env = fake_tag_host(dir.path(), "0", &["infra"]);
    let (app, _tmp) = remote_tag_editor(&["infra"]).await;
    {
        let g = app.lock().await;
        let Some(Modal::WorkspaceTags { target, tags, .. }) = &g.modal else {
            panic!("{:?}", g.modal);
        };
        assert_eq!(
            *target,
            TagTarget::Remote {
                dest: "eben@mini".into(),
                repo: "r".into(),
                workspace: "w".into(),
            }
        );
        assert_eq!(tags, &["infra"]);
    }

    enter_tag(&app, "wip").await;
    assert_eq!(
        listed_tags(&*app.lock().await),
        ["infra", "wip"],
        "shown at once"
    );
    let log = dir.path().join("tag-args.log");
    wait_until(&app, "tag add to reach the host", |_| {
        std::fs::read_to_string(&log)
            .is_ok_and(|s| s.contains("sh -lc 'wsx workspace tag add wip --workspace r/w'"))
    })
    .await;
    let mut g = app.lock().await;
    crate::app::detach_remote(&mut g);
}

/// Edits reach the host in the order they were made, even while earlier
/// ones are still in flight, and the editor ends on the host's tags.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_tag_edits_land_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let _env = fake_tag_host(dir.path(), "0.2", &["infra"]);
    let (app, _tmp) = remote_tag_editor(&["infra"]).await;
    // `wip` lists after `infra`; with an empty filter the editor rows are
    // [infra, wip], so ↓ + space toggles `wip` back off.
    enter_tag(&app, "wip").await;
    {
        let mut g = app.lock().await;
        for code in [KeyCode::Down, KeyCode::Char(' ')] {
            let k = CtEvent::Key(crossterm::event::KeyEvent::new(code, KeyModifiers::empty()));
            handle_event(&mut g, &app, k).await.unwrap();
        }
        assert_eq!(listed_tags(&g), ["infra"], "the removal shows at once");
    }
    let log = dir.path().join("tag-args.log");
    wait_until(&app, "both writes to land", |_| {
        std::fs::read_to_string(&log).is_ok_and(|s| s.lines().count() == 2)
    })
    .await;
    let log = std::fs::read_to_string(&log).unwrap();
    let ops: Vec<_> = log.lines().map(|l| l.contains("tag add wip")).collect();
    assert_eq!(ops, [true, false], "add before rm: {log}");
    let host = dir.path().join("host-tags");
    wait_until(&app, "editor to settle on the host's tags", |g| {
        std::fs::read_to_string(&host).is_ok_and(|s| s == "infra\n")
            && listed_tags(g) == ["infra"]
            && editor_state(g).is_some_and(|(t, _)| t == ["infra"])
    })
    .await;
    let mut g = app.lock().await;
    crate::app::detach_remote(&mut g);
}

/// A failed write isn't blindly undone: the editor asks the host. A refused
/// tag drops out; one the host saved before the call failed stays.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failed_remote_tag_write_shows_the_hosts_tags_and_why() {
    let dir = tempfile::tempdir().unwrap();
    let _env = fake_tag_host(dir.path(), "0", &["infra"]);
    let (app, _tmp) = remote_tag_editor(&["infra"]).await;

    enter_tag(&app, "bad").await;
    wait_until(&app, "refused tag to drop out", |g| {
        listed_tags(g) == ["infra"]
            && editor_state(g)
                .is_some_and(|(t, n)| t == ["infra"] && n.is_some_and(|n| n.contains("nope")))
    })
    .await;

    enter_tag(&app, "late").await;
    wait_until(&app, "saved-but-failed tag to stay", |g| {
        listed_tags(g) == ["infra", "late"]
            && editor_state(g).is_some_and(|(t, n)| {
                t == ["infra", "late"] && n.is_some_and(|n| n.contains("timed out"))
            })
    })
    .await;
    let log = std::fs::read_to_string(dir.path().join("tag-args.log")).unwrap();
    assert!(
        log.contains("wsx workspace tag list --workspace r/w --json"),
        "{log}"
    );
    let mut g = app.lock().await;
    crate::app::detach_remote(&mut g);
}

/// A write that finishes after you've moved to another host's listing leaves
/// that listing alone, even for a workspace with the same repo and name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_remote_tag_result_skips_another_hosts_listing() {
    let dir = tempfile::tempdir().unwrap();
    let _env = fake_tag_host(dir.path(), "0.3", &["infra"]);
    let (app, _tmp) = remote_tag_editor(&["infra"]).await;
    enter_tag(&app, "wip").await;
    {
        let mut g = app.lock().await;
        crate::app::detach_remote(&mut g);
        g.modal = None;
        let list = g.remote_list.as_mut().unwrap();
        list.dest = "eben@other".into();
        list.records[0].tags = vec!["mine".into()];
    }
    let log = dir.path().join("tag-args.log");
    wait_until(&app, "the write to finish", |_| {
        std::fs::read_to_string(&log).is_ok_and(|s| s.contains("tag add wip"))
    })
    .await;
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    let g = app.lock().await;
    assert_eq!(listed_tags(&g), ["mine"]);
}
