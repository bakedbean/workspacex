//! Tests for the `cli` module.
//!
//! Declared from `mod.rs` as a file module so the production code and its
//! ~130 tests are not interleaved in one buffer.

use super::*;
use crate::cli::parse::config::known_setting_key;
use crate::cli::resolve::{self, *};
use crate::error::{Error, Result};

fn parse(args: &[&str]) -> Result<CliAction> {
    let mut v = vec!["wsx".to_string()];
    v.extend(args.iter().map(|s| s.to_string()));
    parse_args(v)
}

#[test]
fn parses_agent_send_with_workspace_flag() {
    match parse(&[
        "agent",
        "send",
        "--workspace",
        "backend/add-widgets",
        "primary",
        "do",
        "the",
        "thing",
    ])
    .unwrap()
    {
        CliAction::AgentSend {
            target,
            prompt,
            workspace,
        } => {
            assert_eq!(target, "primary");
            assert_eq!(prompt, "do the thing");
            assert_eq!(workspace.as_deref(), Some("backend/add-widgets"));
        }
        other => panic!("expected AgentSend, got {other:?}"),
    }
}

#[test]
fn agent_send_flags_are_only_recognised_before_the_label() {
    // Everything from the label onward is body, so a message that itself
    // starts with `--` is preserved verbatim rather than parsed as a flag.
    match parse(&["agent", "send", "claude", "--workspace", "is", "a", "flag"]).unwrap() {
        CliAction::AgentSend {
            target,
            prompt,
            workspace,
        } => {
            assert_eq!(target, "claude");
            assert_eq!(prompt, "--workspace is a flag");
            assert_eq!(workspace, None);
        }
        other => panic!("expected AgentSend, got {other:?}"),
    }
}

#[test]
fn agent_send_rejects_incomplete_invocations() {
    assert!(parse(&["agent", "send", "--workspace"]).is_err()); // flag needs a value
    assert!(parse(&["agent", "send", "--workspace", "backend/x"]).is_err()); // no label
    assert!(parse(&["agent", "send", "--workspace", "backend/x", "primary"]).is_err()); // no body
}

fn seed_spec_store() -> crate::data::store::Store {
    use crate::data::store::{NewWorkspace, Store};
    let store = Store::open_in_memory().unwrap();
    // A repo name containing a space exercises the split-on-LAST-slash rule.
    let repo = store
        .add_repo(std::path::Path::new("/tmp/mb"), "meals backend", "wsx")
        .unwrap();
    store
        .insert_workspace(&NewWorkspace {
            repo_id: repo,
            name: "api-fix",
            branch: "wsx/api-fix",
            worktree_path: std::path::Path::new("/tmp/mb/api-fix"),
            yolo: false,
            agent: crate::pty::session::AgentKind::Claude,
            shared: false,
        })
        .unwrap();
    store
}

#[test]
fn workspace_spec_splits_on_the_last_slash() {
    let store = seed_spec_store();
    let ws = resolve_workspace_spec(&store, "meals backend/api-fix").unwrap();
    assert_eq!(ws.name, "api-fix");
}

#[test]
fn workspace_spec_errors_name_the_valid_alternatives() {
    let store = seed_spec_store();

    let e = resolve_workspace_spec(&store, "noslug")
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("<repo>/<slug>"),
        "must show the expected form: {e}"
    );

    let e = resolve_workspace_spec(&store, "/api-fix")
        .unwrap_err()
        .to_string();
    assert!(e.contains("<repo>/<slug>"), "empty repo is malformed: {e}");

    let e = resolve_workspace_spec(&store, "meals backend/")
        .unwrap_err()
        .to_string();
    assert!(e.contains("<repo>/<slug>"), "empty slug is malformed: {e}");

    let e = resolve_workspace_spec(&store, "nope/api-fix")
        .unwrap_err()
        .to_string();
    assert!(e.contains("meals backend"), "must list known repos: {e}");

    let e = resolve_workspace_spec(&store, "meals backend/nope")
        .unwrap_err()
        .to_string();
    assert!(e.contains("api-fix"), "must list known slugs: {e}");
}

/// Dispatch-arm coverage for `wsx agent send --workspace`: the target
/// workspace, its label resolution, and the `enqueue_message` argument
/// order are the one seam nothing else in the branch exercises.
#[tokio::test]
async fn agent_send_dispatch_targets_the_other_workspaces_primary() {
    use crate::config::Dirs;
    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;
    use crate::test_support::EnvGuard;

    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::for_test(tmp.path());

    // Seed two workspaces directly against the DB file `run_cli` will
    // open, so we can assert the queued row lands against the TARGET,
    // not the sender's own (origin) workspace.
    let (origin_primary, target_ws, target_primary) = {
        let store = Store::open(&dirs.db_path()).unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let origin = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "origin",
                branch: "wsx/origin",
                worktree_path: std::path::Path::new("/tmp/r/origin"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let origin_primary = store
            .add_primary_agent(origin, AgentKind::Claude, 1)
            .unwrap();

        let target = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "target",
                branch: "wsx/target",
                worktree_path: std::path::Path::new("/tmp/r/target"),
                yolo: false,
                agent: AgentKind::Claude,
                shared: false,
            })
            .unwrap();
        let target_primary = store
            .add_primary_agent(target, AgentKind::Claude, 1)
            .unwrap();
        (origin_primary.id, target, target_primary.id)
    };

    let mut env = EnvGuard::new();
    // Point the "is a TUI running" check at an empty scratch dir so the
    // dispatch's stderr warning path is deterministic regardless of the
    // ambient environment (this process may itself be running under a
    // live wsx dashboard).
    env.set("XDG_RUNTIME_DIR", tmp.path());
    // Target resolution must come entirely from `workspace`, not from
    // the sender's own identity, so leave the sender unset.
    env.remove("WSX_AGENT_INSTANCE_ID");

    let action = CliAction::AgentSend {
        target: "primary".to_string(),
        prompt: "do the thing".to_string(),
        workspace: Some("r/target".to_string()),
    };
    run_cli(action, &dirs).await.unwrap();

    let store = Store::open(&dirs.db_path()).unwrap();
    let queued = store.undelivered_messages().unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(
        queued[0].workspace_id, target_ws,
        "must queue against the TARGET workspace"
    );
    assert_eq!(
        queued[0].target_agent_id, target_primary,
        "must resolve `primary` against the TARGET workspace, not the origin"
    );
    assert_ne!(
        queued[0].target_agent_id, origin_primary,
        "must not resolve `primary` against the origin workspace"
    );
    assert_eq!(queued[0].body, "do the thing");
}

/// The unknown-label error must offer `primary` alongside the concrete
/// labels. A cross-workspace sender cannot run `wsx agent list` against
/// the target, so this error is the one place it learns how to recover —
/// and `primary` is the label that works whatever kind the target runs.
#[tokio::test]
async fn agent_send_unknown_label_error_offers_the_primary_alias() {
    use crate::config::Dirs;
    use crate::data::store::{NewWorkspace, Store};
    use crate::pty::session::AgentKind;
    use crate::test_support::EnvGuard;

    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::for_test(tmp.path());
    {
        let store = Store::open(&dirs.db_path()).unwrap();
        let repo = store
            .add_repo(std::path::Path::new("/tmp/r"), "r", "wsx")
            .unwrap();
        let ws = store
            .insert_workspace(&NewWorkspace {
                repo_id: repo,
                name: "target",
                branch: "wsx/target",
                worktree_path: std::path::Path::new("/tmp/r/target"),
                yolo: false,
                agent: AgentKind::Hermes,
                shared: false,
            })
            .unwrap();
        store.add_primary_agent(ws, AgentKind::Hermes, 1).unwrap();
    }

    let mut env = EnvGuard::new();
    env.set("XDG_RUNTIME_DIR", tmp.path());
    env.remove("WSX_AGENT_INSTANCE_ID");

    // Guess the wrong kind label against a hermes-primary workspace —
    // the exact case a sender hits when it cannot enumerate the target.
    let action = CliAction::AgentSend {
        target: "claude".to_string(),
        prompt: "do the thing".to_string(),
        workspace: Some("r/target".to_string()),
    };
    let err = run_cli(action, &dirs).await.unwrap_err().to_string();
    assert!(
        err.contains("hermes"),
        "must list the concrete labels that exist: {err}"
    );
    assert!(
        err.contains("primary"),
        "must offer the primary alias as a recovery path: {err}"
    );
    // Nothing is queued when resolution fails.
    let store = Store::open(&dirs.db_path()).unwrap();
    assert!(store.undelivered_messages().unwrap().is_empty());
}

#[test]
fn misuse_is_tagged_with_group() {
    match parse(&["agent", "send"]) {
        Err(Error::Usage {
            group: Some("agent"),
            ..
        }) => {}
        other => panic!("expected agent-tagged Usage, got {other:?}"),
    }
}

#[test]
fn unknown_command_is_untagged_usage() {
    match parse(&["bogus"]) {
        Err(Error::Usage { group: None, .. }) => {}
        other => panic!("expected untagged Usage, got {other:?}"),
    }
}

#[test]
fn parses_top_level_help_forms() {
    for f in ["--help", "-h", "help"] {
        assert!(matches!(
            parse(&[f]).unwrap(),
            CliAction::Help(HelpTopic::Root)
        ));
    }
}

#[test]
fn parses_version_forms() {
    for f in ["--version", "-V"] {
        assert!(matches!(parse(&[f]).unwrap(), CliAction::Version));
    }
}

#[test]
fn bare_wsx_is_tui() {
    assert!(matches!(parse(&[]).unwrap(), CliAction::Tui { .. }));
}

#[test]
fn parses_select_launch_flag() {
    match parse(&["--select", "meals backend/api-fix"]) {
        Ok(CliAction::Tui {
            select: Some((repo, slug)),
        }) => {
            assert_eq!(repo, "meals backend");
            assert_eq!(slug, "api-fix");
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(matches!(parse(&[]), Ok(CliAction::Tui { select: None })));
    assert!(parse(&["--select"]).is_err());
    assert!(parse(&["--select", "no-slash"]).is_err());
}

#[test]
fn parses_group_help_forms() {
    let want = |a: CliAction| matches!(a, CliAction::Help(HelpTopic::Group("agent")));
    assert!(want(parse(&["agent", "--help"]).unwrap()));
    assert!(want(parse(&["agent", "-h"]).unwrap()));
    assert!(want(parse(&["help", "agent"]).unwrap()));
}

#[test]
fn dashed_help_flag_triggers_group_help_anywhere() {
    let want = |a: CliAction| matches!(a, CliAction::Help(HelpTopic::Group("agent")));
    // After a valid subcommand, a dashed flag still surfaces group help.
    assert!(want(parse(&["agent", "send", "--help"]).unwrap()));
    assert!(want(parse(&["agent", "send", "-h"]).unwrap()));
}

/// The ordering settings the dashboard reads must be reachable from
/// `wsx config set`, or "configurable" is only true for hand-edited SQL.
#[test]
fn dashboard_ordering_settings_are_settable_from_the_cli() {
    for key in ["dashboard_sort_mode", "dashboard_blocked_pin_max_age_secs"] {
        match parse(&["config", "set", key, "x"]).unwrap() {
            CliAction::ConfigSet { key: k, .. } => assert_eq!(k, key),
            other => panic!("expected ConfigSet for {key}, got {other:?}"),
        }
    }
}

#[test]
fn bare_help_is_a_subcommand_not_a_value() {
    // `help` in the subcommand slot → group help.
    assert!(matches!(
        parse(&["repo", "help"]).unwrap(),
        CliAction::Help(HelpTopic::Group("repo"))
    ));
    // `help` as an argument VALUE must NOT trigger help.
    match parse(&["repo", "remove", "help"]).unwrap() {
        CliAction::RepoRemove { name } => assert_eq!(name, "help"),
        other => panic!("expected RepoRemove {{ name: \"help\" }}, got {other:?}"),
    }
    match parse(&["config", "set", "editor_cmd", "help"]).unwrap() {
        CliAction::ConfigSet {
            key,
            source: ValueSource::Literal(v),
        } => {
            assert_eq!(key, "editor_cmd");
            assert_eq!(v, "help");
        }
        other => panic!("expected ConfigSet value \"help\", got {other:?}"),
    }
    match parse(&["agent", "send", "claude", "help"]).unwrap() {
        CliAction::AgentSend {
            target,
            prompt,
            workspace,
        } => {
            assert_eq!(target, "claude");
            assert_eq!(prompt, "help");
            assert_eq!(workspace, None);
        }
        other => panic!("expected AgentSend prompt \"help\", got {other:?}"),
    }
}

#[test]
fn help_for_unknown_group_falls_back_to_root() {
    assert!(matches!(
        parse(&["help", "bogus"]).unwrap(),
        CliAction::Help(HelpTopic::Root)
    ));
}

#[test]
fn group_name_resolves_known_and_unknown() {
    assert_eq!(group_name("agent"), Some("agent"));
    assert_eq!(group_name("workspace"), Some("workspace"));
    assert_eq!(group_name("bogus"), None);
}

#[test]
fn root_help_lists_every_group() {
    let h = render_root_help();
    for g in GROUPS {
        assert!(h.contains(g.name), "root help missing group {}", g.name);
    }
    assert!(h.contains("launches the TUI"));
}

#[test]
fn agent_group_help_lists_its_commands() {
    let h = render_group_help("agent");
    assert!(h.contains("list"));
    assert!(h.contains("add <kind>"));
    assert!(h.contains("send [--workspace <repo>/<slug>] <label> <message...>"));
}

#[test]
fn usage_error_has_message_then_group_block() {
    let s = render_usage_error(Some("agent"), "missing arguments");
    assert!(s.starts_with("error: missing arguments"));
    assert!(s.contains("send [--workspace <repo>/<slug>] <label> <message...>"));
}

#[test]
fn parses_config_set_literal() {
    let a = parse(&["config", "set", "branch_prefix", "bakedbean"]).unwrap();
    match a {
        CliAction::ConfigSet {
            key,
            source: ValueSource::Literal(v),
        } => {
            assert_eq!(key, "branch_prefix");
            assert_eq!(v, "bakedbean");
        }
        _ => panic!("wrong action"),
    }
}

#[test]
fn parses_config_set_file_reference() {
    let a = parse(&["config", "set", "custom_instructions", "@/tmp/foo.md"]).unwrap();
    match a {
        CliAction::ConfigSet {
            key,
            source: ValueSource::File(p),
        } => {
            assert_eq!(key, "custom_instructions");
            assert_eq!(p, std::path::PathBuf::from("/tmp/foo.md"));
        }
        _ => panic!("wrong action"),
    }
}

#[test]
fn rejects_unknown_setting_key() {
    assert!(parse(&["config", "set", "nope", "val"]).is_err());
    assert!(parse(&["config", "get", "nope"]).is_err());
}

#[test]
fn unknown_setting_key_is_tagged_config_usage() {
    match parse(&["config", "set", "nope", "x"]) {
        Err(Error::Usage {
            group: Some("config"),
            msg,
        }) => {
            assert_eq!(msg, "unknown setting key: nope");
        }
        other => panic!("expected config-tagged Usage, got {other:?}"),
    }
    // get and edit forms too
    assert!(matches!(
        parse(&["config", "get", "nope"]),
        Err(Error::Usage {
            group: Some("config"),
            ..
        })
    ));
    assert!(matches!(
        parse(&["config", "edit", "nope"]),
        Err(Error::Usage {
            group: Some("config"),
            ..
        })
    ));
}

#[test]
fn accepts_usage_graph_window() {
    assert!(known_setting_key("usage_graph_window"));
}

#[test]
fn usage_window_validate_accepts_canonical_tokens() {
    assert_eq!(usage_window_validate_and_normalize("24h").unwrap(), "24h");
    assert_eq!(usage_window_validate_and_normalize("1w").unwrap(), "1w");
    assert_eq!(usage_window_validate_and_normalize("1mo").unwrap(), "1mo");
}

#[test]
fn usage_window_validate_trims_whitespace() {
    assert_eq!(usage_window_validate_and_normalize(" 1w\n").unwrap(), "1w");
}

#[test]
fn usage_window_validate_rejects_garbage() {
    assert!(usage_window_validate_and_normalize("week").is_err());
    assert!(usage_window_validate_and_normalize("").is_err());
    assert!(usage_window_validate_and_normalize("1d").is_err());
}

#[test]
fn accepts_diff_cmd() {
    assert!(known_setting_key("diff_cmd"));
}

#[test]
fn accepts_lazygit_cmd() {
    assert!(known_setting_key("lazygit_cmd"));
}

#[test]
fn accepts_chronox_cmd() {
    assert!(known_setting_key("chronox_cmd"));
}

#[test]
fn accepts_mcp_mirror() {
    assert!(known_setting_key("mcp_mirror"));
}

#[test]
fn accepts_remote_control_settings() {
    assert!(known_setting_key("remote_control"));
    assert!(known_setting_key("remote_control_sandbox"));
}

#[test]
fn parses_repo_set_prefix() {
    let a = parse(&["repo", "set-prefix", "myrepo", "bakedbean"]).unwrap();
    match a {
        CliAction::RepoSetPrefix { name, prefix } => {
            assert_eq!(name, "myrepo");
            assert_eq!(prefix, "bakedbean");
        }
        _ => panic!("wrong action"),
    }
}

#[test]
fn parses_repo_set_setup_literal() {
    let a = parse(&["repo", "set-setup", "demo", "bun install"]).unwrap();
    match a {
        CliAction::RepoSetSetup {
            name,
            source: ValueSource::Literal(v),
        } => {
            assert_eq!(name, "demo");
            assert_eq!(v, "bun install");
        }
        _ => panic!("wrong action"),
    }
}

#[test]
fn parses_repo_set_setup_file_reference() {
    let a = parse(&["repo", "set-setup", "demo", "@./setup.sh"]).unwrap();
    match a {
        CliAction::RepoSetSetup {
            name,
            source: ValueSource::File(p),
        } => {
            assert_eq!(name, "demo");
            assert_eq!(p, std::path::PathBuf::from("./setup.sh"));
        }
        _ => panic!("wrong action"),
    }
}

#[test]
fn parses_repo_set_archive_literal() {
    let a = parse(&["repo", "set-archive", "demo", "rm -rf node_modules"]).unwrap();
    match a {
        CliAction::RepoSetArchive {
            name,
            source: ValueSource::Literal(v),
        } => {
            assert_eq!(name, "demo");
            assert_eq!(v, "rm -rf node_modules");
        }
        _ => panic!("wrong action"),
    }
}

#[test]
fn parses_repo_edit_setup_and_edit_archive() {
    match parse(&["repo", "edit-setup", "demo"]).unwrap() {
        CliAction::RepoEditSetup { name } => assert_eq!(name, "demo"),
        _ => panic!("wrong action"),
    }
    match parse(&["repo", "edit-archive", "demo"]).unwrap() {
        CliAction::RepoEditArchive { name } => assert_eq!(name, "demo"),
        _ => panic!("wrong action"),
    }
}

#[test]
fn config_set_accepts_pinned_commands_key() {
    let a = parse(&["config", "set", "pinned_commands", "/feedback"]).unwrap();
    match a {
        CliAction::ConfigSet { key, .. } => assert_eq!(key, "pinned_commands"),
        other => panic!("unexpected action: {other:?}"),
    }
}

#[test]
fn parse_repo_set_pinned_commands_literal() {
    let a = parse(&["repo", "set-pinned-commands", "demo", "PR=/pull-request"]).unwrap();
    match a {
        CliAction::RepoSetPinnedCommands {
            name,
            source: ValueSource::Literal(v),
        } => {
            assert_eq!(name, "demo");
            assert_eq!(v, "PR=/pull-request");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_repo_set_pinned_commands_at_file() {
    let a = parse(&["repo", "set-pinned-commands", "demo", "@./pinned.txt"]).unwrap();
    match a {
        CliAction::RepoSetPinnedCommands {
            name,
            source: ValueSource::File(p),
        } => {
            assert_eq!(name, "demo");
            assert_eq!(p, std::path::PathBuf::from("./pinned.txt"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_repo_edit_pinned_commands() {
    match parse(&["repo", "edit-pinned-commands", "demo"]).unwrap() {
        CliAction::RepoEditPinnedCommands { name } => assert_eq!(name, "demo"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_repo_set_related_repos_literal() {
    let a = parse(&["repo", "set-related-repos", "backend", "frontend,marketing"]).unwrap();
    match a {
        CliAction::RepoSetRelatedRepos { name, source } => {
            assert_eq!(name, "backend");
            assert!(matches!(source, ValueSource::Literal(ref s) if s == "frontend,marketing"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_repo_set_related_repos_at_file() {
    let a = parse(&["repo", "set-related-repos", "backend", "@./related.txt"]).unwrap();
    match a {
        CliAction::RepoSetRelatedRepos { source, .. } => {
            assert!(matches!(source, ValueSource::File(_)));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_repo_set_name() {
    let a = parse(&["repo", "set-name", "myrepo", "my-new-name"]).unwrap();
    match a {
        CliAction::RepoSetName { name, new_name } => {
            assert_eq!(name, "myrepo");
            assert_eq!(new_name, "my-new-name");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parse_repo_edit_related_repos() {
    match parse(&["repo", "edit-related-repos", "backend"]).unwrap() {
        CliAction::RepoEditRelatedRepos { name } => assert_eq!(name, "backend"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_remote_list_no_args() {
    match parse(&["remote"]).unwrap() {
        CliAction::RemoteList => {}
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_remote_run_with_name() {
    match parse(&["remote", "ebenmini"]).unwrap() {
        CliAction::RemoteRun { name } => assert_eq!(name, "ebenmini"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn accepts_remotes_setting_key() {
    assert!(known_setting_key("remotes"));
}

#[test]
fn accepts_shared_hosts_setting_key() {
    assert!(known_setting_key("shared_hosts"));
}

#[test]
fn parses_shared_list_json() {
    match parse(&["shared", "list", "--json"]).unwrap() {
        CliAction::SharedList { json } => assert!(json),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_shared_list_without_json() {
    match parse(&["shared", "list"]).unwrap() {
        CliAction::SharedList { json } => assert!(!json),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_shared_list_rejects_unknown_arg() {
    assert!(parse(&["shared", "list", "--bogus"]).is_err());
}

#[test]
fn parses_shared_rejects_unknown_subcommand() {
    assert!(parse(&["shared", "bogus"]).is_err());
    assert!(parse(&["shared"]).is_err());
}

#[test]
fn parses_repo_set_base_branch_literal() {
    match parse(&["repo", "set-base-branch", "demo", "origin/main"]).unwrap() {
        CliAction::RepoSetBaseBranch { name, value } => {
            assert_eq!(name, "demo");
            assert_eq!(value, "origin/main");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_create_minimal() {
    match parse(&["workspace", "create", "backend"]).unwrap() {
        CliAction::WorkspaceCreate {
            repo,
            name,
            yolo,
            shared,
            agent: None,
            prompt,
        } => {
            assert_eq!(repo, "backend");
            assert!(name.is_none());
            assert!(!yolo);
            assert!(!shared);
            assert!(prompt.is_none());
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_create_with_name_and_yolo() {
    match parse(&[
        "workspace",
        "create",
        "backend",
        "--name",
        "add-widgets",
        "--yolo",
    ])
    .unwrap()
    {
        CliAction::WorkspaceCreate {
            repo,
            name,
            yolo,
            shared,
            agent: None,
            prompt: None,
        } => {
            assert_eq!(repo, "backend");
            assert_eq!(name.as_deref(), Some("add-widgets"));
            assert!(yolo);
            assert!(!shared);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

/// The phone flow: one command creates the workspace AND seeds its
/// agent, so the only thing typed on a phone keyboard is the prompt.
#[test]
fn parses_workspace_create_with_prompt() {
    match parse(&[
        "workspace",
        "create",
        "backend",
        "--prompt",
        "fix the flaky tests",
    ])
    .unwrap()
    {
        CliAction::WorkspaceCreate { repo, prompt, .. } => {
            assert_eq!(repo, "backend");
            assert_eq!(prompt.as_deref(), Some("fix the flaky tests"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

/// `--prompt` composes with every other create flag — the phone flow
/// still wants `--yolo` and an explicit `--name` sometimes.
#[test]
fn parses_workspace_create_prompt_alongside_other_flags() {
    match parse(&[
        "workspace",
        "create",
        "backend",
        "--name",
        "flaky-tests",
        "--yolo",
        "--agent",
        "claude",
        "--prompt",
        "fix the flaky tests",
    ])
    .unwrap()
    {
        CliAction::WorkspaceCreate {
            repo,
            name,
            yolo,
            shared,
            agent,
            prompt,
        } => {
            assert_eq!(repo, "backend");
            assert_eq!(name.as_deref(), Some("flaky-tests"));
            assert!(yolo);
            assert!(!shared);
            assert_eq!(agent.as_deref(), Some("claude"));
            assert_eq!(prompt.as_deref(), Some("fix the flaky tests"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

/// A bare `--prompt` must be a usage error rather than silently
/// creating an unseeded workspace the sender believes is running.
#[test]
fn parses_workspace_create_rejects_prompt_without_value() {
    assert!(parse(&["workspace", "create", "backend", "--prompt"]).is_err());
}

#[test]
fn parses_workspace_create_with_shared() {
    let a = parse(&["workspace", "create", "myrepo", "--shared"]).unwrap();
    match a {
        CliAction::WorkspaceCreate { repo, shared, .. } => {
            assert_eq!(repo, "myrepo");
            assert!(shared);
        }
        other => panic!("wrong action: {other:?}"),
    }
}

#[test]
fn parses_workspace_create_rejects_unknown_arg() {
    assert!(parse(&["workspace", "create", "backend", "--bogus"]).is_err());
}

/// The recovery hint has to be pasteable verbatim, so it must carry the
/// real prompt — not a placeholder — and survive the two things that
/// routinely break a hand-built command line: spaces in a repo name and
/// arbitrary text in a prompt.
#[test]
fn retry_hint_resends_the_actual_prompt() {
    let hint = retry_send_hint("backend", "add-widgets", "fix the flaky tests");
    assert!(
        hint.contains("fix the flaky tests"),
        "must carry the real prompt, not a placeholder: {hint}"
    );
    assert!(
        !hint.contains("\"...\""),
        "a placeholder makes the hint unusable: {hint}"
    );
    assert!(hint.contains("--workspace"), "{hint}");
    assert!(hint.contains("primary"), "{hint}");
}

#[test]
fn retry_hint_quotes_spaces_and_metacharacters() {
    // Repo names may contain spaces — `resolve_workspace_spec` splits on
    // the LAST slash for exactly this reason.
    let spaced = retry_send_hint("meals backend", "add-widgets", "do it");
    assert!(
        spaced.contains("'meals backend/add-widgets'"),
        "a spaced repo name must stay one argument: {spaced}"
    );

    // A prompt is arbitrary text; quotes and shell metacharacters in it
    // must not escape into the command.
    let nasty = retry_send_hint("r", "w", "it's $HOME; rm -rf /");
    let parsed = shlex::split(&nasty).expect("hint must be valid shell");
    assert_eq!(
        parsed.last().map(String::as_str),
        Some("it's $HOME; rm -rf /"),
        "the prompt must round-trip as a single literal argument: {nasty}"
    );
    assert_eq!(
        parsed,
        vec![
            "wsx",
            "agent",
            "send",
            "--workspace",
            "r/w",
            "primary",
            "it's $HOME; rm -rf /"
        ],
        "the hint must parse as exactly the intended argv"
    );
}

fn init_git_repo() -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().unwrap();
    let r = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .current_dir(dir.path())
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    r(&["init", "-q", "-b", "main"]);
    r(&["config", "user.email", "t@e"]);
    r(&["config", "user.name", "t"]);
    r(&["commit", "--allow-empty", "-q", "-m", "init"]);
    dir
}

/// `--prompt` must queue against the workspace it just created, aimed at
/// the primary agent seeded at birth. This is the whole phone flow: the
/// dashboard spawns that agent on demand when it drains the inbox, so a
/// message on the wrong target (or no message at all) is a workspace that
/// silently never starts.
#[tokio::test]
async fn workspace_create_with_prompt_queues_it_to_the_new_primary() {
    use crate::config::Dirs;
    use crate::data::store::Store;
    use crate::test_support::EnvGuard;

    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::for_test(tmp.path());
    let repo_dir = init_git_repo();
    {
        let store = Store::open(&dirs.db_path()).unwrap();
        crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
    }

    let mut env = EnvGuard::new();
    // Point the "is a TUI running" check at an empty scratch dir so the
    // no-dashboard warning path is deterministic regardless of whether
    // this process is itself running under a live wsx dashboard.
    env.set("XDG_RUNTIME_DIR", tmp.path());
    env.remove("WSX_AGENT_INSTANCE_ID");

    run_cli(
        CliAction::WorkspaceCreate {
            repo: "demo".to_string(),
            name: Some("seeded".to_string()),
            yolo: false,
            shared: false,
            agent: None,
            prompt: Some("fix the flaky tests".to_string()),
        },
        &dirs,
    )
    .await
    .unwrap();

    let store = Store::open(&dirs.db_path()).unwrap();
    let ws = store
        .repos()
        .unwrap()
        .into_iter()
        .flat_map(|r| store.workspaces(r.id).unwrap())
        .find(|w| w.name == "seeded")
        .expect("workspace must exist");
    let primary = store
        .primary_instance_id(ws.id)
        .unwrap()
        .expect("create seeds a primary agent at birth");

    let queued = store.undelivered_messages().unwrap();
    assert_eq!(queued.len(), 1, "exactly one seeded prompt");
    assert_eq!(queued[0].workspace_id, ws.id);
    assert_eq!(
        queued[0].target_agent_id, primary,
        "must target the new workspace's primary agent"
    );
    assert_eq!(queued[0].body, "fix the flaky tests");
}

/// Without `--prompt`, create must not invent an inbox message —
/// otherwise every plain `workspace create` would wake an agent.
#[tokio::test]
async fn workspace_create_without_prompt_queues_nothing() {
    use crate::config::Dirs;
    use crate::data::store::Store;
    use crate::test_support::EnvGuard;

    let tmp = tempfile::tempdir().unwrap();
    let dirs = Dirs::for_test(tmp.path());
    let repo_dir = init_git_repo();
    {
        let store = Store::open(&dirs.db_path()).unwrap();
        crate::data::repo::add(&store, repo_dir.path(), "demo", "wsx")
            .await
            .unwrap();
    }

    let mut env = EnvGuard::new();
    env.set("XDG_RUNTIME_DIR", tmp.path());
    env.remove("WSX_AGENT_INSTANCE_ID");

    run_cli(
        CliAction::WorkspaceCreate {
            repo: "demo".to_string(),
            name: Some("quiet".to_string()),
            yolo: false,
            shared: false,
            agent: None,
            prompt: None,
        },
        &dirs,
    )
    .await
    .unwrap();

    let store = Store::open(&dirs.db_path()).unwrap();
    assert!(
        store.undelivered_messages().unwrap().is_empty(),
        "a create without --prompt must leave the inbox untouched"
    );
}

use crate::pty::session::AgentKind;

fn parent_ws(yolo: bool, agent: AgentKind) -> crate::data::store::Workspace {
    use crate::data::store::{RepoId, SetupStatus, Workspace, WorkspaceId, WorkspaceState};
    Workspace {
        id: WorkspaceId(1),
        repo_id: RepoId(1),
        name: "parent".into(),
        branch: "x/parent".into(),
        worktree_path: std::path::PathBuf::from("/tmp/p"),
        state: WorkspaceState::Ready,
        setup_status: SetupStatus::Ok,
        created_at: 0,
        yolo,
        agent,
        shared: false,
        name_color: None,
    }
}

#[test]
fn create_flags_without_parent_keep_todays_defaults() {
    assert_eq!(
        effective_create_flags(false, None, None, AgentKind::Claude),
        (false, AgentKind::Claude)
    );
    assert_eq!(
        effective_create_flags(true, Some("pi"), None, AgentKind::Claude),
        (true, AgentKind::Pi)
    );
}

#[test]
fn create_flags_without_parent_fall_back_to_coding_agent_setting() {
    assert_eq!(
        effective_create_flags(false, None, None, AgentKind::Codex),
        (false, AgentKind::Codex)
    );
}

#[test]
fn create_flags_inherit_yolo_and_agent_from_parent() {
    let parent = parent_ws(true, AgentKind::Pi);
    assert_eq!(
        effective_create_flags(false, None, Some(&parent), AgentKind::Claude),
        (true, AgentKind::Pi)
    );
}

#[test]
fn create_flags_parent_agent_beats_coding_agent_setting() {
    let parent = parent_ws(false, AgentKind::Pi);
    assert_eq!(
        effective_create_flags(false, None, Some(&parent), AgentKind::Codex),
        (false, AgentKind::Pi)
    );
}

#[test]
fn create_flags_explicit_agent_beats_parent() {
    let parent = parent_ws(false, AgentKind::Pi);
    assert_eq!(
        effective_create_flags(false, Some("codex"), Some(&parent), AgentKind::Claude),
        (false, AgentKind::Codex)
    );
}

#[test]
fn create_flags_explicit_yolo_ors_with_parent() {
    let parent = parent_ws(false, AgentKind::Claude);
    assert_eq!(
        effective_create_flags(true, None, Some(&parent), AgentKind::Claude),
        (true, AgentKind::Claude)
    );
}

#[test]
fn create_flags_non_yolo_claude_parent_matches_defaults() {
    let parent = parent_ws(false, AgentKind::Claude);
    assert_eq!(
        effective_create_flags(false, None, Some(&parent), AgentKind::Claude),
        (false, AgentKind::Claude)
    );
}

#[test]
fn parses_workspace_list_no_filter() {
    match parse(&["workspace", "list"]).unwrap() {
        CliAction::WorkspaceList { repo } => assert!(repo.is_none()),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_list_with_repo_filter() {
    match parse(&["workspace", "list", "backend"]).unwrap() {
        CliAction::WorkspaceList { repo } => assert_eq!(repo.as_deref(), Some("backend")),
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_path() {
    match parse(&["workspace", "path", "backend", "add-widgets"]).unwrap() {
        CliAction::WorkspacePath { repo, name } => {
            assert_eq!(repo, "backend");
            assert_eq!(name, "add-widgets");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_rename() {
    match parse(&["workspace", "rename", "backend", "old-slug", "new-slug"]).unwrap() {
        CliAction::WorkspaceRename {
            repo,
            name,
            new_name,
        } => {
            assert_eq!(repo, "backend");
            assert_eq!(name, "old-slug");
            assert_eq!(new_name, "new-slug");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_archive_minimal() {
    match parse(&["workspace", "archive", "backend", "add-widgets"]).unwrap() {
        CliAction::WorkspaceArchive {
            repo,
            name,
            keep_worktree,
            force_delete_branch,
        } => {
            assert_eq!(repo, "backend");
            assert_eq!(name, "add-widgets");
            assert!(!keep_worktree);
            assert!(!force_delete_branch);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_archive_with_flags() {
    match parse(&[
        "workspace",
        "archive",
        "backend",
        "add-widgets",
        "--keep-worktree",
        "--force-delete-branch",
    ])
    .unwrap()
    {
        CliAction::WorkspaceArchive {
            keep_worktree,
            force_delete_branch,
            ..
        } => {
            assert!(keep_worktree);
            assert!(force_delete_branch);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_share() {
    match parse(&["workspace", "share", "backend", "add-widgets"]).unwrap() {
        CliAction::WorkspaceShare { repo, name, shared } => {
            assert_eq!(repo, "backend");
            assert_eq!(name, "add-widgets");
            assert!(shared);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_unshare() {
    match parse(&["workspace", "unshare", "backend", "add-widgets"]).unwrap() {
        CliAction::WorkspaceShare { repo, name, shared } => {
            assert_eq!(repo, "backend");
            assert_eq!(name, "add-widgets");
            assert!(!shared);
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_workspace_rejects_unknown_subcommand() {
    assert!(parse(&["workspace", "bogus"]).is_err());
}

#[test]
fn parses_setup_install_skill() {
    match parse(&["setup", "install-skill"]).unwrap() {
        CliAction::SetupInstallSkill => {}
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn parses_setup_rejects_unknown_subcommand() {
    assert!(parse(&["setup", "bogus"]).is_err());
    assert!(parse(&["setup"]).is_err());
}

#[test]
fn parses_repo_set_base_branch_empty_value() {
    match parse(&["repo", "set-base-branch", "demo", ""]).unwrap() {
        CliAction::RepoSetBaseBranch { name, value } => {
            assert_eq!(name, "demo");
            assert_eq!(value, "");
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn detail_bar_config_seed_returns_pretty_default_when_empty() {
    let seed = resolve::detail_bar_config_seed_for_empty();
    // Sanity: round-trips to default config.
    let parsed: crate::config::detail_bar_config::DetailBarConfig =
        serde_json::from_str(&seed).unwrap();
    assert_eq!(
        parsed,
        crate::config::detail_bar_config::DetailBarConfig::default()
    );
    // Pretty-printed: contains newlines.
    assert!(seed.contains('\n'));
}

#[test]
fn detail_bar_config_validate_rejects_malformed() {
    let result = resolve::detail_bar_config_validate_and_normalize("{not json");
    assert!(result.is_err());
}

#[test]
fn detail_bar_config_validate_clamps_out_of_range() {
    let json = r#"{"height": {"percent": 200}}"#;
    let normalized = resolve::detail_bar_config_validate_and_normalize(json).unwrap();
    let parsed: crate::config::detail_bar_config::DetailBarConfig =
        serde_json::from_str(&normalized).unwrap();
    assert_eq!(parsed.height.percent, 80);
}

#[test]
fn detail_bar_config_validate_accepts_partial() {
    let json = r#"{"visible": false}"#;
    let normalized = resolve::detail_bar_config_validate_and_normalize(json).unwrap();
    let parsed: crate::config::detail_bar_config::DetailBarConfig =
        serde_json::from_str(&normalized).unwrap();
    assert!(!parsed.visible);
    assert_eq!(parsed.height.percent, 30);
}

#[test]
fn detail_bar_config_default_seed_round_trips() {
    use crate::config::detail_bar_config::DetailBarConfig;
    let seed =
        serde_json::to_string_pretty(&DetailBarConfig::default()).expect("serialize default");
    let parsed: DetailBarConfig =
        serde_json::from_str(&seed).expect("seed must parse with new schema");
    assert_eq!(parsed, DetailBarConfig::default());
    // Spot-check: the new shape uses `containers`, not `sections`.
    assert!(seed.contains("\"containers\""));
    assert!(!seed.contains("\"sections\""));
}

#[test]
fn process_doctrine_is_a_known_setting() {
    assert!(known_setting_key("process_doctrine"));
    assert!(known_setting_key("process_doctrine_extra"));
}

#[test]
fn parses_agent_send_joins_prompt() {
    match parse(&["agent", "send", "claude#2", "hello", "there"]).unwrap() {
        CliAction::AgentSend {
            target,
            prompt,
            workspace,
        } => {
            assert_eq!(target, "claude#2");
            assert_eq!(prompt, "hello there");
            assert_eq!(workspace, None, "no flag → current workspace");
        }
        other => panic!("expected AgentSend, got {other:?}"),
    }
}

#[test]
fn workspace_create_accepts_every_agent_kind() {
    use crate::pty::session::AgentKind;
    for k in AgentKind::ALL {
        let name = k.display_name();
        assert!(
            parse(&["workspace", "create", "myrepo", "--agent", name]).is_ok(),
            "--agent {name} must be accepted"
        );
    }
    assert!(parse(&["workspace", "create", "myrepo", "--agent", "bogus"]).is_err());
}

#[test]
fn parses_agent_list_and_add() {
    assert!(matches!(
        parse(&["agent", "list"]).unwrap(),
        CliAction::AgentList
    ));
    assert!(matches!(
        parse(&["agent", "add", "codex"]).unwrap(),
        CliAction::AgentAdd { .. }
    ));
    assert!(parse(&["agent", "add", "bogus"]).is_err());
}

#[test]
fn detail_bar_config_validate_truncates_too_many_containers() {
    let raw = serde_json::json!({
        "containers": [
            ["a"], ["b"], ["c"], ["d"], ["e"], ["f"]
        ]
    })
    .to_string();
    let normalized = resolve::detail_bar_config_validate_and_normalize(&raw)
        .expect("valid JSON should normalize");
    // Truncation happens inside sanitize(); the normalized blob
    // should round-trip to exactly 4 containers.
    let parsed: crate::config::detail_bar_config::DetailBarConfig =
        serde_json::from_str(&normalized).expect("re-parse normalized");
    assert_eq!(parsed.containers.len(), 4);
}

#[test]
fn report_cli_error_formats_usage_block() {
    let e = Error::Usage {
        group: Some("agent"),
        msg: "agent send needs <label> <message...>".into(),
    };
    let s = report_cli_error(&e);
    assert!(s.starts_with("error: agent send needs"));
    assert!(s.contains("send [--workspace <repo>/<slug>] <label> <message...>"));
}

#[test]
fn report_cli_error_falls_back_for_other_errors() {
    let e = Error::UserInput("unknown setting key: nope".into());
    let s = report_cli_error(&e);
    assert!(s.contains("unknown setting key: nope"));
}

#[test]
fn unknown_subcommand_messages_are_clean() {
    // No Debug-formatted Option (`None` / `Some("..")`) leaking into user text.
    let missing = match parse(&["workspace"]) {
        Err(e) => e.to_string(),
        _ => panic!("expected error"),
    };
    assert_eq!(missing, "missing workspace command");
    let unknown = match parse(&["workspace", "bogus"]) {
        Err(e) => e.to_string(),
        _ => panic!("expected error"),
    };
    assert_eq!(unknown, "unknown workspace command: bogus");
    assert!(!missing.contains("None"));
    assert!(!unknown.contains("Some("));
}

#[test]
fn registry_matches_dispatched_groups() {
    // Every group the dispatcher accepts must have a help entry, and every
    // help entry must be a real group. Update BOTH when adding a command group.
    let dispatched = [
        "workspace",
        "agent",
        "repo",
        "config",
        "remote",
        "shared",
        "setup",
        "status",
        "recap",
        "context",
        "waybar",
        "menubar",
    ];
    let registry: Vec<&str> = GROUPS.iter().map(|g| g.name).collect();
    for d in dispatched {
        assert!(
            registry.contains(&d),
            "group `{d}` dispatched but missing from GROUPS"
        );
    }
    for r in &registry {
        assert!(
            dispatched.contains(r),
            "group `{r}` in GROUPS but not dispatched"
        );
    }
}

#[test]
fn parses_status_set_with_message() {
    let a = parse_args(
        [
            "wsx",
            "status",
            "set",
            "blocked",
            "--message",
            "need a decision",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    )
    .unwrap();
    match a {
        CliAction::StatusSet { state, message } => {
            assert_eq!(state, "blocked");
            assert_eq!(message.as_deref(), Some("need a decision"));
        }
        other => panic!("expected StatusSet, got {other:?}"),
    }
}

#[test]
fn parses_status_set_without_message() {
    let a = parse_args(
        ["wsx", "status", "set", "working"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap();
    match a {
        CliAction::StatusSet { state, message } => {
            assert_eq!(state, "working");
            assert_eq!(message, None);
        }
        other => panic!("expected StatusSet, got {other:?}"),
    }
}

#[test]
fn parses_status_clear_and_from_hook() {
    assert!(matches!(
        parse_args(
            ["wsx", "status", "clear"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        )
        .unwrap(),
        CliAction::StatusClear
    ));
    assert!(matches!(
        parse_args(
            ["wsx", "status", "from-hook"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        )
        .unwrap(),
        CliAction::StatusFromHook { agent: None }
    ));
    match parse_args(
        ["wsx", "status", "from-hook", "--agent", "claude"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap()
    {
        CliAction::StatusFromHook { agent } => assert_eq!(agent.as_deref(), Some("claude")),
        other => panic!("expected StatusFromHook, got {other:?}"),
    }
}

#[test]
fn parse_status_from_notify_captures_agent_and_payload() {
    match parse_args(
        [
            "wsx",
            "status",
            "from-notify",
            "--agent",
            "codex",
            "{\"type\":\"agent-turn-complete\"}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
    )
    .unwrap()
    {
        CliAction::StatusFromNotify { agent, payload } => {
            assert_eq!(agent.as_deref(), Some("codex"));
            assert_eq!(
                payload.as_deref(),
                Some("{\"type\":\"agent-turn-complete\"}")
            );
        }
        other => panic!("expected StatusFromNotify, got {other:?}"),
    }
}

#[test]
fn parse_status_from_notify_with_no_args_is_all_none() {
    match parse_args(
        ["wsx", "status", "from-notify"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap()
    {
        CliAction::StatusFromNotify { agent, payload } => {
            assert!(agent.is_none());
            assert!(payload.is_none());
        }
        other => panic!("expected StatusFromNotify, got {other:?}"),
    }
}

#[test]
fn status_set_message_without_value_is_usage_error() {
    let err = parse_args(
        ["wsx", "status", "set", "working", "--message"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Usage { .. }), "got {err:?}");
}

#[test]
fn status_from_hook_agent_without_value_is_usage_error() {
    let err = parse_args(
        ["wsx", "status", "from-hook", "--agent"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Usage { .. }), "got {err:?}");
}

#[test]
fn parses_recap_set_with_all_flags() {
    let a = parse(&[
        "recap",
        "set",
        "--goal",
        "fix auth",
        "--state",
        "tests failing",
        "--next",
        "debug",
    ])
    .unwrap();
    match a {
        CliAction::RecapSet {
            goal, state, next, ..
        } => {
            assert_eq!(goal.as_deref(), Some("fix auth"));
            assert_eq!(state.as_deref(), Some("tests failing"));
            assert_eq!(next.as_deref(), Some("debug"));
        }
        other => panic!("expected RecapSet, got {other:?}"),
    }
}

#[test]
fn parses_recap_set_partial() {
    let a = parse(&["recap", "set", "--state", "tests green"]).unwrap();
    match a {
        CliAction::RecapSet {
            goal, state, next, ..
        } => {
            assert_eq!(goal, None);
            assert_eq!(state.as_deref(), Some("tests green"));
            assert_eq!(next, None);
        }
        other => panic!("expected RecapSet, got {other:?}"),
    }
}

#[test]
fn recap_set_requires_at_least_one_flag() {
    assert!(parse(&["recap", "set"]).is_err());
}

#[test]
fn recap_set_rejects_unknown_flag() {
    assert!(parse(&["recap", "set", "--bogus", "x"]).is_err());
}

#[test]
fn parses_recap_set_short_forms() {
    let a = parse(&[
        "recap",
        "set",
        "--goal-short",
        "Audit V2 invoices, CV-04964",
        "--state-short",
        "3/12 done",
        "--next-short",
        "fix drift calc",
    ])
    .unwrap();
    match a {
        CliAction::RecapSet {
            goal,
            goal_short,
            state_short,
            next_short,
            ..
        } => {
            assert_eq!(goal, None);
            assert_eq!(goal_short.as_deref(), Some("Audit V2 invoices, CV-04964"));
            assert_eq!(state_short.as_deref(), Some("3/12 done"));
            assert_eq!(next_short.as_deref(), Some("fix drift calc"));
        }
        other => panic!("expected RecapSet, got {other:?}"),
    }
}

#[test]
fn recap_set_short_flag_alone_satisfies_at_least_one() {
    assert!(parse(&["recap", "set", "--goal-short", "x"]).is_ok());
}

#[test]
fn parses_recap_show_and_clear() {
    assert!(matches!(
        parse(&["recap", "show"]).unwrap(),
        CliAction::RecapShow
    ));
    assert!(matches!(
        parse(&["recap", "clear"]).unwrap(),
        CliAction::RecapClear
    ));
}

#[test]
fn parses_context_show_and_write() {
    assert!(matches!(
        parse(&["context", "show"]).unwrap(),
        CliAction::ContextShow
    ));
    assert!(matches!(
        parse(&["context", "write"]).unwrap(),
        CliAction::ContextWrite
    ));
}

#[test]
fn context_rejects_missing_unknown_and_trailing_args() {
    assert!(parse(&["context"]).is_err());
    assert!(parse(&["context", "bogus"]).is_err());
    assert!(parse(&["context", "show", "extra"]).is_err());
    assert!(parse(&["context", "write", "--workspace", "r/s"]).is_err());
}

#[test]
fn parses_waybar_commands() {
    assert!(matches!(
        parse(&["waybar", "status"]),
        Ok(CliAction::WaybarStatus)
    ));
    assert!(matches!(
        parse(&["waybar", "menu"]),
        Ok(CliAction::WaybarMenu)
    ));
    match parse(&["waybar", "jump", "meals backend", "api-fix"]) {
        Ok(CliAction::WaybarJump { repo, slug }) => {
            assert_eq!(repo, "meals backend");
            assert_eq!(slug, "api-fix");
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(parse(&["waybar", "jump", "onlyrepo"]).is_err());
    assert!(parse(&["waybar", "bogus"]).is_err());
    assert!(parse(&["waybar"]).is_err());
}

#[test]
fn parses_waybar_menu_entries_and_refresh_prs() {
    assert!(matches!(
        parse(&["waybar", "menu-entries"]),
        Ok(CliAction::WaybarMenuEntries)
    ));
    assert!(matches!(
        parse(&["waybar", "menu-entries", "--json"]),
        Ok(CliAction::WaybarMenuEntries)
    ));
    assert!(matches!(
        parse(&["waybar", "refresh-prs"]),
        Ok(CliAction::WaybarRefreshPrs)
    ));
}

#[test]
fn parses_setup_waybar() {
    assert!(matches!(
        parse(&["setup", "waybar"]),
        Ok(CliAction::SetupWaybar)
    ));
}

#[test]
fn waybar_group_help_renders() {
    let h = render_group_help("waybar");
    assert!(h.contains("wsx waybar —"));
    assert!(h.contains("status"));
    assert!(h.contains("jump <repo> <slug>"));
}

#[test]
fn parses_menubar_commands() {
    assert!(matches!(
        parse(&["menubar", "plugin"]),
        Ok(CliAction::MenubarPlugin)
    ));
    match parse(&["menubar", "jump", "meals backend", "api-fix"]) {
        Ok(CliAction::MenubarJump { repo, slug }) => {
            assert_eq!(repo, "meals backend");
            assert_eq!(slug, "api-fix");
        }
        other => panic!("{other:?}"),
    }
    match parse(&["menubar", "copy-path", "r", "s"]) {
        Ok(CliAction::MenubarCopyPath { repo, slug }) => {
            assert_eq!(repo, "r");
            assert_eq!(slug, "s");
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        parse(&["menubar", "refresh"]),
        Ok(CliAction::MenubarRefresh)
    ));
    assert!(parse(&["menubar", "jump", "onlyrepo"]).is_err());
    assert!(parse(&["menubar", "bogus"]).is_err());
    assert!(parse(&["menubar"]).is_err());
}

#[test]
fn parses_setup_menubar() {
    assert!(matches!(
        parse(&["setup", "menubar"]),
        Ok(CliAction::SetupMenubar)
    ));
}

#[test]
fn menubar_group_help_renders() {
    let h = render_group_help("menubar");
    assert!(h.contains("wsx menubar —"));
    assert!(h.contains("plugin"));
    assert!(h.contains("copy-path"));
}
