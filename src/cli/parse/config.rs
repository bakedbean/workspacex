//! `wsx config` — global settings.

use super::Args;
use crate::cli::action::{CliAction, ValueSource};
use crate::error::{Error, Result};

pub(in crate::cli) fn known_setting_key(k: &str) -> bool {
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
            | "chronox_cmd"
            | "notifications"
            | "theme"
            | "mcp_mirror"
            | "remote_control"
            | "remote_control_sandbox"
            | "pinned_commands"
            | "remotes"
            | "shared_hosts"
            | "dashboard_branch_width"
            | "dashboard_pr_width"
            | "dashboard_sort_mode"
            | "dashboard_blocked_pin_max_age_secs"
            | "coding_agent"
            | "detail_bar_config"
            | "usage_graph_window"
    )
}

pub(in crate::cli) fn parse_config(it: &mut Args) -> Result<CliAction> {
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
