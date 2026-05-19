use crate::config::Dirs;
use crate::error::{Error, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub enum CliAction {
    Tui,
    RepoAdd {
        path: PathBuf,
        name: String,
        branch_prefix: String,
    },
    RepoList,
    RepoRemove {
        name: String,
    },
    RepoSetPrefix {
        name: String,
        prefix: String,
    },
    RepoSetInstructions {
        name: String,
        source: ValueSource,
    },
    RepoSetSetup {
        name: String,
        source: ValueSource,
    },
    RepoSetArchive {
        name: String,
        source: ValueSource,
    },
    RepoEditSetup {
        name: String,
    },
    RepoEditArchive {
        name: String,
    },
    RepoSetPinnedCommands {
        name: String,
        source: ValueSource,
    },
    RepoEditPinnedCommands {
        name: String,
    },
    RepoSetRelatedRepos {
        name: String,
        source: ValueSource,
    },
    RepoEditRelatedRepos {
        name: String,
    },
    ConfigGet {
        key: String,
    },
    ConfigSet {
        key: String,
        source: ValueSource,
    },
    ConfigList,
    ConfigEdit {
        key: String,
    },
    RemoteList,
    RemoteRun {
        name: String,
    },
}

#[derive(Debug)]
pub enum ValueSource {
    Literal(String),
    File(PathBuf),
}

impl ValueSource {
    pub fn from_arg(value: String) -> Self {
        if let Some(path) = value.strip_prefix('@') {
            ValueSource::File(PathBuf::from(path))
        } else {
            ValueSource::Literal(value)
        }
    }

    pub fn resolve(self) -> Result<String> {
        match self {
            ValueSource::Literal(s) => Ok(s),
            ValueSource::File(p) => std::fs::read_to_string(&p)
                .map_err(|e| Error::UserInput(format!("read {}: {e}", p.display()))),
        }
    }
}

fn known_setting_key(k: &str) -> bool {
    matches!(
        k,
        "branch_prefix"
            | "custom_instructions"
            | "nerd_fonts"
            | "editor_cmd"
            | "terminal_cmd"
            | "diff_cmd"
            | "notifications"
            | "theme"
            | "pm_enabled"
            | "pm_custom_instructions"
            | "mcp_mirror"
            | "remote_control"
            | "remote_control_sandbox"
            | "pinned_commands"
            | "remotes"
    )
}

pub fn parse_args(args: Vec<String>) -> Result<CliAction> {
    let mut it = args.into_iter().skip(1);
    match it.next().as_deref() {
        None => Ok(CliAction::Tui),
        Some("repo") => match it.next().as_deref() {
            Some("add") => {
                let path = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo add <path>".into()))?;
                let path = PathBuf::from(path);
                let mut name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let mut branch_prefix = String::new();
                while let Some(arg) = it.next() {
                    match arg.as_str() {
                        "--name" => {
                            name = it
                                .next()
                                .ok_or_else(|| Error::UserInput("--name needs value".into()))?
                        }
                        "--prefix" => {
                            branch_prefix = it
                                .next()
                                .ok_or_else(|| Error::UserInput("--prefix needs value".into()))?
                        }
                        other => return Err(Error::UserInput(format!("unknown arg: {other}"))),
                    }
                }
                Ok(CliAction::RepoAdd {
                    path,
                    name,
                    branch_prefix,
                })
            }
            Some("list") => Ok(CliAction::RepoList),
            Some("remove") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo remove <name>".into()))?;
                Ok(CliAction::RepoRemove { name })
            }
            Some("set-prefix") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo set-prefix <name> <prefix>".into()))?;
                let prefix = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo set-prefix <name> <prefix>".into()))?;
                Ok(CliAction::RepoSetPrefix { name, prefix })
            }
            Some("set-instructions") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-instructions <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-instructions <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetInstructions {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("set-setup") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-setup <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-setup <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetSetup {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("set-archive") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-archive <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-archive <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetArchive {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("edit-setup") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-setup <name>".into()))?;
                Ok(CliAction::RepoEditSetup { name })
            }
            Some("edit-archive") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-archive <name>".into()))?;
                Ok(CliAction::RepoEditArchive { name })
            }
            Some("set-pinned-commands") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-pinned-commands <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-pinned-commands <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetPinnedCommands {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("edit-pinned-commands") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-pinned-commands <name>".into()))?;
                Ok(CliAction::RepoEditPinnedCommands { name })
            }
            Some("set-related-repos") => {
                let name = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-related-repos <name> <value-or-@file>".into())
                })?;
                let value = it.next().ok_or_else(|| {
                    Error::UserInput("repo set-related-repos <name> <value-or-@file>".into())
                })?;
                Ok(CliAction::RepoSetRelatedRepos {
                    name,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("edit-related-repos") => {
                let name = it
                    .next()
                    .ok_or_else(|| Error::UserInput("repo edit-related-repos <name>".into()))?;
                Ok(CliAction::RepoEditRelatedRepos { name })
            }
            other => Err(Error::UserInput(format!("unknown repo action: {other:?}"))),
        },
        Some("config") => match it.next().as_deref() {
            Some("get") => {
                let key = it
                    .next()
                    .ok_or_else(|| Error::UserInput("config get <key>".into()))?;
                if !known_setting_key(&key) {
                    return Err(Error::UserInput(format!("unknown setting key: {key}")));
                }
                Ok(CliAction::ConfigGet { key })
            }
            Some("set") => {
                let key = it
                    .next()
                    .ok_or_else(|| Error::UserInput("config set <key> <value-or-@file>".into()))?;
                if !known_setting_key(&key) {
                    return Err(Error::UserInput(format!("unknown setting key: {key}")));
                }
                let value = it
                    .next()
                    .ok_or_else(|| Error::UserInput("config set <key> <value-or-@file>".into()))?;
                Ok(CliAction::ConfigSet {
                    key,
                    source: ValueSource::from_arg(value),
                })
            }
            Some("list") => Ok(CliAction::ConfigList),
            Some("edit") => {
                let key = it
                    .next()
                    .ok_or_else(|| Error::UserInput("config edit <key>".into()))?;
                if !known_setting_key(&key) {
                    return Err(Error::UserInput(format!("unknown setting key: {key}")));
                }
                Ok(CliAction::ConfigEdit { key })
            }
            other => Err(Error::UserInput(format!(
                "unknown config action: {other:?}"
            ))),
        },
        Some("remote") => match it.next() {
            None => Ok(CliAction::RemoteList),
            Some(name) => Ok(CliAction::RemoteRun { name }),
        },
        Some(other) => Err(Error::UserInput(format!("unknown command: {other}"))),
    }
}

pub async fn run_cli(action: CliAction, dirs: &Dirs) -> Result<()> {
    let store = crate::store::Store::open(&dirs.db_path())?;
    match action {
        CliAction::Tui => unreachable!("handled in main"),
        CliAction::RepoAdd {
            path,
            name,
            branch_prefix,
        } => {
            crate::repo::add(&store, &path, &name, &branch_prefix).await?;
            println!("added repo: {name}");
        }
        CliAction::RepoList => {
            for r in crate::repo::list(&store)? {
                println!("{:<20} {}", r.name, r.path.display());
            }
        }
        CliAction::RepoRemove { name } => {
            let repos = crate::repo::list(&store)?;
            let r = repos
                .into_iter()
                .find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            crate::repo::remove(&store, r.id)?;
            println!("removed repo: {name}");
        }
        CliAction::RepoSetPrefix { name, prefix } => {
            let repos = crate::repo::list(&store)?;
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
        CliAction::RepoSetInstructions { name, source } => {
            let repos = crate::repo::list(&store)?;
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
            let repos = crate::repo::list(&store)?;
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
            let repos = crate::repo::list(&store)?;
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
            let repos = crate::repo::list(&store)?;
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
            let repos = crate::repo::list(&store)?;
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
            let repos = crate::repo::list(&store)?;
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
            let repos = crate::repo::list(&store)?;
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
        CliAction::RepoSetRelatedRepos { name, source } => {
            let repos = crate::repo::list(&store)?;
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
            let repos = crate::repo::list(&store)?;
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
            let new_value = open_in_editor(&key, &current)?;
            let new_value = new_value.trim_end_matches('\n').to_string();
            if new_value.is_empty() {
                store.delete_setting(&key)?;
                println!("cleared {key}");
            } else if new_value == current {
                println!("{key} unchanged");
            } else {
                store.set_setting(&key, &new_value)?;
                println!("set {key} ({} chars)", new_value.len());
            }
        }
        CliAction::RemoteList => {
            let remotes = crate::remotes::list(&store)?;
            if remotes.is_empty() {
                println!("no remotes configured. add one with: wsx config edit remotes");
                return Ok(());
            }
            for r in remotes {
                println!("{}", r.name);
            }
        }
        CliAction::RemoteRun { name } => {
            let command = crate::remotes::lookup(&store, &name)?.ok_or_else(|| {
                let available = crate::remotes::list(&store)
                    .ok()
                    .map(|v| {
                        v.into_iter()
                            .map(|r| r.name)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default();
                if available.is_empty() {
                    Error::UserInput(format!(
                        "no remote named '{name}'. no remotes configured \
                         (add one with: wsx config edit remotes)"
                    ))
                } else {
                    Error::UserInput(format!(
                        "no remote named '{name}'. available: {available}"
                    ))
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
    }
    Ok(())
}

fn open_in_editor(key: &str, initial: &str) -> Result<String> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let dir = std::env::temp_dir();
    let path = dir.join(format!("wsx-{key}-{}.txt", std::process::id()));
    std::fs::write(&path, initial)?;
    let status = std::process::Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| Error::UserInput(format!("spawn editor {editor}: {e}")))?;
    if !status.success() {
        let _ = std::fs::remove_file(&path);
        return Err(Error::UserInput(format!(
            "editor {editor} exited with {status}"
        )));
    }
    let value = std::fs::read_to_string(&path)?;
    let _ = std::fs::remove_file(&path);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<CliAction> {
        let mut v = vec!["wsx".to_string()];
        v.extend(args.iter().map(|s| s.to_string()));
        parse_args(v)
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
    fn accepts_pm_enabled_and_pm_custom_instructions() {
        assert!(known_setting_key("pm_enabled"));
        assert!(known_setting_key("pm_custom_instructions"));
    }

    #[test]
    fn accepts_diff_cmd() {
        assert!(known_setting_key("diff_cmd"));
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
}
