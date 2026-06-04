use crate::config::Dirs;
use crate::error::{Error, Result};
use std::path::PathBuf;

pub struct CmdInfo {
    pub usage: &'static str,
    pub blurb: &'static str,
}

pub struct GroupInfo {
    pub name: &'static str,
    pub blurb: &'static str,
    pub commands: &'static [CmdInfo],
}

pub static GROUPS: &[GroupInfo] = &[
    GroupInfo {
        name: "workspace",
        blurb: "Create, list, rename, and archive workspaces",
        commands: &[
            CmdInfo {
                usage: "create <repo> [--name <slug>] [--yolo] [--agent <kind>]",
                blurb: "Create a workspace (branch + worktree)",
            },
            CmdInfo {
                usage: "list [<repo>]",
                blurb: "List workspaces as TSV rows",
            },
            CmdInfo {
                usage: "path <repo> <slug>",
                blurb: "Print a workspace's worktree path",
            },
            CmdInfo {
                usage: "rename <repo> <old> <new>",
                blurb: "Rename a workspace slug and its branch",
            },
            CmdInfo {
                usage: "archive <repo> <slug> [--keep-worktree] [--force-delete-branch]",
                blurb: "Archive a workspace",
            },
        ],
    },
    GroupInfo {
        name: "agent",
        blurb: "List, add, and message agents in a workspace",
        commands: &[
            CmdInfo {
                usage: "list",
                blurb: "Show agents in the current workspace",
            },
            CmdInfo {
                usage: "add <kind>",
                blurb: "Attach an agent (claude|pi|hermes|codex)",
            },
            CmdInfo {
                usage: "send <label> <message...>",
                blurb: "Queue an async message to a peer agent",
            },
        ],
    },
    GroupInfo {
        name: "repo",
        blurb: "Register and configure repositories",
        commands: &[
            CmdInfo {
                usage: "add <path> [--name <name>] [--prefix <prefix>]",
                blurb: "Register a repository",
            },
            CmdInfo {
                usage: "list",
                blurb: "List registered repositories",
            },
            CmdInfo {
                usage: "remove <name>",
                blurb: "Unregister a repository",
            },
            CmdInfo {
                usage: "set-prefix <name> <prefix>",
                blurb: "Set the branch prefix",
            },
            CmdInfo {
                usage: "set-base-branch <name> <ref-or-empty>",
                blurb: "Set the base branch",
            },
            CmdInfo {
                usage: "set-instructions <name> <value-or-@file>",
                blurb: "Set custom instructions",
            },
            CmdInfo {
                usage: "set-setup <name> <value-or-@file>",
                blurb: "Set the setup script",
            },
            CmdInfo {
                usage: "set-archive <name> <value-or-@file>",
                blurb: "Set the archive script",
            },
            CmdInfo {
                usage: "edit-setup <name>",
                blurb: "Edit the setup script in $EDITOR",
            },
            CmdInfo {
                usage: "edit-archive <name>",
                blurb: "Edit the archive script in $EDITOR",
            },
            CmdInfo {
                usage: "set-pinned-commands <name> <value-or-@file>",
                blurb: "Set pinned commands",
            },
            CmdInfo {
                usage: "edit-pinned-commands <name>",
                blurb: "Edit pinned commands in $EDITOR",
            },
            CmdInfo {
                usage: "set-name <name> <new-name>",
                blurb: "Rename a repository",
            },
            CmdInfo {
                usage: "set-related-repos <name> <value-or-@file>",
                blurb: "Set related repos",
            },
            CmdInfo {
                usage: "edit-related-repos <name>",
                blurb: "Edit related repos in $EDITOR",
            },
        ],
    },
    GroupInfo {
        name: "config",
        blurb: "Get and set global settings",
        commands: &[
            CmdInfo {
                usage: "get <key>",
                blurb: "Print a setting value",
            },
            CmdInfo {
                usage: "set <key> <value-or-@file>",
                blurb: "Set a setting",
            },
            CmdInfo {
                usage: "list",
                blurb: "List all settings",
            },
            CmdInfo {
                usage: "edit <key>",
                blurb: "Edit a setting in $EDITOR",
            },
        ],
    },
    GroupInfo {
        name: "remote",
        blurb: "Run saved remote shortcuts",
        commands: &[CmdInfo {
            usage: "[<name>]",
            blurb: "List remotes, or run the named remote shortcut",
        }],
    },
    GroupInfo {
        name: "setup",
        blurb: "One-off setup helpers",
        commands: &[CmdInfo {
            usage: "install-skill",
            blurb: "Install the wsx Claude Code skill",
        }],
    },
];

pub fn group_name(s: &str) -> Option<&'static str> {
    GROUPS.iter().map(|g| g.name).find(|&n| n == s)
}

/// The dashed help flags. Bare `help` is handled separately — only in a
/// subcommand position — because it is a legitimate argument value/name
/// elsewhere (e.g. a repo named `help`).
fn is_help_flag(tok: &str) -> bool {
    matches!(tok, "--help" | "-h")
}

fn is_version(tok: &str) -> bool {
    matches!(tok, "--version" | "-V")
}

pub fn render_root_help() -> String {
    let mut out = String::from("wsx — git-worktree workspace manager\n\n");
    out.push_str("USAGE:\n  wsx [COMMAND]            (no command launches the TUI)\n\n");
    out.push_str("COMMANDS:\n");
    let width = GROUPS.iter().map(|g| g.name.len()).max().unwrap_or(0);
    for g in GROUPS {
        out.push_str(&format!(
            "  {:<width$}   {}\n",
            g.name,
            g.blurb,
            width = width
        ));
    }
    out.push_str("\nRun `wsx <command> --help` for command details.\n");
    out
}

pub fn render_group_help(name: &str) -> String {
    let Some(g) = GROUPS.iter().find(|g| g.name == name) else {
        return render_root_help();
    };
    let mut out = format!("wsx {} — {}\n\n", g.name, g.blurb);
    out.push_str(&format!("USAGE:\n  wsx {} <command> [args]\n\n", g.name));
    out.push_str("COMMANDS:\n");
    let width = g.commands.iter().map(|c| c.usage.len()).max().unwrap_or(0);
    for c in g.commands {
        out.push_str(&format!(
            "  {:<width$}   {}\n",
            c.usage,
            c.blurb,
            width = width
        ));
    }
    out
}

type Args = dyn Iterator<Item = String>;

pub fn render_usage_error(group: Option<&str>, msg: &str) -> String {
    let block = match group {
        Some(g) => render_group_help(g),
        None => render_root_help(),
    };
    format!("error: {msg}\n\n{block}")
}

/// Formats a CLI error for stderr. Usage errors render the matching help
/// block; everything else falls back to a one-line message.
pub fn report_cli_error(e: &Error) -> String {
    match e {
        Error::Usage { group, msg } => render_usage_error(*group, msg),
        other => format!("error: {other}\n"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum HelpTopic {
    Root,
    Group(&'static str),
}

#[derive(Debug)]
pub enum CliAction {
    Tui,
    Help(HelpTopic),
    Version,
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
    RepoSetBaseBranch {
        name: String,
        value: String,
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
    RepoSetName {
        name: String,
        new_name: String,
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
    WorkspaceCreate {
        repo: String,
        name: Option<String>,
        yolo: bool,
        agent: Option<String>,
    },
    WorkspaceList {
        repo: Option<String>,
    },
    WorkspacePath {
        repo: String,
        name: String,
    },
    WorkspaceRename {
        repo: String,
        name: String,
        new_name: String,
    },
    WorkspaceArchive {
        repo: String,
        name: String,
        keep_worktree: bool,
        force_delete_branch: bool,
    },
    SetupInstallSkill,
    AgentList,
    AgentSend {
        target: String,
        prompt: String,
    },
    AgentAdd {
        kind: String,
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
            | "process_doctrine"
            | "nerd_fonts"
            | "editor_cmd"
            | "terminal_cmd"
            | "diff_cmd"
            | "lazygit_cmd"
            | "notifications"
            | "theme"
            | "pm_enabled"
            | "pm_custom_instructions"
            | "pm_fast_mode"
            | "mcp_mirror"
            | "remote_control"
            | "remote_control_sandbox"
            | "pinned_commands"
            | "remotes"
            | "dashboard_name_width"
            | "dashboard_branch_width"
            | "coding_agent"
            | "detail_bar_config"
    )
}

pub fn parse_args(args: Vec<String>) -> Result<CliAction> {
    let mut rest: Vec<String> = args.into_iter().skip(1).collect();
    let first = if rest.is_empty() {
        None
    } else {
        Some(rest.remove(0))
    };

    match first.as_deref() {
        None => return Ok(CliAction::Tui),
        // Match the literal `help` subcommand before the is_help() flag guard,
        // so `wsx help <group>` resolves the group instead of collapsing to Root.
        Some("help") => {
            let topic = match rest.first().and_then(|s| group_name(s)) {
                Some(g) => HelpTopic::Group(g),
                None => HelpTopic::Root,
            };
            return Ok(CliAction::Help(topic));
        }
        Some(t) if is_help_flag(t) => return Ok(CliAction::Help(HelpTopic::Root)),
        Some(t) if is_version(t) => return Ok(CliAction::Version),
        _ => {}
    }

    let group = first.as_deref().expect("None handled above");

    // Per-group help. `--help`/`-h` are flag-style and may appear anywhere
    // (`wsx agent send --help`); bare `help` is only a help request in the
    // subcommand slot (`wsx agent help`), since it is a valid value/name
    // elsewhere (e.g. `wsx repo remove help` removes a repo named `help`).
    let wants_group_help =
        rest.iter().any(|a| is_help_flag(a)) || rest.first().map(|a| a.as_str()) == Some("help");
    if wants_group_help {
        if let Some(g) = group_name(group) {
            return Ok(CliAction::Help(HelpTopic::Group(g)));
        }
    }

    let mut it = rest.into_iter();
    match group {
        "repo" => parse_repo(&mut it).map_err(|e| tag_group(e, group)),
        "config" => parse_config(&mut it).map_err(|e| tag_group(e, group)),
        "remote" => parse_remote(&mut it).map_err(|e| tag_group(e, group)),
        "workspace" => parse_workspace(&mut it).map_err(|e| tag_group(e, group)),
        "agent" => parse_agent(&mut it).map_err(|e| tag_group(e, group)),
        "setup" => parse_setup(&mut it).map_err(|e| tag_group(e, group)),
        other => Err(Error::Usage {
            group: None,
            msg: format!("unknown command: {other}"),
        }),
    }
}

fn tag_group(e: Error, group: &str) -> Error {
    match e {
        Error::Usage { group: None, msg } => Error::Usage {
            group: group_name(group),
            msg,
        },
        other => other,
    }
}

fn parse_repo(it: &mut Args) -> Result<CliAction> {
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

fn parse_config(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("get") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config get <key>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            Ok(CliAction::ConfigGet { key })
        }
        Some("set") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config set <key> <value-or-@file>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            let value = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config set <key> <value-or-@file>".into(),
            })?;
            Ok(CliAction::ConfigSet {
                key,
                source: ValueSource::from_arg(value),
            })
        }
        Some("list") => Ok(CliAction::ConfigList),
        Some("edit") => {
            let key = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "config edit <key>".into(),
            })?;
            if !known_setting_key(&key) {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("unknown setting key: {key}"),
                });
            }
            Ok(CliAction::ConfigEdit { key })
        }
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown config command: {cmd}"),
                None => "missing config command".into(),
            },
        }),
    }
}

fn parse_remote(it: &mut Args) -> Result<CliAction> {
    match it.next() {
        None => Ok(CliAction::RemoteList),
        Some(name) => Ok(CliAction::RemoteRun { name }),
    }
}

fn parse_workspace(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("create") => {
            let repo = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg:
                    "workspace create <repo> [--name <slug>] [--yolo] [--agent claude|pi|hermes|codex]"
                        .into(),
            })?;
            let mut name: Option<String> = None;
            let mut yolo = false;
            let mut agent: Option<String> = None;
            while let Some(arg) = it.next() {
                match arg.as_str() {
                    "--name" => {
                        name = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--name needs value".into(),
                        })?);
                    }
                    "--yolo" => yolo = true,
                    "--agent" => {
                        agent = Some(it.next().ok_or_else(|| Error::Usage {
                            group: None,
                            msg: "--agent needs value (claude, pi, hermes, or codex)".into(),
                        })?);
                    }
                    other => {
                        return Err(Error::Usage {
                            group: None,
                            msg: format!("unknown arg: {other}"),
                        });
                    }
                }
            }
            if let Some(ref a) = agent
                && a != "pi"
                && a != "claude"
                && a != "hermes"
                && a != "codex"
            {
                return Err(Error::Usage {
                    group: None,
                    msg: format!("--agent must be 'claude', 'pi', 'hermes', or 'codex', got '{a}'"),
                });
            }
            Ok(CliAction::WorkspaceCreate {
                repo,
                name,
                yolo,
                agent,
            })
        }
        Some("list") => {
            let repo = it.next();
            Ok(CliAction::WorkspaceList { repo })
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
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown workspace command: {cmd}"),
                None => "missing workspace command".into(),
            },
        }),
    }
}

fn parse_agent(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("list") => Ok(CliAction::AgentList),
        Some("send") => {
            let target = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "agent send <label> <prompt>".into(),
            })?;
            let rest: Vec<String> = it.collect();
            if rest.is_empty() {
                return Err(Error::Usage {
                    group: None,
                    msg: "agent send <label> <prompt>".into(),
                });
            }
            let prompt = rest.join(" ");
            Ok(CliAction::AgentSend { target, prompt })
        }
        Some("add") => {
            let kind = it.next().ok_or_else(|| Error::Usage {
                group: None,
                msg: "agent add <kind>".into(),
            })?;
            // Validate against the canonical agent set so this can't drift
            // from `AgentKind` as kinds are added/renamed.
            use crate::pty::session::AgentKind;
            if !AgentKind::ALL.iter().any(|k| k.display_name() == kind) {
                let valid = AgentKind::ALL
                    .iter()
                    .map(|k| k.display_name())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::Usage {
                    group: None,
                    msg: format!("agent add: kind must be one of [{valid}], got '{kind}'"),
                });
            }
            Ok(CliAction::AgentAdd { kind })
        }
        _ => Err(Error::Usage {
            group: None,
            msg: "agent <list|send|add> ...".into(),
        }),
    }
}

fn parse_setup(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("install-skill") => Ok(CliAction::SetupInstallSkill),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown setup command: {cmd}"),
                None => "missing setup command".into(),
            },
        }),
    }
}

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
            let outcome = crate::agent::skill::install_to(&target.path)?;
            let path = target.path.display();
            match outcome {
                crate::agent::skill::InstallOutcome::Created => {
                    println!("installed wsx skill for {} to {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Updated => {
                    println!("updated wsx skill for {} at {path}", target.agent);
                }
                crate::agent::skill::InstallOutcome::Unchanged => {
                    println!(
                        "wsx skill for {} already up to date at {path}",
                        target.agent
                    );
                }
            }
        }
        return Ok(());
    }
    let store = crate::data::store::Store::open(&dirs.db_path())?;
    match action {
        CliAction::Tui => unreachable!("handled in main"),
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
                } else {
                    new_value.clone()
                };
                store.set_setting(&key, &normalized)?;
                println!("set {key} ({} chars)", normalized.len());
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
        CliAction::WorkspaceCreate {
            repo,
            name,
            yolo,
            agent,
        } => {
            let r = lookup_repo(&store, &repo)?;
            let worktree_base = dirs.app_dir().join("worktrees");
            std::fs::create_dir_all(&worktree_base)?;
            let agent_kind = crate::pty::session::AgentKind::from_str_or_default(agent.as_deref());
            let created = crate::data::workspace::create(
                &store,
                &r,
                name.as_deref(),
                &worktree_base,
                yolo,
                agent_kind,
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
            if let crate::data::setup::SetupResult::Failed { exit_code } = created.setup_result {
                println!("warning: setup script exited with code {exit_code}");
            }
        }
        CliAction::WorkspaceList { repo } => {
            let filtered = match repo {
                Some(name) => vec![lookup_repo(&store, &name)?],
                None => crate::data::repo::list(&store)?,
            };
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
                crate::data::workspace::rename(&store, &r, &w, &new_name).await?;
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
        CliAction::AgentList => {
            let ws = resolve_current_workspace(&store)?;
            for inst in store.workspace_agents(ws.id)? {
                let tag = if inst.is_primary { "  (primary)" } else { "" };
                println!("{}  {}{}", inst.id.0, inst.label(), tag);
            }
        }
        CliAction::AgentSend { target, prompt } => {
            let ws = resolve_current_workspace(&store)?;
            let target_id = store
                .resolve_instance_label(ws.id, &target)?
                .ok_or_else(|| {
                    Error::UserInput(format!(
                        "no agent '{target}' in this workspace; try `wsx agent list`"
                    ))
                })?;
            let from = std::env::var("WSX_AGENT_INSTANCE_ID")
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .map(crate::data::store::AgentInstanceId);
            store.enqueue_message(ws.id, target_id, from, &prompt)?;
            println!("queued message to {target}");
        }
        CliAction::AgentAdd { kind } => {
            let ws = resolve_current_workspace(&store)?;
            let agent = crate::pty::session::AgentKind::from_str_or_default(Some(&kind));
            let inst = store.add_workspace_agent(ws.id, agent)?;
            println!("added {}", inst.label());
        }
        CliAction::SetupInstallSkill => unreachable!("handled before store open"),
        CliAction::Help(_) | CliAction::Version => {
            unreachable!("handled before store open")
        }
    }
    Ok(())
}

/// Resolve the workspace the current `wsx` invocation is acting within:
/// prefer the `WSX_WORKSPACE_ID` env var (set when wsx spawns an agent), else
/// fall back to matching the current directory against known worktree paths.
fn resolve_current_workspace(
    store: &crate::data::store::Store,
) -> Result<crate::data::store::Workspace> {
    use crate::data::store::WorkspaceId;
    // 1. WSX_WORKSPACE_ID (reliable for agent-initiated calls)
    if let Ok(s) = std::env::var("WSX_WORKSPACE_ID") {
        if let Ok(id) = s.parse::<i64>() {
            if let Some(ws) = store.workspace_by_id(WorkspaceId(id))? {
                return Ok(ws);
            }
        }
    }
    // 2. cwd: find the workspace whose worktree_path is an ancestor-or-equal of cwd
    // Note: this is a raw path-prefix match. If the user `cd`'d into the
    // worktree through a symlink (e.g. macOS /var -> /private/var), cwd may not
    // prefix the stored worktree_path and the match will miss. Setting
    // WSX_WORKSPACE_ID (the agent-spawn path) avoids this entirely.
    let cwd = std::env::current_dir()
        .map_err(|e| Error::UserInput(format!("cannot determine current directory: {e}")))?;
    let ws = store
        .all_workspaces()?
        .into_iter()
        .filter(|w| cwd.starts_with(&w.worktree_path))
        .max_by_key(|w| w.worktree_path.as_os_str().len())
        .ok_or_else(|| {
            Error::UserInput(
                "not inside a wsx workspace (set WSX_WORKSPACE_ID or run from a worktree)".into(),
            )
        })?;
    Ok(ws)
}

fn lookup_repo(store: &crate::data::store::Store, name: &str) -> Result<crate::data::store::Repo> {
    crate::data::repo::list(store)?
        .into_iter()
        .find(|r| r.name == name)
        .ok_or_else(|| Error::UserInput(format!("no repo named {name}")))
}

fn lookup_workspace(
    store: &crate::data::store::Store,
    repo: &crate::data::store::Repo,
    name: &str,
) -> Result<crate::data::store::Workspace> {
    store
        .workspaces(repo.id)?
        .into_iter()
        .find(|w| w.name == name)
        .ok_or_else(|| Error::UserInput(format!("no workspace named {name} in repo {}", repo.name)))
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

/// Seed text for the editor when the global `detail_bar_config`
/// setting is empty — the pretty-printed default config.
fn detail_bar_config_seed_for_empty() -> String {
    serde_json::to_string_pretty(&crate::config::detail_bar_config::DetailBarConfig::default())
        .unwrap_or_else(|_| "{}".to_string())
}

/// Parse, sanitize, and re-serialize a global `detail_bar_config`
/// blob. Returns the pretty-printed normalized JSON.
fn detail_bar_config_validate_and_normalize(raw: &str) -> Result<String> {
    let mut cfg: crate::config::detail_bar_config::DetailBarConfig = serde_json::from_str(raw)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: invalid JSON: {e}")))?;
    cfg.sanitize();
    serde_json::to_string_pretty(&cfg)
        .map_err(|e| Error::UserInput(format!("detail_bar_config: serialize failed: {e}")))
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
        assert!(matches!(parse(&[]).unwrap(), CliAction::Tui));
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
            CliAction::AgentSend { target, prompt } => {
                assert_eq!(target, "claude");
                assert_eq!(prompt, "help");
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
        assert!(h.contains("send <label> <message...>"));
    }

    #[test]
    fn usage_error_has_message_then_group_block() {
        let s = render_usage_error(Some("agent"), "missing arguments");
        assert!(s.starts_with("error: missing arguments"));
        assert!(s.contains("send <label> <message...>"));
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
    fn accepts_pm_enabled_and_pm_custom_instructions() {
        assert!(known_setting_key("pm_enabled"));
        assert!(known_setting_key("pm_custom_instructions"));
    }

    #[test]
    fn accepts_pm_fast_mode() {
        assert!(known_setting_key("pm_fast_mode"));
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
                agent: None,
            } => {
                assert_eq!(repo, "backend");
                assert!(name.is_none());
                assert!(!yolo);
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
                agent: None,
            } => {
                assert_eq!(repo, "backend");
                assert_eq!(name.as_deref(), Some("add-widgets"));
                assert!(yolo);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_workspace_create_rejects_unknown_arg() {
        assert!(parse(&["workspace", "create", "backend", "--bogus"]).is_err());
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
        let seed = super::detail_bar_config_seed_for_empty();
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
        let result = super::detail_bar_config_validate_and_normalize("{not json");
        assert!(result.is_err());
    }

    #[test]
    fn detail_bar_config_validate_clamps_out_of_range() {
        let json = r#"{"height": {"percent": 200}}"#;
        let normalized = super::detail_bar_config_validate_and_normalize(json).unwrap();
        let parsed: crate::config::detail_bar_config::DetailBarConfig =
            serde_json::from_str(&normalized).unwrap();
        assert_eq!(parsed.height.percent, 80);
    }

    #[test]
    fn detail_bar_config_validate_accepts_partial() {
        let json = r#"{"visible": false}"#;
        let normalized = super::detail_bar_config_validate_and_normalize(json).unwrap();
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
    }

    #[test]
    fn parses_agent_send_joins_prompt() {
        match parse(&["agent", "send", "claude#2", "hello", "there"]).unwrap() {
            CliAction::AgentSend { target, prompt } => {
                assert_eq!(target, "claude#2");
                assert_eq!(prompt, "hello there");
            }
            other => panic!("expected AgentSend, got {other:?}"),
        }
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
        let normalized = super::detail_bar_config_validate_and_normalize(&raw)
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
        assert!(s.contains("send <label> <message...>"));
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
        let dispatched = ["workspace", "agent", "repo", "config", "remote", "setup"];
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
}
