use crate::config::Dirs;
use crate::error::{Error, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub enum CliAction {
    Tui,
    RepoAdd { path: PathBuf, name: String, branch_prefix: String },
    RepoList,
    RepoRemove { name: String },
}

pub fn parse_args(args: Vec<String>) -> Result<CliAction> {
    let mut it = args.into_iter().skip(1);
    match it.next().as_deref() {
        None => Ok(CliAction::Tui),
        Some("repo") => match it.next().as_deref() {
            Some("add") => {
                let path = it.next().ok_or_else(|| Error::UserInput("repo add <path>".into()))?;
                let path = PathBuf::from(path);
                let mut name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
                let mut branch_prefix = String::new();
                while let Some(arg) = it.next() {
                    match arg.as_str() {
                        "--name" => name = it.next().ok_or_else(|| Error::UserInput("--name needs value".into()))?,
                        "--prefix" => branch_prefix = it.next().ok_or_else(|| Error::UserInput("--prefix needs value".into()))?,
                        other => return Err(Error::UserInput(format!("unknown arg: {other}"))),
                    }
                }
                Ok(CliAction::RepoAdd { path, name, branch_prefix })
            }
            Some("list") => Ok(CliAction::RepoList),
            Some("remove") => {
                let name = it.next().ok_or_else(|| Error::UserInput("repo remove <name>".into()))?;
                Ok(CliAction::RepoRemove { name })
            }
            other => Err(Error::UserInput(format!("unknown repo action: {other:?}"))),
        },
        Some(other) => Err(Error::UserInput(format!("unknown command: {other}"))),
    }
}

pub async fn run_cli(action: CliAction, dirs: &Dirs) -> Result<()> {
    let store = crate::store::Store::open(&dirs.db_path())?;
    match action {
        CliAction::Tui => unreachable!("handled in main"),
        CliAction::RepoAdd { path, name, branch_prefix } => {
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
            let r = repos.into_iter().find(|r| r.name == name)
                .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))?;
            crate::repo::remove(&store, r.id)?;
            println!("removed repo: {name}");
        }
    }
    Ok(())
}
