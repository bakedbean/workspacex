//! `wsx repo` — registering repos and their per-repo settings.

use super::Args;
use crate::cli::action::{CliAction, ValueSource};
use crate::error::{Error, Result};
use std::path::PathBuf;

pub(in crate::cli) fn parse_repo(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("add") => {
            let path = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo add <path>".into(),
            })?;
            let path = PathBuf::from(path);
            let mut name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let mut branch_prefix = String::new();
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--name" => {
                        name = it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--name needs value".into(),
                        })?
                    }
                    "--prefix" => {
                        branch_prefix = it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--prefix needs value".into(),
                        })?
                    }
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
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
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo remove <name>".into(),
            })?;
            Ok(CliAction::RepoRemove { name })
        }
        Some("set-prefix") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-prefix <name> <prefix>".into(),
            })?;
            let prefix = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-prefix <name> <prefix>".into(),
            })?;
            Ok(CliAction::RepoSetPrefix { name, prefix })
        }
        Some("set-base-branch") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-base-branch <name> <ref-or-empty>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-base-branch <name> <ref-or-empty>".into(),
            })?;
            Ok(CliAction::RepoSetBaseBranch { name, value })
        }
        Some("set-instructions") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-instructions <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-instructions <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetInstructions {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("set-setup") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-setup <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-setup <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetSetup {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("set-archive") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-archive <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-archive <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetArchive {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("edit-setup") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-setup <name>".into(),
            })?;
            Ok(CliAction::RepoEditSetup { name })
        }
        Some("edit-archive") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-archive <name>".into(),
            })?;
            Ok(CliAction::RepoEditArchive { name })
        }
        Some("set-pinned-commands") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-pinned-commands <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-pinned-commands <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetPinnedCommands {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("edit-pinned-commands") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-pinned-commands <name>".into(),
            })?;
            Ok(CliAction::RepoEditPinnedCommands { name })
        }
        Some("set-name") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-name <name> <new-name>".into(),
            })?;
            let new_name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-name <name> <new-name>".into(),
            })?;
            Ok(CliAction::RepoSetName { name, new_name })
        }
        Some("set-related-repos") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-related-repos <name> <value-or-@file>".into(),
            })?;
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo set-related-repos <name> <value-or-@file>".into(),
            })?;
            Ok(CliAction::RepoSetRelatedRepos {
                name,
                source: ValueSource::from_arg(value),
            })
        }
        Some("edit-related-repos") => {
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "repo edit-related-repos <name>".into(),
            })?;
            Ok(CliAction::RepoEditRelatedRepos { name })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown repo command: {cmd}"),
                None => "missing repo command".into(),
            },
        }),
    }
}
