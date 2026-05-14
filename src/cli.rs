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
            | "notifications"
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
}
