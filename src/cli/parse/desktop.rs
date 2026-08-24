//! `wsx setup`, `wsx waybar`, `wsx menubar`, and `wsx remote` — the
//! commands that install or drive things outside the TUI.

use super::Args;
use crate::cli::action::CliAction;
use crate::error::{Error, Result};

pub(in crate::cli) fn parse_remote(it: &mut Args) -> Result<CliAction> {
    match it.next() {
        None => Ok(CliAction::RemoteList),
        Some(name) => Ok(CliAction::RemoteRun { name }),
    }
}

pub(in crate::cli) fn parse_setup(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("install-skill") => Ok(CliAction::SetupInstallSkill),
        Some("waybar") => Ok(CliAction::SetupWaybar),
        Some("menubar") => Ok(CliAction::SetupMenubar),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown setup command: {cmd}"),
                None => "missing setup command".into(),
            },
        }),
    }
}

pub(in crate::cli) fn parse_waybar(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("status") => Ok(CliAction::WaybarStatus),
        Some("menu") => Ok(CliAction::WaybarMenu),
        Some("jump") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "jump needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::WaybarJump { repo, slug })
        }
        Some("menu-entries") => {
            // The installed elephant Lua invokes `menu-entries --json`; make
            // that contract explicit rather than relying on trailing args
            // being silently ignored. Any other trailing arg keeps today's
            // lenient behavior (not rejected).
            let _ = it.next().filter(|a| a == "--json");
            Ok(CliAction::WaybarMenuEntries)
        }
        Some("refresh-prs") => Ok(CliAction::WaybarRefreshPrs),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown waybar command: {cmd}"),
                None => "missing waybar command".into(),
            },
        }),
    }
}

pub(in crate::cli) fn parse_menubar(it: &mut Args) -> Result<CliAction> {
    match it.next().as_deref() {
        Some("plugin") => Ok(CliAction::MenubarPlugin),
        Some("jump") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "jump needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::MenubarJump { repo, slug })
        }
        Some("copy-path") => {
            let (Some(repo), Some(slug)) = (it.next(), it.next()) else {
                return Err(Error::Usage {
                    group: None,
                    msg: "copy-path needs <repo> <slug>".into(),
                });
            };
            Ok(CliAction::MenubarCopyPath { repo, slug })
        }
        Some("refresh") => Ok(CliAction::MenubarRefresh),
        other => Err(Error::Usage {
            group: None,
            msg: match other {
                Some(cmd) => format!("unknown menubar command: {cmd}"),
                None => "missing menubar command".into(),
            },
        }),
    }
}
