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
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                agents: vec![SharedAgentRecord {
                    label: "claude".into(),
                    agent: "claude".into(),
                    tmux_session: Some("wsx-r-w".into()),
                    alive: true,
                }],
                lifecycle: Some(crate::git::forge::BranchLifecycle::PrOpen),
                pr_number: Some(142),
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
