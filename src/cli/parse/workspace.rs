//! `wsx workspace` and `wsx shared` — the workspace lifecycle, and
//! sharing a workspace's tmux session with another machine.

use super::Args;
use crate::cli::action::CliAction;
use crate::error::{Error, Result};

pub(in crate::cli) fn parse_shared(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("list") => {
            let mut json = false;
            let mut all = false;
            for arg in &mut *it {
                match arg.as_str() {
                    "--json" => json = true,
                    "--all" => all = true,
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            Ok(CliAction::SharedList { json, all })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown shared command: {cmd}"),
                None => "missing shared command".into(),
            },
        }),
    }
}

pub(in crate::cli) fn parse_workspace(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("create") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg:
                    "workspace create <repo> [--name <slug>] [--yolo] [--shared] [--agent claude|pi|hermes|codex|omp] [--prompt <text>]"
                        .into(),
            })?;
            let mut name: Option<String> = None;
            let mut yolo = false;
            let mut shared = false;
            let mut agent: Option<String> = None;
            let mut prompt: Option<String> = None;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--name" => {
                        name = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--name needs value".into(),
                        })?);
                    }
                    "--prompt" => {
                        prompt = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--prompt needs value (the text to seed the agent with)".into(),
                        })?);
                    }
                    "--yolo" => yolo = true,
                    "--shared" => shared = true,
                    "--agent" => {
                        agent =
                            Some(
                                it.next().ok_or_else(|| Error::Usage {
                                    group: None,
                                    msg: "--agent needs value (claude, pi, hermes, codex, or omp)"
                                        .into(),
                                })?,
                            );
                    }
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            // Validate against the canonical agent set so this can't drift from
            // `AgentKind` as kinds are added or renamed — the same reason
            // `agent add` validates this way. The hand-maintained chain this
            // replaces would have rejected `omp` on the day it was added.
            if let Some(ref a) = agent
                && !crate::pty::session::AgentKind::ALL
                    .iter()
                    .any(|k| k.display_name() == a)
            {
                let valid = crate::pty::session::AgentKind::ALL
                    .iter()
                    .map(|k| k.display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::Usage {
                    group: None,
                    msg: format!("--agent must be one of [{valid}], got '{a}'"),
                });
            }
            Ok(CliAction::WorkspaceCreate {
                repo,
                name,
                yolo,
                shared,
                agent,
                prompt,
            })
        }
        Some("list") => {
            let mut repo = None;
            let mut json = false;
            for arg in &mut *it {
                match arg.as_str() {
                    "--json" => json = true,
                    _ if repo.is_none() => repo = Some(arg),
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!(
                                "unexpected argument: {other} (usage: workspace list [<repo>] [--json])"
                            ),
                        });
                    }
                }
            }
            Ok(CliAction::WorkspaceList { repo, json })
        }
        Some("path") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace path <repo> <name>".into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace path <repo> <name>".into(),
            })?;
            Ok(CliAction::WorkspacePath { repo, name })
        }
        Some("rename") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace rename <repo> <name> <new-name>".into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace rename <repo> <name> <new-name>".into(),
            })?;
            let new_name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace rename <repo> <name> <new-name>".into(),
            })?;
            Ok(CliAction::WorkspaceRename {
                repo,
                name,
                new_name,
            })
        }
        Some("archive") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace archive <repo> <name> [--keep-worktree] [--force-delete-branch]"
                    .into(),
            })?;
            let name = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "workspace archive <repo> <name> [--keep-worktree] [--force-delete-branch]"
                    .into(),
            })?;
            let mut keep_worktree = false;
            let mut force_delete_branch = false;
            for arg in &mut *it {
                match arg.as_str() {
                    "--keep-worktree" => keep_worktree = true,
                    "--force-delete-branch" => force_delete_branch = true,
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            Ok(CliAction::WorkspaceArchive {
                repo,
                name,
                keep_worktree,
                force_delete_branch,
            })
        }
        Some(sub @ ("share" | "unshare")) => {
            let shared = sub == "share";
            let usage = if shared {
                "workspace share <repo> <name> [--restart [--json]]"
            } else {
                "workspace unshare <repo> <name>"
            };
            let usage_err = || Error::Usage {
                group: None,
                msg: usage.into(),
            };
            let repo = it.next().ok_or_else(usage_err)?;
            let name = it.next().ok_or_else(usage_err)?;
            let mut restart = false;
            let mut json = false;
            for arg in &mut *it {
                match arg.as_str() {
                    "--restart" if shared => restart = true,
                    "--json" if shared => json = true,
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            if json && !restart {
                return Err(Error::Usage {
                    group: None,
                    msg: "--json needs --restart".into(),
                });
            }
            Ok(CliAction::WorkspaceShare {
                repo,
                name,
                shared,
                restart,
                json,
            })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown workspace command: {cmd}"),
                None => "missing workspace command".into(),
            },
        }),
    }
}
