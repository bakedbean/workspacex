//! [`CliAction`] -> effects.
//!
//! The dispatch stays one exhaustive `match` on purpose. Splitting it per
//! group would need a catch-all arm in each group function, which throws
//! away the compiler's guarantee that every `CliAction` is handled — the
//! one check that actually keeps this file honest as commands are added.
//! Arm bodies are short (median ~10 lines); the long ones delegate.

use super::action::{CliAction, HelpTopic};
use super::help::{render_group_help, render_root_help};
use super::resolve::*;
use crate::config::Dirs;
use crate::error::{Error, Result};

pub async fn run_cli(action: CliAction, dirs: &Dirs) -> Result<()> {
    // Actions that don't need the wsx store run before we open it, so a
    // pure `wsx setup install-skill` on a fresh machine doesn't create
    // `~/.local/state/wsx/state.db` as a side effect.
    match &action {
        CliAction::Help(topic) => {
            match topic {
                HelpTopic::Root => print!("{}", render_root_help()),
                HelpTopic::Group(g) => print!("{}", render_group_help(g)),
            }
            return Ok(());
        }
        CliAction::Version => {
            println!("wsx {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }
    if matches!(action, CliAction::SetupInstallSkill) {
        let targets = crate::agent::skill::default_install_targets().ok_or_else(|| {
            Error::UserInput("could not resolve home directory for skill install".into())
        })?;
        for target in targets {
            let outcome = crate::agent::skill::install_to(&target)?;
            let path = target.path.display();
            let skill = target.skill;
            match outcome {
                crate::agent::skill::InstallOutcome::Created => {
                    println!("installed {skill} skill for {} to {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Updated => {
                    println!("updated {skill} skill for {} at {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Unchanged => {
                    println!(
                        "{skill} skill for {} already up to date at {path}",
                        target.agent
                    );
                }
            }
        }
        return Ok(());
    }
    if matches!(action, CliAction::WaybarStatus) {
        #[cfg(target_os = "linux")]
        {
            crate::desktop::waybar::status::print_status(&dirs.db_path());
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        return Err(waybar_linux_only());
    }
    if matches!(action, CliAction::SetupWaybar) {
        #[cfg(target_os = "linux")]
        {
            for line in crate::desktop::waybar::install::run()? {
                println!("{line}");
            }
            return Ok(());
        }
        #[cfg(not(target_os = "linux"))]
        return Err(waybar_linux_only());
    }
    if matches!(action, CliAction::MenubarPlugin) {
        #[cfg(target_os = "macos")]
        {
            crate::desktop::menubar::plugin::print_plugin(&dirs.db_path());
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        return Err(menubar_macos_only());
    }
    if matches!(action, CliAction::SetupMenubar) {
        #[cfg(target_os = "macos")]
        {
            for line in crate::desktop::menubar::install::run()? {
                println!("{line}");
            }
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        return Err(menubar_macos_only());
    }
    let store = crate::data::store::Store::open(&dirs.db_path())?;
    match action {
        CliAction::Tui { .. } => unreachable!("handled in main"),
        CliAction::RepoAdd {
            path,
            name,
            branch_prefix,
        } => {
            crate::data::repo::add(&store, &path, &name, &branch_prefix).await?;
            println!("added repo: {name}");
        }
        CliAction::RepoList => {
            for r in crate::data::repo::list(&store)? {
                println!("{:<20} {}", r.name, r.path.display());
            }
        }
        CliAction::RepoRemove { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            crate::data::repo::remove(&store, r.id)?;
            println!("removed repo: {name}");
        }
        CliAction::RepoSetPrefix { name, prefix } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            store.set_repo_branch_prefix(r.id, &prefix)?;
            if prefix.is_empty() {
                println!("cleared branch prefix for {name} (using global default)");
            } else {
                println!("set branch prefix for {name} to {prefix}");
            }
        }
        CliAction::RepoSetBaseBranch { name, value } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let trimmed = value.trim();
            if trimmed.is_empty() {
                store.set_repo_base_branch(r.id, None)?;
                println!("cleared base branch for {name} (using current HEAD)");
            } else {
                store.set_repo_base_branch(r.id, Some(trimmed))?;
                println!("set base branch for {name} to {trimmed}");
            }
        }
        CliAction::RepoSetInstructions { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_custom_instructions(r.id, None)?;
                println!("cleared custom instructions for {name}");
            } else {
                store.set_repo_custom_instructions(r.id, Some(&value))?;
                println!("set custom instructions for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoSetSetup { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_setup_script(r.id, None)?;
                println!("cleared setup for {name}");
            } else {
                store.set_repo_setup_script(r.id, Some(&value))?;
                println!("set setup for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoSetArchive { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_archive_script(r.id, None)?;
                println!("cleared archive for {name}");
            } else {
                store.set_repo_archive_script(r.id, Some(&value))?;
                println!("set archive for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoEditSetup { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.setup_script.clone().unwrap_or_default();
            let new_value = open_in_editor("setup", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_setup_script(r.id, None)?;
                println!("cleared setup for {name}");
            } else if new_value == current {
                println!("setup unchanged");
            } else {
                store.set_repo_setup_script(r.id, Some(&new_value))?;
                println!("set setup for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::RepoEditArchive { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.archive_script.clone().unwrap_or_default();
            let new_value = open_in_editor("archive", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_archive_script(r.id, None)?;
                println!("cleared archive for {name}");
            } else if new_value == current {
                println!("archive unchanged");
            } else {
                store.set_repo_archive_script(r.id, Some(&new_value))?;
                println!("set archive for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::RepoSetPinnedCommands { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_pinned_commands(r.id, None)?;
                println!("cleared pinned commands for {name}");
            } else {
                store.set_repo_pinned_commands(r.id, Some(&value))?;
                println!("set pinned commands for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoEditPinnedCommands { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.pinned_commands.clone().unwrap_or_default();
            let new_value = open_in_editor("pinned-commands", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_pinned_commands(r.id, None)?;
                println!("cleared pinned commands for {name}");
            } else if new_value == current {
                println!("pinned commands unchanged");
            } else {
                store.set_repo_pinned_commands(r.id, Some(&new_value))?;
                println!("set pinned commands for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::RepoSetName { name, new_name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let trimmed = new_name.trim();
            store.set_repo_name(r.id, trimmed)?;
            println!("renamed repo {name} to {trimmed}");
        }
        CliAction::RepoSetPath { name, path } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let path = crate::data::repo::set_path(&store, r.id, &path).await?;
            println!("set path for {name} to {}", path.display());
        }
        CliAction::RepoSetRelatedRepos { name, source } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let value = source.resolve()?;
            if value.trim().is_empty() {
                store.set_repo_related_repos(r.id, None)?;
                println!("cleared related repos for {name}");
            } else {
                store.set_repo_related_repos(r.id, Some(&value))?;
                println!("set related repos for {name} ({} chars)", value.len());
            }
        }
        CliAction::RepoEditRelatedRepos { name } => {
            let repos = crate::data::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            let current = r.related_repos.clone().unwrap_or_default();
            let new_value = open_in_editor("related-repos", &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.trim().is_empty() {
                store.set_repo_related_repos(r.id, None)?;
                println!("cleared related repos for {name}");
            } else if new_value == current {
                println!("related repos unchanged");
            } else {
                store.set_repo_related_repos(r.id, Some(&new_value))?;
                println!("set related repos for {name} ({} chars)", new_value.len());
            }
        }
        CliAction::ConfigGet { key } => match store.get_setting(&key)? {
            Some(v) => println!("{v}"),
            None => println!("(unset)"),
        },
        CliAction::ConfigSet { key, source } => {
            let value = source.resolve()?;
            if value.is_empty() {
                store.delete_setting(&key)?;
                println!("cleared {key}");
            } else {
                let value = if key == "detail_bar_config" {
                    detail_bar_config_validate_and_normalize(&value)?
                } else if key == "usage_graph_window" {
                    usage_window_validate_and_normalize(&value)?
                } else if key.starts_with("notification_bell_") {
                    bell_pattern_validate_and_normalize(&value)?
                } else {
                    value
                };
                store.set_setting(&key, &value)?;
                println!("set {key} ({} chars)", value.len());
            }
        }
        CliAction::ConfigList => {
            let settings = store.list_settings()?;
            if settings.is_empty() {
                println!("(no settings)");
                return Ok(());
            }
            for (k, v) in settings {
                let preview = if v.len() > 60 {
                    format!("{}…", &v[..57])
                } else {
                    v.clone()
                };
                println!("{:<20} {}", k, preview);
            }
        }
        CliAction::ConfigEdit { key } => {
            let current = store.get_setting(&key)?.unwrap_or_default();
            let seed = if key == "detail_bar_config" && current.is_empty() {
                detail_bar_config_seed_for_empty()
            } else {
                current.clone()
            };
            let new_value = open_in_editor(&key, &seed)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.is_empty() {
                store.delete_setting(&key)?;
                println!("cleared {key}");
            } else if new_value == current {
                println!("{key} unchanged");
            } else {
                let normalized = if key == "detail_bar_config" {
                    detail_bar_config_validate_and_normalize(&new_value)?
                } else if key == "usage_graph_window" {
                    usage_window_validate_and_normalize(&new_value)?
                } else if key.starts_with("notification_bell_") {
                    bell_pattern_validate_and_normalize(&new_value)?
                } else {
                    new_value.clone()
                };
                store.set_setting(&key, &normalized)?;
                println!("set {key} ({} chars)", normalized.len());
            }
        }
        CliAction::ThemePath => {
            println!("{}", dirs.theme_path().display());
        }
        CliAction::ThemeInit => {
            let path = dirs.theme_path();
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // `create_new` so the exists-check and the write are one atomic
            // step: a check-then-write races another `theme init` (or the
            // user's editor) and can clobber a file written in between.
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        Error::UserInput(format!(
                            "{} already exists; edit it in place or delete it to re-init",
                            path.display()
                        ))
                    } else {
                        Error::from(e)
                    }
                })?;
            use std::io::Write as _;
            file.write_all(crate::config::theme_file::DEFAULT_TOML.as_bytes())?;
            println!("wrote {}", path.display());
        }
        CliAction::ThemeCheck { path } => {
            // An explicit path that doesn't exist is a mistake (typo, wrong
            // directory), not "no theme file": only the no-argument default
            // path is allowed to be absent and still succeed.
            if let Some(p) = path.as_deref()
                && !p.exists()
            {
                return Err(Error::UserInput(format!("no such file: {}", p.display())));
            }
            let path = path.unwrap_or_else(|| dirs.theme_path());
            let theme_name = store.get_setting("theme")?.unwrap_or_default();
            let theme = crate::ui::theme::Theme::by_name(&theme_name);
            match crate::config::theme_file::load(&path, &theme) {
                Ok(_) => {
                    if path.exists() {
                        println!("ok: {}", path.display());
                    } else {
                        println!(
                            "ok: no file at {}; the bundled default applies",
                            path.display()
                        );
                    }
                    if !crate::app::theme_reload::bar_theme_enabled(&store) {
                        println!(
                            "note: bar_theme is off, so wsx draws the stock bars; enable with `wsx config set bar_theme on`"
                        );
                    }
                }
                Err(errors) => {
                    for e in &errors {
                        eprintln!("{}: {e}", path.display());
                    }
                    return Err(Error::UserInput(format!(
                        "{} error(s) in {}",
                        errors.len(),
                        path.display()
                    )));
                }
            }
        }
        CliAction::RemoteList => {
            let remotes = crate::commands::remotes::list(&store)?;
            if remotes.is_empty() {
                println!("no remotes configured. add one with: wsx config edit remotes");
                return Ok(());
            }
            for r in remotes {
                println!("{}", r.name);
            }
        }
        CliAction::RemoteRun { name } => {
            let command = crate::commands::remotes::lookup(&store, &name)?.ok_or_else(|| {
                let available = crate::commands::remotes::list(&store)
                    .ok()
                    .map(|v| v.into_iter().map(|r| r.name).collect::<Vec<_>>().join(", "))
                    .unwrap_or_default();
                if available.is_empty() {
                    Error::UserInput(format!(
                        "no remote named '{name}'. no remotes configured \
                         (add one with: wsx config edit remotes)"
                    ))
                } else {
                    Error::UserInput(format!("no remote named '{name}'. available: {available}"))
                }
            })?;
            use std::os::unix::process::CommandExt;
            let err = std::process::Command::new("sh")
                .arg("-c")
                .arg(&command)
                .exec();
            // exec only returns on failure.
            return Err(Error::UserInput(format!("exec sh: {err}")));
        }
        CliAction::SharedList { json } => {
            let mut records = crate::commands::shared::shared_list_records(
                &store,
                crate::pty::tmux::has_session,
            )?;
            // Colorable PR status is only useful to a remote picker consuming
            // `--json`; the human table below doesn't render it, so skip the
            // per-workspace `gh` calls for the plain path.
            if json {
                crate::commands::shared::enrich_with_pr_status(&mut records).await;
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else if records.is_empty() {
                println!("no shared workspaces");
            } else {
                for rec in &records {
                    if rec.agents.is_empty() {
                        println!("{}\t{}\t(no agents)\t-", rec.repo, rec.workspace);
                        continue;
                    }
                    for agent in &rec.agents {
                        let session = agent.tmux_session.as_deref().unwrap_or("-");
                        let alive = match (agent.alive, &agent.tmux_session) {
                            (true, _) => "alive",
                            (false, Some(_)) => "(dead)",
                            (false, None) => "-",
                        };
                        println!("{}\t{}\t{}\t{}", rec.repo, rec.workspace, session, alive);
                    }
                }
            }
        }
        CliAction::WorkspaceCreate {
            repo,
            name,
            yolo,
            shared,
            agent,
            prompt,
        } => {
            let r = lookup_repo(&store, &repo)?;
            let worktree_base = dirs.app_dir().join("worktrees");
            std::fs::create_dir_all(&worktree_base)?;
            // Inherit yolo + agent kind from the workspace this command runs
            // inside (agent handoffs, or a human in a worktree shell); creates
            // from outside any workspace behave as before.
            let parent = resolve_current_workspace(&store).ok();
            let default_agent = crate::pty::session::AgentKind::from_store(&store);
            let (effective_yolo, agent_kind) =
                effective_create_flags(yolo, agent.as_deref(), parent.as_ref(), default_agent);
            let created = crate::data::workspace::create(
                &store,
                &r,
                name.as_deref(),
                &worktree_base,
                effective_yolo,
                shared,
                agent_kind,
                &dirs.log_dir(),
                tokio_util::sync::CancellationToken::new(),
                |_| {},
            )
            .await?;
            println!(
                "created workspace {}/{} at {}",
                r.name,
                created.workspace.name,
                created.workspace.worktree_path.display()
            );
            if let Some(p) = &parent {
                let mut inherited: Vec<String> = Vec::new();
                if effective_yolo && !yolo {
                    inherited.push("yolo".to_string());
                }
                if agent.is_none() && p.agent != default_agent {
                    inherited.push(format!("agent={}", p.agent.display_name()));
                }
                if !inherited.is_empty() {
                    let parent_repo = crate::data::repo::list(&store)?
                        .into_iter()
                        .find(|pr| pr.id == p.repo_id)
                        .map(|pr| pr.name)
                        .unwrap_or_else(|| "(unknown repo)".to_string());
                    println!(
                        "inherited {} from {}/{}",
                        inherited.join(", "),
                        parent_repo,
                        p.name
                    );
                }
            }
            if let crate::data::setup::SetupResult::Failed { exit_code } = created.setup_result {
                println!("warning: setup script exited with code {exit_code}");
                // Only when one was really written: logging is best-effort, so
                // an unwritable log directory must not be reported as a file to
                // go and read.
                if let Some(log) = &created.setup_log {
                    println!("setup log: {}", log.display());
                }
            }
            // Seed the agent LAST: `create` above already awaited the setup
            // script, and the dashboard skips workspaces whose setup hasn't
            // finished, so queueing here can't land on a workspace that isn't
            // ready to spawn.
            if let Some(prompt) = prompt.as_deref() {
                let ws_id = created.workspace.id;
                // `create` seeds a primary agent row at birth, so this
                // resolves immediately — but report rather than unwrap, since
                // the workspace itself already exists on disk either way.
                // Every failure from here on must reach the recovery arm
                // below: the worktree already exists, so propagating with `?`
                // would abort with a bare error and no way to resend.
                let seeded = store
                    .primary_instance_id(ws_id)
                    .and_then(|found| {
                        found.ok_or_else(|| {
                            Error::UserInput("new workspace has no primary agent".to_string())
                        })
                    })
                    .and_then(|target| enqueue_for_agent(&store, ws_id, target, prompt));
                match seeded {
                    Ok(id) => println!("queued starter prompt #{id} to primary"),
                    // The worktree is live and the prompt is not. Hand back a
                    // command that actually resends THIS prompt, rather than
                    // leaving a workspace that looks created but never wakes.
                    Err(e) => {
                        eprintln!(
                            "warning: workspace created but the starter prompt was not queued: {e}\n\
                             retry with: {}",
                            retry_send_hint(&r.name, &created.workspace.name, prompt)
                        );
                    }
                }
            }
        }
        CliAction::WorkspaceList { repo, json } => {
            let filtered = match repo {
                Some(name) => vec![lookup_repo(&store, &name)?],
                None => crate::data::repo::list(&store)?,
            };
            if json {
                let records = crate::commands::inspect::workspace_records(&store, &filtered)?;
                println!("{}", serde_json::to_string_pretty(&records)?);
                return Ok(());
            }
            for r in filtered {
                for w in store.workspaces(r.id)? {
                    println!(
                        "{}\t{}\t{}\t{}",
                        r.name,
                        w.name,
                        w.branch,
                        w.worktree_path.display()
                    );
                }
            }
        }
        CliAction::WorkspacePath { repo, name } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            println!("{}", w.worktree_path.display());
        }
        CliAction::WorkspaceRename {
            repo,
            name,
            new_name,
        } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            if new_name == name {
                println!("workspace {}/{} unchanged", r.name, name);
            } else {
                crate::data::workspace::rename(
                    &store,
                    &r,
                    &w,
                    &new_name,
                    &crate::config::Dirs::discover().log_dir(),
                )
                .await?;
                println!(
                    "renamed workspace {}/{} to {}/{}",
                    r.name, name, r.name, new_name
                );
            }
        }
        CliAction::WorkspaceArchive {
            repo,
            name,
            keep_worktree,
            force_delete_branch,
        } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            let opts = crate::data::workspace::ArchiveOpts {
                keep_worktree,
                force_branch_delete: force_delete_branch,
            };
            crate::data::workspace::archive(&store, &r, &w, opts, |_| {}).await?;
            println!("archived workspace {}/{}", r.name, name);
        }
        CliAction::WorkspaceShare { repo, name, shared } => {
            let r = lookup_repo(&store, &repo)?;
            let w = lookup_workspace(&store, &r, &name)?;
            if w.shared == shared {
                println!(
                    "workspace {}/{} already {}",
                    r.name,
                    name,
                    if shared { "shared" } else { "unshared" }
                );
            } else {
                store.set_workspace_shared(w.id, shared)?;
                println!(
                    "workspace {}/{} is now {}",
                    r.name,
                    name,
                    if shared { "shared" } else { "unshared" }
                );
                println!("note: running sessions keep their current backend until restarted");
            }
        }
        CliAction::AgentList { workspace, json } => {
            let ws = target_workspace(&store, workspace.as_deref())?;
            let agents = crate::commands::inspect::agents(&store, ws.id)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&agents)?);
                return Ok(());
            }
            print!(
                "{}",
                crate::commands::inspect::render_agents(&agents, crate::data::store::now_ms())
            );
        }
        CliAction::AgentSend {
            target,
            body,
            workspace,
        } => {
            let (target_ws, target_id) =
                super::mail::resolve_send_target(&store, &target, workspace.as_deref())?;
            let body = super::mail::read_body(&body)?;
            let id = enqueue_for_agent(&store, target_ws.id, target_id, &body)?;
            // Name the recipient as the sender would address it: the bare
            // label at home, qualified when it lives in another workspace.
            let home = resolve_current_workspace(&store).ok().map(|w| w.id);
            let shown = super::mail::party(&store, Some(target_id), home.unwrap_or(target_ws.id));
            println!("{}", super::mail::queued_line(id, &shown, &body));
        }
        CliAction::AgentWhoami => {
            let me = super::mail::current_agent(&store)?;
            let ws = crate::app::messaging::workspace_ref(&store, me.workspace_id)
                .unwrap_or_else(|| format!("(workspace {})", me.workspace_id.0));
            println!("label: {}", me.label());
            println!("instance: {}", me.id.0);
            println!("workspace: {ws}");
            println!("primary: {}", if me.is_primary { "yes" } else { "no" });
        }
        CliAction::AgentMessages {
            view,
            undelivered,
            limit,
            id,
            json,
        } => {
            use crate::cli::action::MessagesView;
            use crate::data::messages::MessageScope;
            // Labels are shown relative to the caller's workspace; for a
            // plain shell (no agent identity) that is the cwd's workspace.
            let me = super::mail::current_agent(&store);
            let home = |store: &crate::data::store::Store| match &me {
                Ok(inst) => Ok(inst.workspace_id),
                Err(_) => resolve_current_workspace(store).map(|w| w.id),
            };
            if let Some(id) = id {
                let m = store
                    .message_by_id(id)?
                    .ok_or_else(|| Error::UserInput(format!("no message #{id}")))?;
                let viewer = home(&store).unwrap_or(m.workspace_id);
                if json {
                    let rec = super::mail::message_record(&store, &m, viewer);
                    println!("{}", serde_json::to_string_pretty(&rec)?);
                } else {
                    println!("{}", super::mail::full_message(&store, &m, viewer));
                }
                return Ok(());
            }
            let (scope, viewer) = match view {
                MessagesView::Workspace => {
                    let ws = home(&store)?;
                    (MessageScope::Workspace(ws), ws)
                }
                MessagesView::Inbox | MessagesView::Sent => {
                    let inst = me.as_ref().map_err(|e| {
                        Error::UserInput(format!(
                            "{e}; use `wsx agent messages --all` for the whole workspace"
                        ))
                    })?;
                    let scope = if view == MessagesView::Inbox {
                        MessageScope::To(inst.id)
                    } else {
                        MessageScope::From(inst.id)
                    };
                    (scope, inst.workspace_id)
                }
            };
            let messages = store.list_messages(scope, undelivered, limit)?;
            if json {
                let recs: Vec<_> = messages
                    .iter()
                    .map(|m| super::mail::message_record(&store, m, viewer))
                    .collect();
                println!("{}", serde_json::to_string_pretty(&recs)?);
                return Ok(());
            }
            println!("{}", super::mail::LISTING_HEADER);
            for m in &messages {
                println!("{}", super::mail::listing_row(&store, m, viewer));
            }
        }
        CliAction::AgentReply { to, body } => {
            use crate::data::messages::MessageScope;
            let me = super::mail::current_agent(&store)?;
            let original = match to {
                Some(id) => store
                    .message_by_id(id)?
                    .ok_or_else(|| Error::UserInput(format!("no message #{id}")))?,
                None => store
                    .list_messages(MessageScope::To(me.id), false, 1)?
                    .pop()
                    .ok_or_else(|| {
                        Error::UserInput("you have not received any messages to reply to".into())
                    })?,
            };
            if original.target_agent_id != me.id {
                return Err(Error::UserInput(format!(
                    "message #{} was sent to {}, not to you ({}); reply only answers your own mail",
                    original.id,
                    super::mail::party(&store, Some(original.target_agent_id), me.workspace_id),
                    me.label()
                )));
            }
            let sender_id = original.from_agent_id.ok_or_else(|| {
                Error::UserInput(format!(
                    "message #{} came from a shell or editor agent, not a wsx agent; \
                     there is no one to reply to",
                    original.id
                ))
            })?;
            let sender = store.workspace_agents_by_id(sender_id)?.ok_or_else(|| {
                Error::UserInput(format!(
                    "the sender of message #{} (instance {}) has since been removed",
                    original.id, sender_id.0
                ))
            })?;
            let body = super::mail::read_body(&body)?;
            let id = enqueue_for_agent(&store, sender.workspace_id, sender.id, &body)?;
            let shown = super::mail::party(&store, Some(sender.id), me.workspace_id);
            println!(
                "{} in reply to #{}",
                super::mail::queued_line(id, &shown, &body),
                original.id
            );
        }
        CliAction::AgentWait {
            from,
            after,
            timeout_secs,
        } => {
            let me = super::mail::current_agent(&store)?;
            let from_id = from
                .as_deref()
                .map(|f| super::mail::resolve_sender(&store, me.workspace_id, f))
                .transpose()?;
            let hit =
                super::mail::wait_for_message(&store, me.id, from_id, after, timeout_secs).await?;
            let Some(m) = hit else {
                let who = from
                    .as_deref()
                    .map(|f| format!(" from {f}"))
                    .unwrap_or_default();
                // The retry command keeps the caller's filters so a copy-paste
                // does not silently widen the wait.
                let mut retry = String::from("wsx agent wait");
                if let Some(f) = from.as_deref() {
                    retry.push_str(&format!(" --from {}", super::resolve::shell_quote(f)));
                }
                if let Some(a) = after {
                    retry.push_str(&format!(" --after {a}"));
                }
                return Err(Error::UserInput(format!(
                    "no message{who} after {timeout_secs}s; run `{retry}` again \
                     (add --timeout 0 to wait indefinitely)"
                )));
            };
            println!("{}", super::mail::full_message(&store, &m, me.workspace_id));
            if m.delivered_at.is_none() {
                // `wait` is a read: the dashboard still injects the message.
                eprintln!(
                    "note: wsx will also inject message #{} into your session; \
                     that copy is the same message, already shown here",
                    m.id
                );
            }
            eprintln!(
                "note: `wait` does not consume messages; to wait for the next one, \
                 run `wsx agent wait --after {}`",
                m.id
            );
        }
        CliAction::AgentAdd { kind } => {
            let ws = resolve_current_workspace(&store)?;
            let agent = crate::pty::session::AgentKind::from_str_or_default(Some(&kind));
            let inst = store.add_workspace_agent(ws.id, agent)?;
            println!("added {}", inst.label());
        }
        CliAction::StatusSet { state, message } => {
            let parsed = crate::data::store::ReportedState::parse(&state).ok_or_else(|| {
                Error::UserInput(format!(
                    "invalid status '{state}'; expected working|waiting|blocked|done"
                ))
            })?;
            let ws = resolve_current_workspace(&store)?;
            let agent = resolve_env_instance(&store, ws.id).map(|i| i.id);
            store.set_agent_status(ws.id, agent, parsed, message.as_deref(), "model")?;
            println!("status: {}", parsed.as_str());
        }
        CliAction::StatusClear => {
            let ws = resolve_current_workspace(&store)?;
            // An agent clears only its own row; a human's shell clears them all.
            match resolve_env_instance(&store, ws.id) {
                Some(inst) => store.clear_agent_status(ws.id, inst.id)?,
                None => store.clear_workspace_status(ws.id)?,
            }
            println!("status cleared");
        }
        CliAction::StatusShow { workspace, json } => {
            let ws = target_workspace(&store, workspace.as_deref())?;
            let view = crate::commands::inspect::status_view(&store, &ws)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&view)?);
                return Ok(());
            }
            print!(
                "{}",
                crate::commands::inspect::render_status(&view, crate::data::store::now_ms())
            );
        }
        CliAction::StatusFromHook { agent } => {
            use std::io::Read;
            let mut buf = String::new();
            // Hooks pipe JSON on stdin; tolerate empty/garbage by no-op exit 0
            // so a hook never fails the agent's turn.
            let _ = std::io::stdin().read_to_string(&mut buf);
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&buf) {
                if let Ok(ws) = resolve_current_workspace(&store) {
                    let kind = match &agent {
                        Some(a) => crate::pty::session::AgentKind::from_str_or_default(Some(a)),
                        None => ws.agent,
                    };
                    let integration = crate::agent::status::for_agent(kind);
                    if let Some(state) = integration.parse_event(&json) {
                        let inst = resolve_current_instance(&store, ws.id, kind);
                        let _ = store.apply_hook_status(ws.id, inst, state, "hook");
                    }
                    // Remember which harness session this instance is in, so a
                    // respawn resumes that conversation rather than whichever
                    // one in the worktree was most recent (see
                    // `app::spawn::recorded_resume_id`). Attributed by the
                    // instance id the hook inherited from its agent's env.
                    if let Some(sid) = integration.session_id_from_event(&json) {
                        if let Some(inst) = resolve_current_instance(&store, ws.id, kind) {
                            let _ = store.set_instance_agent_session(inst, &sid);
                        }
                    }
                }
            }
            // Always succeed: a status hook must never block or fail the turn.
            // Nothing is printed: a `SessionStart` hook's stdout would be
            // injected into the agent's conversation as context.
        }
        CliAction::StatusFromNotify { agent, payload } => {
            // Codex `notify` passes JSON as the final argv (not stdin). Tolerate
            // missing/garbage payloads by no-op exit 0 — notify must never fail
            // a turn.
            if let Some(payload) = payload {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload) {
                    if let Ok(ws) = resolve_current_workspace(&store) {
                        let kind = match &agent {
                            Some(a) => crate::pty::session::AgentKind::from_str_or_default(Some(a)),
                            None => ws.agent,
                        };
                        let integration = crate::agent::status::for_agent(kind);
                        if let Some(state) = integration.parse_event(&json) {
                            let inst = resolve_current_instance(&store, ws.id, kind);
                            let _ = store.apply_hook_status(ws.id, inst, state, "notify");
                        }
                        // Same per-instance session capture as `from-hook`
                        // (Codex: the thread id, for `codex resume <id>`).
                        if let Some(sid) = integration.session_id_from_event(&json) {
                            if let Some(inst) = resolve_current_instance(&store, ws.id, kind) {
                                let _ = store.set_instance_agent_session(inst, &sid);
                            }
                        }
                    }
                }
            }
            // Always succeed.
        }
        CliAction::RecapSet {
            goal,
            state,
            next,
            goal_short,
            state_short,
            next_short,
        } => {
            let ws = resolve_current_workspace(&store)?;
            store.set_workspace_recap(
                ws.id,
                goal.as_deref(),
                state.as_deref(),
                next.as_deref(),
                goal_short.as_deref(),
                state_short.as_deref(),
                next_short.as_deref(),
            )?;
            println!("recap updated");
        }
        CliAction::RecapShow { workspace, json } => {
            let ws = target_workspace(&store, workspace.as_deref())?;
            let view = crate::commands::inspect::recap_view(&store, &ws)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&view)?);
                return Ok(());
            }
            print!("{}", crate::commands::inspect::render_recap(&view));
        }
        CliAction::RecapClear => {
            let ws = resolve_current_workspace(&store)?;
            store.clear_workspace_recap(ws.id)?;
            println!("recap cleared");
        }
        CliAction::ContextShow { workspace } => {
            let ws = target_workspace(&store, workspace.as_deref())?;
            let digest = crate::commands::context::gather(&store, &ws).await?;
            print!("{}", crate::commands::context::render(&digest));
        }
        CliAction::ContextWrite => {
            let ws = resolve_current_workspace(&store)?;
            let digest = crate::commands::context::gather(&store, &ws).await?;
            let path = crate::commands::context::digest_path(
                dirs,
                &digest.repo_name,
                &digest.workspace_name,
            );
            crate::commands::context::write_atomic(
                &path,
                &crate::commands::context::render(&digest),
            )?;
            println!("{}", path.display());
        }
        #[cfg(target_os = "linux")]
        CliAction::WaybarMenu => crate::desktop::waybar::menu::run_menu(&store)?,
        #[cfg(target_os = "linux")]
        CliAction::WaybarJump { repo, slug } => crate::desktop::waybar::jump::jump(&repo, &slug)?,
        #[cfg(target_os = "linux")]
        CliAction::WaybarMenuEntries => {
            crate::desktop::waybar::entries::run_menu_entries(&store).await?
        }
        #[cfg(target_os = "linux")]
        CliAction::WaybarRefreshPrs => {
            crate::desktop::waybar::entries::run_refresh_prs(&store).await?
        }
        #[cfg(not(target_os = "linux"))]
        CliAction::WaybarMenu
        | CliAction::WaybarJump { .. }
        | CliAction::WaybarMenuEntries
        | CliAction::WaybarRefreshPrs => return Err(waybar_linux_only()),
        #[cfg(target_os = "macos")]
        CliAction::MenubarJump { repo, slug } => {
            let terminal_cmd = store.get_setting("terminal_cmd")?;
            crate::desktop::menubar::jump::jump(&repo, &slug, terminal_cmd.as_deref())?
        }
        #[cfg(target_os = "macos")]
        CliAction::MenubarCopyPath { repo, slug } => {
            crate::desktop::menubar::jump::copy_path(&store, &repo, &slug)?
        }
        #[cfg(target_os = "macos")]
        CliAction::MenubarRefresh => crate::desktop::menubar::refresh::run_refresh(&store).await?,
        #[cfg(not(target_os = "macos"))]
        CliAction::MenubarJump { .. }
        | CliAction::MenubarCopyPath { .. }
        | CliAction::MenubarRefresh => {
            return Err(menubar_macos_only());
        }
        CliAction::SetupInstallSkill
        | CliAction::WaybarStatus
        | CliAction::SetupWaybar
        | CliAction::MenubarPlugin
        | CliAction::SetupMenubar => {
            unreachable!("handled before store open")
        }
        CliAction::Help(_) | CliAction::Version => {
            unreachable!("handled before store open")
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn waybar_linux_only() -> Error {
    Error::UserInput("wsx waybar is only available on Linux (waybar integration)".into())
}

#[cfg(not(target_os = "macos"))]
fn menubar_macos_only() -> Error {
    Error::UserInput("wsx menubar is only available on macOS (SwiftBar integration)".into())
}
