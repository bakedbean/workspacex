//! Workspaces shared from another wsx host over ssh: fetching the list,
//! flattening it to rows, and attaching to / detaching from one.

use super::*;

/// Result of a completed `fetch_shared_list` background fetch against a
/// remote wsx host, stashed on `App` so `Modal::RemoteWorkspaceList`
/// rendering has something to draw from.
#[derive(Debug, Clone)]
pub struct RemoteList {
    pub host_name: String,
    pub dest: String,
    pub records: Vec<crate::commands::shared::SharedWorkspaceRecord>,
    /// The host runs a wsx that lists every workspace and takes
    /// `workspace share --restart`, so workspaces without a live session are
    /// offered for sharing. False for an older host: only live sessions show.
    pub can_share: bool,
}

/// A resolved remote attach target: the display name of the host, its ssh
/// destination, and the remote tmux session name to attach to. Built from a
/// selected `RemoteRow` + its `RemoteList` when the user presses Enter on an
/// alive row, and consumed by `attach_remote`.
#[derive(Debug, Clone)]
pub struct RemoteTarget {
    pub host_name: String,
    pub dest: String,
    pub tmux: String,
}

/// One row of the remote list. A *live* row is one agent session to attach
/// to (`alive`, with a `tmux_session`); multiple agent instances on the same
/// workspace flatten into separate live rows. A *dormant* row stands for a
/// whole workspace with no live session — not shared, or shared with its
/// agents stopped — and Enter on it offers to share it and restart its agents
/// on the host. This is the shared helper both `Modal::RemoteWorkspaceList`'s
/// key handler and its renderer build rows from, so selection indices and
/// rendered rows always agree.
pub(crate) struct RemoteRow<'a> {
    pub workspace: &'a str,
    pub repo: &'a str,
    pub branch: &'a str,
    pub label: &'a str,
    pub tmux_session: Option<&'a str>,
    pub alive: bool,
    /// Whether the workspace is tmux-shared on the host. Tells a dormant row's
    /// "stopped" apart from "not shared".
    pub shared: bool,
    /// The workspace's PR lifecycle as computed on the remote host (see
    /// `SharedWorkspaceRecord::lifecycle`). Per-workspace, so every agent row
    /// flattened from the same record carries the same value. `None` = unknown
    /// (older host, or `gh` unavailable) → the renderer draws the branch dim,
    /// with no lifecycle color.
    pub lifecycle: Option<crate::git::forge::BranchLifecycle>,
    /// The workspace branch's PR number (see `SharedWorkspaceRecord::pr_number`),
    /// per-workspace like `lifecycle`. `None` when there is no PR or `gh`
    /// couldn't answer → the renderer omits the `#<num>` prefix.
    pub pr_number: Option<u32>,
    /// The workspace's tags (see `SharedWorkspaceRecord::tags`), drawn dim
    /// after the branch name when there's room.
    pub tags: &'a [String],
}

impl RemoteRow<'_> {
    /// The tmux session Enter attaches to; `None` on a dormant row.
    pub(crate) fn attach_session(&self) -> Option<&str> {
        self.tmux_session.filter(|_| self.alive)
    }
}

/// Flatten `list.records` into rows: first one live row per attachable agent
/// (`alive` AND carrying a `tmux_session` — the predicate `Enter` needs to
/// build a `RemoteTarget`), in record/agent order; then, when the host
/// `can_share`, one dormant row per workspace with no live agent at all.
/// An older host lists only shared workspaces and can't share more, so its
/// dead sessions are hidden rather than listed as rows that could only
/// answer Enter with "no live session to attach to".
pub(crate) fn remote_rows(list: &RemoteList) -> Vec<RemoteRow<'_>> {
    let mut live = Vec::new();
    let mut dormant = Vec::new();
    for rec in &list.records {
        let row = |label, tmux_session, alive| RemoteRow {
            workspace: &rec.workspace,
            repo: &rec.repo,
            branch: &rec.branch,
            label,
            tmux_session,
            alive,
            shared: rec.shared,
            lifecycle: rec.lifecycle,
            pr_number: rec.pr_number,
            tags: &rec.tags,
        };
        let before = live.len();
        for agent in &rec.agents {
            if let (Some(tmux_session), true) = (agent.tmux_session.as_deref(), agent.alive) {
                live.push(row(&agent.label, Some(tmux_session), true));
            }
        }
        if live.len() == before && list.can_share {
            let label = rec.agents.first().map_or("-", |a| a.label.as_str());
            dormant.push(row(label, None, false));
        }
    }
    live.extend(dormant);
    live
}

#[cfg(test)]
mod remote_rows_tests {
    use super::*;
    use crate::commands::shared::{SharedAgentRecord, SharedWorkspaceRecord};

    #[test]
    fn remote_rows_flatten_agents_and_drop_dead() {
        // A workspace with one live agent and one dead agent contributes only
        // the live row: dead sessions have nothing to `ssh … tmux attach` to,
        // so listing them just produces "no live session to attach to" on
        // Enter. Filtering here keeps the picker attach-only.
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "d".into(),
            can_share: false,
            records: vec![SharedWorkspaceRecord {
                repo: "r".into(),
                workspace: "w".into(),
                branch: "b".into(),
                worktree_path: "/x".into(),
                shared: true,
                agents: vec![
                    SharedAgentRecord {
                        label: "claude".into(),
                        agent: "claude".into(),
                        tmux_session: Some("wsx-r-w".into()),
                        alive: true,
                    },
                    SharedAgentRecord {
                        label: "codex#2".into(),
                        agent: "codex".into(),
                        tmux_session: None,
                        alive: false,
                    },
                ],
                lifecycle: None,
                pr_number: None,
                tags: vec![],
            }],
        };
        let rows = remote_rows(&list);
        assert_eq!(rows.len(), 1, "the dead agent row must be filtered out");
        assert_eq!(rows[0].label, "claude");
        assert!(rows[0].alive && rows[0].tmux_session.is_some());
    }

    #[test]
    fn remote_rows_drop_alive_flag_without_session_name() {
        // Defensive: a row claiming `alive` but carrying no tmux session name
        // can't be attached, so it must not appear either.
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "d".into(),
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
                    tmux_session: None,
                    alive: true,
                }],
                lifecycle: None,
                pr_number: None,
                tags: vec![],
            }],
        };
        assert!(remote_rows(&list).is_empty());
    }

    #[test]
    fn remote_rows_record_with_only_dead_agents_yields_no_rows() {
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "d".into(),
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
                    tmux_session: None,
                    alive: false,
                }],
                lifecycle: None,
                pr_number: None,
                tags: vec![],
            }],
        };
        assert!(remote_rows(&list).is_empty());
    }

    #[test]
    fn remote_rows_empty_records_yields_no_rows() {
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "d".into(),
            can_share: false,
            records: vec![],
        };
        assert!(remote_rows(&list).is_empty());
    }

    fn rec(workspace: &str, shared: bool, agents: Vec<SharedAgentRecord>) -> SharedWorkspaceRecord {
        SharedWorkspaceRecord {
            repo: "r".into(),
            workspace: workspace.into(),
            branch: "b".into(),
            worktree_path: "/x".into(),
            shared,
            agents,
            lifecycle: None,
            pr_number: None,
            tags: vec![],
        }
    }

    fn agent(tmux_session: Option<&str>, alive: bool) -> SharedAgentRecord {
        SharedAgentRecord {
            label: "claude".into(),
            agent: "claude".into(),
            tmux_session: tmux_session.map(Into::into),
            alive,
        }
    }

    #[test]
    fn remote_rows_offer_dormant_workspaces_after_live_ones_when_host_can_share() {
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "d".into(),
            can_share: true,
            records: vec![
                rec("direct", false, vec![agent(None, false)]),
                rec("stopped", true, vec![agent(Some("wsx-r-stopped"), false)]),
                rec("live", true, vec![agent(Some("wsx-r-live"), true)]),
            ],
        };
        let rows = remote_rows(&list);
        let names: Vec<_> = rows.iter().map(|r| r.workspace).collect();
        assert_eq!(names, ["live", "direct", "stopped"]);
        assert_eq!(rows[0].attach_session(), Some("wsx-r-live"));
        assert!(rows[1].attach_session().is_none() && !rows[1].shared);
        // A dead session name is not something to attach to.
        assert!(rows[2].attach_session().is_none() && rows[2].shared);
    }

    #[test]
    fn remote_rows_one_live_agent_keeps_the_workspace_off_the_dormant_list() {
        let list = RemoteList {
            host_name: "mini".into(),
            dest: "d".into(),
            can_share: true,
            records: vec![rec(
                "w",
                true,
                vec![agent(Some("wsx-r-w"), true), agent(None, false)],
            )],
        };
        assert_eq!(remote_rows(&list).len(), 1);
    }
}

/// Spawn `ssh -t <dest> -- "sh -lc \"tmux -u attach -t '=<session>'\""`
/// through the PTY plumbing and enter `View::AttachedRemote`. The `Session`'s
/// `tmux_session` is `None` on purpose: `kill()`/`Drop` sever only the local
/// ssh client; the remote agent persists in the remote tmux server (the
/// Phase 1 persistence contract, one hop away). The `agent` param on
/// `spawn_command_session` is inert plumbing here — `AgentKind::Claude` is
/// passed only to satisfy the signature.
///
/// Remote-command shape, learned the hard way on real hosts:
/// - ONE pre-quoted ssh argument: ssh space-joins remote argv and hands the
///   string to a shell on the host.
/// - Routed through `sh -l`, matching the list fetch: sshd runs the user's
///   default shell in non-login mode (`zsh -c` on macOS), which reads only
///   ~/.zshenv —
///   homebrew's tmux typically isn't on that PATH (`zsh:1: command not
///   found: tmux`). `sh -l` reads ~/.profile, making "wsx and tmux on the
///   host's `sh -l` PATH" the single documented requirement for both legs.
/// - The tmux target stays single-quoted: zsh expands an unquoted `=word`
///   into "path of the command `word`" (`zsh:1: wsx-<name> not found`).
/// - `tmux -u` forces UTF-8: the ssh/`sh -l` context carries no locale
///   (LC_CTYPE=C on real hosts), and without `-u` tmux downgrades Unicode
///   line-drawing to ACS/ASCII — pane borders render as rows of literal
///   `q`s in the attached view.
///
/// Session names are sanitized to `[A-Za-z0-9_-]` (see
/// `pty::tmux::session_name`), so the nested quoting is safe — and that
/// invariant is enforced here at the boundary, because `target.tmux` arrives
/// from the REMOTE host's JSON, not from our own producer.
pub(crate) fn attach_remote(
    app: &mut App,
    target: RemoteTarget,
    cols: u16,
    rows: u16,
) -> Result<()> {
    // The name is interpolated into a shell-parsed string below; reject
    // anything outside the sanitized charset rather than trusting the wire.
    if target.tmux.is_empty()
        || !target
            .tmux
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(crate::error::Error::UserInput(format!(
            "invalid remote tmux session name: {:?}",
            target.tmux
        )));
    }
    let mut cmd = portable_pty::CommandBuilder::new(crate::commands::shared_hosts::ssh_bin());
    cmd.args([
        "-t",
        &target.dest,
        "--",
        &format!("sh -lc \"tmux -u attach -t '={}'\"", target.tmux),
    ]);
    let session = crate::pty::session::spawn_command_session(
        cmd,
        cols,
        rows,
        crate::pty::session::AgentKind::Claude,
        // Report `ssh` (not the agent) if the local ssh binary is missing.
        crate::commands::shared_hosts::ssh_bin(),
        None,
    )?;
    app.remote = Some(std::sync::Arc::new(session));
    app.remote_target = Some(target);
    app.modal = None;
    app.view = crate::ui::View::AttachedRemote;
    Ok(())
}

/// Detach from the remote workspace: kill the local ssh client, clear
/// `remote`/`remote_target`, and return to the dashboard. Only the ssh client
/// dies — the remote agent survives in its tmux server. (On quit, `App` drop
/// runs `Session::Drop`, which likewise kills only the client; nothing extra is
/// needed to honor the persistence contract.) The remote *list* is left intact
/// so a detach lands back in the same modal-less dashboard the attach came
/// from without re-fetching. Detach does not touch the fetched list; Esc on the
/// list modal is what discards the fetched data (see the Esc arm in
/// `app::input`).
pub(crate) fn detach_remote(app: &mut App) {
    if let Some(session) = app.remote.take() {
        session.kill();
    }
    app.remote_target = None;
    app.view = crate::ui::View::Dashboard;
}

/// Reconcile the outcome of a spawned `fetch_shared_list` task.
/// Locks the app briefly; if `pending_remote_gen` no longer matches
/// `my_gen` — the user backed out or kicked off a newer fetch — the result
/// is discarded entirely (including a successful one), so a slow stale
/// fetch can never clobber a newer listing or reopen a modal the user has
/// moved past. Otherwise clears `pending_remote_gen` and, on success,
/// stores the listing and opens `Modal::RemoteWorkspaceList`; on failure
/// surfaces `Modal::Error` with the fetch's error text.
pub(crate) async fn reconcile_remote_list(
    app: SharedApp,
    my_gen: u64,
    host_name: String,
    dest: String,
    result: Result<crate::commands::shared_hosts::SharedListing>,
) {
    let mut g = app.lock().await;
    if g.pending_remote_gen != Some(my_gen) {
        // Stale — leave pending_remote_gen, remote_list, and modal alone.
        return;
    }
    g.pending_remote_gen = None;
    match result {
        Ok(listing) => {
            g.remote_list = Some(RemoteList {
                host_name,
                dest,
                records: listing.records,
                can_share: listing.can_share,
            });
            g.modal = Some(crate::ui::modal::Modal::RemoteWorkspaceList {
                selected: 0,
                notice: None,
            });
        }
        Err(e) => {
            g.modal = Some(crate::ui::modal::Modal::Error {
                message: e.to_string(),
            });
        }
    }
}

/// Reconcile a spawned `share_remote` task started from the dormant row at
/// `selected`. Same generation guard as `reconcile_remote_list`. On success
/// the fresh record replaces the listed one and wsx attaches straight to its
/// first live agent; on failure the list comes back with the error inline.
pub(crate) async fn reconcile_remote_share(
    app: SharedApp,
    my_gen: u64,
    selected: usize,
    result: Result<crate::commands::shared::SharedWorkspaceRecord>,
) {
    let mut g = app.lock().await;
    if g.pending_remote_gen != Some(my_gen) {
        return;
    }
    g.pending_remote_gen = None;
    if g.remote_list.is_none() {
        g.modal = None;
        return;
    }
    let rec = match result {
        Ok(rec) => rec,
        Err(e) => {
            g.modal = Some(crate::ui::modal::Modal::RemoteWorkspaceList {
                selected,
                notice: Some(e.to_string()),
            });
            return;
        }
    };
    if let Some(notice) = adopt_and_attach(&mut g, rec, "shared") {
        g.modal = Some(crate::ui::modal::Modal::RemoteWorkspaceList {
            selected: 0,
            notice: Some(notice),
        });
    }
}

/// Put `rec` — fresh from a share or create on the host — into the listing,
/// replacing the stale record for the same workspace, and attach to its
/// first live agent. Returns `None` once attached (or when no listing is
/// open to show anything on), else the notice to show on the list; `did`
/// names what the host just did, for that notice.
fn adopt_and_attach(
    g: &mut App,
    rec: crate::commands::shared::SharedWorkspaceRecord,
    did: &str,
) -> Option<String> {
    let workspace = format!("{}/{}", rec.repo, rec.workspace);
    let target = adopt(g.remote_list.as_mut()?, rec);
    Some(match target {
        Some(target) => match attach_remote(g, target, 80, 24) {
            Ok(()) => return None,
            Err(e) => format!("{did} {workspace}, but attach failed: {e}"),
        },
        None => format!("{did} {workspace}, but the host reported no live session"),
    })
}

/// Put `rec` into `list`, replacing the stale record for the same
/// workspace, and return its first live agent to attach to, if any.
fn adopt(
    list: &mut RemoteList,
    rec: crate::commands::shared::SharedWorkspaceRecord,
) -> Option<RemoteTarget> {
    let session = rec
        .agents
        .iter()
        .find(|a| a.alive)
        .and_then(|a| a.tmux_session.clone());
    let target = session.map(|tmux| RemoteTarget {
        host_name: list.host_name.clone(),
        dest: list.dest.clone(),
        tmux,
    });
    match list
        .records
        .iter_mut()
        .find(|r| r.repo == rec.repo && r.workspace == rec.workspace)
    {
        Some(slot) => *slot = rec,
        None => list.records.push(rec),
    }
    target
}

/// What the remote new-workspace form opens with once the host's repos
/// arrive: the list row to return to, and the choices `n`/`N` made.
#[derive(Debug, Clone)]
pub struct RemoteFormSeed {
    pub selected: usize,
    pub host_name: String,
    pub yolo: bool,
    /// Preselected when the host has it: the repo of the row `n` was on.
    pub repo: Option<String>,
}

/// Reconcile a spawned `fetch_remote_repos` task. Same generation guard as
/// `reconcile_remote_list`. Opens the new-workspace form on success; a
/// failure, or a host with no repos, returns to the list with the reason.
pub(crate) async fn reconcile_remote_repos(
    app: SharedApp,
    my_gen: u64,
    seed: RemoteFormSeed,
    result: Result<Vec<String>>,
) {
    let mut g = app.lock().await;
    if g.pending_remote_gen != Some(my_gen) {
        return;
    }
    g.pending_remote_gen = None;
    let notice = match result {
        Ok(repos) if !repos.is_empty() => {
            let repo = seed
                .repo
                .and_then(|want| repos.iter().position(|r| *r == want))
                .unwrap_or(0);
            g.modal = Some(crate::ui::modal::Modal::RemoteNewWorkspace(
                crate::ui::modal::RemoteNewWorkspaceForm {
                    selected: seed.selected,
                    host_name: seed.host_name,
                    repos,
                    repo,
                    name_buffer: String::new(),
                    agent: None,
                    yolo: seed.yolo,
                    notice: None,
                },
            ));
            return;
        }
        Ok(_) => format!("no repos are registered on {}", seed.host_name),
        Err(e) => e.to_string(),
    };
    g.modal = Some(crate::ui::modal::Modal::RemoteWorkspaceList {
        selected: seed.selected,
        notice: Some(notice),
    });
}

/// Reconcile a spawned `create_remote` task. Same generation guard as
/// `reconcile_remote_list`. A clean create attaches straight to the new
/// workspace's first live agent. One that exists but whose setup failed or
/// whose agents didn't start joins the listing unattached, selected, with
/// the reason — Enter on its row then attaches or starts it. A failure with
/// no workspace to show brings the form back with the host's reason, so a
/// taken or invalid name can be fixed and resent.
pub(crate) async fn reconcile_remote_create(
    app: SharedApp,
    my_gen: u64,
    mut form: crate::ui::modal::RemoteNewWorkspaceForm,
    result: Result<crate::commands::shared::CreatedWorkspaceRecord>,
) {
    let mut g = app.lock().await;
    if g.pending_remote_gen != Some(my_gen) {
        return;
    }
    g.pending_remote_gen = None;
    match result {
        Ok(created) if created.setup_exit_code.is_none() && created.start_error.is_none() => {
            if let Some(notice) = adopt_and_attach(&mut g, created.record, "created") {
                g.modal = Some(crate::ui::modal::Modal::RemoteWorkspaceList {
                    selected: 0,
                    notice: Some(notice),
                });
            }
        }
        Ok(created) => {
            let Some(list) = g.remote_list.as_mut() else {
                g.modal = None;
                return;
            };
            let (repo, workspace) = (
                created.record.repo.clone(),
                created.record.workspace.clone(),
            );
            let mut notice = format!("created {repo}/{workspace}, but");
            if let Some(code) = created.setup_exit_code {
                notice.push_str(&format!(" its setup script exited with code {code}"));
                if let Some(log) = &created.setup_log {
                    notice.push_str(&format!(" (log on the host: {log})"));
                }
                if created.start_error.is_some() {
                    notice.push_str(" and");
                }
            }
            match &created.start_error {
                Some(e) => notice.push_str(&format!(
                    " its agents didn't start: {e}. [enter] starts them"
                )),
                None => notice.push_str(". [enter] attaches"),
            }
            adopt(list, created.record);
            let selected = remote_rows(list)
                .iter()
                .position(|r| r.repo == repo && r.workspace == workspace)
                .unwrap_or(0);
            g.modal = Some(crate::ui::modal::Modal::RemoteWorkspaceList {
                selected,
                notice: Some(notice),
            });
        }
        Err(e) => {
            form.notice = Some(e.to_string());
            g.modal = Some(crate::ui::modal::Modal::RemoteNewWorkspace(form));
        }
    }
}

#[cfg(test)]
mod reconcile_remote_tests {
    use super::*;
    use crate::commands::shared::SharedWorkspaceRecord;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::Mutex;

    fn make_app() -> (App, TempDir) {
        let store = crate::data::store::Store::open_in_memory().unwrap();
        let tmp = TempDir::new().unwrap();
        let app = App::new(store, tmp.path().to_path_buf()).unwrap();
        (app, tmp)
    }

    #[tokio::test]
    async fn reconcile_remote_list_stores_records_and_discards_stale_gens() {
        let (app, _tmp) = make_app();
        let shared = Arc::new(Mutex::new(app));
        let (g1, g2) = {
            let mut app = shared.lock().await;
            (app.alloc_remote_gen(), app.alloc_remote_gen()) // g2 supersedes g1
        };
        let rec = SharedWorkspaceRecord {
            repo: "r".into(),
            workspace: "w".into(),
            branch: "b".into(),
            worktree_path: "/x".into(),
            shared: true,
            agents: vec![],
            lifecycle: None,
            pr_number: None,
            tags: vec![],
        };
        // Stale gen: ignored entirely.
        reconcile_remote_list(
            shared.clone(),
            g1,
            "mini".into(),
            "host".into(),
            Ok(crate::commands::shared_hosts::SharedListing {
                records: vec![rec.clone()],
                can_share: true,
            }),
        )
        .await;
        assert!(shared.lock().await.remote_list.is_none());
        // Current gen: stored + list modal opened.
        reconcile_remote_list(
            shared.clone(),
            g2,
            "mini".into(),
            "host".into(),
            Ok(crate::commands::shared_hosts::SharedListing {
                records: vec![rec],
                can_share: true,
            }),
        )
        .await;
        {
            let app = shared.lock().await;
            assert_eq!(app.remote_list.as_ref().unwrap().records.len(), 1);
            assert!(matches!(
                app.modal,
                Some(crate::ui::modal::Modal::RemoteWorkspaceList { .. })
            ));
            assert!(app.pending_remote_gen.is_none());
        }
        // Error path: error modal with the message.
        let g3 = shared.lock().await.alloc_remote_gen();
        reconcile_remote_list(
            shared.clone(),
            g3,
            "mini".into(),
            "host".into(),
            Err(crate::error::Error::UserInput("ssh mini: refused".into())),
        )
        .await;
        match &shared.lock().await.modal {
            Some(crate::ui::modal::Modal::Error { message }) => {
                assert!(message.contains("refused"))
            }
            other => panic!("expected error modal, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reconcile_remote_list_skips_all_mutation_when_gen_mismatch() {
        let (mut app, _tmp) = make_app();
        // Simulate: a different modal is already showing (e.g. an Error
        // popped by another flow) and pending_remote_gen advanced past
        // the value our stale task carries.
        app.modal = Some(crate::ui::modal::Modal::Error {
            message: "untouched".into(),
        });
        app.pending_remote_gen = Some(99);
        let shared = Arc::new(Mutex::new(app));
        let rec = SharedWorkspaceRecord {
            repo: "r".into(),
            workspace: "w".into(),
            branch: "b".into(),
            worktree_path: "/x".into(),
            shared: true,
            agents: vec![],
            lifecycle: None,
            pr_number: None,
            tags: vec![],
        };
        reconcile_remote_list(
            shared.clone(),
            7, // stale — does not match pending_remote_gen
            "mini".into(),
            "host".into(),
            Ok(crate::commands::shared_hosts::SharedListing {
                records: vec![rec],
                can_share: true,
            }),
        )
        .await;
        let g = shared.lock().await;
        match &g.modal {
            Some(crate::ui::modal::Modal::Error { message }) => {
                assert_eq!(
                    message, "untouched",
                    "stale reconcile must not overwrite modal"
                );
            }
            other => panic!("expected the pre-existing Error modal to survive, got {other:?}"),
        }
        assert_eq!(
            g.pending_remote_gen,
            Some(99),
            "stale reconcile must not clear pending_remote_gen"
        );
        assert!(
            g.remote_list.is_none(),
            "stale reconcile must not store a remote_list"
        );
    }
}
